use crate::{
    ActionKind, AllowedRoot, ExecutorError, GenerateTestClip, LogEvent, MediaProbe,
    OutputConflictPolicy, PathPolicy, ProbeStream, ScopedPath, SubmitOutcome, TaskAction,
    TaskError, TaskErrorCode, TaskId, TaskRequest, TaskResult, TaskSnapshot, TaskStatus, Toolchain,
    TranscodeToMp4, VerifyMedia, process::run_process, tools::map_process_error,
    types::has_mp4_extension,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const JOURNAL_SCHEMA_VERSION: u32 = 1;
const MIN_TIMEOUT_MS: u64 = 100;
const MAX_TIMEOUT_MS: u64 = 10 * 60 * 1000;
const MAX_EVENTS: usize = 2_000;

pub struct ExecutorConfig {
    pub state_directory: PathBuf,
    pub allowed_roots: Vec<AllowedRoot>,
    pub toolchain: Toolchain,
}

pub struct Executor {
    inner: Arc<Inner>,
    sender: Option<Sender<WorkerMessage>>,
    worker: Option<JoinHandle<()>>,
}

struct Inner {
    state: Mutex<ExecutorState>,
    paths: PathPolicy,
    tools: Toolchain,
    journal_path: PathBuf,
    #[cfg(test)]
    test_hooks: TestHooks,
}

#[cfg(test)]
#[derive(Default)]
struct TestHooks {
    hash_chunk_delay_ms: std::sync::atomic::AtomicU64,
    hash_chunks_seen: std::sync::atomic::AtomicUsize,
    before_publish_delay_ms: std::sync::atomic::AtomicU64,
    before_publish_reached: AtomicBool,
    fail_persistence: AtomicBool,
}

#[derive(Default)]
struct ExecutorState {
    tasks: HashMap<TaskId, TaskRecord>,
    idempotency: HashMap<String, (String, TaskId)>,
    events: Vec<LogEvent>,
}

struct TaskRecord {
    persisted: PersistedTask,
    action: Option<TaskAction>,
    timeout_ms: u64,
    cancelled: Arc<AtomicBool>,
}

#[derive(Clone, Deserialize, Serialize)]
struct PersistedTask {
    id: TaskId,
    idempotency_hash: String,
    request_fingerprint: String,
    status: TaskStatus,
    action: ActionKind,
    result: Option<TaskResult>,
    error: Option<TaskError>,
    created_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Deserialize, Serialize)]
struct Journal {
    schema_version: u32,
    tasks: Vec<PersistedTask>,
}

enum WorkerMessage {
    Run(TaskId),
    Shutdown,
}

impl Executor {
    pub fn new(config: ExecutorConfig) -> Result<Self, ExecutorError> {
        if !config.state_directory.is_absolute() {
            return Err(ExecutorError::InvalidConfiguration(
                "state directory must be absolute",
            ));
        }
        fs::create_dir_all(&config.state_directory).map_err(|_| ExecutorError::StateIo)?;
        let state_directory =
            fs::canonicalize(&config.state_directory).map_err(|_| ExecutorError::StateIo)?;
        let journal_path = state_directory.join("task-state.json");
        let paths = PathPolicy::new(config.allowed_roots)?;
        let mut state = load_state(&journal_path)?;
        recover_interrupted_tasks(&mut state);

        let inner = Arc::new(Inner {
            state: Mutex::new(state),
            paths,
            tools: config.toolchain,
            journal_path,
            #[cfg(test)]
            test_hooks: TestHooks::default(),
        });
        {
            let state = inner.state.lock().map_err(|_| ExecutorError::StateIo)?;
            persist_locked(&inner, &state)?;
        }

        let (sender, receiver) = mpsc::channel();
        let worker_inner = Arc::clone(&inner);
        let worker = thread::Builder::new()
            .name("local-media-executor".to_owned())
            .spawn(move || worker_loop(worker_inner, receiver))
            .map_err(|_| ExecutorError::WorkerUnavailable)?;
        Ok(Self {
            inner,
            sender: Some(sender),
            worker: Some(worker),
        })
    }

    pub fn submit(&self, request: TaskRequest) -> Result<SubmitOutcome, ExecutorError> {
        validate_request_shape(&request)?;
        let key_hash = digest(request.idempotency_key.as_bytes());
        let request_bytes = serde_json::to_vec(&(request.timeout_ms, &request.action))
            .map_err(|_| ExecutorError::InvalidRequest("request could not be fingerprinted"))?;
        let fingerprint = digest(&request_bytes);

        {
            let state = self
                .inner
                .state
                .lock()
                .map_err(|_| ExecutorError::StateIo)?;
            if let Some((existing_fingerprint, task_id)) = state.idempotency.get(&key_hash) {
                return if existing_fingerprint == &fingerprint {
                    Ok(SubmitOutcome::Duplicate(task_id.clone()))
                } else {
                    Err(ExecutorError::IdempotencyConflict)
                };
            }
        }

        validate_action_paths(&self.inner.paths, &request.action)?;
        let now = now_ms();
        let task_id = TaskId::new();
        let action_kind = request.action.kind();
        let record = TaskRecord {
            persisted: PersistedTask {
                id: task_id.clone(),
                idempotency_hash: key_hash.clone(),
                request_fingerprint: fingerprint.clone(),
                status: TaskStatus::Queued,
                action: action_kind,
                result: None,
                error: None,
                created_at_ms: now,
                updated_at_ms: now,
            },
            action: Some(request.action),
            timeout_ms: request.timeout_ms,
            cancelled: Arc::new(AtomicBool::new(false)),
        };

        {
            let mut state = self
                .inner
                .state
                .lock()
                .map_err(|_| ExecutorError::StateIo)?;
            if let Some((existing_fingerprint, existing_task_id)) = state.idempotency.get(&key_hash)
            {
                return if existing_fingerprint == &fingerprint {
                    Ok(SubmitOutcome::Duplicate(existing_task_id.clone()))
                } else {
                    Err(ExecutorError::IdempotencyConflict)
                };
            }
            state
                .idempotency
                .insert(key_hash.clone(), (fingerprint, task_id.clone()));
            state.tasks.insert(task_id.clone(), record);
            push_event(
                &mut state,
                &task_id,
                action_kind,
                TaskStatus::Queued,
                "task_queued",
                None,
            );
            if persist_locked(&self.inner, &state).is_err() {
                state.tasks.remove(&task_id);
                state.idempotency.remove(&key_hash);
                state.events.retain(|event| event.task_id != task_id);
                return Err(ExecutorError::StateIo);
            }
        }

        let sender = self
            .sender
            .as_ref()
            .ok_or(ExecutorError::WorkerUnavailable)?;
        if sender.send(WorkerMessage::Run(task_id.clone())).is_err() {
            mark_worker_unavailable(&self.inner, &task_id)?;
            return Err(ExecutorError::WorkerUnavailable);
        }
        Ok(SubmitOutcome::Accepted(task_id))
    }

