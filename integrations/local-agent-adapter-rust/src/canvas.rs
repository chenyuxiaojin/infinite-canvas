use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::BridgeError;

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
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CanvasOperation {
    CreateTextNode {
        node_id: String,
        title: String,
        content: String,
        position: Point,
        size: CanvasSize,
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

pub trait CanvasOperationAdapter: Send + Sync {
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
}

impl SqliteCanvasAdapter {
    pub fn open(database_path: impl Into<PathBuf>) -> Result<Self, BridgeError> {
        let database_path = database_path.into();
        if !database_path.is_absolute() {
            return Err(BridgeError::forbidden(
                "The desktop canvas database path must be absolute.",
            ));
        }
        let adapter = Self { database_path };
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
            let raw = row?;
            serde_json::from_str(&raw).map_err(|_| {
                BridgeError::internal("A desktop canvas project contains invalid JSON.")
            })
        })
        .collect()
    }

    pub fn save_human_project(&self, project: Value) -> Result<Value, BridgeError> {
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

impl CanvasOperationAdapter for SqliteCanvasAdapter {
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
        let mut proposed = apply_to_project(project, &request.operations)?;
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

fn validate_request(request: &AgentOperationRequest) -> Result<(), BridgeError> {
    validate_identifier("project_id", &request.project_id, 64)?;
    validate_identifier("request_id", &request.request_id, 128)?;
    if request.base_revision.len() != 64
        || !request
            .base_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(BridgeError::invalid(
            "base_revision must be a SHA-256 digest.",
        ));
    }
    if request.operations.is_empty() || request.operations.len() > MAX_OPERATIONS {
        return Err(BridgeError::invalid(
            "An operation request must contain between 1 and 100 operations.",
        ));
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
