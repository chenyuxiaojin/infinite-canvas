use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    os::unix::fs::PermissionsExt,
    process::Command,
    sync::Arc,
};

use assert_cmd::cargo::cargo_bin;
use local_agent_adapter::{
    read_credential_token, Actor, AgentOperationRequest, AgentRuntime, BridgeClient, BridgeError,
    BridgeServer, CanonicalCanvasAdapter, CanvasOperation, CanvasOperationAdapter,
    CanvasProtocolExecutor, CanvasSize, CredentialDocument, CredentialStore, ImageIngestRequest,
    Point, ProjectCreateRequest, ProtocolOutcome, TestClipRequest, VideoGenerationRequest,
    VideoIngestRequest,
};
use rusqlite::Connection;
use serde_json::{json, Value};

struct MockRuntime;

struct FixtureProtocol;

impl CanvasProtocolExecutor for FixtureProtocol {
    fn apply(
        &self,
        mut project: Value,
        batch: Value,
        now: &str,
    ) -> Result<ProtocolOutcome, BridgeError> {
        let previous_revision = project["operationState"]["revision"].as_u64().unwrap_or(0);
        let request_id = batch["requestId"].as_str().unwrap_or_default();
        let duplicate = project["operationState"]["requests"]
            .as_object()
            .is_some_and(|requests| requests.contains_key(request_id));
        if duplicate {
            return Ok(ProtocolOutcome {
                project,
                ok: true,
                duplicate: true,
                previous_revision,
                revision: previous_revision,
                error_code: None,
                error_message: None,
                error: None,
            });
        }
        let base_revision = batch["baseRevision"].as_u64().unwrap_or(u64::MAX);
        if base_revision != previous_revision {
            return Ok(ProtocolOutcome {
                project,
                ok: false,
                duplicate: false,
                previous_revision,
                revision: previous_revision,
                error_code: Some("stale_revision".to_owned()),
                error_message: Some("stale fixture revision".to_owned()),
                error: Some(json!({
                    "code": "stale_revision",
                    "currentRevision": previous_revision
                })),
            });
        }
        for operation in batch["operations"].as_array().unwrap() {
            match operation["type"].as_str().unwrap_or_default() {
                "project.update" => project["title"] = operation["title"].clone(),
                "node.create" => project["nodes"]
                    .as_array_mut()
                    .unwrap()
                    .push(operation["node"].clone()),
                "node.update" => {
                    let node_id = operation["nodeId"].as_str().unwrap();
                    let node = project["nodes"]
                        .as_array_mut()
                        .unwrap()
                        .iter_mut()
                        .find(|node| node["id"].as_str() == Some(node_id))
                        .unwrap();
                    let patch = operation["patch"].as_object().unwrap();
                    for (key, value) in patch {
                        if key == "metadata" {
                            let metadata = node["metadata"].as_object_mut().unwrap();
                            for (metadata_key, metadata_value) in value.as_object().unwrap() {
                                metadata.insert(metadata_key.clone(), metadata_value.clone());
                            }
                        } else {
                            node[key] = value.clone();
                        }
                    }
                }
                "task.start" => {
                    let mut task = operation["task"].clone();
                    task["createdAt"] = Value::String(now.to_owned());
                    task["updatedAt"] = Value::String(now.to_owned());
                    let task_id = task["id"].as_str().unwrap().to_owned();
                    project["operationState"]["tasks"][task_id] = task;
                }
                "task.update" => {
                    let task_id = operation["taskId"].as_str().unwrap();
                    let task = &mut project["operationState"]["tasks"][task_id];
                    task["status"] = operation["status"].clone();
                    task["updatedAt"] = Value::String(now.to_owned());
                    let details = task["details"].as_object_mut().unwrap();
                    for (key, value) in operation["details"].as_object().unwrap() {
                        details.insert(key.clone(), value.clone());
                    }
                }
                _ => {}
            }
        }
        let revision = previous_revision + 1;
        project["updatedAt"] = Value::String(now.to_owned());
        project["operationState"]["revision"] = json!(revision);
        project["operationState"]["requests"][request_id] = json!({
            "fingerprint": "fixture",
            "result": { "ok": true, "duplicate": false, "previousRevision": previous_revision, "revision": revision, "items": [] }
        });
        Ok(ProtocolOutcome {
            project,
            ok: true,
            duplicate: false,
            previous_revision,
            revision,
            error_code: None,
            error_message: None,
            error: None,
        })
    }
}

