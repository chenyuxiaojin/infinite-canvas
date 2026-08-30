use std::{
    env,
    ffi::OsStr,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Sender, TryRecvError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
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
const RESOLVE_BRIDGE_SOURCE: &str = include_str!("resolve_probe.py");
const RESOLVE_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const RESOLVE_STDOUT_LIMIT_BYTES: usize = 16 * 1024;
const RESOLVE_STDERR_LIMIT_BYTES: usize = 16 * 1024;
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(5);

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
        if !Path::new(PYTHON_BIN).is_file() {
            return Err(RuntimeFailure::InvalidResponse);
        }

        // This is the only DaVinci command. The executable, script, arguments and
        // environment keys are fixed; paths come only from standard candidates below.
        // The source is embedded at compile time so a packaged app never depends on
        // the build machine's crate checkout.
        let mut child = Command::new(PYTHON_BIN)
            .args(["-c", RESOLVE_BRIDGE_SOURCE])
            .env_clear()
            .env("INFINITE_CANVAS_RESOLVE_MODULE", module)
            .env("RESOLVE_SCRIPT_LIB", library)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(map_io_failure)?;
        let output = run_bounded_child(
            &mut child,
            RESOLVE_PROBE_TIMEOUT,
            RESOLVE_STDOUT_LIMIT_BYTES,
            RESOLVE_STDERR_LIMIT_BYTES,
        )?;
        debug_assert!(output.stderr.len() <= RESOLVE_STDERR_LIMIT_BYTES);

        Ok(ResolveBridgeResponse {
            exit_code: output.status.code(),
            stdout: String::from_utf8(output.stdout)
                .map_err(|_| RuntimeFailure::InvalidResponse)?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CapturedStream {
    Stdout,
    Stderr,
}

#[derive(Debug)]
struct StreamCapture {
    bytes: Vec<u8>,
    exceeded: bool,
}

#[derive(Debug)]
struct BoundedCommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_bounded_child(
    child: &mut Child,
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<BoundedCommandOutput, RuntimeFailure> {
    let stdout = child.stdout.take().ok_or(RuntimeFailure::Other)?;
    let stderr = child.stderr.take().ok_or(RuntimeFailure::Other)?;
    let (limit_sender, limit_receiver) = mpsc::channel();
    let stdout_reader = spawn_bounded_reader(
        stdout,
        stdout_limit,
        CapturedStream::Stdout,
        limit_sender.clone(),
    );
    let stderr_reader =
        spawn_bounded_reader(stderr, stderr_limit, CapturedStream::Stderr, limit_sender);
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(RuntimeFailure::Other)?;
    let mut forced_failure = None;

    let status = loop {
        match limit_receiver.try_recv() {
            Ok(stream) => {
                forced_failure = Some(limit_failure(stream));
                break terminate_and_reap(child)?;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {}
        }

        if let Some(status) = child.try_wait().map_err(map_io_failure)? {
            break status;
        }

        let now = Instant::now();
        if now >= deadline {
            forced_failure = Some(RuntimeFailure::TimedOut);
            break terminate_and_reap(child)?;
        }
        thread::sleep(CHILD_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    };

    let stdout = join_capture(stdout_reader)?;
    let stderr = join_capture(stderr_reader)?;
    if stdout.exceeded {
        return Err(RuntimeFailure::StdoutLimitExceeded);
    }
    if stderr.exceeded {
        return Err(RuntimeFailure::StderrLimitExceeded);
    }
    if let Some(failure) = forced_failure {
        return Err(failure);
    }

    Ok(BoundedCommandOutput {
        status,
        stdout: stdout.bytes,
        stderr: stderr.bytes,
    })
}

fn spawn_bounded_reader<R>(
    mut reader: R,
    limit: usize,
    stream: CapturedStream,
    limit_sender: Sender<CapturedStream>,
) -> JoinHandle<io::Result<StreamCapture>>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut bytes = Vec::with_capacity(limit.min(4096));
        let mut buffer = [0_u8; 4096];
        let mut exceeded = false;
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            let remaining = limit.saturating_sub(bytes.len());
            let retained = remaining.min(count);
            bytes.extend_from_slice(&buffer[..retained]);
            if retained < count && !exceeded {
                exceeded = true;
                let _ = limit_sender.send(stream);
            }
        }
        Ok(StreamCapture { bytes, exceeded })
    })
}

fn join_capture(
    reader: JoinHandle<io::Result<StreamCapture>>,
) -> Result<StreamCapture, RuntimeFailure> {
    reader
        .join()
        .map_err(|_| RuntimeFailure::Other)?
        .map_err(map_io_failure)
}

fn limit_failure(stream: CapturedStream) -> RuntimeFailure {
    match stream {
        CapturedStream::Stdout => RuntimeFailure::StdoutLimitExceeded,
        CapturedStream::Stderr => RuntimeFailure::StderrLimitExceeded,
    }
}

fn terminate_and_reap(child: &mut Child) -> Result<ExitStatus, RuntimeFailure> {
    if let Some(status) = child.try_wait().map_err(map_io_failure)? {
        return Ok(status);
    }
    if let Err(error) = child.kill() {
        if let Some(status) = child.try_wait().map_err(map_io_failure)? {
            return Ok(status);
        }
        return Err(map_io_failure(error));
    }
    child.wait().map_err(map_io_failure)
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

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_TIMEOUT: Duration = Duration::from_secs(1);
    const TEST_LIMIT: usize = 128;

    fn python_child(code: &str) -> Child {
        Command::new(PYTHON_BIN)
            .args(["-c", code])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("test Python child should start")
    }

    #[test]
    fn bounded_child_collects_normal_stdout_and_stderr() {
        let mut child =
            python_child("import sys; print('normal'); print('diagnostic', file=sys.stderr)");
        let output = run_bounded_child(&mut child, TEST_TIMEOUT, TEST_LIMIT, TEST_LIMIT)
            .expect("normal child should succeed");

        assert!(output.status.success());
        assert_eq!(output.stdout, b"normal\n");
        assert_eq!(output.stderr, b"diagnostic\n");
        assert!(child.try_wait().expect("child state should read").is_some());
    }

    #[test]
    fn bounded_child_times_out_then_terminates_and_reaps() {
        let mut child = python_child("import time; time.sleep(5)");
        let started = Instant::now();
        let failure = run_bounded_child(
            &mut child,
            Duration::from_millis(50),
            TEST_LIMIT,
            TEST_LIMIT,
        )
        .expect_err("sleeping child should time out");

        assert_eq!(failure, RuntimeFailure::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(child.try_wait().expect("child state should read").is_some());
    }

    #[test]
    fn bounded_child_rejects_excessive_stdout_and_reaps() {
        let mut child =
            python_child("import sys; sys.stdout.write('x' * 65536); sys.stdout.flush()");
        let failure = run_bounded_child(&mut child, TEST_TIMEOUT, TEST_LIMIT, TEST_LIMIT)
            .expect_err("oversized stdout should fail");

        assert_eq!(failure, RuntimeFailure::StdoutLimitExceeded);
        assert!(child.try_wait().expect("child state should read").is_some());
    }

    #[test]
    fn bounded_child_rejects_excessive_stderr_and_reaps() {
        let mut child =
            python_child("import sys; sys.stderr.write('x' * 65536); sys.stderr.flush()");
        let failure = run_bounded_child(&mut child, TEST_TIMEOUT, TEST_LIMIT, TEST_LIMIT)
            .expect_err("oversized stderr should fail");

        assert_eq!(failure, RuntimeFailure::StderrLimitExceeded);
        assert!(child.try_wait().expect("child state should read").is_some());
    }

    #[test]
    fn embedded_resolve_bridge_contains_only_the_fixed_read_contract() {
        for method in [
            "scriptapp(\"Resolve\")",
            "GetVersionString()",
            "GetProjectManager()",
            "GetCurrentProject()",
            "GetCurrentTimeline()",
        ] {
            assert!(RESOLVE_BRIDGE_SOURCE.contains(method));
        }
        for denied in ["SaveProject", "StartRendering", "AddItemListToMediaPool"] {
            assert!(!RESOLVE_BRIDGE_SOURCE.contains(denied));
        }
    }
}
