use std::{
    collections::HashMap,
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

pub struct TerminalManager {
    sessions: Arc<Mutex<HashMap<String, TerminalSession>>>,
}

struct TerminalSession {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Serialize, Clone)]
pub struct PtyOutputPayload {
    pub session_id: String,
    pub data: String,
}

#[derive(Serialize, Clone)]
pub struct PtyExitPayload {
    pub session_id: String,
    pub exit_code: Option<u32>,
}

#[derive(Deserialize)]
pub struct PtySpawnOptions {
    pub session_id: String,
    pub shell: Option<String>,
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

#[tauri::command]
pub fn pty_spawn(
    app: AppHandle,
    state: State<'_, TerminalManager>,
    options: PtySpawnOptions,
) -> Result<bool, String> {
    let cols = match options.cols {
        Some(c) if c > 0 => c,
        _ => 80,
    };
    let rows = match options.rows {
        Some(r) if r > 0 => r,
        _ => 24,
    };

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("failed to open PTY: {e}"))?;

    let shell = options
        .shell
        .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string()));

    let mut cmd = CommandBuilder::new(&shell);
    if shell.ends_with("zsh") || shell.ends_with("bash") {
        cmd.arg("-l");
        cmd.arg("-i");
    }

    if let Some(cwd) = options.cwd {
        if PathBuf::from(&cwd).exists() {
            cmd.cwd(cwd);
        }
    }

    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("LANG", "en_US.UTF-8");

    if let Ok(current_path) = std::env::var("PATH") {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/chenhuajin".to_owned());
        let enhanced_path = format!(
            "{home}/.local/bin:{home}/.npm-global/bin:/opt/homebrew/bin:/usr/local/bin:{current_path}"
        );
        cmd.env("PATH", enhanced_path);
    }

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("failed to spawn shell: {e}"))?;

    // Drop slave in parent process so read EOF works cleanly
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("failed to clone PTY reader: {e}"))?;

    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("failed to take PTY writer: {e}"))?;

    let session_id = options.session_id.clone();
    let reader_session_id = session_id.clone();
    let app_handle = app.clone();

    thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    let text = String::from_utf8_lossy(&buffer[..n]).to_string();
                    let payload = PtyOutputPayload {
                        session_id: reader_session_id.clone(),
                        data: text,
                    };
                    let _ = app_handle.emit("pty_data", payload.clone());
                    let _ = app_handle.emit_to("main", "pty_data", payload);
                }
                Err(_) => break,
            }
        }
        let exit_payload = PtyExitPayload {
            session_id: reader_session_id.clone(),
            exit_code: None,
        };
        let _ = app_handle.emit("pty_exit", exit_payload.clone());
        let _ = app_handle.emit_to("main", "pty_exit", exit_payload);
    });

    let mut sessions = state.sessions.lock().unwrap();
    sessions.insert(
        session_id,
        TerminalSession {
            writer,
            master: pair.master,
            child,
        },
    );

    Ok(true)
}

#[tauri::command]
pub fn pty_write(
    state: State<'_, TerminalManager>,
    session_id: String,
    data: String,
) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    if let Some(session) = sessions.get_mut(&session_id) {
        session
            .writer
            .write_all(data.as_bytes())
            .map_err(|e| format!("failed to write to PTY: {e}"))?;
        session
            .writer
            .flush()
            .map_err(|e| format!("failed to flush PTY: {e}"))?;
        Ok(())
    } else {
        Err(format!("session {session_id} not found"))
    }
}

#[tauri::command]
pub fn pty_resize(
    state: State<'_, TerminalManager>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let cols = if cols > 0 { cols } else { 80 };
    let rows = if rows > 0 { rows } else { 24 };
    let mut sessions = state.sessions.lock().unwrap();
    if let Some(session) = sessions.get_mut(&session_id) {
        session
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("failed to resize PTY: {e}"))?;
        Ok(())
    } else {
        Err(format!("session {session_id} not found"))
    }
}

#[tauri::command]
pub fn pty_terminate(state: State<'_, TerminalManager>, session_id: String) -> Result<(), String> {
    let mut sessions = state.sessions.lock().unwrap();
    if let Some(mut session) = sessions.remove(&session_id) {
        let _ = session.child.kill();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_spawn() {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new("/bin/zsh");
        cmd.arg("-l");
        let mut child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut writer = pair.master.take_writer().unwrap();
        writer.write_all(b"echo HELLO_PTY\n").unwrap();
        writer.flush().unwrap();
        let mut buf = [0u8; 1024];
        let n = reader.read(&mut buf).unwrap();
        let out = String::from_utf8_lossy(&buf[..n]);
        println!("PTY OUTPUT: {}", out);
        assert!(n > 0);
        let _ = child.kill();
    }
}
