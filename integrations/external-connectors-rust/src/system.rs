use std::{
    env,
    ffi::OsStr,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use crate::{
    DaVinciRuntime, EagleReadEndpoint, EagleRuntime, HttpReadResponse, ProcessState,
    ResolveBridgeResponse, ResolveDiscovery, ResolveEnvironmentFacts, ResolveEnvironmentStatus,
    RuntimeFailure,
};

const EAGLE_LIBRARY_INFO_URL: &str = "http://127.0.0.1:41595/api/v2/library/info";
const MAX_EAGLE_RESPONSE_BYTES: u64 = 64 * 1024;
const PGRP_BIN: &str = "/usr/bin/pgrep";
const PYTHON_BIN: &str = "/usr/bin/python3";
const RESOLVE_BRIDGE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/resolve_probe.py");

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemEagleRuntime;

impl EagleRuntime for SystemEagleRuntime {
    fn application_installed(&self) -> bool {
        eagle_application_candidates()
            .iter()
            .any(|path| path.is_dir())
    }

    fn process_state(&self) -> Result<ProcessState, RuntimeFailure> {
        fixed_process_check(AllowedProcess::Eagle)
    }

    fn get(&self, endpoint: EagleReadEndpoint) -> Result<HttpReadResponse, RuntimeFailure> {
        let url = match endpoint {
            EagleReadEndpoint::LibraryInfo => EAGLE_LIBRARY_INFO_URL,
        };

        let request = ureq::get(url).timeout(Duration::from_millis(800));
        match request.call() {
            Ok(response) => read_http_response(response.status(), response),
            Err(ureq::Error::Status(status, response)) => read_http_response(status, response),
            Err(ureq::Error::Transport(_)) => Err(RuntimeFailure::Unreachable),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemDaVinciRuntime;

impl DaVinciRuntime for SystemDaVinciRuntime {
    fn discover(&self) -> ResolveDiscovery {
        let paths = resolve_paths();
        ResolveDiscovery {
            installed: paths.application.is_some(),
            scripting_module_found: paths.module.is_some(),
            scripting_library_found: paths.library.is_some(),
            environment: resolve_environment_status(),
        }
    }

    fn process_state(&self) -> Result<ProcessState, RuntimeFailure> {
        fixed_process_check(AllowedProcess::Resolve)
    }

    fn run_read_only_bridge(&self) -> Result<ResolveBridgeResponse, RuntimeFailure> {
        let paths = resolve_paths();
        let module = paths.module.ok_or(RuntimeFailure::InvalidResponse)?;
        let library = paths.library.ok_or(RuntimeFailure::InvalidResponse)?;
        if !Path::new(PYTHON_BIN).is_file() || !Path::new(RESOLVE_BRIDGE).is_file() {
            return Err(RuntimeFailure::InvalidResponse);
        }

        // This is the only DaVinci command. The executable, script, arguments and
        // environment keys are fixed; paths come only from standard candidates below.
        let output = Command::new(PYTHON_BIN)
            .arg(RESOLVE_BRIDGE)
            .env_clear()
            .env("INFINITE_CANVAS_RESOLVE_MODULE", module)
            .env("RESOLVE_SCRIPT_LIB", library)
            .output()
            .map_err(map_io_failure)?;

        Ok(ResolveBridgeResponse {
            exit_code: output.status.code(),
            stdout: String::from_utf8(output.stdout)
                .map_err(|_| RuntimeFailure::InvalidResponse)?,
        })
    }
}

fn read_http_response(
    status: u16,
    response: ureq::Response,
) -> Result<HttpReadResponse, RuntimeFailure> {
    let mut body = String::new();
    response
        .into_reader()
        .take(MAX_EAGLE_RESPONSE_BYTES)
        .read_to_string(&mut body)
        .map_err(map_io_failure)?;
    Ok(HttpReadResponse { status, body })
}

#[derive(Debug, Clone, Copy)]
enum AllowedProcess {
    Eagle,
    Resolve,
}

impl AllowedProcess {
    fn name(self) -> &'static OsStr {
        match self {
            Self::Eagle => OsStr::new("Eagle"),
            Self::Resolve => OsStr::new("Resolve"),
        }
    }
}

fn fixed_process_check(process: AllowedProcess) -> Result<ProcessState, RuntimeFailure> {
    let output = Command::new(PGRP_BIN)
        .args([OsStr::new("-x"), process.name()])
        .output()
        .map_err(map_io_failure)?;
    match output.status.code() {
        Some(0) => Ok(ProcessState::Running),
        Some(1) => Ok(ProcessState::NotRunning),
        _ if contains_permission_error(&output.stderr) => Err(RuntimeFailure::PermissionDenied),
        _ => Err(RuntimeFailure::Other),
    }
}

fn contains_permission_error(stderr: &[u8]) -> bool {
    let lowercase = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    lowercase.contains("permission") || lowercase.contains("operation not permitted")
}

fn map_io_failure(error: std::io::Error) -> RuntimeFailure {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => RuntimeFailure::PermissionDenied,
        std::io::ErrorKind::ConnectionRefused
        | std::io::ErrorKind::ConnectionReset
        | std::io::ErrorKind::ConnectionAborted
        | std::io::ErrorKind::NotConnected
        | std::io::ErrorKind::TimedOut => RuntimeFailure::Unreachable,
        _ => RuntimeFailure::Other,
    }
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn eagle_application_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/Applications/Eagle.app"),
        PathBuf::from("/Applications/Eagle/Eagle.app"),
    ];
    if let Some(home) = home_dir() {
        candidates.push(home.join("Applications/Eagle.app"));
    }
    candidates
}

#[derive(Debug)]
struct ResolvePaths {
    application: Option<PathBuf>,
    module: Option<PathBuf>,
    library: Option<PathBuf>,
}

fn resolve_paths() -> ResolvePaths {
    let applications = resolve_application_candidates();
    let application = applications.iter().find(|path| path.is_dir()).cloned();
    let module = resolve_module_candidates(&applications)
        .into_iter()
        .find(|path| path.is_file());
    let library = resolve_library_candidates(&applications)
        .into_iter()
        .find(|path| path.is_file());
    ResolvePaths {
        application,
        module,
        library,
    }
}

fn resolve_application_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/Applications/DaVinci Resolve/DaVinci Resolve.app"),
        PathBuf::from("/Applications/DaVinci Resolve.app"),
        PathBuf::from("/Applications/DaVinci Resolve Studio.app"),
    ];
    if let Some(home) = home_dir() {
        candidates.extend([
            home.join("Applications/DaVinci Resolve/DaVinci Resolve.app"),
            home.join("Applications/DaVinci Resolve.app"),
            home.join("Applications/DaVinci Resolve Studio.app"),
        ]);
    }
    candidates
}

