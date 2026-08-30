use crate::model::{
    Capability, EndToEndStatus, Evidence, ProbeResponse, ProviderId, ProviderReport,
    ProviderStatus, RuntimeState, ServiceState, ServiceStatus,
};
use std::collections::{HashSet, VecDeque};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_MAX_DEPTH: usize = 7;
const DEFAULT_MAX_DIRECTORIES: usize = 60_000;

#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    pub roots: Vec<PathBuf>,
    pub index_tts_home: Option<PathBuf>,
    pub vox_cpm_home: Option<PathBuf>,
    pub probe_services: bool,
    pub max_depth: usize,
    pub max_directories: usize,
}

impl DiscoveryConfig {
    pub fn from_env() -> Result<Self, String> {
        let index_tts_home = env::var_os("LOCAL_AI_AUDIO_INDEXTTS_HOME").map(PathBuf::from);
        let vox_cpm_home = env::var_os("LOCAL_AI_AUDIO_VOXCPM_HOME").map(PathBuf::from);
        let mut roots = env::var_os("LOCAL_AI_AUDIO_DISCOVERY_ROOTS")
            .map(|value| env::split_paths(&value).collect::<Vec<_>>())
            .unwrap_or_default();

        if roots.is_empty() && index_tts_home.is_none() && vox_cpm_home.is_none() {
            let home = env::var_os("HOME").ok_or("HOME is unavailable; provide discovery roots")?;
            roots.push(PathBuf::from(home));
        }

        Ok(Self {
            roots,
            index_tts_home,
            vox_cpm_home,
            probe_services: true,
            max_depth: DEFAULT_MAX_DEPTH,
            max_directories: DEFAULT_MAX_DIRECTORIES,
        })
    }

    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self {
            roots,
            index_tts_home: None,
            vox_cpm_home: None,
            probe_services: true,
            max_depth: DEFAULT_MAX_DEPTH,
            max_directories: DEFAULT_MAX_DIRECTORIES,
        }
    }
}

pub trait ServiceProbe {
    fn probe(&self, provider: ProviderId) -> ServiceState;
}

#[derive(Debug, Default)]
pub struct LoopbackServiceProbe;

impl ServiceProbe for LoopbackServiceProbe {
    fn probe(&self, provider: ProviderId) -> ServiceState {
        let port = provider.loopback_port();
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        let mut stream = match TcpStream::connect_timeout(&address.into(), Duration::from_secs(1)) {
            Ok(stream) => stream,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionRefused
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::NotConnected
                ) =>
            {
                return ServiceState {
                    status: ServiceStatus::NotRunning,
                    loopback_port: port,
                    http_status: None,
                    identity_confirmed: false,
                    detail: error.to_string(),
                };
            }
            Err(error) => {
                return ServiceState {
                    status: ServiceStatus::Error,
                    loopback_port: port,
                    http_status: None,
                    identity_confirmed: false,
                    detail: error.to_string(),
                };
            }
        };

        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));
        let request =
            format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
        if let Err(error) = stream.write_all(request.as_bytes()) {
            return ServiceState {
                status: ServiceStatus::Error,
                loopback_port: port,
                http_status: None,
                identity_confirmed: false,
                detail: error.to_string(),
            };
        }

        let mut response = Vec::with_capacity(16 * 1024);
        if let Err(error) = stream.take(64 * 1024).read_to_end(&mut response) {
            return ServiceState {
                status: ServiceStatus::Error,
                loopback_port: port,
                http_status: None,
                identity_confirmed: false,
                detail: error.to_string(),
            };
        }
        let response = String::from_utf8_lossy(&response);
        let status = response
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|value| value.parse::<u16>().ok());
        let lowercase = response.to_ascii_lowercase();
        let identity_confirmed = match provider {
            ProviderId::IndexTts25 => lowercase.contains("indextts"),
            ProviderId::VoxCpm2 => lowercase.contains("voxcpm"),
        };
        let ready = status.is_some_and(|code| (200..300).contains(&code)) && identity_confirmed;

        ServiceState {
            status: if ready {
                ServiceStatus::Ready
            } else {
                ServiceStatus::UnexpectedResponse
            },
            loopback_port: port,
            http_status: status,
            identity_confirmed,
            detail: if ready {
                "loopback HTTP response matched provider identity".to_owned()
            } else {
                "listener responded but did not prove provider identity".to_owned()
            },
        }
    }
}

pub fn probe_all(config: &DiscoveryConfig) -> ProbeResponse {
    probe_all_with(config, &LoopbackServiceProbe)
}

