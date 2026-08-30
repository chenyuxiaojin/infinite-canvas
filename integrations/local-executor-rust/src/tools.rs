use crate::{
    ExecutorError,
    process::{ProcessRunError, run_process},
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Clone, Debug)]
pub struct ToolDiscoveryConfig {
    pub trusted_directories: Vec<PathBuf>,
    pub version_timeout: Duration,
}

impl Default for ToolDiscoveryConfig {
    fn default() -> Self {
        Self {
            trusted_directories: vec![
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin"),
                PathBuf::from("/usr/bin"),
            ],
            version_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ToolReport {
    pub name: String,
    pub version_line: String,
}

#[derive(Clone, Debug)]
pub struct Toolchain {
    ffmpeg: PathBuf,
    ffprobe: PathBuf,
    reports: Vec<ToolReport>,
}

impl Toolchain {
    pub fn discover(config: ToolDiscoveryConfig) -> Result<Self, ExecutorError> {
        if config.trusted_directories.is_empty() || config.version_timeout.is_zero() {
            return Err(ExecutorError::InvalidConfiguration(
                "invalid tool discovery settings",
            ));
        }
        let trusted = config
            .trusted_directories
            .iter()
            .filter_map(|directory| fs::canonicalize(directory).ok())
            .filter(|directory| directory.is_dir())
            .collect::<Vec<_>>();
        let ffmpeg = find_tool("ffmpeg", &trusted)?;
        let ffprobe = find_tool("ffprobe", &trusted)?;
        let reports = vec![
            probe_version(&ffmpeg, "ffmpeg", config.version_timeout)?,
            probe_version(&ffprobe, "ffprobe", config.version_timeout)?,
        ];
        Ok(Self {
            ffmpeg,
            ffprobe,
            reports,
        })
    }

    pub fn reports(&self) -> &[ToolReport] {
        &self.reports
    }

    pub(crate) fn ffmpeg(&self) -> &Path {
        &self.ffmpeg
    }

    pub(crate) fn ffprobe(&self) -> &Path {
        &self.ffprobe
    }
}

fn find_tool(name: &str, directories: &[PathBuf]) -> Result<PathBuf, ExecutorError> {
    for directory in directories {
        let candidate = directory.join(name);
        let Ok(metadata) = fs::metadata(&candidate) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        #[cfg(unix)]
        if metadata.permissions().mode() & 0o111 == 0 {
            continue;
        }
        return Ok(candidate);
    }
    Err(ExecutorError::ToolUnavailable)
}

fn probe_version(
    path: &Path,
    expected_name: &str,
    timeout: Duration,
) -> Result<ToolReport, ExecutorError> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let output = run_process(path, &[OsString::from("-version")], timeout, &cancelled)
        .map_err(|_| ExecutorError::ToolUnavailable)?;
    if !output.success {
        return Err(ExecutorError::ToolUnavailable);
    }
    let stdout = String::from_utf8(output.stdout).map_err(|_| ExecutorError::ToolUnavailable)?;
    let version_line = stdout.lines().next().unwrap_or_default().trim();
    let expected_prefix = format!("{expected_name} version ");
    if !version_line.starts_with(&expected_prefix) || version_line.len() > 512 {
        return Err(ExecutorError::ToolUnavailable);
    }
    Ok(ToolReport {
        name: expected_name.to_owned(),
        version_line: version_line.to_owned(),
    })
}

pub(crate) fn map_process_error(error: ProcessRunError) -> crate::TaskError {
    use crate::{TaskError, TaskErrorCode};
    match error {
        ProcessRunError::Spawn => TaskError::new(
            TaskErrorCode::SpawnFailed,
            "media tool could not be started",
            None,
            true,
        ),
        ProcessRunError::Wait => TaskError::new(
            TaskErrorCode::Internal,
            "media tool status could not be read",
            None,
            true,
        ),
        ProcessRunError::Cancelled => {
            TaskError::new(TaskErrorCode::Cancelled, "task was cancelled", None, false)
        }
        ProcessRunError::TimedOut => TaskError::new(
            TaskErrorCode::Timeout,
            "media tool exceeded the task timeout",
            None,
            true,
        ),
    }
}
