use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use base64::Engine;
use local_agent_adapter::{CanonicalCanvasAdapter, CanvasOperationAdapter, VideoIngestRequest};
use local_executor::TaskStatus;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::State;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use crate::{
    agent_bridge::DesktopAgentBridge, local_media::LocalMediaManager, runtime::DesktopRuntime,
};

const CONFIG_DIRECTORY: &str = "paid-generation";
const CONFIG_FILE: &str = "config.json";
const MAX_KEYFRAME_BYTES: u64 = 30 * 1024 * 1024;
const MAX_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024;
const PROVIDER_POLL_INTERVAL: Duration = Duration::from_secs(8);
const PROVIDER_TIMEOUT: Duration = Duration::from_secs(20 * 60);
const EXECUTOR_POLL_INTERVAL: Duration = Duration::from_millis(500);
const EXECUTOR_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PaidGenerationConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub price_yuan_per_second: HashMap<String, f64>,
}

pub(crate) fn config_path(app_data_directory: &Path) -> PathBuf {
    app_data_directory.join(CONFIG_DIRECTORY).join(CONFIG_FILE)
}

pub(crate) fn load_config(app_data_directory: &Path) -> Result<PaidGenerationConfig, String> {
    let path = config_path(app_data_directory);
    if !path.exists() {
        write_config_template(&path)?;
        return Err(format!(
            "付费生成尚未配置：请在 {} 填入 base_url 与 api_key",
            path.display()
        ));
    }
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("cannot read the paid generation config: {error}"))?;
    let config: PaidGenerationConfig = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid paid generation config: {error}"))?;
    if config.base_url.trim().is_empty()
        || !config.base_url.starts_with("http")
        || config.api_key.trim().is_empty()
        || config.model.trim().is_empty()
    {
        return Err(format!(
            "付费生成配置不完整：请在 {} 填入 base_url 与 api_key",
            path.display()
        ));
    }
    Ok(config)
}

pub(crate) fn quote(
    config: &PaidGenerationConfig,
    resolution: &str,
    duration_seconds: u64,
) -> Option<Value> {
    let unit = *config.price_yuan_per_second.get(resolution)?;
    if !unit.is_finite() || unit < 0.0 {
        return None;
    }
    let estimated = (unit * duration_seconds as f64 * 100.0).round() / 100.0;
    Some(json!({
        "configured": true,
        "model": config.model,
        "estimated_cost_yuan": estimated,
        "unit_price_yuan_per_second": unit,
        "currency": "CNY"
    }))
}

