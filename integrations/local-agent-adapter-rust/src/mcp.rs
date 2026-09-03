use std::{
    io::{self, BufRead, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::{json, Value};

use crate::{
    find_project_binding, read_credential_token, Actor, AgentOperationRequest, BridgeClient,
    BridgeError, CanvasOperation,
};

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub fn serve_mcp_stdio(
    endpoint: &str,
    credential_file: &Path,
    project_directory: Option<&Path>,
) -> Result<(), BridgeError> {
    let token = read_credential_token(credential_file)?;
    let client = BridgeClient::new(endpoint, token)?;
    let (directory, binding) = find_project_binding(project_directory)?;
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();

    for line in stdin.lock().lines() {
        let line = line.map_err(|_| BridgeError::invalid("The MCP request could not be read."))?;
        if line.len() > MAX_MESSAGE_BYTES {
            write_response(
                &mut stdout,
                &jsonrpc_error(
                    Value::Null,
                    -32600,
                    "The MCP request exceeds the 1 MiB limit.",
                ),
            )?;
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => {
                write_response(
                    &mut stdout,
                    &jsonrpc_error(Value::Null, -32700, "The MCP request is not valid JSON."),
                )?;
                continue;
            }
        };
        let Some(id) = request.get("id").cloned() else {
            continue;
        };
        let response = handle_request(&client, &directory, &binding.project_id, id, &request);
        write_response(&mut stdout, &response)?;
    }
    Ok(())
}

fn handle_request(
    client: &BridgeClient,
    project_directory: &Path,
    project_id: &str,
    id: Value,
    request: &Value,
) -> Value {
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return jsonrpc_error(id, -32600, "The MCP request method is missing.");
    };
    match method {
        "initialize" => jsonrpc_result(
            id,
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "infinite-canvas", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "This server is bound to one film directory and one Infinite Canvas project. Read canvas_context first. Use canvas_mutate in dry_run mode before apply."
            }),
        ),
        "ping" => jsonrpc_result(id, json!({})),
        "tools/list" => jsonrpc_result(id, json!({ "tools": tool_catalog() })),
        "tools/call" => {
            let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return jsonrpc_error(id, -32602, "The MCP tool name is missing.");
            };
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match call_tool(client, project_directory, project_id, name, arguments) {
                Ok(value) => jsonrpc_result(id, tool_result(value, false)),
                Err(error) => jsonrpc_result(id, tool_result(json!(error.envelope()), true)),
            }
        }
        _ => jsonrpc_error(id, -32601, "The MCP method is not supported."),
    }
}

fn tool_catalog() -> Vec<Value> {
    vec![
        json!({
            "name": "canvas_context",
            "title": "Read film and canvas context",
            "description": "Read the film directory binding, canvas revision, counts, workflow folders, and optionally a compact summary of selected nodes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "node_ids": { "type": "array", "items": { "type": "string" }, "maxItems": 100 }
                },
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true, "destructiveHint": false }
        }),
        json!({
            "name": "canvas_read",
            "title": "Read canvas nodes",
            "description": "Read all nodes or a requested set of node ids from the canvas bound to the current film directory.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "node_ids": { "type": "array", "items": { "type": "string" }, "maxItems": 100 },
                    "include_connections": { "type": "boolean", "default": true }
                },
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true, "destructiveHint": false }
        }),
        json!({
            "name": "canvas_mutate",
            "title": "Change canvas safely",
            "description": "Preview or apply allowlisted canvas operations. Defaults to dry_run. Apply uses the latest canvas revision and an idempotent request id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mode": { "type": "string", "enum": ["dry_run", "apply"], "default": "dry_run" },
                    "request_id": { "type": "string" },
                    "operations": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 100,
                        "items": {
                            "type": "object",
                            "description": "One allowlisted operation: create_text_node, move_node, set_node_text, set_project_title, add_connection, or remove_connection."
                        }
                    }
                },
                "required": ["operations"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true }
        }),
        json!({
            "name": "canvas_task",
            "title": "Inspect or cancel local canvas tasks",
            "description": "Read the local runtime, inspect a task, or cancel a task created by Infinite Canvas.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["runtime", "status", "cancel"] },
                    "task_id": { "type": "string" }
                },
                "required": ["action"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false }
        }),
    ]
}