impl AgentRuntime for MockRuntime {
    fn report(&self) -> Result<Value, BridgeError> {
        Ok(json!({ "transport": "mock", "paid": false }))
    }

    fn media_inbox(&self) -> Result<Value, BridgeError> {
        Ok(json!({
            "kind": "fixed_app_support_inbox",
            "inbox_id": "agent-media-inbox",
            "request_field": "inbox_file_name",
            "accepted_mime_types": ["video/mp4", "image/png", "image/jpeg", "image/webp"],
            "arbitrary_paths": false,
            "absolute_path_exposed": false
        }))
    }

    fn validate_video_ingest(&self, _request: &VideoIngestRequest) -> Result<(), BridgeError> {
        Ok(())
    }

    fn ingest_image(&self, request: &ImageIngestRequest) -> Result<Value, BridgeError> {
        Ok(json!({
            "mode": "allowlisted_image_ingest",
            "paid": false,
            "reference": {
                "assetId": "asset-fedcba9876543210fedcba9876543210",
                "storageKey": "local-ref:asset-fedcba9876543210fedcba9876543210",
                "rootId": "agent-media",
                "relativePath": format!("verified/agent-image-{}.png", &request.expected_sha256[..32]),
                "sha256": request.expected_sha256,
                "mimeType": "image/png",
                "bytes": 67,
                "fileName": request.inbox_file_name,
                "width": 1024,
                "height": 576,
                "mode": "project_copy"
            }
        }))
    }

    fn submit_video_ingest(&self, request: &VideoIngestRequest) -> Result<Value, BridgeError> {
        Ok(json!({
            "task_id": format!("media-{}", request.request_id),
            "duplicate": false,
            "mode": "allowlisted_mp4_ingest",
            "paid": false
        }))
    }

    fn quote_video_generation(
        &self,
        resolution: &str,
        duration_seconds: u64,
    ) -> Result<Value, BridgeError> {
        let unit = if resolution == "2K" { 0.36 } else { 0.09 };
        Ok(json!({
            "configured": true,
            "model": "MiniMax-H3",
            "estimated_cost_yuan": unit * duration_seconds as f64,
            "unit_price_yuan_per_second": unit,
            "currency": "CNY"
        }))
    }

    fn submit_test_clip(&self, request: &TestClipRequest) -> Result<Value, BridgeError> {
        Ok(json!({
            "task_id": format!("task-{}", request.request_id),
            "duplicate": false,
            "mode": "deterministic_local_fixture",
            "paid": false
        }))
    }

    fn task_status(&self, task_id: &str) -> Result<Value, BridgeError> {
        if task_id.starts_with("media-") {
            return Ok(json!({
                "id": task_id,
                "status": "succeeded",
                "action": "transcode_to_mp4",
                "result": {
                    "type": "media_created",
                    "output": { "root": "agent-media", "relative": "verified/shot-001.mp4" },
                    "sha256": "c".repeat(64),
                    "probe": {
                        "duration_ms": 6583,
                        "streams": [
                            { "index": 0, "codec_type": "video", "codec_name": "h264", "width": 1344, "height": 768 },
                            { "index": 1, "codec_type": "audio", "codec_name": "aac", "sample_rate": 48000, "channels": 2 }
                        ]
                    }
                },
                "local_media": {
                    "status": "available",
                    "playbackUrl": "http://127.0.0.1:3213/v1/media/asset?token=must-not-leak",
                    "reference": {
                        "assetId": "asset-0123456789abcdef0123456789abcdef",
                        "storageKey": "local-ref:asset-0123456789abcdef0123456789abcdef",
                        "rootId": "agent-media",
                        "relativePath": "verified/shot-001.mp4",
                        "sha256": "c".repeat(64),
                        "mimeType": "video/mp4",
                        "bytes": 18432,
                        "fileName": "shot-001.mp4",
                        "width": 1344,
                        "height": 768,
                        "durationMs": 6583,
                        "mode": "project_copy"
                    }
                }
            }));
        }
        Ok(json!({ "id": task_id, "status": "succeeded" }))
    }

