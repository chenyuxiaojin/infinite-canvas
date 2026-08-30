use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessFailureKind {
    Spawn,
    Wait,
    Timeout,
    OutputLimit,
    Capture,
}

#[derive(Debug)]
pub(crate) struct ProcessFailure {
    pub kind: ProcessFailureKind,
    pub message: String,
}

impl ProcessFailure {
    pub(crate) fn into_message(self) -> String {
        match self.kind {
            ProcessFailureKind::Spawn
            | ProcessFailureKind::Wait
            | ProcessFailureKind::Timeout
            | ProcessFailureKind::OutputLimit
            | ProcessFailureKind::Capture => self.message,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ProcessLimits {
    pub timeout: Duration,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

#[derive(Debug)]
pub(crate) struct BoundedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub(crate) fn run_bounded(
    command: &mut Command,
    label: &str,
    limits: ProcessLimits,
) -> Result<BoundedOutput, ProcessFailure> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| ProcessFailure {
        kind: ProcessFailureKind::Spawn,
        message: format!("failed to start {label}: {error}"),
    })?;
    let stdout = child.stdout.take().ok_or_else(|| ProcessFailure {
        kind: ProcessFailureKind::Capture,
        message: format!("failed to capture {label} stdout"),
    })?;
    let stderr = child.stderr.take().ok_or_else(|| ProcessFailure {
        kind: ProcessFailureKind::Capture,
        message: format!("failed to capture {label} stderr"),
    })?;

    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout_reader = read_bounded(stdout, limits.max_stdout_bytes, Arc::clone(&exceeded));
    let stderr_reader = read_bounded(stderr, limits.max_stderr_bytes, Arc::clone(&exceeded));
    let started = Instant::now();

    let status = loop {
        if exceeded.load(Ordering::Acquire) {
            kill_and_reap(&mut child);
            let _ = join_reader(stdout_reader);
            let _ = join_reader(stderr_reader);
            return Err(ProcessFailure {
                kind: ProcessFailureKind::OutputLimit,
                message: format!("{label} exceeded the bounded output limit"),
            });
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= limits.timeout => {
                kill_and_reap(&mut child);
                let _ = join_reader(stdout_reader);
                let _ = join_reader(stderr_reader);
                return Err(ProcessFailure {
                    kind: ProcessFailureKind::Timeout,
                    message: format!(
                        "{label} timed out after {} seconds and was terminated",
                        limits.timeout.as_secs_f64()
                    ),
                });
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                kill_and_reap(&mut child);
                let _ = join_reader(stdout_reader);
                let _ = join_reader(stderr_reader);
                return Err(ProcessFailure {
                    kind: ProcessFailureKind::Wait,
                    message: format!("failed while waiting for {label}: {error}"),
                });
            }
        }
    };

    let stdout = join_reader(stdout_reader)?;
    let stderr = join_reader(stderr_reader)?;
    if exceeded.load(Ordering::Acquire) {
        return Err(ProcessFailure {
            kind: ProcessFailureKind::OutputLimit,
            message: format!("{label} exceeded the bounded output limit"),
        });
    }
    Ok(BoundedOutput {
        status,
        stdout,
        stderr,
    })
}

fn read_bounded<R: Read + Send + 'static>(
    mut reader: R,
    limit: usize,
    exceeded: Arc<AtomicBool>,
) -> thread::JoinHandle<Result<Vec<u8>, std::io::Error>> {
    thread::spawn(move || {
        let mut captured = Vec::with_capacity(limit.min(16 * 1024));
        let mut buffer = [0u8; 8192];
        let mut total = 0usize;
        loop {
            let read = reader.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            total = total.saturating_add(read);
            if captured.len() < limit {
                let remaining = limit - captured.len();
                captured.extend_from_slice(&buffer[..read.min(remaining)]);
            }
            if total > limit {
                exceeded.store(true, Ordering::Release);
            }
        }
        Ok(captured)
    })
}

fn join_reader(
    handle: thread::JoinHandle<Result<Vec<u8>, std::io::Error>>,
) -> Result<Vec<u8>, ProcessFailure> {
    handle
        .join()
        .map_err(|_| ProcessFailure {
            kind: ProcessFailureKind::Capture,
            message: "output capture thread panicked".to_owned(),
        })?
        .map_err(|error| ProcessFailure {
            kind: ProcessFailureKind::Capture,
            message: format!("failed to capture process output: {error}"),
        })
}

fn kill_and_reap(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_terminates_and_reaps_direct_child() {
        let mut command = Command::new("/bin/sleep");
        command.arg("5").env_clear();
        let started = Instant::now();
        let error = run_bounded(
            &mut command,
            "timeout fixture",
            ProcessLimits {
                timeout: Duration::from_millis(50),
                max_stdout_bytes: 1024,
                max_stderr_bytes: 1024,
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, ProcessFailureKind::Timeout);
        assert!(error.message.contains("timed out"));
        assert!(error.message.contains("terminated"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn excessive_output_is_bounded_and_fails() {
        let mut command = Command::new("/usr/bin/yes");
        command.env_clear();
        let error = run_bounded(
            &mut command,
            "output fixture",
            ProcessLimits {
                timeout: Duration::from_secs(2),
                max_stdout_bytes: 1024,
                max_stderr_bytes: 1024,
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, ProcessFailureKind::OutputLimit);
        assert!(error.message.contains("bounded output limit"));
    }
}
