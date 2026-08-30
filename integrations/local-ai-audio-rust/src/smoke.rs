use crate::audio::{AudioVerification, verify_audio};
use crate::discovery::validate_smoke_installation;
use crate::model::{EndToEndStatus, ProviderId};
use crate::process::{ProcessLimits, run_bounded};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const SMOKE_TEXT: &str = "本地语音测试完成。";
const SMOKE_TIMEOUT: Duration = Duration::from_secs(300);
const SMOKE_STDOUT_LIMIT: usize = 256 * 1024;
const SMOKE_STDERR_LIMIT: usize = 256 * 1024;
static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovedInstallation {
    provider: ProviderId,
    canonical_path: PathBuf,
}

impl ApprovedInstallation {
    /// Construct only after the user explicitly selects/approves this path.
    /// Discovery reports cannot be converted implicitly.
    pub fn new(provider: ProviderId, explicitly_approved_path: &Path) -> Result<Self, String> {
        Ok(Self {
            provider,
            canonical_path: validate_smoke_installation(provider, explicitly_approved_path)?,
        })
    }

    pub fn provider(&self) -> ProviderId {
        self.provider
    }

    pub fn path(&self) -> &Path {
        &self.canonical_path
    }

    fn revalidate(&self) -> Result<PathBuf, String> {
        let current = validate_smoke_installation(self.provider, &self.canonical_path)?;
        if current != self.canonical_path {
            return Err("approved installation target changed after approval".to_owned());
        }
        Ok(current)
    }
}

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

pub fn run_smoke(approved: &ApprovedInstallation) -> Result<SmokeReport, String> {
    let provider = approved.provider;
    let installation = approved.revalidate()?;
    let acceptance_root = Path::new(env!("CARGO_MANIFEST_DIR")).join(".acceptance");
    fs::create_dir_all(&acceptance_root)
        .map_err(|_| "failed to create acceptance directory".to_owned())?;
    let run_id = unique_run_id(provider)?;
    let mut artifacts = RunArtifacts::create(&acceptance_root, &run_id)?;
    let output_path = artifacts.output_path.clone();

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

    command.current_dir(&installation);
    apply_minimal_model_environment(
        &mut command,
        &artifacts.runtime_home,
        &artifacts.runtime_tmp,
        provider,
    );
    let output = run_bounded(
        &mut command,
        "local model smoke",
        ProcessLimits {
            timeout: SMOKE_TIMEOUT,
            max_stdout_bytes: SMOKE_STDOUT_LIMIT,
            max_stderr_bytes: SMOKE_STDERR_LIMIT,
        },
    )
    .map_err(|error| error.into_message())?;
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let sanitized = sanitize_log(
        &combined,
        &[
            &installation,
            &artifacts.run_directory,
            Path::new(env!("CARGO_MANIFEST_DIR")),
        ],
    );
    if !output.status.success() {
        return Err(format!(
            "local smoke failed with {}: {}",
            output.status,
            tail(&sanitized, 6000)
        ));
    }
    if !output_path.is_file() {
        return Err("local smoke exited successfully without an audio file".to_owned());
    }
    let audio = verify_audio(&output_path)?;

    let report = SmokeReport {
        provider,
        text: SMOKE_TEXT.to_owned(),
        model_directory,
        parameters,
        end_to_end: EndToEndStatus::Passed,
        audio,
        process_log_tail: tail(&sanitized, 4000),
    };
    artifacts.commit()?;
    Ok(report)
}

fn apply_minimal_model_environment(
    command: &mut Command,
    runtime_home: &Path,
    runtime_tmp: &Path,
    provider: ProviderId,
) {
    command
        .env_clear()
        .env("HOME", runtime_home)
        .env("TMPDIR", runtime_tmp)
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONUTF8", "1")
        .env("HF_HUB_OFFLINE", "1")
        .env("TRANSFORMERS_OFFLINE", "1")
        .env("TOKENIZERS_PARALLELISM", "false");
    if provider == ProviderId::IndexTts25 {
        command.env("TORCH_FORCE_NO_WEIGHTS_ONLY_LOAD", "1");
    }
}

