use std::{
    collections::HashSet,
    sync::Arc,
    thread,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::BridgeError;
use url::Url;
const CANVAS_OPERATION_PROTOCOL_VERSION: u64 = 1;

const DESKTOP_LOCAL_USER_ID: &str = "desktop-local";
const MAX_OPERATIONS: usize = 100;
const MAX_TEXT_BYTES: usize = 100_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor {
    Agent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanvasSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaReferencePayload {
    pub asset_id: String,
    pub storage_key: String,
    pub root_id: String,
    pub relative_path: String,
    pub sha256: String,
    pub mime_type: String,
    pub bytes: u64,
    pub file_name: String,
    #[serde(default)]
    pub width: Option<u64>,
    #[serde(default)]
    pub height: Option<u64>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    pub mode: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CanvasOperation {
    CreateTextNode {
        node_id: String,
        title: String,
        content: String,
        position: Point,
        size: CanvasSize,
    },
    CreateImageNode {
        node_id: String,
        title: String,
        reference: MediaReferencePayload,
        position: Point,
        size: CanvasSize,
    },
    CreateVideoNode {
        node_id: String,
        title: String,
        reference: MediaReferencePayload,
        position: Point,
        size: CanvasSize,
    },
    CreateConfigNode {
        node_id: String,
        title: String,
        position: Point,
        size: CanvasSize,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        generation_size: Option<String>,
        #[serde(default)]
        count: Option<u32>,
    },
    MoveNode {
        node_id: String,
        position: Point,
    },
    SetNodeText {
        node_id: String,
        #[serde(default)]
        title: Option<String>,
        content: String,
    },
    SetProjectTitle {
        title: String,
    },
    AddConnection {
        connection_id: String,
        from_node_id: String,
        to_node_id: String,
    },
    RemoveConnection {
        connection_id: String,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentOperationRequest {
    pub project_id: String,
    pub request_id: String,
    pub base_revision: String,
    pub actor: Actor,
    pub operations: Vec<CanvasOperation>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectCreateRequest {
    pub project_id: String,
    pub request_id: String,
    pub actor: Actor,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectCreateResult {
    pub project_id: String,
    pub request_id: String,
    pub actor: Actor,
    pub duplicate: bool,
    pub revision: u64,
    pub project: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CanvasOperationResult {
    pub project_id: String,
    pub request_id: String,
    pub actor: Actor,
    pub dry_run: bool,
    pub duplicate: bool,
    pub previous_revision: String,
    pub revision: String,
    pub proposed_revision: String,
    pub operations_applied: usize,
    pub project: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectSummary {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub revision: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectDocument {
    pub project: Value,
    pub revision: String,
}

#[derive(Clone, Debug)]
pub struct CanvasRuntimeTaskReference {
    pub project_id: String,
    pub canvas_task_id: String,
    pub node_id: String,
    pub kind: String,
    pub revision: u64,
    pub status: String,
}

#[derive(Clone, Debug)]
pub struct ProtocolOutcome {
    pub project: Value,
    pub ok: bool,
    pub duplicate: bool,
    pub previous_revision: u64,
    pub revision: u64,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub error: Option<Value>,
}

pub trait CanvasProtocolExecutor: Send + Sync {
    fn apply(
        &self,
        project: Value,
        batch: Value,
        now: &str,
    ) -> Result<ProtocolOutcome, BridgeError>;
}

pub struct HttpCanvasProtocolExecutor {
    endpoint: String,
    client: ureq::Agent,
}

impl HttpCanvasProtocolExecutor {
    pub fn new(endpoint: &str) -> Result<Self, BridgeError> {
        let parsed = Url::parse(endpoint)
            .map_err(|_| BridgeError::invalid("The canvas protocol endpoint is invalid."))?;
        if parsed.scheme() != "http"
            || parsed.host_str() != Some("127.0.0.1")
            || parsed.port().is_none()
            || parsed.path() != "/internal/canvas-operation"
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(BridgeError::forbidden(
                "The canvas protocol executor must use the fixed loopback endpoint.",
            ));
        }
        Ok(Self {
            endpoint: endpoint.to_owned(),
            client: ureq::AgentBuilder::new()
                .redirects(0)
                .timeout(Duration::from_secs(30))
                .build(),
        })
    }
}

impl CanvasProtocolExecutor for HttpCanvasProtocolExecutor {
    fn apply(
        &self,
        project: Value,
        batch: Value,
        now: &str,
    ) -> Result<ProtocolOutcome, BridgeError> {
        let payload = json!({ "project": project, "batch": batch, "now": now });
        let mut response = None;
        for attempt in 0..3 {
            match self.client.post(&self.endpoint).send_json(payload.clone()) {
                Ok(value) => {
                    response = Some(value);
                    break;
                }
                Err(ureq::Error::Status(_, _)) => {
                    return Err(BridgeError::internal(
                        "The shared canvas operation protocol rejected an internal request.",
                    ))
                }
                Err(ureq::Error::Transport(_)) if attempt < 2 => {
                    // Next may briefly stop accepting connections while the WebView is
                    // navigating. Request IDs make replay safe if the response was lost.
                    thread::sleep(Duration::from_millis(100));
                }
                Err(ureq::Error::Transport(_)) => {
                    return Err(BridgeError::unavailable(
                        "The shared canvas operation protocol is not running.",
                    ))
                }
            }
        }
        let response = response.ok_or_else(|| {
            BridgeError::unavailable("The shared canvas operation protocol is not running.")
        })?;
        let value = response.into_json::<Value>().map_err(|_| {
            BridgeError::internal("The shared canvas operation protocol returned invalid JSON.")
        })?;
        if value["ok"].as_bool() != Some(true) {
            return Err(BridgeError::internal(
                "The shared canvas operation protocol returned an invalid envelope.",
            ));
        }
        parse_protocol_outcome(
            value
                .get("outcome")
                .cloned()
                .ok_or_else(|| BridgeError::internal("The protocol outcome is missing."))?,
        )
    }
}

pub trait CanvasOperationAdapter: Send + Sync {
    fn create_project(&self, _request: ProjectCreateRequest) -> Result<ProjectCreateResult, BridgeError> { Err(BridgeError::unavailable("Canvas protocol unavailable")) }
    fn apply_protocol_batch(&self, _project_id: &str, _batch: Value, _dry_run: bool) -> Result<ProtocolOutcome, BridgeError> { Err(BridgeError::unavailable("Canvas protocol unavailable")) }
    fn find_runtime_task(&self, _runtime_task_id: &str) -> Result<Option<CanvasRuntimeTaskReference>, BridgeError> { Ok(None) }

    fn list_projects(&self) -> Result<Vec<ProjectSummary>, BridgeError>;
    fn get_project(&self, project_id: &str) -> Result<ProjectDocument, BridgeError>;
    fn apply_operations(
        &self,
        request: AgentOperationRequest,
        dry_run: bool,
    ) -> Result<CanvasOperationResult, BridgeError>;
}

pub struct SqliteCanvasAdapter {
    database_path: PathBuf,
    protocol: Option<Arc<dyn CanvasProtocolExecutor>>,
}

impl SqliteCanvasAdapter {
    pub fn open(database_path: impl Into<PathBuf>) -> Result<Self, BridgeError> {
        let database_path = database_path.into();
        if !database_path.is_absolute() {
            return Err(BridgeError::forbidden(
                "The desktop canvas database path must be absolute.",
            ));
        }
        let adapter = Self { database_path, protocol: None };
        let connection = adapter.connect()?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS agent_operation_requests (
                request_id TEXT PRIMARY KEY NOT NULL,
                project_id TEXT NOT NULL,
                payload_hash TEXT NOT NULL,
                response_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_agent_operation_project
                ON agent_operation_requests(project_id, created_at);",
        )?;
        crate::history::initialize(&connection)?;
        Ok(adapter)
    }

    pub fn open_with_protocol(database_path: impl Into<PathBuf>, protocol: Arc<dyn CanvasProtocolExecutor>) -> Result<Self, BridgeError> {
        let mut adapter = Self::open(database_path)?; adapter.protocol = Some(protocol); Ok(adapter)
    }
    fn protocol(&self) -> Result<&dyn CanvasProtocolExecutor, BridgeError> {
        self.protocol.as_deref().ok_or_else(|| BridgeError::unavailable("Canvas operation protocol unavailable"))
    }
    pub fn project_updated_at(&self, project_id: &str) -> Result<String, BridgeError> {
        validate_identifier("project_id", project_id, 64)?;
        self.connect()?
            .query_row(
                "SELECT updated_at FROM canvas_projects
                 WHERE user_id = ?1 AND id = ?2 AND deleted_at = ''",
                params![DESKTOP_LOCAL_USER_ID, project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| BridgeError::not_found("The canvas project was not found."))
    }


    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn deleted_project_ids(&self) -> Result<Vec<String>, BridgeError> {
        let connection = self.connect()?;
        let mut statement = connection.prepare("SELECT id FROM canvas_projects WHERE user_id = ?1 AND deleted_at <> ''")?;
        let rows = statement.query_map([DESKTOP_LOCAL_USER_ID], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(BridgeError::from)
    }

    pub fn list_project_documents(&self) -> Result<Vec<Value>, BridgeError> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT project_data FROM canvas_projects
             WHERE user_id = ?1 AND deleted_at = ''
             ORDER BY updated_at DESC",
        )?;
        let rows = statement.query_map([DESKTOP_LOCAL_USER_ID], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let raw = row?;
            serde_json::from_str(&raw).map_err(|_| {
                BridgeError::internal("A desktop canvas project contains invalid JSON.")
            })
        })
        .collect()
    }

    pub fn save_human_project(&self, project: Value) -> Result<Value, BridgeError> {
        self.save_human_project_checked(project, None)
    }

    pub fn save_human_project_checked(&self, project: Value, expected_revision: Option<&str>) -> Result<Value, BridgeError> {
        let metadata = project_metadata(&project)?;
        let raw = serde_json::to_string(&project)
            .map_err(|_| BridgeError::invalid("The canvas project could not be encoded."))?;
        let mut connection = self.connect()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(BridgeError::from)?;
        let current = transaction
            .query_row(
                "SELECT project_data, updated_at, deleted_at FROM canvas_projects
                 WHERE user_id = ?1 AND id = ?2",
                params![DESKTOP_LOCAL_USER_ID, metadata.id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        if let Some(expected) = expected_revision {
            let matches = match &current {
                Some((raw, _, deleted)) => deleted.is_empty() && revision(raw.as_bytes()) == expected,
                None => expected.is_empty(),
            };
            if !matches {
                return Err(BridgeError::conflict("REVISION_CONFLICT", "画布已有其他修改，当前编辑已保留，请核对后重试。"));
            }
        }
        let previous_raw = current.as_ref().map(|(raw, _, _)| raw.clone());
        let saved = match current {
            Some((current_raw, _, deleted_at)) if !deleted_at.is_empty() => {
                return Err(BridgeError::conflict(
                    "PROJECT_DELETED",
                    "The canvas project has been deleted.",
                ));
            }
            Some((current_raw, current_updated, _))
                if timestamp_is_newer(&current_updated, &metadata.updated_at)? =>
            {
                serde_json::from_str(&current_raw).map_err(|_| {
                    BridgeError::internal("A desktop canvas project contains invalid JSON.")
                })?
            }
            Some(_) => {
                transaction.execute(
                    "UPDATE canvas_projects
                     SET project_data = ?1, updated_at = ?2
                     WHERE user_id = ?3 AND id = ?4 AND deleted_at = ''",
                    params![raw, metadata.updated_at, DESKTOP_LOCAL_USER_ID, metadata.id],
                )?;
                project
            }
            None => {
                transaction.execute(
                    "INSERT INTO canvas_projects
                     (user_id, id, project_data, created_at, updated_at, deleted_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, '')",
                    params![
                        DESKTOP_LOCAL_USER_ID,
                        metadata.id,
                        raw,
                        metadata.created_at,
                        metadata.updated_at
                    ],
                )?;
                project
            }
        };
        crate::history::record_save(&transaction, DESKTOP_LOCAL_USER_ID, &metadata.id, previous_raw.as_deref(), &serde_json::to_string(&saved).map_err(|_|BridgeError::internal("无法记录保存版本"))?)?;
        transaction.commit()?;
        Ok(saved)
    }

    pub fn history_list(&self, id: &str) -> Result<Value, BridgeError> {
        crate::history::list(&self.connect()?, DESKTOP_LOCAL_USER_ID, id)
    }
    pub fn history_preview(&self, id: &str, sequence: i64) -> Result<Value, BridgeError> {
        crate::history::preview(&self.connect()?, DESKTOP_LOCAL_USER_ID, id, sequence)
    }
    pub fn history_restore(&self, id: &str, sequence: i64, expected: &str, request_id: &str) -> Result<Value, BridgeError> {
        let mut connection=self.connect()?;
        let transaction=connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result=crate::history::restore(&transaction, DESKTOP_LOCAL_USER_ID, id, sequence, expected, request_id, |project|project_metadata(project).map(|_|()))?;
        transaction.commit()?;
        Ok(result)
    }

    pub fn delete_human_projects(&self, project_ids: &[String]) -> Result<usize, BridgeError> {
        let ids = project_ids
            .iter()
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .collect::<Vec<_>>();
        if ids.is_empty() || ids.iter().any(|id| !valid_identifier(id, 64)) {
            return Err(BridgeError::invalid(
                "The canvas project identifiers are invalid.",
            ));
        }
        let now = now_rfc3339()?;
        let mut connection = self.connect()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut deleted = 0;
        for id in ids {
            deleted += transaction.execute(
                "UPDATE canvas_projects SET project_data = '', updated_at = ?1, deleted_at = ?1
                 WHERE user_id = ?2 AND id = ?3 AND deleted_at = ''",
                params![now, DESKTOP_LOCAL_USER_ID, id],
            )?;
        }
        transaction.commit()?;
        Ok(deleted)
    }

    fn connect(&self) -> Result<Connection, BridgeError> {
        let connection = Connection::open(&self.database_path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        Ok(connection)
    }
}

impl CanvasOperationAdapter for SqliteCanvasAdapter {
    fn create_project(
        &self,
        request: ProjectCreateRequest,
    ) -> Result<ProjectCreateResult, BridgeError> {
        validate_project_create_request(&request)?;
        let now = now_rfc3339()?;
        let batch = json!({
            "protocolVersion": CANVAS_OPERATION_PROTOCOL_VERSION,
            "actor": "agent",
            "requestId": request.request_id,
            "projectId": request.project_id,
            "baseRevision": 0,
            "timestamp": now,
            "operations": [{ "type": "project.update", "title": request.title }],
        });
        let mut connection = self.connect()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = transaction
            .query_row(
                "SELECT project_data, deleted_at FROM canvas_projects
                 WHERE user_id = ?1 AND id = ?2",
                params![DESKTOP_LOCAL_USER_ID, request.project_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let existed = current.is_some();
        let source = if let Some((raw, deleted_at)) = current {
            if !deleted_at.is_empty() {
                return Err(BridgeError::conflict(
                    "PROJECT_DELETED",
                    "The requested canvas project id belongs to a deleted project.",
                ));
            }
            let project: Value = serde_json::from_str(&raw).map_err(|_| {
                BridgeError::internal("A desktop canvas project contains invalid JSON.")
            })?;
            if project
                .pointer("/operationState/requests")
                .and_then(Value::as_object)
                .is_none_or(|requests| !requests.contains_key(&request.request_id))
            {
                return Err(BridgeError::conflict(
                    "PROJECT_EXISTS",
                    "The requested canvas project id already exists.",
                ));
            }
            project
        } else {
            empty_canvas_project(&request.project_id, &now)
        };
        let outcome = self.protocol()?.apply(source, batch, &now)?;
        if !outcome.ok {
            return Err(protocol_error(&outcome));
        }
        if !existed {
            let raw = serde_json::to_string(&outcome.project).map_err(|_| {
                BridgeError::internal("The created canvas project could not be encoded.")
            })?;
            transaction.execute(
                "INSERT INTO canvas_projects
                 (user_id, id, project_data, created_at, updated_at, deleted_at)
                 VALUES (?1, ?2, ?3, ?4, ?4, '')",
                params![DESKTOP_LOCAL_USER_ID, request.project_id, raw, now],
            )?;
        }
        let current_revision = project_revision(&outcome.project)?;
        transaction.commit()?;
        Ok(ProjectCreateResult {
            project_id: request.project_id,
            request_id: request.request_id,
            actor: request.actor,
            duplicate: outcome.duplicate,
            revision: current_revision,
            project: outcome.project,
        })
    }


    fn apply_protocol_batch(
        &self,
        project_id: &str,
        mut batch: Value,
        dry_run: bool,
    ) -> Result<ProtocolOutcome, BridgeError> {
        validate_identifier("project_id", project_id, 64)?;
        let mut connection = self.connect()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let raw = transaction
            .query_row(
                "SELECT project_data FROM canvas_projects
                 WHERE user_id = ?1 AND id = ?2 AND deleted_at = ''",
                params![DESKTOP_LOCAL_USER_ID, project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| BridgeError::not_found("The canvas project was not found."))?;
        let project: Value = serde_json::from_str(&raw)
            .map_err(|_| BridgeError::internal("The canvas project contains invalid JSON."))?;
        if let Some(expected) = batch["baseRevision"].as_str() {
            let repeated = batch["requestId"].as_str().is_some_and(|id| project["operationState"]["requests"].get(id).is_some());
            let operation_revision = project_revision(&project)?;
            if !repeated && expected != revision(raw.as_bytes()) && expected.parse::<u64>().ok() != Some(operation_revision) {
                return Err(BridgeError::conflict("REVISION_CONFLICT", "画布已有其他修改，请重新读取后重试。"));
            }
            batch["baseRevision"] = json!(operation_revision);
        }
        let now = now_rfc3339()?;
        let outcome = self.protocol()?.apply(project, batch, &now)?;

        if dry_run {
            transaction.rollback()?;
            if outcome.ok {
                return Ok(outcome);
            }
            return Err(protocol_error(&outcome));
        }

        let proposed_raw = serde_json::to_string(&outcome.project).map_err(|_| {
            BridgeError::internal("The updated canvas project could not be encoded.")
        })?;
        let updated_at = outcome.project["updatedAt"]
            .as_str()
            .unwrap_or(now.as_str());
        let updated = transaction.execute(
            "UPDATE canvas_projects SET project_data = ?1, updated_at = ?2
             WHERE user_id = ?3 AND id = ?4 AND project_data = ?5 AND deleted_at = ''",
            params![
                proposed_raw,
                updated_at,
                DESKTOP_LOCAL_USER_ID,
                project_id,
                raw
            ],
        )?;
        if updated != 1 {
            return Err(BridgeError::conflict(
                "STALE_REVISION",
                "The canvas changed while the Agent operation was being applied.",
            ));
        }
        crate::history::record_save(&transaction, DESKTOP_LOCAL_USER_ID, project_id, Some(&raw), &proposed_raw)?;
        transaction.commit()?;
        if outcome.ok {
            Ok(outcome)
        } else {
            Err(protocol_error(&outcome))
        }
    }

    fn find_runtime_task(
        &self,
        runtime_task_id: &str,
    ) -> Result<Option<CanvasRuntimeTaskReference>, BridgeError> {
        validate_identifier("task_id", runtime_task_id, 128)?;
        for project in self.list_project_documents()? {
            let Some(tasks) = project
                .pointer("/operationState/tasks")
                .and_then(Value::as_object)
            else {
                continue;
            };
            for (canvas_task_id, task) in tasks {
                if task
                    .pointer("/details/runtimeTaskId")
                    .and_then(Value::as_str)
                    != Some(runtime_task_id)
                {
                    continue;
                }
                return Ok(Some(CanvasRuntimeTaskReference {
                    project_id: project["id"].as_str().unwrap_or_default().to_owned(),
                    canvas_task_id: canvas_task_id.clone(),
                    node_id: task["nodeId"].as_str().unwrap_or_default().to_owned(),
                    kind: task["kind"].as_str().unwrap_or_default().to_owned(),
                    revision: project_revision(&project)?,
                    status: task["status"].as_str().unwrap_or_default().to_owned(),
                }));
            }
        }
        Ok(None)
    }

    fn list_projects(&self) -> Result<Vec<ProjectSummary>, BridgeError> {
        let connection = self.connect()?;
        let mut statement = connection.prepare(
            "SELECT id, project_data, updated_at FROM canvas_projects
             WHERE user_id = ?1 AND deleted_at = ''
             ORDER BY updated_at DESC",
        )?;
        let rows = statement.query_map([DESKTOP_LOCAL_USER_ID], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (id, raw, updated_at) = row?;
            let project: Value = serde_json::from_str(&raw).map_err(|_| {
                BridgeError::internal("A desktop canvas project contains invalid JSON.")
            })?;
            Ok(ProjectSummary {
                id,
                title: project
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled canvas")
                    .to_owned(),
                updated_at,
                revision: revision(raw.as_bytes()),
            })
        })
        .collect()
    }

    fn get_project(&self, project_id: &str) -> Result<ProjectDocument, BridgeError> {
        validate_identifier("project_id", project_id, 64)?;
        let connection = self.connect()?;
        let raw = connection
            .query_row(
                "SELECT project_data FROM canvas_projects
                 WHERE user_id = ?1 AND id = ?2 AND deleted_at = ''",
                params![DESKTOP_LOCAL_USER_ID, project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| BridgeError::not_found("The canvas project was not found."))?;
        let project = serde_json::from_str(&raw)
            .map_err(|_| BridgeError::internal("The canvas project contains invalid JSON."))?;
        Ok(ProjectDocument {
            project,
            revision: revision(raw.as_bytes()),
        })
    }

    fn apply_operations(
        &self,
        request: AgentOperationRequest,
        dry_run: bool,
    ) -> Result<CanvasOperationResult, BridgeError> {
        validate_request(&request)?;
        let payload = serde_json::to_vec(&request)
            .map_err(|_| BridgeError::invalid("The operation request could not be encoded."))?;
        let payload_hash = revision(&payload);
        let mut connection = self.connect()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

        if !dry_run {
            if let Some((stored_hash, stored_response)) = transaction
                .query_row(
                    "SELECT payload_hash, response_json FROM agent_operation_requests
                     WHERE request_id = ?1",
                    [&request.request_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?
            {
                if stored_hash != payload_hash {
                    return Err(BridgeError::conflict(
                        "REQUEST_ID_REUSED",
                        "The request_id was already used with a different payload.",
                    ));
                }
                let mut result = serde_json::from_str::<CanvasOperationResult>(&stored_response)
                    .map_err(|_| BridgeError::internal("The idempotency record is invalid."))?;
                result.duplicate = true;
                return Ok(result);
            }
        }

        let raw = transaction
            .query_row(
                "SELECT project_data FROM canvas_projects
                 WHERE user_id = ?1 AND id = ?2 AND deleted_at = ''",
                params![DESKTOP_LOCAL_USER_ID, request.project_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .ok_or_else(|| BridgeError::not_found("The canvas project was not found."))?;
        let current_revision = revision(raw.as_bytes());
        if request.base_revision != current_revision {
            return Err(BridgeError::conflict(
                "REVISION_CONFLICT",
                "The canvas changed after the Agent read it; reload before applying operations.",
            )
            .with_details(json!({ "current_revision": current_revision })));
        }
        let project: Value = serde_json::from_str(&raw)
            .map_err(|_| BridgeError::internal("The canvas project contains invalid JSON."))?;
        let mut proposed = if let Some(protocol) = &self.protocol {
            let now = now_rfc3339()?;
            let mut batch = canonical_batch(&request, &now)?;
            batch["baseRevision"] = json!(project_revision(&project)?);
            let outcome = protocol.apply(project, batch, &now)?;
            if !outcome.ok { return Err(protocol_error(&outcome)); }
            outcome.project
        } else { apply_to_project(project, &request.operations)? };
        if !dry_run {
            proposed["updatedAt"] = Value::String(now_rfc3339()?);
        }
        let proposed_raw = serde_json::to_string(&proposed).map_err(|_| {
            BridgeError::internal("The updated canvas project could not be encoded.")
        })?;
        let proposed_revision = revision(proposed_raw.as_bytes());
        let result_revision = if dry_run {
            current_revision.clone()
        } else {
            proposed_revision.clone()
        };
        let result = CanvasOperationResult {
            project_id: request.project_id.clone(),
            request_id: request.request_id.clone(),
            actor: request.actor,
            dry_run,
            duplicate: false,
            previous_revision: current_revision.clone(),
            revision: result_revision,
            proposed_revision,
            operations_applied: request.operations.len(),
            project: proposed,
        };
        if dry_run {
            transaction.rollback()?;
            return Ok(result);
        }
        let updated = transaction.execute(
            "UPDATE canvas_projects SET project_data = ?1, updated_at = ?2
             WHERE user_id = ?3 AND id = ?4 AND project_data = ?5 AND deleted_at = ''",
            params![
                proposed_raw,
                result.project["updatedAt"].as_str().unwrap_or_default(),
                DESKTOP_LOCAL_USER_ID,
                request.project_id,
                raw
            ],
        )?;
        if updated != 1 {
            return Err(BridgeError::conflict(
                "REVISION_CONFLICT",
                "The canvas changed while the Agent operation was being applied.",
            ));
        }
        crate::history::record_save(&transaction, DESKTOP_LOCAL_USER_ID, &request.project_id, Some(&raw), &proposed_raw)?;
        let response_json = serde_json::to_string(&result)
            .map_err(|_| BridgeError::internal("The operation result could not be recorded."))?;
        transaction.execute(
            "INSERT INTO agent_operation_requests
             (request_id, project_id, payload_hash, response_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                request.request_id,
                request.project_id,
                payload_hash,
                response_json,
                now_rfc3339()?
            ],
        )?;
        transaction.commit()?;
        Ok(result)
    }
}

fn canonical_batch(request: &AgentOperationRequest, now: &str) -> Result<Value, BridgeError> {
    let operations = request
        .operations
        .iter()
        .map(canonical_operation)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({
        "protocolVersion": CANVAS_OPERATION_PROTOCOL_VERSION,
        "actor": "agent",
        "requestId": request.request_id,
        "projectId": request.project_id,
        "baseRevision": request.base_revision,
        "timestamp": now,
        "operations": operations,
    }))
}

fn canonical_operation(operation: &CanvasOperation) -> Result<Value, BridgeError> {
    Ok(match operation {
        CanvasOperation::CreateTextNode {
            node_id,
            title,
            content,
            position,
            size,
        } => json!({
            "type": "node.create",
            "node": {
                "id": node_id,
                "type": "text",
                "title": title,
                "position": position,
                "width": size.width,
                "height": size.height,
                "metadata": { "content": content, "prompt": content, "status": "success" }
            }
        }),
        CanvasOperation::CreateImageNode {
            node_id,
            title,
            reference,
            position,
            size,
        } => media_node_operation(node_id, "image", title, reference, position, size),
        CanvasOperation::CreateVideoNode {
            node_id,
            title,
            reference,
            position,
            size,
        } => media_node_operation(node_id, "video", title, reference, position, size),
        CanvasOperation::CreateConfigNode {
            node_id,
            title,
            position,
            size,
            model,
            generation_size,
            count,
        } => {
            let mut metadata = serde_json::Map::new();
            if let Some(model) = model {
                metadata.insert("model".to_owned(), Value::String(model.clone()));
            }
            if let Some(generation_size) = generation_size {
                metadata.insert("size".to_owned(), Value::String(generation_size.clone()));
            }
            if let Some(count) = count {
                metadata.insert("count".to_owned(), json!(count));
            }
            json!({
                "type": "node.create",
                "node": {
                    "id": node_id,
                    "type": "config",
                    "title": title,
                    "position": position,
                    "width": size.width,
                    "height": size.height,
                    "metadata": metadata
                }
            })
        }
        CanvasOperation::MoveNode { node_id, position } => {
            json!({ "type": "node.update", "nodeId": node_id, "patch": { "position": position } })
        }
        CanvasOperation::SetNodeText {
            node_id,
            title,
            content,
        } => {
            let mut patch = json!({
                "metadata": { "content": content, "prompt": content, "status": "success" }
            });
            if let Some(title) = title {
                patch["title"] = Value::String(title.clone());
            }
            json!({ "type": "node.update", "nodeId": node_id, "patch": patch })
        }
        CanvasOperation::SetProjectTitle { title } => {
            json!({ "type": "project.update", "title": title })
        }
        CanvasOperation::AddConnection {
            connection_id,
            from_node_id,
            to_node_id,
        } => json!({
            "type": "connection.create",
            "connection": {
                "id": connection_id,
                "fromNodeId": from_node_id,
                "toNodeId": to_node_id
            }
        }),
        CanvasOperation::RemoveConnection { connection_id } => {
            json!({ "type": "connection.delete", "connectionId": connection_id })
        }
    })
}

fn media_node_operation(
    node_id: &str,
    node_type: &str,
    title: &str,
    reference: &MediaReferencePayload,
    position: &Point,
    size: &CanvasSize,
) -> Value {
    let mut metadata = json!({
        "content": reference.storage_key,
        "storageKey": reference.storage_key,
        "localMedia": reference,
        "status": "success",
        "bytes": reference.bytes,
        "mimeType": reference.mime_type
    });
    if let Some(width) = reference.width {
        metadata["naturalWidth"] = json!(width);
    }
    if let Some(height) = reference.height {
        metadata["naturalHeight"] = json!(height);
    }
    if let Some(duration_ms) = reference.duration_ms {
        metadata["durationMs"] = json!(duration_ms);
    }
    json!({
        "type": "node.create",
        "node": {
            "id": node_id,
            "type": node_type,
            "title": title,
            "position": position,
            "width": size.width,
            "height": size.height,
            "metadata": metadata
        }
    })
}

fn parse_protocol_outcome(value: Value) -> Result<ProtocolOutcome, BridgeError> {
    let project = value
        .get("project")
        .cloned()
        .ok_or_else(|| BridgeError::internal("The shared protocol project is missing."))?;
    let result = value
        .get("result")
        .ok_or_else(|| BridgeError::internal("The shared protocol result is missing."))?;
    let previous_revision = result["previousRevision"].as_u64().ok_or_else(|| {
        BridgeError::internal("The shared protocol previous revision is invalid.")
    })?;
    let revision = result["revision"]
        .as_u64()
        .ok_or_else(|| BridgeError::internal("The shared protocol revision is invalid."))?;
    let error = result.get("error").cloned();
    Ok(ProtocolOutcome {
        project,
        ok: result["ok"].as_bool() == Some(true),
        duplicate: result["duplicate"].as_bool() == Some(true),
        previous_revision,
        revision,
        error_code: error
            .as_ref()
            .and_then(|value| value["code"].as_str())
            .map(ToOwned::to_owned),
        error_message: error
            .as_ref()
            .and_then(|value| value["message"].as_str())
            .map(ToOwned::to_owned),
        error,
    })
}

fn protocol_error(outcome: &ProtocolOutcome) -> BridgeError {
    let message = outcome
        .error_message
        .clone()
        .unwrap_or_else(|| "The shared canvas operation was rejected.".to_owned());
    let mut error = match outcome.error_code.as_deref() {
        Some("stale_revision") => BridgeError::conflict("STALE_REVISION", message),
        Some("request_id_reused") => BridgeError::conflict("REQUEST_ID_REUSED", message),
        Some("locked_node") => BridgeError::conflict("LOCKED_NODE", message),
        Some("node_not_found") | Some("connection_not_found") | Some("task_not_found") => {
            BridgeError::not_found(message)
        }
        Some("node_exists") => BridgeError::conflict("NODE_EXISTS", message),
        Some("connection_exists") => BridgeError::conflict("CONNECTION_EXISTS", message),
        _ => BridgeError::invalid(message),
    };
    error.details = outcome.error.clone();
    error
}

fn project_revision(project: &Value) -> Result<u64, BridgeError> {
    match project.pointer("/operationState/revision") {
        Some(value) => value
            .as_u64()
            .ok_or_else(|| BridgeError::internal("The canvas operation revision is invalid.")),
        None => Ok(0),
    }
}

fn empty_canvas_project(project_id: &str, now: &str) -> Value {
    json!({
        "id": project_id,
        "title": "未命名画布",
        "createdAt": now,
        "updatedAt": now,
        "nodes": [],
        "connections": [],
        "chatSessions": [],
        "activeChatId": null,
        "agentConfig": null,
        "autoTitlePending": false,
        "backgroundMode": "lines",
        "showImageInfo": false,
        "viewport": { "x": 0, "y": 0, "k": 1 },
        "sidePanel": { "open": true, "width": 280 },
        "agentPanel": { "open": false, "width": 390 },
        "operationState": {
            "version": CANVAS_OPERATION_PROTOCOL_VERSION,
            "revision": 0,
            "locks": {},
            "tasks": {},
            "requests": {},
            "audit": []
        }
    })
}

fn validate_project_create_request(request: &ProjectCreateRequest) -> Result<(), BridgeError> {
    validate_identifier("project_id", &request.project_id, 64)?;
    validate_identifier("request_id", &request.request_id, 128)?;
    validate_text("title", &request.title, 256)
}

fn validate_media_reference(
    reference: &MediaReferencePayload,
    mime_prefix: &str,
) -> Result<(), BridgeError> {
    let asset_valid = reference.asset_id.starts_with("asset-")
        && reference.asset_id.len() <= 80
        && reference
            .asset_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    let sha_valid = reference.sha256.len() == 64
        && reference
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
    let relative = &reference.relative_path;
    let relative_valid = !relative.is_empty()
        && relative.len() <= 1024
        && !relative.starts_with('/')
        && !relative.contains('\\')
        && !relative.chars().any(char::is_control)
        && relative
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..");
    let root_valid = !reference.root_id.is_empty()
        && reference.root_id.len() <= 80
        && reference
            .root_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if !asset_valid
        || reference.storage_key != format!("local-ref:{}", reference.asset_id)
        || !sha_valid
        || !relative_valid
        || !root_valid
        || !reference.mime_type.starts_with(mime_prefix)
        || reference.bytes == 0
        || !matches!(reference.mode.as_str(), "reference" | "project_copy")
    {
        return Err(BridgeError::invalid(
            "The media reference must be a controlled local-ref asset with a valid root, relative path and SHA-256.",
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct ProjectMetadata {
    id: String,
    created_at: String,
    updated_at: String,
}

fn project_metadata(project: &Value) -> Result<ProjectMetadata, BridgeError> {
    let object = project
        .as_object()
        .ok_or_else(|| BridgeError::invalid("The canvas project must be a JSON object."))?;
    let id = required_string(object.get("id"), "id", 64)?;
    validate_identifier("project id", &id, 64)?;
    let created_at = required_string(object.get("createdAt"), "createdAt", 64)?;
    let updated_at = required_string(object.get("updatedAt"), "updatedAt", 64)?;
    parse_timestamp(&created_at)?;
    parse_timestamp(&updated_at)?;
    if !object.get("nodes").is_some_and(Value::is_array)
        || !object.get("connections").is_some_and(Value::is_array)
    {
        return Err(BridgeError::invalid(
            "The canvas project must contain nodes and connections arrays.",
        ));
    }
    let mut node_ids=HashSet::new();
    for node in project["nodes"].as_array().unwrap() {
        let id=node["id"].as_str().filter(|id|!id.trim().is_empty()).ok_or_else(||BridgeError::invalid("画布存在空白节点编号。"))?;
        if !node_ids.insert(id){return Err(BridgeError::invalid("画布存在重复节点编号。"));}
    }
    let mut connection_ids=HashSet::new();
    for connection in project["connections"].as_array().unwrap() {
        let id=connection["id"].as_str().filter(|id|!id.trim().is_empty()).ok_or_else(||BridgeError::invalid("画布存在空白连线编号。"))?;
        if !connection_ids.insert(id){return Err(BridgeError::invalid("画布存在重复连线编号。"));}
        if !["fromNodeId","toNodeId"].iter().all(|key|connection[*key].as_str().is_some_and(|id|node_ids.contains(id))){return Err(BridgeError::invalid("连线的节点不存在，保存已停止，原画布未改动。"));}
    }
    Ok(ProjectMetadata {
        id,
        created_at,
        updated_at,
    })
}

fn validate_request(request: &AgentOperationRequest) -> Result<(), BridgeError> {
    validate_identifier("project_id", &request.project_id, 64)?;
    validate_identifier("request_id", &request.request_id, 128)?;
    if request.base_revision.len() != 64 || !request.base_revision.bytes().all(|b| b.is_ascii_hexdigit()) { return Err(BridgeError::invalid("base_revision must be a SHA-256 revision")); }
    if request.operations.is_empty() || request.operations.len() > MAX_OPERATIONS {
        return Err(BridgeError::invalid(
            "The request must contain between 1 and 100 operations.",
        ));
    }
    for operation in &request.operations {
        match operation {
            CanvasOperation::CreateTextNode {
                node_id,
                title,
                content,
                position,
                size,
            } => {
                validate_identifier("node_id", node_id, 64)?;
                validate_text("title", title, 256)?;
                validate_text("content", content, MAX_TEXT_BYTES)?;
                validate_point(position)?;
                validate_size(size)?;
            }
            CanvasOperation::CreateImageNode {
                node_id,
                title,
                reference,
                position,
                size,
            } => {
                validate_identifier("node_id", node_id, 64)?;
                validate_text("title", title, 256)?;
                validate_media_reference(reference, "image/")?;
                validate_point(position)?;
                validate_size(size)?;
            }
            CanvasOperation::CreateVideoNode {
                node_id,
                title,
                reference,
                position,
                size,
            } => {
                validate_identifier("node_id", node_id, 64)?;
                validate_text("title", title, 256)?;
                validate_media_reference(reference, "video/")?;
                validate_point(position)?;
                validate_size(size)?;
            }
            CanvasOperation::CreateConfigNode {
                node_id,
                title,
                position,
                size,
                model,
                generation_size,
                count,
            } => {
                validate_identifier("node_id", node_id, 64)?;
                validate_text("title", title, 256)?;
                validate_point(position)?;
                validate_size(size)?;
                if let Some(model) = model {
                    validate_text("model", model, 64)?;
                }
                if let Some(generation_size) = generation_size {
                    validate_text("generation_size", generation_size, 32)?;
                }
                if let Some(count) = count {
                    if !(1..=9).contains(count) {
                        return Err(BridgeError::invalid(
                            "The config node count must be between 1 and 9.",
                        ));
                    }
                }
            }
            CanvasOperation::MoveNode { node_id, position } => {
                validate_identifier("node_id", node_id, 64)?;
                validate_point(position)?;
            }
            CanvasOperation::SetNodeText {
                node_id,
                title,
                content,
            } => {
                validate_identifier("node_id", node_id, 64)?;
                if let Some(title) = title {
                    validate_text("title", title, 256)?;
                }
                validate_text("content", content, MAX_TEXT_BYTES)?;
            }
            CanvasOperation::SetProjectTitle { title } => {
                validate_text("title", title, 256)?;
            }
            CanvasOperation::AddConnection {
                connection_id,
                from_node_id,
                to_node_id,
            } => {
                validate_identifier("connection_id", connection_id, 64)?;
                validate_identifier("from_node_id", from_node_id, 64)?;
                validate_identifier("to_node_id", to_node_id, 64)?;
            }
            CanvasOperation::RemoveConnection { connection_id } => {
                validate_identifier("connection_id", connection_id, 64)?;
            }
        }
    }
    Ok(())
}

fn apply_to_project(
    mut project: Value,
    operations: &[CanvasOperation],
) -> Result<Value, BridgeError> {
    project_metadata(&project)?;
    for operation in operations {
        match operation {
            CanvasOperation::CreateTextNode {
                node_id,
                title,
                content,
                position,
                size,
            } => {
                validate_identifier("node_id", node_id, 64)?;
                validate_text("title", title, 256)?;
                validate_text("content", content, MAX_TEXT_BYTES)?;
                validate_point(position)?;
                validate_size(size)?;
                let nodes = project["nodes"].as_array_mut().ok_or_else(|| {
                    BridgeError::invalid("The canvas project nodes field is invalid.")
                })?;
                if nodes
                    .iter()
                    .any(|node| node["id"].as_str() == Some(node_id))
                {
                    return Err(BridgeError::conflict(
                        "NODE_EXISTS",
                        "A canvas node already uses the requested node_id.",
                    ));
                }
                nodes.push(json!({
                    "id": node_id,
                    "type": "text",
                    "title": title,
                    "position": { "x": position.x, "y": position.y },
                    "width": size.width,
                    "height": size.height,
                    "metadata": { "content": content, "status": "success" }
                }));
            }
            CanvasOperation::CreateImageNode { .. } | CanvasOperation::CreateVideoNode { .. } | CanvasOperation::CreateConfigNode { .. } => {
                let op = canonical_operation(operation)?;
                let node = op["node"].clone();
                let nodes = project["nodes"].as_array_mut().ok_or_else(|| BridgeError::invalid("Canvas nodes missing"))?;
                if nodes.iter().any(|n| n["id"] == node["id"]) { return Err(BridgeError::conflict("NODE_EXISTS", "The node already exists.")); }
                nodes.push(node);
            }
            CanvasOperation::MoveNode { node_id, position } => {
                validate_point(position)?;
                let node = editable_node(&mut project, node_id)?;
                node["position"] = json!({ "x": position.x, "y": position.y });
            }
            CanvasOperation::SetNodeText {
                node_id,
                title,
                content,
            } => {
                validate_text("content", content, MAX_TEXT_BYTES)?;
                if let Some(title) = title {
                    validate_text("title", title, 256)?;
                }
                let node = editable_node(&mut project, node_id)?;
                if node["type"].as_str() != Some("text") {
                    return Err(BridgeError::forbidden(
                        "set_node_text is limited to editable text nodes.",
                    ));
                }
                if let Some(title) = title {
                    node["title"] = Value::String(title.clone());
                }
                if !node["metadata"].is_object() {
                    node["metadata"] = json!({});
                }
                node["metadata"]["content"] = Value::String(content.clone());
                node["metadata"]["status"] = Value::String("success".to_owned());
            }
            CanvasOperation::SetProjectTitle { title } => {
                validate_text("title", title, 256)?;
                project["title"] = Value::String(title.clone());
                project["autoTitlePending"] = Value::Bool(false);
            }
            CanvasOperation::AddConnection {
                connection_id,
                from_node_id,
                to_node_id,
            } => {
                validate_identifier("connection_id", connection_id, 64)?;
                validate_identifier("from_node_id", from_node_id, 64)?;
                validate_identifier("to_node_id", to_node_id, 64)?;
                if from_node_id == to_node_id {
                    return Err(BridgeError::invalid("A node cannot connect to itself."));
                }
                ensure_node_editable(&project, from_node_id)?;
                ensure_node_editable(&project, to_node_id)?;
                let connections = project["connections"].as_array_mut().ok_or_else(|| {
                    BridgeError::invalid("The canvas project connections field is invalid.")
                })?;
                if connections
                    .iter()
                    .any(|connection| connection["id"].as_str() == Some(connection_id))
                {
                    return Err(BridgeError::conflict(
                        "CONNECTION_EXISTS",
                        "A connection already uses the requested connection_id.",
                    ));
                }
                connections.push(json!({
                    "id": connection_id,
                    "fromNodeId": from_node_id,
                    "toNodeId": to_node_id
                }));
            }
            CanvasOperation::RemoveConnection { connection_id } => {
                validate_identifier("connection_id", connection_id, 64)?;
                let connections = project["connections"].as_array().ok_or_else(|| {
                    BridgeError::invalid("The canvas project connections field is invalid.")
                })?;
                let connection = connections
                    .iter()
                    .find(|connection| connection["id"].as_str() == Some(connection_id))
                    .ok_or_else(|| {
                        BridgeError::not_found("The canvas connection was not found.")
                    })?;
                if let Some(node_id) = connection["fromNodeId"].as_str() {
                    ensure_node_editable(&project, node_id)?;
                }
                if let Some(node_id) = connection["toNodeId"].as_str() {
                    ensure_node_editable(&project, node_id)?;
                }
                let connections = project["connections"].as_array_mut().unwrap();
                connections.retain(|connection| connection["id"].as_str() != Some(connection_id));
            }
        }
    }
    Ok(project)
}

fn editable_node<'a>(project: &'a mut Value, node_id: &str) -> Result<&'a mut Value, BridgeError> {
    validate_identifier("node_id", node_id, 64)?;
    let nodes = project["nodes"]
        .as_array_mut()
        .ok_or_else(|| BridgeError::invalid("The canvas project nodes field is invalid."))?;
    let node = nodes
        .iter_mut()
        .find(|node| node["id"].as_str() == Some(node_id))
        .ok_or_else(|| BridgeError::not_found("The canvas node was not found."))?;
    if node_locked(node) {
        return Err(BridgeError::forbidden(
            "The canvas node is locked by the human editor.",
        ));
    }
    Ok(node)
}

fn ensure_node_editable(project: &Value, node_id: &str) -> Result<(), BridgeError> {
    let nodes = project["nodes"]
        .as_array()
        .ok_or_else(|| BridgeError::invalid("The canvas project nodes field is invalid."))?;
    let node = nodes
        .iter()
        .find(|node| node["id"].as_str() == Some(node_id))
        .ok_or_else(|| BridgeError::not_found("A connection endpoint node was not found."))?;
    if node_locked(node) {
        return Err(BridgeError::forbidden(
            "The canvas node is locked by the human editor.",
        ));
    }
    Ok(())
}

fn node_locked(node: &Value) -> bool {
    node["locked"].as_bool() == Some(true)
        || node["metadata"]["locked"].as_bool() == Some(true)
        || node["metadata"]["agentLocked"].as_bool() == Some(true)
}

fn validate_identifier(label: &str, value: &str, max: usize) -> Result<(), BridgeError> {
    if !valid_identifier(value, max) {
        return Err(BridgeError::invalid(format!("{label} is invalid.")));
    }
    Ok(())
}

fn valid_identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
}

fn validate_text(label: &str, value: &str, max: usize) -> Result<(), BridgeError> {
    if value.len() > max || (label == "title" && value.trim().is_empty()) {
        return Err(BridgeError::invalid(format!("{label} is invalid.")));
    }
    Ok(())
}

fn validate_point(point: &Point) -> Result<(), BridgeError> {
    if !point.x.is_finite()
        || !point.y.is_finite()
        || point.x.abs() > 10_000_000.0
        || point.y.abs() > 10_000_000.0
    {
        return Err(BridgeError::invalid(
            "The canvas position is outside the allowed range.",
        ));
    }
    Ok(())
}

fn validate_size(size: &CanvasSize) -> Result<(), BridgeError> {
    if !size.width.is_finite()
        || !size.height.is_finite()
        || !(40.0..=10_000.0).contains(&size.width)
        || !(40.0..=10_000.0).contains(&size.height)
    {
        return Err(BridgeError::invalid(
            "The canvas node size is outside the allowed range.",
        ));
    }
    Ok(())
}

fn required_string(value: Option<&Value>, label: &str, max: usize) -> Result<String, BridgeError> {
    let value = value.and_then(Value::as_str).unwrap_or_default().trim();
    if value.is_empty() || value.len() > max {
        return Err(BridgeError::invalid(format!(
            "The canvas {label} is invalid."
        )));
    }
    Ok(value.to_owned())
}

fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn now_rfc3339() -> Result<String, BridgeError> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|_| BridgeError::internal("The current timestamp could not be encoded."))
}

fn parse_timestamp(value: &str) -> Result<OffsetDateTime, BridgeError> {
    OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| BridgeError::invalid("The canvas timestamp must use RFC3339 format."))
}

fn timestamp_is_newer(current: &str, incoming: &str) -> Result<bool, BridgeError> {
    let current = OffsetDateTime::parse(current, &Rfc3339)
        .map_err(|_| BridgeError::internal("A desktop canvas timestamp is invalid."))?;
    Ok(current > parse_timestamp(incoming)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_compare_and_swap_preserves_other_writers_and_tombstones() {
        let directory = tempfile::tempdir().unwrap();
        let db = directory.path().join("canvas.db");
        let sql = Connection::open(&db).unwrap();
        sql.execute_batch("CREATE TABLE canvas_projects (user_id TEXT NOT NULL, id TEXT NOT NULL, project_data TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL, deleted_at TEXT NOT NULL DEFAULT '', PRIMARY KEY(user_id,id));").unwrap();
        let adapter = SqliteCanvasAdapter::open(db).unwrap();
        adapter.save_human_project_checked(project(),Some("")).unwrap();
        let base = adapter.get_project("project-1").unwrap();
        let mut first = project(); first["title"] = json!("first writer");
        adapter.save_human_project_checked(first.clone(),Some(&base.revision)).unwrap();
        let mut second = project(); second["title"] = json!("second writer");
        assert_eq!(adapter.save_human_project_checked(second,Some(&base.revision)).unwrap_err().code,"REVISION_CONFLICT");
        assert_eq!(adapter.get_project("project-1").unwrap().project["title"],"first writer");
        adapter.delete_human_projects(&["project-1".into()]).unwrap();
        assert_eq!(adapter.deleted_project_ids().unwrap(),vec!["project-1"]);
        assert!(adapter.save_human_project_checked(first,Some("")).is_err());
    }

    fn project() -> Value {
        json!({
            "id": "project-1",
            "title": "Project",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z",
            "nodes": [],
            "connections": []
        })
    }
    #[test]
    fn graph_validation_rejects_missing_or_duplicate_endpoints_without_mutation() {
        let mut value=project();
        value["nodes"]=json!([{"id":"n"}]);value["connections"]=json!([{"id":"c","fromNodeId":"n","toNodeId":"missing"}]);
        assert!(project_metadata(&value).is_err());
        value["connections"][0]["toNodeId"]=json!("n");assert!(project_metadata(&value).is_ok());
        value["nodes"]=json!([{"id":"n"},{"id":"n"}]);assert!(project_metadata(&value).is_err());
        value["nodes"]=json!([{"id":"n"}]);value["connections"].as_array_mut().unwrap().push(json!({"id":"c","fromNodeId":"n","toNodeId":"n"}));assert!(project_metadata(&value).is_err());
    }

    #[test]
    fn operations_are_schema_limited_and_create_editable_text() {
        let result = apply_to_project(
            project(),
            &[CanvasOperation::CreateTextNode {
                node_id: "node-1".to_owned(),
                title: "Draft".to_owned(),
                content: "Editable".to_owned(),
                position: Point { x: 10.0, y: 20.0 },
                size: CanvasSize {
                    width: 320.0,
                    height: 180.0,
                },
            }],
        )
        .unwrap();
        assert_eq!(result["nodes"][0]["type"], "text");
        assert_eq!(result["nodes"][0]["metadata"]["content"], "Editable");
    }

    #[test]
    fn locked_human_nodes_cannot_be_changed() {
        let mut value = project();
        value["nodes"] = json!([{
            "id": "node-1", "type": "text", "title": "Human",
            "position": {"x": 0, "y": 0}, "width": 100, "height": 100,
            "metadata": {"content": "keep", "locked": true}
        }]);
        let error = apply_to_project(
            value,
            &[CanvasOperation::SetNodeText {
                node_id: "node-1".to_owned(),
                title: None,
                content: "replace".to_owned(),
            }],
        )
        .unwrap_err();
        assert_eq!(error.code, "CAPABILITY_DENIED");
    }

    #[test]
    fn path_fields_are_rejected_before_the_operation_layer() {
        let request = json!({
            "project_id": "project-1",
            "request_id": "request-1",
            "base_revision": "0".repeat(64),
            "actor": "agent",
            "operations": [{
                "type": "create_text_node",
                "node_id": "node-1",
                "title": "Draft",
                "content": "text",
                "position": {"x": 0, "y": 0},
                "size": {"width": 100, "height": 100},
                "path": "../../escape"
            }]
        });
        assert!(serde_json::from_value::<AgentOperationRequest>(request).is_err());
    }
}