fn write_config_template(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "the paid generation config has no parent directory".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create the paid generation config directory: {error}"))?;
    let template = json!({
        "base_url": "",
        "api_key": "",
        "model": "MiniMax-H3",
        "price_yuan_per_second": { "768P": 0.09 }
    });
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
    };
    #[cfg(not(unix))]
    let file = OpenOptions::new().write(true).create_new(true).open(path);
    let mut file = file
        .map_err(|error| format!("cannot create the paid generation config template: {error}"))?;
    serde_json::to_writer_pretty(&mut file, &template)
        .map_err(|error| format!("cannot write the paid generation config template: {error}"))?;
    file.write_all(b"\n")
        .map_err(|error| format!("cannot finish the paid generation config template: {error}"))?;
    Ok(())
}

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn apply_batch_with_retry(
    canvas: &CanonicalCanvasAdapter,
    project_id: &str,
    actor: &str,
    request_label: &str,
    operations: Value,
) -> Result<(), String> {
    for attempt in 0..3 {
        let revision = canvas
            .get_project(project_id)
            .map_err(|error| error.to_string())?
            .revision;
        let batch = json!({
            "protocolVersion": 1,
            "actor": actor,
            "requestId": format!("{request_label}-{revision}-{attempt}"),
            "projectId": project_id,
            "baseRevision": revision,
            "timestamp": now_rfc3339(),
            "operations": operations,
        });
        match canvas.apply_protocol_batch(project_id, batch, false) {
            Ok(_) => return Ok(()),
            Err(error) if error.code.contains("STALE") && attempt < 2 => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
    Err("the canvas batch kept hitting stale revisions".to_owned())
}

fn task_snapshot(
    canvas: &CanonicalCanvasAdapter,
    project_id: &str,
    task_id: &str,
) -> Result<Value, String> {
    let document = canvas
        .get_project(project_id)
        .map_err(|error| error.to_string())?;
    let task = document.project["operationState"]["tasks"][task_id].clone();
    if !task.is_object() {
        return Err(format!("找不到画布任务 {task_id}"));
    }
    Ok(task)
}

fn fail_task(
    canvas: &CanonicalCanvasAdapter,
    project_id: &str,
    task_id: &str,
    node_id: &str,
    message: &str,
) {
    let _ = apply_batch_with_retry(
        canvas,
        project_id,
        "system",
        &format!("system-paidgen-{task_id}-failed"),
        json!([
            {
                "type": "task.update",
                "taskId": task_id,
                "status": "failed",
                "details": { "error": message, "failedAt": now_rfc3339() }
            },
            {
                "type": "node.update",
                "nodeId": node_id,
                "patch": { "metadata": { "status": "error", "errorDetails": message, "progress": 0 } }
            }
        ]),
    );
}

struct ProviderTaskState {
    status: String,
    video_url: Option<String>,
    error: Option<String>,
}

fn h3_create_task(
    config: &PaidGenerationConfig,
    prompt: &str,
    image_data_uri: &str,
    resolution: &str,
    duration_seconds: u64,
) -> Result<String, String> {
    let endpoint = format!(
        "{}/v2/video_generation",
        config.base_url.trim_end_matches('/')
    );
    let body = json!({
        "model": config.model,
        "content": [
            { "type": "text", "text": prompt },
            { "type": "image_url", "image_url": { "url": image_data_uri }, "role": "first_frame" }
        ],
        "resolution": resolution,
        "duration": duration_seconds,
        "ratio": "adaptive"
    });
    let response = ureq::post(&endpoint)
        .set("Authorization", &format!("Bearer {}", config.api_key))
        .timeout(Duration::from_secs(120))
        .send_json(body)
        .map_err(|error| format!("付费生成提交失败: {}", redact_provider_error(error)))?;
    let value: Value = response
        .into_json()
        .map_err(|error| format!("付费生成提交响应无效: {error}"))?;
    ["/task/id", "/task/task_id", "/task_id", "/id"]
        .iter()
        .find_map(|pointer| value.pointer(pointer))
        .and_then(provider_task_id_value)
        .ok_or_else(|| "付费生成提交响应缺少任务 ID".to_owned())
}

fn provider_task_id_value(value: &Value) -> Option<String> {
    if let Some(id) = value.as_str() {
        if !id.trim().is_empty() {
            return Some(id.trim().to_owned());
        }
    }
    value.as_u64().map(|id| id.to_string())
}

fn h3_query_task(
    config: &PaidGenerationConfig,
    provider_task_id: &str,
) -> Result<ProviderTaskState, String> {
    let endpoint = format!(
        "{}/v2/query/video_generation/{}",
        config.base_url.trim_end_matches('/'),
        provider_task_id
    );
    let response = ureq::get(&endpoint)
        .set("Authorization", &format!("Bearer {}", config.api_key))
        .timeout(Duration::from_secs(60))
        .call()
        .map_err(|error| format!("付费生成查询失败: {}", redact_provider_error(error)))?;
    let value: Value = response
        .into_json()
        .map_err(|error| format!("付费生成查询响应无效: {error}"))?;
    let task = value
        .get("task")
        .filter(|task| task.is_object())
        .unwrap_or(&value);
    let status = ["status", "state"]
        .iter()
        .find_map(|key| task.get(*key))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let video_url = ["/content/url", "/video_url", "/content/video_url"]
        .iter()
        .find_map(|pointer| task.pointer(pointer))
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("http"))
        .map(str::to_owned);
    let error = ["/error/message", "/error", "/message"]
        .iter()
        .find_map(|pointer| task.pointer(pointer))
        .and_then(Value::as_str)
        .filter(|message| !message.trim().is_empty())
        .map(str::to_owned);
    Ok(ProviderTaskState {
        status,
        video_url,
        error,
    })
}

fn redact_provider_error(error: ureq::Error) -> String {
    match error {
        ureq::Error::Status(code, response) => {
            let body = response.into_string().unwrap_or_default();
            let trimmed: String = body.chars().take(300).collect();
            format!("HTTP {code}: {trimmed}")
        }
        ureq::Error::Transport(_) => "网络传输失败".to_owned(),
    }
}

fn h3_download(url: &str, target: &Path) -> Result<(u64, String), String> {
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("供应商返回的视频地址不是 HTTP(S)".to_owned());
    }
    let response = ureq::get(url)
        .timeout(Duration::from_secs(10 * 60))
        .call()
        .map_err(|error| format!("付费生成结果下载失败: {}", redact_provider_error(error)))?;
    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(target)
        .map_err(|error| format!("cannot create the downloaded media file: {error}"))?;
    let mut digest = Sha256::new();
    let mut total = 0u64;
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("付费生成结果读取失败: {error}"))?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > MAX_DOWNLOAD_BYTES {
            return Err("付费生成结果超过 1 GiB 边界".to_owned());
        }
        digest.update(&buffer[..read]);
        file.write_all(&buffer[..read])
            .map_err(|error| format!("付费生成结果写入失败: {error}"))?;
    }
    if total == 0 {
        return Err("付费生成结果为空".to_owned());
    }
    file.sync_all()
        .map_err(|error| format!("付费生成结果落盘失败: {error}"))?;
    Ok((total, format!("{:x}", digest.finalize())))
}

