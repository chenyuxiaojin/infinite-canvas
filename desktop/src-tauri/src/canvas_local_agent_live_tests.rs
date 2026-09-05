//! Explicit opt-in: one user-authorized read-only prompt per provider, guarded by a durable ledger.
use super::*;
use std::sync::mpsc::{channel, Receiver};

struct OwnedSession(Arc<Session>);
impl Drop for OwnedSession {
    fn drop(&mut self) {
        self.0.stop();
    }
}

fn next_event(
    s: &Session,
    rx: &Receiver<Value>,
    evidence: &mut Value,
    summary: &Value,
) -> Result<Value, String> {
    let value = rx
        .recv_timeout(Duration::from_secs(300))
        .map_err(|e| format!("等待模型事件：{e}"))?;
    if value["event"] == "error" {
        return Err(value["message"].as_str().unwrap_or("CLI failure").into());
    }
    if value["event"] == "canvas_tool" {
        evidence["toolCalls"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name":value["name"],"arguments":value["arguments"]}));
        let result = if value["name"] == "get_canvas_summary"
            && evidence["toolCalls"].as_array().unwrap().len() == 1
        {
            summary.clone()
        } else {
            json!({"ok":false,"message":"本次只允许一次只读摘要，不执行其他操作"})
        };
        let id = value["id"].as_str().ok_or("tool id missing")?;
        s.tool_requests
            .lock()
            .unwrap()
            .remove(id)
            .ok_or("tool gone")?
            .send(result)
            .map_err(|_| "tool cancelled")?;
    }
    if value["method"] == "session/request_permission" {
        evidence["permissionRequests"]
            .as_array_mut()
            .unwrap()
            .push(value["params"].clone());
        let call = &value["params"]["toolCall"];
        let read_only = call.to_string().contains("get_canvas_summary");
        let option = value["params"]["options"]
            .as_array()
            .and_then(|v| v.iter().find(|item| item["kind"] == "allow_once"));
        let outcome = if read_only && option.is_some() {
            json!({"outcome":"selected","optionId":option.unwrap()["optionId"]})
        } else {
            json!({"outcome":"cancelled"})
        };
        s.write(&json!({"jsonrpc":"2.0","id":value["id"],"result":{"outcome":outcome}}))?;
    }
    if value["event"] == "init" {
        evidence["init"] = value.clone();
    }
    if value["event"] == "step_update" && value["step_update"]["step_type"] == "tool" {
        evidence["nativeToolEvents"]
            .as_array_mut()
            .unwrap()
            .push(value["step_update"].clone());
    }
    if value["method"] == "session/update" {
        let update = &value["params"]["update"];
        if update["sessionUpdate"] == "agent_message_chunk" {
            let text = evidence["reply"].as_str().unwrap_or("").to_owned()
                + update["content"]["text"].as_str().unwrap_or("");
            evidence["reply"] = json!(text);
        }
    }
    Ok(value)
}
fn rpc(
    s: &Session,
    rx: &Receiver<Value>,
    id: u64,
    method: &str,
    params: Value,
    evidence: &mut Value,
    summary: &Value,
) -> Result<Value, String> {
    s.write(&prepare(
        s,
        json!({"id":id,"method":method,"params":params}),
    )?)?;
    loop {
        let event = next_event(s, rx, evidence, summary)?;
        if event["id"] == id && event.get("method").is_none() && event.get("event").is_none() {
            if let Some(error) = event.get("error") {
                return Err(error.to_string());
            }
            return Ok(event["result"].clone());
        }
    }
}

#[test]
#[ignore = "requires explicit CANVAS_READONLY_PROVIDER and evidence directory; sends exactly one paid prompt"]
fn real_local_agent_readonly_once() {
    let provider =
        std::env::var("CANVAS_READONLY_PROVIDER").expect("explicit provider authorization");
    assert!(matches!(provider.as_str(), "grok" | "antigravity"));
    let directory = PathBuf::from(
        std::env::var("CANVAS_READONLY_EVIDENCE").expect("private evidence directory"),
    );
    let summary: Value =
        serde_json::from_slice(&std::fs::read(directory.join("readonly-summary.json")).unwrap())
            .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let (tx, rx) = channel();
    let mut s = tests::session(temp.path(), &provider);
    let mutable = Arc::get_mut(&mut s).unwrap();
    mutable.tools = json!([{"name":"get_canvas_summary","description":"只读读取当前绑定画布的真实标题与节点数","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":{"readOnlyHint":true}}]);
    mutable.channel = Channel::new(move |body| {
        if let tauri::ipc::InvokeResponseBody::Json(value) = body {
            let _ = tx.send(serde_json::from_str(&value).unwrap());
        }
        Ok(())
    });
    let _guard = OwnedSession(s.clone());
    let mut evidence = json!({"provider":provider,"promptCount":0,"toolCalls":[],"permissionRequests":[],"nativeToolEvents":[],"reply":"","snapshot":summary,"completed":false});
    let outcome = (|| -> Result<(), String> {
        tauri::async_runtime::block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let path = format!(
                "/mcp/{}",
                session_key(
                    &provider,
                    "readonly",
                    "fixture",
                    &temp.path().to_string_lossy()
                )
                .unwrap()
            );
            *s.endpoint.lock().unwrap() = format!(
                "http://127.0.0.1:{}{path}",
                listener.local_addr().unwrap().port()
            );
            let app = Router::new().route(&path, post(mcp)).with_state(s.clone());
            *s.http.lock().unwrap() = Some(tauri::async_runtime::spawn(async move {
                let _ = axum::serve(listener, app).await;
            }));
        });
        let log = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(directory.join(format!("{provider}-stderr.log")))
            .map_err(|e| e.to_string())?;
        let mut command = cli_command(
            &s,
            temp.path(),
            &format!("readonly-{provider}-{}", temp.path().display()),
            None,
        )?;
        command.stderr(Stdio::from(log));
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let stdout = child.stdout.take().unwrap();
        *s.stdin.lock().unwrap() = child.stdin.take();
        *s.child.lock().unwrap() = Some(child);
        let reader = s.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<(), String> {
                for line in BufReader::new(stdout).lines() {
                    reader.receive(
                        serde_json::from_str(&line.map_err(|e| e.to_string())?)
                            .map_err(|e| e.to_string())?,
                    )?;
                }
                Err("CLI stdout closed".into())
            })();
            if !reader.stopped.load(Ordering::SeqCst) {
                let _ = reader.emit(json!({"event":"error","message":result.unwrap_err()}));
            }
        });
        if provider == "grok" {
            let init = rpc(&s, &rx, 1, "initialize", json!({}), &mut evidence, &summary)?;
            evidence["capabilities"] = init["agentCapabilities"].clone();
            rpc(
                &s,
                &rx,
                2,
                "authenticate",
                json!({}),
                &mut evidence,
                &summary,
            )?;
            let session = rpc(
                &s,
                &rx,
                3,
                "session/new",
                json!({}),
                &mut evidence,
                &summary,
            )?;
            evidence["sessionId"] = session["sessionId"].clone();
        } else {
            loop {
                if next_event(&s, &rx, &mut evidence, &summary)?["event"] == "init" {
                    break;
                }
            }
        }
        let prompt = "本次只读验收，请仅调用一次 xiaochens_canvas_sidepanel MCP 的 get_canvas_summary，读取绑定画布的真实标题和节点数，然后用中文只报告标题和节点数。不要调用任何其他工具，不生成媒体、不修改节点或文件、不重试。如果该工具不可用或未获权限，直接说明，不用其他方法替代。";
        let ledger = directory.join(format!("{provider}-prompt-reserved.json"));
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(ledger)
            .map_err(|_| "该服务已保留过一次提示发送，禁止自动重试")?;
        file.write_all(
            json!({"provider":provider,"prompt":prompt,"reserved":true})
                .to_string()
                .as_bytes(),
        )
        .map_err(|e| e.to_string())?;
        evidence["promptCount"] = json!(1);
        if provider == "grok" {
            let remote = s.remote_id.lock().unwrap().clone().unwrap();
            let result = rpc(
                &s,
                &rx,
                4,
                "session/prompt",
                json!({"sessionId":remote,"prompt":[{"type":"text","text":prompt}]}),
                &mut evidence,
                &summary,
            )?;
            evidence["result"] = result.clone();
            if result["stopReason"] != "end_turn" {
                return Err(format!("Grok stopReason: {}", result["stopReason"]));
            }
        } else {
            s.write(&prepare(
                &s,
                json!({"event":"user","message":{"content":prompt}}),
            )?)?;
            loop {
                let event = next_event(&s, &rx, &mut evidence, &summary)?;
                if event["event"] == "result" {
                    evidence["result"] = event["result"].clone();
                    evidence["reply"] = event["result"]["response"].clone();
                    if event["result"]["status"] != "SUCCESS" {
                        return Err(event["result"]["error"].to_string());
                    }
                    break;
                }
            }
        }
        if evidence["toolCalls"].as_array().unwrap().len() != 1 {
            return Err("没有恰好一次实际画布工具调用".into());
        }
        let reply = evidence["reply"].as_str().unwrap_or("");
        if !reply.contains(summary["project"]["title"].as_str().unwrap())
            || !reply.contains(&summary["project"]["nodeCount"].to_string())
        {
            return Err("最终回复未正确包含标题与节点数".into());
        }
        evidence["completed"] = json!(true);
        Ok(())
    })();
    s.stop();
    evidence["error"] = json!(outcome.as_ref().err());
    let output = directory.join(format!("{provider}-readonly-result.json"));
    std::fs::write(&output, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    println!(
        "provider={} completed={} tool_calls={} prompt_count={} evidence={}",
        provider,
        evidence["completed"],
        evidence["toolCalls"].as_array().unwrap().len(),
        evidence["promptCount"],
        output.display()
    );
    assert!(outcome.is_ok(), "{}", outcome.unwrap_err());
}