pub fn probe_all_with(config: &DiscoveryConfig, service_probe: &dyn ServiceProbe) -> ProbeResponse {
    let mut found = Vec::new();
    let mut scan_evidence = Vec::new();

    if let Some(path) = &config.index_tts_home {
        add_explicit_candidate(ProviderId::IndexTts25, path, &mut found, &mut scan_evidence);
    }
    if let Some(path) = &config.vox_cpm_home {
        add_explicit_candidate(ProviderId::VoxCpm2, path, &mut found, &mut scan_evidence);
    }

    for root in &config.roots {
        match scan_root(root, config.max_depth, config.max_directories) {
            Ok(paths) => found.extend(paths),
            Err(error) => scan_evidence.push(Evidence::fact(
                "discovery_root_error",
                error,
                Some(root.clone()),
            )),
        }
    }

    found.sort_by(|left, right| left.1.cmp(&right.1));
    found.dedup();

    let providers = [ProviderId::IndexTts25, ProviderId::VoxCpm2]
        .into_iter()
        .map(|provider| {
            let candidates = found
                .iter()
                .filter(|(id, _)| *id == provider)
                .map(|(_, path)| path.clone())
                .collect::<Vec<_>>();
            build_report(
                provider,
                candidates,
                config.probe_services,
                service_probe,
                &scan_evidence,
            )
        })
        .collect();
    ProbeResponse::new(providers)
}

fn scan_root(
    root: &Path,
    max_depth: usize,
    max_directories: usize,
) -> Result<Vec<(ProviderId, PathBuf)>, String> {
    if !root.is_dir() {
        return Err("discovery root is not a directory".to_owned());
    }
    let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut visited = HashSet::new();
    let mut found = Vec::new();

    while let Some((directory, depth)) = queue.pop_front() {
        if visited.len() >= max_directories {
            return Err(format!(
                "discovery stopped after {max_directories} directories; narrow the root"
            ));
        }
        let canonical = match fs::canonicalize(&directory) {
            Ok(path) => path,
            Err(_) => continue,
        };
        if !visited.insert(canonical.clone()) {
            continue;
        }

        if let Some(provider) = identify_installation(&canonical) {
            found.push((provider, canonical));
            continue;
        }
        if depth >= max_depth {
            continue;
        }

        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if should_skip_directory(&name) {
                continue;
            }
            queue.push_back((entry.path(), depth + 1));
        }
    }
    Ok(found)
}

fn should_skip_directory(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "Library" | "node_modules" | "target" | "dist" | "build" | "data"
        )
}

fn identify_installation(path: &Path) -> Option<ProviderId> {
    if path.join("webui.py").is_file()
        && path.join("indextts").is_dir()
        && path.join("checkpoints").is_dir()
    {
        return Some(ProviderId::IndexTts25);
    }
    if path.join("app.py").is_file()
        && path.join("src/voxcpm").is_dir()
        && path.join("pretrained_models").is_dir()
    {
        return Some(ProviderId::VoxCpm2);
    }
    None
}

pub(crate) fn validate_smoke_installation(
    provider: ProviderId,
    path: &Path,
) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|_| "approved installation path is unavailable".to_owned())?;
    if identify_installation(&canonical) != Some(provider) {
        return Err("approved path does not match the requested provider markers".to_owned());
    }

    let (model_directory, required_files) = model_spec(provider, &canonical);
    let missing = required_files
        .iter()
        .filter(|relative| !model_directory.join(relative).is_file())
        .map(|relative| relative.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "approved installation is missing model files: {}",
            missing.join(", ")
        ));
    }
    if !inspect_runtime(&canonical).compatible {
        return Err("approved installation runtime is incompatible".to_owned());
    }

    let required_entry = match provider {
        ProviderId::IndexTts25 => canonical.join("examples/voice_01.wav"),
        ProviderId::VoxCpm2 => canonical.join(".venv/bin/voxcpm"),
    };
    if !required_entry.is_file() {
        return Err("approved installation smoke entry is unavailable".to_owned());
    }
    Ok(canonical)
}

