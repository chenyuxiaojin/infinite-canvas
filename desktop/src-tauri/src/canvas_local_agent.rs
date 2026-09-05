//! Local Grok ACP / Antigravity NDJSON, with an app-owned, per-connection canvas MCP.
//! MCP envelopes follow integrations/local-agent-adapter-rust/src/mcp.rs; HTTP is Axum.
use axum::{
    extract::{DefaultBodyLimit, State as HttpState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read, Write},
    os::unix::{fs::OpenOptionsExt, process::CommandExt},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{ipc::Channel, AppHandle, Manager, State};
use tokio::sync::oneshot;

const MCP_NAME: &str = "xiaochens_canvas_sidepanel";
const MAX_MESSAGE: usize = 20 * 1024 * 1024;
#[derive(Clone, Default)]
pub struct CanvasLocalAgentManager(Arc<Mutex<HashMap<String, Arc<Session>>>>);
struct Session {
    key: String,
    provider: String,
    cwd: PathBuf,
    owner_file: PathBuf,
    config_file: Mutex<Option<PathBuf>>,
    diagnostics_file: Mutex<Option<PathBuf>>,
    ready: AtomicBool,
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    stopped: AtomicBool,
    busy: AtomicBool,
    remote_id: Mutex<Option<String>>,
    requests: Mutex<HashMap<String, String>>,
    permissions: Mutex<HashMap<String, Value>>,
    tools: Value,
    tool_requests: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    mcp_calls: Mutex<HashMap<String, (Value, Option<Value>)>>,
    sequence: AtomicU64,
    endpoint: Mutex<String>,
    http: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    channel: Channel<Value>,
}
impl Session {
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.busy.store(false, Ordering::SeqCst);
        self.tool_requests.lock().unwrap().clear();
        self.permissions.lock().unwrap().clear();
        if let Some(task) = self.http.lock().unwrap().take() {
            task.abort();
        }
        self.stdin.lock().unwrap().take();
        if let Some(mut child) = self.child.lock().unwrap().take() {
            if !matches!(child.try_wait(), Ok(Some(_))) {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
            }
            let _ = child.wait();
        }
        if let Some(path) = self.config_file.lock().unwrap().take() {
            let _ = std::fs::remove_file(path);
        }
        if let Some(path) = self.diagnostics_file.lock().unwrap().take() {
            let _ = std::fs::remove_file(path);
        }
    }
    fn write(&self, value: &Value) -> Result<(), String> {
        if self.stopped.load(Ordering::SeqCst) {
            return Err("本机助手已停止".into());
        }
        let mut bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_MESSAGE {
            return Err("本次消息过大".into());
        }
        bytes.push(b'\n');
        self.stdin
            .lock()
            .unwrap()
            .as_mut()
            .ok_or("本机助手尚未就绪")?
            .write_all(&bytes)
            .map_err(|e| e.to_string())
    }
    fn emit(&self, value: Value) -> Result<(), String> {
        self.channel.send(value).map_err(|e| e.to_string())
    }
    fn save_id(&self, id: &str) -> Result<(), String> {
        if id.is_empty() {
            return Err("本机助手未返回会话标识".into());
        }
        let mut current = self.remote_id.lock().unwrap();
        if current.as_ref().is_some_and(|expected| expected != id) {
            return Err("助手恢复的会话与当前画布不一致".into());
        }
        std::fs::write(&self.owner_file, id).map_err(|e| e.to_string())?;
        *current = Some(id.into());
        Ok(())
    }
    fn receive(&self, message: Value) -> Result<(), String> {
        if self.stopped.load(Ordering::SeqCst) {
            return Ok(());
        }
        if self.provider == "antigravity" {
            if message["event"] == "init" {
                if let Some(path) = self.diagnostics_file.lock().unwrap().as_ref() {
                    let log = std::fs::read_to_string(path)
                        .map_err(|_| "无法核对 Antigravity 启动状态，未发送模型消息")?;
                    if log.contains("not found, falling back to default") {
                        return Err("Antigravity 未能加载画布专用助手配置，已阻止退回默认助手；未发送模型消息".into());
                    }
                }
                self.ready.store(true, Ordering::SeqCst);
                self.save_id(
                    message["conversation_id"]
                        .as_str()
                        .ok_or("Antigravity 缺少会话 ID")?,
                )?;
            }
            if message["event"] == "result" {
                self.busy.store(false, Ordering::SeqCst);
                self.save_id(
                    message["result"]["conversation_id"]
                        .as_str()
                        .ok_or("Antigravity 缺少会话 ID")?,
                )?;
            }
            return self.emit(message);
        }
        if let Some(id) = message.get("id") {
            if let Some(method) = message["method"].as_str() {
                if method == "session/request_permission" {
                    self.permissions
                        .lock()
                        .unwrap()
                        .insert(id.to_string(), message["params"]["options"].clone());
                } else {
                    return self.write(&json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"画布未提供此客户端接口"}}));
                }
            } else {
                let method = self.requests.lock().unwrap().remove(&id.to_string());
                if message.get("error").is_none() {
                    match method.as_deref() {
                        Some("session/new") => self.save_id(
                            message["result"]["sessionId"]
                                .as_str()
                                .ok_or("Grok 缺少会话 ID")?,
                        )?,
                        Some("session/prompt") => {
                            self.busy.store(false, Ordering::SeqCst);
                        }
                        _ => {}
                    }
                }
            }
        }
        self.emit(message)
    }
}
impl CanvasLocalAgentManager {
    pub fn shutdown(&self) {
        let sessions: Vec<_> = self.0.lock().unwrap().drain().map(|(_, s)| s).collect();
        for session in sessions {
            session.stop();
        }
    }
}
fn session_key(
    provider: &str,
    project: &str,
    session: &str,
    connection: &str,
) -> Result<String, String> {
    if !matches!(provider, "grok" | "antigravity")
        || [project, session, connection]
            .iter()
            .any(|s| s.is_empty() || s.len() > 128)
    {
        return Err("本机助手会话标识无效".into());
    }
    Ok(format!(
        "{:x}",
        Sha256::digest(format!("{provider}\0{project}\0{session}"))
    ))
}
fn create_config(session: &Session, path: PathBuf, body: &str) -> Result<PathBuf, String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .map_err(|e| format!("无法创建本次助手配置，原文件未覆盖：{e}"))?;
    // Remember only files this connection successfully created.
    *session.config_file.lock().unwrap() = Some(path.clone());
    file.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    Ok(path)
}
fn prepare(session: &Session, mut message: Value) -> Result<Value, String> {
    if session.provider == "antigravity" {
        if !session.ready.load(Ordering::SeqCst) {
            return Err("Antigravity 尚未完成初始化，未发送模型消息".into());
        }
        if message["event"] != "user" || !message["message"]["content"].is_string() {
            return Err("Antigravity 只接受文字消息".into());
        }
        if session.busy.swap(true, Ordering::SeqCst) {
            return Err("本轮尚未完成".into());
        }
        return Ok(json!({"event":"user","message":{"content":message["message"]["content"]}}));
    }
    let id = message.get("id").cloned().ok_or("缺少 Grok 请求 ID")?;
    let method = message["method"]
        .as_str()
        .ok_or("缺少 Grok 请求方法")?
        .to_owned();
    let requested = &message["params"];
    let params = match method.as_str() {
        "initialize" => json!({"protocolVersion":1,"clientCapabilities":{}}),
        "authenticate" => json!({"methodId":"cached_token","_meta":{"headless":true}}),
        "session/new" | "session/load" => {
            let mut params = json!({"cwd":session.cwd,"mcpServers":[{"type":"http","name":MCP_NAME,"url":*session.endpoint.lock().unwrap(),"headers":[]}]});
            if method == "session/load" {
                let expected = std::fs::read_to_string(&session.owner_file)
                    .map_err(|_| "当前画布没有可恢复的 Grok 会话")?;
                if requested["sessionId"].as_str() != Some(expected.trim()) {
                    return Err("拒绝恢复其他画布会话".into());
                }
                *session.remote_id.lock().unwrap() = Some(expected.trim().into());
                params["sessionId"] = json!(expected.trim());
            }
            params
        }
        "session/prompt" => {
            let remote = session
                .remote_id
                .lock()
                .unwrap()
                .clone()
                .ok_or("Grok 会话未就绪")?;
            if requested["sessionId"].as_str() != Some(&remote) {
                return Err("Grok 会话与画布不一致".into());
            }
            let prompt = requested["prompt"].as_array().ok_or("缺少 Grok 提示")?;
            if prompt.iter().any(|part| part["type"] != "text") {
                return Err("本次接入只接受文字，不能静默丢弃图片".into());
            }
            if session.busy.swap(true, Ordering::SeqCst) {
                return Err("本轮尚未完成".into());
            }
            json!({"sessionId":remote,"prompt":prompt})
        }
        _ => return Err("画布未开放此 Grok 接口".into()),
    };
    message = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
    session
        .requests
        .lock()
        .unwrap()
        .insert(id.to_string(), method);
    Ok(message)
}

