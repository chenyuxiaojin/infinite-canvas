use std::{
    fs::OpenOptions,
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use tauri::{App, AppHandle, Manager, RunEvent, Runtime, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tauri_plugin_shell::{process::CommandChild, ShellExt};

mod agent_bridge;
mod local_media;
mod paid_generation;
mod runtime;

use agent_bridge::DesktopAgentBridge;
use local_media::LocalMediaManager;
use runtime::DesktopRuntime;

#[cfg(not(feature = "integration-acceptance"))]
const WEB_PORT: u16 = 3100;
#[cfg(feature = "integration-acceptance")]
const WEB_PORT: u16 = 3210;
#[cfg(not(feature = "integration-acceptance"))]
const API_PORT: u16 = 3101;
#[cfg(feature = "integration-acceptance")]
const API_PORT: u16 = 3211;
#[cfg(not(feature = "integration-acceptance"))]
const AGENT_BRIDGE_PORT: u16 = 3102;
#[cfg(feature = "integration-acceptance")]
const AGENT_BRIDGE_PORT: u16 = 3212;
#[cfg(not(feature = "integration-acceptance"))]
const LOCAL_MEDIA_PORT: u16 = 3103;
#[cfg(feature = "integration-acceptance")]
const LOCAL_MEDIA_PORT: u16 = 3213;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CANVAS_EXPORT_BYTES: usize = 2 * 1024 * 1024 * 1024;

#[derive(Default)]
struct Sidecars(Mutex<Vec<CommandChild>>);

#[derive(serde::Serialize)]
struct SaveCanvasExportResult {
    saved: bool,
    file_name: Option<String>,
    bytes: usize,
}

fn unique_export_staging_path(target: &Path) -> Result<PathBuf, String> {
    let parent = target
        .parent()
        .ok_or_else(|| "the selected export path has no parent directory".to_string())?;
    let file_name = target
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "the selected export file name is not valid UTF-8".to_string())?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("cannot create export nonce: {error}"))?
        .as_nanos();
    Ok(parent.join(format!(".{file_name}.part-{}-{nonce}", std::process::id())))
}

fn write_new_canvas_export(target: &Path, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 4 || !bytes.starts_with(b"PK\x03\x04") {
        return Err("canvas export payload is not a ZIP archive".to_string());
    }
    if target
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("zip"))
        != Some(true)
    {
        return Err("canvas exports must use the .zip extension".to_string());
    }
    if target.exists() {
        return Err("the selected export path already exists; choose a new file name".to_string());
    }

    let staging = unique_export_staging_path(target)?;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staging)
            .map_err(|error| format!("cannot create export staging file: {error}"))?;
        file.write_all(bytes)
            .map_err(|error| format!("cannot write canvas export: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("cannot sync canvas export: {error}"))?;
        std::fs::hard_link(&staging, target).map_err(|error| {
            format!("cannot publish canvas export without overwriting an existing file: {error}")
        })?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&staging);
    result
}

#[tauri::command]
async fn save_canvas_export(
    app: AppHandle,
    local_media: tauri::State<'_, Arc<LocalMediaManager>>,
    request: tauri::ipc::Request<'_>,
) -> Result<SaveCanvasExportResult, String> {
    let bytes = match request.body() {
        tauri::ipc::InvokeBody::Raw(bytes) => bytes,
        tauri::ipc::InvokeBody::Json(_) => {
            return Err("canvas export requires a raw binary IPC payload".to_string())
        }
    };
    if bytes.is_empty() || bytes.len() > MAX_CANVAS_EXPORT_BYTES {
        return Err(format!(
            "canvas export size must be between 1 byte and {MAX_CANVAS_EXPORT_BYTES} bytes"
        ));
    }

    let selected = app
        .dialog()
        .file()
        .set_title("导出无限画布项目")
        .set_file_name("infinite-canvas-export.zip")
        .add_filter("ZIP archive", &["zip"])
        .blocking_save_file();
    let Some(selected) = selected else {
        return Ok(SaveCanvasExportResult {
            saved: false,
            file_name: None,
            bytes: 0,
        });
    };
    let target = match selected {
        FilePath::Path(path) => path,
        FilePath::Url(_) => return Err("URL export paths are not supported on macOS".to_string()),
    };
    let saved_bytes = if let Some((envelope, base_zip)) = local_media::parse_export_envelope(bytes)?
    {
        local_media.export_archive(&target, base_zip, envelope)? as usize
    } else {
        write_new_canvas_export(&target, bytes)?;
        bytes.len()
    };
    Ok(SaveCanvasExportResult {
        saved: true,
        file_name: target
            .file_name()
            .and_then(|value| value.to_str())
            .map(ToOwned::to_owned),
        bytes: saved_bytes,
    })
}

