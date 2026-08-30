use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

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

pub fn verify_audio(path: &Path) -> Result<AudioVerification, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("audio path is unavailable: {error}"))?;
    if !canonical.is_file() {
        return Err("audio path is not a file".to_owned());
    }

    let probe = Command::new("ffprobe")
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
        .output()
        .map_err(|error| format!("failed to start ffprobe: {error}"))?;
    if !probe.status.success() {
        return Err(format!(
            "ffprobe rejected audio: {}",
            String::from_utf8_lossy(&probe.stderr).trim()
        ));
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

    let decode = Command::new("ffmpeg")
        .args(["-v", "error", "-xerror", "-i"])
        .arg(&canonical)
        .args(["-map", "0:a:0", "-f", "null", "-"])
        .output()
        .map_err(|error| format!("failed to start ffmpeg: {error}"))?;
    if !decode.status.success() {
        return Err(format!(
            "ffmpeg full decode failed: {}",
            String::from_utf8_lossy(&decode.stderr).trim()
        ));
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
