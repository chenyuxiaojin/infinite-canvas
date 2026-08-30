use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PROTOCOL_VERSION: &str = "1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderId {
    #[serde(rename = "index_tts_25")]
    IndexTts25,
    #[serde(rename = "vox_cpm_2")]
    VoxCpm2,
}

impl ProviderId {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::IndexTts25 => "IndexTTS-2.5",
            Self::VoxCpm2 => "VoxCPM2",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderStatus {
    NotFound,
    Discovered,
    Ready,
    NotRunning,
    ModelMissing,
    Incompatible,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndToEndStatus {
    NotRun,
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub speech_synthesis: bool,
    pub voice_design: bool,
    pub reference_audio: bool,
    pub output_formats: Vec<String>,
}

impl Capability {
    pub(crate) fn for_provider(provider: ProviderId) -> Self {
        Self {
            speech_synthesis: true,
            voice_design: provider == ProviderId::VoxCpm2,
            reference_audio: true,
            output_formats: vec!["wav".to_owned()],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Fact,
    Inference,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: EvidenceKind,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl Evidence {
    pub(crate) fn fact(code: &str, message: impl Into<String>, path: Option<PathBuf>) -> Self {
        Self {
            kind: EvidenceKind::Fact,
            code: code.to_owned(),
            message: message.into(),
            path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeState {
    pub python: Option<PathBuf>,
    pub version: Option<String>,
    pub compatible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    Ready,
    NotRunning,
    UnexpectedResponse,
    Error,
    NotChecked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceState {
    pub status: ServiceStatus,
    pub loopback_port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    pub identity_confirmed: bool,
    pub detail: String,
}

impl ServiceState {
    pub fn not_checked(provider: ProviderId) -> Self {
        Self {
            status: ServiceStatus::NotChecked,
            loopback_port: provider.loopback_port(),
            http_status: None,
            identity_confirmed: false,
            detail: "service probe disabled".to_owned(),
        }
    }
}

impl ProviderId {
    pub(crate) fn loopback_port(self) -> u16 {
        match self {
            Self::IndexTts25 => 7860,
            Self::VoxCpm2 => 8808,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderReport {
    pub provider: ProviderId,
    pub display_name: String,
    pub status: ProviderStatus,
    pub capabilities: Capability,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installation: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_directory: Option<PathBuf>,
    pub missing_model_files: Vec<String>,
    pub runtime: RuntimeState,
    pub service: ServiceState,
    /// A probe never upgrades this from `not_run`: HTTP/process evidence alone
    /// is intentionally insufficient. A successful smoke response is the proof.
    pub end_to_end: EndToEndStatus,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProbeResponse {
    pub protocol_version: String,
    pub providers: Vec<ProviderReport>,
}

impl ProbeResponse {
    pub(crate) fn new(providers: Vec<ProviderReport>) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION.to_owned(),
            providers,
        }
    }
}

/// Closed IPC surface for a future Tauri command. It deliberately has no shell
/// command or URL field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcRequest {
    Discover { roots: Vec<PathBuf> },
    VerifyAudio { path: PathBuf },
    SmokeTest { provider: ProviderId },
}