fn call_tool(
    client: &BridgeClient,
    project_directory: &Path,
    project_id: &str,
    name: &str,
    arguments: Value,
) -> Result<Value, BridgeError> {
    if !arguments.is_object() {
        return Err(BridgeError::invalid(
            "MCP tool arguments must be a JSON object.",
        ));
    }
    match name {
        "canvas_context" => canvas_context(client, project_directory, project_id, &arguments),
        "canvas_read" => canvas_read(client, project_id, &arguments),
        "canvas_mutate" => canvas_mutate(client, project_id, &arguments),
        "canvas_task" => canvas_task(client, &arguments),
        _ => Err(BridgeError::not_found(
            "The requested canvas MCP tool does not exist.",
        )),
    }
}

fn canvas_context(
    client: &BridgeClient,
    project_directory: &Path,
    project_id: &str,
    arguments: &Value,
) -> Result<Value, BridgeError> {
    let data = project_data(client, project_id)?;
    let project = data.get("project").cloned().unwrap_or_else(|| json!({}));
    let nodes = project
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let selected_ids = string_array(arguments.get("node_ids"))?;
    let selected_nodes = if selected_ids.is_empty() {
        Vec::new()
    } else {
        nodes
            .iter()
            .filter(|node| {
                node.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| selected_ids.iter().any(|selected| selected == id))
            })
            .map(compact_node)
            .collect::<Vec<_>>()
    };
    let folders = workflow_folders(project_directory);
    Ok(json!({
        "binding": {
            "project_id": project_id,
            "project_title": project.get("title").and_then(Value::as_str).unwrap_or("Untitled canvas"),
            "project_directory": project_directory,
        },
        "canvas": {
            "revision": data.get("revision"),
            "node_count": nodes.len(),
            "connection_count": project.get("connections").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
            "selected_nodes": selected_nodes,
        },
        "workflow_folders": folders,
        "next_step": "Use canvas_read for detail. Use canvas_mutate with mode=dry_run before apply."
    }))
}

fn canvas_read(
    client: &BridgeClient,
    project_id: &str,
    arguments: &Value,
) -> Result<Value, BridgeError> {
    let data = project_data(client, project_id)?;
    let project = data.get("project").cloned().unwrap_or_else(|| json!({}));
    let all_nodes = project
        .get("nodes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let ids = string_array(arguments.get("node_ids"))?;
    let nodes = if ids.is_empty() {
        all_nodes
    } else {
        all_nodes
            .into_iter()
            .filter(|node| {
                node.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| ids.iter().any(|selected| selected == id))
            })
            .collect()
    };
    let include_connections = arguments
        .get("include_connections")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let connections = if include_connections {
        let all_connections = project
            .get("connections")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if ids.is_empty() {
            json!(all_connections)
        } else {
            json!(all_connections
                .into_iter()
                .filter(|connection| {
                    let from = connection.get("fromNodeId").and_then(Value::as_str);
                    let to = connection.get("toNodeId").and_then(Value::as_str);
                    from.is_some_and(|id| ids.iter().any(|selected| selected == id))
                        && to.is_some_and(|id| ids.iter().any(|selected| selected == id))
                })
                .collect::<Vec<_>>())
        }
    } else {
        json!([])
    };
    Ok(json!({
        "project_id": project_id,
        "title": project.get("title"),
        "revision": data.get("revision"),
        "nodes": nodes,
        "connections": connections
    }))
}

fn canvas_mutate(
    client: &BridgeClient,
    project_id: &str,
    arguments: &Value,
) -> Result<Value, BridgeError> {
    let operations: Vec<CanvasOperation> = serde_json::from_value(
        arguments
            .get("operations")
            .cloned()
            .ok_or_else(|| BridgeError::invalid("canvas_mutate requires operations."))?,
    )
    .map_err(|_| {
        BridgeError::invalid("One or more canvas operations do not match the allowlisted schema.")
    })?;
    let mode = arguments
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("dry_run");
    if !matches!(mode, "dry_run" | "apply") {
        return Err(BridgeError::invalid(
            "canvas_mutate mode must be dry_run or apply.",
        ));
    }
    let data = project_data(client, project_id)?;
    let base_revision = data
        .get("revision")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            BridgeError::internal("The Agent Bridge did not return a canvas revision.")
        })?;
    let request_id = arguments
        .get("request_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(next_request_id);
    let request = AgentOperationRequest {
        project_id: project_id.to_owned(),
        request_id,
        base_revision: base_revision.to_owned(),
        actor: Actor::Agent,
        operations,
    };
    let mut response = client.post(
        if mode == "apply" {
            "/v1/canvas/operations/apply"
        } else {
            "/v1/canvas/operations/dry-run"
        },
        &request,
    )?;
    if let Some(data) = response.get_mut("data").and_then(Value::as_object_mut) {
        if let Some(project) = data.remove("project") {
            data.insert(
                "project_summary".to_owned(),
                json!({
                    "id": project.get("id"),
                    "title": project.get("title"),
                    "node_count": project.get("nodes").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
                    "connection_count": project.get("connections").and_then(Value::as_array).map(Vec::len).unwrap_or(0),
                }),
            );
        }
    }
    Ok(response)
}

