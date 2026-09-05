use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Condvar, Mutex},
    thread,
};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Deserialize;
use tauri::{
    ipc::{Channel, InvokeResponseBody},
    State,
};

// Below xterm's recommended 500 KB high watermark, including bytes in transit.
const OUTPUT_BUDGET: usize = 256 * 1024;
const OUTPUT_CHUNK: usize = 16 * 1024;

#[derive(Default)]
struct OutputState {
    sent: u64,
    acknowledged: u64,
    boundaries: VecDeque<u64>,
    closed: bool,
}

struct OutputFlow {
    state: Mutex<OutputState>,
    changed: Condvar,
}

impl Default for OutputFlow {
    fn default() -> Self {
        Self {
            state: Mutex::new(OutputState::default()),
            changed: Condvar::new(),
        }
    }
}

impl OutputFlow {
    fn close(&self) {
        self.state.lock().unwrap().closed = true;
        self.changed.notify_all();
    }

    fn closed(&self) -> bool {
        self.state.lock().unwrap().closed
    }

    fn wait(&self, drained: bool) -> Result<bool, String> {
        let mut state = self.state.lock().unwrap();
        loop {
            if state.closed {
                return Ok(false);
            }
            let pending = state.sent - state.acknowledged;
            if (drained && pending == 0) || (!drained && pending < OUTPUT_BUDGET as u64) {
                return Ok(true);
            }
            // A background/paused WebView is not a failed shell. Wait without
            // a wall-clock kill policy; close/error explicitly wakes this wait.
            state = self.changed.wait(state).unwrap();
        }
    }

    fn capacity(&self) -> usize {
        let state = self.state.lock().unwrap();
        OUTPUT_CHUNK.min(OUTPUT_BUDGET - (state.sent - state.acknowledged) as usize)
    }

    fn sent(&self, bytes: usize) -> Result<bool, String> {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return Ok(false);
        }
        if bytes == 0
            || bytes > OUTPUT_CHUNK
            || state.sent - state.acknowledged + bytes as u64 > OUTPUT_BUDGET as u64
        {
            return Err("终端输出超出未消费预算".to_owned());
        }
        state.sent = state
            .sent
            .checked_add(bytes as u64)
            .ok_or("终端输出计数溢出")?;
        let boundary = state.sent;
        state.boundaries.push_back(boundary);
        Ok(true)
    }

    fn acknowledge(&self, consumed: u64) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return Err("终端会话已关闭".to_owned());
        }
        if consumed == state.acknowledged {
            return Ok(());
        } // Duplicate ACK never grants credit twice.
        let Some(index) = state
            .boundaries
            .iter()
            .position(|boundary| *boundary == consumed)
        else {
            return Err("终端消费确认无效或顺序错误".to_owned());
        };
        // Cumulative ACK may cover several already-consumed packets, but must
        // end at an exact sent boundary; no partial, future or backward credit.
        state.boundaries.drain(..=index);
        state.acknowledged = consumed;
        self.changed.notify_all();
        Ok(())
    }
}

#[derive(Default)]
struct TerminalSession {
    flow: OutputFlow,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    child: Mutex<Option<Box<dyn Child + Send + Sync>>>,
}

impl TerminalSession {
    fn install(
        &self,
        writer: Box<dyn Write + Send>,
        master: Box<dyn MasterPty + Send>,
        child: Box<dyn Child + Send + Sync>,
    ) -> Result<(), String> {
        let flow = self.flow.state.lock().unwrap();
        *self.writer.lock().unwrap() = Some(writer);
        *self.master.lock().unwrap() = Some(master);
        *self.child.lock().unwrap() = Some(child);
        // Even a late child must be owned by the cancelled slot. The caller
        // drops its reader before stop() closes the remaining PTY handles and
        // reaps it; waiting here can deadlock macOS PTY exit draining.
        if flow.closed {
            Err("终端启动已取消".to_owned())
        } else {
            Ok(())
        }
    }

