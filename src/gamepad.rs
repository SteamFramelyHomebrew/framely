//! Optional, session-user-only input bridge. One container owns the virtual pad.
use anyhow::{ensure, Context, Result};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
#[derive(Clone)]
pub struct Mount {
    pub context: String,
    pub token: String,
    pub event: PathBuf,
    pub grab: PathBuf,
    pub ready: PathBuf,
}
struct Bridge {
    mount: Mount,
    child: Child,
    input: ChildStdin,
    package: Option<String>,
    enabled: bool,
}
static BRIDGE: Mutex<Option<Bridge>> = Mutex::new(None);
static SESSION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn session_started() {
    SESSION.store(true, std::sync::atomic::Ordering::Relaxed);
}
static WATCHER: OnceLock<()> = OnceLock::new();
impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = writeln!(self.input, "disable");
        unsafe {
            libc::kill(self.child.id() as i32, libc::SIGTERM);
        };
        let began = Instant::now();
        while began.elapsed() < Duration::from_millis(500) {
            if self.child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.mount.ready);
    }
}
pub fn stop() {
    BRIDGE.lock().unwrap().take();
}
pub fn stop_context(context: &str) {
    let mut b = BRIDGE.lock().unwrap();
    if b.as_ref().is_some_and(|b| b.mount.context == context) {
        b.take();
    }
}
pub fn stop_app(context: &str, package: &str) {
    let mut b = BRIDGE.lock().unwrap();
    if b.as_ref()
        .is_some_and(|b| b.mount.context == context && b.package.as_deref() == Some(package))
    {
        b.take();
    }
}
pub fn current(context: &str) -> Option<Mount> {
    let mut b = BRIDGE.lock().unwrap();
    if let Some(value) = b.as_mut() {
        if value.child.try_wait().ok().flatten().is_some() {
            b.take();
        }
    }
    b.as_ref()
        .filter(|b| b.mount.context == context)
        .map(|b| b.mount.clone())
}
pub fn prepare(storage: &Path, context: &str) -> Result<Mount> {
    ensure!(SESSION.load(std::sync::atomic::Ordering::Relaxed),"Gamepad launch requires the UI session; open the APK from the management panel or launcher");
    ensure!(
        unsafe { libc::geteuid() } != 0,
        "Gamepad input must run as the Steam session user"
    );
    if let Some(m) = current(context) {
        let mut b = BRIDGE.lock().unwrap();
        if let Some(bridge) = b.as_mut() {
            writeln!(bridge.input, "disable")?;
            bridge.package = None;
            bridge.enabled = false;
        }
        return Ok(m);
    }
    stop();
    let exe = std::env::current_exe()?;
    let distribution = exe
        .parent()
        .and_then(Path::parent)
        .context("Missing Framely runtime directory")?;
    let helper = distribution.join("lib/openvr/framely-gamepad");
    let grab = distribution.join("lib/openvr/libframely-gamepad-grab.so");
    let manifest = distribution.join("share/input/gamepad/actions.json");
    ensure!(
        helper.is_file() && grab.is_file() && manifest.is_file(),
        "Gamepad input runtime is missing; install a complete Framely build"
    );
    let token = hex::encode(rand::random::<[u8; 16]>());
    let ready = storage.join(format!("gamepad-{token}"));
    fs::create_dir(&ready)?;
    fs::set_permissions(&ready, fs::Permissions::from_mode(0o733))?;
    let log = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(storage.join("logs/gamepad.log"))?;
    let mut child = Command::new(helper)
        .arg(manifest)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(log)
        .spawn()?;
    let input = child.stdin.take().context("Missing gamepad control pipe")?;
    let stdout = child
        .stdout
        .take()
        .context("Missing gamepad readiness pipe")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = tx.send(result);
    });
    let event = rx
        .recv_timeout(Duration::from_secs(8))
        .context("Gamepad input did not become ready")
        .and_then(|v| v.map_err(Into::into))
        .map(|s| PathBuf::from(s.trim()));
    let mut bridge = Bridge {
        mount: Mount {
            context: context.into(),
            token,
            event: PathBuf::new(),
            grab,
            ready,
        },
        child,
        input,
        package: None,
        enabled: false,
    };
    let event = event?;
    ensure!(
        valid_node(&event) && fs::metadata(&event)?.file_type().is_char_device(),
        "Invalid gamepad event device"
    );
    bridge.mount.event = event;
    let mount = bridge.mount.clone();
    *BRIDGE.lock().unwrap() = Some(bridge);
    WATCHER.get_or_init(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_millis(700));
            let target = {
                let b = BRIDGE.lock().unwrap();
                b.as_ref()
                    .and_then(|b| b.package.as_ref().map(|p| (b.mount.clone(), p.clone())))
            };
            let Some((mount, package)) = target else {
                continue;
            };
            let (alive, focused) = crate::apk::gamepad_health(&mount.context, &package);
            let mut b = BRIDGE.lock().unwrap();
            if b.as_ref().is_none_or(|b| b.mount.token != mount.token) {
                continue;
            }
            if alive == Some(false)
                || b.as_mut()
                    .is_some_and(|b| b.child.try_wait().ok().flatten().is_some())
            {
                b.take();
                continue;
            }
            let bridge = b.as_mut().unwrap();
            let enabled = focused && claimed(&mount.ready);
            if bridge.enabled != enabled {
                if writeln!(
                    bridge.input,
                    "{}",
                    if enabled { "enable" } else { "disable" }
                )
                .is_err()
                {
                    b.take();
                    continue;
                }
                bridge.enabled = enabled;
            }
        });
    });
    Ok(mount)
}
pub fn activate(context: &str, package: &str) -> Result<()> {
    let mount = current(context).context("Gamepad input is no longer running")?;
    let began = Instant::now();
    while !claimed(&mount.ready) && began.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(30));
    }
    ensure!(
        claimed(&mount.ready),
        "Android did not claim the virtual gamepad; restart the container and retry"
    );
    let mut b = BRIDGE.lock().unwrap();
    let bridge = b
        .as_mut()
        .filter(|b| b.mount.token == mount.token)
        .context("Gamepad input is no longer running")?;
    bridge.package = Some(package.into());
    Ok(())
}
fn claimed(directory: &Path) -> bool {
    let Ok(file) = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(directory.join("ready"))
    else {
        return false;
    };
    if !file.metadata().is_ok_and(|m| m.is_file() && m.len() == 6) {
        return false;
    }
    let mut data = String::new();
    file.take(16).read_to_string(&mut data).is_ok() && data == "ready\n"
}
fn valid_node(p: &Path) -> bool {
    p.parent() == Some(Path::new("/dev/input"))
        && p.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
            s.strip_prefix("event")
                .is_some_and(|n| !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()))
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_requires_a_regular_acknowledgement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ready");
        assert!(!claimed(dir.path()));
        fs::write(&path, "ready\n").unwrap();
        assert!(claimed(dir.path()));
        fs::write(&path, "not ready\n").unwrap();
        assert!(!claimed(dir.path()));
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink("/dev/zero", &path).unwrap();
        assert!(!claimed(dir.path()));
        fs::remove_file(&path).unwrap();
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(!claimed(dir.path()));
    }
    #[test]
    fn only_event_nodes_are_accepted() {
        assert!(valid_node(Path::new("/dev/input/event5")));
        for path in [
            "/dev/input/event",
            "/dev/uinput",
            "/dev/input/../event5",
            "/dev/input/event5/child",
            "/tmp/event5",
        ] {
            assert!(!valid_node(Path::new(path)));
        }
    }
}