async fn mcp(
    HttpState(session): HttpState<Arc<Session>>,
    headers: HeaderMap,
    Json(request): Json<Value>,
) -> Response {
    if headers.contains_key("origin") || session.stopped.load(Ordering::SeqCst) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(id) = request.get("id").cloned() else {
        return StatusCode::ACCEPTED.into_response();
    };
    let result = match request["method"].as_str() {
        Some("initialize") => {
            json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"xiaochens-canvas","version":"1.0.0"}})
        }
        Some("ping") => json!({}),
        Some("tools/list") => json!({"tools":session.tools}),
        Some("tools/call") => {
            let name = request["params"]["name"].as_str().unwrap_or("");
            if !session.busy.load(Ordering::SeqCst)
                || !session
                    .tools
                    .as_array()
                    .is_some_and(|tools| tools.iter().any(|tool| tool["name"] == name))
            {
                return Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"当前画布工具不可用"}})).into_response();
            }
            let request_key = id.to_string();
            {
                let mut calls = session.mcp_calls.lock().unwrap();
                if let Some((params, result)) = calls.get(&request_key) {
                    if params == &request["params"] {
                        if let Some(result) = result {
                            return Json(json!({"jsonrpc":"2.0","id":id,"result":result}))
                                .into_response();
                        }
                    }
                    return Json(json!({"jsonrpc":"2.0","id":id,"error":{"code":-32600,"message":"工具请求已处理或仍在执行，不重复修改画布"}})).into_response();
                }
                calls.insert(request_key.clone(), (request["params"].clone(), None));
            }
            let call_id = session.sequence.fetch_add(1, Ordering::SeqCst).to_string();
            let (sender, receiver) = oneshot::channel();
            session
                .tool_requests
                .lock()
                .unwrap()
                .insert(call_id.clone(), sender);
            if session.emit(json!({"event":"canvas_tool","id":call_id,"name":name,"arguments":request["params"]["arguments"]})).is_err() { session.tool_requests.lock().unwrap().remove(&call_id); return StatusCode::SERVICE_UNAVAILABLE.into_response(); }
            let outcome = tokio::time::timeout(Duration::from_secs(600), receiver).await;
            session.tool_requests.lock().unwrap().remove(&call_id);
            let value = outcome.ok().and_then(Result::ok).unwrap_or_else(
                || json!({"ok":false,"message":"画布操作已停止或超时，未自动重试"}),
            );
            let result = json!({"isError":value["ok"] != true,"content":[{"type":"text","text":value.to_string()}]});
            session.mcp_calls.lock().unwrap().insert(
                request_key,
                (request["params"].clone(), Some(result.clone())),
            );
            result
        }
        _ => return Json(
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"未开放此 MCP 接口"}}),
        )
        .into_response(),
    };
    Json(json!({"jsonrpc":"2.0","id":id,"result":result})).into_response()
}

