use local_ai_audio::{
    DiscoveryConfig, EndToEndStatus, IpcRequest, ProviderId, ProviderStatus, ServiceProbe,
    ServiceState, ServiceStatus, probe_all_with,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct FixtureDir(PathBuf);

impl FixtureDir {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("local-ai-audio-{nonce}"));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct FakeServiceProbe(ServiceStatus);

impl ServiceProbe for FakeServiceProbe {
    fn probe(&self, provider: ProviderId) -> ServiceState {
        ServiceState {
            status: self.0,
            loopback_port: match provider {
                ProviderId::IndexTts25 => 7860,
                ProviderId::VoxCpm2 => 8808,
            },
            http_status: (self.0 == ServiceStatus::Ready).then_some(200),
            identity_confirmed: self.0 == ServiceStatus::Ready,
            detail: "fixture".to_owned(),
        }
    }
}

#[test]
fn complete_fixture_maps_service_states_without_claiming_e2e() {
    let fixture = FixtureDir::new();
    create_index_fixture(&fixture.0, "3.11.13");

    let ready = probe_all_with(
        &DiscoveryConfig::with_roots(vec![fixture.0.clone()]),
        &FakeServiceProbe(ServiceStatus::Ready),
    );
    assert_eq!(ready.providers[0].status, ProviderStatus::Ready);
    assert_eq!(ready.providers[0].end_to_end, EndToEndStatus::NotRun);

    let stopped = probe_all_with(
        &DiscoveryConfig::with_roots(vec![fixture.0.clone()]),
        &FakeServiceProbe(ServiceStatus::NotRunning),
    );
    assert_eq!(stopped.providers[0].status, ProviderStatus::NotRunning);

    let error = probe_all_with(
        &DiscoveryConfig::with_roots(vec![fixture.0.clone()]),
        &FakeServiceProbe(ServiceStatus::UnexpectedResponse),
    );
    assert_eq!(error.providers[0].status, ProviderStatus::Error);
}

#[test]
fn missing_model_and_incompatible_runtime_have_precise_status() {
    let fixture = FixtureDir::new();
    let index = create_index_fixture(&fixture.0, "3.11.13");
    fs::remove_file(index.join("checkpoints/gpt.pth")).unwrap();
    let missing = probe_all_with(
        &DiscoveryConfig::with_roots(vec![fixture.0.clone()]),
        &FakeServiceProbe(ServiceStatus::NotRunning),
    );
    assert_eq!(missing.providers[0].status, ProviderStatus::ModelMissing);
    assert_eq!(missing.providers[0].missing_model_files, ["gpt.pth"]);

    let fixture = FixtureDir::new();
    create_index_fixture(&fixture.0, "3.9.19");
    let incompatible = probe_all_with(
        &DiscoveryConfig::with_roots(vec![fixture.0.clone()]),
        &FakeServiceProbe(ServiceStatus::Ready),
    );
    assert_eq!(
        incompatible.providers[0].status,
        ProviderStatus::Incompatible
    );
}

#[test]
fn probe_disabled_yields_discovered_and_ipc_has_no_shell_or_url() {
    let fixture = FixtureDir::new();
    create_vox_fixture(&fixture.0, "3.11.13");
    let mut config = DiscoveryConfig::with_roots(vec![fixture.0.clone()]);
    config.probe_services = false;
    let result = probe_all_with(&config, &FakeServiceProbe(ServiceStatus::Ready));
    assert_eq!(result.providers[1].status, ProviderStatus::Discovered);

    let encoded = serde_json::to_string(&IpcRequest::SmokeTest {
        provider: ProviderId::VoxCpm2,
        installation: PathBuf::from("/user/approved/provider"),
    })
    .unwrap();
    assert!(!encoded.contains("shell"));
    assert!(!encoded.contains("url"));
    assert!(!encoded.contains("command"));
    assert!(encoded.contains("installation"));
}

#[test]
fn discovery_never_executes_a_marker_matched_directory() {
    let fixture = FixtureDir::new();
    let install = create_vox_fixture(&fixture.0, "3.11.13");
    let sentinel = fixture.0.join("unexpected-execution");
    let executable = install.join(".venv/bin/voxcpm");
    fs::write(
        &executable,
        format!("#!/bin/sh\ntouch '{}'\n", sentinel.display()),
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();

    let report = probe_all_with(
        &DiscoveryConfig::with_roots(vec![fixture.0.clone()]),
        &FakeServiceProbe(ServiceStatus::NotRunning),
    );
    assert!(report.providers[1].installation.is_some());
    assert!(!sentinel.exists());

    let output = Command::new(env!("CARGO_BIN_EXE_local-ai-audio"))
        .args(["smoke", "vox_cpm_2"])
        .env("LOCAL_AI_AUDIO_DISCOVERY_ROOTS", &fixture.0)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!sentinel.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--install"));
}

#[test]
fn status_contract_serializes_all_required_states() {
    let statuses = [
        ProviderStatus::Discovered,
        ProviderStatus::Ready,
        ProviderStatus::NotRunning,
        ProviderStatus::ModelMissing,
        ProviderStatus::Incompatible,
        ProviderStatus::Error,
    ];
    assert_eq!(
        serde_json::to_value(statuses).unwrap(),
        serde_json::json!([
            "discovered",
            "ready",
            "not_running",
            "model_missing",
            "incompatible",
            "error"
        ])
    );
}

fn create_index_fixture(root: &Path, version: &str) -> PathBuf {
    let install = root.join("IndexTTS-fixture");
    touch(&install.join("webui.py"));
    fs::create_dir_all(install.join("indextts")).unwrap();
    touch(&install.join(".venv/bin/python"));
    fs::write(
        install.join(".venv/pyvenv.cfg"),
        format!("version_info = {version}\n"),
    )
    .unwrap();
    for marker in [
        "config.yaml",
        "gpt.pth",
        "s2mel.pth",
        "codec.pth",
        "feat1.pt",
        "feat2.pt",
        "multilingual_zh_ja_yue_char_del.tiktoken",
        "wav2vec2bert_stats.pt",
        "qwen0.6bemo4-merge/config.json",
        "qwen0.6bemo4-merge/model.safetensors",
        "hf_cache/semantic_codec_model.safetensors",
        "hf_cache/campplus_cn_common.bin",
        "hf_cache/bigvgan/config.json",
        "hf_cache/bigvgan/bigvgan_generator.pt",
        "hf_cache/w2v-bert-2.0/config.json",
        "hf_cache/w2v-bert-2.0/model.safetensors",
    ] {
        touch(&install.join("checkpoints").join(marker));
    }
    install
}

fn create_vox_fixture(root: &Path, version: &str) -> PathBuf {
    let install = root.join("VoxCPM-fixture");
    touch(&install.join("app.py"));
    fs::create_dir_all(install.join("src/voxcpm")).unwrap();
    touch(&install.join(".venv/bin/python"));
    fs::write(
        install.join(".venv/pyvenv.cfg"),
        format!("version_info = {version}\n"),
    )
    .unwrap();
    for marker in [
        "config.json",
        "model.safetensors",
        "audiovae.pth",
        "tokenizer.json",
        "tokenization_voxcpm2.py",
    ] {
        touch(&install.join("pretrained_models/VoxCPM2").join(marker));
    }
    install
}

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, b"fixture").unwrap();
}
