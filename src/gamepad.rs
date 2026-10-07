//! Optional, session-user-only input bridges. Each running container retains its pad.
use anyhow::{ensure, Context, Result};
use std::{
    collections::BTreeMap,
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
    pub direct: bool,
    pub source: String,
    pub context: String,
    pub token: String,
    pub event: PathBuf,
    pub grab: PathBuf,
    pub layout: PathBuf,
    pub ready: PathBuf,
}
struct Bridge {
    mount: Mount,
    child: Child,
    input: ChildStdin,
    package: Option<String>,
    enabled: bool,
}
#[derive(Default)]
struct Bridges {
    contexts: BTreeMap<String, Bridge>,
    active: Option<String>,
}
static BRIDGE: Mutex<Bridges> = Mutex::new(Bridges {
    contexts: BTreeMap::new(),
    active: None,
});
impl Bridge {
    fn pause(&mut self) -> Result<()> {
        writeln!(self.input, "disable")?;
        self.enabled = false;
        Ok(())
    }
    fn update(&mut self, alive: Option<bool>, focused: bool) -> Result<()> {
        // Activity/process records can disappear briefly during game updates,
        // splash-screen handoffs or a query timeout. Keep the mounted device;
        // only neutralize input until the selected app is foreground again.
        let enabled = alive != Some(false) && focused && claimed(&self.mount.ready);
        if self.enabled != enabled {
            writeln!(self.input, "{}", if enabled { "enable" } else { "disable" })?;
            self.enabled = enabled;
        }
        Ok(())
    }
}

