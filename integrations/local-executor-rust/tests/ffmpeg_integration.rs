use local_executor::{
    AllowedRoot, Executor, ExecutorConfig, GenerateTestClip, OutputConflictPolicy, RootId,
    ScopedPath, SubmitOutcome, TaskAction, TaskRequest, TaskResult, TaskStatus,
    ToolDiscoveryConfig, Toolchain, TranscodeToMp4,
};
use std::{process::Command, time::Duration};
use tempfile::tempdir;

#[test]
#[ignore = "requires ffmpeg and ffprobe in a trusted system directory"]
fn deterministic_sample_is_probed_fully_decoded_and_restart_stable() {
    let directory = tempdir().unwrap();
    let state_directory = directory.path().join("state");
    let root_id = RootId::new("integration").unwrap();
    let request = TaskRequest {
        idempotency_key: "integration-sample-v1".to_owned(),
        timeout_ms: 30_000,
        action: TaskAction::GenerateTestClip(GenerateTestClip {
            output: ScopedPath::new(root_id.clone(), "sample.mp4").unwrap(),
            duration_ms: 750,
            width: 320,
            height: 180,
            frame_rate: 24,
            conflict_policy: OutputConflictPolicy::Reject,
        }),
    };
    let tools = Toolchain::discover(ToolDiscoveryConfig::default()).unwrap();
    let reports = tools.reports().to_vec();
    let executor = Executor::new(ExecutorConfig {
        state_directory: state_directory.clone(),
        allowed_roots: vec![AllowedRoot::new(root_id.clone(), directory.path()).unwrap()],
        toolchain: tools.clone(),
    })
    .unwrap();
    let accepted = executor.submit(request.clone()).unwrap();
    let task_id = accepted.task_id().clone();
    let snapshot = executor.wait(&task_id, Duration::from_secs(35)).unwrap();
    assert_eq!(snapshot.status, TaskStatus::Succeeded, "{snapshot:?}");
    let TaskResult::MediaCreated { sha256, probe, .. } = snapshot.result.unwrap() else {
        panic!("expected created media result")
    };
    assert_eq!(sha256.len(), 64);
    assert!(probe.has_video());
    assert!(probe.has_audio());
    assert!(
        probe
            .duration_ms
            .is_some_and(|duration| (700..=900).contains(&duration))
    );
    let output = directory.path().join("sample.mp4");
    assert!(output.is_file());

    let ffprobe = reports
        .iter()
        .find(|report| report.name == "ffprobe")
        .unwrap();
    assert!(ffprobe.version_line.starts_with("ffprobe version "));
    let probe_status = Command::new("/opt/homebrew/bin/ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "json",
        ])
        .arg(&output)
        .status()
        .unwrap();
    assert!(probe_status.success());
    let decode_status = Command::new("/opt/homebrew/bin/ffmpeg")
        .args(["-hide_banner", "-nostdin", "-v", "error", "-xerror", "-i"])
        .arg(&output)
        .args(["-map", "0", "-f", "null", "-"])
        .status()
        .unwrap();
    assert!(decode_status.success());

    let conflict = executor.submit(TaskRequest {
        idempotency_key: "integration-conflict-v1".to_owned(),
        timeout_ms: 30_000,
        action: TaskAction::GenerateTestClip(GenerateTestClip {
            output: ScopedPath::new(root_id.clone(), "sample.mp4").unwrap(),
            duration_ms: 750,
            width: 320,
            height: 180,
            frame_rate: 24,
            conflict_policy: OutputConflictPolicy::Reject,
        }),
    });
    assert!(matches!(
        conflict,
        Err(local_executor::ExecutorError::OutputConflict)
    ));

    let transcode = executor
        .submit(TaskRequest {
            idempotency_key: "integration-transcode-v1".to_owned(),
            timeout_ms: 30_000,
            action: TaskAction::TranscodeToMp4(TranscodeToMp4 {
                input: ScopedPath::new(root_id.clone(), "sample.mp4").unwrap(),
                output: ScopedPath::new(root_id.clone(), "sample.mp4").unwrap(),
                conflict_policy: OutputConflictPolicy::UniqueSuffix,
            }),
        })
        .unwrap();
    let transcode_snapshot = executor
        .wait(transcode.task_id(), Duration::from_secs(35))
        .unwrap();
    assert_eq!(transcode_snapshot.status, TaskStatus::Succeeded);
    let TaskResult::MediaCreated {
        output: transcoded_output,
        probe: transcoded_probe,
        ..
    } = transcode_snapshot.result.unwrap()
    else {
        panic!("expected transcoded media result")
    };
    assert_eq!(
        transcoded_output.relative,
        std::path::PathBuf::from("sample-1.mp4")
    );
    assert!(transcoded_probe.has_video());
    let transcoded_path = directory.path().join("sample-1.mp4");
    let transcoded_decode_status = Command::new("/opt/homebrew/bin/ffmpeg")
        .args(["-hide_banner", "-nostdin", "-v", "error", "-xerror", "-i"])
        .arg(&transcoded_path)
        .args(["-map", "0", "-f", "null", "-"])
        .status()
        .unwrap();
    assert!(transcoded_decode_status.success());
    drop(executor);

    let reopened = Executor::new(ExecutorConfig {
        state_directory,
        allowed_roots: vec![AllowedRoot::new(root_id, directory.path()).unwrap()],
        toolchain: tools,
    })
    .unwrap();
    let duplicate = reopened.submit(request).unwrap();
    assert!(matches!(duplicate, SubmitOutcome::Duplicate(_)));
    assert_eq!(duplicate.task_id(), &task_id);
    assert_eq!(
        reopened.task(&task_id).unwrap().status,
        TaskStatus::Succeeded
    );
}
