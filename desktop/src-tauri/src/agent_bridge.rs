use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    sync::{Arc, Mutex},
};

use local_agent_adapter::{
    BridgeServer, CanvasOperationAdapter, CredentialStore, SqliteCanvasAdapter, BRIDGE_PORT,
};
use serde_json::Value;
use tauri::State;

use crate::runtime::DesktopRuntime;

pub(crate) struct DesktopAgentBridge {
    server: Mutex<Option<BridgeServer>>,
    canvas: Arc<SqliteCanvasAdapter>,
}

impl DesktopAgentBridge {
    pub(crate) fn start(
        app_data_directory: &Path,
        database_path: &Path,
        runtime: Arc<DesktopRuntime>,
    ) -> Result<Self, String> {
        let canvas =
            Arc::new(SqliteCanvasAdapter::open(database_path).map_err(|error| {
                format!("cannot open the shared desktop canvas store: {error}")
            })?);
        let credentials = Arc::new(
            CredentialStore::load_or_create(app_data_directory.join("agent-bridge"))
                .map_err(|error| format!("cannot prepare the local Agent credential: {error}"))?,
        );
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), BRIDGE_PORT);
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
) -> Result<Value, String> {
    bridge
        .canvas
        .save_human_project(project)
        .map_err(|error| error.to_string())
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