static SESSION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn session_started() {
    SESSION.store(true, std::sync::atomic::Ordering::Relaxed);
}
static WATCHER: OnceLock<()> = OnceLock::new();
impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = writeln!(self.input, "disable");
        // A watcher may already have reaped an exited helper. Never signal
        // that recycled PID while removing its cached mount.
        if self.child.try_wait().ok().flatten().is_none() {
            unsafe {
                libc::kill(self.child.id() as i32, libc::SIGTERM);
            }
        }
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
    let mut bridges = BRIDGE.lock().unwrap();
    bridges.active = None;
    bridges.contexts.clear();
}
pub fn set_rumble_enabled(enabled: bool) {
    let mut bridges = BRIDGE.lock().unwrap();
    for bridge in bridges.contexts.values_mut() {
        let _ = writeln!(
            bridge.input,
            "{}",
            if enabled { "rumble-on" } else { "rumble-off" }
        );
    }
}
pub fn set_trigger_threshold(percent: u8) {
    let mut bridges = BRIDGE.lock().unwrap();
    for bridge in bridges.contexts.values_mut() {
        let _ = writeln!(bridge.input, "trigger-threshold {percent}");
    }
}
pub fn stop_context(context: &str) {
    let mut bridges = BRIDGE.lock().unwrap();
    if bridges.active.as_deref() == Some(context) {
        bridges.active = None;
    }
    bridges.contexts.remove(context);
}
pub fn stop_app(context: &str, package: &str) {
    let mut bridges = BRIDGE.lock().unwrap();
    if let Some(bridge) = bridges.contexts.get_mut(context) {
        if bridge.package.as_deref() == Some(package) {
            let _ = bridge.pause();
            bridge.package = None;
            if bridges.active.as_deref() == Some(context) {
                bridges.active = None;
            }
        }
    }
}
pub fn current(context: &str) -> Option<Mount> {
    let mut bridges = BRIDGE.lock().unwrap();
    if bridges
        .contexts
        .get_mut(context)
        .is_some_and(|b| b.child.try_wait().ok().flatten().is_some())
    {
        bridges.contexts.remove(context);
        if bridges.active.as_deref() == Some(context) {
            bridges.active = None;
        }
    }
    bridges.contexts.get(context).map(|b| b.mount.clone())
}
pub fn prepare(
    storage: &Path,
    context: &str,
    source: &str,
    rumble: bool,
    threshold: u8,
) -> Result<Mount> {
    ensure!(
        matches!(source, "steam" | "frame" | "steam-direct"),
        "Invalid gamepad input source"
    );
    ensure!(SESSION.load(std::sync::atomic::Ordering::Relaxed),"Gamepad launch requires the UI session; open the APK from the management panel or launcher");
    ensure!(
        unsafe { libc::geteuid() } != 0,
        "Gamepad input must run as the Steam session user"
    );
    ensure!(
        (1..=100).contains(&threshold),
        "Invalid gamepad trigger threshold"
    );
    let current = current(context);
    {
        let mut bridges = BRIDGE.lock().unwrap();
        bridges.active = None;
        for bridge in bridges.contexts.values_mut() {
            bridge.pause()?;
        }
        if let Some(bridge) = bridges.contexts.get_mut(context) {
            bridge.package = None;
            writeln!(bridge.input, "trigger-threshold {threshold}")?;
        }
    }
    if let Some(mount) = current {
        ensure!(
            mount.source == source,
            "Controller source changed; close and restart this container"
        );
        return Ok(mount);
    }
    let exe = std::env::current_exe()?;
    let distribution = exe
        .parent()
        .and_then(Path::parent)
        .context("Missing Framely runtime directory")?;
    let helper = distribution.join("lib/openvr/framely-gamepad");
    let grab = distribution.join("lib/openvr/libframely-gamepad-grab.so");
    let manifest = distribution.join("share/input/gamepad/actions.json");
    let direct = source == "steam-direct";
    let layout = distribution.join(if direct {
        "share/input/gamepad/Vendor_28de_Product_11ff.kl"
    } else {
        "share/input/gamepad/Vendor_0001_Product_f001.kl"
    });
    ensure!(
        helper.is_file() && grab.is_file() && manifest.is_file() && layout.is_file(),
        "Gamepad input runtime is missing; install a complete Framely build"
    );
    let token = hex::encode(rand::random::<[u8; 16]>());
    let ready = storage.join(format!("gamepad-{token}"));
    fs::create_dir(&ready)?;
    fs::set_permissions(&ready, fs::Permissions::from_mode(0o733))?;
    let app_key = format!("framely.gamepad.{token}");
    let app_manifest = ready.join("application.vrmanifest");
    fs::write(
        &app_manifest,
        serde_json::to_vec(&serde_json::json!({
            "applications":[{"app_key":app_key,"launch_type":"binary","is_self_identified":true,
                "binary_path_linux_arm":helper,"binary_path_linux":helper,"action_manifest_path":manifest,
                "strings":{"en_us":{"name":"Framely gamepad input"}}}]
        }))?,
    )?;
    fs::set_permissions(&app_manifest, fs::Permissions::from_mode(0o600))?;
    let log = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(storage.join("logs/gamepad.log"))?;
    let home = crate::steam::home()?;
    let steam = if home.join(".local/share/Steam").exists() {
        home.join(".local/share/Steam")
    } else {
        home.join(".steam/steam")
    };
    let mut child = Command::new(helper)
        .env("FRAMELY_GAMEPAD_SOURCE", source)
        .env("FRAMELY_GAMEPAD_READY", &ready)
        .env("FRAMELY_GAMEPAD_TRIGGER_THRESHOLD", threshold.to_string())
        .env("FRAMELY_GAMEPAD_RUMBLE", if rumble { "1" } else { "0" })
        .env(
            "FRAMELY_STEAM_SDL_LIBRARY",
            steam.join("steamrtarm64/libSDL3.so.0"),
        )
        .env(
            "FRAMELY_STEAM_GAMEPAD_INFO",
            steam.join("config/virtualgamepadinfo.txt"),
        )
        .arg(manifest)
        .arg(&app_manifest)
        .arg(&app_key)
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
            direct,
            source: source.into(),
            context: context.into(),
            token,
            event: PathBuf::new(),
            grab,
            layout,
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
    BRIDGE
        .lock()
        .unwrap()
        .contexts
        .insert(context.into(), bridge);
    WATCHER.get_or_init(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_millis(700));
            let target = {
                let mut bridges = BRIDGE.lock().unwrap();
                bridges
                    .contexts
                    .retain(|_, b| b.child.try_wait().ok().flatten().is_none());
                if bridges
                    .active
                    .as_ref()
                    .is_some_and(|context| !bridges.contexts.contains_key(context))
                {
                    bridges.active = None;
                }
                bridges
                    .active
                    .as_ref()
                    .and_then(|context| bridges.contexts.get(context))
                    .and_then(|b| b.package.as_ref().map(|p| (b.mount.clone(), p.clone())))
            };
            let Some((mount, package)) = target else {
                continue;
            };
            let (alive, focused) = crate::apk::gamepad_health(&mount.context, &package);
            let mut bridges = BRIDGE.lock().unwrap();
            if bridges.active.as_deref() != Some(&mount.context) {
                continue;
            }
            let Some(bridge) = bridges
                .contexts
                .get_mut(&mount.context)
                .filter(|b| b.mount.token == mount.token && b.package.as_deref() == Some(&package))
            else {
                continue;
            };
            if bridge.child.try_wait().ok().flatten().is_some()
                || bridge.update(alive, focused).is_err()
            {
                eprintln!("Gamepad bridge exited for container {}", mount.context);
                bridges.contexts.remove(&mount.context);
                bridges.active = None;
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
        "Android did not register the gamepad; restart the container and retry"
    );
    select_target(context, package)
}
/// Select routing before native startup acknowledgement. The watcher still
/// requires the target activity to be foreground and Android to claim the pad.
pub fn select_target(context: &str, package: &str) -> Result<()> {
    let mount = current(context).context("Gamepad input is no longer running")?;
    let mut bridges = BRIDGE.lock().unwrap();
    let bridge = bridges
        .contexts
        .get_mut(context)
        .filter(|b| b.mount.token == mount.token)
        .context("Gamepad input is no longer running")?;
    bridge.package = Some(package.into());
    bridges.active = Some(context.into());
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
    fn temporary_game_handoffs_neutralize_input_without_disconnect() {
        let directory = tempfile::tempdir().unwrap();
        let mut child = Command::new("sh")
            .args([
                "-c",
                "while IFS= read -r line; do printf '%s\n' \"$line\"; done",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let id = child.id();
        let mut bridge = Bridge {
            mount: Mount {
                direct: false,
                source: "steam".into(),
                context: "test".into(),
                token: "same-device".into(),
                event: PathBuf::from("/dev/input/event999"),
                grab: PathBuf::new(),
                layout: PathBuf::new(),
                ready: directory.path().into(),
            },
            child,
            input,
            package: Some("com.example.game".into()),
            enabled: false,
        };
        let mut read = || {
            let mut line = String::new();
            output.read_line(&mut line).unwrap();
            line
        };
        // Native routing may be selected before Android boots. Foreground
        // alone must not enable a device Android has not yet claimed.
        bridge.update(Some(true), true).unwrap();
        assert!(!bridge.enabled);
        fs::write(directory.path().join("ready"), "ready\n").unwrap();
        bridge.update(Some(true), false).unwrap();
        assert!(!bridge.enabled);
        bridge.update(Some(true), true).unwrap();
        assert_eq!(read(), "enable\n");
        bridge.update(Some(false), false).unwrap();
        assert_eq!(read(), "disable\n");
        bridge.update(None, false).unwrap();
        assert!(bridge.child.try_wait().unwrap().is_none());
        assert!(claimed(&bridge.mount.ready));
        bridge.update(Some(true), true).unwrap();
        assert_eq!(read(), "enable\n");
        bridge.pause().unwrap();
        assert_eq!(read(), "disable\n");
        assert_eq!(bridge.child.id(), id);
        assert_eq!(bridge.mount.token, "same-device");
        assert!(bridge.child.try_wait().unwrap().is_none());
    }
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
