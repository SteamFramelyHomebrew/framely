use super::settings::Settings;
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::{BufRead, BufReader, Write},
    os::unix::{fs::PermissionsExt, net::UnixDatagram, process::CommandExt},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
struct Worker {
    child: Child,
    input: ChildStdin,
    output: mpsc::Receiver<Value>,
}
impl Worker {
    fn new(mut command: Command, log: PathBuf) -> Result<Self> {
        super::runtime::bind_to_session(&mut command);
        let mut child = command
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(
                fs::OpenOptions::new().create(true).append(true).open(log)?,
            ))
            .spawn()?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else {
                    break;
                };
                if let Ok(v) = serde_json::from_str(&line) {
                    if tx.send(v).is_err() {
                        break;
                    }
                }
            }
        });
        Ok(Self {
            child,
            input,
            output: rx,
        })
    }
    fn stop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGTERM);
            }
            let end = Instant::now() + Duration::from_secs(2);
            while self.child.try_wait().ok().flatten().is_none() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(20));
            }
            if self.child.try_wait().ok().flatten().is_none() {
                unsafe {
                    libc::kill(-(self.child.id() as i32), libc::SIGKILL);
                }
            }
        }
        let _ = self.child.wait();
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}
pub struct Receivers {
    pub directory: PathBuf,
    binaries: PathBuf,
    configuration: Option<(bool, bool, String)>,
    socket: Option<UnixDatagram>,
    worker: Option<Worker>,
    airplay: Option<Child>,
    airplay_id: String,
    sequence: u64,
    pub sessions: BTreeMap<String, Value>,
    events: VecDeque<Value>,
    pub error: Option<String>,
}
impl Receivers {
    pub fn new(directory: PathBuf, binaries: PathBuf) -> Self {
        Self {
            directory,
            binaries,
            configuration: None,
            socket: None,
            worker: None,
            airplay: None,
            airplay_id: String::new(),
            sequence: 0,
            sessions: BTreeMap::new(),
            events: VecDeque::new(),
            error: None,
        }
    }
    pub fn configure(&mut self, s: &Settings) -> Result<()> {
        let configuration = (s.airplay, s.dlna, s.receiver_name.clone());
        if self.configuration.as_ref() == Some(&configuration) {
            return Ok(());
        }
        self.stop();
        self.configuration = Some(configuration);
        self.error = None;
        let result = (|| {
            fs::create_dir_all(&self.directory)?;
            fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))?;
            let path = self.directory.join("events.sock");
            let _ = fs::remove_file(&path);
            let socket = UnixDatagram::bind(path)?;
            socket.set_nonblocking(true)?;
            self.socket = Some(socket);
            self.write_description(&s.receiver_name)?;
            let executable = self.binaries.join("framely-receiver");
            ensure!(executable.is_file(), "缺少内置投屏接收程序");
            let mut cmd = Command::new(executable);
            cmd.arg(&self.directory)
                .arg(self.directory.join("upnp"))
                .arg(if s.dlna { "1" } else { "0" })
                .env("GST_PLUGIN_PATH", &self.binaries);
            self.worker = Some(Worker::new(cmd, self.directory.join("receiver.log"))?);
            if s.airplay {
                self.start_airplay(&s.receiver_name)?;
            }
            Ok(())
        })();
        if let Err(e) = &result {
            self.error = Some(format!("{e:#}"));
        }
        result
    }
    fn write_description(&self, name: &str) -> Result<()> {
        let xml = self.directory.join("upnp");
        fs::create_dir_all(&xml)?;
        let resources = self.binaries.join("upnp");
        for file in [
            "AVTransport.xml",
            "RenderingControl.xml",
            "ConnectionManager.xml",
        ] {
            fs::copy(resources.join(file), xml.join(file))?;
        }
        let name = name
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let services=["AVTransport","RenderingControl","ConnectionManager"].map(|s|format!("<service><serviceType>urn:schemas-upnp-org:service:{s}:1</serviceType><serviceId>urn:upnp-org:serviceId:{s}</serviceId><SCPDURL>/{s}.xml</SCPDURL><controlURL>/{s}/control</controlURL><eventSubURL>/{s}/event</eventSubURL></service>")).join("");
        // The advertised renderer has a persistent identity for this Steam user.
        use sha2::{Digest, Sha256};
        let mut identity = fs::read("/etc/machine-id").context("无法读取设备身份")?;
        identity.extend_from_slice(crate::steam::home()?.as_os_str().as_encoded_bytes());
        let hash = hex::encode(Sha256::digest(identity));
        let uuid = format!(
            "{}-{}-{}-{}-{}",
            &hash[..8],
            &hash[8..12],
            &hash[12..16],
            &hash[16..20],
            &hash[20..32]
        );
        fs::write(xml.join("device.xml"),format!("<?xml version=\"1.0\"?><root xmlns=\"urn:schemas-upnp-org:device-1-0\"><specVersion><major>1</major><minor>0</minor></specVersion><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><friendlyName>{name}</friendlyName><manufacturer>Framely</manufacturer><modelName>Framely</modelName><UDN>uuid:{uuid}</UDN><serviceList>{services}</serviceList></device></root>"))?;
        Ok(())
    }
    fn start_airplay(&mut self, name: &str) -> Result<()> {
        let executable = self.binaries.join("uxplay");
        ensure!(executable.is_file(), "缺少内置 AirPlay 接收程序");
        self.airplay_id = crate::session::cast_random_key()[..32].into();
        let log = fs::OpenOptions::new().create(true).truncate(true).write(true)
            .open(self.directory.join("airplay.log"))?;
        let mut cmd = Command::new(executable);
        cmd.process_group(0)
            .args([
                "-n",
                name,
                "-avdec",
                "-hls",
                "-vs",
                "framelyvideosink",
                "-as",
                "framelyaudiosink",
            ])
            .env("GST_PLUGIN_PATH", &self.binaries)
            .env("FRAMELY_CAST_DIR", &self.directory)
            .env("FRAMELY_CAST_ID", &self.airplay_id)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        super::runtime::bind_to_session(&mut cmd);
        self.airplay = Some(cmd.spawn()?);
        Ok(())
    }
    fn event(&mut self, mut e: Value) {
        let Some(id) = e["id"].as_str().map(str::to_owned) else {
            if e["event"] == "fatal" {
                self.error = e["error"].as_str().map(str::to_owned);
            }
            return;
        };
        if !id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') || id.len() > 64 {
            return;
        }
        if e["protocol"] == "AirPlay" && id != self.airplay_id {
            return;
        }
        let kind = e["event"].as_str().unwrap_or("");
        if matches!(kind, "request" | "video" | "audio") {
            let fresh = !self.sessions.contains_key(&id);
            if fresh {
                e["accepted"] = json!(false);
                e["createdAt"] = json!(crate::service::now_ms());
                self.sessions.insert(id.clone(), e.clone());
                self.events
                    .push_back(json!({"kind":"cast.request","session":e}));
            } else {
                let session = self.sessions.get_mut(&id).unwrap();
                let mut changed = false;
                if e["width"].as_u64().unwrap_or(0) > 0 && e["height"].as_u64().unwrap_or(0) > 0 {
                    changed = session["width"] != e["width"] || session["height"] != e["height"];
                    session["width"] = e["width"].clone();
                    session["height"] = e["height"].clone();
                    session["mediaType"] = json!("video");
                } else if e["mediaType"] == "video" && session["mediaType"] != "video" {
                    session["mediaType"] = json!("video");
                    changed = true;
                }
                if session["accepted"] == true {
                    if changed || matches!(kind, "video" | "audio") {
                        self.events.push_back(json!({"kind":"cast.window","session":session.clone()}));
                    }
                } else if kind == "request" && e["protocol"] == "AirPlay" {
                    session["createdAt"] = json!(crate::service::now_ms());
                    self.events.push_back(json!({"kind":"cast.request","session":session.clone()}));
                }
            }
        } else if matches!(kind, "ended" | "error") {
            if let Some(mut s) = self.sessions.remove(&id) {
                s["error"] = e["error"].clone();
                self.events
                    .push_back(json!({"kind":"cast.ended","session":s}));
            }
            self.clean_session(&id);
        }
    }
    pub fn poll(&mut self) -> Vec<Value> {
        let mut values = Vec::new();
        if let Some(w) = &self.worker {
            values.extend(w.output.try_iter());
        }
        if let Some(socket) = &self.socket {
            loop {
                let mut bytes = [0; 16384];
                let Ok(n) = socket.recv(&mut bytes) else {
                    break;
                };
                if let Ok(v) = serde_json::from_slice(&bytes[..n]) {
                    values.push(v);
                }
            }
        }
        let mut disconnected = false;
        for e in values {
            if e["protocol"] == "AirPlay"
                && e["event"] == "ended"
                && e["id"] == self.airplay_id
                && self.sessions.contains_key(&self.airplay_id)
            {
                disconnected = true;
            }
            self.event(e);
        }
        if disconnected {
            if let Some(mut p) = self.airplay.take() {
                stop_child(&mut p);
            }
            if let Some(config) = &self.configuration {
                let name = config.2.clone();
                if config.0 {
                    if let Err(e) = self.start_airplay(&name) {
                        self.error = Some(e.to_string());
                    }
                }
            }
        }

        let mut retained = VecDeque::new();
        while let Some(e) = self.events.pop_front() {
            if e["kind"] == "cast.engine.event" {
                self.event(e["data"].clone());
            } else {
                retained.push_back(e);
            }
        }
        self.events = retained;
        if let Some(code) = self
            .worker
            .as_mut()
            .and_then(|w| w.child.try_wait().ok().flatten())
        {
            self.error = Some(format!("DLNA 接收程序已退出：{code}"));
            self.worker = None;
            self.end_protocol("DLNA");
        }
        if let Some(code) = self
            .airplay
            .as_mut()
            .and_then(|w| w.try_wait().ok().flatten())
        {
            let details = fs::read_to_string(self.directory.join("airplay.log")).unwrap_or_default();
            let tail = details.lines().rev().take(8).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
            self.error = Some(format!("AirPlay 接收程序已退出：{code}\n{tail}"));
            self.airplay = None;
            self.end_protocol("AirPlay");
        }
        // Pending requests expire; a delayed notification must never open an old cast.
        let expired: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, s)| {
                s["accepted"] != true
                    && crate::service::now_ms().saturating_sub(s["createdAt"].as_u64().unwrap_or(0))
                        > 60000
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            let _ = self.control(&id, "reject", json!({}));
        }
        self.events.drain(..).collect()
    }
    fn request(&mut self, mut value: Value) -> Result<Value> {
        self.sequence += 1;
        value["request"] = json!(self.sequence);
        let sequence = self.sequence;
        let w = self.worker.as_mut().context("投屏服务尚未启动")?;
        writeln!(w.input, "{value}")?;
        w.input.flush()?;
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            let reply = w
                .output
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .context("投屏服务未响应")?;
            if reply["request"].as_u64() == Some(sequence) {
                if let Some(e) = reply["error"].as_str() {
                    bail!("{e}");
                }
                return Ok(reply["result"].clone());
            }
            self.events
                .push_back(json!({"kind":"cast.engine.event","data":reply}));
        }
    }
    pub fn devices(&mut self) -> Result<Value> {
        self.request(json!({"kind":"devices"}))
    }
    pub fn send(&mut self, id: &str, uri: &str) -> Result<Value> {
        self.request(json!({"kind":"send","id":id,"uri":uri}))
    }
    pub fn stop_send(&mut self, id: &str) -> Result<Value> {
        self.request(json!({"kind":"send.stop","id":id}))
    }
    pub fn control(&mut self, id: &str, kind: &str, params: Value) -> Result<Value> {
        let session = self.sessions.get(id).context("投屏请求已过期")?.clone();
        ensure!(
            matches!(
                kind,
                "accept" | "reject" | "stop" | "pause" | "volume" | "seek" | "status"
            ),
            "Invalid media action"
        );
        ensure!(
            matches!(kind, "accept" | "reject" | "stop") || session["accepted"] == true,
            "尚未接受投屏"
        );
        ensure!(
            !matches!(kind, "accept" | "reject") || session["accepted"] != true,
            "投屏已接受"
        );
        if session["protocol"] == "AirPlay" {
            ensure!(id == self.airplay_id, "投屏请求已过期");
            let path = self.directory.join(format!("{id}.accept"));
            match kind {
                "accept" => {
                    fs::write(&path, [])?;
                }
                "pause" => {
                    if params["paused"].as_bool().unwrap_or(true) {
                        let _ = fs::remove_file(path);
                    } else {
                        fs::write(path, [])?;
                    }
                }
                "reject" | "stop" => {
                    let _ = fs::remove_file(path);
                    if let Some(mut child) = self.airplay.take() {
                        stop_child(&mut child);
                    }
                    self.clean_session(id);
                    self.sessions.remove(id);
                    self.events
                        .push_back(json!({"kind":"cast.ended","session":session}));
                    let name = self.configuration.as_ref().context("服务已关闭")?.2.clone();
                    self.start_airplay(&name)?;
                    return Ok(json!(true));
                }
                "volume" => {
                    let volume = params["volume"].as_f64().context("Invalid volume")?;
                    ensure!(
                        volume.is_finite() && (0.0..=1.0).contains(&volume),
                        "Invalid volume"
                    );
                    fs::write(
                        self.directory.join(format!("{id}.volume")),
                        volume.to_string(),
                    )?;
                }
                "status" => return Ok(json!({"paused":!path.exists(),"position":0,"duration":0})),
                _ => bail!("AirPlay 不支持跳转"),
            };
        } else {
            let mut p = params;
            p["kind"] = json!(kind);
            p["id"] = json!(id);
            let result = self.request(p)?;
            if kind == "status" {
                return Ok(result);
            }
        }
        if kind == "accept" {
            self.sessions.get_mut(id).unwrap()["accepted"] = json!(true);
            if session["protocol"] == "AirPlay" {
                self.events
                    .push_back(json!({"kind":"cast.window","session":self.sessions[id]}));
            }
        }
        if matches!(kind, "stop" | "reject") {
            self.clean_session(id);
            self.sessions.remove(id);
            self.events
                .push_back(json!({"kind":"cast.ended","session":session}));
        }
        Ok(json!(true))
    }
    pub fn window(&self, id: &str) -> Result<Value> {
        let s = self.sessions.get(id).context("投屏已经结束")?;
        ensure!(s["accepted"] == true, "尚未接受投屏");
        Ok(s.clone())
    }
    fn end_protocol(&mut self, protocol: &str) {
        let ids: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, s)| s["protocol"] == protocol)
            .map(|(id, _)| id.clone())
            .collect();
        for id in ids {
            self.event(json!({"event":"ended","id":id}));
        }
    }
    fn clean_session(&self, id: &str) {
        for suffix in ["accept", "frame", "volume"] {
            let _ = fs::remove_file(self.directory.join(format!("{id}.{suffix}")));
        }
    }
    fn stop(&mut self) {
        self.worker = None;
        if let Some(mut p) = self.airplay.take() {
            stop_child(&mut p);
        }
        for (id, s) in &self.sessions {
            self.clean_session(id);
            self.events
                .push_back(json!({"kind":"cast.ended","session":s}));
        }
        self.sessions.clear();
        self.socket = None;
    }
}
impl Drop for Receivers {
    fn drop(&mut self) {
        self.stop();
    }
}

