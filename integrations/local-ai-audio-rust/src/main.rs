use local_ai_audio::{
    ApprovedInstallation, DiscoveryConfig, ProviderId, probe_all, run_smoke, verify_audio,
};
use serde::Serialize;
use std::env;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1);
    let command = arguments
        .next()
        .and_then(|value| value.into_string().ok())
        .ok_or_else(usage)?;
    match command.as_str() {
        "probe" => {
            let mut config = DiscoveryConfig::from_env()?;
            let roots = parse_roots(arguments.collect())?;
            if !roots.is_empty() {
                config.roots = roots;
            }
            print_json(&probe_all(&config))
        }
        "verify-audio" => {
            let path = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
            if arguments.next().is_some() {
                return Err(usage());
            }
            print_json(&verify_audio(&path)?)
        }
        "smoke" => {
            let provider = arguments
                .next()
                .and_then(|value| value.into_string().ok())
                .and_then(|value| parse_provider(&value))
                .ok_or_else(usage)?;
            let installation = parse_explicit_installation(arguments.collect())?;
            let approved = ApprovedInstallation::new(provider, &installation)?;
            print_json(&run_smoke(&approved)?)
        }
        _ => Err(usage()),
    }
}

fn parse_explicit_installation(arguments: Vec<std::ffi::OsString>) -> Result<PathBuf, String> {
    let mut arguments = arguments.into_iter();
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new("--install")) {
        return Err(usage());
    }
    let path = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    if arguments.next().is_some() {
        return Err(usage());
    }
    Ok(path)
}

fn parse_roots(arguments: Vec<std::ffi::OsString>) -> Result<Vec<PathBuf>, String> {
    let mut roots = Vec::new();
    let mut arguments = arguments.into_iter();
    while let Some(flag) = arguments.next() {
        if flag != "--root" {
            return Err(usage());
        }
        roots.push(arguments.next().map(PathBuf::from).ok_or_else(usage)?);
        if roots.len() > 8 {
            return Err("at most eight discovery roots are accepted".to_owned());
        }
    }
    Ok(roots)
}

fn parse_provider(value: &str) -> Option<ProviderId> {
    match value {
        "index_tts_25" => Some(ProviderId::IndexTts25),
        "vox_cpm_2" => Some(ProviderId::VoxCpm2),
        _ => None,
    }
}

fn print_json(value: &impl Serialize) -> Result<(), String> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| error.to_string())?
    );
    Ok(())
}

fn usage() -> String {
    "usage: local-ai-audio probe [--root PATH]... | verify-audio PATH | smoke <index_tts_25|vox_cpm_2> --install USER_APPROVED_PATH\nconfiguration: LOCAL_AI_AUDIO_DISCOVERY_ROOTS, LOCAL_AI_AUDIO_INDEXTTS_HOME, LOCAL_AI_AUDIO_VOXCPM_HOME".to_owned()
}