#[test]
fn antigravity_endpoint_resume_preserves_address() {
    tauri::async_runtime::block_on(async {
        let temp = tempfile::tempdir().unwrap();
        let (listener, path) = bind_endpoint(temp.path(), "chat", true, false)
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        assert!(bind_endpoint(temp.path(), "chat", true, true)
            .await
            .is_err());
        drop(listener);
        let (restored, restored_path) = bind_endpoint(temp.path(), "chat", true, true)
            .await
            .unwrap();
        assert_eq!(address, restored.local_addr().unwrap());
        assert_eq!(path, restored_path);
        assert!(bind_endpoint(temp.path(), "other", true, true)
            .await
            .is_err());
    });
}
#[test]
fn antigravity_fallback_is_rejected_before_ready() {
    let temp = tempfile::tempdir().unwrap();
    let s = tests::session(temp.path(), "antigravity");
    let log = temp.path().join("fallback.log");
    std::fs::write(&log, "Agent not found, falling back to default").unwrap();
    *s.diagnostics_file.lock().unwrap() = Some(log);
    assert!(s
        .receive(json!({"event":"init","conversation_id":"wrong"}))
        .is_err());
    assert!(!s.ready.load(Ordering::SeqCst));
    assert!(!s.owner_file.exists());
}
#[test]
#[ignore = "real CLI startup/resume only; zero user messages and zero model requests"]
fn antigravity_real_startup_resume_without_model() {
    let temp = tempfile::tempdir().unwrap();
    let mut remote: Option<String> = None;
    let mut original_endpoint = String::new();
    for round in 0..2 {
        let s = tests::session(temp.path(), "antigravity");
        let _guard = OwnedSession(s.clone());
        let (listener, path) =
            tauri::async_runtime::block_on(bind_endpoint(temp.path(), "probe", true, round == 1))
                .unwrap();
        let endpoint = format!(
            "http://127.0.0.1:{}{path}",
            listener.local_addr().unwrap().port()
        );
        if round == 0 {
            original_endpoint = endpoint.clone();
        } else {
            assert_eq!(endpoint, original_endpoint);
        }
        *s.endpoint.lock().unwrap() = endpoint.clone();
        let mut command = cli_command(
            &s,
            temp.path(),
            &format!("zero-model-{}-{round}", temp.path().display()),
            remote.as_deref(),
        )
        .unwrap();
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"{\"event\":\"control_request\"}\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        // Unsupported control_request returns a nonzero exit after init; no model turn is sent.
        let events: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        let init = events.iter().find(|v| v["event"] == "init").unwrap();
        s.receive(init.clone()).unwrap();
        assert!(s.ready.load(Ordering::SeqCst));
        let id = init["conversation_id"].as_str().unwrap();
        if let Some(ref expected) = remote {
            assert_eq!(id, expected);
        }
        remote = Some(id.to_owned());
        let db = PathBuf::from(std::env::var("HOME").unwrap())
            .join(".gemini/antigravity-cli/conversations")
            .join(format!("{id}.db"));
        let db =
            rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        let blob: Vec<u8> = db
            .query_row(
                "SELECT data FROM trajectory_metadata_blob LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let has = |needle: &[u8]| blob.windows(needle.len()).any(|w| w == needle);
        assert!(has(endpoint.as_bytes()));
        assert!(has(MCP_NAME.as_bytes()));
        assert!(has(b"call_mcp_tool") && has(b"finish"));
        assert!(!has(b"run_command") && !has(b"generate_image"));
        let generations: i64 = db
            .query_row("SELECT COUNT(*) FROM gen_metadata", [], |row| row.get(0))
            .unwrap();
        assert_eq!(generations, 0);
        println!("round={round} agent_snapshot=true endpoint_preserved=true model_generations=0");
        s.stop();
        drop(listener);
    }
}
