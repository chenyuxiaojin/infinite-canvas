use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    sync::Arc,
    thread::{self, JoinHandle},
};

use axum::{
    body::Body,
    extract::{rejection::JsonRejection, DefaultBodyLimit, Path, State},
    http::{HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::oneshot;

use crate::{
    capabilities, AgentOperationRequest, AgentRuntime, BridgeError, CanvasOperationAdapter,
    CredentialStore, ImageIngestRequest, ProjectCreateRequest, TestClipRequest, VideoIngestRequest,
};

pub const BRIDGE_PORT: u16 = 3102;

#[derive(Clone)]
struct BridgeState {
    credentials: Arc<CredentialStore>,
    canvas: Arc<dyn CanvasOperationAdapter>,
    runtime: Arc<dyn AgentRuntime>,
}

#[derive(Serialize)]
struct Success<T: Serialize> {
    ok: bool,
    data: T,
}

impl<T: Serialize> Success<T> {
    fn new(data: T) -> Self {
        Self { ok: true, data }
    }
}

pub struct BridgeServer {
    address: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl BridgeServer {
    pub fn start(
        address: SocketAddr,
        credentials: Arc<CredentialStore>,
        canvas: Arc<dyn CanvasOperationAdapter>,
        runtime: Arc<dyn AgentRuntime>,
    ) -> Result<Self, BridgeError> {
        if address.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
            return Err(BridgeError::forbidden(
                "The Agent Bridge may only bind to 127.0.0.1.",
            ));
        }
        let listener = TcpListener::bind(address).map_err(|_| {
            BridgeError::unavailable("The Agent Bridge loopback port is unavailable.")
        })?;
        listener.set_nonblocking(true).map_err(|_| {
            BridgeError::internal("The Agent Bridge listener could not be configured.")
        })?;
        let address = listener
            .local_addr()
            .map_err(|_| BridgeError::internal("The Agent Bridge address is unavailable."))?;
        let state = BridgeState {
            credentials,
            canvas,
            runtime,
        };
        let router = router(state.clone());
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let worker = thread::Builder::new()
            .name("infinite-canvas-agent-bridge".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                let Ok(runtime) = runtime else {
                    return;
                };
                runtime.block_on(async move {
                    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
                        return;
                    };
                    let _ = axum::serve(listener, router)
                        .with_graceful_shutdown(async {
                            let _ = shutdown_rx.await;
                        })
                        .await;
                });
            })
            .map_err(|_| BridgeError::internal("The Agent Bridge worker could not be started."))?;
        Ok(Self {
            address,
            shutdown: Some(shutdown_tx),
            worker: Some(worker),
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn stop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for BridgeServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn router(state: BridgeState) -> Router {
    Router::new()
        .route("/v1/capabilities", get(get_capabilities))
        .route("/v1/projects", get(list_projects).post(create_project))
        .route("/v1/projects/:project_id", get(get_project))
        .route("/v1/canvas/operations/dry-run", post(dry_run_operations))
        .route("/v1/canvas/operations/apply", post(apply_operations))
        .route("/v1/runtime", get(runtime_report))
        .route("/v1/media/inbox", get(media_inbox))
        .route("/v1/media/video-ingests", post(submit_video_ingest))
        .route("/v1/media/image-ingests", post(submit_image_ingest))
        .route("/v1/tasks/test-clips", post(submit_test_clip))
        .route("/v1/tasks/:task_id", get(task_status))
        .route("/v1/tasks/:task_id/cancel", post(cancel_task))
        .route("/v1/credentials/revoke", post(revoke_credential))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), authenticate))
        .with_state(state)
}

async fn authenticate(
    State(state): State<BridgeState>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, BridgeError> {
    let token = request
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .unwrap_or_default();
    if !state.credentials.authenticate(token) {
        return Err(BridgeError::unauthorized());
    }
    Ok(next.run(request).await)
}

async fn get_capabilities() -> Json<Success<Value>> {
    Json(Success::new(capabilities::catalog()))
}

async fn list_projects(
    State(state): State<BridgeState>,
) -> Result<Json<Success<Value>>, BridgeError> {
    Ok(Json(Success::new(json!(state.canvas.list_projects()?))))
}

async fn create_project(
    State(state): State<BridgeState>,
    payload: Result<Json<ProjectCreateRequest>, JsonRejection>,
) -> Result<Json<Success<Value>>, BridgeError> {
    let Json(request) = structured_json(payload)?;
    Ok(Json(Success::new(json!(state
        .canvas
        .create_project(request)?))))
}

async fn get_project(
    State(state): State<BridgeState>,
    Path(project_id): Path<String>,
) -> Result<Json<Success<Value>>, BridgeError> {
    Ok(Json(Success::new(json!(state
        .canvas
        .get_project(&project_id)?))))
}

async fn dry_run_operations(
    State(state): State<BridgeState>,
    payload: Result<Json<AgentOperationRequest>, JsonRejection>,
) -> Result<Json<Success<Value>>, BridgeError> {
    let Json(request) = structured_json(payload)?;
    Ok(Json(Success::new(json!(state
        .canvas
        .apply_operations(request, true)?))))
}

async fn apply_operations(
    State(state): State<BridgeState>,
    payload: Result<Json<AgentOperationRequest>, JsonRejection>,
) -> Result<Json<Success<Value>>, BridgeError> {
    let Json(request) = structured_json(payload)?;
    Ok(Json(Success::new(json!(state
        .canvas
        .apply_operations(request, false)?))))
}

async fn runtime_report(
    State(state): State<BridgeState>,
) -> Result<Json<Success<Value>>, BridgeError> {
    Ok(Json(Success::new(state.runtime.report()?)))
}

async fn media_inbox(
    State(state): State<BridgeState>,
) -> Result<Json<Success<Value>>, BridgeError> {
    Ok(Json(Success::new(state.runtime.media_inbox()?)))
}

async fn submit_video_ingest(
    State(state): State<BridgeState>,
    payload: Result<Json<VideoIngestRequest>, JsonRejection>,
) -> Result<Json<Success<Value>>, BridgeError> {
    let Json(request) = structured_json(payload)?;
    state.runtime.validate_video_ingest(&request)?;
    let canvas_task_id = format!("agent-media-{}", request.request_id);
    let timestamp = now_rfc3339()?;
    let started = state.canvas.apply_protocol_batch(
        &request.project_id,
        json!({
            "protocolVersion": 1,
            "actor": "agent",
            "requestId": request.request_id,
            "projectId": request.project_id,
            "baseRevision": request.base_revision,
            "timestamp": timestamp,
            "operations": [
                {
                    "type": "node.create",
                    "node": {
                        "id": request.node_id,
                        "type": "video",
                        "title": request.title,
                        "position": request.position,
                        "width": request.size.width,
                        "height": request.size.height,
                        "metadata": {
                            "status": "loading",
                            "progress": 5,
                            "generationMode": "video",
                            "prompt": request.title,
                            "localTaskKind": "agent_video_ingest",
                            "localCanvasTaskId": canvas_task_id,
                            "agentMediaInboxFile": request.inbox_file_name,
                            "expectedSha256": request.expected_sha256
                        }
                    }
                },
                {
                    "type": "task.start",
                    "task": {
                        "id": canvas_task_id,
                        "nodeId": request.node_id,
                        "kind": "agent_video_ingest",
                        "status": "queued",
                        "requestId": request.request_id,
                        "details": {
                            "paid": false,
                            "source": "agent_bridge",
                            "expectedSha256": request.expected_sha256
                        }
                    }
                }
            ]
        }),
        false,
    )?;

    if started.duplicate {
        if let Some(runtime_task_id) = started
            .project
            .pointer(&format!(
                "/operationState/tasks/{}/details/runtimeTaskId",
                json_pointer_token(&canvas_task_id)
            ))
            .and_then(Value::as_str)
        {
            let current_revision = started
                .project
                .pointer("/operationState/revision")
                .and_then(Value::as_u64)
                .unwrap_or(started.revision);
            return Ok(Json(Success::new(json!({
                "task_id": runtime_task_id,
                "duplicate": true,
                "mode": "allowlisted_mp4_ingest",
                "paid": false,
                "canvas_task_id": canvas_task_id,
                "canvas_revision": current_revision
            }))));
        }
    }

    let runtime_result = match state.runtime.submit_video_ingest(&request) {
        Ok(result) => result,
        Err(error) => {
            let _ = update_canvas_ingest_task(
                &state.canvas,
                CanvasIngestTaskUpdate {
                    project_id: &request.project_id,
                    node_id: &request.node_id,
                    canvas_task_id: &canvas_task_id,
                    status: "failed",
                    details: json!({ "runtimeError": { "code": error.code, "message": error.message } }),
                    metadata: json!({
                        "status": "error",
                        "errorDetails": error.message,
                    }),
                    include_task_update: true,
                },
            );
            return Err(error);
        }
    };
    let runtime_task_id = runtime_result["task_id"]
        .as_str()
        .ok_or_else(|| BridgeError::internal("The desktop runtime task id is missing."))?;
    let updated = update_canvas_ingest_task(
        &state.canvas,
        CanvasIngestTaskUpdate {
            project_id: &request.project_id,
            node_id: &request.node_id,
            canvas_task_id: &canvas_task_id,
            status: "running",
            details: json!({
                "runtimeTaskId": runtime_task_id,
                "runtime": runtime_result,
            }),
            metadata: json!({
                "localTaskId": runtime_task_id,
                "localTaskKind": "agent_video_ingest",
                "localCanvasTaskId": canvas_task_id,
                "status": "loading",
                "progress": 15
            }),
            include_task_update: true,
        },
    )?;
    let mut response = runtime_result;
    response["canvas_task_id"] = Value::String(canvas_task_id);
    response["canvas_revision"] = json!(updated.revision.max(started.revision));
    Ok(Json(Success::new(response)))
}

async fn submit_image_ingest(
    State(state): State<BridgeState>,
    payload: Result<Json<ImageIngestRequest>, JsonRejection>,
) -> Result<Json<Success<Value>>, BridgeError> {
    let Json(request) = structured_json(payload)?;
    let ingested = state.runtime.ingest_image(&request)?;
    let reference = ingested
        .get("reference")
        .filter(|value| value.is_object())
        .cloned()
        .ok_or_else(|| BridgeError::internal("The ingested image reference is missing."))?;
    let storage_key = reference["storageKey"]
        .as_str()
        .filter(|value| value.starts_with("local-ref:asset-"))
        .ok_or_else(|| BridgeError::internal("The ingested image storage key is invalid."))?
        .to_owned();
    let width = reference["width"]
        .as_u64()
        .filter(|value| *value > 0)
        .ok_or_else(|| BridgeError::internal("The ingested image width is invalid."))?;
    let height = reference["height"]
        .as_u64()
        .filter(|value| *value > 0)
        .ok_or_else(|| BridgeError::internal("The ingested image height is invalid."))?;
    let timestamp = now_rfc3339()?;
    let applied = state.canvas.apply_protocol_batch(
        &request.project_id,
        json!({
            "protocolVersion": 1,
            "actor": "agent",
            "requestId": request.request_id,
            "projectId": request.project_id,
            "baseRevision": request.base_revision,
            "timestamp": timestamp,
            "operations": [{
                "type": "node.create",
                "node": {
                    "id": request.node_id,
                    "type": "image",
                    "title": request.title,
                    "position": request.position,
                    "width": request.size.width,
                    "height": request.size.height,
                    "metadata": {
                        "content": storage_key,
                        "storageKey": storage_key,
                        "localMedia": reference,
                        "status": "success",
                        "naturalWidth": width,
                        "naturalHeight": height,
                        "bytes": reference["bytes"],
                        "mimeType": reference["mimeType"]
                    }
                }
            }]
        }),
        false,
    )?;
    Ok(Json(Success::new(json!({
        "mode": "allowlisted_image_ingest",
        "paid": false,
        "duplicate": applied.duplicate,
        "node_id": request.node_id,
        "canvas_revision": applied.revision,
        "reference": reference
    }))))
}

async fn submit_test_clip(
    State(state): State<BridgeState>,
    payload: Result<Json<TestClipRequest>, JsonRejection>,
) -> Result<Json<Success<Value>>, BridgeError> {
    let Json(request) = structured_json(payload)?;
    let canvas_task_id = format!("agent-task-{}", request.request_id);
    let timestamp = now_rfc3339()?;
    let started = state.canvas.apply_protocol_batch(
        &request.project_id,
        json!({
            "protocolVersion": 1,
            "actor": "agent",
            "requestId": request.request_id,
            "projectId": request.project_id,
            "baseRevision": request.base_revision,
            "timestamp": timestamp,
            "operations": [{
                "type": "task.start",
                "task": {
                    "id": canvas_task_id,
                    "nodeId": request.node_id,
                    "kind": "deterministic_test_clip",
                    "status": "queued",
                    "requestId": request.request_id,
                    "details": { "paid": false, "source": "agent_bridge" }
                }
            }]
        }),
        false,
    )?;

    let runtime_result = match state.runtime.submit_test_clip(&request) {
        Ok(result) => result,
        Err(error) => {
            let _ = update_canvas_task(
                &state.canvas,
                &request.project_id,
                &canvas_task_id,
                "failed",
                json!({ "runtimeError": { "code": error.code, "message": error.message } }),
            );
            return Err(error);
        }
    };
    let runtime_task_id = runtime_result["task_id"]
        .as_str()
        .ok_or_else(|| BridgeError::internal("The desktop runtime task id is missing."))?;
    let updated = update_canvas_task(
        &state.canvas,
        &request.project_id,
        &canvas_task_id,
        "running",
        json!({
            "runtimeTaskId": runtime_task_id,
            "runtime": runtime_result,
        }),
    )?;
    let mut response = runtime_result;
    response["canvas_task_id"] = Value::String(canvas_task_id);
    response["canvas_revision"] = json!(updated.revision.max(started.revision));
    Ok(Json(Success::new(response)))
}

async fn task_status(
    State(state): State<BridgeState>,
    Path(task_id): Path<String>,
) -> Result<Json<Success<Value>>, BridgeError> {
    let mut snapshot = state.runtime.task_status(&task_id)?;
    if let Some(local_media) = snapshot
        .get_mut("local_media")
        .and_then(Value::as_object_mut)
    {
        local_media.remove("playbackUrl");
        local_media.remove("playback_url");
    }
    sync_runtime_task_snapshot(&state.canvas, &task_id, &snapshot)?;
    Ok(Json(Success::new(snapshot)))
}

async fn cancel_task(
    State(state): State<BridgeState>,
    Path(task_id): Path<String>,
) -> Result<Json<Success<Value>>, BridgeError> {
    if let Some(task) = state.canvas.find_runtime_task(&task_id)? {
        if !matches!(
            task.status.as_str(),
            "cancel_requested" | "cancelled" | "succeeded" | "failed"
        ) {
            let timestamp = now_rfc3339()?;
            state.canvas.apply_protocol_batch(
                &task.project_id,
                json!({
                    "protocolVersion": 1,
                    "actor": "agent",
                    "requestId": format!("agent-task-cancel-{}-{}", task.canvas_task_id, task.revision),
                    "projectId": task.project_id,
                    "baseRevision": task.revision,
                    "timestamp": timestamp,
                    "operations": [{
                        "type": "task.cancel",
                        "taskId": task.canvas_task_id,
                        "reason": "Agent Bridge cancellation request"
                    }]
                }),
                false,
            )?;
        }
    }
    Ok(Json(Success::new(state.runtime.cancel_task(&task_id)?)))
}

fn sync_runtime_task_snapshot(
    canvas: &Arc<dyn CanvasOperationAdapter>,
    runtime_task_id: &str,
    snapshot: &Value,
) -> Result<(), BridgeError> {
    let Some(task) = canvas.find_runtime_task(runtime_task_id)? else {
        return Ok(());
    };
    let Some(status) = snapshot["status"].as_str() else {
        return Err(BridgeError::internal("The desktop task status is invalid."));
    };
    if status == "queued" {
        return Ok(());
    }
    if !matches!(status, "running" | "cancelled" | "succeeded" | "failed") {
        return Err(BridgeError::internal(
            "The desktop task status is unsupported.",
        ));
    }
    let status_changed = status != task.status;
    if task.kind == "agent_video_ingest" {
        let should_repair_legacy_node = !status_changed
            && matches!(status, "cancelled" | "succeeded" | "failed")
            && ingest_node_is_still_loading(canvas, &task, runtime_task_id)?;
        if !status_changed && !should_repair_legacy_node {
            return Ok(());
        }
        let metadata =
            ingest_node_metadata(runtime_task_id, &task.canvas_task_id, status, snapshot)?;
        update_canvas_ingest_task(
            canvas,
            CanvasIngestTaskUpdate {
                project_id: &task.project_id,
                node_id: &task.node_id,
                canvas_task_id: &task.canvas_task_id,
                status,
                details: json!({ "runtimeTaskId": runtime_task_id, "runtimeSnapshot": snapshot }),
                metadata,
                include_task_update: status_changed,
            },
        )?;
        return Ok(());
    }
    if !status_changed {
        return Ok(());
    }
    update_canvas_task(
        canvas,
        &task.project_id,
        &task.canvas_task_id,
        status,
        json!({ "runtimeTaskId": runtime_task_id, "runtimeSnapshot": snapshot }),
    )?;
    Ok(())
}

fn ingest_node_is_still_loading(
    canvas: &Arc<dyn CanvasOperationAdapter>,
    task: &crate::CanvasRuntimeTaskReference,
    runtime_task_id: &str,
) -> Result<bool, BridgeError> {
    let project = canvas.get_project(&task.project_id)?.project;
    let node = project["nodes"]
        .as_array()
        .and_then(|nodes| {
            nodes
                .iter()
                .find(|node| node["id"].as_str() == Some(task.node_id.as_str()))
        })
        .ok_or_else(|| BridgeError::internal("The canvas media task node is missing."))?;
    Ok(
        node["metadata"]["localTaskId"].as_str() == Some(runtime_task_id)
            && node["metadata"]["status"].as_str() == Some("loading"),
    )
}

fn ingest_node_metadata(
    runtime_task_id: &str,
    canvas_task_id: &str,
    status: &str,
    snapshot: &Value,
) -> Result<Value, BridgeError> {
    let common = json!({
        "localTaskId": runtime_task_id,
        "localTaskKind": "agent_video_ingest",
        "localCanvasTaskId": canvas_task_id,
    });
    let mut metadata = common
        .as_object()
        .cloned()
        .ok_or_else(|| BridgeError::internal("The canvas media metadata is invalid."))?;
    match status {
        "succeeded" => {
            let result = snapshot
                .get("result")
                .filter(|result| result["type"].as_str() == Some("media_created"))
                .ok_or_else(|| {
                    BridgeError::internal("The completed desktop media result is missing.")
                })?;
            let sha256 = result["sha256"]
                .as_str()
                .filter(|value| value.len() == 64)
                .ok_or_else(|| {
                    BridgeError::internal("The completed desktop media digest is invalid.")
                })?;
            let probe = result
                .get("probe")
                .ok_or_else(|| BridgeError::internal("The desktop media probe is missing."))?;
            let video = probe["streams"]
                .as_array()
                .and_then(|streams| {
                    streams
                        .iter()
                        .find(|stream| stream["codec_type"].as_str() == Some("video"))
                })
                .ok_or_else(|| {
                    BridgeError::internal("The desktop media video stream is missing.")
                })?;
            let width = video["width"]
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or_else(|| BridgeError::internal("The desktop media width is invalid."))?;
            let height = video["height"]
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or_else(|| BridgeError::internal("The desktop media height is invalid."))?;
            let duration_ms = probe["duration_ms"]
                .as_u64()
                .filter(|value| *value > 0)
                .ok_or_else(|| BridgeError::internal("The desktop media duration is invalid."))?;
            let local_media = snapshot
                .get("local_media")
                .and_then(|value| value.get("reference"))
                .filter(|value| value.is_object())
                .ok_or_else(|| {
                    BridgeError::internal("The completed local media reference is missing.")
                })?;
            let storage_key = local_media["storageKey"]
                .as_str()
                .filter(|value| value.starts_with("local-ref:asset-"))
                .ok_or_else(|| {
                    BridgeError::internal("The completed local media storage key is invalid.")
                })?;
            metadata.extend([
                ("content".to_owned(), Value::String(storage_key.to_owned())),
                (
                    "storageKey".to_owned(),
                    Value::String(storage_key.to_owned()),
                ),
                ("localMedia".to_owned(), local_media.clone()),
                ("status".to_owned(), Value::String("success".to_owned())),
                ("progress".to_owned(), json!(100)),
                ("naturalWidth".to_owned(), json!(width)),
                ("naturalHeight".to_owned(), json!(height)),
                ("durationMs".to_owned(), json!(duration_ms)),
                (
                    "bytes".to_owned(),
                    local_media.get("bytes").cloned().unwrap_or(Value::Null),
                ),
                (
                    "localTaskSha256".to_owned(),
                    Value::String(sha256.to_owned()),
                ),
                ("mimeType".to_owned(), Value::String("video/mp4".to_owned())),
                ("errorDetails".to_owned(), Value::Null),
            ]);
        }
        "failed" | "cancelled" => {
            let message = snapshot
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or(if status == "cancelled" {
                    "The local media task was cancelled."
                } else {
                    "The local media task failed."
                });
            metadata.extend([
                ("status".to_owned(), Value::String("error".to_owned())),
                ("errorDetails".to_owned(), Value::String(message.to_owned())),
            ]);
        }
        "running" => {
            metadata.extend([
                ("status".to_owned(), Value::String("loading".to_owned())),
                ("progress".to_owned(), json!(60)),
            ]);
        }
        _ => {
            return Err(BridgeError::internal(
                "The media task status is unsupported.",
            ))
        }
    }
    Ok(Value::Object(metadata))
}

fn update_canvas_task(
    canvas: &Arc<dyn CanvasOperationAdapter>,
    project_id: &str,
    canvas_task_id: &str,
    status: &str,
    details: Value,
) -> Result<crate::ProtocolOutcome, BridgeError> {
    let mut last_error = None;
    for attempt in 0..3 {
        let project = canvas.get_project(project_id)?;
        let current_status = project
            .project
            .pointer(&format!(
                "/operationState/tasks/{}/status",
                json_pointer_token(canvas_task_id)
            ))
            .and_then(Value::as_str);
        if current_status == Some(status) {
            return Ok(crate::ProtocolOutcome {
                project: project.project,
                ok: true,
                duplicate: true,
                previous_revision: project.revision,
                revision: project.revision,
                error_code: None,
                error_message: None,
                error: None,
            });
        }
        let timestamp = now_rfc3339()?;
        let result = canvas.apply_protocol_batch(
            project_id,
            json!({
                "protocolVersion": 1,
                "actor": "system",
                "requestId": format!("system-task-{}-{}-{}-{}", canvas_task_id, status, project.revision, attempt),
                "projectId": project_id,
                "baseRevision": project.revision,
                "timestamp": timestamp,
                "operations": [{
                    "type": "task.update",
                    "taskId": canvas_task_id,
                    "status": status,
                    "details": details
                }]
            }),
            false,
        );
        match result {
            Ok(outcome) => return Ok(outcome),
            Err(error) if error.code == "STALE_REVISION" => last_error = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        BridgeError::conflict(
            "STALE_REVISION",
            "The canvas kept changing while the task status was committed.",
        )
    }))
}

struct CanvasIngestTaskUpdate<'a> {
    project_id: &'a str,
    node_id: &'a str,
    canvas_task_id: &'a str,
    status: &'a str,
    details: Value,
    metadata: Value,
    include_task_update: bool,
}

