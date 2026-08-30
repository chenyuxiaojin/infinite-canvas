use crate::ExecutorError;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct TaskId(String);

impl TaskId {
    pub(crate) fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct RootId(String);

impl RootId {
    pub fn new(value: impl Into<String>) -> Result<Self, ExecutorError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        if !valid {
            return Err(ExecutorError::InvalidConfiguration("invalid root id"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScopedPath {
    pub root: RootId,
    pub relative: PathBuf,
}

impl ScopedPath {
    pub fn new(root: RootId, relative: impl Into<PathBuf>) -> Result<Self, ExecutorError> {
        let scoped = Self {
            root,
            relative: relative.into(),
        };
        scoped.validate()?;
        Ok(scoped)
    }

    pub(crate) fn validate(&self) -> Result<(), ExecutorError> {
        if self.relative.as_os_str().is_empty() || self.relative.is_absolute() {
            return Err(ExecutorError::PathDenied);
        }
        for component in self.relative.components() {
            let Component::Normal(value) = component else {
                return Err(ExecutorError::PathDenied);
            };
            let value = value.to_str().ok_or(ExecutorError::PathDenied)?;
            if value.chars().any(is_forbidden_path_character) {
                return Err(ExecutorError::PathDenied);
            }
        }
        Ok(())
    }

    pub(crate) fn with_relative(&self, relative: PathBuf) -> Result<Self, ExecutorError> {
        Self::new(self.root.clone(), relative)
    }
}

fn is_forbidden_path_character(character: char) -> bool {
    matches!(
        character,
        '\0' | '\n' | '\r' | ';' | '&' | '|' | '`' | '$' | '>' | '<' | '*' | '?' | '[' | ']' | '\\'
    )
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputConflictPolicy {
    Reject,
    UniqueSuffix,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GenerateTestClip {
    pub output: ScopedPath,
    pub duration_ms: u32,
    pub width: u16,
    pub height: u16,
    pub frame_rate: u16,
    pub conflict_policy: OutputConflictPolicy,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TranscodeToMp4 {
    pub input: ScopedPath,
    pub output: ScopedPath,
    pub conflict_policy: OutputConflictPolicy,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyMedia {
    pub input: ScopedPath,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "parameters")]
pub enum TaskAction {
    GenerateTestClip(GenerateTestClip),
    TranscodeToMp4(TranscodeToMp4),
    VerifyMedia(VerifyMedia),
}

impl TaskAction {
    pub fn kind(&self) -> ActionKind {
        match self {
            Self::GenerateTestClip(_) => ActionKind::GenerateTestClip,
            Self::TranscodeToMp4(_) => ActionKind::TranscodeToMp4,
            Self::VerifyMedia(_) => ActionKind::VerifyMedia,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    GenerateTestClip,
    TranscodeToMp4,
    VerifyMedia,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRequest {
    pub idempotency_key: String,
    pub timeout_ms: u64,
    pub action: TaskAction,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProbeStream {
    pub index: u32,
    pub codec_type: String,
    pub codec_name: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MediaProbe {
    pub duration_ms: Option<u64>,
    pub streams: Vec<ProbeStream>,
}

impl MediaProbe {
    pub fn has_video(&self) -> bool {
        self.streams
            .iter()
            .any(|stream| stream.codec_type == "video")
    }

    pub fn has_audio(&self) -> bool {
        self.streams
            .iter()
            .any(|stream| stream.codec_type == "audio")
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum TaskResult {
    MediaCreated {
        output: ScopedPath,
        sha256: String,
        probe: MediaProbe,
    },
    MediaVerified {
        input: ScopedPath,
        sha256: String,
        probe: MediaProbe,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskErrorCode {
    InvalidConfiguration,
    InvalidRequest,
    PathDenied,
    ToolUnavailable,
    TaskNotFound,
    IdempotencyConflict,
    OutputConflict,
    SpawnFailed,
    ProcessExit,
    Timeout,
    Cancelled,
    VerificationFailed,
    InterruptedByRestart,
    StateIo,
    Internal,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskError {
    pub code: TaskErrorCode,
    pub message: String,
    pub exit_code: Option<i32>,
    pub retryable: bool,
    #[serde(default)]
    pub side_effects_may_exist: bool,
}

impl TaskError {
    pub(crate) fn new(
        code: TaskErrorCode,
        message: impl Into<String>,
        exit_code: Option<i32>,
        retryable: bool,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            exit_code,
            retryable,
            side_effects_may_exist: false,
        }
    }

    pub(crate) fn with_possible_side_effects(mut self) -> Self {
        self.side_effects_may_exist = true;
        self
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TaskSnapshot {
    pub id: TaskId,
    pub status: TaskStatus,
    pub action: ActionKind,
    pub result: Option<TaskResult>,
    pub error: Option<TaskError>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubmitOutcome {
    Accepted(TaskId),
    Duplicate(TaskId),
}

impl SubmitOutcome {
    pub fn task_id(&self) -> &TaskId {
        match self {
            Self::Accepted(id) | Self::Duplicate(id) => id,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LogEvent {
    pub task_id: TaskId,
    pub action: ActionKind,
    pub status: TaskStatus,
    pub event: String,
    pub error_code: Option<TaskErrorCode>,
    pub timestamp_ms: u64,
}

pub(crate) fn has_mp4_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("mp4"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal_absolute_and_shell_metacharacters() {
        let root = RootId::new("project").unwrap();
        for denied in ["../outside.mp4", "/tmp/out.mp4", "clips/a;touch.mp4"] {
            assert!(ScopedPath::new(root.clone(), denied).is_err(), "{denied}");
        }
        assert!(ScopedPath::new(root, "clips/normal name.mp4").is_ok());
    }

    #[test]
    fn unknown_or_argument_bearing_actions_do_not_deserialize() {
        let unknown = r#"{
            "idempotency_key":"one","timeout_ms":1000,
            "action":{"type":"run_shell","parameters":{"command":"whoami"}}
        }"#;
        assert!(serde_json::from_str::<TaskRequest>(unknown).is_err());

        let extra = r#"{
            "idempotency_key":"one","timeout_ms":1000,
            "action":{"type":"verify_media","parameters":{
                "input":{"root":"project","relative":"clip.mp4"},
                "args":["- arbitrary"]
            }}
        }"#;
        assert!(serde_json::from_str::<TaskRequest>(extra).is_err());
    }
}