    fn cancel_task(&self, task_id: &str) -> Result<Value, BridgeError> {
        Ok(json!({ "task_id": task_id, "cancelled": true }))
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    server: BridgeServer,
    credentials: Arc<CredentialStore>,
    canvas: Arc<CanonicalCanvasAdapter>,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("infinite-canvas.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE canvas_projects (
                    user_id TEXT NOT NULL,
                    id TEXT NOT NULL,
                    project_data TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    deleted_at TEXT NOT NULL DEFAULT '',
                    PRIMARY KEY (user_id, id)
                );",
            )
            .unwrap();
        drop(connection);
        let canvas =
            Arc::new(CanonicalCanvasAdapter::open(&database, Arc::new(FixtureProtocol)).unwrap());
        canvas
            .save_human_project(json!({
                "id": "project-1",
                "title": "Shared canvas",
                "createdAt": "2026-01-01T00:00:00Z",
                "updatedAt": "2026-01-01T00:00:00Z",
                "nodes": [],
                "connections": [],
                "operationState": {
                    "version": 1,
                    "revision": 0,
                    "locks": {},
                    "tasks": {},
                    "requests": {},
                    "audit": []
                }
            }))
            .unwrap();
        let credentials =
            Arc::new(CredentialStore::load_or_create(root.path().join("agent-bridge")).unwrap());
        let server = BridgeServer::start(
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
            credentials.clone(),
            canvas.clone(),
            Arc::new(MockRuntime),
        )
        .unwrap();
        Self {
            _root: root,
            server,
            credentials,
            canvas,
        }
    }

    fn endpoint(&self) -> String {
        format!("http://{}", self.server.address())
    }

    fn client(&self) -> BridgeClient {
        BridgeClient::new(
            &self.endpoint(),
            read_credential_token(&self.credentials.path()).unwrap(),
        )
        .unwrap()
    }
}

#[test]
fn bridge_refuses_any_non_loopback_bind() {
    let root = tempfile::tempdir().unwrap();
    let database = root.path().join("canvas.db");
    Connection::open(&database)
        .unwrap()
        .execute_batch(
            "CREATE TABLE canvas_projects (
                user_id TEXT, id TEXT, project_data TEXT, created_at TEXT,
                updated_at TEXT, deleted_at TEXT, PRIMARY KEY (user_id, id)
            );",
        )
        .unwrap();
    let credentials = Arc::new(CredentialStore::load_or_create(root.path().join("auth")).unwrap());
    let canvas =
        Arc::new(CanonicalCanvasAdapter::open(database, Arc::new(FixtureProtocol)).unwrap());
    let result = BridgeServer::start(
        "0.0.0.0:0".parse().unwrap(),
        credentials,
        canvas,
        Arc::new(MockRuntime),
    );
    let Err(error) = result else {
        panic!("a non-loopback bind must be rejected");
    };
    assert_eq!(error.code, "CAPABILITY_DENIED");
}

#[test]
fn missing_credential_and_non_allowlisted_routes_are_structured_rejections() {
    let fixture = Fixture::new();
    let unauthorized = ureq::get(&format!("{}/v1/capabilities", fixture.endpoint()))
        .call()
        .unwrap_err();
    let ureq::Error::Status(401, response) = unauthorized else {
        panic!("expected 401");
    };
    let body: Value = response.into_json().unwrap();
    assert_eq!(body["error"]["code"], "UNAUTHORIZED");

    let denied = fixture.client().get("/v1/shell").unwrap_err();
    assert_eq!(denied.code, "CAPABILITY_NOT_FOUND");
    assert_eq!(denied.status, 404);

    let malformed = ureq::post(&format!(
        "{}/v1/canvas/operations/apply",
        fixture.endpoint()
    ))
    .set(
        "Authorization",
        &format!(
            "Bearer {}",
            read_credential_token(&fixture.credentials.path()).unwrap()
        ),
    )
    .set("Content-Type", "application/json")
    .send_string("{\"project_id\":\"project-1\",\"path\":\"../../escape\"}")
    .unwrap_err();
    let ureq::Error::Status(400, response) = malformed else {
        panic!("expected a structured 400 rejection");
    };
    let body: Value = response.into_json().unwrap();
    assert_eq!(body["error"]["code"], "INVALID_REQUEST");
}

#[test]
fn credential_revocation_invalidates_the_old_bearer_immediately() {
    let fixture = Fixture::new();
    let old_token = read_credential_token(&fixture.credentials.path()).unwrap();
    let old_client = BridgeClient::new(&fixture.endpoint(), old_token).unwrap();
    let response = old_client
        .post("/v1/credentials/revoke", &json!({}))
        .unwrap();
    assert_eq!(response["data"]["revoked"], true);
    assert!(response["data"].get("secret").is_none());
    assert_eq!(
        old_client.get("/v1/capabilities").unwrap_err().code,
        "UNAUTHORIZED"
    );
    assert!(fixture.client().get("/v1/capabilities").is_ok());
}