    pub fn task(&self, task_id: &TaskId) -> Result<TaskSnapshot, ExecutorError> {
        let state = self
            .inner
            .state
            .lock()
            .map_err(|_| ExecutorError::StateIo)?;
        let record = state
            .tasks
            .get(task_id)
            .ok_or(ExecutorError::TaskNotFound)?;
        Ok(snapshot(&record.persisted))
    }

    pub fn wait(&self, task_id: &TaskId, timeout: Duration) -> Result<TaskSnapshot, ExecutorError> {
        let started = Instant::now();
        loop {
            let snapshot = self.task(task_id)?;
            if snapshot.status.is_terminal() || started.elapsed() >= timeout {
                return Ok(snapshot);
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn cancel(&self, task_id: &TaskId) -> Result<bool, ExecutorError> {
        let mut state = self
            .inner
            .state
            .lock()
            .map_err(|_| ExecutorError::StateIo)?;
        let (action, status, cancelled) = {
            let record = state
                .tasks
                .get_mut(task_id)
                .ok_or(ExecutorError::TaskNotFound)?;
            if record.persisted.status.is_terminal() {
                return Ok(false);
            }
            record.cancelled.store(true, Ordering::SeqCst);
            if record.persisted.status == TaskStatus::Queued {
                record.persisted.status = TaskStatus::Cancelled;
                record.persisted.error = Some(cancelled_error());
                record.persisted.updated_at_ms = now_ms();
            }
            (
                record.persisted.action,
                record.persisted.status,
                Arc::clone(&record.cancelled),
            )
        };
        cancelled.store(true, Ordering::SeqCst);
        push_event(
            &mut state,
            task_id,
            action,
            status,
            "cancellation_requested",
            Some(TaskErrorCode::Cancelled),
        );
        persist_locked(&self.inner, &state)?;
        Ok(true)
    }

    pub fn events(&self) -> Result<Vec<LogEvent>, ExecutorError> {
        let state = self
            .inner
            .state
            .lock()
            .map_err(|_| ExecutorError::StateIo)?;
        Ok(state.events.clone())
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        if let Ok(mut state) = self.inner.state.lock() {
            let now = now_ms();
            for record in state.tasks.values_mut() {
                if !record.persisted.status.is_terminal() {
                    record.cancelled.store(true, Ordering::SeqCst);
                    if record.persisted.status == TaskStatus::Queued {
                        record.persisted.status = TaskStatus::Cancelled;
                        record.persisted.error = Some(cancelled_error());
                        record.persisted.updated_at_ms = now;
                    }
                }
            }
            let _ = persist_locked(&self.inner, &state);
        }
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(WorkerMessage::Shutdown);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn worker_loop(inner: Arc<Inner>, receiver: Receiver<WorkerMessage>) {
    while let Ok(message) = receiver.recv() {
        match message {
            WorkerMessage::Run(task_id) => run_task(&inner, &task_id),
            WorkerMessage::Shutdown => break,
        }
    }
}

fn run_task(inner: &Arc<Inner>, task_id: &TaskId) {
    let (action, timeout, cancelled) = {
        let Ok(mut state) = inner.state.lock() else {
            return;
        };
        let Some(record) = state.tasks.get_mut(task_id) else {
            return;
        };
        if record.persisted.status != TaskStatus::Queued {
            return;
        }
        let Some(action) = record.action.clone() else {
            return;
        };
        record.persisted.status = TaskStatus::Running;
        record.persisted.updated_at_ms = now_ms();
        let action_kind = record.persisted.action;
        let timeout = Duration::from_millis(record.timeout_ms);
        let cancelled = Arc::clone(&record.cancelled);
        if persist_locked(inner, &state).is_err() {
            if let Some(record) = state.tasks.get_mut(task_id) {
                record.persisted.status = TaskStatus::Failed;
                record.persisted.result = None;
                record.persisted.error = Some(state_persistence_error(false));
                record.persisted.updated_at_ms = now_ms();
            }
            push_event(
                &mut state,
                task_id,
                action_kind,
                TaskStatus::Failed,
                "task_start_persist_failed",
                Some(TaskErrorCode::StateIo),
            );
            return;
        }
        push_event(
            &mut state,
            task_id,
            action_kind,
            TaskStatus::Running,
            "task_started",
            None,
        );
        (action, timeout, cancelled)
    };

    let result = execute_action(inner, &action, timeout, &cancelled);
    let output_may_exist = matches!(&result, Ok(TaskResult::MediaCreated { .. }));
    let Ok(mut state) = inner.state.lock() else {
        return;
    };
    let Some(record) = state.tasks.get_mut(task_id) else {
        return;
    };
    match result {
        Ok(result) => {
            record.persisted.status = TaskStatus::Succeeded;
            record.persisted.result = Some(result);
            record.persisted.error = None;
        }
        Err(error) if error.code == TaskErrorCode::Cancelled => {
            record.persisted.status = TaskStatus::Cancelled;
            record.persisted.error = Some(error);
        }
        Err(error) => {
            record.persisted.status = TaskStatus::Failed;
            record.persisted.error = Some(error);
        }
    }
    record.persisted.updated_at_ms = now_ms();
    let action_kind = record.persisted.action;
    let status = record.persisted.status;
    let error_code = record.persisted.error.as_ref().map(|error| error.code);
    if persist_locked(inner, &state).is_err() {
        if let Some(record) = state.tasks.get_mut(task_id) {
            record.persisted.status = TaskStatus::Failed;
            if !output_may_exist {
                record.persisted.result = None;
            }
            record.persisted.error = Some(state_persistence_error(output_may_exist));
            record.persisted.updated_at_ms = now_ms();
        }
        push_event(
            &mut state,
            task_id,
            action_kind,
            TaskStatus::Failed,
            "task_final_persist_failed",
            Some(TaskErrorCode::StateIo),
        );
    } else {
        push_event(
            &mut state,
            task_id,
            action_kind,
            status,
            "task_finished",
            error_code,
        );
    }
}

fn execute_action(
    inner: &Inner,
    action: &TaskAction,
    timeout: Duration,
    cancelled: &Arc<AtomicBool>,
) -> Result<TaskResult, TaskError> {
    let started = Instant::now();
    match action {
        TaskAction::GenerateTestClip(parameters) => {
            execute_generate(inner, parameters, started, timeout, cancelled)
        }
        TaskAction::TranscodeToMp4(parameters) => {
            execute_transcode(inner, parameters, started, timeout, cancelled)
        }
        TaskAction::VerifyMedia(parameters) => {
            execute_verify(inner, parameters, started, timeout, cancelled)
        }
    }
}

fn execute_generate(
    inner: &Inner,
    parameters: &GenerateTestClip,
    started: Instant,
    timeout: Duration,
    cancelled: &Arc<AtomicBool>,
) -> Result<TaskResult, TaskError> {
    let (output, target) =
        prepare_output(&inner.paths, &parameters.output, parameters.conflict_policy)?;
    let temporary = TemporaryOutput::new(temporary_output(&target));
    let duration = format!("{:.3}", f64::from(parameters.duration_ms) / 1000.0);
    let video_source = format!(
        "testsrc2=size={}x{}:rate={}",
        parameters.width, parameters.height, parameters.frame_rate
    );
    let arguments = strings_to_args(&[
        "-hide_banner",
        "-nostdin",
        "-loglevel",
        "error",
        "-n",
        "-f",
        "lavfi",
        "-i",
        &video_source,
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=1000:sample_rate=48000",
        "-t",
        &duration,
        "-shortest",
        "-c:v",
        "mpeg4",
        "-q:v",
        "3",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        "96k",
        "-movflags",
        "+faststart",
    ]);
    let mut arguments = arguments;
    arguments.push(temporary.path().as_os_str().to_owned());
    let run = run_with_deadline(
        inner.tools.ffmpeg(),
        &arguments,
        started,
        timeout,
        cancelled,
    );
    match run {
        Ok(output) if output.success => {}
        Ok(output) => return Err(process_exit_error(output.status_code)),
        Err(error) => return Err(error),
    }

    let probe = probe_and_decode(inner, temporary.path(), started, timeout, cancelled)?;
    if !probe.has_video() || !probe.has_audio() {
        return Err(verification_error(
            "generated sample is missing an expected stream",
        ));
    }
    let sha256 = sha256_file(inner, temporary.path(), started, timeout, cancelled)?;
    check_before_publish(inner, started, timeout, cancelled)?;
    publish_without_overwrite(temporary.path(), &target)?;
    Ok(TaskResult::MediaCreated {
        output,
        sha256,
        probe,
    })
}

fn execute_transcode(
    inner: &Inner,
    parameters: &TranscodeToMp4,
    started: Instant,
    timeout: Duration,
    cancelled: &Arc<AtomicBool>,
) -> Result<TaskResult, TaskError> {
    let input = inner
        .paths
        .resolve_existing_file(&parameters.input)
        .map_err(task_error_from_executor)?;
    let input_probe = probe_only(inner, &input, started, timeout, cancelled)?;
    if !input_probe.has_video() {
        return Err(verification_error("transcode input has no video stream"));
    }
    let (output, target) =
        prepare_output(&inner.paths, &parameters.output, parameters.conflict_policy)?;
    let temporary = TemporaryOutput::new(temporary_output(&target));
    let mut arguments =
        strings_to_args(&["-hide_banner", "-nostdin", "-loglevel", "error", "-n", "-i"]);
    arguments.push(input.as_os_str().to_owned());
    arguments.extend(strings_to_args(&[
        "-map",
        "0:v:0",
        "-map",
        "0:a:0?",
        "-c:v",
        "mpeg4",
        "-q:v",
        "3",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        "128k",
        "-movflags",
        "+faststart",
    ]));
    arguments.push(temporary.path().as_os_str().to_owned());
    let run = run_with_deadline(
        inner.tools.ffmpeg(),
        &arguments,
        started,
        timeout,
        cancelled,
    );
    match run {
        Ok(output) if output.success => {}
        Ok(output) => return Err(process_exit_error(output.status_code)),
        Err(error) => return Err(error),
    }
    let probe = probe_and_decode(inner, temporary.path(), started, timeout, cancelled)?;
    if !probe.has_video() {
        return Err(verification_error("transcode output has no video stream"));
    }
    let sha256 = sha256_file(inner, temporary.path(), started, timeout, cancelled)?;
    check_before_publish(inner, started, timeout, cancelled)?;
    publish_without_overwrite(temporary.path(), &target)?;
    Ok(TaskResult::MediaCreated {
        output,
        sha256,
        probe,
    })
}

fn execute_verify(
    inner: &Inner,
    parameters: &VerifyMedia,
    started: Instant,
    timeout: Duration,
    cancelled: &Arc<AtomicBool>,
) -> Result<TaskResult, TaskError> {
    let input = inner
        .paths
        .resolve_existing_file(&parameters.input)
        .map_err(task_error_from_executor)?;
    let probe = probe_and_decode(inner, &input, started, timeout, cancelled)?;
    let sha256 = sha256_file(inner, &input, started, timeout, cancelled)?;
    Ok(TaskResult::MediaVerified {
        input: parameters.input.clone(),
        sha256,
        probe,
    })
}

fn prepare_output(
    paths: &PathPolicy,
    requested: &ScopedPath,
    policy: OutputConflictPolicy,
) -> Result<(ScopedPath, PathBuf), TaskError> {
    for suffix in 0..10_000_u32 {
        let scoped = if suffix == 0 {
            requested.clone()
        } else {
            let stem = requested
                .relative
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| verification_error("invalid output file name"))?;
            let parent = requested.relative.parent().unwrap_or_else(|| Path::new(""));
            requested
                .with_relative(parent.join(format!("{stem}-{suffix}.mp4")))
                .map_err(task_error_from_executor)?
        };
        let absolute = paths
            .resolve_output(&scoped)
            .map_err(task_error_from_executor)?;
        if !absolute.exists() {
            return Ok((scoped, absolute));
        }
        if policy == OutputConflictPolicy::Reject {
            return Err(TaskError::new(
                TaskErrorCode::OutputConflict,
                "output already exists",
                None,
                false,
            ));
        }
    }
    Err(TaskError::new(
        TaskErrorCode::OutputConflict,
        "no unique output name is available",
        None,
        false,
    ))
}

fn temporary_output(target: &Path) -> PathBuf {
    let file_name = target
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("output");
    target.with_file_name(format!(
        ".{file_name}.local-executor-{}.part.mp4",
        Uuid::new_v4()
    ))
}

struct TemporaryOutput {
    path: PathBuf,
}

impl TemporaryOutput {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporaryOutput {
    fn drop(&mut self) {
        remove_if_present(&self.path);
    }
}

fn check_before_publish(
    _inner: &Inner,
    started: Instant,
    timeout: Duration,
    cancelled: &Arc<AtomicBool>,
) -> Result<(), TaskError> {
    #[cfg(test)]
    {
        _inner
            .test_hooks
            .before_publish_reached
            .store(true, Ordering::SeqCst);
        let delay_ms = _inner
            .test_hooks
            .before_publish_delay_ms
            .load(Ordering::SeqCst);
        if delay_ms > 0 {
            thread::sleep(Duration::from_millis(delay_ms));
        }
    }
    check_execution_budget(started, timeout, cancelled)
}

fn publish_without_overwrite(temporary: &Path, target: &Path) -> Result<(), TaskError> {
    match fs::hard_link(temporary, target) {
        Ok(()) => {
            remove_if_present(temporary);
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            remove_if_present(temporary);
            Err(TaskError::new(
                TaskErrorCode::OutputConflict,
                "output appeared before publish",
                None,
                false,
            ))
        }
        Err(_) => {
            remove_if_present(temporary);
            Err(TaskError::new(
                TaskErrorCode::Internal,
                "verified output could not be published",
                None,
                true,
            ))
        }
    }
}

fn probe_and_decode(
    inner: &Inner,
    input: &Path,
    started: Instant,
    timeout: Duration,
    cancelled: &Arc<AtomicBool>,
) -> Result<MediaProbe, TaskError> {
    let probe = probe_only(inner, input, started, timeout, cancelled)?;
    let mut arguments =
        strings_to_args(&["-hide_banner", "-nostdin", "-v", "error", "-xerror", "-i"]);
    arguments.push(input.as_os_str().to_owned());
    arguments.extend(strings_to_args(&["-map", "0", "-f", "null", "-"]));
    let output = run_with_deadline(
        inner.tools.ffmpeg(),
        &arguments,
        started,
        timeout,
        cancelled,
    )?;
    if !output.success {
        return Err(verification_error_with_exit(
            "full media decode failed",
            output.status_code,
        ));
    }
    Ok(probe)
}

fn probe_only(
    inner: &Inner,
    input: &Path,
    started: Instant,
    timeout: Duration,
    cancelled: &Arc<AtomicBool>,
) -> Result<MediaProbe, TaskError> {
    let mut arguments = strings_to_args(&[
        "-v",
        "error",
        "-show_entries",
        "format=duration:stream=index,codec_type,codec_name,width,height,sample_rate,channels",
        "-of",
        "json",
    ]);
    arguments.push(input.as_os_str().to_owned());
    let output = run_with_deadline(
        inner.tools.ffprobe(),
        &arguments,
        started,
        timeout,
        cancelled,
    )?;
    if !output.success {
        return Err(verification_error_with_exit(
            "ffprobe rejected the media",
            output.status_code,
        ));
    }
    parse_probe(&output.stdout)
}

#[derive(Deserialize)]
struct RawProbe {
    #[serde(default)]
    streams: Vec<RawProbeStream>,
    format: Option<RawProbeFormat>,
}

#[derive(Deserialize)]
struct RawProbeStream {
    index: u32,
    codec_type: String,
    codec_name: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    sample_rate: Option<String>,
    channels: Option<u32>,
}

#[derive(Deserialize)]
struct RawProbeFormat {
    duration: Option<String>,
}

fn parse_probe(bytes: &[u8]) -> Result<MediaProbe, TaskError> {
    let raw: RawProbe = serde_json::from_slice(bytes)
        .map_err(|_| verification_error("ffprobe returned invalid JSON"))?;
    if raw.streams.is_empty() {
        return Err(verification_error("media contains no streams"));
    }
    let duration_ms = raw
        .format
        .and_then(|format| format.duration)
        .and_then(|duration| duration.parse::<f64>().ok())
        .filter(|duration| duration.is_finite() && *duration >= 0.0)
        .map(|duration| (duration * 1000.0).round() as u64);
    let streams = raw
        .streams
        .into_iter()
        .map(|stream| ProbeStream {
            index: stream.index,
            codec_type: stream.codec_type,
            codec_name: stream.codec_name,
            width: stream.width,
            height: stream.height,
            sample_rate: stream.sample_rate.and_then(|rate| rate.parse().ok()),
            channels: stream.channels,
        })
        .collect();
    Ok(MediaProbe {
        duration_ms,
        streams,
    })
}

fn run_with_deadline(
    program: &Path,
    arguments: &[OsString],
    started: Instant,
    timeout: Duration,
    cancelled: &Arc<AtomicBool>,
) -> Result<crate::process::ProcessOutput, TaskError> {
    check_execution_budget(started, timeout, cancelled)?;
    let remaining = timeout
        .checked_sub(started.elapsed())
        .ok_or_else(timeout_error)?;
    if remaining.is_zero() {
        return Err(timeout_error());
    }
    run_process(program, arguments, remaining, cancelled).map_err(map_process_error)
}

fn validate_request_shape(request: &TaskRequest) -> Result<(), ExecutorError> {
    if request.idempotency_key.is_empty()
        || request.idempotency_key.len() > 256
        || request.idempotency_key.chars().any(char::is_control)
    {
        return Err(ExecutorError::InvalidRequest("invalid idempotency key"));
    }
    if !(MIN_TIMEOUT_MS..=MAX_TIMEOUT_MS).contains(&request.timeout_ms) {
        return Err(ExecutorError::InvalidRequest(
            "timeout is outside the allowed range",
        ));
    }
    match &request.action {
        TaskAction::GenerateTestClip(parameters) => {
            parameters.output.validate()?;
            if !(100..=10_000).contains(&parameters.duration_ms)
                || !(16..=4_096).contains(&parameters.width)
                || !(16..=4_096).contains(&parameters.height)
                || parameters.width % 2 != 0
                || parameters.height % 2 != 0
                || !(1..=120).contains(&parameters.frame_rate)
                || !has_mp4_extension(&parameters.output.relative)
            {
                return Err(ExecutorError::InvalidRequest(
                    "invalid deterministic clip parameters",
                ));
            }
        }
        TaskAction::TranscodeToMp4(parameters) => {
            parameters.input.validate()?;
            parameters.output.validate()?;
            if !has_mp4_extension(&parameters.output.relative) {
                return Err(ExecutorError::InvalidRequest("output must be an mp4 file"));
            }
        }
        TaskAction::VerifyMedia(parameters) => parameters.input.validate()?,
    }
    Ok(())
}

fn validate_action_paths(paths: &PathPolicy, action: &TaskAction) -> Result<(), ExecutorError> {
    match action {
        TaskAction::GenerateTestClip(parameters) => {
            let output = paths.resolve_output(&parameters.output)?;
            if parameters.conflict_policy == OutputConflictPolicy::Reject && output.exists() {
                return Err(ExecutorError::OutputConflict);
            }
        }
        TaskAction::TranscodeToMp4(parameters) => {
            paths.resolve_existing_file(&parameters.input)?;
            let output = paths.resolve_output(&parameters.output)?;
            if parameters.conflict_policy == OutputConflictPolicy::Reject && output.exists() {
                return Err(ExecutorError::OutputConflict);
            }
        }
        TaskAction::VerifyMedia(parameters) => {
            paths.resolve_existing_file(&parameters.input)?;
        }
    }
    Ok(())
}

fn load_state(path: &Path) -> Result<ExecutorState, ExecutorError> {
    if !path.exists() {
        return Ok(ExecutorState::default());
    }
    let bytes = fs::read(path).map_err(|_| ExecutorError::StateIo)?;
    let journal: Journal = serde_json::from_slice(&bytes).map_err(|_| ExecutorError::StateIo)?;
    if journal.schema_version != JOURNAL_SCHEMA_VERSION {
        return Err(ExecutorError::InvalidConfiguration(
            "unsupported journal schema",
        ));
    }
    let mut state = ExecutorState::default();
    for persisted in journal.tasks {
        if state.tasks.contains_key(&persisted.id)
            || state.idempotency.contains_key(&persisted.idempotency_hash)
        {
            return Err(ExecutorError::StateIo);
        }
        state.idempotency.insert(
            persisted.idempotency_hash.clone(),
            (persisted.request_fingerprint.clone(), persisted.id.clone()),
        );
        state.tasks.insert(
            persisted.id.clone(),
            TaskRecord {
                persisted,
                action: None,
                timeout_ms: 0,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        );
    }
    Ok(state)
}

fn recover_interrupted_tasks(state: &mut ExecutorState) {
    let now = now_ms();
    let mut recovered = Vec::new();
    for record in state.tasks.values_mut() {
        if matches!(
            record.persisted.status,
            TaskStatus::Queued | TaskStatus::Running
        ) {
            record.persisted.status = TaskStatus::Failed;
            record.persisted.error = Some(TaskError::new(
                TaskErrorCode::InterruptedByRestart,
                "task was interrupted by executor restart and was not replayed",
                None,
                true,
            ));
            record.persisted.updated_at_ms = now;
            recovered.push((record.persisted.id.clone(), record.persisted.action));
        }
    }
    for (task_id, action) in recovered {
        push_event(
            state,
            &task_id,
            action,
            TaskStatus::Failed,
            "task_recovered_as_failed",
            Some(TaskErrorCode::InterruptedByRestart),
        );
    }
}

fn persist_locked(inner: &Inner, state: &ExecutorState) -> Result<(), ExecutorError> {
    #[cfg(test)]
    if inner.test_hooks.fail_persistence.load(Ordering::SeqCst) {
        return Err(ExecutorError::StateIo);
    }
    let mut tasks = state
        .tasks
        .values()
        .map(|record| record.persisted.clone())
        .collect::<Vec<_>>();
    tasks.sort_by(|left, right| {
        left.created_at_ms
            .cmp(&right.created_at_ms)
            .then_with(|| left.id.as_str().cmp(right.id.as_str()))
    });
    let journal = Journal {
        schema_version: JOURNAL_SCHEMA_VERSION,
        tasks,
    };
    let bytes = serde_json::to_vec_pretty(&journal).map_err(|_| ExecutorError::StateIo)?;
    let parent = inner.journal_path.parent().ok_or(ExecutorError::StateIo)?;
    let temporary = parent.join(format!(".task-state-{}.tmp", Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|_| ExecutorError::StateIo)?;
    file.write_all(&bytes).map_err(|_| ExecutorError::StateIo)?;
    file.sync_all().map_err(|_| ExecutorError::StateIo)?;
    drop(file);
    fs::rename(&temporary, &inner.journal_path).map_err(|_| {
        let _ = fs::remove_file(&temporary);
        ExecutorError::StateIo
    })?;
    Ok(())
}

fn snapshot(task: &PersistedTask) -> TaskSnapshot {
    TaskSnapshot {
        id: task.id.clone(),
        status: task.status,
        action: task.action,
        result: task.result.clone(),
        error: task.error.clone(),
        created_at_ms: task.created_at_ms,
        updated_at_ms: task.updated_at_ms,
    }
}

fn mark_worker_unavailable(inner: &Inner, task_id: &TaskId) -> Result<(), ExecutorError> {
    let mut state = inner.state.lock().map_err(|_| ExecutorError::StateIo)?;
    let record = state
        .tasks
        .get_mut(task_id)
        .ok_or(ExecutorError::TaskNotFound)?;
    record.persisted.status = TaskStatus::Failed;
    record.persisted.error = Some(TaskError::new(
        TaskErrorCode::Internal,
        "executor worker is unavailable",
        None,
        true,
    ));
    record.persisted.updated_at_ms = now_ms();
    persist_locked(inner, &state)
}

fn push_event(
    state: &mut ExecutorState,
    task_id: &TaskId,
    action: ActionKind,
    status: TaskStatus,
    event: &str,
    error_code: Option<TaskErrorCode>,
) {
    state.events.push(LogEvent {
        task_id: task_id.clone(),
        action,
        status,
        event: event.to_owned(),
        error_code,
        timestamp_ms: now_ms(),
    });
    if state.events.len() > MAX_EVENTS {
        state.events.drain(..state.events.len() - MAX_EVENTS);
    }
}

fn sha256_file(
    _inner: &Inner,
    path: &Path,
    started: Instant,
    timeout: Duration,
    cancelled: &Arc<AtomicBool>,
) -> Result<String, TaskError> {
    check_execution_budget(started, timeout, cancelled)?;
    let mut file =
        fs::File::open(path).map_err(|_| verification_error("media hash could not be read"))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        check_execution_budget(started, timeout, cancelled)?;
        let read = file
            .read(&mut buffer)
            .map_err(|_| verification_error("media hash could not be read"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        #[cfg(test)]
        {
            _inner
                .test_hooks
                .hash_chunks_seen
                .fetch_add(1, Ordering::SeqCst);
            let delay_ms = _inner.test_hooks.hash_chunk_delay_ms.load(Ordering::SeqCst);
            if delay_ms > 0 {
                thread::sleep(Duration::from_millis(delay_ms));
            }
        }
        check_execution_budget(started, timeout, cancelled)?;
    }
    check_execution_budget(started, timeout, cancelled)?;
    Ok(hex::encode(hasher.finalize()))
}

fn check_execution_budget(
    started: Instant,
    timeout: Duration,
    cancelled: &Arc<AtomicBool>,
) -> Result<(), TaskError> {
    if cancelled.load(Ordering::SeqCst) {
        return Err(cancelled_error());
    }
    if started.elapsed() >= timeout {
        return Err(timeout_error());
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn strings_to_args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn remove_if_present(path: &Path) {
    let _ = fs::remove_file(path);
}

fn process_exit_error(exit_code: Option<i32>) -> TaskError {
    TaskError::new(
        TaskErrorCode::ProcessExit,
        "media tool exited unsuccessfully",
        exit_code,
        true,
    )
}

fn timeout_error() -> TaskError {
    TaskError::new(
        TaskErrorCode::Timeout,
        "task exceeded its total timeout",
        None,
        true,
    )
}

fn state_persistence_error(side_effects_may_exist: bool) -> TaskError {
    let error = TaskError::new(
        TaskErrorCode::StateIo,
        "task state could not be durably persisted",
        None,
        true,
    );
    if side_effects_may_exist {
        error.with_possible_side_effects()
    } else {
        error
    }
}

fn cancelled_error() -> TaskError {
    TaskError::new(TaskErrorCode::Cancelled, "task was cancelled", None, false)
}

fn verification_error(message: &'static str) -> TaskError {
    TaskError::new(TaskErrorCode::VerificationFailed, message, None, false)
}

fn verification_error_with_exit(message: &'static str, exit_code: Option<i32>) -> TaskError {
    TaskError::new(TaskErrorCode::VerificationFailed, message, exit_code, false)
}

fn task_error_from_executor(error: ExecutorError) -> TaskError {
    TaskError::new(error.code(), error.to_string(), None, false)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RootId, ToolDiscoveryConfig};
    use std::time::Duration;
    use tempfile::tempdir;

    #[cfg(unix)]
    fn write_executable(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        fs::write(path, body).unwrap();
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }

    #[cfg(unix)]
    fn mock_executor(ffmpeg_body: &str) -> (Executor, tempfile::TempDir, RootId) {
        mock_executor_with_probe(
            ffmpeg_body,
            r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffprobe version mock-1"; exit 0; fi
echo '{"streams":[{"index":0,"codec_type":"video","codec_name":"mpeg4","width":16,"height":16},{"index":1,"codec_type":"audio","codec_name":"aac","sample_rate":"48000","channels":1}],"format":{"duration":"0.100"}}'
"#,
        )
    }

    #[cfg(unix)]
    fn mock_executor_with_probe(
        ffmpeg_body: &str,
        ffprobe_body: &str,
    ) -> (Executor, tempfile::TempDir, RootId) {
        let directory = tempdir().unwrap();
        let tools = directory.path().join("tools");
        let root = directory.path().join("root");
        let state = directory.path().join("state");
        fs::create_dir_all(&tools).unwrap();
        fs::create_dir_all(&root).unwrap();
        write_executable(&tools.join("ffmpeg"), ffmpeg_body);
        write_executable(&tools.join("ffprobe"), ffprobe_body);
        let toolchain = Toolchain::discover(ToolDiscoveryConfig {
            trusted_directories: vec![tools],
            version_timeout: Duration::from_secs(30),
        })
        .unwrap();
        let root_id = RootId::new("test").unwrap();
        let executor = Executor::new(ExecutorConfig {
            state_directory: state,
            allowed_roots: vec![AllowedRoot::new(root_id.clone(), root).unwrap()],
            toolchain,
        })
        .unwrap();
        (executor, directory, root_id)
    }

    fn generate_request(root: RootId, name: &str, key: &str, timeout_ms: u64) -> TaskRequest {
        TaskRequest {
            idempotency_key: key.to_owned(),
            timeout_ms,
            action: TaskAction::GenerateTestClip(GenerateTestClip {
                output: ScopedPath::new(root, name).unwrap(),
                duration_ms: 100,
                width: 16,
                height: 16,
                frame_rate: 1,
                conflict_policy: OutputConflictPolicy::Reject,
            }),
        }
    }

    fn verify_request(root: RootId, name: &str, key: &str, timeout_ms: u64) -> TaskRequest {
        TaskRequest {
            idempotency_key: key.to_owned(),
            timeout_ms,
            action: TaskAction::VerifyMedia(VerifyMedia {
                input: ScopedPath::new(root, name).unwrap(),
            }),
        }
    }

    fn wait_until(mut condition: impl FnMut() -> bool) {
        for _ in 0..300 {
            if condition() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("condition was not reached before the test deadline");
    }

    #[cfg(unix)]
    #[test]
    fn duplicate_key_returns_stable_id_and_changed_request_conflicts() {
        let script = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffmpeg version mock-1"; exit 0; fi
last=""
for arg in "$@"; do last="$arg"; done
if [ "$last" != "-" ]; then : > "$last"; fi
"#;
        let (executor, _directory, root) = mock_executor(script);
        let request = generate_request(root.clone(), "clip.mp4", "stable-key", 5_000);
        let first = executor.submit(request.clone()).unwrap();
        let duplicate = executor.submit(request).unwrap();
        assert!(matches!(first, SubmitOutcome::Accepted(_)));
        assert!(matches!(duplicate, SubmitOutcome::Duplicate(_)));
        assert_eq!(first.task_id(), duplicate.task_id());

        let changed = generate_request(root, "different.mp4", "stable-key", 5_000);
        assert!(matches!(
            executor.submit(changed),
            Err(ExecutorError::IdempotencyConflict)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_duplicate_submissions_create_exactly_one_task() {
        use std::sync::Barrier;

        let script = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffmpeg version mock-1"; exit 0; fi
last=""
for arg in "$@"; do last="$arg"; done
if [ "$last" != "-" ]; then : > "$last"; fi
"#;
        let (executor, _directory, root) = mock_executor(script);
        let executor = Arc::new(executor);
        let barrier = Arc::new(Barrier::new(8));
        let mut threads = Vec::new();
        for _ in 0..8 {
            let executor = Arc::clone(&executor);
            let barrier = Arc::clone(&barrier);
            let request = generate_request(root.clone(), "clip.mp4", "concurrent-key", 5_000);
            threads.push(thread::spawn(move || {
                barrier.wait();
                executor.submit(request).unwrap()
            }));
        }
        let outcomes = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, SubmitOutcome::Accepted(_)))
                .count(),
            1
        );
        let stable_id = outcomes[0].task_id();
        assert!(
            outcomes
                .iter()
                .all(|outcome| outcome.task_id() == stable_id)
        );
    }

    #[cfg(unix)]
    #[test]
    fn running_task_can_be_cancelled() {
        let script = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffmpeg version mock-1"; exit 0; fi
exec /bin/sleep 5
"#;
        let (executor, _directory, root) = mock_executor(script);
        let outcome = executor
            .submit(generate_request(root, "slow.mp4", "cancel-key", 10_000))
            .unwrap();
        let id = outcome.task_id().clone();
        for _ in 0..100 {
            if executor.task(&id).unwrap().status == TaskStatus::Running {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(executor.cancel(&id).unwrap());
        let snapshot = executor.wait(&id, Duration::from_secs(2)).unwrap();
        assert_eq!(snapshot.status, TaskStatus::Cancelled);
        assert_eq!(snapshot.error.unwrap().code, TaskErrorCode::Cancelled);
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_interrupts_hashing_after_decode() {
        let script = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffmpeg version mock-1"; exit 0; fi
exit 0
"#;
        let (executor, directory, root) = mock_executor(script);
        let input = directory.path().join("root/large.mp4");
        fs::File::create(&input)
            .unwrap()
            .set_len(512 * 1024 * 1024)
            .unwrap();
        executor
            .inner
            .test_hooks
            .hash_chunk_delay_ms
            .store(2, Ordering::SeqCst);
        let id = executor
            .submit(verify_request(root, "large.mp4", "hash-cancel-key", 5_000))
            .unwrap()
            .task_id()
            .clone();
        wait_until(|| {
            executor
                .inner
                .test_hooks
                .hash_chunks_seen
                .load(Ordering::SeqCst)
                >= 10
        });
        assert!(executor.cancel(&id).unwrap());
        let snapshot = executor.wait(&id, Duration::from_secs(2)).unwrap();
        assert_eq!(snapshot.status, TaskStatus::Cancelled);
        assert_eq!(snapshot.error.unwrap().code, TaskErrorCode::Cancelled);
    }

    #[cfg(unix)]
    #[test]
    fn total_timeout_interrupts_hashing_after_decode() {
        let script = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffmpeg version mock-1"; exit 0; fi
exit 0
"#;
        let (executor, directory, root) = mock_executor(script);
        let input = directory.path().join("root/large.mp4");
        fs::File::create(&input)
            .unwrap()
            .set_len(8 * 1024 * 1024)
            .unwrap();
        executor
            .inner
            .test_hooks
            .hash_chunk_delay_ms
            .store(2, Ordering::SeqCst);
        let id = executor
            .submit(verify_request(root, "large.mp4", "hash-timeout-key", 100))
            .unwrap()
            .task_id()
            .clone();
        let snapshot = executor.wait(&id, Duration::from_secs(2)).unwrap();
        assert_eq!(snapshot.status, TaskStatus::Failed);
        assert_eq!(snapshot.error.unwrap().code, TaskErrorCode::Timeout);
        assert!(
            executor
                .inner
                .test_hooks
                .hash_chunks_seen
                .load(Ordering::SeqCst)
                > 0
        );
    }

    #[cfg(unix)]
    #[test]
    fn cancellation_before_publish_does_not_create_target() {
        let script = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffmpeg version mock-1"; exit 0; fi
last=""
for arg in "$@"; do last="$arg"; done
if [ "$last" != "-" ]; then : > "$last"; fi
"#;
        let (executor, directory, root) = mock_executor(script);
        executor
            .inner
            .test_hooks
            .before_publish_delay_ms
            .store(200, Ordering::SeqCst);
        let id = executor
            .submit(generate_request(
                root,
                "publish-cancel.mp4",
                "publish-cancel-key",
                5_000,
            ))
            .unwrap()
            .task_id()
            .clone();
        wait_until(|| {
            executor
                .inner
                .test_hooks
                .before_publish_reached
                .load(Ordering::SeqCst)
        });
        assert!(executor.cancel(&id).unwrap());
        let snapshot = executor.wait(&id, Duration::from_secs(2)).unwrap();
        assert_eq!(snapshot.status, TaskStatus::Cancelled);
        assert!(!directory.path().join("root/publish-cancel.mp4").exists());
        assert_eq!(
            fs::read_dir(directory.path().join("root")).unwrap().count(),
            0
        );
    }

    #[cfg(unix)]
    #[test]
    fn total_timeout_before_publish_does_not_create_target() {
        let script = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffmpeg version mock-1"; exit 0; fi
last=""
for arg in "$@"; do last="$arg"; done
if [ "$last" != "-" ]; then : > "$last"; fi
"#;
        let (executor, directory, root) = mock_executor(script);
        executor
            .inner
            .test_hooks
            .before_publish_delay_ms
            .store(200, Ordering::SeqCst);
        let id = executor
            .submit(generate_request(
                root,
                "publish-timeout.mp4",
                "publish-timeout-key",
                100,
            ))
            .unwrap()
            .task_id()
            .clone();
        let snapshot = executor.wait(&id, Duration::from_secs(2)).unwrap();
        assert_eq!(snapshot.status, TaskStatus::Failed);
        assert_eq!(snapshot.error.unwrap().code, TaskErrorCode::Timeout);
        assert!(!directory.path().join("root/publish-timeout.mp4").exists());
        assert_eq!(
            fs::read_dir(directory.path().join("root")).unwrap().count(),
            0
        );
    }

    #[cfg(unix)]
    #[test]
    fn queued_task_can_be_cancelled_without_starting() {
        let script = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffmpeg version mock-1"; exit 0; fi
exec /bin/sleep 5
"#;
        let (executor, _directory, root) = mock_executor(script);
        let running = executor
            .submit(generate_request(
                root.clone(),
                "running.mp4",
                "running-key",
                10_000,
            ))
            .unwrap()
            .task_id()
            .clone();
        for _ in 0..100 {
            if executor.task(&running).unwrap().status == TaskStatus::Running {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let queued = executor
            .submit(generate_request(root, "queued.mp4", "queued-key", 10_000))
            .unwrap()
            .task_id()
            .clone();
        assert_eq!(executor.task(&queued).unwrap().status, TaskStatus::Queued);
        assert!(executor.cancel(&queued).unwrap());
        assert_eq!(
            executor.task(&queued).unwrap().status,
            TaskStatus::Cancelled
        );
        assert!(executor.cancel(&running).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn timeout_and_process_exit_are_structured() {
        let slow = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffmpeg version mock-1"; exit 0; fi
exec /bin/sleep 5
"#;
        let (executor, _directory, root) = mock_executor(slow);
        let id = executor
            .submit(generate_request(root, "timeout.mp4", "timeout-key", 100))
            .unwrap()
            .task_id()
            .clone();
        let snapshot = executor.wait(&id, Duration::from_secs(2)).unwrap();
        assert_eq!(snapshot.status, TaskStatus::Failed);
        assert_eq!(snapshot.error.unwrap().code, TaskErrorCode::Timeout);

        let failed = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffmpeg version mock-1"; exit 0; fi
exit 23
"#;
        let (executor, _directory, root) = mock_executor(failed);
        let id = executor
            .submit(generate_request(root, "failed.mp4", "exit-key", 1_000))
            .unwrap()
            .task_id()
            .clone();
        let snapshot = executor.wait(&id, Duration::from_secs(2)).unwrap();
        assert_eq!(snapshot.status, TaskStatus::Failed);
        let error = snapshot.error.unwrap();
        assert_eq!(error.code, TaskErrorCode::ProcessExit);
        assert_eq!(error.exit_code, Some(23));
    }

    #[cfg(unix)]
    #[test]
    fn verification_failure_removes_partial_output() {
        let ffmpeg = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffmpeg version mock-1"; exit 0; fi
last=""
for arg in "$@"; do last="$arg"; done
if [ "$last" != "-" ]; then : > "$last"; fi
"#;
        let ffprobe = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffprobe version mock-1"; exit 0; fi
exit 7
"#;
        let (executor, directory, root) = mock_executor_with_probe(ffmpeg, ffprobe);
        let id = executor
            .submit(generate_request(
                root,
                "verification-fails.mp4",
                "verification-key",
                1_000,
            ))
            .unwrap()
            .task_id()
            .clone();
        let snapshot = executor.wait(&id, Duration::from_secs(2)).unwrap();
        assert_eq!(snapshot.status, TaskStatus::Failed);
        assert_eq!(
            snapshot.error.unwrap().code,
            TaskErrorCode::VerificationFailed
        );
        assert_eq!(
            fs::read_dir(directory.path().join("root")).unwrap().count(),
            0
        );
    }

    #[cfg(unix)]
    #[test]
    fn persistence_failures_block_start_and_downgrade_unstable_success() {
        let script = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffmpeg version mock-1"; exit 0; fi
last=""
for arg in "$@"; do last="$arg"; done
if [ "$last" != "-" ]; then
  : > "$0.started"
  while [ ! -f "$0.gate" ]; do sleep 0.01; done
  : > "$last"
fi
"#;
        let (executor, directory, root) = mock_executor(script);
        let first = executor
            .submit(generate_request(
                root.clone(),
                "first.mp4",
                "persist-first-key",
                5_000,
            ))
            .unwrap()
            .task_id()
            .clone();
        let started_marker = directory.path().join("tools/ffmpeg.started");
        wait_until(|| started_marker.exists());
        let second = executor
            .submit(generate_request(
                root.clone(),
                "second.mp4",
                "persist-second-key",
                5_000,
            ))
            .unwrap()
            .task_id()
            .clone();
        assert_eq!(executor.task(&second).unwrap().status, TaskStatus::Queued);

        executor
            .inner
            .test_hooks
            .fail_persistence
            .store(true, Ordering::SeqCst);
        fs::write(directory.path().join("tools/ffmpeg.gate"), b"continue").unwrap();

        let first_snapshot = executor.wait(&first, Duration::from_secs(2)).unwrap();
        let second_snapshot = executor.wait(&second, Duration::from_secs(2)).unwrap();
        assert_eq!(first_snapshot.status, TaskStatus::Failed);
        assert_eq!(
            first_snapshot.error.as_ref().unwrap().code,
            TaskErrorCode::StateIo
        );
        assert!(
            first_snapshot
                .error
                .as_ref()
                .unwrap()
                .side_effects_may_exist
        );
        assert!(first_snapshot.result.is_some());
        assert!(directory.path().join("root/first.mp4").is_file());

        assert_eq!(second_snapshot.status, TaskStatus::Failed);
        assert_eq!(
            second_snapshot.error.as_ref().unwrap().code,
            TaskErrorCode::StateIo
        );
        assert!(!second_snapshot.error.unwrap().side_effects_may_exist);
        assert!(second_snapshot.result.is_none());
        assert!(!directory.path().join("root/second.mp4").exists());
        let events = executor.events().unwrap();
        assert!(
            events
                .iter()
                .any(|event| event.event == "task_final_persist_failed")
        );
        assert!(
            events
                .iter()
                .any(|event| event.event == "task_start_persist_failed")
        );

        drop(executor);
        let toolchain = Toolchain::discover(ToolDiscoveryConfig {
            trusted_directories: vec![directory.path().join("tools")],
            version_timeout: Duration::from_secs(30),
        })
        .unwrap();
        let reopened = Executor::new(ExecutorConfig {
            state_directory: directory.path().join("state"),
            allowed_roots: vec![AllowedRoot::new(root, directory.path().join("root")).unwrap()],
            toolchain,
        })
        .unwrap();
        for task_id in [&first, &second] {
            let recovered = reopened.task(task_id).unwrap();
            assert_eq!(recovered.status, TaskStatus::Failed);
            assert_eq!(
                recovered.error.unwrap().code,
                TaskErrorCode::InterruptedByRestart
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn failed_submission_persistence_rolls_back_in_memory_registration() {
        let script = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffmpeg version mock-1"; exit 0; fi
exit 0
"#;
        let (executor, _directory, root) = mock_executor(script);
        executor
            .inner
            .test_hooks
            .fail_persistence
            .store(true, Ordering::SeqCst);
        let result = executor.submit(generate_request(
            root,
            "never-queued.mp4",
            "submit-state-io-key",
            1_000,
        ));
        assert!(matches!(result, Err(ExecutorError::StateIo)));
        let state = executor.inner.state.lock().unwrap();
        assert!(state.tasks.is_empty());
        assert!(state.idempotency.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn structured_events_do_not_log_paths_or_arguments() {
        let failed = r#"#!/bin/sh
if [ "$1" = "-version" ]; then echo "ffmpeg version mock-1"; exit 0; fi
exit 23
"#;
        let (executor, directory, root) = mock_executor(failed);
        let id = executor
            .submit(generate_request(
                root,
                "private-client-name.mp4",
                "secret-key-value",
                1_000,
            ))
            .unwrap()
            .task_id()
            .clone();
        let _ = executor.wait(&id, Duration::from_secs(2)).unwrap();
        let logs = serde_json::to_string(&executor.events().unwrap()).unwrap();
        assert!(!logs.contains("private-client-name"));
        assert!(!logs.contains("secret-key-value"));
        let journal = fs::read_to_string(directory.path().join("state/task-state.json")).unwrap();
        assert!(!journal.contains("private-client-name"));
        assert!(!journal.contains("secret-key-value"));
    }

    #[test]
    fn interrupted_records_recover_as_failed_without_replay() {
        let id = TaskId::new();
        let persisted = PersistedTask {
            id: id.clone(),
            idempotency_hash: "key".to_owned(),
            request_fingerprint: "request".to_owned(),
            status: TaskStatus::Running,
            action: ActionKind::GenerateTestClip,
            result: None,
            error: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        };
        let mut state = ExecutorState::default();
        state
            .idempotency
            .insert("key".to_owned(), ("request".to_owned(), id.clone()));
        state.tasks.insert(
            id.clone(),
            TaskRecord {
                persisted,
                action: None,
                timeout_ms: 0,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        );
        recover_interrupted_tasks(&mut state);
        let recovered = &state.tasks[&id].persisted;
        assert_eq!(recovered.status, TaskStatus::Failed);
        assert_eq!(
            recovered.error.as_ref().unwrap().code,
            TaskErrorCode::InterruptedByRestart
        );
    }
}