fn resolve_module_candidates(applications: &[PathBuf]) -> Vec<PathBuf> {
    let mut candidates = vec![PathBuf::from(
        "/Library/Application Support/Blackmagic Design/DaVinci Resolve/Developer/Scripting/Modules/DaVinciResolveScript.py",
    )];
    if let Some(home) = home_dir() {
        candidates.push(home.join(
            "Library/Application Support/Blackmagic Design/DaVinci Resolve/Developer/Scripting/Modules/DaVinciResolveScript.py",
        ));
    }
    candidates.extend(applications.iter().map(|application| {
        application.join("Contents/Libraries/Fusion/Modules/DaVinciResolveScript.py")
    }));
    candidates
}

fn resolve_library_candidates(applications: &[PathBuf]) -> Vec<PathBuf> {
    applications
        .iter()
        .map(|application| application.join("Contents/Libraries/Fusion/fusionscript.so"))
        .collect()
}

fn resolve_environment_status() -> ResolveEnvironmentFacts {
    ResolveEnvironmentFacts {
        resolve_script_api: environment_path_status("RESOLVE_SCRIPT_API"),
        resolve_script_lib: environment_path_status("RESOLVE_SCRIPT_LIB"),
        pythonpath: environment_path_status("PYTHONPATH"),
    }
}

fn environment_path_status(key: &str) -> ResolveEnvironmentStatus {
    match env::var(key).ok().filter(|value| !value.is_empty()) {
        None => ResolveEnvironmentStatus::Missing,
        Some(value) if standard_resolve_environment_value(key, &value) => {
            ResolveEnvironmentStatus::StandardPath
        }
        Some(_) => ResolveEnvironmentStatus::NonstandardPathIgnored,
    }
}

fn standard_resolve_environment_value(key: &str, value: &str) -> bool {
    let normalized = value.replace('\\', "/");
    match key {
        "RESOLVE_SCRIPT_API" => normalized.ends_with(
            "/Library/Application Support/Blackmagic Design/DaVinci Resolve/Developer/Scripting",
        ) || normalized
            == "/Library/Application Support/Blackmagic Design/DaVinci Resolve/Developer/Scripting",
        "RESOLVE_SCRIPT_LIB" => normalized.ends_with(
            "/DaVinci Resolve.app/Contents/Libraries/Fusion/fusionscript.so",
        ) || normalized.ends_with(
            "/DaVinci Resolve Studio.app/Contents/Libraries/Fusion/fusionscript.so",
        ),
        "PYTHONPATH" => normalized.split(':').any(|entry| {
            entry.ends_with(
                "/Library/Application Support/Blackmagic Design/DaVinci Resolve/Developer/Scripting/Modules",
            )
        }),
        _ => false,
    }
}