#[test]
fn apply_is_revision_guarded_and_idempotent() {
    let fixture = Fixture::new();
    let document = fixture.canvas.get_project("project-1").unwrap();
    let request = AgentOperationRequest {
        project_id: "project-1".to_owned(),
        request_id: "request-1".to_owned(),
        base_revision: document.revision,
        actor: Actor::Agent,
        operations: vec![CanvasOperation::SetProjectTitle {
            title: "Agent draft".to_owned(),
        }],
    };
    let first = fixture
        .canvas
        .apply_operations(request.clone(), false)
        .unwrap();
    let duplicate = fixture.canvas.apply_operations(request, false).unwrap();
    assert!(!first.duplicate);
    assert!(duplicate.duplicate);
    assert_eq!(first.revision, duplicate.revision);
    assert_eq!(duplicate.project["title"], "Agent draft");

    let stale = AgentOperationRequest {
        project_id: "project-1".to_owned(),
        request_id: "request-2".to_owned(),
        base_revision: 0,
        actor: Actor::Agent,
        operations: vec![CanvasOperation::SetProjectTitle {
            title: "Stale".to_owned(),
        }],
    };
    assert_eq!(
        fixture
            .canvas
            .apply_operations(stale, false)
            .unwrap_err()
            .code,
        "STALE_REVISION"
    );
}

#[test]
fn project_creation_is_protocol_backed_and_idempotent() {
    let fixture = Fixture::new();
    let request = ProjectCreateRequest {
        project_id: "agent-review-wall".to_owned(),
        request_id: "create-review-wall".to_owned(),
        actor: Actor::Agent,
        title: "Agent review wall".to_owned(),
    };
    let first = fixture.canvas.create_project(request.clone()).unwrap();
    fixture
        .canvas
        .apply_operations(
            AgentOperationRequest {
                project_id: "agent-review-wall".to_owned(),
                request_id: "rename-review-wall".to_owned(),
                base_revision: 1,
                actor: Actor::Agent,
                operations: vec![CanvasOperation::SetProjectTitle {
                    title: "Agent review wall updated".to_owned(),
                }],
            },
            false,
        )
        .unwrap();
    let duplicate = fixture.canvas.create_project(request).unwrap();
    assert!(!first.duplicate);
    assert!(duplicate.duplicate);
    assert_eq!(first.revision, 1);
    assert_eq!(duplicate.revision, 2);
    assert_eq!(duplicate.project["title"], "Agent review wall updated");

    let reused_id = fixture
        .canvas
        .create_project(ProjectCreateRequest {
            project_id: "agent-review-wall".to_owned(),
            request_id: "different-request".to_owned(),
            actor: Actor::Agent,
            title: "Must not replace".to_owned(),
        })
        .unwrap_err();
    assert_eq!(reused_id.code, "PROJECT_EXISTS");
}

