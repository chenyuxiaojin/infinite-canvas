use infinite_canvas_external_connectors::{
    DaVinciProvider, DaVinciRuntime, EagleProvider, EagleReadEndpoint, EagleRuntime,
    HttpReadResponse, ProcessState, ProviderFacts, ProviderStatus, ResolveBridgeResponse,
    ResolveDiscovery, ResolveEnvironmentFacts, ResolveEnvironmentStatus, RuntimeFailure,
};
use pretty_assertions::assert_eq;

#[derive(Clone)]
struct MockEagleRuntime {
    installed: bool,
    process: Result<ProcessState, RuntimeFailure>,
    response: Result<HttpReadResponse, RuntimeFailure>,
}

impl EagleRuntime for MockEagleRuntime {
    fn application_installed(&self) -> bool {
        self.installed
    }

    fn process_state(&self) -> Result<ProcessState, RuntimeFailure> {
        self.process
    }

    fn get(&self, endpoint: EagleReadEndpoint) -> Result<HttpReadResponse, RuntimeFailure> {
        assert_eq!(endpoint, EagleReadEndpoint::LibraryInfo);
        self.response.clone()
    }
}

#[derive(Clone)]
struct MockDaVinciRuntime {
    discovery: ResolveDiscovery,
    process: Result<ProcessState, RuntimeFailure>,
    bridge: Result<ResolveBridgeResponse, RuntimeFailure>,
}

impl DaVinciRuntime for MockDaVinciRuntime {
    fn discover(&self) -> ResolveDiscovery {
        self.discovery.clone()
    }

    fn process_state(&self) -> Result<ProcessState, RuntimeFailure> {
        self.process
    }

    fn run_read_only_bridge(&self) -> Result<ResolveBridgeResponse, RuntimeFailure> {
        self.bridge.clone()
    }
}

fn eagle(
    installed: bool,
    process: Result<ProcessState, RuntimeFailure>,
    response: Result<HttpReadResponse, RuntimeFailure>,
) -> MockEagleRuntime {
    MockEagleRuntime {
        installed,
        process,
        response,
    }
}

fn davinci(
    installed: bool,
    process: Result<ProcessState, RuntimeFailure>,
    module: bool,
    library: bool,
    bridge: Result<ResolveBridgeResponse, RuntimeFailure>,
) -> MockDaVinciRuntime {
    MockDaVinciRuntime {
        discovery: ResolveDiscovery {
            installed,
            scripting_module_found: module,
            scripting_library_found: library,
            environment: ResolveEnvironmentFacts {
                resolve_script_api: ResolveEnvironmentStatus::Missing,
                resolve_script_lib: ResolveEnvironmentStatus::Missing,
                pythonpath: ResolveEnvironmentStatus::Missing,
            },
        },
        process,
        bridge,
    }
}

fn http(status: u16, body: &str) -> Result<HttpReadResponse, RuntimeFailure> {
    Ok(HttpReadResponse {
        status,
        body: body.to_owned(),
    })
}

fn bridge(exit_code: i32, stdout: &str) -> Result<ResolveBridgeResponse, RuntimeFailure> {
    Ok(ResolveBridgeResponse {
        exit_code: Some(exit_code),
        stdout: stdout.to_owned(),
    })
}

#[test]
fn eagle_reports_not_installed_without_fabricating_availability() {
    let report = EagleProvider::new(eagle(
        false,
        Ok(ProcessState::NotRunning),
        Err(RuntimeFailure::Unreachable),
    ))
    .probe();
    assert_eq!(report.status, ProviderStatus::NotInstalled);
}

#[test]
fn eagle_reports_not_running_when_bundle_exists() {
    let report = EagleProvider::new(eagle(
        true,
        Ok(ProcessState::NotRunning),
        Err(RuntimeFailure::Unreachable),
    ))
    .probe();
    assert_eq!(report.status, ProviderStatus::NotRunning);
}

#[test]
fn eagle_does_not_accept_api_success_when_process_is_not_running() {
    let report = EagleProvider::new(eagle(
        true,
        Ok(ProcessState::NotRunning),
        http(200, include_str!("fixtures/eagle/library-info-ok.json")),
    ))
    .probe();
    assert_eq!(report.status, ProviderStatus::NotRunning);
}