fn build_report(
    provider: ProviderId,
    candidates: Vec<PathBuf>,
    probe_services: bool,
    service_probe: &dyn ServiceProbe,
    scan_evidence: &[Evidence],
) -> ProviderReport {
    let mut evidence = scan_evidence.to_vec();
    let Some(installation) = candidates.first().cloned() else {
        evidence.push(Evidence::fact(
            "installation_not_found",
            "no matching installation was found in the configured roots",
            None,
        ));
        return ProviderReport {
            provider,
            display_name: provider.display_name().to_owned(),
            status: ProviderStatus::NotFound,
            capabilities: Capability::for_provider(provider),
            installation: None,
            model_directory: None,
            missing_model_files: Vec::new(),
            runtime: RuntimeState {
                python: None,
                version: None,
                compatible: false,
            },
            service: ServiceState::not_checked(provider),
            end_to_end: EndToEndStatus::NotRun,
            evidence,
        };
    };

    evidence.push(Evidence::fact(
        "installation_found",
        "installation markers found",
        Some(installation.clone()),
    ));
    if candidates.len() > 1 {
        evidence.push(Evidence::fact(
            "multiple_installations",
            format!(
                "{} installations found; using first sorted path",
                candidates.len()
            ),
            None,
        ));
    }

    let (model_directory, required_files) = model_spec(provider, &installation);
    let missing_model_files = required_files
        .iter()
        .filter(|relative| !model_directory.join(relative).is_file())
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if missing_model_files.is_empty() {
        evidence.push(Evidence::fact(
            "model_markers_complete",
            format!("{} required model markers found", required_files.len()),
            Some(model_directory.clone()),
        ));
    } else {
        evidence.push(Evidence::fact(
            "model_files_missing",
            format!("missing: {}", missing_model_files.join(", ")),
            Some(model_directory.clone()),
        ));
    }

    let runtime = inspect_runtime(&installation);
    if runtime.compatible {
        evidence.push(Evidence::fact(
            "runtime_compatible",
            runtime
                .version
                .clone()
                .unwrap_or_else(|| "compatible Python runtime marker found".to_owned()),
            runtime.python.clone(),
        ));
    } else {
        evidence.push(Evidence::fact(
            "runtime_incompatible",
            runtime
                .version
                .clone()
                .unwrap_or_else(|| "isolated Python runtime is missing".to_owned()),
            runtime.python.clone(),
        ));
    }

    let service = if probe_services {
        service_probe.probe(provider)
    } else {
        ServiceState::not_checked(provider)
    };
    evidence.push(Evidence::fact(
        "service_probe",
        service.detail.clone(),
        None,
    ));

    let status = if !missing_model_files.is_empty() {
        ProviderStatus::ModelMissing
    } else if !runtime.compatible {
        ProviderStatus::Incompatible
    } else {
        match service.status {
            ServiceStatus::Ready => ProviderStatus::Ready,
            ServiceStatus::NotRunning => ProviderStatus::NotRunning,
            ServiceStatus::NotChecked => ProviderStatus::Discovered,
            ServiceStatus::UnexpectedResponse | ServiceStatus::Error => ProviderStatus::Error,
        }
    };

    ProviderReport {
        provider,
        display_name: provider.display_name().to_owned(),
        status,
        capabilities: Capability::for_provider(provider),
        installation: Some(installation),
        model_directory: Some(model_directory),
        missing_model_files,
        runtime,
        service,
        end_to_end: EndToEndStatus::NotRun,
        evidence,
    }
}

fn add_explicit_candidate(
    expected: ProviderId,
    path: &Path,
    found: &mut Vec<(ProviderId, PathBuf)>,
    evidence: &mut Vec<Evidence>,
) {
    let Ok(canonical) = path.canonicalize() else {
        evidence.push(Evidence::fact(
            "explicit_installation_invalid",
            "configured installation path is unavailable",
            Some(path.to_path_buf()),
        ));
        return;
    };
    if identify_installation(&canonical) == Some(expected) {
        found.push((expected, canonical));
    } else {
        evidence.push(Evidence::fact(
            "explicit_installation_invalid",
            "configured path does not match the requested provider markers",
            Some(canonical),
        ));
    }
}

fn model_spec(provider: ProviderId, installation: &Path) -> (PathBuf, Vec<PathBuf>) {
    match provider {
        ProviderId::IndexTts25 => (
            installation.join("checkpoints"),
            [
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
            ]
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        ),
        ProviderId::VoxCpm2 => (
            installation.join("pretrained_models/VoxCPM2"),
            [
                "config.json",
                "model.safetensors",
                "audiovae.pth",
                "tokenizer.json",
                "tokenization_voxcpm2.py",
            ]
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        ),
    }
}

fn inspect_runtime(installation: &Path) -> RuntimeState {
    let python = installation.join(".venv/bin/python");
    if !python.is_file() {
        return RuntimeState {
            python: None,
            version: None,
            compatible: false,
        };
    }
    let pyvenv = fs::read_to_string(installation.join(".venv/pyvenv.cfg")).ok();
    let version = pyvenv.as_deref().and_then(|contents| {
        contents
            .lines()
            .find_map(|line| line.strip_prefix("version_info = "))
            .map(str::trim)
            .map(str::to_owned)
    });
    let compatible = version
        .as_deref()
        .and_then(|version| {
            let mut parts = version.split('.');
            Some((
                parts.next()?.parse::<u8>().ok()?,
                parts.next()?.parse::<u8>().ok()?,
            ))
        })
        .is_some_and(|(major, minor)| major == 3 && (10..13).contains(&minor));

    RuntimeState {
        python: Some(python),
        version,
        compatible,
    }
}