fn loopback_address(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

fn ensure_ports_available() -> Result<(), String> {
    for port in [WEB_PORT, API_PORT, AGENT_BRIDGE_PORT, LOCAL_MEDIA_PORT] {
        TcpListener::bind(loopback_address(port))
            .map_err(|error| format!("local port {port} is unavailable: {error}"))?;
    }
    Ok(())
}

fn wait_for_port(port: u16) -> Result<(), String> {
    let deadline = Instant::now() + STARTUP_TIMEOUT;
    let address = loopback_address(port);
    while Instant::now() < deadline {
        if TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!(
        "timed out waiting for local service on 127.0.0.1:{port}"
    ))
}

fn remember_sidecar<R: Runtime>(app: &AppHandle<R>, child: CommandChild) {
    app.state::<Sidecars>().0.lock().unwrap().push(child);
}

fn stop_sidecars<R: Runtime>(app: &AppHandle<R>) {
    let sidecars = app.state::<Sidecars>();
    let mut children = sidecars.0.lock().unwrap();
    for child in children.drain(..).rev() {
        let _ = child.kill();
    }
}

fn stop_desktop_runtime<R: Runtime>(app: &AppHandle<R>) {
    if let Some(runtime) = app.try_state::<Arc<DesktopRuntime>>() {
        runtime.shutdown();
    }
}

fn stop_agent_bridge<R: Runtime>(app: &AppHandle<R>) {
    if let Some(bridge) = app.try_state::<DesktopAgentBridge>() {
        bridge.stop();
    }
}

fn stop_local_media<R: Runtime>(app: &AppHandle<R>) {
    if let Some(manager) = app.try_state::<Arc<LocalMediaManager>>() {
        manager.shutdown();
    }
}

fn spawn_sidecar<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    current_dir: &Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<(), String> {
    let mut command = app
        .shell()
        .sidecar(name)
        .map_err(|error| format!("cannot prepare {name} sidecar: {error}"))?
        .current_dir(current_dir)
        .args(args);
    for (key, value) in envs {
        command = command.env(key, value);
    }
    let (mut events, child) = command
        .spawn()
        .map_err(|error| format!("cannot start {name} sidecar: {error}"))?;
    remember_sidecar(app, child);
    tauri::async_runtime::spawn(async move { while events.recv().await.is_some() {} });
    Ok(())
}

fn is_canvas_url(url: &tauri::Url) -> bool {
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port_or_known_default() == Some(WEB_PORT)
}

fn start_desktop(app: &mut App) -> Result<(), String> {
    ensure_ports_available()?;

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("cannot resolve app data directory: {error}"))?;
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|error| format!("cannot resolve app resource directory: {error}"))?;
    let web_dir = resource_dir.join("web");
    let database_path = app_data_dir.join("infinite-canvas.db");
    let log_dir = app_data_dir.join("logs").join("ai-calls");

    std::fs::create_dir_all(&app_data_dir)
        .map_err(|error| format!("cannot create app data directory: {error}"))?;
    std::fs::create_dir_all(&log_dir)
        .map_err(|error| format!("cannot create app log directory: {error}"))?;
    let local_media = LocalMediaManager::start(&app_data_dir, LOCAL_MEDIA_PORT, WEB_PORT)?;
    app.manage(local_media.clone());
    let desktop_runtime = Arc::new(DesktopRuntime::initialize(&app_data_dir, local_media));
    app.manage(desktop_runtime.clone());
    if !web_dir.join("server.js").is_file() {
        return Err(format!(
            "packaged Next.js server is missing at {}",
            web_dir.display()
        ));
    }

    let database = database_path.to_string_lossy();
    let logs = log_dir.to_string_lossy();
    let api_base_url = format!("http://127.0.0.1:{API_PORT}");
    spawn_sidecar(
        app.handle(),
        "infinite-canvas-api",
        &app_data_dir,
        &[],
        &[
            ("AI_LOG_DIR", logs.as_ref()),
            ("BIND_HOST", "127.0.0.1"),
            ("DATABASE_DSN", database.as_ref()),
            ("PORT", &API_PORT.to_string()),
            ("STORAGE_DRIVER", "sqlite"),
        ],
    )?;
    wait_for_port(API_PORT)?;

    let agent_bridge = DesktopAgentBridge::start(
        &app_data_dir,
        &database_path,
        desktop_runtime,
        WEB_PORT,
        AGENT_BRIDGE_PORT,
    )?;
    app.manage(agent_bridge);

    spawn_sidecar(
        app.handle(),
        "node",
        &web_dir,
        &["server.js"],
        &[
            ("API_BASE_URL", api_base_url.as_str()),
            ("HOSTNAME", "127.0.0.1"),
            ("NODE_ENV", "production"),
            ("PORT", &WEB_PORT.to_string()),
        ],
    )?;
    wait_for_port(WEB_PORT)?;

    let web_url = format!("http://127.0.0.1:{WEB_PORT}")
        .parse()
        .map_err(|error| format!("cannot prepare desktop canvas URL: {error}"))?;
    WebviewWindowBuilder::new(app, "main", WebviewUrl::External(web_url))
        .title("无限画布")
        .inner_size(1440.0, 900.0)
        .min_inner_size(1100.0, 700.0)
        .resizable(true)
        .on_navigation(is_canvas_url)
        .build()
        .map_err(|error| format!("cannot create desktop window: {error}"))?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(Sidecars::default())
        .invoke_handler(tauri::generate_handler![
            runtime::probe_desktop_runtime,
            runtime::generate_desktop_test_clip,
            runtime::generate_canvas_test_clip,
            runtime::desktop_task_status,
            runtime::desktop_task_media_reference,
            runtime::cancel_desktop_task,
            local_media::select_local_media,
            local_media::resolve_local_media_reference,
            local_media::relink_local_media_reference,
            local_media::local_media_request_evidence,
            local_media::import_canvas_archive,
            paid_generation::approve_paid_generation,
            paid_generation::reject_paid_generation,
            agent_bridge::desktop_canvas_projects,
            agent_bridge::save_desktop_canvas_project,
            agent_bridge::delete_desktop_canvas_projects,
            agent_bridge::desktop_canvas_project_revision,
            save_canvas_export,
        ])
        .setup(|app| {
            if let Err(error) = start_desktop(app) {
                stop_agent_bridge(app.handle());
                stop_local_media(app.handle());
                stop_desktop_runtime(app.handle());
                stop_sidecars(app.handle());
                eprintln!("desktop startup failed: {error}");
                std::process::exit(1);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building the Tauri application")
        .run(|app, event| {
            if matches!(event, RunEvent::ExitRequested { .. } | RunEvent::Exit) {
                stop_agent_bridge(app);
                stop_local_media(app);
                stop_desktop_runtime(app);
                stop_sidecars(app);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_is_limited_to_the_fixed_loopback_origin() {
        assert!(is_canvas_url(
            &format!("http://127.0.0.1:{WEB_PORT}/canvas/test")
                .parse()
                .unwrap()
        ));
        assert!(!is_canvas_url(
            &format!("http://localhost:{WEB_PORT}/canvas/test")
                .parse()
                .unwrap()
        ));
        assert!(!is_canvas_url(
            &format!("http://127.0.0.1:{API_PORT}/api/health")
                .parse()
                .unwrap()
        ));
        assert!(!is_canvas_url(&"https://example.com".parse().unwrap()));
    }

    #[test]
    fn canvas_export_is_published_once_without_overwrite() {
        let root = std::env::temp_dir().join(format!(
            "infinite-canvas-export-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("project.zip");
        let payload = b"PK\x03\x04deterministic-test";

        write_new_canvas_export(&target, payload).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), payload);
        let error = write_new_canvas_export(&target, b"PK\x03\x04replacement").unwrap_err();
        assert!(error.contains("already exists"));
        assert_eq!(std::fs::read(&target).unwrap(), payload);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn canvas_export_rejects_non_zip_payloads_and_extensions() {
        let root = std::env::temp_dir().join(format!(
            "infinite-canvas-export-validation-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();

        assert!(write_new_canvas_export(&root.join("project.zip"), b"not-a-zip").is_err());
        assert!(write_new_canvas_export(&root.join("project.txt"), b"PK\x03\x04zip").is_err());
        assert!(std::fs::read_dir(&root).unwrap().next().is_none());

        std::fs::remove_dir_all(root).unwrap();
    }
}