// Antigravity resumes the original agent snapshot, including its MCP URL.
async fn bind_endpoint(
    directory: &std::path::Path,
    key: &str,
    persistent: bool,
    resume: bool,
) -> Result<(tokio::net::TcpListener, String), String> {
    let file = directory.join(format!("{key}.endpoint.json"));
    let stored: Option<Value> = if persistent && resume {
        Some(
            serde_json::from_slice(&std::fs::read(&file).map_err(|_| "恢复地址缺失，请新建对话")?)
                .map_err(|_| "恢复地址损坏，请新建对话")?,
        )
    } else {
        None
    };
    let port = if let Some(ref value) = stored {
        value["port"]
            .as_u64()
            .filter(|p| *p > 0 && *p <= 65535)
            .ok_or("恢复端口无效")? as u16
    } else {
        0
    };
    let token = if let Some(ref value) = stored {
        let token = value["token"].as_str().ok_or("恢复令牌无效")?;
        if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("恢复令牌无效".into());
        }
        token.to_owned()
    } else {
        let mut random = [0u8; 32];
        getrandom::fill(&mut random).map_err(|e| e.to_string())?;
        format!("{:x}", Sha256::digest(random))
    };
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|_| "本次助手地址无法启用；恢复端口可能被占用，请关闭旧连接后重试")?;
    if persistent && !resume {
        let value =
            json!({"port":listener.local_addr().map_err(|e| e.to_string())?.port(),"token":token});
        let temp = directory.join(format!("{key}.endpoint.tmp"));
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        out.write_all(value.to_string().as_bytes())
            .map_err(|e| e.to_string())?;
        std::fs::rename(temp, file).map_err(|e| e.to_string())?;
    }
    Ok((listener, format!("/mcp/{token}")))
}

