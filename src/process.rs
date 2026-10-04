use crate::model::*;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::fd::AsRawFd,
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
pub type Events = Arc<Mutex<std::collections::VecDeque<Value>>>;
pub fn event(events: &Events, v: Value) {
    let mut q = events.lock().unwrap();
    if q.len() >= 256 {
        q.pop_front();
    }
    q.push_back(v);
}
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitInfo {
    pub reason: String,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub oom: bool,
}
#[cfg(test)]
thread_local! { pub static TEST_TOOLS: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) }; }
fn tool(name: &str) -> Command {
    #[cfg(test)]
    if let Some(root) = TEST_TOOLS.with(|p| p.borrow().clone()) {
        return Command::new(root.join(name));
    }
    Command::new(name)
}
pub fn open_web_link(url: &str) -> Result<()> {
    validate_web_link(url)?;
    let result = tool("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .context("无法启动系统浏览器")?;
    ensure!(
        result.status.success(),
        "无法打开链接：{}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(())
}

fn command(m: &Manifest, payload: &Path, manager: u32, logs: &Path, unit: &str) -> Result<Command> {
    let b = m.backend.as_ref().context("Plugin has no backend")?;
    let home = logs
        .parent()
        .context("Missing state directory")?
        .join("data")
        .join(&m.id)
        .join(match b.run_as {
            RunAs::Steamos => "steamos",
            RunAs::Root => "root",
        });
    fs::create_dir_all(home.parent().unwrap())?;
    ensure!(
        !fs::symlink_metadata(&home).is_ok_and(|m| m.file_type().is_symlink()),
        "Plugin data path is a symlink"
    );
    let fresh = !home.exists();
    fs::create_dir_all(&home)?;
    let user = match b.run_as {
        RunAs::Root => "root".to_owned(),
        RunAs::Steamos => manager.to_string(),
    };
    let mut cmd = tool("systemd-run");
    cmd.args(["--quiet", "--wait", "--pipe", "--service-type=exec"]);
    cmd.arg(format!("--unit={unit}"));
    for p in [
        format!("User={user}"),
        format!("WorkingDirectory={}", payload.display()),
        "KillMode=control-group".into(),
        "TimeoutStopSec=5".into(),
        "TasksMax=128".into(),
        format!("MemoryMax={}M", b.memory_limit_mib),
        "NoNewPrivileges=yes".into(),
    ] {
        cmd.arg(format!("--property={p}"));
    }
    if fresh {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&home, fs::Permissions::from_mode(0o700))?;
        let status = tool("chown").arg(&user).arg(&home).status()?;
        ensure!(status.success(), "Cannot set data ownership");
    }
    cmd.arg(format!("--setenv=FRAMELY_DATA_DIR={}", home.display()))
        .arg(format!("--setenv=FRAMELY_PLUGIN_ID={}", m.id));
    match b.run_as {
        RunAs::Steamos => {
            cmd.arg(format!("--setenv=HOME={}", user_home(manager)?));
        }
        RunAs::Root => {
            cmd.arg("--setenv=HOME=/root");
        }
    }
    cmd.arg(format!("--setenv=FRAMELY_PLUGIN_VERSION={}", m.version));
    cmd.arg("--")
        .arg(payload.join(&b.entry))
        .args(&b.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    Ok(cmd)
}
fn log_stream(mut stream: impl Read + Send + 'static, log: std::path::PathBuf) {
    std::thread::spawn(move || {
        let mut buffer = [0; 4096];
        loop {
            let n = match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            if fs::metadata(&log).is_ok_and(|m| m.len() > 2 * 1024 * 1024) {
                let _ = fs::rename(&log, log.with_extension("log.1"));
            }
            if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&log) {
                let _ = file.write_all(&buffer[..n]);
            }
        }
    });
}
pub fn hook(
    m: &Manifest,
    h: &Hook,
    payload: &Path,
    manager: u32,
    logs: &Path,
    context: Value,
    timeout: Duration,
) -> Result<()> {
    let mut launch = m.clone();
    launch.backend = Some(Backend {
        entry: h.entry.clone(),
        args: h.args.clone(),
        run_as: m.run_as(),
        autostart: false,
        restart: RestartPolicy::Never,
        restart_limit: 3,
        memory_limit_mib: m.memory_limit_mib(),
    });
    let unit = format!("framely-hook-{}-{:032x}", m.id, rand::random::<u128>());
    let cmd = command(&launch, payload, manager, logs, &unit)?;
    // Insert env/property arguments before the executable delimiter.
    let args: Vec<_> = cmd.get_args().map(|a| a.to_owned()).collect();
    let split = args
        .iter()
        .position(|a| a == "--")
        .context("Missing service delimiter")?;
    let program = cmd.get_program().to_owned();
    let mut cmd = Command::new(program);
    cmd.args(&args[..split])
        .arg(format!("--property=RuntimeMaxSec={}", timeout.as_secs()))
        .arg(format!(
            "--setenv=FRAMELY_LIFECYCLE={}",
            context["phase"].as_str().unwrap_or("")
        ))
        .arg(format!("--setenv=FRAMELY_LIFECYCLE_CONTEXT={context}"))
        .args(&args[split..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    fs::create_dir_all(logs)?;
    let mut child = cmd.spawn().context("Cannot execute lifecycle command")?;
    let log = logs.join(format!("{}.lifecycle.log", m.id));
    log_stream(child.stdout.take().unwrap(), log.clone());
    log_stream(child.stderr.take().unwrap(), log);
    let end = Instant::now() + timeout;
    let result = loop {
        if let Some(status) = child.try_wait()? {
            break if status.success() {
                Ok(())
            } else {
                Err(anyhow::anyhow!(
                    "Lifecycle {} exited: {status}",
                    context["phase"]
                ))
            };
        }
        if Instant::now() >= end {
            let _ = tool("systemctl").args(["stop", &unit]).status();
            let _ = child.kill();
            let _ = child.wait();
            break Err(anyhow::anyhow!("Lifecycle {} timed out", context["phase"]));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let _ = tool("systemctl").args(["stop", &unit]).status();
    let _ = tool("systemctl").args(["reset-failed", &unit]).status();
    result
}
pub struct Running {
    child: Child,
    input: ChildStdin,
    pending: Arc<Mutex<HashMap<u64, mpsc::Sender<Value>>>>,
    seq: u64,
    unit: String,
    failure: Option<ExitInfo>,
}
impl Running {
    pub fn start(
        m: &Manifest,
        payload: &Path,
        manager: u32,
        events: Events,
        logs: &Path,
    ) -> Result<Self> {
        let unit = format!("framely-backend-{}", m.id);
        let mut cmd = command(m, payload, manager, logs, &unit)?;
        fs::create_dir_all(logs)?;
        cmd.stderr(Stdio::piped());
        let mut child = cmd.spawn().context("Cannot start plugin service")?;
        let input = child.stdin.take().unwrap();
        let fd = input.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        ensure!(
            flags >= 0 && unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } >= 0,
            "Cannot configure backend input"
        );
        let mut stderr = child.stderr.take().unwrap();
        let log = logs.join(format!("{}.log", m.id));
        std::thread::spawn(move || {
            let mut buffer = [0; 4096];
            loop {
                let n = match stderr.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                if fs::metadata(&log).is_ok_and(|m| m.len() > 2 * 1024 * 1024) {
                    let _ = fs::rename(&log, log.with_extension("log.1"));
                }
                if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&log) {
                    let _ = file.write_all(&buffer[..n]);
                }
            }
        });
        let output = child.stdout.take().unwrap();
        let pending: Arc<Mutex<HashMap<u64, mpsc::Sender<Value>>>> = Arc::default();
        let responses = pending.clone();
        let id = m.id.clone();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut line = Vec::new();
                match reader.by_ref().take(65537).read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
                if line.len() > 65536 || line.last() != Some(&b'\n') {
                    event(
                        &events,
                        json!({"kind":"plugin.error","plugin":id,"message":"Backend emitted an oversized or incomplete message"}),
                    );
                    break;
                }
                match serde_json::from_slice::<Value>(&line) {
                    Ok(v) => {
                        if let Some(n) = v["id"].as_u64() {
                            if let Some(tx) = responses.lock().unwrap().remove(&n) {
                                let _ = tx.send(v);
                            }
                        } else if let Some(e) = v["event"].as_str() {
                            event(
                                &events,
                                json!({"kind":"plugin.event","plugin":id,"event":e,"data":v["data"]}),
                            );
                        }
                    }
                    Err(_) => event(
                        &events,
                        json!({"kind":"plugin.error","plugin":id,"message":"Backend stdout must contain JSON protocol messages"}),
                    ),
                }
            }
            responses.lock().unwrap().clear();
            event(&events, json!({"kind":"plugin.exited","plugin":id}));
        });
        Ok(Self {
            child,
            input,
            pending,
            seq: 0,
            unit,
            failure: None,
        })
    }
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.call_timeout(method, params, Duration::from_secs(15))
    }
    pub fn call_timeout(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        ensure!(self.child.try_wait()?.is_none(), "Plugin backend exited");
        ensure!(method.len() <= 120, "Method too long");
        self.seq += 1;
        let id = self.seq;
        let (tx, rx) = mpsc::channel();
        let request = json!({"id":id,"method":method,"params":params});
        let mut bytes = serde_json::to_vec(&request)?;
        ensure!(bytes.len() <= 65536, "Backend request too large");
        bytes.push(b'\n');
        self.pending.lock().unwrap().insert(id, tx);
        let write = write_with_timeout(&mut self.input, &bytes);
        if let Err(e) = write {
            self.pending.lock().unwrap().remove(&id);
            self.failure = Some(ExitInfo {
                reason: "transport-failure".into(),
                exit_code: None,
                signal: None,
                oom: false,
            });
            self.stop();
            return Err(e).context("Backend input failed; backend stopped");
        }
        let result = rx.recv_timeout(timeout);
        self.pending.lock().unwrap().remove(&id);
        let reply = match result {
            Ok(reply) => reply,
            Err(e) => {
                self.failure = Some(ExitInfo {
                    reason: if matches!(e, mpsc::RecvTimeoutError::Timeout) {
                        "call-timeout"
                    } else {
                        "unexpected-exit"
                    }
                    .into(),
                    exit_code: None,
                    signal: None,
                    oom: false,
                });
                let _ = self.exit_info();
                self.stop();
                return Err(e).context("Plugin call timed out or backend exited; backend stopped");
            }
        };
        if let Some(e) = reply.get("error") {
            anyhow::bail!("Plugin error: {}", e);
        }
        Ok(reply["result"].clone())
    }
    pub fn exited(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_some()
    }
    pub fn exit_info(&mut self) -> ExitInfo {
        use std::os::unix::process::ExitStatusExt;
        let status = self.child.try_wait().ok().flatten();
        let mut info = ExitInfo {
            reason: if status.is_some_and(|s| s.success()) {
                "exited"
            } else {
                "crash"
            }
            .into(),
            exit_code: status.and_then(|s| s.code()),
            signal: status.and_then(|s| s.signal()),
            oom: false,
        };
        if let Ok(output) = tool("systemctl")
            .args([
                "show",
                &self.unit,
                "--property=Result,ExecMainCode,ExecMainStatus",
            ])
            .output()
        {
            let values: HashMap<_, _> = String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|l| l.split_once('=').map(|(a, b)| (a.to_owned(), b.to_owned())))
                .collect();
            if let Some(code) = values.get("ExecMainStatus").and_then(|v| v.parse().ok()) {
                match values.get("ExecMainCode").map(String::as_str) {
                    Some("1") => {
                        info.exit_code = Some(code);
                        info.signal = None;
                    }
                    Some("2" | "3") => {
                        info.signal = Some(code);
                        info.exit_code = None;
                    }
                    _ => {}
                }
            }
            info.oom = values.get("Result").is_some_and(|r| r == "oom-kill");
            if info.oom {
                info.reason = "oom".into();
            }
        }
        if let Some(previous) = &self.failure {
            info.reason = previous.reason.clone();
            info.oom |= previous.oom;
            if previous.exit_code.is_some() || previous.signal.is_some() {
                info.exit_code = previous.exit_code;
                info.signal = previous.signal;
            }
        }
        if info.oom {
            info.reason = "oom".into();
        }
        self.failure = Some(info.clone());
        info
    }
    pub fn stop(&mut self) {
        let _ = tool("systemctl").args(["stop", &self.unit]).status();
        let end = Instant::now() + Duration::from_secs(6);
        while Instant::now() < end {
            if self.child.try_wait().ok().flatten().is_some() {
                let _ = tool("systemctl")
                    .args(["reset-failed", &self.unit])
                    .status();
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = tool("systemctl")
            .args(["reset-failed", &self.unit])
            .status();
    }
}
fn write_with_timeout(input: &mut ChildStdin, bytes: &[u8]) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut offset = 0;
    while offset < bytes.len() {
        match input.write(&bytes[offset..]) {
            Ok(0) => anyhow::bail!("Backend input closed"),
            Ok(n) => offset += n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                ensure!(Instant::now() < deadline, "Backend input timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
impl Drop for Running {
    fn drop(&mut self) {
        self.stop();
    }
}
pub fn user_home(uid: u32) -> Result<String> {
    let data = fs::read_to_string("/etc/passwd")?;
    for line in data.lines() {
        let f: Vec<_> = line.split(':').collect();
        if f.len() >= 7 && f[2] == uid.to_string() {
            return Ok(f[5].into());
        }
    }
    anyhow::bail!("Steam user not found")
}
