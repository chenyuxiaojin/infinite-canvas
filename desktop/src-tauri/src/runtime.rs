use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

use local_agent_adapter::{
    AgentRuntime, BridgeError, ImageIngestRequest, TestClipRequest, VideoIngestRequest,
};
use local_ai_audio::{
    Capability, EndToEndStatus, LoopbackServiceProbe, ProviderId as AudioProviderId,
    ProviderStatus as AudioProviderStatus, ServiceProbe, ServiceState, ServiceStatus,
};
use local_executor::{
    AllowedRoot, Executor, ExecutorConfig, ExecutorError, GenerateTestClip, MediaProbe,
    OutputConflictPolicy, RootId, ScopedPath, SubmitOutcome, TaskAction, TaskId, TaskRequest,
    TaskResult, TaskSnapshot, TaskStatus, ToolDiscoveryConfig, Toolchain, TranscodeToMp4,
};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;

use crate::local_media::{LocalMediaManager, LocalMediaResolution, TaskMediaReferenceInput};

const ACCEPTANCE_ROOT_ID: &str = "desktop-acceptance";
const AGENT_MEDIA_ROOT_ID: &str = "agent-media";
const TEST_CLIP_IDEMPOTENCY_KEY: &str = "desktop-p2-deterministic-clip-v1";
const MAX_AGENT_MEDIA_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_AGENT_IMAGE_BYTES: u64 = 100 * 1024 * 1024;
const AGENT_IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

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
    acceptance_directory: Option<PathBuf>,
    agent_media_directory: Option<PathBuf>,
    local_media: std::sync::Arc<LocalMediaManager>,
}