fn canvas_task(client: &BridgeClient, arguments: &Value) -> Result<Value, BridgeError> {
    let action = arguments
        .get("action")
        .and_then(Value::as_str)
        .ok_or_else(|| BridgeError::invalid("canvas_task requires an action."))?;
    match action {
        "runtime" => client.get("/v1/runtime"),
        "status" | "cancel" => {
            let task_id = arguments
                .get("task_id")
                .and_then(Value::as_str)
                .ok_or_else(|| BridgeError::invalid("This canvas_task action requires task_id."))?;
            validate_identifier(task_id)?;
            if action == "status" {
                client.get(&format!("/v1/tasks/{task_id}"))
            } else {
                client.post(&format!("/v1/tasks/{task_id}/cancel"), &json!({}))
            }
        }
        _ => Err(BridgeError::invalid(
            "canvas_task action must be runtime, status, or cancel.",
        )),
    }
}

fn project_data(client: &BridgeClient, project_id: &str) -> Result<Value, BridgeError> {
    let response = client.get(&format!("/v1/projects/{project_id}"))?;
    response
        .get("data")
        .cloned()
        .ok_or_else(|| BridgeError::internal("The Agent Bridge returned no project data."))
}

fn string_array(value: Option<&Value>) -> Result<Vec<String>, BridgeError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or_else(|| BridgeError::invalid("node_ids must be an array."))?;
    if array.len() > 100 {
        return Err(BridgeError::invalid(
            "node_ids may contain at most 100 values.",
        ));
    }
    array
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| BridgeError::invalid("Every node id must be a string."))
        })
        .collect()
}

fn compact_node(node: &Value) -> Value {
    let metadata = node.get("metadata").cloned().unwrap_or_else(|| json!({}));
    json!({
        "id": node.get("id"),
        "type": node.get("type"),
        "title": node.get("title"),
        "content": metadata.get("content").or_else(|| metadata.get("prompt")),
        "local_media": metadata.get("localMedia"),
    })
}

fn workflow_folders(project_directory: &Path) -> Vec<String> {
    let mut values = std::fs::read_dir(project_directory)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.'))
        .collect::<Vec<_>>();
    values.sort();
    values.truncate(40);
    values
}

fn validate_identifier(value: &str) -> Result<(), BridgeError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(BridgeError::invalid("The task id is invalid."));
    }
    Ok(())
}

fn next_request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or_default();
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("mcp-{nanos:x}-{sequence:x}")
}

fn tool_result(value: Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(&value)
        .unwrap_or_else(|_| "{\"error\":\"JSON encoding failed\"}".to_owned());
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": value,
        "isError": is_error
    })
}

fn jsonrpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn jsonrpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn write_response(output: &mut impl Write, value: &Value) -> Result<(), BridgeError> {
    serde_json::to_writer(&mut *output, value)
        .map_err(|_| BridgeError::internal("The MCP response could not be encoded."))?;
    output
        .write_all(b"\n")
        .map_err(|_| BridgeError::internal("The MCP response could not be written."))?;
    output
        .flush()
        .map_err(|_| BridgeError::internal("The MCP response could not be flushed."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_are_deterministic_and_include_the_four_canvas_capabilities() {
        let names = tool_catalog()
            .into_iter()
            .map(|tool| tool["name"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            [
                "canvas_context",
                "canvas_read",
                "canvas_mutate",
                "canvas_task"
            ]
        );
    }

    #[test]
    fn generated_request_ids_are_valid_route_identifiers() {
        let first = next_request_id();
        let second = next_request_id();
        assert_ne!(first, second);
        validate_identifier(&first).unwrap();
    }
}
