//! MP4 recording shares the capture encoder, with AAC audio for compatibility.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    os::unix::{fs::OpenOptionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub struct Recording {
    child: Child,
    path: PathBuf,
    started: Instant,
    progress: PathBuf,
    log: PathBuf,
    pub id: String,
}
fn has_media(progress: &str) -> bool {
    // Stream-copy progress has no `frame=` field on the headset's FFmpeg.
    let positive = |name: &str| {
        progress
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix(name))
            .is_some_and(|value| value.trim().parse::<i64>().is_ok_and(|n| n > 0))
    };
    positive("out_time_us=") && positive("total_size=")
}
impl Recording {
    pub fn start(
        rtsp_port: u16,
        directory: &Path,
        output: &Path,
        codec: super::settings::Codec,
    ) -> Result<Self> {
        Self::start_command(Command::new("ffmpeg"), rtsp_port, directory, output, codec)
    }
    fn start_command(
        mut cmd: Command,
        rtsp_port: u16,
        directory: &Path,
        output: &Path,
        codec: super::settings::Codec,
    ) -> Result<Self> {
        fs::create_dir_all(output).context("无法创建录制目录")?;
        let id = hex::encode(rand::random::<[u8; 16]>());
        let path = output.join(format!(
            "Framely-{}-{}.mp4",
            crate::service::now_ms(),
            &id[..8]
        ));
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        let progress = directory.join(format!("recording-{id}.progress"));
        let log = directory.join(format!("recording-{id}.log"));
        super::runtime::bind_to_session(&mut cmd);
        let spawned = cmd
            .process_group(0)
            .args([
                "-hide_banner",
                "-loglevel",
                "warning",
                "-y",
                "-fflags",
                "+genpts",
                "-analyzeduration",
                "1000000",
                "-probesize",
                "1048576",
                "-rtsp_transport",
                "tcp",
                "-i",
                &format!("rtsp://127.0.0.1:{rtsp_port}/headset"),
                "-map",
                "0:v:0",
                "-map",
                "0:a?",
                "-c:v",
                "copy",
                "-c:a",
                "aac",
                "-b:a",
                "128k",
                "-avoid_negative_ts",
                "make_zero",
                "-stats_period",
                "0.2",
                "-progress",
            ])
            .arg(&progress)
            // Fragmented MP4 remains usable without a final global index, and
            // avoids buffering an entire recording until it is stopped.
            .args([
                "-flush_packets",
                "1",
                "-movflags",
                "+frag_keyframe+empty_moov+default_base_moof",
                "-f",
                "mp4",
            ])
            .args(if codec == super::settings::Codec::H265 {
                vec!["-tag:v", "hvc1"]
            } else {
                vec![]
            })
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&log)?,
            )
            .spawn()
            .context("启动录制程序失败");
        let child = match spawned {
            Ok(child) => child,
            Err(error) => {
                let _ = fs::remove_file(&path);
                let _ = fs::remove_file(&log);
                return Err(error);
            }
        };
        let mut recording = Self {
            child,
            path,
            started: Instant::now(),
            progress,
            log,
            id,
        };
        let ready = (|| -> Result<()> {
            let deadline = Instant::now() + Duration::from_secs(12);
            loop {
                ensure!(
                    recording.child.try_wait()?.is_none(),
                    "录制程序启动失败：{}",
                    recording.detail()
                );
                if fs::read_to_string(&recording.progress)
                    .ok()
                    .is_some_and(|s| has_media(&s))
                {
                    return Ok(());
                }
                ensure!(
                    Instant::now() < deadline,
                    "录制启动超时：{}",
                    recording.detail()
                );
                std::thread::sleep(Duration::from_millis(50));
            }
        })();
        if let Err(error) = ready {
            let _ = recording.finish();
            return Err(error);
        }
        Ok(recording)
    }
    pub fn status(&self) -> Value {
        json!({"active":true,"id":self.id,"path":self.path,"elapsedSeconds":self.started.elapsed().as_secs()})
    }
    pub fn exited(&mut self) -> Result<bool> {
        Ok(self.child.try_wait()?.is_some())
    }
    pub fn detail(&self) -> String {
        fs::read_to_string(&self.log)
            .map(|s| {
                s.chars()
                    .rev()
                    .take(2048)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn finish(&mut self) -> Result<PathBuf> {
        if self.child.try_wait()?.is_none() {
            if let Some(mut stdin) = self.child.stdin.take() {
                let _ = stdin.write_all(b"q\n");
            }
            let deadline = Instant::now() + Duration::from_secs(8);
            while self.child.try_wait()?.is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(25));
            }
            if self.child.try_wait()?.is_none() {
                unsafe {
                    libc::kill(-(self.child.id() as i32), libc::SIGINT);
                }
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            while self.child.try_wait()?.is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(25));
            }
            if self.child.try_wait()?.is_none() {
                unsafe {
                    libc::kill(-(self.child.id() as i32), libc::SIGKILL);
                }
                let _ = self.child.wait();
                anyhow::bail!("录制结束超时，已保留可恢复文件：{}", self.path.display());
            }
        }
        let status = self.child.wait()?;
        ensure!(
            status.success(),
            "录制异常结束，文件已保留：{}；{}",
            self.path.display(),
            self.detail()
        );
        Ok(self.path.clone())
    }
}
impl Drop for Recording {
    fn drop(&mut self) {
        let _ = self.finish();
        let _ = fs::remove_file(&self.progress);
        let _ = fs::remove_file(&self.log);
    }
}