struct DriverContext {
    canvas: Arc<CanonicalCanvasAdapter>,
    runtime: Arc<DesktopRuntime>,
    local_media: Arc<LocalMediaManager>,
    project_id: String,
    task_id: String,
}

fn drive(context: &DriverContext) -> Result<(), String> {
    let canvas = context.canvas.as_ref();
    let task = task_snapshot(canvas, &context.project_id, &context.task_id)?;
    if task["kind"] != "paid_video_generation" || task["details"]["paid"] != true {
        return Err("该任务不是受控付费生成任务".to_owned());
    }
    if task["status"] != "queued" {
        return Err(format!(
            "任务状态 {} 不可执行（需要先人工批准）",
            task["status"]
        ));
    }
    let node_id = task["nodeId"]
        .as_str()
        .ok_or_else(|| "付费任务缺少节点".to_owned())?
        .to_owned();
    let details = &task["details"];
    let prompt = details["prompt"]
        .as_str()
        .filter(|prompt| !prompt.trim().is_empty())
        .ok_or_else(|| "付费任务缺少提示词".to_owned())?
        .to_owned();
    let resolution = details["resolution"]
        .as_str()
        .filter(|value| matches!(*value, "768P" | "2K"))
        .ok_or_else(|| "付费任务分辨率无效".to_owned())?
        .to_owned();
    let duration_seconds = details["durationSeconds"]
        .as_u64()
        .filter(|value| (4..=15).contains(value))
        .ok_or_else(|| "付费任务时长无效".to_owned())?;
    let image_node_id = details["imageNodeId"]
        .as_str()
        .ok_or_else(|| "付费任务缺少关键帧节点".to_owned())?
        .to_owned();

    let config = load_config(context.local_media.app_data_directory())?;

    let document = canvas
        .get_project(&context.project_id)
        .map_err(|error| error.to_string())?;
    let image_node = document.project["nodes"]
        .as_array()
        .and_then(|nodes| {
            nodes
                .iter()
                .find(|node| node["id"].as_str() == Some(image_node_id.as_str()))
        })
        .ok_or_else(|| format!("找不到关键帧节点 {image_node_id}"))?;
    let reference = serde_json::from_value(image_node["metadata"]["localMedia"].clone())
        .map_err(|_| "关键帧节点缺少受控本机媒体引用".to_owned())?;
    let image_bytes = context
        .local_media
        .read_verified_media(&reference, MAX_KEYFRAME_BYTES)?;
    let image_data_uri = format!(
        "data:{};base64,{}",
        reference.mime_type,
        base64::engine::general_purpose::STANDARD.encode(&image_bytes)
    );
    drop(image_bytes);

    apply_batch_with_retry(
        canvas,
        &context.project_id,
        "system",
        &format!("system-paidgen-{}-submitting", context.task_id),
        json!([
            { "type": "task.update", "taskId": context.task_id, "status": "running", "details": { "phase": "provider_submitting", "startedAt": now_rfc3339() } },
            { "type": "node.update", "nodeId": node_id, "patch": { "metadata": { "status": "loading", "progress": 10 } } }
        ]),
    )?;

    let provider_task_id = h3_create_task(
        &config,
        &prompt,
        &image_data_uri,
        &resolution,
        duration_seconds,
    )?;
    apply_batch_with_retry(
        canvas,
        &context.project_id,
        "system",
        &format!("system-paidgen-{}-provider", context.task_id),
        json!([
            { "type": "task.update", "taskId": context.task_id, "status": "running", "details": { "phase": "provider_running", "providerTaskId": provider_task_id } },
            { "type": "node.update", "nodeId": node_id, "patch": { "metadata": { "progress": 25 } } }
        ]),
    )?;

    let deadline = Instant::now() + PROVIDER_TIMEOUT;
    let video_url = loop {
        if Instant::now() > deadline {
            return Err("付费生成超时（20 分钟）".to_owned());
        }
        let state = h3_query_task(&config, &provider_task_id)?;
        let failed = state.status.contains("fail")
            || state.status.contains("error")
            || state.status.contains("cancel");
        if failed {
            let detail = state
                .error
                .unwrap_or_else(|| format!("状态 {}", state.status));
            return Err(format!("供应商生成失败: {detail}"));
        }
        if let Some(url) = state.video_url {
            break url;
        }
        std::thread::sleep(PROVIDER_POLL_INTERVAL);
    };

    let media_directory = context
        .runtime
        .agent_media_directory()
        .ok_or_else(|| "本机媒体执行器不可用".to_owned())?;
    let inbox_file_name = format!("{}.mp4", context.task_id);
    let inbox_path = media_directory.join("inbox").join(&inbox_file_name);
    let (_downloaded_bytes, sha256) = h3_download(&video_url, &inbox_path)?;

    apply_batch_with_retry(
        canvas,
        &context.project_id,
        "system",
        &format!("system-paidgen-{}-verifying", context.task_id),
        json!([
            { "type": "task.update", "taskId": context.task_id, "status": "running", "details": { "phase": "verifying", "outputSha256": sha256 } },
            { "type": "node.update", "nodeId": node_id, "patch": { "metadata": { "progress": 70 } } }
        ]),
    )?;

    let verify_request = VideoIngestRequest {
        project_id: context.project_id.clone(),
        node_id: node_id.clone(),
        request_id: format!("{}-verify", context.task_id),
        base_revision: 0,
        actor: local_agent_adapter::Actor::Agent,
        inbox_file_name: inbox_file_name.clone(),
        expected_sha256: sha256.clone(),
        title: "paid generation output".to_owned(),
        position: local_agent_adapter::Point { x: 0.0, y: 0.0 },
        size: local_agent_adapter::CanvasSize {
            width: 320.0,
            height: 180.0,
        },
    };
    let executor_task_id = context
        .runtime
        .submit_paid_media_verification(&verify_request)
        .map_err(|error| error.to_string())?;

    let executor_deadline = Instant::now() + EXECUTOR_TIMEOUT;
    let snapshot = loop {
        if Instant::now() > executor_deadline {
            return Err("本机媒体验收超时".to_owned());
        }
        let snapshot = context
            .runtime
            .paid_media_task(&executor_task_id)
            .map_err(|error| error.to_string())?;
        match snapshot.status {
            TaskStatus::Succeeded => break snapshot,
            TaskStatus::Failed | TaskStatus::Cancelled => {
                return Err("本机媒体验收失败：生成结果没有通过 ffprobe/解码检查".to_owned());
            }
            _ => std::thread::sleep(EXECUTOR_POLL_INTERVAL),
        }
    };
    let _ = std::fs::remove_file(&inbox_path);

    let resolution_result = crate::runtime::task_media_reference(&context.runtime, &snapshot)?;
    let media = &resolution_result.reference;
    let storage_key = media.storage_key.clone();
    let (Some(width), Some(height), Some(duration_ms)) =
        (media.width, media.height, media.duration_ms)
    else {
        return Err("生成结果缺少可用的媒体探测信息".to_owned());
    };
    apply_batch_with_retry(
        canvas,
        &context.project_id,
        "system",
        &format!("system-paidgen-{}-succeeded", context.task_id),
        json!([
            {
                "type": "task.update",
                "taskId": context.task_id,
                "status": "succeeded",
                "details": {
                    "phase": "delivered",
                    "runtimeTaskId": executor_task_id.as_str(),
                    "outputSha256": media.sha256,
                    "actualDurationMs": duration_ms,
                    "completedAt": now_rfc3339()
                }
            },
            {
                "type": "node.update",
                "nodeId": node_id,
                "patch": {
                    "metadata": {
                        "content": storage_key,
                        "storageKey": storage_key,
                        "localMedia": media,
                        "status": "success",
                        "progress": 100,
                        "naturalWidth": width,
                        "naturalHeight": height,
                        "durationMs": duration_ms,
                        "bytes": media.bytes,
                        "mimeType": "video/mp4",
                        "localTaskSha256": media.sha256,
                        "localTaskId": executor_task_id.as_str(),
                        "localCanvasTaskId": context.task_id
                    }
                }
            }
        ]),
    )
}