impl DesktopRuntime {
    pub(crate) fn initialize(
        app_data_directory: &Path,
        local_media: std::sync::Arc<LocalMediaManager>,
    ) -> Self {
        let toolchain = match Toolchain::discover(ToolDiscoveryConfig::default()) {
            Ok(toolchain) => toolchain,
            Err(_) => {
                return Self::unavailable(
                    "FFmpeg or ffprobe was not found in the desktop allowlist.",
                    local_media,
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
        let agent_media_directory = app_data_directory.join("agent-media");
        let agent_media_inbox = agent_media_directory.join("inbox");
        let agent_media_verified = agent_media_directory.join("verified");
        if std::fs::create_dir_all(&state_directory).is_err()
            || std::fs::create_dir_all(&acceptance_directory).is_err()
            || std::fs::create_dir_all(&agent_media_inbox).is_err()
            || std::fs::create_dir_all(&agent_media_verified).is_err()
        {
            return Self {
                executor: Mutex::new(None),
                acceptance_directory: None,
                agent_media_directory: None,
                local_media,
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
                    agent_media_directory: None,
                    local_media,
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
                    agent_media_directory: None,
                    local_media,
                    ffmpeg: FfmpegSummary {
                        status: "error",
                        diagnostic: "The desktop acceptance root could not be registered."
                            .to_owned(),
                        tools,
                    },
                };
            }
        };
        let agent_media_root_id = match RootId::new(AGENT_MEDIA_ROOT_ID) {
            Ok(root_id) => root_id,
            Err(_) => {
                return Self {
                    executor: Mutex::new(None),
                    acceptance_directory: None,
                    agent_media_directory: None,
                    local_media,
                    ffmpeg: FfmpegSummary {
                        status: "error",
                        diagnostic: "The Agent media root identifier is invalid.".to_owned(),
                        tools,
                    },
                };
            }
        };
        let agent_media_root = match AllowedRoot::new(agent_media_root_id, &agent_media_directory) {
            Ok(root) => root,
            Err(_) => {
                return Self {
                    executor: Mutex::new(None),
                    acceptance_directory: None,
                    agent_media_directory: None,
                    local_media,
                    ffmpeg: FfmpegSummary {
                        status: "error",
                        diagnostic: "The Agent media root could not be registered.".to_owned(),
                        tools,
                    },
                };
            }
        };
        let executor = match Executor::new(ExecutorConfig {
            state_directory,
            allowed_roots: vec![allowed_root, agent_media_root],
            toolchain,
        }) {
            Ok(executor) => executor,
            Err(_) => {
                return Self {
                    executor: Mutex::new(None),
                    acceptance_directory: None,
                    agent_media_directory: None,
                    local_media,
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
            agent_media_directory: Some(agent_media_directory),
            local_media,
            ffmpeg: FfmpegSummary {
                status: "available",
                diagnostic: "The allowlisted local media executor is ready.".to_owned(),
                tools,
            },
        }
    }

    fn unavailable(diagnostic: &str, local_media: std::sync::Arc<LocalMediaManager>) -> Self {
        Self {
            executor: Mutex::new(None),
            acceptance_directory: None,
            agent_media_directory: None,
            local_media,
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

    pub(crate) fn agent_media_directory(&self) -> Option<&PathBuf> {
        self.agent_media_directory.as_ref()
    }

    pub(crate) fn submit_paid_media_verification(
        &self,
        request: &VideoIngestRequest,
    ) -> Result<TaskId, BridgeError> {
        let media_directory = self
            .agent_media_directory
            .as_ref()
            .ok_or_else(|| BridgeError::unavailable(self.ffmpeg.diagnostic.clone()))?;
        let task = agent_video_ingest_request(media_directory, request)?;
        let outcome = self.with_agent_executor(|executor| executor.submit(task))?;
        Ok(match outcome {
            SubmitOutcome::Accepted(task_id) | SubmitOutcome::Duplicate(task_id) => task_id,
        })
    }

    pub(crate) fn paid_media_task(&self, task_id: &TaskId) -> Result<TaskSnapshot, BridgeError> {
        self.with_agent_executor(|executor| executor.task(task_id))
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

    fn media_inbox(&self) -> Result<Value, BridgeError> {
        let directory = self
            .agent_media_directory
            .as_ref()
            .map(|directory| directory.join("inbox"))
            .ok_or_else(|| BridgeError::unavailable(self.ffmpeg.diagnostic.clone()))?;
        if !directory.is_dir() {
            return Err(BridgeError::unavailable(
                "The fixed Agent media inbox is unavailable.",
            ));
        }
        Ok(json!({
            "kind": "fixed_app_support_inbox",
            "inbox_id": "agent-media-inbox",
            "request_field": "inbox_file_name",
            "accepted_mime_types": ["video/mp4", "image/png", "image/jpeg", "image/webp"],
            "max_file_bytes": MAX_AGENT_MEDIA_BYTES,
            "max_image_file_bytes": MAX_AGENT_IMAGE_BYTES,
            "arbitrary_paths": false,
            "absolute_path_exposed": false
        }))
    }

    fn validate_video_ingest(&self, request: &VideoIngestRequest) -> Result<(), BridgeError> {
        let media_directory = self
            .agent_media_directory
            .as_ref()
            .ok_or_else(|| BridgeError::unavailable(self.ffmpeg.diagnostic.clone()))?;
        agent_video_ingest_request(media_directory, request).map(|_| ())
    }

    fn submit_video_ingest(&self, request: &VideoIngestRequest) -> Result<Value, BridgeError> {
        let media_directory = self
            .agent_media_directory
            .as_ref()
            .ok_or_else(|| BridgeError::unavailable(self.ffmpeg.diagnostic.clone()))?;
        let task = agent_video_ingest_request(media_directory, request)?;
        let outcome = self.with_agent_executor(|executor| executor.submit(task))?;
        let (task_id, duplicate) = match outcome {
            SubmitOutcome::Accepted(task_id) => (task_id, false),
            SubmitOutcome::Duplicate(task_id) => (task_id, true),
        };
        Ok(json!({
            "task_id": task_id.as_str(),
            "duplicate": duplicate,
            "mode": "allowlisted_mp4_ingest",
            "paid": false
        }))
    }

    fn ingest_image(&self, request: &ImageIngestRequest) -> Result<Value, BridgeError> {
        let media_directory = self
            .agent_media_directory
            .as_ref()
            .ok_or_else(|| BridgeError::unavailable(self.ffmpeg.diagnostic.clone()))?;
        agent_image_ingest(media_directory, &self.local_media, request)
    }

    fn quote_video_generation(
        &self,
        resolution: &str,
        duration_seconds: u64,
    ) -> Result<Value, BridgeError> {
        let config = crate::paid_generation::load_config(self.local_media.app_data_directory())
            .map_err(BridgeError::unavailable)?;
        crate::paid_generation::quote(&config, resolution, duration_seconds).ok_or_else(|| {
            BridgeError::unavailable(format!(
                "付费生成配置缺少 {resolution} 的单价，请补充 price_yuan_per_second"
            ))
        })
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
        task_snapshot_value(self, &task_id, snapshot, false).map_err(BridgeError::internal)
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
) -> Result<Value, String> {
    let snapshot = runtime
        .with_executor(|executor| executor.task(&task_id).map_err(|error| error.to_string()))?;
    task_snapshot_value(&runtime, &task_id, snapshot, true)
}

#[tauri::command]
pub(crate) fn desktop_task_media_reference(
    runtime: State<'_, std::sync::Arc<DesktopRuntime>>,
    task_id: TaskId,
) -> Result<LocalMediaResolution, String> {
    let snapshot = runtime
        .with_executor(|executor| executor.task(&task_id).map_err(|error| error.to_string()))?;
    task_media_reference(&runtime, &snapshot)
}

fn task_snapshot_value(
    runtime: &DesktopRuntime,
    task_id: &TaskId,
    snapshot: TaskSnapshot,
    include_playback_url: bool,
) -> Result<Value, String> {
    let mut value = serde_json::to_value(&snapshot)
        .map_err(|_| "The desktop task status could not be encoded.".to_owned())?;
    if let Ok(mut reference) = task_media_reference(runtime, &snapshot) {
        if !include_playback_url {
            reference.playback_url = None;
        }
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "local_media".to_owned(),
                serde_json::to_value(reference)
                    .map_err(|_| "The local media reference could not be encoded.".to_owned())?,
            );
            object.insert("id".to_owned(), Value::String(task_id.as_str().to_owned()));
        }
    }
    Ok(value)
}

pub(crate) fn task_media_reference(
    runtime: &DesktopRuntime,
    snapshot: &TaskSnapshot,
) -> Result<LocalMediaResolution, String> {
    let (scoped, expected_sha256, probe) = completed_media_result(snapshot)?;
    let (root, root_id) = if scoped.root.as_str() == ACCEPTANCE_ROOT_ID {
        (
            runtime
                .acceptance_directory
                .as_ref()
                .ok_or_else(|| "The desktop acceptance directory is unavailable.".to_owned())?,
            ACCEPTANCE_ROOT_ID,
        )
    } else {
        (
            runtime
                .agent_media_directory
                .as_ref()
                .ok_or_else(|| "The Agent media directory is unavailable.".to_owned())?,
            AGENT_MEDIA_ROOT_ID,
        )
    };
    let video = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type == "video");
    runtime
        .local_media
        .reference_for_task_media(TaskMediaReferenceInput {
            root_id,
            root,
            relative: &scoped.relative,
            sha256: &expected_sha256,
            mime_type: "video/mp4",
            width: video.and_then(|stream| stream.width.map(u64::from)),
            height: video.and_then(|stream| stream.height.map(u64::from)),
            duration_ms: probe.duration_ms,
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

fn agent_video_ingest_request(
    media_directory: &Path,
    request: &VideoIngestRequest,
) -> Result<TaskRequest, BridgeError> {
    validate_project_id(&request.project_id).map_err(BridgeError::invalid)?;
    validate_agent_identifier("node_id", &request.node_id, 64)?;
    validate_agent_identifier("request_id", &request.request_id, 128)?;
    if request.title.trim().is_empty() || request.title.len() > 256 {
        return Err(BridgeError::invalid("The Agent video title is invalid."));
    }
    if !request.position.x.is_finite()
        || !request.position.y.is_finite()
        || request.position.x.abs() > 10_000_000.0
        || request.position.y.abs() > 10_000_000.0
    {
        return Err(BridgeError::invalid(
            "The Agent video position is outside the allowed range.",
        ));
    }
    if !request.size.width.is_finite()
        || !request.size.height.is_finite()
        || !(40.0..=10_000.0).contains(&request.size.width)
        || !(40.0..=10_000.0).contains(&request.size.height)
    {
        return Err(BridgeError::invalid(
            "The Agent video size is outside the allowed range.",
        ));
    }
    validate_inbox_file_name(&request.inbox_file_name, &["mp4"])?;
    validate_sha256(&request.expected_sha256)?;
    let input_path = media_directory.join("inbox").join(&request.inbox_file_name);
    let metadata = std::fs::symlink_metadata(&input_path)
        .map_err(|_| BridgeError::not_found("The allowlisted inbox media was not found."))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_AGENT_MEDIA_BYTES
    {
        return Err(BridgeError::forbidden(
            "The inbox video crossed the fixed media boundary.",
        ));
    }
    let actual_sha256 = sha256_file(&input_path)?;
    if actual_sha256 != request.expected_sha256 {
        return Err(BridgeError::conflict(
            "MEDIA_DIGEST_MISMATCH",
            "The inbox video does not match expected_sha256.",
        ));
    }
    let root = RootId::new(AGENT_MEDIA_ROOT_ID).map_err(|error| {
        BridgeError::internal(format!("The Agent media root is invalid: {error}"))
    })?;
    let input = ScopedPath::new(
        root.clone(),
        Path::new("inbox").join(&request.inbox_file_name),
    )
    .map_err(|_| BridgeError::forbidden("The inbox file name crossed the allowlisted root."))?;
    let digest = hex_sha256(format!("{}:{}", request.project_id, request.request_id).as_bytes());
    let output = ScopedPath::new(root, format!("verified/agent-video-{}.mp4", &digest[..20]))
        .map_err(|_| BridgeError::internal("The verified Agent media path is invalid."))?;
    Ok(TaskRequest {
        idempotency_key: format!(
            "desktop-agent-video-ingest-v1:{}:{}:{}",
            request.project_id, request.request_id, request.expected_sha256
        ),
        timeout_ms: Duration::from_secs(10 * 60).as_millis() as u64,
        action: TaskAction::TranscodeToMp4(TranscodeToMp4 {
            input,
            output,
            conflict_policy: OutputConflictPolicy::Reject,
        }),
    })
}

fn agent_image_ingest(
    media_directory: &Path,
    local_media: &LocalMediaManager,
    request: &ImageIngestRequest,
) -> Result<Value, BridgeError> {
    validate_project_id(&request.project_id).map_err(BridgeError::invalid)?;
    validate_agent_identifier("node_id", &request.node_id, 64)?;
    validate_agent_identifier("request_id", &request.request_id, 128)?;
    if request.title.trim().is_empty() || request.title.len() > 256 {
        return Err(BridgeError::invalid("The Agent image title is invalid."));
    }
    if !request.position.x.is_finite()
        || !request.position.y.is_finite()
        || request.position.x.abs() > 10_000_000.0
        || request.position.y.abs() > 10_000_000.0
    {
        return Err(BridgeError::invalid(
            "The Agent image position is outside the allowed range.",
        ));
    }
    if !request.size.width.is_finite()
        || !request.size.height.is_finite()
        || !(40.0..=10_000.0).contains(&request.size.width)
        || !(40.0..=10_000.0).contains(&request.size.height)
    {
        return Err(BridgeError::invalid(
            "The Agent image size is outside the allowed range.",
        ));
    }
    validate_inbox_file_name(&request.inbox_file_name, AGENT_IMAGE_EXTENSIONS)?;
    validate_sha256(&request.expected_sha256)?;
    let file_name = Path::new(&request.inbox_file_name);
    let extension = file_name
        .extension()
        .and_then(|extension| extension.to_str())
        .ok_or_else(|| BridgeError::invalid("The inbox image extension is invalid."))?;
    let mime_type = crate::local_media::mime_type_for_path(file_name)
        .map_err(BridgeError::invalid)
        .and_then(|mime| {
            if mime.starts_with("image/") {
                Ok(mime)
            } else {
                Err(BridgeError::invalid(
                    "The inbox image MIME type is not an image.",
                ))
            }
        })?;
    let relative_output = format!(
        "verified/agent-image-{}.{extension}",
        &request.expected_sha256[..32]
    );
    let target = media_directory.join(&relative_output);
    // 已验收副本按内容寻址；inbox 文件被清理后同一请求重放仍可成功。
    let verified_exists =
        matches!(std::fs::symlink_metadata(&target), Ok(metadata) if metadata.is_file());
    if !verified_exists {
        let input_path = media_directory.join("inbox").join(&request.inbox_file_name);
        let metadata = std::fs::symlink_metadata(&input_path)
            .map_err(|_| BridgeError::not_found("The allowlisted inbox image was not found."))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() == 0
            || metadata.len() > MAX_AGENT_IMAGE_BYTES
        {
            return Err(BridgeError::forbidden(
                "The inbox image crossed the fixed media boundary.",
            ));
        }
        let actual_sha256 = sha256_file(&input_path)?;
        if actual_sha256 != request.expected_sha256 {
            return Err(BridgeError::conflict(
                "MEDIA_DIGEST_MISMATCH",
                "The inbox image does not match expected_sha256.",
            ));
        }
        crate::local_media::copy_verified_file(&input_path, &target, &request.expected_sha256)
            .map_err(BridgeError::internal)?;
    }
    let probe = crate::local_media::probe_media(&target)
        .map_err(|_| BridgeError::invalid("The inbox image could not be decoded as an image."))?;
    // ffprobe 对损坏文件可能回 0×0 流而不报错，必须拒绝零尺寸。
    let (Some(width), Some(height)) = (
        probe.width.filter(|value| *value > 0),
        probe.height.filter(|value| *value > 0),
    ) else {
        return Err(BridgeError::invalid(
            "The inbox image has no decodable dimensions.",
        ));
    };
    let resolution = local_media
        .reference_for_task_media(TaskMediaReferenceInput {
            root_id: AGENT_MEDIA_ROOT_ID,
            root: media_directory,
            relative: Path::new(&relative_output),
            sha256: &request.expected_sha256,
            mime_type: &mime_type,
            width: Some(width),
            height: Some(height),
            duration_ms: None,
        })
        .map_err(BridgeError::internal)?;
    Ok(json!({
        "mode": "allowlisted_image_ingest",
        "paid": false,
        "reference": resolution.reference,
    }))
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
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err("The canvas project identifier is invalid.".to_owned());
    }
    Ok(())
}

fn completed_media_result(
    snapshot: &TaskSnapshot,
) -> Result<(ScopedPath, String, MediaProbe), String> {
    if snapshot.status != TaskStatus::Succeeded {
        return Err("The desktop task has not completed successfully.".to_owned());
    }
    let Some(TaskResult::MediaCreated {
        output,
        sha256,
        probe,
    }) = &snapshot.result
    else {
        return Err("The desktop task did not create media.".to_owned());
    };
    let file_name = output.relative.to_string_lossy();
    let allowed = if output.root.as_str() == ACCEPTANCE_ROOT_ID {
        output.relative.components().count() == 1
            && (file_name == "desktop-test-clip.mp4"
                || ((file_name.starts_with("canvas-test-clip-")
                    || file_name.starts_with("agent-test-clip-"))
                    && file_name.ends_with(".mp4")))
    } else if output.root.as_str() == AGENT_MEDIA_ROOT_ID {
        output.relative.components().count() == 2
            && output.relative.parent() == Some(Path::new("verified"))
            && output
                .relative
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("agent-video-") && name.ends_with(".mp4"))
    } else {
        false
    };
    if !allowed {
        return Err("The desktop task output is outside the approved media roots.".to_owned());
    }
    Ok((output.clone(), sha256.clone(), probe.clone()))
}

fn validate_agent_identifier(label: &str, value: &str, max: usize) -> Result<(), BridgeError> {
    if value.is_empty()
        || value.len() > max
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(BridgeError::invalid(format!(
            "The Agent {label} is invalid."
        )));
    }
    Ok(())
}

fn validate_inbox_file_name(value: &str, extensions: &[&str]) -> Result<(), BridgeError> {
    let path = Path::new(value);
    let extension = path.extension().and_then(|extension| extension.to_str());
    if value.is_empty()
        || value.len() > 255
        || value.chars().any(char::is_control)
        || path.components().count() != 1
        || path.file_name().and_then(|name| name.to_str()) != Some(value)
        || !extension.is_some_and(|extension| extensions.contains(&extension))
    {
        return Err(BridgeError::invalid(format!(
            "inbox_file_name must be one {} file name without a path.",
            extensions.join("/")
        )));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), BridgeError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(BridgeError::invalid(
            "expected_sha256 must be a lowercase SHA-256 digest.",
        ));
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, BridgeError> {
    let mut file = std::fs::File::open(path)
        .map_err(|_| BridgeError::not_found("The allowlisted inbox media was not found."))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| BridgeError::internal("The inbox media could not be hashed."))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
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

    fn video_ingest_request(file_name: &str, sha256: String) -> VideoIngestRequest {
        VideoIngestRequest {
            project_id: "project-1".to_owned(),
            node_id: "video-1".to_owned(),
            request_id: "ingest-1".to_owned(),
            base_revision: 0,
            actor: local_agent_adapter::Actor::Agent,
            inbox_file_name: file_name.to_owned(),
            expected_sha256: sha256,
            title: "Shot 001".to_owned(),
            position: local_agent_adapter::Point { x: 0.0, y: 0.0 },
            size: local_agent_adapter::CanvasSize {
                width: 320.0,
                height: 180.0,
            },
        }
    }

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
    fn video_ingest_accepts_only_a_hashed_mp4_basename_in_the_fixed_inbox() {
        let root = tempfile::tempdir().unwrap();
        let inbox = root.path().join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        let bytes = b"fixture-mp4-bytes";
        std::fs::write(inbox.join("shot-001.mp4"), bytes).unwrap();
        let request = video_ingest_request("shot-001.mp4", hex_sha256(bytes));
        let task = agent_video_ingest_request(root.path(), &request).unwrap();
        let timeout_ms = task.timeout_ms;
        let TaskAction::TranscodeToMp4(parameters) = task.action else {
            panic!("expected the fixed transcode action");
        };
        assert_eq!(parameters.input.root.as_str(), AGENT_MEDIA_ROOT_ID);
        assert_eq!(
            parameters.input.relative,
            PathBuf::from("inbox/shot-001.mp4")
        );
        assert_eq!(parameters.output.root.as_str(), AGENT_MEDIA_ROOT_ID);
        assert_eq!(
            parameters.output.relative.parent(),
            Some(Path::new("verified"))
        );
        assert_eq!(parameters.conflict_policy, OutputConflictPolicy::Reject);
        assert_eq!(timeout_ms, 10 * 60 * 1000);
    }

    #[test]
    fn video_ingest_rejects_paths_symlinks_and_digest_mismatches() {
        let root = tempfile::tempdir().unwrap();
        let inbox = root.path().join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(inbox.join("shot.mp4"), b"fixture").unwrap();

        let traversal = video_ingest_request("../shot.mp4", hex_sha256(b"fixture"));
        assert_eq!(
            agent_video_ingest_request(root.path(), &traversal)
                .unwrap_err()
                .code,
            "INVALID_REQUEST"
        );

        let mismatch = video_ingest_request("shot.mp4", "a".repeat(64));
        assert_eq!(
            agent_video_ingest_request(root.path(), &mismatch)
                .unwrap_err()
                .code,
            "MEDIA_DIGEST_MISMATCH"
        );

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(inbox.join("shot.mp4"), inbox.join("link.mp4")).unwrap();
            let symlink = video_ingest_request("link.mp4", hex_sha256(b"fixture"));
            assert_eq!(
                agent_video_ingest_request(root.path(), &symlink)
                    .unwrap_err()
                    .code,
                "CAPABILITY_DENIED"
            );
        }
    }

    fn image_ingest_request(file_name: &str, sha256: String) -> ImageIngestRequest {
        ImageIngestRequest {
            project_id: "project-1".to_owned(),
            node_id: "image-1".to_owned(),
            request_id: "image-ingest-1".to_owned(),
            base_revision: 0,
            actor: local_agent_adapter::Actor::Agent,
            inbox_file_name: file_name.to_owned(),
            expected_sha256: sha256,
            title: "Frame 001".to_owned(),
            position: local_agent_adapter::Point { x: 0.0, y: 0.0 },
            size: local_agent_adapter::CanvasSize {
                width: 320.0,
                height: 240.0,
            },
        }
    }

    const TINY_PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn image_ingest_registers_a_content_addressed_reference_and_replays_without_inbox() {
        let root = tempfile::tempdir().unwrap();
        let media = root.path().join("agent-media");
        std::fs::create_dir_all(media.join("inbox")).unwrap();
        let inbox_file = media.join("inbox").join("frame-001.png");
        std::fs::write(&inbox_file, TINY_PNG).unwrap();
        if crate::local_media::probe_media(&inbox_file).is_err() {
            eprintln!("skipping image ingest probe test: ffprobe unavailable");
            return;
        }
        let local_media = LocalMediaManager::offline_for_tests(root.path()).unwrap();
        let request = image_ingest_request("frame-001.png", hex_sha256(TINY_PNG));
        let result = agent_image_ingest(&media, &local_media, &request).unwrap();
        assert_eq!(result["mode"], "allowlisted_image_ingest");
        assert_eq!(result["paid"], false);
        let reference = &result["reference"];
        assert_eq!(reference["rootId"], AGENT_MEDIA_ROOT_ID);
        assert_eq!(reference["width"], 1);
        assert_eq!(reference["height"], 1);
        assert_eq!(reference["mimeType"], "image/png");
        assert_eq!(reference["sha256"], hex_sha256(TINY_PNG));
        let relative = reference["relativePath"].as_str().unwrap();
        assert!(relative.starts_with("verified/agent-image-") && relative.ends_with(".png"));
        assert!(reference["storageKey"]
            .as_str()
            .unwrap()
            .starts_with("local-ref:asset-"));
        assert!(result.get("playbackUrl").is_none());
        assert!(reference.get("playbackUrl").is_none());

        std::fs::remove_file(&inbox_file).unwrap();
        let replay = agent_image_ingest(&media, &local_media, &request).unwrap();
        assert_eq!(replay["reference"]["sha256"], reference["sha256"]);

        let garbage = media.join("inbox").join("broken.webp");
        std::fs::write(&garbage, b"not-an-image").unwrap();
        let broken = image_ingest_request("broken.webp", hex_sha256(b"not-an-image"));
        assert_eq!(
            agent_image_ingest(&media, &local_media, &broken)
                .unwrap_err()
                .code,
            "INVALID_REQUEST"
        );
    }

    #[test]
    fn image_ingest_rejects_paths_extensions_symlinks_and_digest_mismatches() {
        let root = tempfile::tempdir().unwrap();
        let media = root.path().join("agent-media");
        std::fs::create_dir_all(media.join("inbox")).unwrap();
        let local_media = LocalMediaManager::offline_for_tests(root.path()).unwrap();
        std::fs::write(media.join("inbox").join("frame.png"), b"fixture").unwrap();

        let traversal = image_ingest_request("../frame.png", hex_sha256(b"fixture"));
        assert_eq!(
            agent_image_ingest(&media, &local_media, &traversal)
                .unwrap_err()
                .code,
            "INVALID_REQUEST"
        );

        let wrong_extension = image_ingest_request("frame.mp4", hex_sha256(b"fixture"));
        assert_eq!(
            agent_image_ingest(&media, &local_media, &wrong_extension)
                .unwrap_err()
                .code,
            "INVALID_REQUEST"
        );

        let mismatch = image_ingest_request("frame.png", "a".repeat(64));
        assert_eq!(
            agent_image_ingest(&media, &local_media, &mismatch)
                .unwrap_err()
                .code,
            "MEDIA_DIGEST_MISMATCH"
        );

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(
                media.join("inbox").join("frame.png"),
                media.join("inbox").join("link.png"),
            )
            .unwrap();
            let symlink = image_ingest_request("link.png", hex_sha256(b"fixture"));
            assert_eq!(
                agent_image_ingest(&media, &local_media, &symlink)
                    .unwrap_err()
                    .code,
                "CAPABILITY_DENIED"
            );
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
