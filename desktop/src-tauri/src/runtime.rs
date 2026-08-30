use std::{path::Path, sync::Mutex, time::Duration};

use local_ai_audio::{
    Capability, EndToEndStatus, LoopbackServiceProbe, ProviderId as AudioProviderId,
    ProviderStatus as AudioProviderStatus, ServiceProbe, ServiceState, ServiceStatus,
};
use local_executor::{
    AllowedRoot, Executor, ExecutorConfig, GenerateTestClip, OutputConflictPolicy, RootId,
    ScopedPath, SubmitOutcome, TaskAction, TaskId, TaskRequest, TaskSnapshot, ToolDiscoveryConfig,
    Toolchain,
};
use serde::Serialize;
use tauri::State;

const ACCEPTANCE_ROOT_ID: &str = "desktop-acceptance";
const TEST_CLIP_IDEMPOTENCY_KEY: &str = "desktop-p2-deterministic-clip-v1";

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

pub(crate) struct DesktopRuntime {
    executor: Mutex<Option<Executor>>,
    ffmpeg: FfmpegSummary,
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
}

#[tauri::command]
pub(crate) async fn probe_desktop_runtime(
    runtime: State<'_, DesktopRuntime>,
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

#[tauri::command]
pub(crate) fn generate_desktop_test_clip(
    runtime: State<'_, DesktopRuntime>,
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
pub(crate) fn desktop_task_status(
    runtime: State<'_, DesktopRuntime>,
    task_id: TaskId,
) -> Result<TaskSnapshot, String> {
    runtime.with_executor(|executor| executor.task(&task_id).map_err(|error| error.to_string()))
}

#[tauri::command]
pub(crate) fn cancel_desktop_task(
    runtime: State<'_, DesktopRuntime>,
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
