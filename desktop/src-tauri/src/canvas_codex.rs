//! Fixed local Codex transport. Credentials and model requests stay in the official CLI.
use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, BufReader, Read, Write},
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex},
};

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{ipc::Channel, AppHandle, Manager, State};

const MAX_MESSAGE: usize = 20 * 1024 * 1024;

#[derive(Clone, Default)]
pub struct CanvasCodexManager(Arc<Mutex<HashMap<String, Arc<Session>>>>);

struct Session {
    cwd: PathBuf,
    owner_file: PathBuf,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    stopped: AtomicBool,
    authenticated: AtomicBool,
    configured: AtomicBool,
    thread_id: Mutex<Option<String>>,
    requests: Mutex<HashMap<String, String>>,
    tool_requests: Mutex<HashSet<String>>,
    config: Mutex<serde_json::Map<String, Value>>,
}

impl Session {
    fn write(&self, value: &Value) -> Result<(), String> {
        if self.stopped.load(Ordering::SeqCst) { return Err("Codex 连接已关闭".into()); }
        let mut bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_MESSAGE { return Err("本次输入过大，请减少图片或文字引用".into()); }
        bytes.push(b'\n');
        self.stdin.lock().unwrap().as_mut().ok_or("Codex 尚未就绪")?
            .write_all(&bytes).map_err(|e| format!("Codex 连接写入失败：{e}"))
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.stdin.lock().unwrap().take();
        if let Some(mut child) = self.child.lock().unwrap().take() {
            // Only this app's owned process group, never a user's existing Codex.
            if !matches!(child.try_wait(), Ok(Some(_))) {
                unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); }
                let _ = child.kill();
            }
            let _ = child.wait();
        }
    }

    fn receive(&self, mut message: Value) -> Result<Option<Value>, String> {
        if let Some(id) = message.get("id").cloned() {
            let key = id.to_string();
            if let Some(method) = message.get("method").and_then(Value::as_str) {
                if method == "item/tool/call" {
                    if message["params"]["threadId"].as_str() != self.thread_id.lock().unwrap().as_deref() {
                        return Err("Codex 工具请求与当前画布会话不一致".into());
                    }
                    self.tool_requests.lock().unwrap().insert(key);
                } else {
                    let response = match method {
                        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => json!({"id":id,"result":{"decision":"decline"}}),
                        "item/tool/requestUserInput" => json!({"id":id,"result":{"answers":{}}}),
                        _ => json!({"id":id,"error":{"code":-32601,"message":"画布仅开放画布工具；请直接向用户提问"}}),
                    };
                    self.write(&response)?;
                    return Ok(None);
                }
            } else if let Some(method) = self.requests.lock().unwrap().remove(&key) {
                if message.get("error").is_none() {
                    match method.as_str() {
                        "account/read" => {
                            let account_type = message["result"]["account"]["type"].as_str().map(str::to_owned);
                            self.authenticated.store(account_type.as_deref() == Some("chatgpt"), Ordering::SeqCst);
                            message["result"] = json!({"accountType":account_type});
                        }
                        "config/read" => {
                            if let Some(servers) = message["result"]["config"]["mcp_servers"].as_object() {
                                for name in servers.keys() {
                                    // App-server config keys are plain dotted paths, not TOML quoted
                                    // keys. config/read is redacted, so never replay server definitions.
                                    if name.is_empty() || !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-') {
                                        return Err("已有 MCP 名称含特殊字符，无法可靠隔离；未启动模型请求".into());
                                    }
                                    self.config.lock().unwrap().insert(format!("mcp_servers.{name}.enabled"), json!(false));
                                }
                            }
                            self.configured.store(true, Ordering::SeqCst);
                            message["result"] = json!({"configured":true});
                        }
                        "thread/start" | "thread/resume" => {
                            let id = message["result"]["thread"]["id"].as_str().ok_or("Codex 未返回会话 ID")?.to_owned();
                            if method == "thread/start" {
                                std::fs::write(&self.owner_file, &id).map_err(|e| format!("无法保存画布会话绑定：{e}"))?;
                            }
                            *self.thread_id.lock().unwrap() = Some(id);
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(Some(message))
    }
}

impl CanvasCodexManager {
    fn get(&self, id: &str) -> Result<Arc<Session>, String> {
        self.0.lock().unwrap().get(id).cloned().ok_or_else(|| "Codex 连接不存在或已关闭".into())
    }

    pub fn shutdown(&self) {
        let sessions: Vec<_> = self.0.lock().unwrap().drain().map(|(_, session)| session).collect();
        for session in sessions { session.stop(); }
    }
}

fn codex_executable() -> Result<PathBuf, String> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("无法找到用户目录")?);
    [home.join(".npm-global/bin/codex"), home.join(".local/bin/codex"), PathBuf::from("/opt/homebrew/bin/codex"), PathBuf::from("/usr/local/bin/codex")]
        .into_iter().find(|path| path.is_file()).ok_or_else(|| "未找到本机 Codex，请先安装官方 Codex CLI 并登录 ChatGPT".into())
}

fn isolated_config() -> serde_json::Map<String, Value> {
    let mut config = serde_json::Map::new();
    for feature in ["apps", "plugins", "hooks", "shell_tool", "unified_exec", "shell_snapshot", "multi_agent", "goals", "image_generation", "computer_use", "browser_use", "browser_use_external", "in_app_browser", "in_app_local_automation", "memories"] {
        config.insert(format!("features.{feature}"), json!(false));
    }
    config.insert("web_search".into(), json!("disabled"));
    config
}

#[tauri::command]
pub async fn canvas_codex_open(
    app: AppHandle, manager: State<'_, CanvasCodexManager>, connection_id: String,
    project_id: String, session_id: String, on_event: Channel<Value>,
) -> Result<(), String> {
    if connection_id.is_empty() || connection_id.len() > 128 || session_id.len() > 128 || project_id.is_empty() {
        return Err("画布会话标识无效".into());
    }
    let cwd = crate::project_binding::bound_canvas_workspace(&project_id)?;
    let directory = app.path().app_data_dir().map_err(|e| e.to_string())?.join("codex-canvas-sessions");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let owner_file = directory.join(format!("{:x}.txt", Sha256::digest(format!("{project_id}\0{session_id}"))));
    let session = Arc::new(Session {
        cwd, owner_file, child: Mutex::new(None), stdin: Mutex::new(None), stopped: AtomicBool::new(false),
        authenticated: AtomicBool::new(false), configured: AtomicBool::new(false), thread_id: Mutex::new(None),
        requests: Mutex::new(HashMap::new()), tool_requests: Mutex::new(HashSet::new()), config: Mutex::new(isolated_config()),
    });
    {
        let mut sessions = manager.0.lock().unwrap();
        if sessions.contains_key(&connection_id) || sessions.values().any(|other| other.owner_file == session.owner_file) {
            return Err("这个画布对话已有 Codex 正在运行".into());
        }
        sessions.insert(connection_id.clone(), session.clone());
    }
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            let mut command = Command::new(codex_executable()?);
            command.arg("app-server").current_dir(&session.cwd)
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).process_group(0);
            for (key, value) in isolated_config() { command.args(["-c", &format!("{key}={value}")]); }
            if let Some(home) = std::env::var_os("HOME") {
                let home = home.to_string_lossy();
                command.env("PATH", format!("{home}/.local/bin:{home}/.npm-global/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"));
            }
            let mut child = command.spawn().map_err(|e| format!("无法启动本机 Codex：{e}"))?;
            let stdout = child.stdout.take().ok_or("Codex 没有输出管道")?;
            *session.stdin.lock().unwrap() = child.stdin.take();
            *session.child.lock().unwrap() = Some(child);
            if session.stopped.load(Ordering::SeqCst) { session.stop(); return Err("Codex 启动已取消".into()); }
            let reader_session = session.clone();
            let reader_manager = manager.clone();
            let reader_id = connection_id.clone();
            std::thread::spawn(move || {
                let read_result = (|| -> Result<(), String> {
                    let mut reader = BufReader::new(stdout);
                    loop {
                        let mut line = Vec::new();
                        let bytes = reader.by_ref().take((MAX_MESSAGE + 1) as u64).read_until(b'\n', &mut line).map_err(|e| e.to_string())?;
                        if bytes == 0 { break; }
                        if bytes > MAX_MESSAGE { return Err("Codex 输出超过单条消息限制".into()); }
                        let value = serde_json::from_slice(&line).map_err(|e| format!("Codex 返回了无效消息：{e}"))?;
                        if let Some(message) = reader_session.receive(value)? {
                            on_event.send(message).map_err(|e| e.to_string())?;
                        }
                    }
                    Ok(())
                })();
                if !reader_session.stopped.load(Ordering::SeqCst) {
                    let _ = on_event.send(json!({"method":"canvas/closed","params":{"message":read_result.err().unwrap_or_else(|| "Codex 连接已结束".into())}}));
                }
                reader_session.stop();
                let mut sessions = reader_manager.0.lock().unwrap();
                if sessions.get(&reader_id).is_some_and(|current| Arc::ptr_eq(current, &reader_session)) { sessions.remove(&reader_id); }
            });
            Ok(())
        })();
        if result.is_err() {
            session.stop();
            let mut sessions = manager.0.lock().unwrap();
            if sessions.get(&connection_id).is_some_and(|current| Arc::ptr_eq(current, &session)) { sessions.remove(&connection_id); }
        }
        result
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn canvas_codex_send(manager: State<'_, CanvasCodexManager>, connection_id: String, message: Value) -> Result<(), String> {
    let session = manager.get(&connection_id)?;
    tauri::async_runtime::spawn_blocking(move || send_message(&session, message)).await.map_err(|e| e.to_string())?
}

