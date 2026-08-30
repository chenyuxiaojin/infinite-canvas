use crate::process::{ProcessLimits, run_bounded};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const MEDIA_STDOUT_LIMIT: usize = 128 * 1024;
const MEDIA_STDERR_LIMIT: usize = 128 * 1024;
const FFPROBE_TIMEOUT: Duration = Duration::from_secs(15);
const FFMPEG_TIMEOUT: Duration = Duration::from_secs(120);

const FFPROBE_CANDIDATES: &[&str] = &[
    "/opt/homebrew/bin/ffprobe",
    "/usr/local/bin/ffprobe",
    "/usr/bin/ffprobe",
];
const FFMPEG_CANDIDATES: &[&str] = &[
    "/opt/homebrew/bin/ffmpeg",
    "/usr/local/bin/ffmpeg",
    "/usr/bin/ffmpeg",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioVerification {
    pub path: PathBuf,
    pub format_name: String,
    pub duration_seconds: f64,
    pub codec_name: String,
    pub sample_rate_hz: u32,
    pub channels: u16,
    pub ffprobe_ok: bool,
    pub full_decode_ok: bool,
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    format: ProbeFormat,
    streams: Vec<ProbeStream>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: String,
    format_name: String,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_name: String,
    sample_rate: String,
    channels: u16,
}

#[derive(Debug)]
struct TrustedMediaTools {
    ffprobe: PathBuf,
    ffmpeg: PathBuf,
}

impl TrustedMediaTools {
    fn resolve() -> Result<Self, String> {
        Ok(Self {
            ffprobe: resolve_fixed_tool(FFPROBE_CANDIDATES, "ffprobe")?,
            ffmpeg: resolve_fixed_tool(FFMPEG_CANDIDATES, "ffmpeg")?,
        })
    }
}

pub fn verify_audio(path: &Path) -> Result<AudioVerification, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("audio path is unavailable: {error}"))?;
    if !canonical.is_file() {
        return Err("audio path is not a file".to_owned());
    }

    let tools = TrustedMediaTools::resolve()?;

    let mut probe_command = Command::new(&tools.ffprobe);
    probe_command
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "format=duration,format_name:stream=codec_name,sample_rate,channels",
            "-of",
            "json",
        ])
        .arg(&canonical)
        .env_clear()
        .env("LANG", "C")
        .env("LC_ALL", "C");
    let probe = run_bounded(
        &mut probe_command,
        "ffprobe",
        ProcessLimits {
            timeout: FFPROBE_TIMEOUT,
            max_stdout_bytes: MEDIA_STDOUT_LIMIT,
            max_stderr_bytes: MEDIA_STDERR_LIMIT,
        },
    )
    .map_err(|error| error.into_message())?;
    if !probe.status.success() {
        return Err("ffprobe rejected audio".to_owned());
    }
    let parsed: ProbeOutput = serde_json::from_slice(&probe.stdout)
        .map_err(|error| format!("invalid ffprobe JSON: {error}"))?;
    let stream = parsed
        .streams
        .first()
        .ok_or_else(|| "ffprobe found no audio stream".to_owned())?;
    let duration_seconds = parsed
        .format
        .duration
        .parse::<f64>()
        .map_err(|error| format!("invalid duration from ffprobe: {error}"))?;
    let sample_rate_hz = stream
        .sample_rate
        .parse::<u32>()
        .map_err(|error| format!("invalid sample rate from ffprobe: {error}"))?;
    if duration_seconds <= 0.0 || sample_rate_hz == 0 || stream.channels == 0 {
        return Err("ffprobe returned an empty audio stream".to_owned());
    }

    let mut decode_command = Command::new(&tools.ffmpeg);
    decode_command
        .args(["-v", "error", "-xerror", "-i"])
        .arg(&canonical)
        .args(["-map", "0:a:0", "-f", "null", "-"])
        .env_clear()
        .env("LANG", "C")
        .env("LC_ALL", "C");
    let decode = run_bounded(
        &mut decode_command,
        "ffmpeg",
        ProcessLimits {
            timeout: FFMPEG_TIMEOUT,
            max_stdout_bytes: MEDIA_STDOUT_LIMIT,
            max_stderr_bytes: MEDIA_STDERR_LIMIT,
        },
    )
    .map_err(|error| error.into_message())?;
    if !decode.status.success() {
        return Err("ffmpeg full decode failed".to_owned());
    }

    Ok(AudioVerification {
        path: canonical,
        format_name: parsed.format.format_name,
        duration_seconds,
        codec_name: stream.codec_name.clone(),
        sample_rate_hz,
        channels: stream.channels,
        ffprobe_ok: true,
        full_decode_ok: true,
    })
}

fn resolve_fixed_tool(candidates: &[&str], label: &str) -> Result<PathBuf, String> {
    for candidate in candidates {
        let path = Path::new(candidate);
        if !path.is_file() {
            continue;
        }
        let Ok(canonical) = fs::canonicalize(path) else {
            continue;
        };
        if canonical.is_file() && is_executable(&canonical) {
            return Ok(canonical);
        }
    }
    Err(format!(
        "trusted {label} executable was not found in the internal candidate set"
    ))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}
