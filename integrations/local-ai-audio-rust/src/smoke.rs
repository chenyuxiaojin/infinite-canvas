use crate::audio::{AudioVerification, verify_audio};
use crate::model::{EndToEndStatus, ProviderId};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SMOKE_TEXT: &str = "本地语音测试完成。";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmokeReport {
    pub provider: ProviderId,
    pub text: String,
    pub model_directory: PathBuf,
    pub parameters: Vec<String>,
    pub end_to_end: EndToEndStatus,
    pub audio: AudioVerification,
    pub process_log_tail: String,
}

pub fn run_smoke(provider: ProviderId, installation: &Path) -> Result<SmokeReport, String> {
    let installation = installation
        .canonicalize()
        .map_err(|error| format!("installation is unavailable: {error}"))?;
    let output_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join(".acceptance");
    fs::create_dir_all(&output_directory)
        .map_err(|error| format!("failed to create acceptance directory: {error}"))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs();
    let output_path = output_directory.join(format!(
        "{}-{timestamp}.wav",
        match provider {
            ProviderId::IndexTts25 => "indextts25",
            ProviderId::VoxCpm2 => "voxcpm2",
        }
    ));

    let (mut command, model_directory, parameters) = match provider {
        ProviderId::IndexTts25 => {
            let python = installation.join(".venv/bin/python");
            let runner = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/index_v25_smoke.py");
            let mut command = Command::new(python);
            command
                .arg(runner)
                .arg("--install")
                .arg(&installation)
                .arg("--output")
                .arg(&output_path);
            (
                command,
                installation.join("checkpoints"),
                vec![
                    "device=mps".to_owned(),
                    "language=ZH".to_owned(),
                    "reference=upstream examples/voice_01.wav".to_owned(),
                    "emotion=disabled".to_owned(),
                    "seed=42".to_owned(),
                    "duration_factor=1.0".to_owned(),
                ],
            )
        }
        ProviderId::VoxCpm2 => {
            let executable = installation.join(".venv/bin/voxcpm");
            let model = installation.join("pretrained_models/VoxCPM2");
            let mut command = Command::new(executable);
            command
                .args(["design", "--text", SMOKE_TEXT])
                .args(["--control", "清晰自然的普通话声音"])
                .args(["--cfg-value", "2.0"])
                .args(["--inference-timesteps", "4"])
                .args(["--seed", "42"])
                .arg("--model-path")
                .arg(&model)
                .args([
                    "--device",
                    "mps",
                    "--local-files-only",
                    "--no-denoiser",
                    "--no-optimize",
                    "--output",
                ])
                .arg(&output_path);
            (
                command,
                model,
                vec![
                    "mode=voice_design".to_owned(),
                    "device=mps".to_owned(),
                    "control=清晰自然的普通话声音".to_owned(),
                    "cfg=2.0".to_owned(),
                    "inference_timesteps=4".to_owned(),
                    "seed=42".to_owned(),
                    "denoiser=disabled".to_owned(),
                    "network=local_files_only".to_owned(),
                ],
            )
        }
    };

    command
        .current_dir(&installation)
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .env("PYTHONNOUSERSITE", "1");
    if provider == ProviderId::IndexTts25 {
        command.env("TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD", "1");
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to start local smoke test: {error}"))?;
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(format!(
            "local smoke test failed with {}: {}",
            output.status,
            tail(&combined, 6000)
        ));
    }
    let audio = verify_audio(&output_path)?;

    Ok(SmokeReport {
        provider,
        text: SMOKE_TEXT.to_owned(),
        model_directory,
        parameters,
        end_to_end: EndToEndStatus::Passed,
        audio,
        process_log_tail: tail(&combined, 4000),
    })
}

fn tail(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars().rev().take(max_chars).collect::<Vec<_>>();
    chars.reverse();
    chars.into_iter().collect()
}
