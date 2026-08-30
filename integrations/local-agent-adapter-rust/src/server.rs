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
use tokio::sync::oneshot;

use crate::{
    capabilities, AgentOperationRequest, AgentRuntime, BridgeError, CanvasOperationAdapter,
    CredentialStore, TestClipRequest,
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
        .route("/v1/projects", get(list_projects))
        .route("/v1/projects/:project_id", get(get_project))
        .route("/v1/canvas/operations/dry-run", post(dry_run_operations))
        .route("/v1/canvas/operations/apply", post(apply_operations))
        .route("/v1/runtime", get(runtime_report))
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

async fn submit_test_clip(
    State(state): State<BridgeState>,
    payload: Result<Json<TestClipRequest>, JsonRejection>,
) -> Result<Json<Success<Value>>, BridgeError> {
    let Json(request) = structured_json(payload)?;
    let project = state.canvas.get_project(&request.project_id)?;
    if project.revision != request.base_revision {
        return Err(BridgeError::conflict(
            "REVISION_CONFLICT",
            "The canvas changed after the Agent read it; reload before submitting the task.",
        )
        .with_details(json!({ "current_revision": project.revision })));
    }
    Ok(Json(Success::new(
        state.runtime.submit_test_clip(&request)?,
    )))
}

async fn task_status(
    State(state): State<BridgeState>,
    Path(task_id): Path<String>,
) -> Result<Json<Success<Value>>, BridgeError> {
    Ok(Json(Success::new(state.runtime.task_status(&task_id)?)))
}

async fn cancel_task(
    State(state): State<BridgeState>,
    Path(task_id): Path<String>,
) -> Result<Json<Success<Value>>, BridgeError> {
    Ok(Json(Success::new(state.runtime.cancel_task(&task_id)?)))
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