#[test]
fn allowlisted_video_ingest_creates_one_shared_video_node_and_task() {
    let fixture = Fixture::new();
    let request = VideoIngestRequest {
        project_id: "project-1".to_owned(),
        node_id: "video-1".to_owned(),
        request_id: "ingest-1".to_owned(),
        base_revision: 0,
        actor: Actor::Agent,
        inbox_file_name: "shot-001.mp4".to_owned(),
        expected_sha256: "a".repeat(64),
        title: "Shot 001".to_owned(),
        position: Point { x: 10.0, y: 20.0 },
        size: CanvasSize {
            width: 320.0,
            height: 180.0,
        },
    };
    let first = fixture
        .client()
        .post("/v1/media/video-ingests", &request)
        .unwrap();
    assert_eq!(first["data"]["task_id"], "media-ingest-1");
    assert_eq!(first["data"]["canvas_revision"], 2);

    let duplicate = fixture
        .client()
        .post("/v1/media/video-ingests", &request)
        .unwrap();
    assert_eq!(duplicate["data"]["duplicate"], true);
    assert_eq!(duplicate["data"]["canvas_revision"], 2);
    let project = fixture.canvas.get_project("project-1").unwrap().project;
    assert_eq!(project["nodes"].as_array().unwrap().len(), 1);
    assert_eq!(project["nodes"][0]["type"], "video");
    assert_eq!(
        project["nodes"][0]["metadata"]["localTaskId"],
        "media-ingest-1"
    );
    assert_eq!(
        project["operationState"]["tasks"]["agent-media-ingest-1"]["details"]["runtimeTaskId"],
        "media-ingest-1"
    );

    let completed = fixture.client().get("/v1/tasks/media-ingest-1").unwrap();
    assert_eq!(completed["data"]["status"], "succeeded");
    let completed_project = fixture.canvas.get_project("project-1").unwrap();
    assert_eq!(completed_project.revision, 3);
    assert_eq!(
        completed_project.project["operationState"]["tasks"]["agent-media-ingest-1"]["status"],
        "succeeded"
    );
    let metadata = &completed_project.project["nodes"][0]["metadata"];
    assert_eq!(
        metadata["content"],
        "local-ref:asset-0123456789abcdef0123456789abcdef"
    );
    assert_eq!(
        metadata["storageKey"],
        "local-ref:asset-0123456789abcdef0123456789abcdef"
    );
    assert_eq!(metadata["localMedia"]["rootId"], "agent-media");
    assert_eq!(
        metadata["localMedia"]["relativePath"],
        "verified/shot-001.mp4"
    );
    assert!(completed["data"]["local_media"]
        .get("playbackUrl")
        .is_none());
    assert_eq!(metadata["status"], "success");
    assert_eq!(metadata["progress"], 100);
    assert_eq!(metadata["naturalWidth"], 1344);
    assert_eq!(metadata["naturalHeight"], 768);
    assert_eq!(metadata["durationMs"], 6583);
    assert_eq!(metadata["localTaskSha256"], "c".repeat(64));
    assert_eq!(metadata["mimeType"], "video/mp4");

    fixture.client().get("/v1/tasks/media-ingest-1").unwrap();
    assert_eq!(fixture.canvas.get_project("project-1").unwrap().revision, 3);
}

#[test]
fn allowlisted_image_ingest_creates_one_final_image_node_without_playback_urls() {
    let fixture = Fixture::new();
    let request = ImageIngestRequest {
        project_id: "project-1".to_owned(),
        node_id: "image-1".to_owned(),
        request_id: "image-ingest-1".to_owned(),
        base_revision: 0,
        actor: Actor::Agent,
        inbox_file_name: "frame-001.png".to_owned(),
        expected_sha256: "e".repeat(64),
        title: "Frame 001".to_owned(),
        position: Point { x: 10.0, y: 20.0 },
        size: CanvasSize {
            width: 320.0,
            height: 180.0,
        },
    };
    let first = fixture
        .client()
        .post("/v1/media/image-ingests", &request)
        .unwrap();
    assert_eq!(first["data"]["mode"], "allowlisted_image_ingest");
    assert_eq!(first["data"]["paid"], false);
    assert_eq!(first["data"]["duplicate"], false);
    assert_eq!(first["data"]["node_id"], "image-1");
    assert_eq!(first["data"]["canvas_revision"], 1);
    assert!(first["data"]["reference"].get("playbackUrl").is_none());

    let duplicate = fixture
        .client()
        .post("/v1/media/image-ingests", &request)
        .unwrap();
    assert_eq!(duplicate["data"]["duplicate"], true);
    assert_eq!(duplicate["data"]["canvas_revision"], 1);

    let project = fixture.canvas.get_project("project-1").unwrap();
    assert_eq!(project.revision, 1);
    assert_eq!(project.project["nodes"].as_array().unwrap().len(), 1);
    let node = &project.project["nodes"][0];
    assert_eq!(node["type"], "image");
    assert_eq!(node["title"], "Frame 001");
    let metadata = &node["metadata"];
    assert_eq!(
        metadata["storageKey"],
        "local-ref:asset-fedcba9876543210fedcba9876543210"
    );
    assert_eq!(metadata["content"], metadata["storageKey"]);
    assert_eq!(metadata["status"], "success");
    assert_eq!(metadata["naturalWidth"], 1024);
    assert_eq!(metadata["naturalHeight"], 576);
    assert_eq!(metadata["mimeType"], "image/png");
    assert_eq!(metadata["localMedia"]["rootId"], "agent-media");
    assert_eq!(metadata["localMedia"]["sha256"], "e".repeat(64));
    assert!(metadata["localMedia"].get("playbackUrl").is_none());
    assert_eq!(
        project.project["operationState"]["tasks"]
            .as_object()
            .unwrap()
            .len(),
        0,
        "synchronous image ingest must not leave a task behind"
    );
}