fn sanitize_log(value: &str, redacted_paths: &[&Path]) -> String {
    let mut sanitized = value.to_owned();
    for path in redacted_paths {
        if let Some(path) = path.to_str() {
            sanitized = sanitized.replace(path, "<local-path>");
        }
    }
    sanitized
        .lines()
        .map(|line| {
            let lowercase = line.to_ascii_lowercase();
            if [
                "authorization",
                "api_key",
                "api-key",
                "password=",
                "token=",
                "proxy=",
            ]
            .iter()
            .any(|marker| lowercase.contains(marker))
            {
                "<sensitive-log-line-redacted>"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn unique_run_id(provider: ProviderId) -> Result<String, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock is before the Unix epoch".to_owned())?
        .as_nanos();
    let counter = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(format!(
        "{}-{nanos}-{}-{counter}",
        match provider {
            ProviderId::IndexTts25 => "indextts25",
            ProviderId::VoxCpm2 => "voxcpm2",
        },
        std::process::id()
    ))
}

#[derive(Debug)]
struct RunArtifacts {
    run_directory: PathBuf,
    output_path: PathBuf,
    runtime_home: PathBuf,
    runtime_tmp: PathBuf,
    committed: bool,
}

impl RunArtifacts {
    fn create(acceptance_root: &Path, run_id: &str) -> Result<Self, String> {
        let run_directory = acceptance_root.join(run_id);
        fs::create_dir(&run_directory).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                "acceptance output collision; refusing to overwrite".to_owned()
            } else {
                "failed to create unique acceptance run directory".to_owned()
            }
        })?;
        let runtime_home = run_directory.join("runtime/home");
        let runtime_tmp = run_directory.join("runtime/tmp");
        let output_path = run_directory.join("sample.wav");
        let artifacts = Self {
            run_directory,
            output_path,
            runtime_home,
            runtime_tmp,
            committed: false,
        };
        fs::create_dir_all(&artifacts.runtime_home)
            .and_then(|_| fs::create_dir_all(&artifacts.runtime_tmp))
            .map_err(|_| "failed to create isolated smoke runtime".to_owned())?;
        if artifacts.output_path.exists() {
            return Err("acceptance output already exists; refusing to overwrite".to_owned());
        }
        Ok(artifacts)
    }

    fn commit(&mut self) -> Result<(), String> {
        if !self.output_path.is_file() {
            return Err("cannot commit acceptance run without verified audio".to_owned());
        }
        let runtime = self.run_directory.join("runtime");
        if runtime.exists() {
            fs::remove_dir_all(runtime)
                .map_err(|_| "failed to remove isolated smoke runtime".to_owned())?;
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for RunArtifacts {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.run_directory);
        }
    }
}

fn tail(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars().rev().take(max_chars).collect::<Vec<_>>();
    chars.reverse();
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::run_bounded;
    use std::fs::OpenOptions;

    fn fixture_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "local-ai-audio-{name}-{}-{}",
            std::process::id(),
            RUN_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn minimal_environment_does_not_inherit_sensitive_values() {
        let root = fixture_root("env");
        let home = root.join("home");
        let tmp = root.join("tmp");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&tmp).unwrap();
        let mut command = Command::new("/usr/bin/env");
        command.env("LOCAL_AI_AUDIO_SECRET", "must-not-leak");
        apply_minimal_model_environment(&mut command, &home, &tmp, ProviderId::VoxCpm2);
        let output = run_bounded(
            &mut command,
            "environment fixture",
            ProcessLimits {
                timeout: Duration::from_secs(2),
                max_stdout_bytes: 16 * 1024,
                max_stderr_bytes: 1024,
            },
        )
        .unwrap();
        let environment = String::from_utf8(output.stdout).unwrap();
        assert!(!environment.contains("LOCAL_AI_AUDIO_SECRET"));
        assert!(!environment.contains("must-not-leak"));
        assert!(!environment.contains("HTTP_PROXY"));
        assert!(!environment.contains("HTTPS_PROXY"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn output_collision_is_rejected_and_failed_run_is_removed() {
        let root = fixture_root("artifacts");
        fs::create_dir_all(&root).unwrap();
        let existing = root.join("same-id");
        fs::create_dir(&existing).unwrap();
        let collision = RunArtifacts::create(&root, "same-id").unwrap_err();
        assert!(collision.contains("collision"));

        let failed_directory;
        {
            let artifacts = RunArtifacts::create(&root, "failed-id").unwrap();
            failed_directory = artifacts.run_directory.clone();
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&artifacts.output_path)
                .unwrap();
        }
        assert!(!failed_directory.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn logs_redact_paths_and_sensitive_lines() {
        let path = Path::new("/private/provider/install");
        let sanitized = sanitize_log(
            "/private/provider/install/model loaded\nTOKEN=secret-value",
            &[path],
        );
        assert!(!sanitized.contains("/private/provider/install"));
        assert!(!sanitized.contains("secret-value"));
    }
}