fn cli_command(
    session: &Session,
    directory: &std::path::Path,
    connection_id: &str,
    resume_id: Option<&str>,
) -> Result<Command, String> {
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("无法找到用户目录")?);
    let executable = home.join(".local/bin").join(if session.provider == "grok" {
        "grok"
    } else {
        "agy"
    });
    if !executable.is_file() {
        return Err(format!(
            "未找到本机 {} CLI，请先安装并登录",
            session.provider
        ));
    }
    let mut command = Command::new(executable);
    let unique = format!("{:x}", Sha256::digest(connection_id));
    if session.provider == "grok" {
        let profile = create_config(
            session,
            directory.join(format!("grok-{unique}.md")),
            include_str!("canvas_grok_agent.md"),
        )?;
        command
            .args([
                "--no-auto-update",
                "agent",
                "--no-leader",
                "--agent-profile",
            ])
            .arg(profile)
            .arg("stdio");
    } else {
        let diagnostics = directory.join(format!("antigravity-{unique}.log"));
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&diagnostics)
            .map_err(|e| e.to_string())?;
        *session.diagnostics_file.lock().unwrap() = Some(diagnostics.clone());
        command.arg("--log-file").arg(diagnostics);
        if resume_id.is_none() {
            // CLI 1.1.26 selects --agent before discovering workspace agents. A unique
            // temporary definition in its official global discovery directory is required.
            let agents = home.join(".gemini/config/agents");
            std::fs::create_dir_all(&agents).map_err(|e| e.to_string())?;
            let name = format!("xiaochens-canvas-{unique}");
            create_config(session, agents.join(format!("{name}.md")), &format!("---\nname: {name}\ndescription: 小陈的画布侧栏助手\nmainAgent: true\nsubagent: false\ninheritCustomizations: false\ncommandExecutionPolicy: off\ntools: [call_mcp_tool, finish]\nmcpServers:\n  - name: xiaochens_canvas_sidepanel\n    serverUrl: {}\n---\n只通过 canvas MCP 操作画布。媒体和删除由画布用户确认。不使用终端、文件、浏览器或其他外部工具。\n", session.endpoint.lock().unwrap()))?;
            command.args(["--agent", &name]);
        }
        command.args([
            "--disable-slash-commands",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
        ]);
        if let Some(id) = resume_id {
            command.args(["--conversation", id]);
        }
    }
    command.current_dir(&session.cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).process_group(0)
                .env("PATH", format!("{}/.local/bin:{}/.npm-global/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin", home.display(), home.display()));
    Ok(command)
}