#[test]
fn eagle_reports_permission_missing_for_forbidden_read() {
    let report =
        EagleProvider::new(eagle(true, Ok(ProcessState::Running), http(403, "{}"))).probe();
    assert_eq!(report.status, ProviderStatus::PermissionMissing);
}

#[test]
fn eagle_reports_incompatible_for_abnormal_json() {
    let report = EagleProvider::new(eagle(
        true,
        Ok(ProcessState::Running),
        http(200, include_str!("fixtures/eagle/malformed.json")),
    ))
    .probe();
    assert_eq!(report.status, ProviderStatus::Incompatible);
}

#[test]
fn eagle_reports_error_for_unexpected_http_status() {
    let report =
        EagleProvider::new(eagle(true, Ok(ProcessState::Running), http(500, "{}"))).probe();
    assert_eq!(report.status, ProviderStatus::Error);
}

#[test]
fn eagle_available_report_drops_library_name_and_path() {
    let report = EagleProvider::new(eagle(
        true,
        Ok(ProcessState::Running),
        http(200, include_str!("fixtures/eagle/library-info-ok.json")),
    ))
    .probe();
    assert_eq!(report.status, ProviderStatus::Available);
    assert!(matches!(
        report.facts,
        ProviderFacts::Eagle(ref facts) if facts.library_loaded
    ));
    let serialized = serde_json::to_string(&report).unwrap();
    assert!(!serialized.contains("fixture-secret"));
    assert!(!serialized.contains("/fixture/private"));
}

#[test]
fn davinci_reports_not_installed_before_running_any_bridge() {
    let report = DaVinciProvider::new(davinci(
        false,
        Ok(ProcessState::NotRunning),
        false,
        false,
        Err(RuntimeFailure::Other),
    ))
    .probe();
    assert_eq!(report.status, ProviderStatus::NotInstalled);
}

#[test]
fn davinci_reports_not_running_when_bundle_exists() {
    let report = DaVinciProvider::new(davinci(
        true,
        Ok(ProcessState::NotRunning),
        true,
        true,
        Err(RuntimeFailure::Other),
    ))
    .probe();
    assert_eq!(report.status, ProviderStatus::NotRunning);
}

#[test]
fn davinci_reports_incompatible_when_scripting_files_are_missing() {
    let report = DaVinciProvider::new(davinci(
        true,
        Ok(ProcessState::Running),
        false,
        true,
        Err(RuntimeFailure::Other),
    ))
    .probe();
    assert_eq!(report.status, ProviderStatus::Incompatible);
}

#[test]
fn davinci_reports_permission_missing_when_connection_is_denied() {
    let report = DaVinciProvider::new(davinci(
        true,
        Ok(ProcessState::Running),
        true,
        true,
        bridge(3, include_str!("fixtures/resolve/permission-missing.json")),
    ))
    .probe();
    assert_eq!(report.status, ProviderStatus::PermissionMissing);
}

#[test]
fn davinci_reports_incompatible_for_malformed_bridge_response() {
    let report = DaVinciProvider::new(davinci(
        true,
        Ok(ProcessState::Running),
        true,
        true,
        bridge(0, include_str!("fixtures/resolve/malformed.json")),
    ))
    .probe();
    assert_eq!(report.status, ProviderStatus::Incompatible);
}

#[test]
fn davinci_available_report_drops_project_and_timeline_names() {
    let report = DaVinciProvider::new(davinci(
        true,
        Ok(ProcessState::Running),
        true,
        true,
        bridge(0, include_str!("fixtures/resolve/available.json")),
    ))
    .probe();
    assert_eq!(report.status, ProviderStatus::Available);
    assert!(matches!(
        report.facts,
        ProviderFacts::DavinciResolve(ref facts)
            if facts.project_loaded && facts.timeline_loaded
    ));
    let serialized = serde_json::to_string(&report).unwrap();
    assert!(!serialized.contains("fixture-secret"));
}

#[test]
fn serialized_status_uses_the_shared_snake_case_contract() {
    assert_eq!(
        serde_json::to_string(&ProviderStatus::PermissionMissing).unwrap(),
        "\"permission_missing\""
    );
}