fn send_message(session: &Session, message: Value) -> Result<(), String> {
    session.write(&prepare_message(session, message)?)
}

fn prepare_message(session: &Session, mut message: Value) -> Result<Value, String> {
    let id = message.get("id").cloned();
    if let Some(method) = message.get("method").and_then(Value::as_str).map(str::to_owned) {
        match method.as_str() {
            "initialize" => message["params"] = json!({"clientInfo":{"name":"xiaochens_canvas","title":"小陈的画布","version":"1.0.0"},"capabilities":{"experimentalApi":true}}),
            "initialized" => message["params"] = json!({}),
            "account/read" => message["params"] = json!({"refreshToken":false}),
            "config/read" => message["params"] = json!({"includeLayers":false,"cwd":session.cwd}),
            "thread/start" | "thread/resume" => {
                if !session.authenticated.load(Ordering::SeqCst) || !session.configured.load(Ordering::SeqCst) {
                    return Err("请先在官方 Codex 登录 ChatGPT；画布试接不使用 API 密钥".into());
                }
                if session.thread_id.lock().unwrap().is_some() { return Err("当前连接已有会话".into()); }
                let requested = &message["params"];
                let mut params = json!({"cwd":session.cwd,"approvalPolicy":"never","sandbox":"read-only","modelProvider":"openai","config":*session.config.lock().unwrap(),"baseInstructions":requested["baseInstructions"],"developerInstructions":requested["developerInstructions"]});
                if method == "thread/start" {
                    params["dynamicTools"] = requested["dynamicTools"].clone();
                    params["environments"] = json!([]);
                    params["serviceName"] = json!("xiaochens_canvas");
                } else {
                    let expected = std::fs::read_to_string(&session.owner_file).map_err(|_| "这条 Codex 会话不属于当前画布，请新建对话")?;
                    if requested["threadId"].as_str() != Some(expected.trim()) { return Err("拒绝接续其他画布或其他应用的 Codex 会话".into()); }
                    params["threadId"] = json!(expected.trim());
                    params["excludeTurns"] = json!(true);
                }
                message["params"] = params;
            }
            "turn/start" | "turn/interrupt" | "thread/compact/start" => {
                let expected = session.thread_id.lock().unwrap().clone().ok_or("Codex 会话尚未创建")?;
                if message["params"]["threadId"].as_str() != Some(&expected) { return Err("Codex 会话与当前画布不一致".into()); }
                message["params"] = match method.as_str() {
                    "turn/start" => {
                        let input = message["params"]["input"].as_array().ok_or("缺少 Codex 输入")?;
                        if input.iter().any(|item| !matches!(item["type"].as_str(), Some("text" | "image"))) { return Err("画布仅接收文字和图片引用".into()); }
                        json!({"threadId":expected,"input":input,"cwd":session.cwd,"approvalPolicy":"never","sandboxPolicy":{"type":"readOnly","networkAccess":false},"environments":[]})
                    }
                    "turn/interrupt" => json!({"threadId":expected,"turnId":message["params"]["turnId"]}),
                    _ => json!({"threadId":expected}),
                };
            }
            _ => return Err("画布未开放此 Codex 接口".into()),
        }
        if let Some(id) = id { session.requests.lock().unwrap().insert(id.to_string(), method); }
    } else {
        let id = id.ok_or("无效 Codex 工具响应")?;
        if !session.tool_requests.lock().unwrap().remove(&id.to_string()) { return Err("Codex 工具请求已处理或不存在".into()); }
        message = json!({"id":id,"result":message["result"]});
    }
    Ok(message)
}

