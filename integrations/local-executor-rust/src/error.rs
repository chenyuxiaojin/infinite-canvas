use crate::TaskErrorCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExecutorError {
    #[error("invalid executor configuration: {0}")]
    InvalidConfiguration(&'static str),
    #[error("invalid task request: {0}")]
    InvalidRequest(&'static str),
    #[error("path is outside the registered root or violates the path policy")]
    PathDenied,
    #[error("registered root was not found")]
    UnknownRoot,
    #[error("required media tool is unavailable or failed version verification")]
    ToolUnavailable,
    #[error("task id was not found")]
    TaskNotFound,
    #[error("idempotency key was already used with a different request")]
    IdempotencyConflict,
    #[error("output already exists and overwrite is forbidden")]
    OutputConflict,
    #[error("executor state could not be read or written")]
    StateIo,
    #[error("executor worker is unavailable")]
    WorkerUnavailable,
}

impl ExecutorError {
    pub fn code(&self) -> TaskErrorCode {
        match self {
            Self::InvalidConfiguration(_) => TaskErrorCode::InvalidConfiguration,
            Self::InvalidRequest(_) => TaskErrorCode::InvalidRequest,
            Self::PathDenied | Self::UnknownRoot => TaskErrorCode::PathDenied,
            Self::ToolUnavailable => TaskErrorCode::ToolUnavailable,
            Self::TaskNotFound => TaskErrorCode::TaskNotFound,
            Self::IdempotencyConflict => TaskErrorCode::IdempotencyConflict,
            Self::OutputConflict => TaskErrorCode::OutputConflict,
            Self::StateIo => TaskErrorCode::StateIo,
            Self::WorkerUnavailable => TaskErrorCode::Internal,
        }
    }
}
