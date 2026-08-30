//! Allowlisted, restart-aware local media execution for a future Tauri adapter.
//!
//! The crate deliberately exposes typed actions instead of a command string. The host
//! registers trusted roots and tools; task requests only contain root IDs and relative paths.

mod error;
mod executor;
mod paths;
mod process;
mod tools;
mod types;

pub use error::ExecutorError;
pub use executor::{Executor, ExecutorConfig};
pub use paths::{AllowedRoot, PathPolicy};
pub use tools::{ToolDiscoveryConfig, Toolchain};
pub use types::{
    ActionKind, GenerateTestClip, LogEvent, MediaProbe, OutputConflictPolicy, ProbeStream, RootId,
    ScopedPath, SubmitOutcome, TaskAction, TaskError, TaskErrorCode, TaskId, TaskRequest,
    TaskResult, TaskSnapshot, TaskStatus, TranscodeToMp4, VerifyMedia,
};
