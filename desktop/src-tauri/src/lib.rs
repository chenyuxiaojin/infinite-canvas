use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::Path,
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use tauri::{App, AppHandle, Manager, RunEvent, Runtime, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_shell::{process::CommandChild, ShellExt};

mod runtime;

use runtime::DesktopRuntime;

const WEB_PORT: u16 = 3100;
const API_PORT: u16 = 3101;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Sidecars(Mutex<Vec<CommandChild>>);

fn loopback_address(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)
}

fn ensure_ports_available() -> Result<(), String> {
    for port in [WEB_PORT, API_PORT] {
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
    if let Some(runtime) = app.try_state::<DesktopRuntime>() {
        runtime.shutdown();
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
    app.manage(DesktopRuntime::initialize(&app_data_dir));
    if !web_dir.join("server.js").is_file() {
        return Err(format!(
            "packaged Next.js server is missing at {}",
            web_dir.display()
        ));
    }

    let database = database_path.to_string_lossy();
    let logs = log_dir.to_string_lossy();
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

    spawn_sidecar(
        app.handle(),
        "node",
        &web_dir,
        &["server.js"],
        &[
            ("API_BASE_URL", "http://127.0.0.1:3101"),
            ("HOSTNAME", "127.0.0.1"),
            ("NODE_ENV", "production"),
            ("PORT", &WEB_PORT.to_string()),
        ],
    )?;
    wait_for_port(WEB_PORT)?;

    WebviewWindowBuilder::new(
        app,
        "main",
        WebviewUrl::External("http://127.0.0.1:3100".parse().unwrap()),
    )
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
        .plugin(tauri_plugin_shell::init())
        .manage(Sidecars::default())
        .invoke_handler(tauri::generate_handler![
            runtime::probe_desktop_runtime,
            runtime::generate_desktop_test_clip,
            runtime::desktop_task_status,
            runtime::cancel_desktop_task,
        ])
        .setup(|app| {
            if let Err(error) = start_desktop(app) {
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
            &"http://127.0.0.1:3100/canvas/test".parse().unwrap()
        ));
        assert!(!is_canvas_url(
            &"http://localhost:3100/canvas/test".parse().unwrap()
        ));
        assert!(!is_canvas_url(
            &"http://127.0.0.1:3101/api/health".parse().unwrap()
        ));
        assert!(!is_canvas_url(&"https://example.com".parse().unwrap()));
    }
}