fn stop_child(child: &mut Child) {
    if child.try_wait().ok().flatten().is_some() {
        return;
    }
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGTERM);
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    while child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if child.try_wait().ok().flatten().is_none() {
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn airplay_setup_requests_consent_before_frames_and_after_reconnect() {
        let root=tempfile::tempdir().unwrap();
        let mut r=Receivers::new(root.path().to_owned(),root.path().join("bin"));
        r.airplay_id="0123".into();
        for _ in 0..2 {
            r.event(json!({"event":"request","id":"0123","protocol":"AirPlay"}));
            assert_eq!(r.events.pop_front().unwrap()["kind"],"cast.request");
            assert!(r.window("0123").is_err());
            r.event(json!({"event":"request","id":"0123","protocol":"AirPlay"}));
            assert_eq!(r.events.pop_front().unwrap()["kind"],"cast.request");
            r.control("0123","accept",json!({})).unwrap();
            assert_eq!(r.window("0123").unwrap()["accepted"],true);
            r.event(json!({"event":"ended","id":"0123","protocol":"AirPlay"}));
            assert!(!root.path().join("0123.accept").exists());
            r.events.clear();
        }
    }
    #[test]
    fn airplay_video_setup_promotes_audio_session_before_decoding() {
        let root = tempfile::tempdir().unwrap();
        let mut r = Receivers::new(root.path().to_owned(), root.path().join("bin"));
        r.airplay_id = "0123".into();
        r.event(json!({"event":"request","id":"0123","protocol":"AirPlay","mediaType":"audio"}));
        r.control("0123", "accept", json!({})).unwrap();
        r.events.clear();
        r.event(json!({"event":"request","id":"0123","protocol":"AirPlay","mediaType":"video"}));
        assert_eq!(r.window("0123").unwrap()["mediaType"], "video");
        assert_eq!(r.events.pop_front().unwrap()["kind"], "cast.window");
        r.event(json!({"event":"request","id":"0123","protocol":"AirPlay","width":1080,"height":1920}));
        let update = r.events.pop_front().unwrap();
        assert_eq!(update["session"]["width"], 1080);
        assert_eq!(update["session"]["height"], 1920);
        r.event(json!({"event":"audio","id":"0123","protocol":"AirPlay","mediaType":"audio"}));
        assert_eq!(r.window("0123").unwrap()["mediaType"], "video");
    }
    #[test]
    fn consent_blocks_playback_controls_and_rejects_stale_airplay_sessions() {
        let root = tempfile::tempdir().unwrap();
        let mut r = Receivers::new(root.path().to_owned(), root.path().join("bin"));
        r.airplay_id = "0123".into();
        r.event(json!({"event":"video","id":"0123","protocol":"AirPlay","width":320,"height":240}));
        assert!(r.window("0123").is_err());
        assert!(r.control("0123", "pause", json!({})).is_err());
        r.control("0123", "accept", json!({})).unwrap();
        assert_eq!(r.window("0123").unwrap()["width"], 320);
        assert!(r.control("0123", "accept", json!({})).is_err());
        r.airplay_id = "4567".into();
        assert!(r.control("0123", "volume", json!({"volume":0.5})).is_err());
    }
    #[test]
    fn session_updates_preserve_consent_and_disconnect_cleans_only_its_protocol() {
        let root = tempfile::tempdir().unwrap();
        let mut r = Receivers::new(root.path().to_owned(), root.path().join("bin"));
        r.airplay_id = "4567".into();
        for (id, protocol) in [("0123", "DLNA"), ("4567", "AirPlay")] {
            r.event(json!({"event":"request","id":id,"protocol":protocol}));
            r.sessions.get_mut(id).unwrap()["accepted"] = json!(true);
            for suffix in ["frame", "accept", "volume"] {
                fs::write(root.path().join(format!("{id}.{suffix}")), []).unwrap();
            }
        }
        r.event(json!({"event":"video","id":"0123","protocol":"DLNA","width":240,"height":320}));
        assert_eq!(r.window("0123").unwrap()["height"], 320);
        assert_eq!(r.sessions.len(), 2);
        r.end_protocol("DLNA");
        assert!(r.window("0123").is_err());
        assert!(r.window("4567").is_ok());
        for suffix in ["frame", "accept", "volume"] {
            assert!(!root.path().join(format!("0123.{suffix}")).exists());
        }
        assert!(r
            .events
            .iter()
            .any(|e| e["kind"] == "cast.ended" && e["session"]["id"] == "0123"));
    }
}