#[tauri::command]
pub async fn canvas_codex_close(manager: State<'_, CanvasCodexManager>, connection_id: String) -> Result<(), String> {
    let session = manager.0.lock().unwrap().remove(&connection_id);
    if let Some(session) = session { tauri::async_runtime::spawn_blocking(move || session.stop()).await.map_err(|e| e.to_string())?; }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(directory: &std::path::Path) -> Session {
        Session {
            cwd: directory.to_owned(), owner_file: directory.join("owner.txt"), child: Mutex::new(None), stdin: Mutex::new(None),
            stopped: AtomicBool::new(false), authenticated: AtomicBool::new(true), configured: AtomicBool::new(true),
            thread_id: Mutex::new(None), requests: Mutex::new(HashMap::new()), tool_requests: Mutex::new(HashSet::new()), config: Mutex::new(isolated_config()),
        }
    }

    #[test]
    fn canvas_codex_blocks_auth_config_filesystem_and_shell_methods() {
        let temp = tempfile::tempdir().unwrap();
        let session = session(temp.path());
        for method in ["account/login/start", "account/logout", "config/value/write", "fs/writeFile", "process/spawn", "command/exec", "thread/shellCommand"] {
            assert!(prepare_message(&session, json!({"id":1,"method":method,"params":{}})).is_err(), "{method}");
        }
    }

    #[test]
    fn canvas_codex_overrides_permissions_and_never_accepts_caller_config() {
        let temp = tempfile::tempdir().unwrap();
        let session = session(temp.path());
        let result = prepare_message(&session, json!({"id":1,"method":"thread/start","params":{"cwd":"/","sandbox":"danger-full-access","config":{"features.apps":true},"dynamicTools":[],"baseInstructions":"canvas"}})).unwrap();
        assert_eq!(result["params"]["sandbox"], "read-only");
        assert_eq!(result["params"]["cwd"], temp.path().to_string_lossy().as_ref());
        assert_eq!(result["params"]["config"]["features.apps"], false);
        assert_eq!(result["params"]["config"]["features.shell_tool"], false);
        assert_eq!(result["params"]["config"]["features.hooks"], false);
        assert_eq!(result["params"]["config"]["features.image_generation"], false);
        assert_eq!(result["params"]["config"]["features.computer_use"], false);
        assert_eq!(result["params"]["environments"], json!([]));
        session.authenticated.store(false, Ordering::SeqCst);
        assert!(prepare_message(&session, json!({"id":2,"method":"thread/start","params":{}})).is_err());
    }

    #[test]
    fn canvas_codex_resume_requires_owned_thread_and_turn_is_scoped() {
        let temp = tempfile::tempdir().unwrap();
        let session = session(temp.path());
        std::fs::write(&session.owner_file, "owned-thread").unwrap();
        assert!(prepare_message(&session, json!({"id":1,"method":"thread/resume","params":{"threadId":"another"}})).is_err());
        assert!(prepare_message(&session, json!({"id":2,"method":"thread/resume","params":{"threadId":"owned-thread"}})).is_ok());
        *session.thread_id.lock().unwrap() = Some("owned-thread".into());
        assert!(prepare_message(&session, json!({"id":3,"method":"turn/start","params":{"threadId":"another","input":[]}})).is_err());
        assert!(prepare_message(&session, json!({"id":4,"method":"turn/start","params":{"threadId":"owned-thread","input":[{"type":"localImage","path":"/secret"}]}})).is_err());
        let turn = prepare_message(&session, json!({"id":5,"method":"turn/start","params":{"threadId":"owned-thread","input":[{"type":"text","text":"hello"}],"sandboxPolicy":{"type":"dangerFullAccess"}}})).unwrap();
        assert_eq!(turn["params"]["sandboxPolicy"], json!({"type":"readOnly","networkAccess":false}));
    }

    #[test]
    fn canvas_codex_redacts_account_and_config_and_disables_existing_mcp() {
        let temp = tempfile::tempdir().unwrap();
        let session = session(temp.path());
        session.requests.lock().unwrap().insert("1".into(), "account/read".into());
        let result = session.receive(json!({"id":1,"result":{"account":{"type":"chatgpt","email":"private@example.invalid","token":"secret"}}})).unwrap().unwrap();
        assert_eq!(result["result"], json!({"accountType":"chatgpt"}));
        session.requests.lock().unwrap().insert("2".into(), "config/read".into());
        let result = session.receive(json!({"id":2,"result":{"config":{"mcp_servers":{"canvas":{"env":{"SECRET":"private"}},"node_repl":{"tool_timeout_sec":""}},"api_key":"private"}}})).unwrap().unwrap();
        assert_eq!(result["result"], json!({"configured":true}));
        assert_eq!(session.config.lock().unwrap()["mcp_servers.canvas.enabled"], false);
        assert_eq!(session.config.lock().unwrap()["mcp_servers.node_repl.enabled"], false);
        assert!(!serde_json::to_string(&*session.config.lock().unwrap()).unwrap().contains("private"));
        session.requests.lock().unwrap().insert("3".into(), "config/read".into());
        assert!(session.receive(json!({"id":3,"result":{"config":{"mcp_servers":{"foo.bar":{}}}}})).is_err());
    }

    #[test]
    fn canvas_codex_tool_response_is_single_use() {
        let temp = tempfile::tempdir().unwrap();
        let session = session(temp.path());
        session.tool_requests.lock().unwrap().insert("7".into());
        let response = json!({"id":7,"result":{"success":true,"contentItems":[]}});
        assert!(prepare_message(&session, response.clone()).is_ok());
        assert!(prepare_message(&session, response).is_err());
    }

    #[test]
    fn canvas_codex_stop_reaps_only_owned_child() {
        let temp = tempfile::tempdir().unwrap();
        let session = session(temp.path());
        let mut child = Command::new("/bin/cat").stdin(Stdio::piped()).stdout(Stdio::null()).process_group(0).spawn().unwrap();
        let pid = child.id();
        *session.stdin.lock().unwrap() = child.stdin.take();
        *session.child.lock().unwrap() = Some(child);
        session.stop();
        session.stop();
        assert!(session.child.lock().unwrap().is_none());
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    }
}