    fn stop(&self) {
        self.flow.close(); // Wake budget/drain waiters before any process or IO lock.
        let mut child_slot = self.child.lock().unwrap();
        if let Some(mut child) = child_slot.take() {
            if !matches!(child.try_wait(), Ok(Some(_))) {
                #[cfg(unix)]
                if let Some(pid) = child.process_id() {
                    // Only the foreground job in this PTY's own OS session.
                    // Never signal an arbitrary supplied PID or another session.
                    if let Some(group) = self
                        .master
                        .lock()
                        .unwrap()
                        .as_ref()
                        .and_then(|master| master.process_group_leader())
                    {
                        if group > 1
                            && group != pid as i32
                            && unsafe { libc::getsid(group) } == pid as i32
                        {
                            unsafe {
                                libc::kill(-group, libc::SIGKILL);
                            }
                        }
                    }
                }
                let _ = child.kill();
            }
            // macOS can keep a killed PTY child in exiting state until the
            // unread master is closed. Close BEFORE wait, even when another
            // owner still retains this session Arc. The output worker observes
            // closed within its 100 ms poll and drops its separate reader.
            drop(self.master.lock().unwrap().take());
            drop(self.writer.lock().unwrap().take());
            let _ = child.wait(); // Reap, rather than merely sending a signal.
        }
        // No manager lock is held while closing IO handles or waiting.
    }
}

#[derive(Clone, Default)]
pub struct TerminalManager {
    sessions: Arc<Mutex<HashMap<String, Arc<TerminalSession>>>>,
}

impl TerminalManager {
    fn register(&self, id: &str) -> Result<Arc<TerminalSession>, String> {
        if id.is_empty() || id.len() > 128 {
            return Err("终端会话 ID 无效".to_owned());
        }
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.contains_key(id) {
            return Err("终端会话已存在".to_owned());
        }
        let session = Arc::new(TerminalSession::default());
        sessions.insert(id.to_owned(), session.clone());
        Ok(session)
    }

    fn get(&self, id: &str) -> Result<Arc<TerminalSession>, String> {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| format!("session {id} not found"))
    }

    fn remove(
        &self,
        id: &str,
        expected: Option<&Arc<TerminalSession>>,
    ) -> Option<Arc<TerminalSession>> {
        let mut sessions = self.sessions.lock().unwrap();
        if expected.is_some_and(|expected| {
            !sessions
                .get(id)
                .is_some_and(|current| Arc::ptr_eq(current, expected))
        }) {
            return None;
        }
        sessions.remove(id)
    }

    pub(crate) fn shutdown(&self) {
        let sessions: Vec<_> = self
            .sessions
            .lock()
            .unwrap()
            .drain()
            .map(|(_, session)| session)
            .collect();
        for session in &sessions {
            session.flow.close();
        }
        for session in sessions {
            session.stop();
        }
    }

    fn acknowledge(&self, id: &str, consumed: u64) -> Result<(), String> {
        let session = self.get(id)?;
        let result = session.flow.acknowledge(consumed);
        if result.is_err() {
            session.flow.close();
        }
        result
    }
}

#[derive(Deserialize)]
pub struct PtySpawnOptions {
    pub session_id: String,
    pub shell: Option<String>,
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

fn spawn_session(
    manager: TerminalManager,
    options: PtySpawnOptions,
    on_output: Channel<InvokeResponseBody>,
) -> Result<bool, String> {
    // Reserve before opening/spawning or sending the first packet. Termination
    // can close this slot even while the OS is still starting the shell.
    let session = manager.register(&options.session_id)?;
    let result = (|| {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: options.rows.filter(|value| *value > 0).unwrap_or(24),
                cols: options.cols.filter(|value| *value > 0).unwrap_or(80),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| format!("failed to open PTY: {error}"))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| error.to_string())?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| error.to_string())?;
        let shell = options
            .shell
            .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_owned()));
        let mut command = CommandBuilder::new(&shell);
        if shell.ends_with("zsh") || shell.ends_with("bash") {
            command.arg("-l");
            command.arg("-i");
        }
        if let Some(cwd) = options.cwd.filter(|cwd| PathBuf::from(cwd).is_dir()) {
            command.cwd(cwd);
        }
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("LANG", "en_US.UTF-8");
        if let Ok(path) = std::env::var("PATH") {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/chenhuajin".to_owned());
            command.env("PATH", format!("{home}/.local/bin:{home}/.npm-global/bin:/opt/homebrew/bin:/usr/local/bin:{path}"));
        }
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| format!("failed to spawn shell: {error}"))?;
        drop(pair.slave);
        #[cfg(unix)]
        let poll_fd = pair.master.as_raw_fd();
        #[cfg(not(unix))]
        let poll_fd: Option<i32> = None;
        session.install(writer, pair.master, child)?;
        let id = options.session_id.clone();
        let active = session.clone();
        let owner = manager.clone();
        thread::Builder::new()
            .name("canvas-pty-output".to_owned())
            .spawn(move || {
                let result = pump_output(reader, poll_fd, &active.flow, |bytes| {
                    on_output
                        .send(InvokeResponseBody::Raw(bytes))
                        .map_err(|error| error.to_string())
                });
                if let Err(error) = result {
                    if !active.flow.closed() {
                        let message = serde_json::json!({ "error": error }).to_string();
                        let _ = on_output.send(InvokeResponseBody::Json(message));
                    }
                }
                if let Some(session) = owner.remove(&id, Some(&active)) {
                    session.stop();
                }
            })
            .map_err(|error| format!("无法启动终端读取线程：{error}"))?;
        Ok(true)
    })();
    if result.is_err() {
        manager.remove(&options.session_id, Some(&session));
        // Terminate may already have removed this slot before install finished.
        // Always clean this exact Arc, never a new session reusing the same ID.
        session.stop();
    }
    result
}

