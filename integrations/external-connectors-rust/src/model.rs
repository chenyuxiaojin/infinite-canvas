use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Eagle,
    DavinciResolve,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStatus {
    Available,
    NotInstalled,
    NotRunning,
    PermissionMissing,
    Incompatible,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    NotRunning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeFailure {
    PermissionDenied,
    Unreachable,
    InvalidResponse,
    TimedOut,
    StdoutLimitExceeded,
    StderrLimitExceeded,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EagleFacts {
    pub installed: bool,
    pub running: bool,
    pub api_reachable: bool,
    pub api_version: String,
    pub library_loaded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolveEnvironmentStatus {
    Missing,
    StandardPath,
    NonstandardPathIgnored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveEnvironmentFacts {
    pub resolve_script_api: ResolveEnvironmentStatus,
    pub resolve_script_lib: ResolveEnvironmentStatus,
    pub pythonpath: ResolveEnvironmentStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaVinciFacts {
    pub installed: bool,
    pub running: bool,
    pub scripting_module_found: bool,
    pub scripting_library_found: bool,
    pub environment: ResolveEnvironmentFacts,
    pub version: Option<String>,
    pub project_loaded: bool,
    pub timeline_loaded: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderFacts {
    Eagle(EagleFacts),
    DavinciResolve(DaVinciFacts),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderReport {
    pub provider: ProviderKind,
    pub status: ProviderStatus,
    pub diagnostic: String,
    /// Fixed, read-only contract supported by this provider implementation.
    pub capabilities: Vec<String>,
    /// Deliberately minimal facts. No library, project, timeline, or media names.
    pub facts: ProviderFacts,
}
