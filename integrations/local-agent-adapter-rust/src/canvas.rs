use std::{
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::Duration,
};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use url::Url;

use crate::BridgeError;

const DESKTOP_LOCAL_USER_ID: &str = "desktop-local";
const MAX_OPERATIONS: usize = 100;
const MAX_TEXT_BYTES: usize = 100_000;
const CANVAS_OPERATION_PROTOCOL_VERSION: u64 = 1;

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
    pub base_revision: u64,
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
    pub previous_revision: u64,
    pub revision: u64,
    pub proposed_revision: u64,
    pub operations_applied: usize,
    pub project: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectSummary {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectDocument {
    pub project: Value,
    pub revision: u64,
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
    fn list_projects(&self) -> Result<Vec<ProjectSummary>, BridgeError>;
    fn get_project(&self, project_id: &str) -> Result<ProjectDocument, BridgeError>;
    fn create_project(
        &self,
        request: ProjectCreateRequest,
    ) -> Result<ProjectCreateResult, BridgeError>;
    fn apply_operations(
        &self,
        request: AgentOperationRequest,
        dry_run: bool,
    ) -> Result<CanvasOperationResult, BridgeError>;
    fn apply_protocol_batch(
        &self,
        project_id: &str,
        batch: Value,
        dry_run: bool,
    ) -> Result<ProtocolOutcome, BridgeError>;
    fn find_runtime_task(
        &self,
        runtime_task_id: &str,
    ) -> Result<Option<CanvasRuntimeTaskReference>, BridgeError>;
}

pub struct CanonicalCanvasAdapter {
    database_path: PathBuf,
    protocol: Arc<dyn CanvasProtocolExecutor>,
}

impl CanonicalCanvasAdapter {
    pub fn open(
        database_path: impl Into<PathBuf>,
        protocol: Arc<dyn CanvasProtocolExecutor>,
    ) -> Result<Self, BridgeError> {
        let database_path = database_path.into();
        if !database_path.is_absolute() {
            return Err(BridgeError::forbidden(
                "The desktop canvas database path must be absolute.",
            ));
        }
        let adapter = Self {
            database_path,
            protocol,
        };
        adapter.connect()?;
        Ok(adapter)
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
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
            serde_json::from_str(&row?).map_err(|_| {
                BridgeError::internal("A desktop canvas project contains invalid JSON.")
            })
        })
        .collect()
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

    pub fn save_human_project(&self, project: Value) -> Result<Value, BridgeError> {
        let metadata = project_metadata(&project)?;
        let incoming_revision = project_revision(&project)?;
        let raw = serde_json::to_string(&project)
            .map_err(|_| BridgeError::invalid("The canvas project could not be encoded."))?;
        let mut connection = self.connect()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
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
        let saved = match current {
            Some((_, _, deleted_at)) if !deleted_at.is_empty() => {
                return Err(BridgeError::conflict(
                    "PROJECT_DELETED",
                    "The canvas project has been deleted.",
                ));
            }
            Some((current_raw, current_updated, _)) => {
                let current_project: Value = serde_json::from_str(&current_raw).map_err(|_| {
                    BridgeError::internal("A desktop canvas project contains invalid JSON.")
                })?;
                let current_revision = project_revision(&current_project)?;
                let state_conflict = current_revision == incoming_revision
                    && current_project.get("operationState") != project.get("operationState");
                if current_revision > incoming_revision
                    || state_conflict
                    || (current_revision == incoming_revision
                        && timestamp_is_newer(&current_updated, &metadata.updated_at)?)
                {
                    current_project
                } else {
                    transaction.execute(
                        "UPDATE canvas_projects
                         SET project_data = ?1, updated_at = ?2
                         WHERE user_id = ?3 AND id = ?4 AND deleted_at = ''",
                        params![raw, metadata.updated_at, DESKTOP_LOCAL_USER_ID, metadata.id],
                    )?;
                    project
                }
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
        transaction.commit()?;
        Ok(saved)
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

impl CanvasOperationAdapter for CanonicalCanvasAdapter {
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
                title: project["title"]
                    .as_str()
                    .unwrap_or("Untitled canvas")
                    .to_owned(),
                updated_at,
                revision: project_revision(&project)?,
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
        let project: Value = serde_json::from_str(&raw)
            .map_err(|_| BridgeError::internal("The canvas project contains invalid JSON."))?;
        Ok(ProjectDocument {
            revision: project_revision(&project)?,
            project,
        })
    }

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
        let outcome = self.protocol.apply(source, batch, &now)?;
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

    fn apply_operations(
        &self,
        request: AgentOperationRequest,
        dry_run: bool,
    ) -> Result<CanvasOperationResult, BridgeError> {
        validate_request(&request)?;
        let now = now_rfc3339()?;
        let batch = canonical_batch(&request, &now)?;
        let outcome = self.apply_protocol_batch(&request.project_id, batch, dry_run)?;
        Ok(CanvasOperationResult {
            project_id: request.project_id.clone(),
            request_id: request.request_id.clone(),
            actor: request.actor,
            dry_run,
            duplicate: outcome.duplicate,
            previous_revision: outcome.previous_revision,
            revision: if dry_run {
                outcome.previous_revision
            } else {
                outcome.revision
            },
            proposed_revision: outcome.revision,
            operations_applied: request.operations.len(),
            project: outcome.project,
        })
    }

    fn apply_protocol_batch(
        &self,
        project_id: &str,
        batch: Value,
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
        let now = now_rfc3339()?;
        let outcome = self.protocol.apply(project, batch, &now)?;

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
    Ok(ProjectMetadata {
        id,
        created_at,
        updated_at,
    })
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

fn validate_request(request: &AgentOperationRequest) -> Result<(), BridgeError> {
    validate_identifier("project_id", &request.project_id, 64)?;
    validate_identifier("request_id", &request.request_id, 128)?;
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

    struct UnusedProtocol;

    impl CanvasProtocolExecutor for UnusedProtocol {
        fn apply(
            &self,
            _project: Value,
            _batch: Value,
            _now: &str,
        ) -> Result<ProtocolOutcome, BridgeError> {
            Err(BridgeError::internal("unused test protocol"))
        }
    }

    fn project(revision: u64, updated_at: &str) -> Value {
        json!({
            "id": "project-1",
            "title": "Acceptance",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": updated_at,
            "nodes": [],
            "connections": [],
            "operationState": {
                "version": 1,
                "revision": revision,
                "locks": {},
                "tasks": {},
                "requests": {},
                "audit": []
            }
        })
    }

    #[test]
    fn external_operations_map_to_the_canonical_protocol_only() {
        let request = AgentOperationRequest {
            project_id: "project-1".to_owned(),
            request_id: "request-1".to_owned(),
            base_revision: 7,
            actor: Actor::Agent,
            operations: vec![
                CanvasOperation::SetProjectTitle {
                    title: "Agent draft".to_owned(),
                },
                CanvasOperation::SetNodeText {
                    node_id: "node-1".to_owned(),
                    title: None,
                    content: "Editable".to_owned(),
                },
            ],
        };
        let batch = canonical_batch(&request, "2026-01-01T00:00:00Z").unwrap();
        assert_eq!(batch["protocolVersion"], 1);
        assert_eq!(batch["baseRevision"], 7);
        assert_eq!(batch["operations"][0]["type"], "project.update");
        assert_eq!(batch["operations"][1]["type"], "node.update");
        assert_eq!(
            batch["operations"][1]["patch"]["metadata"]["content"],
            "Editable"
        );
    }

    fn media_reference(mime_type: &str) -> MediaReferencePayload {
        MediaReferencePayload {
            asset_id: "asset-0123456789abcdef0123456789abcdef".to_owned(),
            storage_key: "local-ref:asset-0123456789abcdef0123456789abcdef".to_owned(),
            root_id: "agent-media".to_owned(),
            relative_path: "verified/agent-image-0123.png".to_owned(),
            sha256: "a".repeat(64),
            mime_type: mime_type.to_owned(),
            bytes: 2048,
            file_name: "agent-image-0123.png".to_owned(),
            width: Some(2048),
            height: Some(1152),
            duration_ms: None,
            mode: "project_copy".to_owned(),
        }
    }

    #[test]
    fn media_and_config_nodes_map_to_generic_node_create_with_controlled_references() {
        let request = AgentOperationRequest {
            project_id: "project-1".to_owned(),
            request_id: "request-media".to_owned(),
            base_revision: 3,
            actor: Actor::Agent,
            operations: vec![
                CanvasOperation::CreateImageNode {
                    node_id: "image-1".to_owned(),
                    title: "关键帧".to_owned(),
                    reference: media_reference("image/png"),
                    position: Point { x: 0.0, y: 0.0 },
                    size: CanvasSize {
                        width: 320.0,
                        height: 180.0,
                    },
                },
                CanvasOperation::CreateConfigNode {
                    node_id: "config-1".to_owned(),
                    title: "生成配置".to_owned(),
                    position: Point { x: 400.0, y: 0.0 },
                    size: CanvasSize {
                        width: 240.0,
                        height: 160.0,
                    },
                    model: Some("MiniMax-H3".to_owned()),
                    generation_size: Some("16:9".to_owned()),
                    count: Some(1),
                },
            ],
        };
        let batch = canonical_batch(&request, "2026-01-01T00:00:00Z").unwrap();
        assert_eq!(batch["operations"][0]["type"], "node.create");
        assert_eq!(batch["operations"][0]["node"]["type"], "image");
        let metadata = &batch["operations"][0]["node"]["metadata"];
        assert_eq!(
            metadata["storageKey"],
            "local-ref:asset-0123456789abcdef0123456789abcdef"
        );
        assert_eq!(metadata["content"], metadata["storageKey"]);
        assert_eq!(metadata["localMedia"]["rootId"], "agent-media");
        assert_eq!(metadata["naturalWidth"], 2048);
        assert_eq!(batch["operations"][1]["node"]["type"], "config");
        assert_eq!(
            batch["operations"][1]["node"]["metadata"]["model"],
            "MiniMax-H3"
        );
        assert_eq!(batch["operations"][1]["node"]["metadata"]["count"], 1);
    }

    #[test]
    fn media_node_references_reject_wrong_mime_traversal_and_free_form_keys() {
        let base = |reference: MediaReferencePayload| AgentOperationRequest {
            project_id: "project-1".to_owned(),
            request_id: "request-neg".to_owned(),
            base_revision: 0,
            actor: Actor::Agent,
            operations: vec![CanvasOperation::CreateImageNode {
                node_id: "image-1".to_owned(),
                title: "关键帧".to_owned(),
                reference,
                position: Point { x: 0.0, y: 0.0 },
                size: CanvasSize {
                    width: 320.0,
                    height: 180.0,
                },
            }],
        };
        assert!(validate_request(&base(media_reference("video/mp4"))).is_err());
        let mut traversal = media_reference("image/png");
        traversal.relative_path = "../escape.png".to_owned();
        assert!(validate_request(&base(traversal)).is_err());
        let mut mismatched_key = media_reference("image/png");
        mismatched_key.storage_key = "local-ref:asset-other".to_owned();
        assert!(validate_request(&base(mismatched_key)).is_err());
        let mut absolute = media_reference("image/png");
        absolute.relative_path = "/etc/passwd".to_owned();
        assert!(validate_request(&base(absolute)).is_err());
        assert!(validate_request(&base(media_reference("image/png"))).is_ok());
    }

    #[test]
    fn path_fields_are_rejected_before_the_operation_layer() {
        let request = json!({
            "project_id": "project-1",
            "request_id": "request-1",
            "base_revision": 0,
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

    #[test]
    fn protocol_executor_endpoint_is_fixed_to_loopback() {
        assert!(
            HttpCanvasProtocolExecutor::new("http://127.0.0.1:3100/internal/canvas-operation")
                .is_ok()
        );
        for endpoint in [
            "http://localhost:3100/internal/canvas-operation",
            "http://0.0.0.0:3100/internal/canvas-operation",
            "https://127.0.0.1:3100/internal/canvas-operation",
            "http://127.0.0.1:3100/other",
        ] {
            assert!(HttpCanvasProtocolExecutor::new(endpoint).is_err());
        }
    }

    #[test]
    fn a_higher_canvas_revision_wins_even_if_its_wall_clock_is_older() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("canvas.db");
        Connection::open(&database)
            .unwrap()
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
        let adapter = CanonicalCanvasAdapter::open(database, Arc::new(UnusedProtocol)).unwrap();
        adapter
            .save_human_project(project(1, "2026-01-02T00:00:00Z"))
            .unwrap();
        adapter
            .save_human_project(project(2, "2026-01-01T12:00:00Z"))
            .unwrap();
        assert_eq!(adapter.get_project("project-1").unwrap().revision, 2);
    }

    #[test]
    fn desktop_poll_reads_updated_at_without_decoding_project_json() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("canvas.db");
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
        connection
            .execute(
                "INSERT INTO canvas_projects (user_id, id, project_data, created_at, updated_at, deleted_at)
                 VALUES (?1, ?2, 'not-json', ?3, ?4, '')",
                params![DESKTOP_LOCAL_USER_ID, "project-1", "2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z"],
            )
            .unwrap();
        drop(connection);

        let adapter = CanonicalCanvasAdapter::open(database, Arc::new(UnusedProtocol)).unwrap();
        assert_eq!(adapter.project_updated_at("project-1").unwrap(), "2026-01-02T00:00:00Z");
    }
}
