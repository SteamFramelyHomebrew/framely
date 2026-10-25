use super::settings::{Codec, Eye, Output, Settings, Source};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub struct Casting {
    pub directory: PathBuf,
    pub binaries: PathBuf,
    workers: Vec<Child>,
    started: Option<Instant>,
    streaming: bool,
    pub whep_port: u16,
    rtsp_port: u16,
    api_port: u16,
    dlna_started: bool,
    vp8: Option<(Child, Instant)>,
    compatibility_poll: Instant,
    active_settings: Option<Settings>,
    pub hls_port: u16,
    pub stream_key: String,
    error: Option<String>,
    preview_restore: Option<Option<Settings>>,
    recording: Option<super::recording::Recording>,
    recording_error: Option<String>,
    recording_path: Option<PathBuf>,
    pub region: super::region::Region,
}
fn port() -> Result<u16> {
    Ok(std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port())
}
impl Casting {
    pub fn new(directory: PathBuf, binaries: PathBuf) -> Self {
        Self {
            directory,
            binaries,
            workers: Vec::new(),
            started: None,
            streaming: false,
            whep_port: 0,
            rtsp_port: 0,
            api_port: 0,
            dlna_started: false,
            vp8: None,
            compatibility_poll: Instant::now(),
            active_settings: None,
            hls_port: 0,
            stream_key: String::new(),
            error: None,
            preview_restore: None,
            recording: None,
            recording_error: None,
            recording_path: None,
            region: Default::default(),
        }
    }
    pub fn status(&mut self) -> Value {
        self.poll_recording();
        self.poll_workers();
        let view = fs::File::open(self.directory.join("stream.log")).ok().and_then(|file| { let mut log = String::new(); file.take(65536).read_to_string(&mut log).ok()?; Some(log) }).and_then(|log| log.lines().filter_map(|line|serde_json::from_str::<Value>(line).ok()).find(|v|v["event"]=="view"));
        json!({"running":self.streaming && self.started.is_some(),"error":self.error,"preview":self.preview_restore.is_some(),"view":view,
            "watchPath":if self.streaming && self.started.is_some(){Some("/cast/watch")}else{None},
            "elapsedSeconds":if self.streaming {self.started.map(|t|t.elapsed().as_secs())} else {None},
            "codec":self.active_settings.as_ref().map(|s|s.codec.name()),"dominantEye":dominant_eye().ok(),"recording":self.recording_status()})
    }
    pub fn refresh_region(&mut self, configured: &Settings) -> Result<()> {
        let mut effective=self.active_settings.clone().unwrap_or_else(||configured.clone());
        effective.recording_region_visible=configured.recording_region_visible;
        let active=if self.started.is_some() && super::region::supported(&effective) && (effective.recording_region_visible || self.region.editing) {
            Some(self.status()["view"].clone())
        } else {None};
        let result=self.region.refresh(&effective,&self.binaries,active);
        if let Err(error)=&result {self.region.fail(error);}
        result
    }
    pub fn region_status(&self) -> Value {
        self.region.snapshot((self.streaming && self.started.is_some()) || self.recording.is_some())
    }
    fn poll_workers(&mut self) {
        let exited = self
            .workers
            .iter_mut()
            .find_map(|p| p.try_wait().ok().flatten());
        if let Some(code) = exited {
            self.error = Some(format!("串流进程已退出：{code}"));
            self.stop_workers();
            super::retain_failed_log(&self.directory, "stream");
        }
    }
    pub fn recording_status(&self) -> Value {
        let mut value = self.recording.as_ref().map(|r|r.status()).unwrap_or(json!({"active":false,"path":self.recording_path}));
        value["error"] = json!(self.recording_error);
        value
    }
    fn poll_recording(&mut self) {
        if self.recording.as_mut().is_some_and(|r|r.exited().unwrap_or(true)) {
            self.recording_error = Some(format!("录制进程意外退出：{}",self.recording.as_ref().unwrap().detail()));
            let _ = self.stop_recording();
        }
    }
    pub fn validate_recording_settings(&self, settings: &Settings) -> Result<()> {
        ensure!(self.recording.is_none() || self.active_settings.as_ref().is_some_and(|s|s.same_capture(settings)), "请先停止录制再调整取景");
        Ok(())
    }
    pub fn record(&mut self, settings: &Settings, panel_socket: Option<&Path>, output: &Path) -> Result<Value> {
        ensure!(!self.region.editing, "请先关闭画幅调整窗口");
        self.status();
        ensure!(self.preview_restore.is_none(), "取景预览期间不能录制");
        ensure!(self.recording.is_none(), "录制已在运行");
        let capture_started = self.started.is_none();
        if capture_started {
            self.start_capture(settings, panel_socket, false)?;
        }
        let result = self.start_recording(output);
        if result.is_err() && capture_started { self.stop_workers(); }
        result
    }
    fn start_recording(&mut self, output: &Path) -> Result<Value> {
        self.status();
        ensure!(self.started.is_some(), "请先开始串流");
        ensure!(self.preview_restore.is_none(), "取景预览期间不能录制");
        ensure!(self.recording.is_none(), "录制已在运行");
        self.recording_error = None;
        match super::recording::Recording::start(self.rtsp_port, &self.directory, output, self.codec()) {
            Ok(recording) => {self.recording = Some(recording); Ok(self.recording_status())},
            Err(error) => {self.recording_error = Some(format!("{error:#}")); Err(error)}
        }
    }
    pub fn stop_recording(&mut self) -> Result<Value> {
        let result = self.finish_recording();
        if !self.streaming {self.stop_workers();}
        result
    }
    fn finish_recording(&mut self) -> Result<Value> {
        if let Some(mut recording) = self.recording.take() {
            match recording.finish() {
                Ok(path) => self.recording_path = Some(path),
                Err(error) => {self.recording_error = Some(format!("{error:#}")); return Err(error);}
            }
        }
        Ok(self.recording_status())
    }
    pub fn stop_recording_id(&mut self, id: &str) {
        if self.recording.as_ref().is_some_and(|r|r.id == id) {let _ = self.stop_recording();}
    }
    fn stop_workers(&mut self) {
        let _ = self.finish_recording();
        for child in &mut self.workers {
            if child.try_wait().ok().flatten().is_some() {
                continue;
            }
            // Each worker owns a process group, including its media children.
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGTERM);
            }
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        for child in &mut self.workers {
            while child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(20));
            }
            if child.try_wait().ok().flatten().is_none() {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
            }
            let _ = child.wait();
        }
        self.workers.clear();
        self.dlna_started = false;
        self.stop_vp8();
        self.active_settings = None;
        self.region.invalidate();
        self.started = None;
        self.streaming = false;
        self.stream_key.clear();
    }
    /// Negotiate with browsers that omit H.264 (including the bundled CEF).
    /// All VP8 viewers share one on-demand compatibility encoder.
    #[cfg(test)]
    pub(crate) fn test_rtsp_port(&self) -> u16 {self.rtsp_port}
    #[cfg(test)]
    pub(crate) fn test_whep_server(&mut self, port: u16) {
        self.started = Some(Instant::now());
        self.streaming = true;
        self.active_settings = Some(Settings::default());
        self.whep_port = port;
    }
    pub fn codec(&self) -> Codec { self.active_settings.as_ref().map(|s|s.codec).unwrap_or_default() }
    pub fn ensure_vp8(&mut self) -> Result<()> {
        ensure!(self.status()["running"] == true, "请先开始头显串流");
        if self.vp8.is_some() {
            return Ok(());
        }
        let s = self
            .active_settings
            .clone()
            .context("Missing capture settings")?;
        let source = format!("rtsp://127.0.0.1:{}/headset", self.rtsp_port);
        let target = format!("rtsp://127.0.0.1:{}/headset_vp8", self.rtsp_port);
        self.spawn(Command::new("ffmpeg").args([
            "-hide_banner",
            "-loglevel",
            "warning",
            "-nostdin",
            "-rtsp_transport",
            "tcp",
            "-i",
            &source,
            "-map",
            "0:v:0",
            "-map",
            "0:a?",
            "-c:v",
            "libvpx",
            "-deadline",
            "realtime",
            "-cpu-used",
            "8",
            "-lag-in-frames",
            "0",
            "-error-resilient",
            "1",
            "-threads",
            "2",
            "-r",
            &s.fps.to_string(),
            "-g",
            &s.fps.to_string(),
            "-b:v",
            &format!("{}M", s.bitrate_mbps),
            "-c:a",
            "copy",
            "-f",
            "rtsp",
            "-rtsp_transport",
            "tcp",
            &target,
        ]))?;
        self.vp8 = Some((self.workers.pop().unwrap(), Instant::now()));
        let ready = (|| {
            let end = Instant::now() + Duration::from_secs(12);
            while !self.path_ready("headset_vp8") {
                ensure!(Instant::now() < end, "兼容编码启动超时");
                ensure!(
                    self.vp8.as_mut().unwrap().0.try_wait()?.is_none(),
                    "兼容编码启动失败"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(())
        })();
        if ready.is_err() {
            self.stop_vp8();
        }
        ready
    }
    fn stop_vp8(&mut self) {
        if let Some((mut child, _)) = self.vp8.take() {
            if child.try_wait().ok().flatten().is_none() {
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGTERM);
                }
                let end = Instant::now() + Duration::from_secs(2);
                while child.try_wait().ok().flatten().is_none() && Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(20));
                }
                if child.try_wait().ok().flatten().is_none() {
                    unsafe {
                        libc::kill(-(child.id() as i32), libc::SIGKILL);
                    }
                }
            }
            let _ = child.wait();
        }
    }
    pub fn maintain(&mut self) {
        self.poll_recording();
        self.poll_workers();
        if self.compatibility_poll.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.compatibility_poll = Instant::now();
        let Some((child, started)) = &mut self.vp8 else {
            return;
        };
        if child.try_wait().ok().flatten().is_some() {
            self.stop_vp8();
            return;
        }
        // Allow ICE establishment before deciding the compatibility encoder is idle.
        if started.elapsed() < Duration::from_secs(10) {
            return;
        }
        let idle = ureq::get(&format!(
            "http://127.0.0.1:{}/v3/paths/get/headset_vp8",
            self.api_port
        ))
        .timeout(Duration::from_millis(300))
        .call()
        .ok()
        .and_then(|r| r.into_json::<Value>().ok())
        .is_some_and(|v| v["readers"].as_array().is_some_and(Vec::is_empty));
        if idle {
            self.stop_vp8();
        }
    }
    fn path_ready(&self, path: &str) -> bool {
        ureq::get(&format!(
            "http://127.0.0.1:{}/v3/paths/get/{path}",
            self.api_port
        ))
        .timeout(Duration::from_millis(300))
        .call()
        .ok()
        .and_then(|r| r.into_json::<Value>().ok())
        .is_some_and(|v| v["ready"] == true)
    }
    pub fn ensure_dlna(&mut self) -> Result<()> {
        ensure!(self.status()["running"] == true, "请先开始头显串流");
        if self.dlna_started {
            return Ok(());
        }
        let source = format!("rtsp://127.0.0.1:{}/headset", self.rtsp_port);
        let target = format!("rtsp://127.0.0.1:{}/dlna", self.rtsp_port);
        let mut command=Command::new("ffmpeg");command.args([
            "-hide_banner",
            "-loglevel",
            "warning",
            "-nostdin",
            "-rtsp_transport",
            "tcp",
            "-i",
            &source,
            "-map",
            "0:v:0",
            "-map",
            "0:a?",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-ar",
            "48000",
            "-f",
            "rtsp",
            "-rtsp_transport",
            "tcp",
        ]);
        if self.active_settings.as_ref().is_some_and(|s|s.codec==Codec::H265) {command.args(["-c:v","libx264","-preset","ultrafast","-tune","zerolatency","-profile:v","baseline","-bf","0","-r",&self.active_settings.as_ref().unwrap().fps.to_string()]);} else {command.args(["-c:v","copy"]);}
        command.arg(&target);
        self.spawn(&mut command)?;
        let deadline=Instant::now()+Duration::from_secs(12);
        while !self.path_ready("dlna") {
            ensure!(self.workers.last_mut().unwrap().try_wait()?.is_none(),"DLNA 兼容编码启动失败：{}",fs::read_to_string(self.directory.join("stream.log")).unwrap_or_default());
            ensure!(Instant::now()<deadline,"DLNA 兼容编码启动超时");
            std::thread::sleep(Duration::from_millis(50));
        }
        self.dlna_started = true;
        Ok(())
    }
    pub fn stop(&mut self) -> Value {
        self.preview_restore = None;
        self.stop_workers();
        self.error = None;
        json!(true)
    }
    fn configure_streaming(&mut self, enabled: bool) -> Result<()> {
        ureq::AgentBuilder::new().redirects(0).timeout(Duration::from_secs(3)).build()
            .patch(&format!("http://127.0.0.1:{}/v3/config/global/patch", self.api_port))
            .send_json(json!({"webrtc":enabled,"hls":enabled}))
            .context("无法切换串流服务")?;
        self.streaming = enabled;
        if !enabled {self.stream_key.clear();self.stop_vp8();}
        else {self.stream_key = crate::session::cast_random_key();}
        Ok(())
    }
    pub fn stop_streaming(&mut self) -> Result<Value> {
        self.status();
        if self.recording.is_some() {
            if self.streaming {self.configure_streaming(false)?;}
        } else {self.stop();}
        Ok(json!(true))
    }
    pub fn start_preview(&mut self, settings: &Settings, socket: &Path) -> Result<Value> {
        ensure!(!self.region.editing, "请先关闭画幅调整窗口");
        ensure!(self.recording.is_none(), "请先停止录制再调整取景");
        ensure!(self.preview_restore.is_none(), "取景预览已在运行");
        let restore = self.active_settings.clone();
        self.stop_workers();
        let mut reference = settings.clone();
        reference.source = Source::Screen;
        reference.codec = Codec::H264;
        reference.output = Output::Eye;
        reference.width = 960;
        reference.height = 960;
        reference.fps = 30;
        reference.bitrate_mbps = 4;
        reference.horizontal_fov = None;
        reference.center_x = 0.;
        reference.center_y = 0.;
        reference.system_audio = false;
        reference.microphone = false;
        self.start(&reference, Some(socket))?;
        self.preview_restore = Some(restore);
        Ok(self.status())
    }
    pub fn end_preview(&mut self) -> Result<Option<Settings>> {
        let restore = self.preview_restore.take().context("没有正在运行的取景预览")?;
        self.stop_workers();
        self.error = None;
        Ok(restore)
    }
    fn spawn(&mut self, cmd: &mut Command) -> Result<()> {
        bind_to_session(cmd);
        cmd.process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.directory.join("stream.log"))?;
        cmd.stderr(Stdio::from(log));
        self.workers.push(cmd.spawn().context("启动串流程序失败")?);
        Ok(())
    }
    pub fn start(&mut self, settings: &Settings, panel_socket: Option<&Path>) -> Result<Value> {
        ensure!(!self.region.editing, "请先关闭画幅调整窗口");
        self.status();
        ensure!(!self.streaming, "串流已在运行");
        if self.started.is_some() {
            // Recording already owns the encoder. Enabling viewing must not
            // restart it or change the recording's resolution/bitrate.
            ensure!(self.active_settings.as_ref().is_some_and(|s|s.same_capture(settings)), "请先停止录制再调整取景");
            self.configure_streaming(true)?;
            return Ok(self.status());
        }
        self.start_capture(settings, panel_socket, true)
    }
    fn start_capture(&mut self, settings: &Settings, panel_socket: Option<&Path>, streaming: bool) -> Result<Value> {
        self.region.invalidate();
        settings.validate()?;
        ensure!(self.started.is_none(), "串流已在运行");
        fs::create_dir_all(&self.directory)?;
        fs::set_permissions(&self.directory, fs::Permissions::from_mode(0o700))?;
        // A new session does not accumulate unbounded log history.
        fs::write(self.directory.join("stream.log"), [])?;
        self.streaming = streaming;
        let result = self.start_inner(settings, panel_socket);
        if let Err(e) = &result {
            use std::io::{Read, Seek, SeekFrom};
            let mut detail = String::new();
            if let Ok(mut log) = fs::File::open(self.directory.join("stream.log")) {
                if let Ok(length) = log.metadata().map(|m| m.len()) {
                    let _ = log.seek(SeekFrom::Start(length.saturating_sub(2048)));
                    let mut bytes = Vec::new();
                    let _ = log.take(2048).read_to_end(&mut bytes);
                    detail = String::from_utf8_lossy(&bytes).into_owned();
                }
            }
            self.error = Some(format!("{e:#}\n{}", detail.trim()));
            self.stop_workers();
            super::retain_failed_log(&self.directory, "stream");
            return Err(anyhow::anyhow!(self.error.clone().unwrap()));
        }
        result
    }
    fn start_inner(&mut self, s: &Settings, panel_socket: Option<&Path>) -> Result<Value> {
        let mediamtx = self.binaries.join("mediamtx");
        ensure!(
            mediamtx.is_file(),
            "缺少内置媒体服务，请重新安装完整 Framely 运行包"
        );
        let rtsp = port()?;
        self.rtsp_port = rtsp;
        self.api_port = port()?;
        self.whep_port = port()?;
        self.hls_port = port()?;
        self.stream_key = if self.streaming {crate::session::cast_random_key()} else {String::new()};
        let configuration = format!("logLevel: warn\nrtspAddress: 127.0.0.1:{rtsp}\nrtspTransports: [tcp]\nrtmp: no\nmoq: no\nsrt: no\napi: yes\napiAddress: 127.0.0.1:{}\nmetrics: no\nplayback: no\nwebrtc: {}\nwebrtcAddress: 127.0.0.1:{}\nwebrtcLocalUDPAddress: :0\nwebrtcLocalTCPAddress: :0\nhls: {}\nhlsAddress: 127.0.0.1:{}\nhlsVariant: mpegts\nhlsSegmentCount: 3\nhlsSegmentDuration: 1s\npaths:\n  headset:\n    source: publisher\n  dlna:\n    source: publisher\n  headset_vp8:\n    source: publisher\n", self.api_port, self.streaming, self.whep_port, self.streaming, self.hls_port);
        let config_path = self.directory.join("mediamtx.yml");
        fs::write(&config_path, configuration)?;
        self.spawn(Command::new(mediamtx).arg(config_path))?;
        // Wait for readiness instead of hiding startup failures behind a fixed sleep.
        let deadline = Instant::now() + Duration::from_secs(5);
        while std::net::TcpStream::connect(("127.0.0.1", rtsp)).is_err() {
            ensure!(Instant::now() < deadline, "媒体服务启动超时");
            if let Some(code) = self.workers[0].try_wait()? {
                bail!("媒体服务启动失败：{code}");
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        let mut ffmpeg = Command::new("ffmpeg");
        ffmpeg.args(["-hide_banner", "-loglevel", "warning", "-nostdin"]);
        let mut captured = None;
        if s.source == Source::Screen {
            let executable = self.binaries.join("framely-capture");
            ensure!(executable.is_file(), "缺少屏幕采集程序");
            let socket = panel_socket.context("缺少屏幕采集助手连接")?;
            let stream = std::os::unix::net::UnixStream::connect(socket)?;
            use std::os::fd::AsRawFd;
            let fd = stream.as_raw_fd();
            let eye = if s.output == Output::Raw {
                0
            } else {
                match s.eye {
                    Eye::Left => 0,
                    Eye::Right => 1,
                    Eye::SteamVR => {
                        dominant_eye().context("无法读取 SteamVR 主力眼，请手动选择左眼或右眼")?
                    }
                }
            };
            let mut command = Command::new(executable);
            command.args([
                "--width",
                &s.width.to_string(),
                "--height",
                &s.height.to_string(),
                "--fps",
                &s.fps.to_string(),
                "--bitrate",
                &s.bitrate_mbps.to_string(),
                "--eye",
                &eye.to_string(),
                "--codec",
                s.codec.name(),
            ]);
            if s.output == Output::Raw {
                command.arg("--raw");
            } else {
                if let Some(fov) = s.horizontal_fov {
                    command.args(["--fov", &fov.to_string()]);
                }
                // Bind signed values to their options: older capture workers'
                // clap parser treats a separate negative number as a new flag.
                command.args([
                    format!("--center-x={}", s.center_x),
                    format!("--center-y={}", s.center_y),
                ]);
            }
            bind_to_session(&mut command);
            command
                .process_group(0)
                .env("FRAMELY_GRAB_FD", "3")
                .stdout(Stdio::piped())
                .stdin(Stdio::null());
            let log = fs::OpenOptions::new()
                .append(true)
                .open(self.directory.join("stream.log"))?;
            command.stderr(Stdio::from(log));
            unsafe {
                command.pre_exec(move || {
                    if fd != 3 && libc::dup2(fd, 3) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            let mut child = command.spawn()?;
            drop(stream);
            captured = child.stdout.take();
            self.workers.push(child);
            ffmpeg.args([
                "-fflags",
                "+genpts",
                "-thread_queue_size",
                "4",
                "-probesize",
                "32768",
                "-analyzeduration",
                "0",
                "-framerate",
                &s.fps.to_string(),
                // Timestamp raw packets before FFmpeg schedules the inputs.
                // Use the capture frame count rather than arrival wallclock:
                // pipe reads can batch frames and distort scheduler pacing.
                "-bsf:v",
                &video_packet_timestamps(s.fps),
                "-f",
                if s.codec == Codec::H265 {"hevc"} else {"h264"},
                "-i",
                "pipe:0",
            ]);
        } else {
            ffmpeg.args(["-thread_queue_size", "4", "-f", "v4l2", "-i", "/dev/video99"]);
        }
        if s.system_audio {
            add_pulse_input(&mut ffmpeg, "@DEFAULT_MONITOR@");
        }
        if s.microphone {
            add_pulse_input(&mut ffmpeg, "default");
        }
        ffmpeg.args(["-map", "0:v:0"]);
        let audio = map_audio(&mut ffmpeg, s.system_audio, s.microphone);
        if s.source == Source::Screen {
            ffmpeg.args(["-c:v", "copy"]);
            if s.codec == Codec::H265 {
                // Iris HEVC headers omit reliable frame timing. Write the requested
                // timing without decoding/re-encoding, avoiding false 50 fps playback.
                ffmpeg.args(["-bsf:v", &format!(
                    "hevc_metadata=tick_rate={}:num_ticks_poc_diff_one=1", s.fps
                )]);
            }
        } else {
            ffmpeg.args(["-vf", &format!("fps={},scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2,format=yuv420p",s.fps,s.width,s.height,s.width,s.height),
                "-c:v", if s.codec==Codec::H265 {"libx265"} else {"libx264"}, "-preset", "ultrafast", "-tune", "zerolatency", "-bf", "0", "-g", &s.fps.to_string(), "-b:v", &format!("{}M",s.bitrate_mbps)]);
            if s.codec==Codec::H265 {ffmpeg.args(["-x265-params", "bframes=0:rc-lookahead=0:repeat-headers=1:pools=2:frame-threads=1"]);} else {ffmpeg.args(["-profile:v", "baseline"]);}
        }
        if audio {
            ffmpeg.args(["-c:a", "libopus", "-b:a", "96k", "-ar", "48000", "-application", "lowdelay", "-frame_duration", "10"]);
        } else {
            ffmpeg.arg("-an");
        }
        ffmpeg.args([
            "-max_interleave_delta",
            "100000",
            "-flush_packets",
            "1",
            "-f",
            "rtsp",
            "-rtsp_transport",
            "tcp",
            &format!("rtsp://127.0.0.1:{rtsp}/headset"),
        ]);
        bind_to_session(&mut ffmpeg);
        ffmpeg.process_group(0).stdout(Stdio::null());
        ffmpeg.stdin(captured.map(Stdio::from).unwrap_or_else(Stdio::null));
        ffmpeg.stderr(Stdio::from(
            fs::OpenOptions::new()
                .append(true)
                .open(self.directory.join("stream.log"))?,
        ));
        self.workers
            .push(ffmpeg.spawn().context("启动 FFmpeg 失败")?);
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            if self
                .workers
                .iter_mut()
                .any(|child| child.try_wait().ok().flatten().is_some())
            {
                bail!("采集或编码程序启动失败，请查看串流日志");
            }
            if ureq::get(&format!(
                "http://127.0.0.1:{}/v3/paths/get/headset",
                self.api_port
            ))
            .timeout(Duration::from_millis(300))
            .call()
            .ok()
            .and_then(|r| r.into_json::<Value>().ok())
            .is_some_and(|v| v["ready"] == true)
            {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "采集未输出画面，请确认头显已唤醒"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        self.started = Some(Instant::now());
        self.active_settings = Some(s.clone());
        self.error = None;
        Ok(self.status())
    }
}
impl Drop for Casting {
    fn drop(&mut self) {
        self.stop_workers();
    }
}

pub fn dominant_eye() -> Result<usize> {
    let home = crate::steam::home()?;
    let custom = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    let files = [
        custom.join("openvr/config/steamvr.vrsettings"),
        home.join(".local/share/Steam/config/steamvr.vrsettings"),
        PathBuf::from("/opt/steamvr/resources/settings/default.vrsettings"),
    ];
    for path in files {
        let Ok(bytes) = fs::read(path) else {
            continue;
        };
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        if let Some(eye) = value["steamvr"]["dominantEye"].as_u64().filter(|v| *v <= 1) {
            return Ok(eye as usize);
        }
    }
    bail!("未找到 SteamVR 主力眼设置")
}

pub fn binary_directory(assets: &Path) -> PathBuf {
    let root = assets
        .parent()
        .and_then(Path::parent)
        .unwrap_or(Path::new("."));
    let installed = root.join("lib/media");
    if installed.is_dir() {
        installed
    } else {
        root.join("media/bin")
    }
}

// Pulse defaults to large fragments. Small fragments and independent, bounded
// input queues prevent a blocking audio read from backing up live video.
fn add_pulse_input(command: &mut Command, device: &str) {
    command.args(["-thread_queue_size", "4", "-f", "pulse", "-sample_rate", "48000", "-channels", "2", "-fragment_size", "3840", "-i", device]);
}

// Pulse capture timestamps use the audio server's clock, while raw panel video
// starts at zero. Generate audio PTS from the sample count in that same origin.
// libopus low-delay mode subtracts its 120-sample pre-roll from packet PTS, so
// offset the input by 120 samples and keep the first RTSP packet non-negative.
fn map_audio(command: &mut Command, system_audio: bool, microphone: bool) -> bool {
    let audio = system_audio || microphone;
    if system_audio && microphone {
        command.args([
            "-filter_complex",
            "[1:a][2:a]amix=inputs=2:duration=longest:normalize=0,asetpts=(N+120)/SR/TB[a]",
            "-map",
            "[a]",
        ]);
    } else if audio {
        command.args(["-map", "1:a:0", "-af", "asetpts=(N+120)/SR/TB"]);
    }
    audio
}

fn video_packet_timestamps(fps: u32) -> String {
    format!("setts=time_base=1/{fps}:pts=N:dts=N:duration=1")
}

pub fn bind_to_session(command: &mut Command) {
    // Linux PDEATHSIG follows the spawning thread, not the session process.
    // HTTP request threads end immediately after returning a response. Worker
    // cleanup belongs to the owning runtime and the session's systemd cgroup.
    command.process_group(0);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_mapping_generates_monotonic_timestamps() {
        let arguments = |system_audio, microphone| {
            let mut command = Command::new("ffmpeg");
            let enabled = map_audio(&mut command, system_audio, microphone);
            let arguments = command
                .get_args()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            (enabled, arguments)
        };
        assert_eq!(arguments(false, false), (false, vec![]));
        for inputs in [(true, false), (false, true)] {
            assert_eq!(
                arguments(inputs.0, inputs.1),
                (
                    true,
                    vec!["-map", "1:a:0", "-af", "asetpts=(N+120)/SR/TB"]
                        .into_iter()
                        .map(str::to_owned)
                        .collect()
                )
            );
        }
        assert_eq!(
            arguments(true, true),
            (
                true,
                vec![
                    "-filter_complex",
                    "[1:a][2:a]amix=inputs=2:duration=longest:normalize=0,asetpts=(N+120)/SR/TB[a]",
                    "-map",
                    "[a]",
                ]
                .into_iter()
                .map(str::to_owned)
                .collect()
            )
        );
    }
    #[test]
    fn cast_recording_only_stays_private_and_releases_capture_on_stop() {
        let dir=tempfile::tempdir().unwrap();let output=dir.path().join("videos");
        let mut cast=Casting::new(dir.path().into(),dir.path().into());
        cast.started=Some(Instant::now());cast.active_settings=Some(Settings::default());
        cast.recording=Some(super::super::recording::test_recording(dir.path(),&output,false));
        let status=cast.status();
        assert_eq!(status["running"],false);assert_eq!(status["recording"]["active"],true);
        assert!(status["watchPath"].is_null());assert!(cast.stream_key.is_empty());
        cast.stop_streaming().unwrap();assert!(cast.recording.is_some());
        let id=cast.recording.as_ref().unwrap().id.clone();cast.stop_recording_id(&id);
        assert!(cast.started.is_none());assert!(cast.active_settings.is_none());
        assert_eq!(cast.recording_path.as_ref().unwrap().extension().unwrap(),"mp4");
    }
    #[test]
    fn cast_recording_stream_toggle_keeps_encoder_and_recording() {
        use tiny_http::{Response,Server};
        let server=Server::http("127.0.0.1:0").unwrap();
        let port=server.server_addr().to_ip().unwrap().port();
        let worker=std::thread::spawn(move || {
            for enabled in [true,false,true] {
                let mut req=server.recv().unwrap();assert_eq!(req.method(),&tiny_http::Method::Patch);
                assert_eq!(req.url(),"/v3/config/global/patch");
                let value:Value=serde_json::from_reader(req.as_reader()).unwrap();
                assert_eq!(value,json!({"webrtc":enabled,"hls":enabled}));
                req.respond(Response::from_string("{}")).unwrap();
            }
        });
        let dir=tempfile::tempdir().unwrap();let output=dir.path().join("videos");
        let mut cast=Casting::new(dir.path().into(),dir.path().into());
        let settings=Settings::default();cast.api_port=port;
        cast.started=Some(Instant::now());cast.active_settings=Some(settings.clone());
        cast.recording=Some(super::super::recording::test_recording(dir.path(),&output,false));
        let started=cast.started;let id=cast.recording.as_ref().unwrap().id.clone();
        assert_eq!(cast.start(&settings,None).unwrap()["running"],true);
        cast.stop_streaming().unwrap();assert_eq!(cast.status()["running"],false);
        assert_eq!(cast.recording.as_ref().unwrap().id,id);assert_eq!(cast.started,started);
        assert_eq!(cast.start(&settings,None).unwrap()["running"],true);
        cast.stop_recording().unwrap();assert!(cast.streaming);assert_eq!(cast.started,started);
        worker.join().unwrap();cast.stop();
    }
    #[test]
    fn cast_recording_capture_settings_are_shared_and_locked() {
        let dir=tempfile::tempdir().unwrap();let output=dir.path().join("videos");
        let mut cast=Casting::new(dir.path().into(),dir.path().into());
        let mut settings=Settings::default();assert_eq!(settings.width*3,settings.height*4);
        cast.started=Some(Instant::now());cast.active_settings=Some(settings.clone());
        cast.recording=Some(super::super::recording::test_recording(dir.path(),&output,false));
        settings.height=1080;assert!(cast.validate_recording_settings(&settings).is_err());
        assert!(cast.start(&settings,None).is_err());
        settings.height=1440;settings.bitrate_mbps+=1;assert!(cast.validate_recording_settings(&settings).is_err());
        settings.bitrate_mbps-=1;settings.airplay=true;settings.receiver_name="Another name".into();
        assert!(cast.validate_recording_settings(&settings).is_ok());
    }
    #[test]
    fn cast_recording_stale_indicator_does_not_stop_new_recording_and_stream_stop_finishes() {
        let dir=tempfile::tempdir().unwrap();let output=dir.path().join("videos");
        let mut cast=Casting::new(dir.path().into(),dir.path().into());
        assert!(cast.start_recording(&output).is_err());
        cast.started=Some(Instant::now());
        cast.streaming=true;
        cast.recording=Some(super::super::recording::test_recording(dir.path(),&output,false));
        assert!(cast.start_recording(&output).is_err());
        assert!(cast.start_preview(&Settings::default(),dir.path()).is_err());
        cast.stop_recording_id("old-recording-id");assert!(cast.recording.is_some());
        let id=cast.recording.as_ref().unwrap().id.clone();cast.stop_recording_id(&id);
        assert!(cast.recording.is_none());assert!(cast.started.is_some());
        cast.recording=Some(super::super::recording::test_recording(dir.path(),&output,false));
        cast.stop();assert!(cast.recording.is_none());assert!(cast.started.is_none());
        assert_eq!(fs::read_to_string(cast.recording_path.as_ref().unwrap()).unwrap(),"header\ntrailer\n");
    }
    #[test]
    fn cast_recording_process_failure_keeps_stream_and_reports_error() {
        let dir=tempfile::tempdir().unwrap();let output=dir.path().join("videos");
        let mut cast=Casting::new(dir.path().into(),dir.path().into());cast.started=Some(Instant::now());cast.streaming=true;
        cast.recording=Some(super::super::recording::test_recording(dir.path(),&output,true));
        let end=Instant::now()+Duration::from_secs(3);
        while cast.recording.is_some()&&Instant::now()<end {cast.maintain();std::thread::sleep(Duration::from_millis(25));}
        assert!(cast.recording.is_none());assert!(cast.started.is_some());assert!(cast.recording_error.is_some());
    }
    #[test]
    fn copied_video_packets_receive_frame_timestamps() {
        assert_eq!(
            video_packet_timestamps(60),
            "setts=time_base=1/60:pts=N:dts=N:duration=1"
        );
    }
    #[test]
    fn media_worker_survives_the_http_request_thread() {
        let mut child = std::thread::spawn(|| {
            let mut command = Command::new("sleep");
            command.arg("30");
            bind_to_session(&mut command);
            command.spawn().unwrap()
        }).join().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let exited = child.try_wait().unwrap();
        let _ = child.kill();
        let _ = child.wait();
        assert!(exited.is_none(), "media ended with its request thread: {exited:?}");
    }
    #[test]
    #[ignore = "requires Steam Frame, FFmpeg and the packaged media workers"]
    fn steamvr_stream_has_requested_output_and_stops_all_workers() {
        let directory = tempfile::tempdir().unwrap();
        let binaries = PathBuf::from(std::env::var("FRAMELY_MEDIA_BIN").unwrap());
        let mut cast = Casting::new(directory.path().to_owned(), binaries);
        let settings = Settings {
            source: Source::SteamVR,
            width: 640,
            height: 360,
            fps: 30,
            system_audio: true,
            ..Settings::default()
        };
        cast.start(&settings, None).unwrap_or_else(|e| {
            panic!(
                "{e:#}\n{}",
                fs::read_to_string(directory.path().join("stream.log")).unwrap_or_default()
            )
        });
        let address = format!("rtsp://127.0.0.1:{}/headset", cast.rtsp_port);
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-rtsp_transport",
                "tcp",
                "-show_entries",
                "stream=codec_name,width,height,r_frame_rate",
                "-of",
                "json",
                &address,
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let streams: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(streams["streams"][0]["width"], 640);
        assert_eq!(streams["streams"][0]["height"], 360);
        assert_eq!(streams["streams"][0]["codec_name"], "h264");
        assert!(streams["streams"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["codec_name"] == "opus"));
        cast.ensure_dlna().unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        while ureq::get(&format!(
            "http://127.0.0.1:{}/dlna/index.m3u8",
            cast.hls_port
        ))
        .timeout(Duration::from_millis(500))
        .call()
        .is_err()
        {
            assert!(
                Instant::now() < end,
                "{}",
                fs::read_to_string(directory.path().join("stream.log")).unwrap_or_default()
            );
        }
        let rtsp = cast.rtsp_port;
        cast.stop();
        assert!(std::net::TcpStream::connect(("127.0.0.1", rtsp)).is_err());
    }
}
