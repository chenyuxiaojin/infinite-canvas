use serde::Deserialize;

use crate::{
    DaVinciFacts, ProcessState, ProviderFacts, ProviderKind, ProviderReport, ProviderStatus,
    ResolveEnvironmentFacts, RuntimeFailure,
};

const CAPABILITIES: [&str; 4] = [
    "discover.installation",
    "discover.runtime",
    "read.version",
    "read.current_context",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveDiscovery {
    pub installed: bool,
    pub scripting_module_found: bool,
    pub scripting_library_found: bool,
    pub environment: ResolveEnvironmentFacts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveBridgeResponse {
    pub exit_code: Option<i32>,
    pub stdout: String,
}

pub trait DaVinciRuntime {
    fn discover(&self) -> ResolveDiscovery;
    fn process_state(&self) -> Result<ProcessState, RuntimeFailure>;
    fn run_read_only_bridge(&self) -> Result<ResolveBridgeResponse, RuntimeFailure>;
}

pub struct DaVinciProvider<R> {
    runtime: R,
}

impl<R> DaVinciProvider<R>
where
    R: DaVinciRuntime,
{
    pub fn new(runtime: R) -> Self {
        Self { runtime }
    }

    pub fn probe(&self) -> ProviderReport {
        let discovery = self.runtime.discover();
        if !discovery.installed {
            return report(
                ProviderStatus::NotInstalled,
                "DaVinci Resolve was not found in a standard macOS application location.",
                facts(&discovery, false, None, false, false),
            );
        }

        let process = self.runtime.process_state();
        let running = matches!(process, Ok(ProcessState::Running));
        match process {
            Ok(ProcessState::NotRunning) => {
                return report(
                    ProviderStatus::NotRunning,
                    "DaVinci Resolve is installed but is not running.",
                    facts(&discovery, false, None, false, false),
                )
            }
            Err(RuntimeFailure::PermissionDenied) => {
                return report(
                    ProviderStatus::PermissionMissing,
                    "DaVinci Resolve is installed, but macOS denied process discovery.",
                    facts(&discovery, false, None, false, false),
                )
            }
            Err(_) => {
                return report(
                    ProviderStatus::Error,
                    "DaVinci Resolve process discovery failed unexpectedly.",
                    facts(&discovery, false, None, false, false),
                )
            }
            Ok(ProcessState::Running) => {}
        }

        if !discovery.scripting_module_found || !discovery.scripting_library_found {
            return report(
                ProviderStatus::Incompatible,
                "DaVinci Resolve is running, but a required local scripting interface component was not found.",
                facts(&discovery, running, None, false, false),
            );
        }

        match self.runtime.run_read_only_bridge() {
            Ok(output) => self.report_bridge(discovery, output),
            Err(RuntimeFailure::PermissionDenied) => report(
                ProviderStatus::PermissionMissing,
                "DaVinci Resolve denied the fixed read-only scripting probe.",
                facts(&discovery, running, None, false, false),
            ),
            Err(RuntimeFailure::InvalidResponse) => report(
                ProviderStatus::Incompatible,
                "DaVinci Resolve returned an unsupported response to the read-only scripting probe.",
                facts(&discovery, running, None, false, false),
            ),
            Err(_) => report(
                ProviderStatus::Error,
                "The DaVinci Resolve read-only scripting probe failed unexpectedly.",
                facts(&discovery, running, None, false, false),
            ),
        }
    }

    fn report_bridge(
        &self,
        discovery: ResolveDiscovery,
        output: ResolveBridgeResponse,
    ) -> ProviderReport {
        let parsed: BridgePayload = match serde_json::from_str(output.stdout.trim()) {
            Ok(value) => value,
            Err(_) => return report(
                ProviderStatus::Incompatible,
                "DaVinci Resolve returned malformed JSON to the fixed read-only scripting probe.",
                facts(&discovery, true, None, false, false),
            ),
        };

        if output.exit_code != Some(0) || !parsed.ok {
            let (status, diagnostic) = match parsed.code.as_deref() {
                Some("resolve_unavailable") | Some("permission_denied") => (
                    ProviderStatus::PermissionMissing,
                    "DaVinci Resolve is running, but the scripting API did not grant a connection.",
                ),
                Some("module_import_failed") | Some("scripting_library_unavailable") => (
                    ProviderStatus::Incompatible,
                    "DaVinci Resolve's installed scripting interface could not be loaded.",
                ),
                _ => (
                    ProviderStatus::Error,
                    "DaVinci Resolve's fixed read-only scripting probe failed.",
                ),
            };
            return report(
                status,
                diagnostic,
                facts(&discovery, true, None, false, false),
            );
        }

        let version = parsed.version.and_then(sanitize_version);
        if version.is_none() {
            return report(
                ProviderStatus::Incompatible,
                "DaVinci Resolve connected, but did not return a supported version value.",
                facts(
                    &discovery,
                    true,
                    None,
                    parsed.project_loaded,
                    parsed.timeline_loaded,
                ),
            );
        }

        report(
            ProviderStatus::Available,
            "DaVinci Resolve's fixed read-only scripting probe connected successfully.",
            facts(
                &discovery,
                true,
                version,
                parsed.project_loaded,
                parsed.timeline_loaded,
            ),
        )
    }
}

#[derive(Debug, Deserialize)]
struct BridgePayload {
    ok: bool,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    project_loaded: bool,
    #[serde(default)]
    timeline_loaded: bool,
}

fn sanitize_version(version: String) -> Option<String> {
    let trimmed = version.trim();
    if trimmed.is_empty()
        || trimmed.len() > 64
        || !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ' | '(' | ')'))
    {
        return None;
    }
    Some(trimmed.to_owned())
}

fn facts(
    discovery: &ResolveDiscovery,
    running: bool,
    version: Option<String>,
    project_loaded: bool,
    timeline_loaded: bool,
) -> DaVinciFacts {
    DaVinciFacts {
        installed: discovery.installed,
        running,
        scripting_module_found: discovery.scripting_module_found,
        scripting_library_found: discovery.scripting_library_found,
        environment: discovery.environment.clone(),
        version,
        project_loaded,
        timeline_loaded,
    }
}

fn report(status: ProviderStatus, diagnostic: &str, facts: DaVinciFacts) -> ProviderReport {
    ProviderReport {
        provider: ProviderKind::DavinciResolve,
        status,
        diagnostic: diagnostic.to_owned(),
        capabilities: CAPABILITIES
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        facts: ProviderFacts::DavinciResolve(facts),
    }
}