#[tauri::command]
pub async fn canvas_local_agent_open(
    app: AppHandle,
    manager: State<'_, CanvasLocalAgentManager>,
    connection_id: String,
    provider: String,
    project_id: String,
    session_id: String,
    tools: Value,
    resume_id: Option<String>,
    on_event: Channel<Value>,
) -> Result<(), String> {
    let key = session_key(&provider, &project_id, &session_id, &connection_id)?;
    let cwd = crate::project_binding::bound_canvas_workspace(&project_id)?;
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("local-agent-canvas-sessions");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let owner_file = directory.join(format!("{key}.txt"));
    if let Some(ref id) = resume_id {
        if std::fs::read_to_string(&owner_file)
            .map_err(|_| "当前画布没有可恢复会话，请新建对话")?
            .trim()
            != id
        {
            return Err("拒绝恢复其他画布的会话".into());
        }
    }
    if !tools.is_array() {
        return Err("无效画布工具目录".into());
    }
    let session = Arc::new(Session {
        key: key.clone(),
        provider: provider.clone(),
        cwd: cwd.clone(),
        owner_file,
        config_file: Mutex::new(None),
        diagnostics_file: Mutex::new(None),
        ready: AtomicBool::new(false),
        child: Mutex::new(None),
        stdin: Mutex::new(None),
        stopped: AtomicBool::new(false),
        busy: AtomicBool::new(false),
        remote_id: Mutex::new(resume_id.clone()),
        requests: Mutex::new(HashMap::new()),
        permissions: Mutex::new(HashMap::new()),
        tools,
        tool_requests: Mutex::new(HashMap::new()),
        mcp_calls: Mutex::new(HashMap::new()),
        sequence: AtomicU64::new(1),
        endpoint: Mutex::new(String::new()),
        http: Mutex::new(None),
        channel: on_event,
    });
    {
        let mut sessions = manager.0.lock().unwrap();
        if sessions.contains_key(&connection_id) || sessions.values().any(|s| s.key == key) {
            return Err("这条对话已有本机助手正在运行".into());
        }
        sessions.insert(connection_id.clone(), session.clone());
    }
    let manager = manager.inner().clone();
    let result = async {
        let (listener, path) = bind_endpoint(&directory, &key, provider == "antigravity", resume_id.is_some()).await?;
        *session.endpoint.lock().unwrap() = format!("http://127.0.0.1:{}{path}", listener.local_addr().map_err(|e| e.to_string())?.port());
        let router = Router::new().route(&path, post(mcp)).layer(DefaultBodyLimit::max(MAX_MESSAGE)).with_state(session.clone());
        *session.http.lock().unwrap() = Some(tauri::async_runtime::spawn(async move { let _ = axum::serve(listener, router).await; }));
        let spawn_session = session.clone(); let spawn_manager = manager.clone(); let reader_id = connection_id.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut command = cli_command(&spawn_session, &directory, &reader_id, resume_id.as_deref())?;
            let mut child = command.spawn().map_err(|e| format!("无法启动本机助手：{e}"))?;
            let stdout = child.stdout.take().ok_or("助手没有输出管道")?;
            *spawn_session.stdin.lock().unwrap() = child.stdin.take();
            *spawn_session.child.lock().unwrap() = Some(child);
            if spawn_session.stopped.load(Ordering::SeqCst) { spawn_session.stop(); return Err("启动已取消".into()); }
            std::thread::spawn(move || {
                let result = (|| -> Result<(), String> {
                    let mut reader = BufReader::new(stdout);
                    loop {
                        let mut line = Vec::new();
                        let count = reader.by_ref().take((MAX_MESSAGE + 1) as u64).read_until(b'\n', &mut line).map_err(|e| e.to_string())?;
                        if count == 0 { break; }
                        if count > MAX_MESSAGE { return Err("本机助手输出过大".into()); }
                        spawn_session.receive(serde_json::from_slice(&line).map_err(|e| format!("本机助手消息格式错误：{e}"))?)?;
                    }
                    Ok(())
                })();
                if !spawn_session.stopped.load(Ordering::SeqCst) { let _ = spawn_session.emit(json!({"event":"error","message":result.err().unwrap_or_else(|| "本机助手连接意外结束，请检查本机登录和 CLI 状态".into())})); }
                spawn_session.stop();
                let mut sessions = spawn_manager.0.lock().unwrap();
                if sessions.get(&reader_id).is_some_and(|current| Arc::ptr_eq(current, &spawn_session)) { sessions.remove(&reader_id); }
            });
            Ok(())
        }).await.map_err(|e| e.to_string())?
    }.await;
    if result.is_err() {
        session.stop();
        manager.0.lock().unwrap().remove(&connection_id);
    }
    result
}
#[tauri::command]
pub async fn canvas_local_agent_send(
    manager: State<'_, CanvasLocalAgentManager>,
    connection_id: String,
    message: Value,
) -> Result<(), String> {
    let session = manager
        .0
        .lock()
        .unwrap()
        .get(&connection_id)
        .cloned()
        .ok_or("本机助手已停止")?;
    tauri::async_runtime::spawn_blocking(move || session.write(&prepare(&session, message)?))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn canvas_local_agent_respond(
    manager: State<'_, CanvasLocalAgentManager>,
    connection_id: String,
    id: String,
    result: Value,
) -> Result<(), String> {
    let session = manager
        .0
        .lock()
        .unwrap()
        .get(&connection_id)
        .cloned()
        .ok_or("本机助手已停止")?;
    let sender = session
        .tool_requests
        .lock()
        .unwrap()
        .remove(&id)
        .ok_or("工具请求已失效")?;
    sender.send(result).map_err(|_| "工具请求已停止".into())
}
#[tauri::command]
pub async fn canvas_local_agent_permission(
    manager: State<'_, CanvasLocalAgentManager>,
    connection_id: String,
    id: Value,
    option_id: Option<String>,
) -> Result<(), String> {
    let session = manager
        .0
        .lock()
        .unwrap()
        .get(&connection_id)
        .cloned()
        .ok_or("本机助手已停止")?;
    let options = session
        .permissions
        .lock()
        .unwrap()
        .remove(&id.to_string())
        .ok_or("权限请求已失效")?;
    let outcome = if let Some(option) = option_id {
        if !options.as_array().is_some_and(|list| {
            list.iter()
                .any(|item| item["optionId"] == option && item["kind"] == "allow_once")
        }) {
            return Err("只允许单次授权".into());
        }
        json!({"outcome":"selected","optionId":option})
    } else {
        json!({"outcome":"cancelled"})
    };
    tauri::async_runtime::spawn_blocking(move || {
        session.write(&json!({"jsonrpc":"2.0","id":id,"result":{"outcome":outcome}}))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn canvas_local_agent_close(
    manager: State<'_, CanvasLocalAgentManager>,
    connection_id: String,
) -> Result<(), String> {
    let session = manager.0.lock().unwrap().remove(&connection_id);
    if let Some(session) = session {
        tauri::async_runtime::spawn_blocking(move || session.stop())
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    pub(super) fn session(path: &std::path::Path, provider: &str) -> Arc<Session> {
        Arc::new(Session {
            key: "test".into(),
            provider: provider.into(),
            cwd: path.into(),
            owner_file: path.join("owner.txt"),
            config_file: Mutex::new(None),
            diagnostics_file: Mutex::new(None),
            ready: AtomicBool::new(false),
            child: Mutex::new(None),
            stdin: Mutex::new(None),
            stopped: AtomicBool::new(false),
            busy: AtomicBool::new(false),
            remote_id: Mutex::new(None),
            requests: Mutex::new(HashMap::new()),
            permissions: Mutex::new(HashMap::new()),
            tools: json!([{"name":"get_node","inputSchema":{"type":"object"}}]),
            tool_requests: Mutex::new(HashMap::new()),
            mcp_calls: Mutex::new(HashMap::new()),
            sequence: AtomicU64::new(1),
            endpoint: Mutex::new("http://127.0.0.1:1/mcp/test-token".into()),
            http: Mutex::new(None),
            channel: Channel::new(|_| Ok(())),
        })
    }
    #[test]
    fn local_agent_scopes_resume_and_config_to_provider_project_chat() {
        let key = session_key("grok", "film", "chat", "connection").unwrap();
        assert_ne!(
            key,
            session_key("antigravity", "film", "chat", "connection").unwrap()
        );
        assert_ne!(
            key,
            session_key("grok", "other", "chat", "connection").unwrap()
        );
        assert_ne!(
            key,
            session_key("grok", "film", "other", "connection").unwrap()
        );
        assert!(session_key("shell", "film", "chat", "connection").is_err());
        let dir = tempfile::tempdir().unwrap();
        let s = session(dir.path(), "grok");
        std::fs::write(&s.owner_file, "owned").unwrap();
        assert!(prepare(
            &s,
            json!({"id":1,"method":"session/load","params":{"sessionId":"other"}})
        )
        .is_err());
        let request = prepare(&s, json!({"id":2,"method":"session/load","params":{"sessionId":"owned","cwd":"/","mcpServers":[]}})).unwrap();
        assert_eq!(request["params"]["cwd"], json!(dir.path()));
        assert_eq!(request["params"]["mcpServers"][0]["name"], MCP_NAME);
        assert!(s.save_id("other").is_err());
    }
    #[test]
    fn local_agent_rejects_non_canvas_control_and_non_text_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let s = session(dir.path(), "grok");
        for method in [
            "auth/logout",
            "fs/writeFile",
            "terminal/create",
            "config/write",
            "session/set_model",
        ] {
            assert!(prepare(&s, json!({"id":1,"method":method})).is_err());
        }
        *s.remote_id.lock().unwrap() = Some("owned".into());
        assert!(prepare(&s, json!({"id":1,"method":"session/prompt","params":{"sessionId":"owned","prompt":[{"type":"image"}]}})).is_err());
        let a = session(dir.path(), "antigravity");
        assert!(prepare(&a, json!({"event":"user","message":{"content":"early"}})).is_err());
        a.ready.store(true, Ordering::SeqCst);
        assert!(prepare(&a, json!({"event":"control_response"})).is_err());
        assert!(prepare(
            &a,
            json!({"event":"user","message":{"content":[{"type":"image"}]}})
        )
        .is_err());
        assert!(prepare(&a, json!({"event":"user","message":{"content":"hello"}})).is_ok());
        assert!(prepare(
            &a,
            json!({"event":"user","message":{"content":"duplicate"}})
        )
        .is_err());
    }
    #[test]
    fn local_agent_cleanup_never_removes_preexisting_config() {
        let dir = tempfile::tempdir().unwrap();
        let s = session(dir.path(), "grok");
        let existing = dir.path().join("user.md");
        std::fs::write(&existing, "keep").unwrap();
        assert!(create_config(&s, existing.clone(), "overwrite").is_err());
        s.stop();
        assert_eq!(std::fs::read_to_string(existing).unwrap(), "keep");
        let s = session(dir.path(), "grok");
        let owned = dir.path().join("owned.md");
        create_config(&s, owned.clone(), "test").unwrap();
        s.stop();
        assert!(!owned.exists());
    }
    #[test]
    fn local_agent_cancellation_reaps_child_and_pending_tool() {
        let dir = tempfile::tempdir().unwrap();
        let s = session(dir.path(), "grok");
        let mut child = Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .process_group(0)
            .spawn()
            .unwrap();
        let pid = child.id();
        *s.stdin.lock().unwrap() = child.stdin.take();
        *s.child.lock().unwrap() = Some(child);
        let (tx, mut rx) = oneshot::channel();
        s.tool_requests.lock().unwrap().insert("tool".into(), tx);
        s.stop();
        s.stop();
        assert!(rx.try_recv().is_err());
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
        assert!(s.write(&json!({})).is_err());
    }
    #[test]
    fn local_agent_mcp_roundtrip_and_origin_rejection() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let dir = tempfile::tempdir().unwrap(); let s = session(dir.path(), "grok");
            let mut headers = HeaderMap::new(); headers.insert("origin", "http://evil.invalid".parse().unwrap());
            assert_eq!(mcp(HttpState(s.clone()), headers, Json(json!({"id":1,"method":"initialize"}))).await.status(), StatusCode::FORBIDDEN);
            let response = mcp(HttpState(s.clone()), HeaderMap::new(), Json(json!({"id":1,"method":"tools/list"}))).await;
            let body = axum::body::to_bytes(response.into_body(), MAX_MESSAGE).await.unwrap();
            let value: Value = serde_json::from_slice(&body).unwrap(); assert_eq!(value["result"]["tools"][0]["name"], "get_node");
            s.busy.store(true, Ordering::SeqCst);
            let call = tauri::async_runtime::spawn(mcp(HttpState(s.clone()), HeaderMap::new(), Json(json!({"id":2,"method":"tools/call","params":{"name":"get_node","arguments":{"nodeId":"a"}}}))));
            for _ in 0..100 { if !s.tool_requests.lock().unwrap().is_empty() { break; } tokio::time::sleep(Duration::from_millis(5)).await; }
            let sender = s.tool_requests.lock().unwrap().remove("1").expect("MCP must wait for frontend result");
            sender.send(json!({"ok":true,"text":"node a"})).unwrap();
            let response = call.await.unwrap(); let bytes = axum::body::to_bytes(response.into_body(), MAX_MESSAGE).await.unwrap();
            let value: Value = serde_json::from_slice(&bytes).unwrap(); assert_eq!(value["result"]["isError"], false);
            assert!(value["result"]["content"][0]["text"].as_str().unwrap().contains("node a"));
            let replay = mcp(HttpState(s.clone()), HeaderMap::new(), Json(json!({"id":2,"method":"tools/call","params":{"name":"get_node","arguments":{"nodeId":"a"}}}))).await;
            let replay_body = axum::body::to_bytes(replay.into_body(), MAX_MESSAGE).await.unwrap();
            assert_eq!(serde_json::from_slice::<Value>(&replay_body).unwrap(), value);
            assert!(s.tool_requests.lock().unwrap().is_empty());
            s.stop();
        });
    }
}

#[cfg(test)]
#[path = "canvas_local_agent_live_tests.rs"]
mod live_tests;