fn pump_output(
    mut reader: Box<dyn Read + Send>,
    poll_fd: Option<i32>,
    flow: &OutputFlow,
    mut send: impl FnMut(Vec<u8>) -> Result<(), String>,
) -> Result<(), String> {
    let mut buffer = [0u8; OUTPUT_CHUNK];
    loop {
        if !flow.wait(false)? {
            return Ok(());
        }
        #[cfg(unix)]
        if let Some(fd) = poll_fd {
            let mut descriptor = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut descriptor, 1, 100) };
            if ready == 0 {
                continue;
            }
            if ready < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return Err("终端读取轮询失败".to_owned());
            }
            if flow.closed() {
                return Ok(());
            }
        }
        match reader.read(&mut buffer[..flow.capacity()]) {
            Ok(0) => break,
            Ok(bytes) => {
                if !flow.sent(bytes)? {
                    return Ok(());
                }
                send(buffer[..bytes].to_vec())?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            // Unix PTYs may signal a closed slave as EIO instead of a zero read.
            Err(error) if cfg!(unix) && error.raw_os_error() == Some(libc::EIO) => break,
            Err(error) => return Err(format!("终端读取失败：{error}")),
        }
    }
    if flow.wait(true)? {
        send(Vec::new())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn pty_spawn(
    state: State<'_, TerminalManager>,
    options: PtySpawnOptions,
    on_output: Channel<InvokeResponseBody>,
) -> Result<bool, String> {
    let manager = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || spawn_session(manager, options, on_output))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn pty_ack(
    state: State<'_, TerminalManager>,
    session_id: String,
    consumed: u64,
) -> Result<(), String> {
    state.acknowledge(&session_id, consumed)
}

#[tauri::command]
pub async fn pty_write(
    state: State<'_, TerminalManager>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    let session = state.get(&session_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        if session.flow.closed() {
            return Err("终端会话已关闭".to_owned());
        }
        let mut writer = session.writer.lock().unwrap();
        let writer = writer.as_mut().ok_or("终端仍在启动")?;
        writer
            .write_all(data.as_bytes())
            .and_then(|_| writer.flush())
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn pty_resize(
    state: State<'_, TerminalManager>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let session = state.get(&session_id)?;
    let master = session.master.lock().unwrap();
    master
        .as_ref()
        .ok_or("终端仍在启动")?
        .resize(PtySize {
            rows: rows.max(1),
            cols: cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn pty_terminate(
    state: State<'_, TerminalManager>,
    session_id: String,
) -> Result<(), String> {
    if let Some(session) = state.remove(&session_id, None) {
        session.flow.close();
        tauri::async_runtime::spawn_blocking(move || session.stop())
            .await
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Cursor,
        sync::{
            atomic::{AtomicU64, Ordering},
            mpsc,
        },
        time::{Duration, Instant},
    };

    fn fill(flow: &OutputFlow) {
        for _ in 0..OUTPUT_BUDGET / OUTPUT_CHUNK {
            assert!(flow.sent(OUTPUT_CHUNK).unwrap());
        }
    }

    #[test]
    fn budget_rejects_overrun_invalid_ack_and_duplicate_credit() {
        let flow = OutputFlow::default();
        fill(&flow);
        assert_eq!(flow.capacity(), 0);
        assert!(flow.sent(1).is_err());
        assert!(flow.acknowledge(1).is_err());
        assert!(flow.acknowledge((OUTPUT_BUDGET + 1) as u64).is_err());
        assert!(flow.acknowledge((OUTPUT_CHUNK * 2 - 1) as u64).is_err());
        flow.acknowledge(OUTPUT_CHUNK as u64).unwrap();
        flow.acknowledge(OUTPUT_CHUNK as u64).unwrap();
        assert_eq!(flow.state.lock().unwrap().acknowledged, OUTPUT_CHUNK as u64);
        assert!(flow.sent(OUTPUT_CHUNK).unwrap());
        assert_eq!(flow.capacity(), 0);
    }

    #[test]
    fn cumulative_ack_consumes_exact_sent_boundaries_without_duplicate_credit() {
        let flow = OutputFlow::default();
        fill(&flow);
        let end = (OUTPUT_CHUNK * 5) as u64;
        flow.acknowledge(end).unwrap();
        assert_eq!(flow.state.lock().unwrap().acknowledged, end);
        assert_eq!(flow.state.lock().unwrap().boundaries.len(), 11);
        flow.acknowledge(end).unwrap();
        assert_eq!(flow.state.lock().unwrap().boundaries.len(), 11);
        assert!(flow.acknowledge(end - 1).is_err());
        assert!(flow.acknowledge((OUTPUT_CHUNK * 4) as u64).is_err());
        assert!(flow.acknowledge((OUTPUT_BUDGET + 1) as u64).is_err());
        flow.acknowledge(OUTPUT_BUDGET as u64).unwrap();
        assert!(flow.state.lock().unwrap().boundaries.is_empty());
        assert_eq!(
            flow.state.lock().unwrap().acknowledged,
            OUTPUT_BUDGET as u64
        );
        fill(&flow);
        assert_eq!(flow.capacity(), 0);
    }

    #[test]
    fn slow_consumer_blocks_until_ack_without_holding_manager_lock() {
        let manager = TerminalManager::default();
        let session = manager.register("slow").unwrap();
        fill(&session.flow);
        let (tx, rx) = mpsc::channel();
        let active = session.clone();
        let worker = thread::spawn(move || tx.send(active.flow.wait(false)).unwrap());
        assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
        let other = manager.register("other").unwrap();
        assert!(!other.flow.closed());
        session.flow.acknowledge(OUTPUT_CHUNK as u64).unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap());
        worker.join().unwrap();
        manager.shutdown();
    }

    #[test]
    fn close_releases_capacity_and_eof_drain_waiters() {
        for drained in [false, true] {
            let flow = Arc::new(OutputFlow::default());
            fill(&flow);
            let (tx, rx) = mpsc::channel();
            let active = flow.clone();
            let worker = thread::spawn(move || tx.send(active.wait(drained)).unwrap());
            assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
            flow.close();
            assert!(!rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap());
            worker.join().unwrap();
            assert!(flow.acknowledge(OUTPUT_CHUNK as u64).is_err());
        }
    }

    #[test]
    fn rejected_ack_closes_and_releases_the_blocked_reader() {
        let manager = TerminalManager::default();
        let session = manager.register("bad-ack").unwrap();
        fill(&session.flow);
        let (tx, rx) = mpsc::channel();
        let active = session.clone();
        let worker = thread::spawn(move || tx.send(active.flow.wait(false)).unwrap());
        assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
        assert!(manager.acknowledge("bad-ack", 1).is_err());
        assert!(!rx.recv_timeout(Duration::from_secs(1)).unwrap().unwrap());
        worker.join().unwrap();
        manager.shutdown();
    }

    #[test]
    fn eof_follows_consumption_not_just_transport_delivery() {
        let flow = Arc::new(OutputFlow::default());
        let (tx, rx) = mpsc::channel();
        let active = flow.clone();
        let worker = thread::spawn(move || {
            pump_output(
                Box::new(Cursor::new(b"bytes".to_vec())),
                None,
                &active,
                |bytes| {
                    tx.send(bytes).unwrap();
                    Ok(())
                },
            )
        });
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), b"bytes");
        assert!(rx.recv_timeout(Duration::from_millis(20)).is_err());
        flow.acknowledge(5).unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(1)).unwrap().is_empty());
        worker.join().unwrap().unwrap();
    }

    #[test]
    fn pump_preserves_every_byte_with_immediate_ack_and_single_eof() {
        let flow = OutputFlow::default();
        let expected = "跨块中文😀🧪\r\n".repeat(100_000).into_bytes();
        let mut actual = Vec::new();
        let mut eof = 0;
        pump_output(
            Box::new(Cursor::new(expected.clone())),
            None,
            &flow,
            |bytes| {
                if bytes.is_empty() {
                    eof += 1;
                } else {
                    actual.extend_from_slice(&bytes);
                    flow.acknowledge(actual.len() as u64)?;
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(eof, 1);
        assert!(flow.state.lock().unwrap().boundaries.is_empty());
    }

    #[test]
    fn transport_and_reader_errors_are_not_normal_eof() {
        let flow = OutputFlow::default();
        assert!(
            pump_output(Box::new(Cursor::new(vec![1])), None, &flow, |_| Err(
                "closed channel".to_owned()
            ))
            .unwrap_err()
            .contains("closed channel")
        );
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("read error"))
            }
        }
        assert!(
            pump_output(Box::new(Broken), None, &OutputFlow::default(), |_| panic!(
                "no EOF on read failure"
            ))
            .unwrap_err()
            .contains("read error")
        );
    }

    #[test]
    fn duplicate_session_and_late_old_cleanup_cannot_replace_new_session() {
        let manager = TerminalManager::default();
        let old = manager.register("same").unwrap();
        assert!(manager.register("same").is_err());
        manager.remove("same", None).unwrap().stop();
        let new = manager.register("same").unwrap();
        assert!(manager.remove("same", Some(&old)).is_none());
        assert!(Arc::ptr_eq(&new, &manager.get("same").unwrap()));
        manager.shutdown();
        assert!(manager.sessions.lock().unwrap().is_empty());
    }

    fn options(id: &str, shell: &str) -> PtySpawnOptions {
        PtySpawnOptions {
            session_id: id.to_owned(),
            shell: Some(shell.to_owned()),
            cwd: None,
            cols: Some(80),
            rows: Some(24),
        }
    }

    struct Cleanup(TerminalManager);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.0.shutdown();
        }
    }

    #[test]
    fn failed_spawn_removes_the_reserved_session() {
        let manager = TerminalManager::default();
        assert!(spawn_session(
            manager.clone(),
            options("fail", "/nonexistent-canvas-test-shell"),
            Channel::new(|_| Ok(()))
        )
        .is_err());
        assert!(manager.sessions.lock().unwrap().is_empty());
    }

    #[test]
    fn cancellation_before_install_reaps_the_late_child() {
        let session = TerminalSession::default();
        session.flow.close();
        let pair = native_pty_system().openpty(PtySize::default()).unwrap();
        let writer = pair.master.take_writer().unwrap();
        let child = pair
            .slave
            .spawn_command(CommandBuilder::new("/bin/sh"))
            .unwrap();
        let pid = child.process_id().unwrap();
        drop(pair.slave);
        assert!(session
            .install(writer, pair.master, child)
            .unwrap_err()
            .contains("取消"));
        session.stop();
        #[cfg(unix)]
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    }

    #[test]
    fn test_pty_spawn() {
        let manager = TerminalManager::default();
        let _cleanup = Cleanup(manager.clone());
        let owner = manager.clone();
        let consumed = AtomicU64::new(0);
        let (tx, rx) = mpsc::channel();
        let output = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                if !bytes.is_empty() {
                    let end = consumed.fetch_add(bytes.len() as u64, Ordering::SeqCst)
                        + bytes.len() as u64;
                    // Earliest packet can ACK before spawn_session has returned.
                    owner.get("pty").unwrap().flow.acknowledge(end).unwrap();
                }
                tx.send(bytes).unwrap();
            } else {
                panic!("unexpected PTY error");
            }
            Ok(())
        });
        assert!(spawn_session(manager.clone(), options("pty", "/bin/sh"), output).unwrap());
        let session = manager.get("pty").unwrap();
        let pid = session
            .child
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .process_id()
            .unwrap();
        session
            .writer
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .write_all("printf 'FLOW_中文😀_OK\\n'; exit\n".as_bytes())
            .unwrap();
        let mut bytes = Vec::new();
        loop {
            let chunk = rx.recv_timeout(Duration::from_secs(3)).unwrap();
            if chunk.is_empty() {
                break;
            }
            bytes.extend_from_slice(&chunk);
        }
        assert!(String::from_utf8(bytes)
            .unwrap()
            .contains("FLOW_中文😀_OK\r\n"));
        session.stop();
        #[cfg(unix)]
        assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
    }

    #[test]
    fn real_pty_packet_shape_for_one_mib_burst() {
        let manager = TerminalManager::default();
        let _cleanup = Cleanup(manager.clone());
        let owner = manager.clone();
        let consumed = AtomicU64::new(0);
        let (tx, rx) = mpsc::channel();
        let output = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                if !bytes.is_empty() {
                    let end = consumed.fetch_add(bytes.len() as u64, Ordering::SeqCst)
                        + bytes.len() as u64;
                    owner.acknowledge("packet-shape", end).unwrap();
                }
                tx.send(bytes).unwrap();
            }
            Ok(())
        });
        spawn_session(manager.clone(), options("packet-shape", "/bin/sh"), output).unwrap();
        let session = manager.get("packet-shape").unwrap();
        session
            .writer
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .write_all(b"/usr/bin/head -c 1048576 /dev/zero; exit\n")
            .unwrap();
        let mut packets = Vec::new();
        let mut zeros = 0;
        loop {
            let bytes = rx.recv_timeout(Duration::from_secs(3)).unwrap();
            if bytes.is_empty() {
                break;
            }
            packets.push(bytes.len());
            zeros += bytes.iter().filter(|byte| **byte == 0).count();
        }
        session.stop();
        assert_eq!(zeros, 1048576);
        let bytes: usize = packets.iter().sum();
        println!("PTY_PACKET_SHAPE {{\"bytes\":{bytes},\"packets\":{},\"minimum\":{},\"maximum\":{},\"mean\":{}}}",
            packets.len(), packets.iter().min().unwrap(), packets.iter().max().unwrap(), bytes / packets.len());
    }

    #[test]
    fn real_pty_cancel_releases_idle_and_budget_blocked_reader() {
        struct Finished(mpsc::Sender<()>);
        impl Drop for Finished {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        for reason in ["idle", "budget", "shutdown", "bad-ack"] {
            let manager = TerminalManager::default();
            let _cleanup = Cleanup(manager.clone());
            let (done_tx, done_rx) = mpsc::channel();
            let finished = Finished(done_tx);
            let received = Arc::new(AtomicU64::new(0));
            let count = received.clone();
            let output = Channel::new(move |body| {
                let _keep_until_channel_drop = &finished;
                if let InvokeResponseBody::Raw(bytes) = body {
                    count.fetch_add(bytes.len() as u64, Ordering::SeqCst);
                }
                Ok(()) // Intentionally no consumer ACK.
            });
            spawn_session(manager.clone(), options("cancel", "/bin/sh"), output).unwrap();
            let session = manager.get("cancel").unwrap();
            let pid = session
                .child
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .process_id()
                .unwrap();
            if reason != "idle" {
                session
                    .writer
                    .lock()
                    .unwrap()
                    .as_mut()
                    .unwrap()
                    .write_all(b"/usr/bin/head -c 1048576 /dev/zero\n")
                    .unwrap();
                let deadline = Instant::now() + Duration::from_secs(3);
                while received.load(Ordering::SeqCst) < OUTPUT_BUDGET as u64
                    && Instant::now() < deadline
                {
                    thread::sleep(Duration::from_millis(5));
                }
                assert_eq!(received.load(Ordering::SeqCst), OUTPUT_BUDGET as u64);
            }
            #[cfg(unix)]
            let foreground = session
                .master
                .lock()
                .unwrap()
                .as_ref()
                .and_then(|master| master.process_group_leader());
            match reason {
                "shutdown" => manager.shutdown(),
                "bad-ack" => assert!(manager.acknowledge("cancel", 1).is_err()),
                _ => manager.remove("cancel", None).unwrap().stop(),
            }
            done_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("reader and channel must be released");
            assert!(manager.sessions.lock().unwrap().is_empty());
            assert!(session.child.lock().unwrap().is_none());
            assert!(session.master.lock().unwrap().is_none());
            assert!(session.writer.lock().unwrap().is_none());
            #[cfg(unix)]
            {
                assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
                if let Some(foreground) = foreground.filter(|group| *group != pid as i32) {
                    let deadline = Instant::now() + Duration::from_secs(1);
                    while unsafe { libc::kill(foreground, 0) } == 0 && Instant::now() < deadline {
                        thread::sleep(Duration::from_millis(5));
                    }
                    assert_eq!(unsafe { libc::kill(foreground, 0) }, -1);
                }
            }
        }
    }
}
