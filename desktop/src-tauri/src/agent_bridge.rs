use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    sync::{Arc, Mutex},
};

use local_agent_adapter::{
    BridgeServer, CanonicalCanvasAdapter, CanvasOperationAdapter, CredentialStore,
    HttpCanvasProtocolExecutor,
};
use serde_json::Value;
use tauri::State;

use crate::runtime::DesktopRuntime;

pub(crate) struct DesktopAgentBridge {
    server: Mutex<Option<BridgeServer>>,
    canvas: Arc<CanonicalCanvasAdapter>,
}

impl DesktopAgentBridge {
    pub(crate) fn start(
        app_data_directory: &Path,
        database_path: &Path,
        runtime: Arc<DesktopRuntime>,
        web_port: u16,
        bridge_port: u16,
    ) -> Result<Self, String> {
        let protocol_endpoint = format!("http://127.0.0.1:{web_port}/internal/canvas-operation");
        let protocol = Arc::new(
            HttpCanvasProtocolExecutor::new(&protocol_endpoint)
                .map_err(|error| format!("cannot configure the shared canvas protocol: {error}"))?,
        );
        let canvas = Arc::new(
            CanonicalCanvasAdapter::open(database_path, protocol)
                .map_err(|error| format!("cannot open the shared desktop canvas store: {error}"))?,
        );
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
) -> Result<u64, String> {
    bridge
        .canvas
        .get_project(&project_id)
        .map(|document| document.revision)
        .map_err(|error| error.to_string())
}
