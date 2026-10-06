//! User-owned PTYs. Never forwards input to the core or persists terminal output.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
const CAPACITY: usize = 2 * 1024 * 1024;
#[derive(Default)]
struct Output {
    bytes: VecDeque<u8>,
    end: u64,
    exited: bool,
    owner: Option<String>,
}
struct Session {
    id: String,
    name: Mutex<String>,
    output: Mutex<Output>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    stop: AtomicBool,
    shell_pid: Option<u32>,
    killer: Mutex<Box<dyn portable_pty::ChildKiller + Send + Sync>>,
}
impl Session {
    fn close(&self) {
        if self.stop.swap(true, Ordering::AcqRel) {
            return;
        }
        // The shell owns a distinct Unix session. Kill its process groups, including
        // foreground jobs and background children, before releasing the PTY.
        if let Some(pid) = self.shell_pid {
            if let Ok(entries) = std::fs::read_dir("/proc") {
                for entry in entries.flatten() {
                    if let Ok(child) = entry.file_name().to_string_lossy().parse::<i32>() {
                        if unsafe { libc::getsid(child) } == pid as i32 {
                            unsafe {
                                libc::kill(child, libc::SIGKILL);
                            }
                        }
                    }
                }
            }
        }
        self.writer.lock().unwrap().take();
        self.master.lock().unwrap().take();
        let _ = self.killer.lock().unwrap().kill();
        self.output.lock().unwrap().exited = true;
    }
    fn describe(&self) -> Value {
        let out = self.output.lock().unwrap();
        json!({"id":self.id,"name":*self.name.lock().unwrap(),"exited":out.exited,"controlled":out.owner.is_some()})
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}
#[derive(Default)]
pub struct Terminals {
    sessions: Mutex<BTreeMap<String, Arc<Session>>>,
}
impl Drop for Terminals {
    fn drop(&mut self) {
        for session in self.sessions.get_mut().unwrap().values() {
            session.close();
        }
    }
}
fn name(v: &Value) -> Result<String> {
    let n = v.as_str().unwrap_or("Terminal").trim();
    ensure!(
        !n.is_empty() && n.len() <= 128 && !n.chars().any(char::is_control),
        "Invalid terminal name"
    );
    Ok(n.into())
}
impl Terminals {
    pub fn api(&self, home: &Path, p: &Value) -> Result<Value> {
        ensure!(
            unsafe { libc::geteuid() } != 0,
            "Terminal must run as the Steam session user"
        );
        let op = p["operation"].as_str().unwrap_or("list");
        if op == "list" {
            return Ok(json!(self
                .sessions
                .lock()
                .unwrap()
                .values()
                .map(|s| s.describe())
                .collect::<Vec<_>>()));
        }
        if op == "create" {
            let session_name = name(&p["name"])?;
            let mut sessions = self.sessions.lock().unwrap();
            ensure!(
                sessions.len() < 12,
                "Close a terminal before opening another"
            );
            let pair = native_pty_system().openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })?;
            let login_shell = std::fs::read_to_string("/etc/passwd")
                .ok()
                .and_then(|text| {
                    text.lines().find_map(|line| {
                        let fields: Vec<_> = line.split(':').collect();
                        (fields.len() >= 7 && fields[2] == unsafe { libc::geteuid() }.to_string())
                            .then(|| fields[6].to_string())
                    })
                });
            let shell = login_shell
                .or_else(|| std::env::var("SHELL").ok())
                .filter(|s| Path::new(s).is_absolute() && Path::new(s).is_file())
                .unwrap_or_else(|| "/bin/bash".into());
            let mut cmd = CommandBuilder::new(shell);
            cmd.arg("-l");
            cmd.cwd(home);
            cmd.env("TERM", "xterm-256color");
            cmd.env("COLORTERM", "truecolor");
            let mut child = pair.slave.spawn_command(cmd)?;
            drop(pair.slave);
            let mut reader = pair.master.try_clone_reader()?;
            if let Some(fd) = pair.master.as_raw_fd() {
                unsafe {
                    let flags = libc::fcntl(fd, libc::F_GETFL);
                    ensure!(
                        flags >= 0 && libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) >= 0,
                        "Cannot configure PTY"
                    );
                }
            }
            let s = Arc::new(Session {
                id: hex::encode(rand::random::<[u8; 16]>()),
                name: Mutex::new(session_name),
                shell_pid: child.process_id(),
                output: Mutex::default(),
                writer: Mutex::new(Some(pair.master.take_writer()?)),
                master: Mutex::new(Some(pair.master)),
                stop: AtomicBool::new(false),
                killer: Mutex::new(child.clone_killer()),
            });
            let read_session = s.clone();
            std::thread::spawn(move || {
                let mut bytes = [0u8; 16384];
                while !read_session.stop.load(Ordering::Acquire) {
                    match reader.read(&mut bytes) {
                        Ok(0) => break,
                        Ok(n) => {
                            let mut out = read_session.output.lock().unwrap();
                            out.bytes.extend(&bytes[..n]);
                            out.end += n as u64;
                            while out.bytes.len() > CAPACITY {
                                out.bytes.pop_front();
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(15))
                        }
                        Err(_) => break,
                    }
                }
            });
            let wait_session = s.clone();
            std::thread::spawn(move || {
                let _ = child.wait();
                wait_session.output.lock().unwrap().exited = true;
            });
            let result = s.describe();
            sessions.insert(s.id.clone(), s);
            return Ok(result);
        }
        let id = p["id"].as_str().context("Missing terminal")?;
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .context("Terminal not found")?;
        match op {
            "rename" => *session.name.lock().unwrap() = name(&p["name"])?,
            "close" => {
                ensure!(p["approve"] == true, "Confirm closing the terminal");
                session.close();
                self.sessions.lock().unwrap().remove(id);
            }
            _ => anyhow::bail!("Unknown terminal operation"),
        };
        Ok(json!(true))
    }
    pub fn websocket<S: Read + Write>(
        &self,
        id: &str,
        mut ws: tungstenite::WebSocket<S>,
        valid: impl Fn() -> bool,
    ) -> Result<()> {
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .context("Terminal not found")?;
        let client = hex::encode(rand::random::<[u8; 16]>());
        {
            let mut output = session.output.lock().unwrap();
            if output.owner.is_none() {
                output.owner = Some(client.clone());
            }
        }
        let mut cursor = 0u64;
        let result = (|| -> Result<()> {
            loop {
                ensure!(valid(), "Terminal login expired");
                let msg = ws.read()?;
                if msg.is_close() {
                    break;
                }
                if !msg.is_text() {
                    continue;
                }
                let p: Value = serde_json::from_str(msg.to_text()?)?;
                if p["claim"] == true {
                    session.output.lock().unwrap().owner = Some(client.clone());
                }
                let owned = session.output.lock().unwrap().owner.as_deref() == Some(&client);
                if let Some(data) = p["input"].as_str() {
                    ensure!(owned, "Terminal is read-only; take control first");
                    ensure!(data.len() <= 131072, "Terminal input too large");
                    let bytes = STANDARD.decode(data)?;
                    let mut writer = session.writer.lock().unwrap();
                    let writer = writer.as_mut().context("Terminal exited")?;
                    let deadline = std::time::Instant::now() + Duration::from_secs(5);
                    let mut offset = 0;
                    while offset < bytes.len() {
                        ensure!(!session.stop.load(Ordering::Acquire), "Terminal exited");
                        ensure!(
                            std::time::Instant::now() < deadline,
                            "Terminal input stalled; reconnect without replaying input"
                        );
                        match writer.write(&bytes[offset..]) {
                            Ok(0) => anyhow::bail!("Terminal exited"),
                            Ok(n) => offset += n,
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(15))
                            }
                            Err(e) => return Err(e.into()),
                        }
                    }
                }
                if let (Some(cols), Some(rows)) = (p["cols"].as_u64(), p["rows"].as_u64()) {
                    if owned {
                        ensure!(
                            (2..=500).contains(&cols) && (2..=300).contains(&rows),
                            "Invalid terminal dimensions"
                        );
                        if let Some(master) = session.master.lock().unwrap().as_ref() {
                            master.resize(PtySize {
                                cols: cols as u16,
                                rows: rows as u16,
                                pixel_width: 0,
                                pixel_height: 0,
                            })?;
                        }
                    }
                }
                let out = session.output.lock().unwrap();
                let start = out.end - out.bytes.len() as u64;
                let offset = cursor.max(start).min(out.end) - start;
                let data: Vec<u8> = out
                    .bytes
                    .iter()
                    .skip(offset as usize)
                    .take(131072)
                    .copied()
                    .collect();
                let reply = json!({"cursor":cursor.max(start).min(out.end)+data.len() as u64,"data":STANDARD.encode(data),"truncated":cursor<start,"writable":owned,"exited":out.exited});
                drop(out);
                cursor = reply["cursor"].as_u64().unwrap();
                ws.send(tungstenite::Message::Text(reply.to_string().into()))?;
            }
            Ok(())
        })();
        let mut out = session.output.lock().unwrap();
        if out.owner.as_deref() == Some(&client) {
            out.owner = None;
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_capacity_and_names() {
        assert!(name(&json!("\n")).is_err());
        assert!(name(&json!("x".repeat(129))).is_err());
        assert_eq!(name(&json!("测试")).unwrap(), "测试");
    }
}
#[cfg(test)]
mod pty_tests {
    use super::*;
    #[test]
    fn reconnect_replays_bounded_output_and_single_writer_can_take_control() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let terminals = Arc::new(Terminals::default());
        let home = tempfile::tempdir().unwrap();
        let v = terminals
            .api(home.path(), &json!({"operation":"create"}))
            .unwrap();
        let id = v["id"].as_str().unwrap().to_string();
        let session = terminals.sessions.lock().unwrap()[&id].clone();
        {
            let mut out = session.output.lock().unwrap();
            out.bytes = VecDeque::from(vec![b'x'; CAPACITY]);
            out.end = CAPACITY as u64 + 123;
        }
        fn connection(
            t: Arc<Terminals>,
            id: String,
        ) -> (
            tungstenite::WebSocket<std::os::unix::net::UnixStream>,
            std::thread::JoinHandle<()>,
        ) {
            let (server, client) = std::os::unix::net::UnixStream::pair().unwrap();
            client
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let handle = std::thread::spawn(move || {
                let _ = t.websocket(
                    &id,
                    tungstenite::WebSocket::from_raw_socket(
                        server,
                        tungstenite::protocol::Role::Server,
                        None,
                    ),
                    || true,
                );
            });
            (
                tungstenite::WebSocket::from_raw_socket(
                    client,
                    tungstenite::protocol::Role::Client,
                    None,
                ),
                handle,
            )
        }
        fn exchange(
            ws: &mut tungstenite::WebSocket<std::os::unix::net::UnixStream>,
            value: Value,
        ) -> Value {
            ws.send(tungstenite::Message::Text(value.to_string().into()))
                .unwrap();
            serde_json::from_str(ws.read().unwrap().to_text().unwrap()).unwrap()
        }
        let (mut first, worker1) = connection(terminals.clone(), id.clone());
        let initial = exchange(&mut first, json!({}));
        assert_eq!(initial["writable"], true);
        assert_eq!(initial["truncated"], true);
        assert!(
            STANDARD
                .decode(initial["data"].as_str().unwrap())
                .unwrap()
                .len()
                <= 131072
        );
        let (mut second, worker2) = connection(terminals.clone(), id.clone());
        assert_eq!(exchange(&mut second, json!({}))["writable"], false);
        assert_eq!(
            exchange(&mut second, json!({"claim":true}))["writable"],
            true
        );
        assert_eq!(exchange(&mut first, json!({}))["writable"], false);
        first.close(None).unwrap();
        second.close(None).unwrap();
        worker1.join().unwrap();
        worker2.join().unwrap();
        assert!(session.output.lock().unwrap().owner.is_none());
        terminals
            .api(
                home.path(),
                &json!({"operation":"close","id":id,"approve":true}),
            )
            .unwrap();
    }
    #[test]
    fn shell_output_survives_without_a_browser_and_close_releases_pty() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let terminals = Terminals::default();
        let home = tempfile::tempdir().unwrap();
        let v = terminals
            .api(home.path(), &json!({"operation":"create"}))
            .unwrap();
        let id = v["id"].as_str().unwrap();
        let session = terminals.sessions.lock().unwrap()[id].clone();
        session
            .writer
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .write_all(b"printf 'FRAMELY_%s\\n' 'PTY_TEST'\r")
            .unwrap();
        for _ in 0..100 {
            if String::from_utf8_lossy(
                &session
                    .output
                    .lock()
                    .unwrap()
                    .bytes
                    .iter()
                    .copied()
                    .collect::<Vec<_>>(),
            )
            .contains("FRAMELY_PTY_TEST")
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let out = session
            .output
            .lock()
            .unwrap()
            .bytes
            .iter()
            .copied()
            .collect::<Vec<_>>();
        assert!(String::from_utf8_lossy(&out).contains("FRAMELY_PTY_TEST"));
        assert!(terminals
            .api(home.path(), &json!({"operation":"close","id":id}))
            .is_err());
        terminals
            .api(
                home.path(),
                &json!({"operation":"close","id":id,"approve":true}),
            )
            .unwrap();
        assert!(session.master.lock().unwrap().is_none());
        assert!(session.writer.lock().unwrap().is_none());
    }
}