#[test]
fn paid_video_generation_only_creates_a_pending_approval_task_and_placeholder() {
    let fixture = Fixture::new();
    let ingest = ImageIngestRequest {
        project_id: "project-1".to_owned(),
        node_id: "keyframe-1".to_owned(),
        request_id: "keyframe-ingest-1".to_owned(),
        base_revision: 0,
        actor: Actor::Agent,
        inbox_file_name: "frame-001.png".to_owned(),
        expected_sha256: "e".repeat(64),
        title: "Frame 001".to_owned(),
        position: Point { x: 0.0, y: 0.0 },
        size: CanvasSize {
            width: 320.0,
            height: 180.0,
        },
    };
    fixture
        .client()
        .post("/v1/media/image-ingests", &ingest)
        .unwrap();

    let request = VideoGenerationRequest {
        project_id: "project-1".to_owned(),
        node_id: "gen-1".to_owned(),
        request_id: "paid-request-1".to_owned(),
        base_revision: 1,
        actor: Actor::Agent,
        title: "S01 生成".to_owned(),
        prompt: "镜头缓推，全景剪影，写实光线".to_owned(),
        image_node_id: "keyframe-1".to_owned(),
        resolution: "768P".to_owned(),
        duration_seconds: 6,
        position: Point { x: 400.0, y: 0.0 },
        size: CanvasSize {
            width: 420.0,
            height: 236.0,
        },
    };
    let first = fixture
        .client()
        .post("/v1/generation/video-requests", &request)
        .unwrap();
    assert_eq!(first["data"]["status"], "pending_approval");
    assert_eq!(first["data"]["paid"], true);
    assert_eq!(first["data"]["approval_required"], true);
    assert_eq!(first["data"]["duplicate"], false);
    assert_eq!(first["data"]["canvas_task_id"], "paid-gen-paid-request-1");
    assert_eq!(first["data"]["canvas_revision"], 2);
    let cost = first["data"]["estimated_cost_yuan"].as_f64().unwrap();
    assert!((cost - 0.54).abs() < 1e-9);

    let duplicate = fixture
        .client()
        .post("/v1/generation/video-requests", &request)
        .unwrap();
    assert_eq!(duplicate["data"]["duplicate"], true);
    assert_eq!(duplicate["data"]["canvas_revision"], 2);

    let project = fixture.canvas.get_project("project-1").unwrap();
    assert_eq!(project.revision, 2);
    assert_eq!(project.project["nodes"].as_array().unwrap().len(), 2);
    let generated = project.project["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == "gen-1")
        .unwrap();
    assert_eq!(generated["type"], "video");
    assert_eq!(generated["metadata"]["status"], "pending_approval");
    assert_eq!(generated["metadata"]["imageNodeId"], "keyframe-1");
    let task = &project.project["operationState"]["tasks"]["paid-gen-paid-request-1"];
    assert_eq!(task["status"], "pending_approval");
    assert_eq!(task["kind"], "paid_video_generation");
    assert_eq!(task["details"]["paid"], true);
    assert_eq!(task["details"]["prompt"], "镜头缓推，全景剪影，写实光线");
    assert_eq!(task["details"]["resolution"], "768P");
    assert_eq!(task["details"]["durationSeconds"], 6);

    let mut missing_keyframe = request.clone();
    missing_keyframe.request_id = "paid-request-2".to_owned();
    missing_keyframe.node_id = "gen-2".to_owned();
    missing_keyframe.image_node_id = "no-such-node".to_owned();
    missing_keyframe.base_revision = 2;
    let not_found = fixture
        .client()
        .post("/v1/generation/video-requests", &missing_keyframe)
        .unwrap_err();
    assert_eq!(not_found.code, "NOT_FOUND");

    let mut bad_resolution = request.clone();
    bad_resolution.request_id = "paid-request-3".to_owned();
    bad_resolution.node_id = "gen-3".to_owned();
    bad_resolution.resolution = "4K".to_owned();
    bad_resolution.base_revision = 2;
    let invalid = fixture
        .client()
        .post("/v1/generation/video-requests", &bad_resolution)
        .unwrap_err();
    assert_eq!(invalid.code, "INVALID_REQUEST");
}

#[test]
fn completed_legacy_ingest_repairs_its_loading_node_without_rewriting_the_task() {
    let fixture = Fixture::new();
    let request = VideoIngestRequest {
        project_id: "project-1".to_owned(),
        node_id: "legacy-video".to_owned(),
        request_id: "legacy-ingest".to_owned(),
        base_revision: 0,
        actor: Actor::Agent,
        inbox_file_name: "legacy.mp4".to_owned(),
        expected_sha256: "d".repeat(64),
        title: "Legacy shot".to_owned(),
        position: Point { x: 0.0, y: 0.0 },
        size: CanvasSize {
            width: 320.0,
            height: 180.0,
        },
    };
    fixture
        .client()
        .post("/v1/media/video-ingests", &request)
        .unwrap();
    fixture
        .canvas
        .apply_protocol_batch(
            "project-1",
            json!({
                "protocolVersion": 1,
                "actor": "system",
                "requestId": "legacy-task-only-completion",
                "projectId": "project-1",
                "baseRevision": 2,
                "timestamp": "2026-01-01T00:00:03Z",
                "operations": [{
                    "type": "task.update",
                    "taskId": "agent-media-legacy-ingest",
                    "status": "succeeded",
                    "details": { "legacyTaskOnly": true }
                }]
            }),
            false,
        )
        .unwrap();
    let legacy = fixture.canvas.get_project("project-1").unwrap();
    assert_eq!(legacy.revision, 3);
    assert_eq!(legacy.project["nodes"][0]["metadata"]["status"], "loading");

    fixture
        .client()
        .get("/v1/tasks/media-legacy-ingest")
        .unwrap();
    let repaired = fixture.canvas.get_project("project-1").unwrap();
    assert_eq!(repaired.revision, 4);
    assert_eq!(
        repaired.project["nodes"][0]["metadata"]["status"],
        "success"
    );
    assert_eq!(
        repaired.project["nodes"][0]["metadata"]["content"],
        "local-ref:asset-0123456789abcdef0123456789abcdef"
    );
    assert_eq!(
        repaired.project["operationState"]["tasks"]["agent-media-legacy-ingest"]["details"]
            ["legacyTaskOnly"],
        true
    );
    fixture
        .client()
        .get("/v1/tasks/media-legacy-ingest")
        .unwrap();
    assert_eq!(fixture.canvas.get_project("project-1").unwrap().revision, 4);
}

#[test]
fn arbitrary_media_paths_are_rejected_by_the_schema() {
    let fixture = Fixture::new();
    let request = json!({
        "project_id": "project-1",
        "node_id": "video-1",
        "request_id": "ingest-path",
        "base_revision": 0,
        "actor": "agent",
        "inbox_file_name": "shot.mp4",
        "expected_sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "title": "Shot",
        "position": { "x": 0, "y": 0 },
        "size": { "width": 320, "height": 180 },
        "path": "../../escape.mp4"
    });
    let error = fixture
        .client()
        .post("/v1/media/video-ingests", &request)
        .unwrap_err();
    assert_eq!(error.code, "INVALID_REQUEST");
    assert!(
        fixture.canvas.get_project("project-1").unwrap().project["nodes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn cli_emits_json_and_stable_exit_codes_without_token_arguments() {
    let fixture = Fixture::new();
    let binary = cargo_bin("infinite-canvas");
    let output = Command::new(&binary)
        .args([
            "--endpoint",
            &fixture.endpoint(),
            "--credential-file",
            fixture.credentials.path().to_str().unwrap(),
            "capabilities",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["ok"], true);
    assert_eq!(json["data"]["transport"]["listen_host"], "127.0.0.1");

    let wrong = fixture._root.path().join("wrong-credential.json");
    fs::write(
        &wrong,
        serde_json::to_vec(&CredentialDocument {
            version: 1,
            credential_id: "wrong-credential".to_owned(),
            secret: "wrong-secret-that-is-long-enough-for-validation".to_owned(),
        })
        .unwrap(),
    )
    .unwrap();
    fs::set_permissions(&wrong, fs::Permissions::from_mode(0o600)).unwrap();
    let rejected = Command::new(binary)
        .args([
            "--endpoint",
            &fixture.endpoint(),
            "--credential-file",
            wrong.to_str().unwrap(),
            "projects",
            "list",
        ])
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(4));
    let json: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(json["error"]["code"], "UNAUTHORIZED");
    assert!(!String::from_utf8_lossy(&rejected.stdout).contains("wrong-secret"));

    let usage = Command::new(cargo_bin("infinite-canvas"))
        .arg("not-a-command")
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(2));
    let json: Value = serde_json::from_slice(&usage.stdout).unwrap();
    assert_eq!(json["error"]["code"], "INVALID_REQUEST");
}

#[test]
fn cli_exposes_project_create_and_fixed_inbox_video_ingest() {
    let fixture = Fixture::new();
    let binary = cargo_bin("infinite-canvas");
    let create_file = fixture._root.path().join("create-project.json");
    fs::write(
        &create_file,
        serde_json::to_vec(&ProjectCreateRequest {
            project_id: "cli-review-wall".to_owned(),
            request_id: "cli-create-review-wall".to_owned(),
            actor: Actor::Agent,
            title: "CLI review wall".to_owned(),
        })
        .unwrap(),
    )
    .unwrap();
    let endpoint = fixture.endpoint();
    let credential_path = fixture.credentials.path();
    let common = [
        "--endpoint",
        endpoint.as_str(),
        "--credential-file",
        credential_path.to_str().unwrap(),
    ];
    let created = Command::new(&binary)
        .args(common)
        .args(["projects", "create", "--file"])
        .arg(&create_file)
        .output()
        .unwrap();
    assert_eq!(created.status.code(), Some(0));
    let created: Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(created["data"]["project_id"], "cli-review-wall");
    assert_eq!(created["data"]["revision"], 1);

    let inbox = Command::new(&binary)
        .args(common)
        .args(["media", "inbox"])
        .output()
        .unwrap();
    assert_eq!(inbox.status.code(), Some(0));
    let inbox: Value = serde_json::from_slice(&inbox.stdout).unwrap();
    assert_eq!(inbox["data"]["arbitrary_paths"], false);
    assert_eq!(inbox["data"]["inbox_id"], "agent-media-inbox");
    assert_eq!(inbox["data"]["absolute_path_exposed"], false);
    assert!(inbox["data"].get("path").is_none());

    let ingest_file = fixture._root.path().join("ingest-video.json");
    fs::write(
        &ingest_file,
        serde_json::to_vec(&VideoIngestRequest {
            project_id: "project-1".to_owned(),
            node_id: "cli-video-1".to_owned(),
            request_id: "cli-ingest-1".to_owned(),
            base_revision: 0,
            actor: Actor::Agent,
            inbox_file_name: "shot-001.mp4".to_owned(),
            expected_sha256: "b".repeat(64),
            title: "CLI Shot 001".to_owned(),
            position: Point { x: 0.0, y: 0.0 },
            size: CanvasSize {
                width: 320.0,
                height: 180.0,
            },
        })
        .unwrap(),
    )
    .unwrap();
    let ingested = Command::new(&binary)
        .args(common)
        .args(["media", "video", "ingest", "--file"])
        .arg(&ingest_file)
        .output()
        .unwrap();
    assert_eq!(ingested.status.code(), Some(0));
    let ingested: Value = serde_json::from_slice(&ingested.stdout).unwrap();
    assert_eq!(ingested["data"]["task_id"], "media-cli-ingest-1");
    assert_eq!(ingested["data"]["canvas_revision"], 2);

    let image_file = fixture._root.path().join("ingest-image.json");
    fs::write(
        &image_file,
        serde_json::to_vec(&ImageIngestRequest {
            project_id: "project-1".to_owned(),
            node_id: "cli-image-1".to_owned(),
            request_id: "cli-image-ingest-1".to_owned(),
            base_revision: 2,
            actor: Actor::Agent,
            inbox_file_name: "frame-001.png".to_owned(),
            expected_sha256: "f".repeat(64),
            title: "CLI Frame 001".to_owned(),
            position: Point { x: 0.0, y: 0.0 },
            size: CanvasSize {
                width: 320.0,
                height: 240.0,
            },
        })
        .unwrap(),
    )
    .unwrap();
    let image_ingested = Command::new(binary)
        .args(common)
        .args(["media", "image", "ingest", "--file"])
        .arg(&image_file)
        .output()
        .unwrap();
    assert_eq!(image_ingested.status.code(), Some(0));
    let image_ingested: Value = serde_json::from_slice(&image_ingested.stdout).unwrap();
    assert_eq!(image_ingested["data"]["node_id"], "cli-image-1");
    assert_eq!(image_ingested["data"]["canvas_revision"], 3);
}
