use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    sync::{Arc, Mutex},
};

use local_agent_adapter::{
    BridgeServer, CanvasOperationAdapter, CredentialStore, SqliteCanvasAdapter, HttpCanvasProtocolExecutor,
};
use serde_json::Value;
use tauri::State;

use crate::runtime::DesktopRuntime;

pub(crate) struct DesktopAgentBridge {
    server: Mutex<Option<BridgeServer>>,
    canvas: Arc<SqliteCanvasAdapter>,
}

impl DesktopAgentBridge {
    pub(crate) fn canvas(&self) -> Arc<SqliteCanvasAdapter> {
        self.canvas.clone()
    }

    pub(crate) fn start(
        app_data_directory: &Path,
        database_path: &Path,
        runtime: Arc<DesktopRuntime>,
        web_port: u16,
        bridge_port: u16,
    ) -> Result<Self, String> {
        let canvas =
            Arc::new(SqliteCanvasAdapter::open_with_protocol(database_path, Arc::new(HttpCanvasProtocolExecutor::new(&format!("http://127.0.0.1:{web_port}/internal/canvas-operation")).map_err(|e| e.to_string())?)).map_err(|error| {
                format!("cannot open the shared desktop canvas store: {error}")
            })?);
        let credentials = Arc::new(
            CredentialStore::load_or_create(app_data_directory.join("agent-bridge"))
                .map_err(|error| format!("cannot prepare the local Agent credential: {error}"))?,
        );
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), bridge_port);
        let server = BridgeServer::start(address, credentials, canvas.clone(), runtime)
            .map_err(|error| format!("cannot start the local Agent Bridge: {error}"))?;
        Ok(Self {
            server: Mutex::new(Some(server)),
            canvas,
        })
    }

    pub(crate) fn stop(&self) {
        if let Ok(mut server) = self.server.lock() {
            if let Some(mut server) = server.take() {
                server.stop();
            }
        }
    }
}

#[tauri::command]
pub(crate) fn desktop_canvas_projects(
    bridge: State<'_, DesktopAgentBridge>,
) -> Result<Vec<Value>, String> {
    bridge
        .canvas
        .list_project_documents()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn desktop_canvas_project_ids(
    bridge: State<'_, DesktopAgentBridge>,
) -> Result<Vec<String>, String> {
    bridge
        .canvas
        .list_projects()
        .map(|projects| projects.into_iter().map(|project| project.id).collect())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn desktop_canvas_project(
    bridge: State<'_, DesktopAgentBridge>,
    project_id: String,
) -> Result<Value, String> {
    bridge
        .canvas
        .get_project(&project_id)
        .map(|document| document.project)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn save_desktop_canvas_project(
    bridge: State<'_, DesktopAgentBridge>,
    project: Value,
    expected_revision: Option<String>,
) -> Result<local_agent_adapter::ProjectDocument, String> {
    let id = project["id"].as_str().ok_or("缺少画布 ID")?.to_owned();
    bridge.canvas.save_human_project_checked(project, expected_revision.as_deref()).map_err(|error| error.to_string())?;
    bridge.canvas.get_project(&id).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn delete_desktop_canvas_projects(
    bridge: State<'_, DesktopAgentBridge>,
    project_ids: Vec<String>,
) -> Result<usize, String> {
    bridge
        .canvas
        .delete_human_projects(&project_ids)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn desktop_canvas_project_revision(
    bridge: State<'_, DesktopAgentBridge>,
    project_id: String,
) -> Result<String, String> {
    bridge
        .canvas
        .get_project(&project_id)
        .map(|document| document.revision)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn desktop_canvas_deleted_ids(bridge: State<'_, DesktopAgentBridge>) -> Result<Vec<String>, String> {
    bridge.canvas.deleted_project_ids().map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn desktop_canvas_history(bridge: State<'_, DesktopAgentBridge>, project_id: String) -> Result<Value, String> {
    bridge.canvas.history_list(&project_id).map_err(|e|e.to_string())
}
#[tauri::command]
pub(crate) fn desktop_canvas_history_preview(bridge: State<'_, DesktopAgentBridge>, project_id: String, sequence: i64) -> Result<Value, String> {
    bridge.canvas.history_preview(&project_id,sequence).map_err(|e|e.to_string())
}
#[tauri::command]
pub(crate) fn desktop_canvas_history_restore(bridge: State<'_, DesktopAgentBridge>, project_id: String, sequence: i64, expected_revision: String, request_id: String) -> Result<Value, String> {
    bridge.canvas.history_restore(&project_id,sequence,&expected_revision,&request_id).map_err(|e|e.to_string())
}

#[tauri::command]
pub(crate) fn desktop_canvas_document(bridge: State<'_, DesktopAgentBridge>, project_id: String) -> Result<local_agent_adapter::ProjectDocument, String> {
    bridge.canvas.get_project(&project_id).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn desktop_canvas_project_updated_at(
    bridge: State<'_, DesktopAgentBridge>,
    project_id: String,
) -> Result<String, String> {
    bridge
        .canvas
        .project_updated_at(&project_id)
        .map_err(|error| error.to_string())
}
