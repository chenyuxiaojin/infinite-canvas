use serde_json::Value;

use crate::{
    EagleFacts, ProcessState, ProviderFacts, ProviderKind, ProviderReport, ProviderStatus,
    RuntimeFailure,
};

const CAPABILITIES: [&str; 4] = [
    "discover.installation",
    "discover.runtime",
    "read.api_health",
    "read.library_context",
];

/// The complete Eagle HTTP allowlist. No caller-provided URL or path is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EagleReadEndpoint {
    LibraryInfo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReadResponse {
    pub status: u16,
    pub body: String,
}

pub trait EagleRuntime {
    fn application_installed(&self) -> bool;
    fn process_state(&self) -> Result<ProcessState, RuntimeFailure>;
    fn get(&self, endpoint: EagleReadEndpoint) -> Result<HttpReadResponse, RuntimeFailure>;
}

pub struct EagleProvider<R> {
    runtime: R,
}

impl<R> EagleProvider<R>
where
    R: EagleRuntime,
{
    pub fn new(runtime: R) -> Self {
        Self { runtime }
    }

    pub fn probe(&self) -> ProviderReport {
        let installed = self.runtime.application_installed();
        let process = self.runtime.process_state();
        let running = matches!(process, Ok(ProcessState::Running));
        if matches!(process, Ok(ProcessState::NotRunning)) {
            return report(
                if installed {
                    ProviderStatus::NotRunning
                } else {
                    ProviderStatus::NotInstalled
                },
                if installed {
                    "Eagle is installed but is not running."
                } else {
                    "Eagle was not found in a standard macOS application location."
                },
                facts(installed, false, false, false),
            );
        }

        match self.runtime.get(EagleReadEndpoint::LibraryInfo) {
            Ok(response) => self.report_http(installed, running, response),
            Err(RuntimeFailure::PermissionDenied) => report(
                ProviderStatus::PermissionMissing,
                "Eagle's local read-only API probe was denied.",
                facts(installed, running, false, false),
            ),
            Err(RuntimeFailure::Unreachable)
                if matches!(process, Err(RuntimeFailure::PermissionDenied)) =>
            {
                report(
                    ProviderStatus::PermissionMissing,
                    "Eagle process visibility and the local API are both unavailable due to a permission boundary.",
                    facts(installed, false, false, false),
                )
            }
            Err(RuntimeFailure::Unreachable) if !installed => report(
                ProviderStatus::NotInstalled,
                "Eagle was not found in a standard macOS application location.",
                facts(false, false, false, false),
            ),
            Err(RuntimeFailure::Unreachable) => report(
                ProviderStatus::Error,
                "Eagle appears to be running, but its local read-only API is unreachable.",
                facts(installed, running, false, false),
            ),
            Err(RuntimeFailure::InvalidResponse) => report(
                ProviderStatus::Incompatible,
                "Eagle returned a response that does not match the supported local API shape.",
                facts(installed, running, true, false),
            ),
            Err(RuntimeFailure::Other) => report(
                ProviderStatus::Error,
                "The Eagle read-only probe failed unexpectedly.",
                facts(installed, running, false, false),
            ),
            Err(
                RuntimeFailure::TimedOut
                | RuntimeFailure::StdoutLimitExceeded
                | RuntimeFailure::StderrLimitExceeded,
            ) => report(
                ProviderStatus::Error,
                "The Eagle read-only probe crossed a runtime safety boundary.",
                facts(installed, running, false, false),
            ),
        }
    }

    fn report_http(
        &self,
        installed: bool,
        running: bool,
        response: HttpReadResponse,
    ) -> ProviderReport {
        match response.status {
            401 | 403 => report(
                ProviderStatus::PermissionMissing,
                "Eagle's local API rejected the read-only library health request.",
                facts(installed, running, true, false),
            ),
            404 | 405 => report(
                ProviderStatus::Incompatible,
                "Eagle is reachable, but the supported V2 read-only endpoint is unavailable.",
                facts(installed, running, true, false),
            ),
            200..=299 => match parse_library_info(&response.body) {
                Some(library_loaded) => report(
                    ProviderStatus::Available,
                    if library_loaded {
                        "Eagle's V2 local API is healthy and a library context is available."
                    } else {
                        "Eagle's V2 local API is healthy; no loaded library context was returned."
                    },
                    facts(installed, true, true, library_loaded),
                ),
                None => report(
                    ProviderStatus::Incompatible,
                    "Eagle returned malformed or unsupported JSON for the V2 library health request.",
                    facts(installed, running, true, false),
                ),
            },
            _ => report(
                ProviderStatus::Error,
                "Eagle's local API returned an unexpected status for the read-only request.",
                facts(installed, running, true, false),
            ),
        }
    }
}

fn parse_library_info(body: &str) -> Option<bool> {
    let value: Value = serde_json::from_str(body).ok()?;
    let object = value.as_object()?;
    if let Some(data) = object.get("data") {
        return Some(data.is_object());
    }
    match object.get("status").and_then(Value::as_str) {
        Some("success") => Some(false),
        Some("error") => Some(false),
        _ => None,
    }
}

fn facts(installed: bool, running: bool, api_reachable: bool, library_loaded: bool) -> EagleFacts {
    EagleFacts {
        installed,
        running,
        api_reachable,
        api_version: "v2".to_owned(),
        library_loaded,
    }
}

fn report(status: ProviderStatus, diagnostic: &str, facts: EagleFacts) -> ProviderReport {
    ProviderReport {
        provider: ProviderKind::Eagle,
        status,
        diagnostic: diagnostic.to_owned(),
        capabilities: CAPABILITIES
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
        facts: ProviderFacts::Eagle(facts),
    }
}
