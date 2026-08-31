use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

use local_agent_adapter::{AgentRuntime, BridgeError, TestClipRequest, VideoIngestRequest};
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
            "accepted_mime_types": ["video/mp4"],
            "max_file_bytes": MAX_AGENT_MEDIA_BYTES,
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

fn task_media_reference(
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
    validate_inbox_file_name(&request.inbox_file_name)?;
    validate_sha256(&request.expected_sha256)?;
    let input_path = media_directory.join("inbox").join(&request.inbox_file_name);
    let metadata = std::fs::symlink_metadata(&input_path)
        .map_err(|_| BridgeError::not_found("The allowlisted inbox video was not found."))?;
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

fn validate_inbox_file_name(value: &str) -> Result<(), BridgeError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > 255
        || value.chars().any(char::is_control)
        || path.components().count() != 1
        || path.file_name().and_then(|name| name.to_str()) != Some(value)
        || path.extension().and_then(|extension| extension.to_str()) != Some("mp4")
    {
        return Err(BridgeError::invalid(
            "inbox_file_name must be one .mp4 file name without a path.",
        ));
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
        .map_err(|_| BridgeError::not_found("The allowlisted inbox video was not found."))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| BridgeError::internal("The inbox video could not be hashed."))?;
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