fn spawn_driver(context: DriverContext) {
    std::thread::Builder::new()
        .name(format!("paid-generation-{}", context.task_id))
        .spawn(move || {
            if let Err(message) = drive(&context) {
                if let Ok(task) =
                    task_snapshot(&context.canvas, &context.project_id, &context.task_id)
                {
                    if let Some(node_id) = task["nodeId"].as_str() {
                        fail_task(
                            &context.canvas,
                            &context.project_id,
                            &context.task_id,
                            node_id,
                            &message,
                        );
                    }
                }
            }
        })
        .ok();
}

#[tauri::command]
pub(crate) fn approve_paid_generation(
    bridge: State<'_, DesktopAgentBridge>,
    runtime: State<'_, Arc<DesktopRuntime>>,
    local_media: State<'_, Arc<LocalMediaManager>>,
    project_id: String,
    task_id: String,
) -> Result<Value, String> {
    let canvas = bridge.canvas();
    let task = task_snapshot(&canvas, &project_id, &task_id)?;
    if task["kind"] != "paid_video_generation" || task["details"]["paid"] != true {
        return Err("该任务不是受控付费生成任务".to_owned());
    }
    if task["status"] != "pending_approval" {
        return Err(format!("任务状态 {} 不在待批准状态", task["status"]));
    }
    // 先确认配置可用，避免批准落库后立即失败。
    load_config(local_media.app_data_directory())?;
    apply_batch_with_retry(
        &canvas,
        &project_id,
        "human",
        &format!("human-approve-{task_id}"),
        json!([{ "type": "task.approve", "taskId": task_id }]),
    )?;
    spawn_driver(DriverContext {
        canvas,
        runtime: runtime.inner().clone(),
        local_media: local_media.inner().clone(),
        project_id,
        task_id: task_id.clone(),
    });
    Ok(json!({ "approved": true, "task_id": task_id }))
}

#[tauri::command]
pub(crate) fn reject_paid_generation(
    bridge: State<'_, DesktopAgentBridge>,
    project_id: String,
    task_id: String,
    reason: Option<String>,
) -> Result<Value, String> {
    let canvas = bridge.canvas();
    let task = task_snapshot(&canvas, &project_id, &task_id)?;
    if task["status"] != "pending_approval" {
        return Err(format!("任务状态 {} 不在待批准状态", task["status"]));
    }
    let node_id = task["nodeId"]
        .as_str()
        .ok_or_else(|| "付费任务缺少节点".to_owned())?
        .to_owned();
    let reason = reason
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "已拒绝".to_owned());
    apply_batch_with_retry(
        &canvas,
        &project_id,
        "human",
        &format!("human-reject-{task_id}"),
        json!([
            { "type": "task.cancel", "taskId": task_id, "reason": reason },
            { "type": "node.update", "nodeId": node_id, "patch": { "metadata": { "status": "error", "errorDetails": format!("已拒绝：{reason}") } } }
        ]),
    )?;
    Ok(json!({ "rejected": true, "task_id": task_id }))
}
