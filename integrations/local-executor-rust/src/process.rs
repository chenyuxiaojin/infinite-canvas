use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const CAPTURE_LIMIT: usize = 64 * 1024;

#[derive(Debug)]
pub(crate) struct ProcessOutput {
    pub status_code: Option<i32>,
    pub success: bool,
    pub stdout: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum ProcessRunError {
    Spawn,
    Wait,
    Cancelled,
    TimedOut,
}

pub(crate) fn run_process(
    program: &Path,
    arguments: &[OsString],
    timeout: Duration,
    cancelled: &Arc<AtomicBool>,
) -> Result<ProcessOutput, ProcessRunError> {
    if cancelled.load(Ordering::SeqCst) {
        return Err(ProcessRunError::Cancelled);
    }

    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| ProcessRunError::Spawn)?;

    let stdout = child.stdout.take().ok_or(ProcessRunError::Spawn)?;
    let stderr = child.stderr.take().ok_or(ProcessRunError::Spawn)?;
    let stdout_reader = thread::spawn(move || drain_capped(stdout));
    let stderr_reader = thread::spawn(move || drain_capped(stderr));
    let started = Instant::now();

    let status = loop {
        if cancelled.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(ProcessRunError::Cancelled);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(ProcessRunError::TimedOut);
        }
        match child.try_wait().map_err(|_| ProcessRunError::Wait)? {
            Some(status) => break status,
            None => thread::sleep(Duration::from_millis(20)),
        }
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let _stderr = stderr_reader.join().unwrap_or_default();
    Ok(ProcessOutput {
        status_code: status.code(),
        success: status.success(),
        stdout,
    })
}

fn drain_capped(mut reader: impl Read) -> Vec<u8> {
    let mut captured = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let Ok(read) = reader.read(&mut buffer) else {
            break;
        };
        if read == 0 {
            break;
        }
        if captured.len() < CAPTURE_LIMIT {
            let remaining = CAPTURE_LIMIT - captured.len();
            let to_copy = remaining.min(read);
            let _ = captured.write_all(&buffer[..to_copy]);
        }
    }
    captured
}
