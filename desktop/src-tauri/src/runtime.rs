use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

use local_agent_adapter::{AgentRuntime, BridgeError, TestClipRequest};
use local_ai_audio::{
    Capability, EndToEndStatus, LoopbackServiceProbe, ProviderId as AudioProviderId,
    ProviderStatus as AudioProviderStatus, ServiceProbe, ServiceState, ServiceStatus,
};
use local_executor::{
    AllowedRoot, Executor, ExecutorConfig, ExecutorError, GenerateTestClip, OutputConflictPolicy,
    RootId, ScopedPath, SubmitOutcome, TaskAction, TaskId, TaskRequest, TaskResult, TaskSnapshot,
    TaskStatus, ToolDiscoveryConfig, Toolchain,
};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;

const ACCEPTANCE_ROOT_ID: &str = "desktop-acceptance";
const TEST_CLIP_IDEMPOTENCY_KEY: &str = "desktop-p2-deterministic-clip-v1";
const MAX_DESKTOP_TASK_MEDIA_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct ToolSummary {
    name: String,
    version_line: String,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct FfmpegSummary {
    status: &'static str,
    diagnostic: String,
    tools: Vec<ToolSummary>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AudioProviderSummary {
    provider: AudioProviderId,
    display_name: String,
    status: AudioProviderStatus,
    capabilities: Capability,
    installation_found: bool,
    models_complete: bool,
    runtime_version: Option<String>,
    runtime_compatible: bool,
    service_status: ServiceStatus,
    service_identity_confirmed: bool,
    end_to_end: EndToEndStatus,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct AudioProbeSummary {
    status: &'static str,
    diagnostic: String,
    providers: Vec<AudioProviderSummary>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DesktopRuntimeReport {
    transport: &'static str,
    ffmpeg: FfmpegSummary,
    connectors: Vec<external_connectors::ProviderReport>,
    audio: AudioProbeSummary,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SubmittedTask {
    task_id: String,
    duplicate: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DesktopTaskMedia {
    task_id: String,
    mime_type: &'static str,
    file_name: String,
    sha256: String,
    bytes: Vec<u8>,
}

pub(crate) struct DesktopRuntime {
    executor: Mutex<Option<Executor>>,
    ffmpeg: FfmpegSummary,
    acceptance_directory: Option<PathBuf>,
}

impl DesktopRuntime {
    pub(crate) fn initialize(app_data_directory: &Path) -> Self {
        let toolchain = match Toolchain::discover(ToolDiscoveryConfig::default()) {
            Ok(toolchain) => toolchain,
            Err(_) => {
                return Self::unavailable(
                    "FFmpeg or ffprobe was not found in the desktop allowlist.",
                );
            }
        };
        let tools = toolchain
            .reports()
            .iter()
            .map(|report| ToolSummary {
                name: report.name.clone(),
                version_line: report.version_line.clone(),
            })
            .collect::<Vec<_>>();

        let executor_root = app_data_directory.join("local-executor");
        let state_directory = executor_root.join("state");
        let acceptance_directory = executor_root.join("acceptance");
        if std::fs::create_dir_all(&state_directory).is_err()
            || std::fs::create_dir_all(&acceptance_directory).is_err()
        {
            return Self {
                executor: Mutex::new(None),
                acceptance_directory: None,
                ffmpeg: FfmpegSummary {
                    status: "error",
                    diagnostic: "The desktop executor data directories could not be prepared."
                        .to_owned(),
                    tools,
                },
            };
        }

        let root_id = match RootId::new(ACCEPTANCE_ROOT_ID) {
            Ok(root_id) => root_id,
            Err(_) => {
                return Self {
                    executor: Mutex::new(None),
                    acceptance_directory: None,
                    ffmpeg: FfmpegSummary {
                        status: "error",
                        diagnostic: "The built-in desktop root identifier is invalid.".to_owned(),
                        tools,
                    },
                };
            }
        };
        let allowed_root = match AllowedRoot::new(root_id, &acceptance_directory) {
            Ok(root) => root,
            Err(_) => {
                return Self {
                    executor: Mutex::new(None),
                    acceptance_directory: None,
                    ffmpeg: FfmpegSummary {
                        status: "error",
                        diagnostic: "The desktop acceptance root could not be registered."
                            .to_owned(),
                        tools,
                    },
                };
            }
        };
        let executor = match Executor::new(ExecutorConfig {
            state_directory,
            allowed_roots: vec![allowed_root],
            toolchain,
        }) {
            Ok(executor) => executor,
            Err(_) => {
                return Self {
                    executor: Mutex::new(None),
                    acceptance_directory: None,
                    ffmpeg: FfmpegSummary {
                        status: "error",
                        diagnostic: "The desktop media task journal could not be opened."
                            .to_owned(),
                        tools,
                    },
                };
            }
        };

        Self {
            executor: Mutex::new(Some(executor)),
            acceptance_directory: Some(acceptance_directory),
            ffmpeg: FfmpegSummary {
                status: "available",
                diagnostic: "The allowlisted local media executor is ready.".to_owned(),
                tools,
            },
        }
    }

    fn unavailable(diagnostic: &str) -> Self {
        Self {
            executor: Mutex::new(None),
            acceptance_directory: None,
            ffmpeg: FfmpegSummary {
                status: "unavailable",
                diagnostic: diagnostic.to_owned(),
                tools: Vec::new(),
            },
        }
    }

    pub(crate) fn shutdown(&self) {
        let executor = self
            .executor
            .lock()
            .ok()
            .and_then(|mut executor| executor.take());
        drop(executor);
    }

    fn with_executor<T>(
        &self,
        operation: impl FnOnce(&Executor) -> Result<T, String>,
    ) -> Result<T, String> {
        let executor = self
            .executor
            .lock()
            .map_err(|_| "The desktop executor state is unavailable.".to_owned())?;
        let executor = executor
            .as_ref()
            .ok_or_else(|| self.ffmpeg.diagnostic.clone())?;
        operation(executor)
    }

    fn with_agent_executor<T>(
        &self,
        operation: impl FnOnce(&Executor) -> Result<T, ExecutorError>,
    ) -> Result<T, BridgeError> {
        let executor = self
            .executor
            .lock()
            .map_err(|_| BridgeError::internal("The desktop executor state is unavailable."))?;
        let executor = executor
            .as_ref()
            .ok_or_else(|| BridgeError::unavailable(self.ffmpeg.diagnostic.clone()))?;
        operation(executor).map_err(map_executor_error)
    }

    fn report(&self) -> DesktopRuntimeReport {
        DesktopRuntimeReport {
            transport: "agent_bridge",
            ffmpeg: self.ffmpeg.clone(),
            connectors: external_connectors::probe_all(),
            audio: probe_desktop_audio_services(),
        }
    }
}

#[tauri::command]
pub(crate) async fn probe_desktop_runtime(
    runtime: State<'_, std::sync::Arc<DesktopRuntime>>,
) -> Result<DesktopRuntimeReport, String> {
    let ffmpeg = runtime.ffmpeg.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let connectors = external_connectors::probe_all();
        let audio = probe_desktop_audio_services();
        DesktopRuntimeReport {
            transport: "tauri_ipc",
            ffmpeg,
            connectors,
            audio,
        }
    })
    .await
    .map_err(|_| "The desktop runtime probe could not complete.".to_owned())
}

impl AgentRuntime for DesktopRuntime {
    fn report(&self) -> Result<Value, BridgeError> {
        serde_json::to_value(self.report())
            .map_err(|_| BridgeError::internal("The desktop runtime report could not be encoded."))
    }

    fn submit_test_clip(&self, request: &TestClipRequest) -> Result<Value, BridgeError> {
        let task = agent_test_clip_request(&request.project_id, &request.request_id)?;
        let outcome = self.with_agent_executor(|executor| executor.submit(task))?;
        let (task_id, duplicate) = match outcome {
            SubmitOutcome::Accepted(task_id) => (task_id, false),
            SubmitOutcome::Duplicate(task_id) => (task_id, true),
        };
        Ok(json!({
            "task_id": task_id.as_str(),
            "duplicate": duplicate,
            "mode": "deterministic_local_fixture",
            "paid": false
        }))
    }

    fn task_status(&self, task_id: &str) -> Result<Value, BridgeError> {
        let task_id = parse_task_id(task_id)?;
        let snapshot = self.with_agent_executor(|executor| executor.task(&task_id))?;
        serde_json::to_value(snapshot)
            .map_err(|_| BridgeError::internal("The desktop task status could not be encoded."))
    }

    fn cancel_task(&self, task_id: &str) -> Result<Value, BridgeError> {
        let task_id = parse_task_id(task_id)?;
        let cancelled = self.with_agent_executor(|executor| executor.cancel(&task_id))?;
        Ok(json!({ "task_id": task_id.as_str(), "cancelled": cancelled }))
    }
}

#[tauri::command]
pub(crate) fn generate_desktop_test_clip(
    runtime: State<'_, std::sync::Arc<DesktopRuntime>>,
) -> Result<SubmittedTask, String> {
    let request = deterministic_test_clip_request()?;
    runtime.with_executor(|executor| {
        let outcome = executor
            .submit(request)
            .map_err(|error| error.to_string())?;
        let (task_id, duplicate) = match outcome {
            SubmitOutcome::Accepted(task_id) => (task_id, false),
            SubmitOutcome::Duplicate(task_id) => (task_id, true),
        };
        Ok(SubmittedTask {
            task_id: task_id.as_str().to_owned(),
            duplicate,
        })
    })
}

#[tauri::command]
pub(crate) fn generate_canvas_test_clip(
    runtime: State<'_, std::sync::Arc<DesktopRuntime>>,
    project_id: String,
) -> Result<SubmittedTask, String> {
    let request = canvas_test_clip_request(&project_id)?;
    submit_task(&runtime, request)
}

#[tauri::command]
pub(crate) fn desktop_task_status(
    runtime: State<'_, std::sync::Arc<DesktopRuntime>>,
    task_id: TaskId,
) -> Result<TaskSnapshot, String> {
    runtime.with_executor(|executor| executor.task(&task_id).map_err(|error| error.to_string()))
}

#[tauri::command]
pub(crate) fn desktop_task_media(
    runtime: State<'_, std::sync::Arc<DesktopRuntime>>,
    task_id: TaskId,
) -> Result<DesktopTaskMedia, String> {
    let snapshot = runtime
        .with_executor(|executor| executor.task(&task_id).map_err(|error| error.to_string()))?;
    let (relative, expected_sha256) = completed_media_result(&snapshot)?;
    let acceptance_directory = runtime
        .acceptance_directory
        .as_ref()
        .ok_or_else(|| "The desktop acceptance directory is unavailable.".to_owned())?;
    let path = acceptance_directory.join(&relative);
    let metadata = std::fs::metadata(&path)
        .map_err(|_| "The verified desktop task media is unavailable.".to_owned())?;
    if !metadata.is_file() || metadata.len() > MAX_DESKTOP_TASK_MEDIA_BYTES {
        return Err("The desktop task media crossed the fixed size boundary.".to_owned());
    }
    let bytes = std::fs::read(&path)
        .map_err(|_| "The verified desktop task media could not be read.".to_owned())?;
    let actual_sha256 = hex_sha256(&bytes);
    if actual_sha256 != expected_sha256 {
        return Err("The desktop task media no longer matches its verified digest.".to_owned());
    }
    let file_name = relative
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "The desktop task media name is invalid.".to_owned())?
        .to_owned();
    Ok(DesktopTaskMedia {
        task_id: task_id.as_str().to_owned(),
        mime_type: "video/mp4",
        file_name,
        sha256: actual_sha256,
        bytes,
    })
}

#[tauri::command]
pub(crate) fn cancel_desktop_task(
    runtime: State<'_, std::sync::Arc<DesktopRuntime>>,
    task_id: TaskId,
) -> Result<bool, String> {
    runtime.with_executor(|executor| executor.cancel(&task_id).map_err(|error| error.to_string()))
}

fn deterministic_test_clip_request() -> Result<TaskRequest, String> {
    let root = RootId::new(ACCEPTANCE_ROOT_ID).map_err(|error| error.to_string())?;
    let output =
        ScopedPath::new(root, "desktop-test-clip.mp4").map_err(|error| error.to_string())?;
    Ok(TaskRequest {
        idempotency_key: TEST_CLIP_IDEMPOTENCY_KEY.to_owned(),
        timeout_ms: Duration::from_secs(30).as_millis() as u64,
        action: TaskAction::GenerateTestClip(GenerateTestClip {
            output,
            duration_ms: 1_000,
            width: 320,
            height: 180,
            frame_rate: 24,
            conflict_policy: OutputConflictPolicy::UniqueSuffix,
        }),
    })
}

fn canvas_test_clip_request(project_id: &str) -> Result<TaskRequest, String> {
    validate_project_id(project_id)?;
    let digest = hex_sha256(project_id.as_bytes());
    let output_name = format!("canvas-test-clip-{}.mp4", &digest[..16]);
    fixed_test_clip_request(
        format!("desktop-p3-canvas-test-clip-v1:{project_id}"),
        &output_name,
    )
}

fn agent_test_clip_request(project_id: &str, request_id: &str) -> Result<TaskRequest, BridgeError> {
    validate_project_id(project_id).map_err(BridgeError::invalid)?;
    if request_id.is_empty()
        || request_id.len() > 128
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(BridgeError::invalid("The Agent request_id is invalid."));
    }
    let digest = hex_sha256(format!("{project_id}:{request_id}").as_bytes());
    let output_name = format!("agent-test-clip-{}.mp4", &digest[..16]);
    fixed_test_clip_request(
        format!("desktop-agent-test-clip-v1:{project_id}:{request_id}"),
        &output_name,
    )
    .map_err(BridgeError::invalid)
}

fn fixed_test_clip_request(
    idempotency_key: String,
    output_name: &str,
) -> Result<TaskRequest, String> {
    let root = RootId::new(ACCEPTANCE_ROOT_ID).map_err(|error| error.to_string())?;
    let output = ScopedPath::new(root, output_name).map_err(|error| error.to_string())?;
    Ok(TaskRequest {
        idempotency_key,
        timeout_ms: Duration::from_secs(30).as_millis() as u64,
        action: TaskAction::GenerateTestClip(GenerateTestClip {
            output,
            duration_ms: 1_000,
            width: 320,
            height: 180,
            frame_rate: 24,
            conflict_policy: OutputConflictPolicy::Reject,
        }),
    })
}

fn submit_task(runtime: &DesktopRuntime, request: TaskRequest) -> Result<SubmittedTask, String> {
    runtime.with_executor(|executor| {
        let outcome = executor
            .submit(request)
            .map_err(|error| error.to_string())?;
        let (task_id, duplicate) = match outcome {
            SubmitOutcome::Accepted(task_id) => (task_id, false),
            SubmitOutcome::Duplicate(task_id) => (task_id, true),
        };
        Ok(SubmittedTask {
            task_id: task_id.as_str().to_owned(),
            duplicate,
        })
    })
}

fn validate_project_id(project_id: &str) -> Result<(), String> {
    if project_id.is_empty()
        || project_id.len() > 64
        || !project_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err("The canvas project identifier is invalid.".to_owned());
    }
    Ok(())
}

fn completed_media_result(snapshot: &TaskSnapshot) -> Result<(PathBuf, String), String> {
    if snapshot.status != TaskStatus::Succeeded {
        return Err("The desktop task has not completed successfully.".to_owned());
    }
    let Some(TaskResult::MediaCreated { output, sha256, .. }) = &snapshot.result else {
        return Err("The desktop task did not create media.".to_owned());
    };
    if output.root.as_str() != ACCEPTANCE_ROOT_ID || output.relative.components().count() != 1 {
        return Err("The desktop task output is outside the acceptance root.".to_owned());
    }
    let file_name = output.relative.to_string_lossy();
    if file_name != "desktop-test-clip.mp4"
        && !(file_name.starts_with("canvas-test-clip-") && file_name.ends_with(".mp4"))
    {
        return Err("The desktop task output is not an approved test clip.".to_owned());
    }
    Ok((output.relative.clone(), sha256.clone()))
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn parse_task_id(task_id: &str) -> Result<TaskId, BridgeError> {
    if task_id.is_empty()
        || task_id.len() > 128
        || !task_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(BridgeError::invalid(
            "The desktop task identifier is invalid.",
        ));
    }
    serde_json::from_value(Value::String(task_id.to_owned()))
        .map_err(|_| BridgeError::invalid("The desktop task identifier is invalid."))
}

fn map_executor_error(error: ExecutorError) -> BridgeError {
    match error {
        ExecutorError::TaskNotFound => BridgeError::not_found("The desktop task was not found."),
        ExecutorError::PathDenied | ExecutorError::UnknownRoot => {
            BridgeError::forbidden("The desktop task path crossed the allowlisted root boundary.")
        }
        ExecutorError::IdempotencyConflict | ExecutorError::OutputConflict => {
            BridgeError::conflict("TASK_CONFLICT", error.to_string())
        }
        ExecutorError::ToolUnavailable | ExecutorError::WorkerUnavailable => {
            BridgeError::unavailable(error.to_string())
        }
        ExecutorError::InvalidConfiguration(_) | ExecutorError::InvalidRequest(_) => {
            BridgeError::invalid(error.to_string())
        }
        ExecutorError::StateIo => BridgeError::internal(error.to_string()),
    }
}

fn probe_desktop_audio_services() -> AudioProbeSummary {
    let probe = LoopbackServiceProbe;
    AudioProbeSummary {
        status: "service_only",
        diagnostic: "Only fixed loopback service identities were checked; installation paths require explicit desktop selection.".to_owned(),
        providers: [AudioProviderId::IndexTts25, AudioProviderId::VoxCpm2]
            .into_iter()
            .map(|provider| summarize_audio_service(provider, probe.probe(provider)))
            .collect(),
    }
}

fn summarize_audio_service(
    provider: AudioProviderId,
    service: ServiceState,
) -> AudioProviderSummary {
    let status = match service.status {
        ServiceStatus::Ready => AudioProviderStatus::Ready,
        ServiceStatus::NotRunning | ServiceStatus::NotChecked => AudioProviderStatus::NotRunning,
        ServiceStatus::UnexpectedResponse | ServiceStatus::Error => AudioProviderStatus::Error,
    };
    AudioProviderSummary {
        provider,
        display_name: provider.display_name().to_owned(),
        status,
        capabilities: Capability {
            speech_synthesis: true,
            voice_design: provider == AudioProviderId::VoxCpm2,
            reference_audio: true,
            output_formats: vec!["wav".to_owned()],
        },
        installation_found: false,
        models_complete: false,
        runtime_version: None,
        runtime_compatible: false,
        service_status: service.status,
        service_identity_confirmed: service.identity_confirmed,
        end_to_end: EndToEndStatus::NotRun,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_request_has_no_command_or_freeform_path_surface() {
        let encoded = serde_json::to_string(&deterministic_test_clip_request().unwrap()).unwrap();
        assert!(encoded.contains("generate_test_clip"));
        assert!(encoded.contains("desktop-acceptance"));
        assert!(encoded.contains("desktop-test-clip.mp4"));
        for denied in ["shell", "command", "executable", "url", "args"] {
            assert!(!encoded.contains(denied));
        }
    }

    #[test]
    fn canvas_request_hashes_the_project_id_into_a_fixed_output() {
        let request = canvas_test_clip_request("project_123-test").unwrap();
        assert_eq!(
            request.idempotency_key,
            "desktop-p3-canvas-test-clip-v1:project_123-test"
        );
        let TaskAction::GenerateTestClip(parameters) = request.action else {
            panic!("expected the fixed test-clip action");
        };
        assert_eq!(parameters.output.root.as_str(), ACCEPTANCE_ROOT_ID);
        assert_eq!(
            parameters.output.relative,
            PathBuf::from("canvas-test-clip-a9049bba5ac5d275.mp4")
        );
        assert_eq!(parameters.conflict_policy, OutputConflictPolicy::Reject);
    }

    #[test]
    fn canvas_request_rejects_untrusted_project_identifiers() {
        for project_id in [
            "",
            "../escape",
            "contains/slash",
            "contains space",
            "$(shell)",
        ] {
            assert!(canvas_test_clip_request(project_id).is_err());
        }
        assert!(canvas_test_clip_request(&"a".repeat(65)).is_err());
    }

    #[test]
    fn audio_summary_exposes_no_path_url_or_port_surface() {
        let summary = summarize_audio_service(
            AudioProviderId::IndexTts25,
            ServiceState {
                status: ServiceStatus::NotRunning,
                loopback_port: 7860,
                http_status: None,
                identity_confirmed: false,
                detail: "fixture".to_owned(),
            },
        );
        let encoded = serde_json::to_string(&summary).unwrap();
        assert!(!encoded.contains("path"));
        assert!(!encoded.contains("python"));
        assert!(!encoded.contains("evidence"));
        assert!(!encoded.contains("7860"));
        assert!(!encoded.contains("http"));
    }

    #[test]
    fn not_running_service_never_claims_installation_or_model_success() {
        let summary = summarize_audio_service(
            AudioProviderId::VoxCpm2,
            ServiceState {
                status: ServiceStatus::NotRunning,
                loopback_port: 8808,
                http_status: None,
                identity_confirmed: false,
                detail: "fixture".to_owned(),
            },
        );
        assert_eq!(summary.status, AudioProviderStatus::NotRunning);
        assert!(!summary.installation_found);
        assert!(!summary.models_complete);
        assert!(!summary.runtime_compatible);
        assert_eq!(summary.end_to_end, EndToEndStatus::NotRun);
    }
}