fn update_canvas_ingest_task(
    canvas: &Arc<dyn CanvasOperationAdapter>,
    update: CanvasIngestTaskUpdate<'_>,
) -> Result<crate::ProtocolOutcome, BridgeError> {
    let CanvasIngestTaskUpdate {
        project_id,
        node_id,
        canvas_task_id,
        status,
        details,
        metadata,
        include_task_update,
    } = update;
    let mut last_error = None;
    for attempt in 0..3 {
        let project = canvas.get_project(project_id)?;
        let timestamp = now_rfc3339()?;
        let current_task_status = project
            .project
            .pointer(&format!(
                "/operationState/tasks/{}/status",
                json_pointer_token(canvas_task_id)
            ))
            .and_then(Value::as_str);
        let node_metadata = project.project["nodes"]
            .as_array()
            .and_then(|nodes| {
                nodes
                    .iter()
                    .find(|node| node["id"].as_str() == Some(node_id))
            })
            .and_then(|node| node["metadata"].as_object())
            .ok_or_else(|| BridgeError::internal("The canvas media task node is missing."))?;
        let metadata_is_current = metadata.as_object().is_some_and(|patch| {
            patch
                .iter()
                .all(|(key, value)| node_metadata.get(key).unwrap_or(&Value::Null) == value)
        });
        let mut operations = Vec::new();
        if include_task_update && current_task_status != Some(status) {
            operations.insert(
                0,
                json!({
                    "type": "task.update",
                    "taskId": canvas_task_id,
                    "status": status,
                    "details": details
                }),
            );
        }
        if !metadata_is_current {
            operations.push(json!({
                "type": "node.update",
                "nodeId": node_id,
                "patch": { "metadata": metadata }
            }));
        }
        if operations.is_empty() {
            return Ok(crate::ProtocolOutcome {
                project: project.project,
                ok: true,
                duplicate: true,
                previous_revision: project.revision,
                revision: project.revision,
                error_code: None,
                error_message: None,
                error: None,
            });
        }
        let result = canvas.apply_protocol_batch(
            project_id,
            json!({
                "protocolVersion": 1,
                "actor": "system",
                "requestId": format!("system-media-{}-{}-{}-{}", canvas_task_id, status, project.revision, attempt),
                "projectId": project_id,
                "baseRevision": project.revision,
                "timestamp": timestamp,
                "operations": operations
            }),
            false,
        );
        match result {
            Ok(outcome) => return Ok(outcome),
            Err(error) if error.code == "STALE_REVISION" => last_error = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        BridgeError::conflict(
            "STALE_REVISION",
            "The canvas kept changing while the media task was committed.",
        )
    }))
}

fn json_pointer_token(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn now_rfc3339() -> Result<String, BridgeError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|_| BridgeError::internal("The Agent Bridge clock is unavailable."))
}

async fn revoke_credential(
    State(state): State<BridgeState>,
) -> Result<Json<Success<Value>>, BridgeError> {
    let credential_id = state.credentials.revoke_and_replace()?;
    Ok(Json(Success::new(json!({
        "revoked": true,
        "replacement_credential_id": credential_id
    }))))
}

async fn not_found(_headers: HeaderMap) -> Response {
    BridgeError::new(
        "CAPABILITY_NOT_FOUND",
        StatusCode::NOT_FOUND,
        "The requested Agent Bridge capability is not allowlisted.",
    )
    .into_response()
}

async fn method_not_allowed() -> Response {
    BridgeError::new(
        "METHOD_NOT_ALLOWED",
        StatusCode::METHOD_NOT_ALLOWED,
        "The HTTP method is not allowlisted for this Agent Bridge capability.",
    )
    .into_response()
}

fn structured_json<T>(payload: Result<Json<T>, JsonRejection>) -> Result<Json<T>, BridgeError> {
    payload.map_err(|_| {
        BridgeError::invalid("The JSON request does not match the allowlisted capability schema.")
    })
}