#[cfg(test)]
pub(super) fn test_recording(directory: &Path, output: &Path, fail: bool) -> Recording {
    let script = directory.join("fake-ffmpeg.sh");
    fs::write(
        &script,
        r#"previous=''
for argument in "$@"; do
  if [ "$previous" = '-progress' ]; then progress="$argument"; fi
  previous="$argument"
done
printf 'header\n' > "$previous"
printf 'out_time_us=1000\ntotal_size=100\n' > "$progress"
if [ "$FAIL_LATER" = 1 ]; then sleep 0.4; exit 1; fi
read command
[ "$command" = q ] || exit 2
printf 'trailer\n' >> "$previous"
"#,
    )
    .unwrap();
    let mut command = Command::new("sh");
    command
        .arg(script)
        .env("FAIL_LATER", if fail { "1" } else { "0" });
    Recording::start_command(command, 1, directory, output, super::settings::Codec::H264).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn cast_recording_copy_progress_does_not_require_frame_count() {
        assert!(has_media(
            "total_size=38485\nout_time_us=1191811\nprogress=continue\n"
        ));
        assert!(!has_media("total_size=38485\nout_time_us=N/A\n"));
        assert!(!has_media("frame=1\ntotal_size=0\nout_time_us=0\n"));
    }

    #[test]
    fn cast_recording_finalizes_and_uses_unique_private_files() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("videos");
        let mut first = test_recording(dir.path(), &output, false);
        let path = first.finish().unwrap();
        assert_eq!(path.extension().unwrap(), "mp4");
        assert_eq!(fs::read_to_string(&path).unwrap(), "header\ntrailer\n");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let mut second = test_recording(dir.path(), &output, false);
        assert_ne!(first.id, second.id);
        assert_ne!(path, second.finish().unwrap());
    }
    #[test]
    fn cast_recording_drop_finalizes_and_cleans_progress() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("videos");
        let first = test_recording(dir.path(), &output, false);
        let path = first.path.clone();
        let progress = first.progress.clone();
        drop(first);
        assert_eq!(fs::read_to_string(path).unwrap(), "header\ntrailer\n");
        assert!(!progress.exists());
    }
    #[test]
    fn cast_recording_spawn_failure_cleans_empty_output() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("videos");
        assert!(Recording::start_command(
            Command::new("/framely-no-such-ffmpeg"),
            1,
            dir.path(),
            &output,
            super::super::settings::Codec::H264
        )
        .is_err());
        assert_eq!(fs::read_dir(output).unwrap().count(), 0);
    }

    #[test]
    #[ignore = "requires FFmpeg, ffprobe and MediaMTX; uses isolated test streams"]
    fn cast_recording_real_mux_preserves_video_audio_and_finalizes() {
        struct Worker(Child);
        impl Drop for Worker {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let port = || {
            std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port()
        };
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("videos");
        let rtsp = port();
        let api = port();
        let whep = port();
        let hls = port();
        let config = dir.path().join("mediamtx.yml");
        fs::write(&config,format!("logLevel: warn\nrtspAddress: 127.0.0.1:{rtsp}\nrtspTransports: [tcp]\napi: yes\napiAddress: 127.0.0.1:{api}\nrtmp: no\nhls: no\nhlsAddress: 127.0.0.1:{hls}\nwebrtc: no\nwebrtcAddress: 127.0.0.1:{whep}\nwebrtcLocalUDPAddress: :0\nwebrtcLocalTCPAddress: :0\nsrt: no\nmoq: no\npaths:\n  headset:\n    source: publisher\n")).unwrap();
        let binary = std::env::var("FRAMELY_MEDIA_MTX").unwrap();
        let _server = Worker(
            Command::new(binary)
                .arg(config)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let end = Instant::now() + Duration::from_secs(5);
        while std::net::TcpStream::connect(("127.0.0.1", rtsp)).is_err() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(50));
        }
        for (codec, audio) in [("h264", true), ("hevc", true), ("h264", false)] {
            let mut command = Command::new("ffmpeg");
            command.args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-re",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=160x96:rate=15",
            ]);
            if audio {
                command.args([
                    "-re",
                    "-f",
                    "lavfi",
                    "-i",
                    "sine=frequency=440:sample_rate=48000",
                ]);
            }
            command.args([
                "-c:v",
                if codec == "hevc" {
                    "libx265"
                } else {
                    "libx264"
                },
                "-preset",
                "ultrafast",
                "-tune",
                "zerolatency",
                "-threads",
                "1",
                "-g",
                "15",
            ]);
            if codec == "hevc" {
                command.args([
                    "-x265-params",
                    "log-level=error:pools=1:frame-threads=1:bframes=0",
                ]);
            }
            if audio {
                command.args(["-c:a", "libopus"]);
            } else {
                command.arg("-an");
            }
            let address = format!("rtsp://127.0.0.1:{rtsp}/headset");
            let _publisher = Worker(
                command
                    .args(["-f", "rtsp", "-rtsp_transport", "tcp", &address])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let end = Instant::now() + Duration::from_secs(8);
            loop {
                if ureq::get(&format!("http://127.0.0.1:{api}/v3/paths/get/headset"))
                    .timeout(Duration::from_millis(500))
                    .call()
                    .ok()
                    .and_then(|r| r.into_json::<Value>().ok())
                    .is_some_and(|v| v["ready"] == true)
                {
                    break;
                }
                assert!(Instant::now() < end, "publisher not ready");
                std::thread::sleep(Duration::from_millis(50));
            }
            let mut recording = Recording::start(
                rtsp,
                dir.path(),
                &output,
                if codec == "hevc" {
                    super::super::settings::Codec::H265
                } else {
                    super::super::settings::Codec::H264
                },
            )
            .unwrap();
            // Toggling viewing services must leave the RTSP publisher/recorder
            // intact. Record-only initially has no WebRTC or HLS listeners.
            assert!(std::net::TcpStream::connect(("127.0.0.1", whep)).is_err());
            for enabled in [true, false] {
                ureq::patch(&format!("http://127.0.0.1:{api}/v3/config/global/patch"))
                    .timeout(Duration::from_secs(3))
                    .send_json(json!({"webrtc":enabled,"hls":enabled}))
                    .unwrap();
                let end = Instant::now() + Duration::from_secs(3);
                while [whep, hls].iter().any(|port| {
                    std::net::TcpStream::connect(("127.0.0.1", *port)).is_ok() != enabled
                }) {
                    assert!(Instant::now() < end, "viewing listeners did not switch");
                    std::thread::sleep(Duration::from_millis(20));
                }
                let path: Value =
                    ureq::get(&format!("http://127.0.0.1:{api}/v3/paths/get/headset"))
                        .timeout(Duration::from_secs(2))
                        .call()
                        .unwrap()
                        .into_json()
                        .unwrap();
                assert_eq!(path["ready"], true, "viewing toggle interrupted capture");
                assert!(!recording.exited().unwrap());
            }
            std::thread::sleep(Duration::from_millis(800));
            let path = recording.finish().unwrap();
            assert_eq!(path.extension().unwrap(), "mp4");
            let probe = Command::new("ffprobe")
                .args([
                    "-v",
                    "error",
                    "-show_streams",
                    "-show_format",
                    "-of",
                    "json",
                ])
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                probe.status.success(),
                "{}",
                String::from_utf8_lossy(&probe.stderr)
            );
            let data: Value = serde_json::from_slice(&probe.stdout).unwrap();
            let streams = data["streams"].as_array().unwrap();
            assert!(
                data["format"]["format_name"]
                    .as_str()
                    .unwrap()
                    .contains("mp4"),
                "{data}"
            );
            if codec == "hevc" {
                assert!(
                    streams
                        .iter()
                        .any(|s| s["codec_name"] == "hevc" && s["codec_tag_string"] == "hvc1"),
                    "{data}"
                );
            }
            assert!(
                streams
                    .iter()
                    .any(|s| s["codec_name"] == codec && s["width"] == 160 && s["height"] == 96),
                "{data}"
            );
            assert_eq!(
                streams.iter().any(|s| s["codec_name"] == "aac"),
                audio,
                "{data}"
            );
            assert!(
                data["format"]["duration"]
                    .as_str()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap()
                    > 0.5,
                "{data}"
            );
            let decoded = Command::new("ffmpeg")
                .args(["-v", "error", "-xerror", "-i"])
                .arg(&path)
                .args(["-f", "null", "-"])
                .output()
                .unwrap();
            assert!(
                decoded.status.success(),
                "saved recording is not decodable: {}",
                String::from_utf8_lossy(&decoded.stderr)
            );
            if audio {
                let pcm = Command::new("ffmpeg")
                    .args(["-v", "error", "-i"])
                    .arg(&path)
                    .args(["-map", "0:a:0", "-t", "0.2", "-f", "s16le", "pipe:1"])
                    .output()
                    .unwrap();
                assert!(pcm.status.success());
                assert!(
                    pcm.stdout
                        .chunks_exact(2)
                        .any(|s| i16::from_le_bytes([s[0], s[1]]).unsigned_abs() > 1000),
                    "recorded audio is silent"
                );
            }
            println!(
                "PASS: {codec}, audio={audio}, finalized {} bytes",
                fs::metadata(path).unwrap().len()
            );
            drop(recording);
            drop(_publisher);
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}
