//! Session-only lifecycle monitoring; never touches package or application data.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

#[derive(Clone)]
struct Sample {
    instance: String,
    package: String,
    window: Option<bool>,
    alive: BTreeSet<String>,
}
#[derive(Default)]
struct Watch {
    instance: String,
    package: String,
    visible: bool,
    seen: bool,
    absent: Option<Instant>,
    closed: Option<Instant>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reason {
    WindowClosed,
    AppExited,
}
impl Watch {
    fn observe(
        &mut self,
        s: &Sample,
        now: Instant,
        automatic: bool,
        close: bool,
    ) -> Option<Reason> {
        if self.instance != s.instance || self.package != s.package {
            *self = Self {
                instance: s.instance.clone(),
                package: s.package.clone(),
                ..Self::default()
            };
        }
        if s.window == Some(true) {
            self.visible = true;
            self.closed = None;
        } else if s.window == Some(false) && self.visible {
            self.closed.get_or_insert(now);
        } else if s.window.is_none() {
            self.closed = None;
        }
        if s.alive.contains(&s.package) {
            self.seen = true;
            self.absent = None;
        } else if self.seen && automatic {
            self.absent.get_or_insert(now);
        } else {
            self.absent = None;
        }
        // A full-display window may represent an old shared context. Protect its other apps.
        if s.alive.iter().any(|p| p != &s.package) {
            self.absent = None;
            return None;
        }
        if close
            && self
                .closed
                .is_some_and(|t| now.saturating_duration_since(t) >= Duration::from_secs(1))
        {
            return Some(Reason::WindowClosed);
        }
        if automatic
            && self
                .absent
                .is_some_and(|t| now.saturating_duration_since(t) >= Duration::from_secs(15))
        {
            return Some(Reason::AppExited);
        }
        None
    }
    fn pause(&mut self) {
        self.absent = None;
        self.closed = None;
    }
}
const PROBE: &str = r#"
printf 'FRAMELY_ACTIVE='; getprop lepton.active_app_id
printf 'FRAMELY_WINDOW='; getprop waydroid.active_apps
printf 'FRAMELY_PROCESSES\n'
dumpsys activity processes || exit 1
printf '\nFRAMELY_LIFECYCLE_DONE\n'
"#;
fn short(args: &[&str]) -> Result<String> {
    let mut cmd = crate::process::tool("podman");
    cmd.args(args);
    output(cmd, Duration::from_secs(3), None)
}
fn instance(c: &Container) -> Result<String> {
    let text = short(&[
        "inspect",
        "--format",
        "{{.Id}}|{{.State.StartedAt}}|{{.State.Running}}",
        &format!("lepton-{}", c.name),
    ])?;
    let text = text.trim();
    ensure!(
        text.ends_with("|true") && text.split('|').count() == 3,
        "Container unavailable"
    );
    Ok(text.into())
}
fn packages(c: &Container) -> Result<BTreeSet<String>> {
    let xml = fs::read_to_string(c.baked.join("data_overlay/system/packages.xml"))?;
    ensure!(
        xml.len() <= 16 * 1024 * 1024 && xml.contains("<packages"),
        "Unknown package database"
    );
    let mut out = BTreeSet::new();
    for tag in xml.split('<').filter(|t| t.starts_with("package ")) {
        if let (Some(p), Some(path)) = (attr(tag, "name"), attr(tag, "codePath")) {
            if apk_metadata::valid_package(p) && path.starts_with("/data/") {
                out.insert(p.into());
            }
        }
    }
    ensure!(!out.is_empty(), "No known application packages");
    Ok(out)
}
fn sample(c: &Container) -> Result<Sample> {
    let identity = instance(c)?;
    let text = short(&[
        "exec",
        &format!("lepton-{}", c.name),
        "sh",
        "-c",
        PROBE,
        "framely-lifecycle",
    ])?;
    ensure!(
        text.ends_with("FRAMELY_LIFECYCLE_DONE\n"),
        "Incomplete lifecycle response"
    );
    let (props, processes) = text
        .split_once("FRAMELY_PROCESSES\n")
        .context("Unknown lifecycle response")?;
    ensure!(
        processes.starts_with("ACTIVITY MANAGER RUNNING PROCESSES"),
        "Unknown process response"
    );
    let package = props
        .lines()
        .find_map(|l| l.strip_prefix("FRAMELY_ACTIVE="))
        .context("Missing active package")?
        .trim()
        .to_owned();
    ensure!(
        apk_metadata::valid_package(&package),
        "Unknown active package"
    );
    let window = match props
        .lines()
        .find_map(|l| l.strip_prefix("FRAMELY_WINDOW="))
    {
        Some("Waydroid") => Some(true),
        Some("none") => Some(false),
        _ => None,
    };
    let packages = packages(c)?;
    ensure!(
        packages.contains(&package),
        "Active package missing from package database"
    );
    let alive = packages
        .into_iter()
        .filter(|p| gamepad_present(processes, p))
        .collect();
    // A restart during the query must not inherit the old absence/close observation.
    ensure!(
        instance(c)? == identity,
        "Container changed while monitoring"
    );
    Ok(Sample {
        instance: identity,
        package,
        window,
        alive,
    })
}
fn candidates(home: &Path, db: &Database) -> Vec<Container> {
    let contexts: BTreeSet<_> = db
        .records
        .values()
        .filter(|r| !r.removed)
        .map(|r| r.context.clone())
        .collect();
    let base = home.join(".local/share/lepton/contexts");
    let mut out = Vec::new();
    for id in contexts {
        if !safe(&id) || id.starts_with("steamlaunch-") {
            continue;
        }
        let mut name = id.clone();
        let mut baked = base.join(&id).join("baked");
        if id.starts_with("external-") {
            let Some(path) = db
                .roots
                .iter()
                .find(|p| format!("external-{}", hash(&p.to_string_lossy())) == id)
            else {
                continue;
            };
            baked = path.clone();
            name = db
                .root_contexts
                .get(path)
                .cloned()
                .or_else(|| path.parent()?.file_name()?.to_str().map(str::to_owned))
                .unwrap_or_default();
        }
        if !safe(&name) || name.starts_with("steamlaunch-") {
            continue;
        }
        let Ok(baked) = validate_baked(&baked) else {
            continue;
        };
        out.push(Container {
            id,
            managed: db.owned_contexts.contains(&name),
            name,
            baked,
            running: true,
            steam: false,
        });
    }
    let mut counts = BTreeMap::new();
    for c in &out {
        *counts.entry(c.name.clone()).or_insert(0) += 1;
    }
    out.retain(|c| counts[&c.name] == 1);
    out
}
struct LaunchMark {
    instance: String,
    package: String,
    flat: bool,
}
static LAUNCHES: Mutex<BTreeMap<String, LaunchMark>> = Mutex::new(BTreeMap::new());
pub(super) fn launched(c: &Container, a: &App) {
    if let Ok(instance) = instance(c) {
        LAUNCHES.lock().unwrap().insert(
            c.name.clone(),
            LaunchMark {
                instance,
                package: a.metadata.package.clone(),
                flat: a.show_window.unwrap_or(!a.metadata.vr),
            },
        );
    }
}
#[derive(Default)]
pub(super) struct Monitor {
    watches: BTreeMap<String, Watch>,
}
impl Monitor {
    pub(super) fn tick(&mut self, home: &Path, now: Instant) -> Result<()> {
        if MUTATION.try_lock().is_err() {
            for w in self.watches.values_mut() {
                w.pause();
            }
            return Ok(());
        }
        let db = load(home)?;
        if !db.auto_stop_container && !db.stop_container_on_close {
            self.watches.clear();
            return Ok(());
        }
        let mut cs = candidates(home, &db);
        let known: BTreeSet<_> = cs.iter().map(|c| c.name.clone()).collect();
        LAUNCHES.lock().unwrap().retain(|n, _| known.contains(n));
        let Ok(running) = short(&["ps", "--format", "{{.Names}}|{{.Pid}}"]) else {
            self.watches.clear();
            return Ok(());
        };
        let running: BTreeSet<_> = running
            .lines()
            .filter_map(|row| {
                let (name, pid) = row.split_once('|')?;
                runtime_pid_alive(pid)
                    .then(|| name.strip_prefix("lepton-"))
                    .flatten()
            })
            .collect();
        cs.retain(|c| running.contains(c.name.as_str()));
        let names: BTreeSet<_> = cs.iter().map(|c| c.name.clone()).collect();
        self.watches.retain(|n, _| names.contains(n));
        for c in cs {
            if db
                .records
                .values()
                .any(|r| r.context == c.id && r.pending.is_some())
            {
                self.watches.remove(&c.name);
                continue;
            }
            let Ok(s) = sample(&c) else {
                self.watches.remove(&c.name);
                continue;
            };
            if !db
                .records
                .values()
                .any(|r| r.context == c.id && !r.removed && r.metadata.package == s.package)
            {
                self.watches.remove(&c.name);
                continue;
            }
            let w = self.watches.entry(c.name.clone()).or_default();
            if let Some(mark) = LAUNCHES.lock().unwrap().remove(&c.name) {
                if mark.instance == s.instance && mark.package == s.package {
                    *w = Watch {
                        instance: mark.instance,
                        package: mark.package,
                        visible: mark.flat,
                        seen: true,
                        ..Watch::default()
                    };
                }
            }
            let Some(reason) =
                w.observe(&s, now, db.auto_stop_container, db.stop_container_on_close)
            else {
                continue;
            };
            // Match mutation exclusion in operate(), including other Framely CLI processes.
            let Ok(_guard) = MUTATION.try_lock() else {
                w.pause();
                continue;
            };
            let lock = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(root(home).join("operation.lock"))?;
            if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                w.pause();
                continue;
            }
            let latest = load(home)?;
            if (reason == Reason::AppExited && !latest.auto_stop_container)
                || (reason == Reason::WindowClosed && !latest.stop_container_on_close)
            {
                w.pause();
                continue;
            }
            if latest
                .records
                .values()
                .any(|r| r.context == c.id && r.pending.is_some())
                || !latest
                    .records
                    .values()
                    .any(|r| r.context == c.id && !r.removed && r.metadata.package == s.package)
            {
                w.pause();
                continue;
            }
            let Ok(fresh) = sample(&c) else {
                w.pause();
                continue;
            };
            if fresh.instance != s.instance
                || fresh.package != s.package
                || fresh.alive.iter().any(|p| p != &s.package)
                || (reason == Reason::WindowClosed && fresh.window != Some(false))
                || (reason == Reason::AppExited && !fresh.alive.is_empty())
            {
                w.pause();
                continue;
            }
            let log = root(home).join("logs").join(format!(
                "{}-lifecycle-{}.log",
                hash(&c.id),
                now_secs()
            ));
            fs::write(&log, format!("Stopping container: {reason:?}\n"))?;
            if let Err(e) = stop(&c, &log) {
                eprintln!("APK lifecycle stop failed for {}: {e:#}", c.name);
            }
            self.watches.remove(&c.name);
        }
        Ok(())
    }
}
fn now_secs() -> u64 {
    super::now()
}
static STARTED: OnceLock<()> = OnceLock::new();
pub(super) fn start() {
    STARTED.get_or_init(|| {
        std::thread::spawn(|| {
            let mut monitor = Monitor::default();
            loop {
                if let Ok(home) = crate::steam::home() {
                    if let Err(e) = monitor.tick(&home, Instant::now()) {
                        eprintln!("APK lifecycle monitor: {e:#}");
                    }
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(visible: Option<bool>, alive: &[&str]) -> Sample {
        Sample {
            instance: "boot1".into(),
            package: "app.one".into(),
            window: visible,
            alive: alive.iter().map(|p| p.to_string()).collect(),
        }
    }
    #[test]
    fn automatic_exit_requires_confirmed_absence_and_preserves_background_apps() {
        let mut w = Watch::default();
        let n = Instant::now();
        assert_eq!(w.observe(&s(Some(true), &["app.one"]), n, true, true), None);
        assert_eq!(
            w.observe(
                &s(Some(true), &["app.one"]),
                n + Duration::from_secs(60),
                true,
                true
            ),
            None
        );
        assert_eq!(
            w.observe(&s(Some(true), &[]), n + Duration::from_secs(61), true, true),
            None
        );
        assert_eq!(
            w.observe(&s(Some(true), &[]), n + Duration::from_secs(75), true, true),
            None
        );
        assert_eq!(
            w.observe(&s(Some(true), &[]), n + Duration::from_secs(76), true, true),
            Some(Reason::AppExited)
        );
        w.pause();
        assert_eq!(
            w.observe(&s(Some(true), &[]), n + Duration::from_secs(90), true, true),
            None
        );
    }
    #[test]
    fn native_window_close_is_independent_of_auto_exit_and_headless_windows_do_not_close() {
        let n = Instant::now();
        let mut w = Watch::default();
        assert_eq!(
            w.observe(&s(Some(false), &["app.one"]), n, false, true),
            None
        );
        assert_eq!(
            w.observe(
                &s(Some(false), &["app.one"]),
                n + Duration::from_secs(60),
                false,
                true
            ),
            None
        );
        w.observe(&s(Some(true), &["app.one"]), n, false, true);
        w.observe(&s(Some(false), &["app.one"]), n, false, true);
        assert_eq!(
            w.observe(
                &s(Some(false), &["app.one"]),
                n + Duration::from_secs(1),
                false,
                true
            ),
            Some(Reason::WindowClosed)
        );
        assert_eq!(
            w.observe(
                &s(Some(false), &["app.one"]),
                n + Duration::from_secs(2),
                false,
                false
            ),
            None
        );
    }
    #[test]
    fn restart_shared_apps_and_unknown_queries_cancel_stop() {
        let n = Instant::now();
        let mut w = Watch::default();
        w.observe(&s(Some(true), &["app.one"]), n, true, true);
        w.observe(&s(Some(false), &[]), n, true, true);
        assert_eq!(
            w.observe(
                &s(Some(false), &["app.two"]),
                n + Duration::from_secs(20),
                true,
                true
            ),
            None
        );
        w.observe(&s(None, &[]), n + Duration::from_secs(21), true, true);
        assert_eq!(
            w.observe(
                &s(Some(false), &["app.one"]),
                n + Duration::from_secs(22),
                true,
                true
            ),
            None
        );
        let mut restarted = s(Some(false), &[]);
        restarted.instance = "boot2".into();
        assert_eq!(
            w.observe(&restarted, n + Duration::from_secs(40), true, true),
            None
        );
    }
}
