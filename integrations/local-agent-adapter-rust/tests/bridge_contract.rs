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
    CanvasProtocolExecutor, CredentialDocument, CredentialStore, ProtocolOutcome, TestClipRequest,
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
        let duplicate = project["operationState"]["processedRequests"]
            .as_array()
            .is_some_and(|entries| {
                entries
                    .iter()
                    .any(|entry| entry["requestId"].as_str() == Some(request_id))
            });
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
            if operation["type"] == "project.update" {
                project["title"] = operation["title"].clone();
            }
        }
        let revision = previous_revision + 1;
        project["updatedAt"] = Value::String(now.to_owned());
        project["operationState"]["revision"] = json!(revision);
        project["operationState"]["processedRequests"] = json!([{
            "requestId": request_id,
            "fingerprint": "fixture",
            "revision": revision,
            "status": "applied",
            "result": { "ok": true, "duplicate": false, "previousRevision": previous_revision, "revision": revision, "items": [] }
        }]);
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

    fn submit_test_clip(&self, request: &TestClipRequest) -> Result<Value, BridgeError> {
        Ok(json!({
            "task_id": format!("task-{}", request.request_id),
            "duplicate": false,
            "mode": "deterministic_local_fixture",
            "paid": false
        }))
    }

    fn task_status(&self, task_id: &str) -> Result<Value, BridgeError> {
        Ok(json!({ "task_id": task_id, "status": "succeeded" }))
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
                    "protocolVersion": 1,
                    "revision": 0,
                    "locks": [],
                    "processedRequests": [],
                    "auditLog": [],
                    "undoStack": []
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
