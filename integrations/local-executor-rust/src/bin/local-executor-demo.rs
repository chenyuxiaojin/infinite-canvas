use local_executor::{
    AllowedRoot, Executor, ExecutorConfig, GenerateTestClip, OutputConflictPolicy, RootId,
    ScopedPath, SubmitOutcome, TaskAction, TaskRequest, ToolDiscoveryConfig, Toolchain,
};
use std::{env, error::Error, fs, path::PathBuf, time::Duration};

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    let toolchain = Toolchain::discover(ToolDiscoveryConfig::default())?;
    match arguments.as_slice() {
        [command] if command == "probe-tools" => {
            println!("{}", serde_json::to_string_pretty(toolchain.reports())?);
        }
        [command, directory] if command == "sample" => {
            let root_path = fs::canonicalize(PathBuf::from(directory))?;
            if !root_path.is_dir() {
                return Err("sample path must be an existing directory".into());
            }
            let root_id = RootId::new("sample")?;
            let executor = Executor::new(ExecutorConfig {
                state_directory: root_path.join(".local-executor-state"),
                allowed_roots: vec![AllowedRoot::new(root_id.clone(), &root_path)?],
                toolchain,
            })?;
            let request = TaskRequest {
                idempotency_key: "deterministic-sample-v1".to_owned(),
                timeout_ms: 30_000,
                action: TaskAction::GenerateTestClip(GenerateTestClip {
                    output: ScopedPath::new(root_id, "deterministic-sample.mp4")?,
                    duration_ms: 1_000,
                    width: 320,
                    height: 180,
                    frame_rate: 24,
                    conflict_policy: OutputConflictPolicy::Reject,
                }),
            };
            let outcome = executor.submit(request)?;
            let id = match outcome {
                SubmitOutcome::Accepted(id) | SubmitOutcome::Duplicate(id) => id,
            };
            let snapshot = executor.wait(&id, Duration::from_secs(35))?;
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
            if !snapshot.status.is_terminal() || snapshot.error.is_some() {
                return Err("sample task did not succeed".into());
            }
        }
        _ => {
            return Err(
                "usage: local-executor-demo probe-tools | sample <existing-directory>".into(),
            );
        }
    }
    Ok(())
}
