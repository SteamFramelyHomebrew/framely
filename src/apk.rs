//! Session-owned Lepton APK management. Persistent files outlive Framely uninstall.
use crate::{
    apk_metadata::{self, Metadata},
    jobs::Cancellation,
};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, PermissionsExt},
        io::AsRawFd,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
mod cleanup;
mod lifecycle;
mod native;
mod steam_bridge;
mod steam_shortcuts;
pub(crate) mod steam_ui;
mod storage;
pub fn steam_session_started() -> Result<()> {
    steam_bridge::start_session()?;
    steam_shortcuts::start_auto_registration();
    Ok(())
}
pub fn native_steam(app: &str, token: &str, command: &[String]) -> Result<()> {
    native::run(app, token, command)
}
pub fn steam_wrapper(app: &str, token: &str) -> Result<()> {
    steam_bridge::run(app, token)
}
pub fn lifecycle_started() {
    lifecycle::start();
}
static MUTATION: Mutex<()> = Mutex::new(());
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    #[serde(default)]
    steam_launch: bool,
    #[serde(default)]
    steam_preference: Option<bool>,
    #[serde(default)]
    steam_registration_error: Option<String>,
    #[serde(default)]
    steam_transient_generation: Option<String>,
    #[serde(default)]
    steam_transient_expires: u64,
    #[serde(default)]
    steam_app_id: Option<u32>,
    #[serde(default)]
    steam_token: Option<String>,
    #[serde(default)]
    steam_binding: Option<steam_shortcuts::Binding>,
    id: String,
    context: String,
    metadata: Metadata,
    removed: bool,
    #[serde(default)]
    launcher_seen: bool,
    #[serde(default)]
    pending: Option<String>,
    #[serde(default)]
    activity: Option<String>,
    #[serde(default)]
    show_window: Option<bool>,
    #[serde(default)]
    orientation: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Database {
    #[serde(default = "close_container_default")]
    retain_steam_entries: bool,
    #[serde(default)]
    auto_stop_container: bool,
    #[serde(default = "close_container_default")]
    stop_container_on_close: bool,
    #[serde(default)]
    gamepad_enabled: bool,
    #[serde(default = "gamepad_source_default")]
    gamepad_source: String,
    #[serde(default = "close_container_default")]
    gamepad_rumble: bool,
    #[serde(default)]
    owned_contexts: Vec<String>,
    #[serde(default)]
    root_contexts: BTreeMap<PathBuf, String>,
    #[serde(default)]
    roots: Vec<PathBuf>,
    #[serde(default)]
    records: BTreeMap<String, Record>,
}
impl Default for Database {
    fn default() -> Self {
        Self {
            retain_steam_entries: true,
            auto_stop_container: false,
            stop_container_on_close: true,
            gamepad_enabled: false,
            gamepad_source: gamepad_source_default(),
            gamepad_rumble: true,
            owned_contexts: Vec::new(),
            root_contexts: BTreeMap::new(),
            roots: Vec::new(),
            records: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Container {
    id: String,
    name: String,
    baked: PathBuf,
    running: bool,
    managed: bool,
    steam: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct App {
    state_known: bool,
    id: String,
    context: String,
    metadata: Metadata,
    installed: bool,
    running: bool,
    managed: bool,
    steam: bool,
    size: u64,
    pending: Option<String>,
    activity: Option<String>,
    show_window: Option<bool>,
    orientation: Option<String>,
}
fn gamepad_source_default() -> String {
    "steam".into()
}
fn close_container_default() -> bool {
    true
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn hash(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))[..24].into()
}
fn safe(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 200
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        && s != "."
        && s != ".."
}
fn root(home: &Path) -> PathBuf {
    home.join(".local/share/framely/apk-manager")
}
fn init(home: &Path) -> Result<PathBuf> {
    let r = root(home);
    for p in [
        home.join(".local"),
        home.join(".local/share"),
        home.join(".local/share/framely"),
        r.clone(),
    ] {
        ensure!(
            !fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()),
            "APK storage is a symlink"
        );
        fs::create_dir_all(&p)?;
    }
    fs::set_permissions(&r, fs::Permissions::from_mode(0o700))?;
    for p in ["reviews", "apks", "backups", "logs", "uploads"] {
        let d = r.join(p);
        ensure!(
            !fs::symlink_metadata(&d).is_ok_and(|m| m.file_type().is_symlink()),
            "APK storage is a symlink"
        );
        fs::create_dir_all(&d)?;
    }
    Ok(r)
}
fn load(home: &Path) -> Result<Database> {
    let p = root(home).join("state.json");
    if !p.exists() {
        return Ok(Database::default());
    }
    ensure!(
        !fs::symlink_metadata(&p)?.file_type().is_symlink(),
        "Invalid APK state file"
    );
    let b = fs::read(p)?;
    ensure!(b.len() < 32 * 1024 * 1024, "APK state is too large");
    Ok(serde_json::from_slice(&b)?)
}
fn save(home: &Path, db: &Database) -> Result<()> {
    let r = init(home)?;
    let state = r.join("state.json");
    let bytes = serde_json::to_vec(db)?;
    // Inventory refresh and Steam registration can rediscover unchanged state.
    // Keep those reads from forcing another file and directory journal commit.
    if fs::symlink_metadata(&state).is_ok_and(|m| m.file_type().is_file())
        && fs::read(&state)? == bytes
    {
        return Ok(());
    }
    let tmp = r.join(format!(
        "state-{}.tmp",
        hex::encode(rand::random::<[u8; 12]>())
    ));
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(&bytes)?;
    f.sync_all()?;
    fs::rename(tmp, state)?;
    fs::File::open(r)?.sync_all()?;
    Ok(())
}
use std::os::unix::fs::OpenOptionsExt;
fn runner(home: &Path) -> Result<PathBuf> {
    for library in crate::steam::library_roots(home) {
        let p = library.join("steamapps/common/Lepton/lepton");
        if p.is_file() {
            return Ok(p);
        }
    }
    bail!("Lepton is not installed; install it through Steam first")
}
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    tag.split_once(&format!("{name}=\""))?.1.split('"').next()
}
fn output(mut cmd: Command, timeout: Duration, log: Option<&Path>) -> Result<String> {
    let path = std::env::temp_dir().join(format!(
        "framely-apk-command-{}",
        hex::encode(rand::random::<[u8; 16]>())
    ));
    let f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    cmd.stdin(Stdio::null()).stdout(f.try_clone()?).stderr(f);
    let mut child = cmd.spawn()?;
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break Some(s);
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    let mut b = String::new();
    fs::File::open(&path)?
        .take(4 * 1024 * 1024)
        .read_to_string(&mut b)
        .ok();
    let _ = fs::remove_file(path);
    if let Some(log) = log {
        let mut f = fs::OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(log)?;
        writeln!(f, "{}\n{}", now(), b)?;
    }
    ensure!(
        status.is_some(),
        "Command timed out; check the installed state before retrying"
    );
    ensure!(status.unwrap().success(), "{}", b.trim());
    Ok(b)
}
fn podman(args: &[&str], log: Option<&Path>) -> Result<String> {
    let mut cmd = crate::process::tool("podman");
    cmd.args(args);
    output(cmd, Duration::from_secs(180), log)
}
fn running(name: &str) -> bool {
    podman(
        &[
            "inspect",
            "--format",
            "{{.State.Running}}|{{.State.Pid}}",
            &format!("lepton-{name}"),
        ],
        None,
    )
    .is_ok_and(|s| {
        s.trim()
            .split_once('|')
            .is_some_and(|(state, pid)| state == "true" && runtime_pid_alive(pid))
    })
}
// Podman can retain Running=true after both conmon and the OCI init process die.
// Inspect the host PID as well, without executing anything inside a dead container.
fn runtime_pid_alive(value: &str) -> bool {
    let Ok(pid) = value.parse::<u32>() else {
        return false;
    };
    if pid == 0 {
        return false;
    }
    let proc = PathBuf::from(format!("/proc/{pid}"));
    if !proc.is_dir() {
        return false;
    }
    // A zombie has exited even while its /proc entry remains. An unreadable stat
    // alone is not evidence of exit: retain the live directory conservatively.
    !fs::read_to_string(proc.join("stat")).is_ok_and(|s| {
        s.rsplit_once(')')
            .is_some_and(|(_, tail)| matches!(tail.split_whitespace().next(), Some("Z" | "X")))
    })
}
fn validate_baked(p: &Path) -> Result<PathBuf> {
    let p = fs::canonicalize(p)?;
    ensure!(
        p.file_name().is_some_and(|s| s == "baked"),
        "Select a Lepton baked data directory"
    );
    ensure!(
        fs::metadata(&p)?.uid() == unsafe { libc::geteuid() },
        "Lepton data belongs to another user"
    );
    ensure!(
        p.join("data_overlay/system/packages.xml").is_file(),
        "No Android package database in this directory"
    );
    Ok(p)
}
fn containers(home: &Path, db: &Database) -> Vec<Container> {
    let mut out = Vec::new();
    let base = home.join(".local/share/lepton/contexts");
    if let Ok(entries) = fs::read_dir(&base) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !safe(&name) {
                continue;
            }
            let baked = e.path().join("baked");
            if let Ok(baked) = validate_baked(&baked) {
                out.push(Container {
                    id: name.clone(),
                    managed: db.owned_contexts.contains(&name),
                    steam: name.starts_with("steamlaunch-"),
                    running: false,
                    name,
                    baked,
                });
            }
        }
    }
    for id in &db.owned_contexts {
        if safe(id) && !out.iter().any(|c| c.id == *id) {
            let baked = base.join(id).join("baked");
            if baked.parent().is_some_and(|p| p.is_dir())
                && !fs::symlink_metadata(&baked).is_ok_and(|m| m.file_type().is_symlink())
            {
                out.push(Container {
                    id: id.clone(),
                    name: id.clone(),
                    baked,
                    running: false,
                    managed: true,
                    steam: false,
                });
            }
        }
    }
    for baked in &db.roots {
        if let Ok(baked) = validate_baked(baked) {
            if out.iter().any(|c| c.baked == baked) {
                continue;
            }
            let inferred = baked
                .parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let name = db.root_contexts.get(&baked).cloned().unwrap_or(inferred);
            if safe(&name) {
                out.push(Container {
                    id: format!("external-{}", hash(&baked.to_string_lossy())),
                    name,
                    baked,
                    running: false,
                    managed: false,
                    steam: false,
                });
            }
        }
    }
    // Podman labels locate Steam/custom compatdata directories even outside the default root.
    if let Ok(s) = podman(&["ps", "--format", "{{.Names}}|{{.Pid}}"], None) {
        for n in s.lines().filter_map(|s| {
            let (name, pid) = s.split_once('|')?;
            runtime_pid_alive(pid)
                .then(|| name.strip_prefix("lepton-"))
                .flatten()
        }) {
            if !safe(n) {
                continue;
            }
            if let Some(c) = out.iter_mut().find(|c| c.name == n) {
                c.running = true;
                continue;
            }
            if let Ok(p) = podman(
                &[
                    "inspect",
                    "--format",
                    "{{ index .Config.Labels \"STEAM_COMPAT_DATA_PATH\" }}",
                    &format!("lepton-{n}"),
                ],
                None,
            ) {
                if let Ok(baked) = validate_baked(&Path::new(p.trim()).join("baked")) {
                    if let Some(c) = out.iter_mut().find(|c| c.baked == baked) {
                        c.name = n.into();
                        c.steam |= n.starts_with("steamlaunch-")
                            && !db.records.values().any(|r| {
                                r.context == c.id && native::runtime_name(r).as_deref() == Some(n)
                            });
                        c.running = true;
                    } else {
                        out.push(Container {
                            id: format!("external-{}", hash(&baked.to_string_lossy())),
                            name: n.into(),
                            baked,
                            running: true,
                            managed: false,
                            steam: n.starts_with("steamlaunch-"),
                        });
                    }
                }
            }
        }
    } else {
        // Preserve the old per-container fallback when the bulk query fails.
        for c in &mut out {
            c.running = running(&c.name);
        }
    }
    out
}
fn remove_tree(p: &Path) -> Result<()> {
    let m = fs::symlink_metadata(p)?;
    if m.is_dir() && !m.file_type().is_symlink() {
        ensure!(
            m.uid() == unsafe { libc::geteuid() },
            "Directory belongs to another user"
        );
        fs::set_permissions(p, fs::Permissions::from_mode(0o700))?;
        for e in fs::read_dir(p)? {
            remove_tree(&e?.path())?;
        }
        fs::remove_dir(p)?;
    } else {
        fs::remove_file(p)?;
    }
    Ok(())
}
fn size(p: &Path) -> u64 {
    if let Ok(m) = fs::symlink_metadata(p) {
        if m.file_type().is_symlink() {
            return 0;
        }
        if m.is_file() {
            return m.len();
        }
        if let Ok(es) = fs::read_dir(p) {
            return es.flatten().map(|e| size(&e.path())).sum();
        }
    }
    0
}
fn cached_metadata(path: &Path) -> Result<Metadata> {
    type Cache = BTreeMap<PathBuf, (u64, u64, u64, Metadata)>;
    static CACHE: std::sync::OnceLock<Mutex<Cache>> = std::sync::OnceLock::new();
    let stat = fs::metadata(path)?;
    let stamp = (stat.len(), stat.mtime() as u64, stat.ino());
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some((size, time, inode, meta)) = cache.lock().unwrap().get(path) {
        if (*size, *time, *inode) == stamp {
            return Ok(meta.clone());
        }
    }
    let meta = apk_metadata::read(path)?;
    let mut cache = cache.lock().unwrap();
    if cache.len() > 512 {
        cache.clear()
    }
    cache.insert(path.to_owned(), (stamp.0, stamp.1, stamp.2, meta.clone()));
    Ok(meta)
}
fn restriction(c: &Container, package: &str) -> Option<(bool, bool, Vec<String>)> {
    let p = c
        .baked
        .join("data_overlay/system/users/0/package-restrictions.xml");
    let bytes = fs::read(p).ok()?;
    if bytes.len() > 16 * 1024 * 1024 {
        return None;
    }
    let xml = String::from_utf8(bytes).ok()?;
    for rest in xml.split("<pkg ").skip(1) {
        let tag = rest.split('>').next()?;
        if attr(tag, "name") != Some(package) {
            continue;
        }
        let installed = attr(tag, "installed") != Some("false");
        let enabled = !matches!(attr(tag, "enabled"), Some("2" | "3" | "4"));
        let body = rest.split("</pkg>").next().unwrap_or(rest);
        let disabled = body
            .split_once("<disabled-components>")
            .map(|(_, b)| {
                b.split("</disabled-components>")
                    .next()
                    .unwrap_or("")
                    .split('<')
                    .filter(|t| t.starts_with("item "))
                    .filter_map(|t| attr(t, "name").map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        return Some((installed, enabled, disabled));
    }
    None
}
fn installed(home: &Path, c: &Container) -> Result<Vec<(Metadata, PathBuf)>> {
    let p = c.baked.join("data_overlay/system/packages.xml");
    let bytes = fs::read(&p)?;
    ensure!(
        bytes.len() <= 16 * 1024 * 1024,
        "Package database is too large"
    );
    let xml = String::from_utf8(bytes)?;
    let live_packages = if c.running {
        let text = podman(
            &[
                "exec",
                &format!("lepton-{}", c.name),
                "pm",
                "list",
                "packages",
                "--user",
                "0",
            ],
            None,
        )?;
        ensure!(
            !text.contains("Error:") && !text.contains("Exception"),
            "Cannot read live Android package list"
        );
        Some(
            text.lines()
                .filter_map(|s| s.trim().strip_prefix("package:").map(str::to_owned))
                .collect::<std::collections::BTreeSet<_>>(),
        )
    } else {
        None
    };
    let mut out = Vec::new();
    for tag in xml.split('<').filter(|t| t.starts_with("package ")) {
        let Some(package) = attr(tag, "name").filter(|p| apk_metadata::valid_package(p)) else {
            continue;
        };
        if live_packages
            .as_ref()
            .is_some_and(|packages| !packages.contains(package))
        {
            continue;
        }
        if live_packages.is_none() && restriction(c, package).is_some_and(|r| !r.0) {
            continue;
        }
        let Some(code) = attr(tag, "codePath").and_then(|p| p.strip_prefix("/data/")) else {
            continue;
        };
        if code.split('/').any(|p| p == ".." || p.is_empty()) {
            continue;
        }
        let direct = c.baked.join("data_overlay").join(code).join("base.apk");
        let candidates = [
            direct.clone(),
            c.baked.join("app_overlay/base.apk"),
            c.baked.join("app_lowerdir/base.apk"),
        ];
        let apk = candidates
            .into_iter()
            .find(|p| {
                p.is_file()
                    && fs::canonicalize(p).is_ok_and(|p| p.starts_with(&c.baked))
                    && cached_metadata(p).is_ok_and(|m| m.package == package)
            })
            .or_else(|| {
                let cached = root(home)
                    .join("apks")
                    .join(hash(&format!("{}/{}", c.id, package)))
                    .join("base.apk");
                let canonical = fs::canonicalize(&cached).ok()?;
                let cache_root = fs::canonicalize(root(home).join("apks")).ok()?;
                (canonical.starts_with(&cache_root)
                    && cached_metadata(&cached).is_ok_and(|m| {
                        m.package == package
                            && Some(m.version_code)
                                == attr(tag, "version").and_then(|v| v.parse().ok())
                    }))
                .then_some(cached)
            })
            .unwrap_or(direct);
        if apk.exists() {
            ensure!(
                fs::canonicalize(&apk)?.starts_with(&c.baked)
                    || apk.starts_with(root(home).join("apks")),
                "APK path escapes container"
            );
        }
        let mut meta = cached_metadata(&apk).unwrap_or_else(|_| Metadata {
            package: package.into(),
            name: package.into(),
            version: attr(tag, "version").unwrap_or("?").into(),
            version_code: attr(tag, "version").unwrap_or("0").parse().unwrap_or(0),
            ..Default::default()
        });
        meta.package = package.into();
        if let Some((_, enabled, disabled)) = restriction(c, package) {
            if !enabled {
                meta.activities.clear();
                meta.declared_activities.clear();
            } else {
                meta.activities.retain(|a| !disabled.contains(a));
                meta.declared_activities.retain(|a| !disabled.contains(a));
            }
        }
        if c.running && restriction(c, package).is_none_or(|r| r.1) {
            if let Ok(s) = podman(
                &[
                    "exec",
                    &format!("lepton-{}", c.name),
                    "cmd",
                    "package",
                    "resolve-activity",
                    "--brief",
                    "-a",
                    "android.intent.action.MAIN",
                    "-c",
                    "android.intent.category.LAUNCHER",
                    "-p",
                    package,
                ],
                None,
            ) {
                if let Some(a) = s
                    .lines()
                    .find_map(|l| l.trim().strip_prefix(&format!("{package}/")))
                {
                    let a = if a.starts_with('.') {
                        format!("{package}{a}")
                    } else {
                        a.into()
                    };
                    if meta.declared_activities.contains(&a)
                        && !meta.activities.contains(&a)
                        && restriction(c, package).is_none_or(|r| !r.2.contains(&a))
                    {
                        meta.activities.insert(0, a);
                    }
                }
            }
        }
        out.push((meta, apk));
    }
    Ok(out)
}
fn apps(home: &Path, db: &Database, cs: &[Container], measure: bool) -> (Vec<App>, Vec<String>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let mut failed_contexts = std::collections::BTreeSet::new();
    for c in cs {
        match installed(home, c) {
            Ok(packages) => {
                for (mut metadata, apk) in packages {
                    let id = format!("{}/{}", c.id, metadata.package);
                    let rec = db.records.get(&id);
                    if let Some(r) = rec {
                        if metadata.name == metadata.package {
                            metadata.name = r.metadata.name.clone()
                        }
                        if metadata.icon.is_none() {
                            metadata.icon = r.metadata.icon.clone();
                        }
                    }
                    let data = c.baked.join("data_overlay");
                    let n = if measure {
                        size(apk.parent().unwrap())
                            + size(&data.join("data").join(&metadata.package))
                            + size(&data.join("user/0").join(&metadata.package))
                            + size(&data.join("user_de/0").join(&metadata.package))
                            + size(&data.join("media/0/Android/data").join(&metadata.package))
                            + size(&data.join("media/0/Android/obb").join(&metadata.package))
                    } else {
                        0
                    };
                    let activity = rec
                        .and_then(|r| r.activity.clone())
                        .filter(|a| metadata.declared_activities.contains(a));
                    out.push(App {
                        state_known: true,
                        id,
                        context: c.id.clone(),
                        metadata,
                        installed: true,
                        running: c.running,
                        managed: c.managed,
                        steam: c.steam,
                        size: n,
                        pending: rec.and_then(|r| r.pending.clone()),
                        activity,
                        show_window: rec.and_then(|r| r.show_window),
                        orientation: rec.and_then(|r| r.orientation.clone()),
                    });
                }
            }
            Err(e) => {
                failed_contexts.insert(c.id.clone());
                warnings.push(format!("{}: {e:#}", c.name));
            }
        }
    }
    for r in db.records.values() {
        if !out.iter().any(|a| a.id == r.id) {
            let available = cs.iter().any(|c| c.id == r.context);
            let state_known = available
                && !failed_contexts.contains(&r.context)
                && (r.removed
                    || cs.iter().any(|c| {
                        c.id == r.context
                            && restriction(c, &r.metadata.package).is_some_and(|r| !r.0)
                    }));
            out.push(App {
                state_known,
                id: r.id.clone(),
                context: r.context.clone(),
                metadata: r.metadata.clone(),
                installed: !state_known && !r.removed,
                running: false,
                managed: cs.iter().any(|c| c.id == r.context && c.managed),
                steam: cs.iter().any(|c| c.id == r.context && c.steam),
                size: cs
                    .iter()
                    .find(|c| c.id == r.context)
                    .map(|c| {
                        if !measure {
                            return 0;
                        }
                        let d = c.baked.join("data_overlay");
                        size(&d.join("data").join(&r.metadata.package))
                            + size(&d.join("user/0").join(&r.metadata.package))
                            + size(&d.join("user_de/0").join(&r.metadata.package))
                    })
                    .unwrap_or(0),
                pending: r.pending.clone().or_else(|| {
                    if !available {
                        Some("Container unavailable; data has not been deleted".into())
                    } else if !state_known {
                        Some("APK could not be read; reconcile before deleting data".into())
                    } else {
                        None
                    }
                }),
                activity: r.activity.clone(),
                show_window: r.show_window,
                orientation: r.orientation.clone(),
            });
        }
    }
    for c in cs {
        if let Ok(xml) = fs::read_to_string(c.baked.join("data_overlay/system/packages.xml")) {
            if xml.len() > 16 * 1024 * 1024 {
                continue;
            }
            for tag in xml.split('<').filter(|t| t.starts_with("package ")) {
                let Some(package) = attr(tag, "name").filter(|s| apk_metadata::valid_package(s))
                else {
                    continue;
                };
                if !attr(tag, "codePath").is_some_and(|s| s.starts_with("/data/"))
                    || !restriction(c, package).is_some_and(|r| !r.0)
                {
                    continue;
                }
                let id = format!("{}/{}", c.id, package);
                if out.iter().any(|a| a.id == id) {
                    continue;
                }
                out.push(App {
                    state_known: true,
                    id,
                    context: c.id.clone(),
                    metadata: Metadata {
                        package: package.into(),
                        name: package.into(),
                        version: attr(tag, "version").unwrap_or("?").into(),
                        version_code: attr(tag, "version")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0),
                        ..Default::default()
                    },
                    installed: false,
                    running: c.running,
                    managed: c.managed,
                    steam: c.steam,
                    size: size(&c.baked.join("data_overlay/data").join(package)),
                    pending: None,
                    activity: None,
                    show_window: None,
                    orientation: None,
                });
            }
        }
    }
    let _ = home;
    (out, warnings)
}
fn expire_reviews(home: &Path) {
    let Ok(_guard) = MUTATION.try_lock() else {
        return;
    };
    let d = root(home).join("reviews");
    if let Ok(es) = fs::read_dir(d) {
        for e in es.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.len() != 48
                || !name.bytes().all(|b| b.is_ascii_hexdigit())
                || fs::symlink_metadata(e.path()).is_ok_and(|m| m.file_type().is_symlink())
            {
                continue;
            }
            if fs::read_to_string(e.path().join("created"))
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .is_some_and(|t| now().saturating_sub(t) >= 900)
            {
                let _ = fs::remove_dir_all(e.path());
            }
        }
    }
}
pub fn list(home: &Path) -> Result<Value> {
    expire_reviews(home);
    let db = load(home)?;
    let cs = containers(home, &db);
    let (apps, warnings) = apps(home, &db, &cs, true);
    if let Ok(_guard) = MUTATION.try_lock() {
        let r = init(home)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(r.join("operation.lock"))?;
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            let mut latest = load(home)?;
            for a in &apps {
                record_app(&mut latest, a);
            }
            save(home, &latest)?;
        }
    }
    Ok(
        json!({"apps":apps.iter().map(|a| { let mut v=serde_json::to_value(a).unwrap(); v["steamLaunch"]=json!(db.records.get(&a.id).is_none_or(steam_shortcuts::wanted)); v["steamRegistered"]=json!(db.records.get(&a.id).is_some_and(|r|r.steam_launch));v["steamRegistrationError"]=json!(db.records.get(&a.id).and_then(|r|r.steam_registration_error.as_ref())); v }).collect::<Vec<_>>(),"containers":cs.iter().map(|c| { let mut v=serde_json::to_value(c).unwrap(); v["acceptsAdditionalApps"]=json!(false); v }).collect::<Vec<_>>(),"warnings":warnings,"roots":db.roots,"available":runner(home).is_ok(),"retainSteamEntries":db.retain_steam_entries,"gamepadEnabled":db.gamepad_enabled,"gamepadSource":db.gamepad_source,"gamepadRumble":db.gamepad_rumble,"autoStopContainer":db.auto_stop_container,"stopContainerOnClose":db.stop_container_on_close}),
    )
}
pub fn launcher(home: &Path) -> Vec<Value> {
    let Ok(db) = load(home) else { return vec![] };
    let cs = containers(home, &db);
    let (apps, _) = apps(home, &db, &cs, false);
    apps.into_iter()
        .filter_map(|a| {
            let has_target = !a.metadata.activities.is_empty() || a.activity.is_some();
            let previously_visible = db.records.get(&a.id).is_some_and(|r| {
                !r.removed && (r.launcher_seen || !r.metadata.activities.is_empty() || r.activity.is_some())
            });
            if !a.installed || a.steam || (!has_target && !previously_visible) {
                return None;
            }
            let unavailable = if !a.state_known {
                Some("Application state is temporarily unavailable. Refresh or reconcile it in APK management.")
            } else if !has_target {
                Some("No enabled launch activity is currently available. Check the application in APK management.")
            } else {
                None
            };
            Some(json!({"id":a.id,"kind":"lepton","name":a.metadata.name,"icon":a.metadata.icon,"launchUnavailable":unavailable,"steamAppId":db.records.get(&a.id).and_then(|r|r.steam_app_id)}))
        })
        .collect()
}
fn space(p: &Path, needed: u64) -> Result<()> {
    use std::ffi::CString;
    let p = CString::new(p.as_os_str().as_encoded_bytes())?;
    let mut s = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    ensure!(
        unsafe { libc::statvfs(p.as_ptr(), s.as_mut_ptr()) } == 0,
        "Cannot check free disk space"
    );
    let s = unsafe { s.assume_init() };
    let free = (s.f_bavail as u64).saturating_mul(s.f_frsize as u64);
    ensure!(
        free > needed.saturating_add(128 * 1024 * 1024),
        "Not enough disk space for APK, installation and backup"
    );
    Ok(())
}
pub fn upload_dir(home: &Path, total: u64) -> Result<PathBuf> {
    ensure!(
        total > 0 && total <= 8 * 1024 * 1024 * 1024,
        "APK upload must be between 1 byte and 8 GiB"
    );
    let r = init(home)?;
    space(&r, total * 3)?;
    Ok(r.join("uploads"))
}
pub fn inspect_local(home: &Path, path: &Path, cancel: &Cancellation) -> Result<Value> {
    cancel.check()?;
    let path = crate::file_browser::checked(home, path)?;
    ensure!(
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("apk")),
        "Select an APK file"
    );
    let metadata = fs::metadata(&path)?;
    ensure!(metadata.is_file(), "Select a regular APK file");
    ensure!(
        metadata.len() > 0 && metadata.len() <= 8 * 1024 * 1024 * 1024,
        "APK must be between 1 byte and 8 GiB"
    );
    inspect(home, &path, cancel)
}
pub fn inspect(home: &Path, staged: &Path, cancel: &Cancellation) -> Result<Value> {
    let _guard = MUTATION.lock().unwrap();
    cancel.check()?;
    let r = init(home)?;
    space(&r, fs::metadata(staged)?.len() * 3)?;
    let ticket = hex::encode(rand::random::<[u8; 24]>());
    let d = r.join("reviews").join(&ticket);
    fs::create_dir(&d)?;
    let snapshot = d.join("base.apk");
    let metadata = match (|| -> Result<Metadata> {
        fs::copy(staged, &snapshot)?;
        cancel.check()?;
        apk_metadata::read(&snapshot)
    })() {
        Ok(metadata) => metadata,
        Err(error) => {
            let _ = fs::remove_dir_all(&d);
            return Err(error);
        }
    };
    fs::write(d.join("metadata.json"), serde_json::to_vec(&metadata)?)?;
    fs::write(d.join("created"), now().to_string())?;
    fs::write(
        d.join("sha256"),
        crate::package::digest_reader(&mut fs::File::open(d.join("base.apk"))?)?,
    )?;
    fs::set_permissions(d.join("base.apk"), fs::Permissions::from_mode(0o400))?;
    if let Err(e) = cancel.check() {
        let _ = fs::remove_dir_all(d);
        return Err(e);
    }
    Ok(json!({"ticket":ticket,"metadata":metadata,"bytes":fs::metadata(snapshot)?.len()}))
}
fn review(home: &Path, ticket: &str) -> Result<(PathBuf, Metadata)> {
    ensure!(
        ticket.len() == 48 && ticket.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid APK review"
    );
    let d = root(home).join("reviews").join(ticket);
    ensure!(
        !fs::symlink_metadata(&d)?.file_type().is_symlink(),
        "Invalid review path"
    );
    let t: u64 = fs::read_to_string(d.join("created"))?.parse()?;
    ensure!(
        now().saturating_sub(t) < 900,
        "APK review expired; select the APK again"
    );
    ensure!(
        !fs::symlink_metadata(d.join("base.apk"))?
            .file_type()
            .is_symlink(),
        "Reviewed APK is a symlink"
    );
    ensure!(
        crate::package::digest_reader(&mut fs::File::open(d.join("base.apk"))?)?
            == fs::read_to_string(d.join("sha256"))?,
        "APK changed since review"
    );
    let meta = apk_metadata::read(&d.join("base.apk"))?;
    Ok((d, meta))
}
fn restore_package_mount(home: &Path, c: &Container, log: &Path) -> Result<()> {
    // Lepton's cmd wrapper moves every installation into the same steam_app
    // mount. Migrate old registrations, and repair every cached package rather
    // than only whichever APK last occupied that mount.
    let db = load(home)?;
    for (metadata, apk) in installed(
        home,
        &Container {
            running: false,
            ..c.clone()
        },
    )? {
        if db
            .records
            .get(&format!("{}/{}", c.id, metadata.package))
            .is_some_and(|r| r.removed)
            || restriction(c, &metadata.package).is_some_and(|r| !r.0)
        {
            continue;
        }
        if !apk.is_file() {
            continue;
        }
        // The discovery source may still be a legacy mount/cache while Android
        // already has a valid independent installation. Never reinstall solely
        // because of that source path: replacing an APK also stops its process.
        if !persistent_registration(c, &metadata, log)? {
            install_package(c, &apk, log)?;
            ensure!(
                live_installed(c, &metadata.package, log)?,
                "Android package registration did not recover"
            );
        }
    }
    Ok(())
}
fn persistent_registration(c: &Container, expected: &Metadata, log: &Path) -> Result<bool> {
    let paths = podman(
        &[
            "exec",
            &format!("lepton-{}", c.name),
            "pm",
            "path",
            "--user",
            "0",
            &expected.package,
        ],
        Some(log),
    )?;
    ensure!(
        !paths.contains("Error:") && !paths.contains("Exception"),
        "Cannot verify Android APK path; no package was replaced"
    );
    let Some(path) = paths.lines().find_map(|line| {
        line.trim()
            .strip_prefix("package:")
            .filter(|p| p.ends_with("/base.apk"))
    }) else {
        return Ok(false);
    };
    let Some(relative) = path.strip_prefix("/data/app/") else {
        return Ok(false);
    };
    ensure!(
        !relative.split('/').any(|p| matches!(p, "" | "." | "..")),
        "Invalid Android APK path; no package was replaced"
    );
    let root = c.baked.join("data_overlay/app");
    let apk = match fs::canonicalize(root.join(relative)) {
        Ok(path) => path,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    ensure!(
        apk.starts_with(fs::canonicalize(&root)?) && apk.starts_with(&c.baked),
        "Android APK path escapes container; no package was replaced"
    );
    ensure!(
        fs::metadata(&apk)?.is_file(),
        "Android APK is not a regular file; no package was replaced"
    );
    let actual = cached_metadata(&apk)?;
    ensure!(
        actual.package == expected.package,
        "Android APK path points to another package; no package was replaced"
    );
    // A stale host registration must never downgrade an externally updated APK.
    Ok(actual.version_code >= expected.version_code)
}
// Adapt only the entry script in a private temporary file. Keep Lepton's original
// libraries and dev-context data handling; app mode can clear existing baked data.
fn window_runtime(home: &Path) -> Result<(PathBuf, PathBuf)> {
    let dir = root(home).join("steam/window-runtime");
    fs::create_dir_all(&dir)?;
    let dex = dir.join("framely-window.dex");
    let script = dir.join("window.sh");
    steam_shortcuts::write_owned(
        &dex,
        include_bytes!("../native/android/framely-window.dex"),
        0o600,
    )?;
    steam_shortcuts::write_owned(
        &script,
        include_bytes!("../native/android/window.sh"),
        0o600,
    )?;
    Ok((dex, script))
}
fn direct_launch_script(source: &str) -> Result<String> {
    let directory =
        "SCRIPT_DIR=$( cd -- \"$( dirname -- \"${BASH_SOURCE[0]}\" )\" &> /dev/null && pwd )";
    let include = "source \"${SCRIPT_DIR}/liblepton/liblepton.sh\"";
    ensure!(
        source.matches(directory).count() == 1 && source.matches(include).count() == 1,
        "Unsupported Lepton entry script; cannot safely configure the APK display"
    );
    let hooks = r#"
# Preserve downloaded Android resources in development contexts. Lepton's
# single-app launcher deletes media/0 before creating its external-storage link.
function framely_prepare_media() {
    if [[ -n "${FRAMELY_EXTERNAL_MEDIA_DIR:-}" ]]; then
        framely_prepare_external_media || return
        if [[ "${FRAMELY_NATIVE_LAUNCH:-false}" == true ]]; then
            rm -f -- "$(data_mount_path)/media/0" || return
        fi
        return 0
    fi
    local media="$(data_mount_path)/media/0"
    local external="${STEAM_COMPAT_DATA_PATH:-}/external"
    if [[ -d "${STEAM_COMPAT_DATA_PATH:-}" ]]; then
        if [[ -L "$media" ]]; then
            [[ "$(readlink "$media")" == "$external" ]] || { echo "Unexpected Android media link; refusing to replace it" >&2; return 64; }
            rm -f -- "$media"
        elif [[ -d "$media" ]]; then
            if [[ -z "$(find "$media" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
                rmdir -- "$media"
            elif [[ -z "$(find "$external" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
                rmdir -- "$external"
                mv -- "$media" "$external"
            else
                echo "Both Android media locations contain data; refusing to discard either" >&2
                return 64
            fi
        elif [[ -e "$media" ]]; then
            echo "Invalid Android media location" >&2; return 64
        fi
    else
        [[ ! -L "$media" ]] || { echo "Unexpected Android media link" >&2; return 64; }
        mkdir -p -- "$media"
    fi
}
for framely_mount_hook in setup_mounts setup_podman_mounts; do
    declare -F "$framely_mount_hook" >/dev/null || continue
    framely_mount_body="$(declare -f "$framely_mount_hook")"
    framely_media_remove='rm -rf "$(data_mount_path)/media/0"'
    # Refuse an unrecognized storage hook before invoking it. A future Lepton
    # syntax change must never silently bypass resource preservation.
    if [[ "$framely_mount_hook" == setup_mounts && "$framely_mount_body" == *'$(data_mount_path)/media/0'* && "$framely_mount_body" != *"$framely_media_remove"* ]]; then
        echo "Unsupported Lepton media setup; refusing to risk downloaded resources" >&2
        exit 64
    fi
    if [[ "$framely_mount_body" == *"$framely_media_remove"* ]]; then
        framely_mount_body="${framely_mount_body//"$framely_media_remove"/framely_prepare_media || return}"
        if [[ "${FRAMELY_NATIVE_LAUNCH:-false}" == true ]]; then
            framely_native_link='ln -s "${STEAM_COMPAT_DATA_PATH}/external"'
            framely_preserved_link='ln -s "${FRAMELY_EXTERNAL_MEDIA_DIR:?}"'
            framely_mount_body="${framely_mount_body//"$framely_native_link"/"$framely_preserved_link"}"
        fi
        eval "$framely_mount_body"
    fi
    unset framely_mount_body framely_media_remove
done
unset framely_mount_hook
# Bind the resource directory directly, outside Android's /data overlay.
# Adopted Steam contexts already have their own external-storage mount.
if [[ -n "${FRAMELY_SHADER_CACHE_DIR:-}${FRAMELY_EXTERNAL_MEDIA_DIR:-}" ]]; then
    declare -F setup_podman_mounts >/dev/null || { echo "Unsupported Lepton storage mounts" >&2; exit 64; }
    eval "$(declare -f setup_podman_mounts | sed '1s/setup_podman_mounts/framely_original_storage_mounts/')"
    function setup_podman_mounts() {
        framely_original_storage_mounts "$@" || return
        if [[ -n "${FRAMELY_EXTERNAL_MEDIA_DIR:-}" ]]; then
            podman_mount_entry "$FRAMELY_EXTERNAL_MEDIA_DIR" "$FRAMELY_EXTERNAL_MEDIA_DIR" rw
        fi
        if [[ -n "${FRAMELY_SHADER_CACHE_DIR:-}" ]]; then
            podman_mount_entry "$FRAMELY_SHADER_CACHE_DIR" /data/shaders rw,U
        fi
    }
fi
# Per-user readonly helper uses Android's task API, without patching its image.
if [[ -n "${FRAMELY_WINDOW_DEX:-}" ]]; then
    declare -F setup_podman_mounts >/dev/null || { echo "Unsupported Lepton window mounts" >&2; exit 64; }
    eval "$(declare -f setup_podman_mounts | sed '1s/setup_podman_mounts/framely_original_window_mounts/')"
    function setup_podman_mounts() {
        framely_original_window_mounts "$@" || return
        podman_mount_entry "$FRAMELY_WINDOW_DEX" /vendor/share/framely-window.dex ro
        podman_mount_entry "${FRAMELY_WINDOW_SCRIPT:?}" /vendor/share/framely-window.sh ro
    }
fi
# Optional virtual gamepad: mount only its event node, not host input devices.
if [[ -n "${FRAMELY_GAMEPAD_EVENT:-}" ]]; then
    for hook in setup_podman_mounts generate_zygote_launch_rc; do
        declare -F "$hook" >/dev/null || { echo "Unsupported Lepton gamepad hooks" >&2; exit 64; }
    done
    eval "$(declare -f setup_podman_mounts | sed '1s/setup_podman_mounts/framely_original_gamepad_mounts/')"
    function setup_podman_mounts() {
        framely_original_gamepad_mounts "$@"
        podman_mount_entry "${FRAMELY_GAMEPAD_EVENT:?}" /dev/input/event250 rw
        podman_mount_entry "${FRAMELY_GAMEPAD_GRAB:?}" /vendor/lib64/libframely_gamepad_grab.so ro
        if [[ -n "${FRAMELY_GAMEPAD_LAYOUT:-}" ]]; then
            podman_mount_entry "$FRAMELY_GAMEPAD_LAYOUT" /system/usr/keylayout/Vendor_0001_Product_f001.kl ro
        fi
        podman_mount_entry "${FRAMELY_GAMEPAD_READY:?}" /framely-gamepad-ready rw
    }
    eval "$(declare -f generate_zygote_launch_rc | sed '1s/generate_zygote_launch_rc/framely_original_gamepad_zygote/')"
    function generate_zygote_launch_rc() {
        framely_original_gamepad_zygote "$@"
        local rc="$(prefix)/init.zygote64.rc"
        [[ "$(grep -c '^service zygote ' "$rc")" == 1 ]] && ! grep -q 'setenv LD_PRELOAD' "$rc" || { echo "Unsupported Lepton gamepad zygote" >&2; return 64; }
        sed -i '/^service zygote /a\    setenv LD_PRELOAD /vendor/lib64/libframely_gamepad_grab.so' "$rc"
    }
fi
# Inject ownership only after Lepton has selected the existing development
# context. Setting SteamAppId before its entry script would select a new
# steamlaunch context and lose access to the user's installed application data.
if [[ -n "${FRAMELY_STEAM_APP_ID:-}" ]]; then
    [[ "$FRAMELY_STEAM_APP_ID" =~ ^[0-9]+$ ]] || { echo "Invalid Steam owner" >&2; exit 64; }
    export SteamAppId="$FRAMELY_STEAM_APP_ID"
fi
# Framely: configure display visibility and shape, retaining the dev context.
# Do not use is_app: Lepton's app bake path can reset existing application data.
eval "$(declare -f setup_props | sed '1s/setup_props/framely_original_setup_props/')"
function app_wants_flatscreen() {
    [[ "${APP_WANTS_FLATSCREEN:-true}" == true ]]
}
function setup_props() {
    framely_original_setup_props "$@"
    if [[ -n "${FRAMELY_SHADER_CACHE_DIR:-}" ]]; then
        sed -i '/^mesa\.shader\.cache\.disable=/d; /^mesa\.shader\.cache\.dir=/d' "$(props_file)"
        printf '\nmesa.shader.cache.disable=false\nmesa.shader.cache.dir=/data/shaders\n' >> "$(props_file)"
    fi
    if [[ "${FRAMELY_BACKGROUND_BOOT:-true}" == true ]]; then
        sed -i 's/^waydroid.background_start=false$/waydroid.background_start=true/' "$(props_file)"
        printf '\nwaydroid.active_apps=none\n' >> "$(props_file)"
    fi
    case "${FRAMELY_WINDOW_ORIENTATION:-auto}" in
        portrait) printf '\npersist.waydroid.width=1080\npersist.waydroid.height=1920\n' >> "$(props_file)" ;;
        landscape) printf '\npersist.waydroid.width=1920\npersist.waydroid.height=1080\n' >> "$(props_file)" ;;
    esac
    printf '\nframely.window_orientation=%s\n' "${FRAMELY_WINDOW_ORIENTATION:-auto}" >> "$(props_file)"
    printf '\nframely.gamepad.bridge=%s\n' "${FRAMELY_GAMEPAD_TOKEN:-}" >> "$(props_file)"
}
"#;
    Ok(source
        .replace(directory, "SCRIPT_DIR=\"${FRAMELY_LEPTON_DIR:?}\"")
        .replace(include, &format!("{include}\n{}\n{hooks}", storage::HOOKS)))
}
fn start(home: &Path, c: &Container, show: Option<bool>, log: &Path) -> Result<()> {
    start_container(home, c, show, true, log)
}
fn start_container(
    home: &Path,
    c: &Container,
    show: Option<bool>,
    direct: bool,
    log: &Path,
) -> Result<()> {
    start_oriented_container(home, c, show, direct, None, None, None, log)
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum AndroidReadiness {
    Full,
    PackageManager,
    RemovalWithData,
}
const ANDROID_READY_PROBE: &str = r#"
boot=$(getprop sys.boot_completed)
echo FRAMELY_BOOT=$boot
pm_ready=0
pm path android | grep -q '^package:' && pm_ready=1
echo FRAMELY_PM=$pm_ready
if [ "$1" = package ]; then
    [ "$boot" = 1 ] && [ "$pm_ready" = 1 ] && echo FRAMELY_ANDROID_READY
    exit 0
fi
user=$(am get-started-user-state 0)
echo FRAMELY_USER=$user
storage=0
if [ -d /storage/emulated/0 ] && sm list-volumes all | grep -Eq '^emulated(;0)? mounted( |$)'; then storage=1; fi
echo FRAMELY_STORAGE=$storage
[ "$boot" = 1 ] && [ "$pm_ready" = 1 ] && [ "$user" = RUNNING_UNLOCKED ] && [ "$storage" = 1 ] && echo FRAMELY_ANDROID_READY
exit 0
"#;
fn android_readiness_failure(snapshot: &str, requirement: AndroidReadiness) -> &'static str {
    if !snapshot.lines().any(|s| s.starts_with("FRAMELY_BOOT=")) {
        "Android readiness probe did not respond; export logs and retry."
    } else if !snapshot.lines().any(|s| s == "FRAMELY_BOOT=1") {
        "Android startup timed out; export logs and retry."
    } else if !snapshot.lines().any(|s| s == "FRAMELY_PM=1") {
        "Android PackageManager did not become ready; export logs and retry."
    } else if requirement != AndroidReadiness::PackageManager
        && !snapshot
            .lines()
            .any(|s| s == "FRAMELY_USER=RUNNING_UNLOCKED")
    {
        "Android user did not unlock; export logs and retry."
    } else {
        "Android external storage did not mount; export logs and retry."
    }
}
fn wait_android_ready_for(
    c: &Container,
    requirement: AndroidReadiness,
    log: Option<&Path>,
) -> Result<()> {
    wait_android_ready_until(c, requirement, log, Duration::from_secs(60))
}
fn wait_android_ready_until(
    c: &Container,
    requirement: AndroidReadiness,
    log: Option<&Path>,
    timeout: Duration,
) -> Result<()> {
    let began = Instant::now();
    let mut last = String::new();
    let mut seen_running = c.running;
    while began.elapsed() < timeout {
        let mut command = crate::process::tool("podman");
        command.args([
            "exec",
            &format!("lepton-{}", c.name),
            "sh",
            "-c",
            ANDROID_READY_PROBE,
            "framely-android-ready",
            if requirement == AndroidReadiness::PackageManager {
                "package"
            } else {
                "full"
            },
        ]);
        let snapshot = output(command, Duration::from_secs(3), log)
            .unwrap_or_else(|e| format!("Android readiness probe failed: {e}"));
        if snapshot != last {
            if let Some(log) = log {
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .mode(0o600)
                    .open(log)?;
                writeln!(file, "Android readiness ({requirement:?}):\n{snapshot}")?;
            }
            last = snapshot;
        }
        if last
            .lines()
            .any(|line| line.trim() == "FRAMELY_ANDROID_READY")
        {
            return Ok(());
        }
        let mut inspect = crate::process::tool("podman");
        inspect.args([
            "inspect",
            "--format",
            "{{.State.Running}}|{{.State.Pid}}|{{.State.Status}}|{{.State.ExitCode}}",
            &format!("lepton-{}", c.name),
        ]);
        if let Ok(state) = output(inspect, Duration::from_secs(2), None) {
            let fields: Vec<_> = state.trim().split('|').collect();
            let alive = fields.first() == Some(&"true")
                && fields.get(1).is_some_and(|pid| runtime_pid_alive(pid));
            if alive {
                seen_running = true;
            } else if seen_running || began.elapsed() >= Duration::from_secs(30) {
                if let Some(log) = log {
                    let mut file = fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .mode(0o600)
                        .open(log)?;
                    writeln!(
                        file,
                        "Container stopped during Android readiness: {}",
                        state.trim()
                    )?;
                }
                bail!("Android container stopped before becoming ready; export logs and retry.");
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    bail!(android_readiness_failure(&last, requirement))
}
fn start_oriented_container(
    home: &Path,
    c: &Container,
    show: Option<bool>,
    direct: bool,
    orientation: Option<&str>,
    gamepad: Option<&crate::gamepad::Mount>,
    steam_app_id: Option<u32>,
    log: &Path,
) -> Result<()> {
    start_oriented_container_for(
        home,
        c,
        show,
        direct,
        orientation,
        gamepad,
        steam_app_id,
        AndroidReadiness::Full,
        log,
    )
}
fn start_oriented_container_for(
    home: &Path,
    c: &Container,
    show: Option<bool>,
    direct: bool,
    orientation: Option<&str>,
    gamepad: Option<&crate::gamepad::Mount>,
    steam_app_id: Option<u32>,
    readiness: AndroidReadiness,
    log: &Path,
) -> Result<()> {
    ensure!(
        orientation.is_none_or(|s| matches!(s, "auto" | "portrait" | "landscape")),
        "Invalid window orientation"
    );
    if running(&c.name) {
        wait_android_ready_for(
            &Container {
                running: true,
                ..c.clone()
            },
            readiness,
            Some(log),
        )?;
        return if readiness == AndroidReadiness::Full {
            restore_package_mount(home, c, log)
        } else {
            Ok(())
        };
    }
    let lock_dir = root(home).join("steam");
    fs::create_dir_all(&lock_dir)?;
    let _context_lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(lock_dir.join(format!("context-{}.lock", hash(&c.id))))?;
    ensure!(
        unsafe { libc::flock(_context_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "A native Steam launch still owns this container; wait for it to finish closing"
    );
    let runner = runner(home)?;
    storage::prepare(&c.baked)?;
    // Manual container startup must use the same resource-preserving adapter.
    let temporary;
    let mut cmd = {
        let script = direct_launch_script(&fs::read_to_string(&runner)?)?;
        let path = root(home).join("logs").join(format!(
            "lepton-launch-{}.sh",
            hex::encode(rand::random::<[u8; 16]>())
        ));
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o700)
            .open(&path)?;
        file.write_all(script.as_bytes())?;
        let mut command = Command::new(&path);
        command.env(
            "FRAMELY_LEPTON_DIR",
            runner.parent().context("Missing Lepton directory")?,
        );
        temporary = Some(path);
        command
    };
    cmd.args(["start", &c.name])
        .env(
            "FRAMELY_BACKGROUND_BOOT",
            if direct { "true" } else { "false" },
        )
        .env("FRAMELY_WINDOW_ORIENTATION", orientation.unwrap_or("auto"))
        .env("LEPTON_NO_CLEANUP", "true")
        .env("TERM", "dumb")
        .env(
            "APP_WANTS_FLATSCREEN",
            if show.unwrap_or(true) {
                "true"
            } else {
                "false"
            },
        );
    let (window_dex, window_script) = window_runtime(home)?;
    cmd.env("FRAMELY_WINDOW_DEX", window_dex)
        .env("FRAMELY_WINDOW_SCRIPT", window_script);
    cmd.env_remove("SteamAppId");
    storage::configure(
        &mut cmd,
        &c.baked,
        c.id.starts_with("external-") || c.name != c.id,
    );
    if let Some(id) = steam_app_id {
        cmd.env("FRAMELY_STEAM_APP_ID", id.to_string());
    } else {
        cmd.env_remove("FRAMELY_STEAM_APP_ID");
    }
    if let Some(mount) = gamepad {
        cmd.env("FRAMELY_GAMEPAD_EVENT", &mount.event)
            .env("FRAMELY_GAMEPAD_GRAB", &mount.grab)
            .env("FRAMELY_GAMEPAD_LAYOUT", &mount.layout)
            .env("FRAMELY_GAMEPAD_READY", &mount.ready)
            .env("FRAMELY_GAMEPAD_TOKEN", &mount.token);
    }
    if c.id.starts_with("external-") || c.name != c.id {
        cmd.env(
            "STEAM_COMPAT_DATA_PATH",
            c.baked.parent().context("Missing compatdata parent")?,
        );
    }
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(log)?;
    cmd.stdin(Stdio::null())
        .stdout(file.try_clone()?)
        .stderr(file);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            if let Some(path) = temporary {
                let _ = fs::remove_file(path);
            }
            return Err(error.into());
        }
    };
    std::thread::spawn(move || {
        let _ = child.wait();
        if let Some(path) = temporary {
            let _ = fs::remove_file(path);
        }
    });
    // PackageManager can respond before the user's external storage is ready.
    // Lepton's own app launcher also waits for boot and media mounting.
    wait_android_ready_for(
        &Container {
            running: false,
            ..c.clone()
        },
        readiness,
        Some(log),
    )?;
    if readiness == AndroidReadiness::Full {
        restore_package_mount(home, c, log)
    } else {
        Ok(())
    }
}
fn wrapper_active(name: &str) -> bool {
    fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let Ok(meta) = fs::metadata(entry.path()) else {
                return false;
            };
            if meta.uid() != unsafe { libc::geteuid() } {
                return false;
            }
            let Ok(bytes) = fs::read(entry.path().join("cmdline")) else {
                return false;
            };
            let args: Vec<_> = bytes
                .split(|b| *b == 0)
                .filter_map(|b| std::str::from_utf8(b).ok())
                .collect();
            args.windows(3).any(|a| {
                (a[0].ends_with("/lepton")
                    || (a[0].contains("/framely/apk-manager/logs/lepton-launch-")
                        && a[0].ends_with(".sh")))
                    && a[1] == "start"
                    && a[2] == name
            })
        })
}
fn stop(c: &Container, log: &Path) -> Result<()> {
    crate::gamepad::stop_context(&c.name);
    if running(&c.name) {
        podman(
            &["stop", "--time", "10", &format!("lepton-{}", c.name)],
            Some(log),
        )?;
    }
    for _ in 0..200 {
        if !running(&c.name) && !wrapper_active(&c.name) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("Container did not stop")
}
fn store_apk(source: &Path, directory: &Path) -> Result<()> {
    ensure!(
        !fs::symlink_metadata(directory).is_ok_and(|m| m.file_type().is_symlink()),
        "APK cache is a symlink"
    );
    fs::create_dir_all(directory)?;
    let temporary = directory.join(format!(
        "incoming-{}",
        hex::encode(rand::random::<[u8; 12]>())
    ));
    fs::copy(source, &temporary)?;
    fs::File::open(&temporary)?.sync_all()?;
    fs::rename(temporary, directory.join("base.apk"))?;
    fs::File::open(directory)?.sync_all()?;
    Ok(())
}
fn copy(from: &Path, to: &Path, log: &Path) -> Result<()> {
    let mut cmd = crate::process::tool("cp");
    cmd.args(["-a", "--reflink=auto", "--"]).arg(from).arg(to);
    output(cmd, Duration::from_secs(900), Some(log))?;
    Ok(())
}
fn record_app(db: &mut Database, a: &App) {
    if !a.state_known {
        return;
    }
    db.records
        .entry(a.id.clone())
        .and_modify(|r| {
            if a.installed {
                r.launcher_seen |= !r.metadata.activities.is_empty()
                    || r.activity.is_some()
                    || !a.metadata.activities.is_empty()
                    || a.activity.is_some();
                r.metadata = a.metadata.clone();
                r.removed = false;
            }
        })
        .or_insert_with(|| Record {
            id: a.id.clone(),
            context: a.context.clone(),
            metadata: a.metadata.clone(),
            removed: !a.installed,
            launcher_seen: !a.metadata.activities.is_empty() || a.activity.is_some(),
            pending: None,
            activity: a.activity.clone(),
            show_window: a.show_window,
            orientation: a.orientation.clone(),
            ..Default::default()
        });
}
fn app(home: &Path, db: &Database, id: &str) -> Result<(App, Container)> {
    let cs = containers(home, db);
    let (context, _) = id.rsplit_once('/').context("Invalid APK application ID")?;
    let target: Vec<_> = cs.iter().filter(|c| c.id == context).cloned().collect();
    let (apps, _) = apps(home, db, &target, false);
    let a = apps
        .into_iter()
        .find(|a| a.id == id)
        .context("APK application not found")?;
    ensure!(!a.steam, "Manage Steam APKs through Steam");
    let mut c = cs
        .iter()
        .find(|c| c.id == a.context)
        .cloned()
        .context("Container unavailable; add its data location first")?;
    if !c.running {
        if let Some(name) = db.records.get(id).and_then(native::runtime_name) {
            c.name = name;
        }
    }
    ensure!(
        !cs.iter()
            .any(|other| other.name == c.name && other.baked != c.baked),
        "Container name is ambiguous; correct its data location first"
    );
    Ok((a, c))
}
fn discard_runtime_files(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_socket() || meta.file_type().is_fifo() {
        fs::remove_file(path)?;
    } else if meta.is_dir() && !meta.file_type().is_symlink() {
        for entry in fs::read_dir(path)? {
            discard_runtime_files(&entry?.path())?;
        }
    }
    Ok(())
}
fn snapshot_inventory(
    path: &Path,
    relative: &Path,
    out: &mut BTreeMap<String, String>,
) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    let key = relative
        .to_str()
        .context("Backup has a non-UTF8 path")?
        .to_owned();
    if meta.file_type().is_symlink() {
        out.insert(
            key,
            format!("link:{}", fs::read_link(path)?.to_string_lossy()),
        );
    } else if meta.is_dir() {
        out.insert(key, "directory".into());
        for e in fs::read_dir(path)? {
            let e = e?;
            snapshot_inventory(&e.path(), &relative.join(e.file_name()), out)?;
        }
    } else if meta.is_file() {
        let mut file = fs::File::open(path)?;
        let mut digest = Sha256::new();
        let mut bytes = [0u8; 65536];
        loop {
            let n = file.read(&mut bytes)?;
            if n == 0 {
                break;
            }
            digest.update(&bytes[..n]);
        }
        out.insert(key, format!("sha256:{}", hex::encode(digest.finalize())));
    } else if meta.file_type().is_char_device() && meta.rdev() == 0 {
        // OverlayFS whiteouts encode removals from the immutable Android base layer.
        out.insert(key, "overlay-whiteout".into());
    } else {
        bail!("Unsupported file in backup: {}", path.display());
    }
    Ok(())
}
fn recover_restores(home: &Path, db: &mut Database) -> Result<()> {
    let base = home.join(".local/share/lepton/contexts");
    let mut locations = BTreeMap::new();
    for id in &db.owned_contexts {
        ensure!(safe(id), "Invalid managed context");
        locations.insert(id.clone(), base.join(id));
    }
    if let Ok(entries) = fs::read_dir(&base) {
        for e in entries.flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            if safe(&id) && !id.starts_with("steamlaunch-") && e.file_type()?.is_dir() {
                locations.insert(id, e.path());
            }
        }
    }
    for baked in &db.roots {
        if let Some(parent) = baked.parent() {
            locations.insert(
                format!("external-{}", hash(&baked.to_string_lossy())),
                parent.to_path_buf(),
            );
        }
    }
    for (id, parent) in locations {
        let journal = parent.join("framely-restore.json");
        if !journal.exists() {
            continue;
        }
        ensure!(
            !fs::symlink_metadata(&journal)?.file_type().is_symlink(),
            "Restore journal is a symlink"
        );
        let data: Value = serde_json::from_slice(&fs::read(&journal)?)?;
        let old = data["old"]
            .as_str()
            .filter(|s| safe(s) && s.starts_with("previous-"))
            .context("Invalid restore journal")?;
        let replacement = data["replacement"]
            .as_str()
            .filter(|s| safe(s) && s.starts_with("restore-"))
            .context("Invalid restore journal")?;
        let baked = parent.join("baked");
        let old = parent.join(old);
        let replacement = parent.join(replacement);
        if baked.exists() && !replacement.exists() && old.exists() {
            let records: Vec<Record> = serde_json::from_value(data["records"].clone())?;
            ensure!(
                records
                    .iter()
                    .all(|r| r.context == id && r.id == format!("{id}/{}", r.metadata.package)),
                "Restore records have invalid identities"
            );
            db.records.retain(|_, r| r.context != id);
            for mut record in records {
                record.pending = None;
                db.records.insert(record.id.clone(), record);
            }
            save(home, db)?;
            remove_tree(&old)?;
        } else if !baked.exists() && old.exists() {
            fs::rename(&old, &baked)?;
        } else {
            ensure!(
                baked.exists(),
                "Interrupted restore needs manual recovery; no data was deleted"
            );
        }
        if replacement.exists() {
            remove_tree(&replacement)?;
        }
        fs::remove_file(journal)?;
        fs::File::open(&parent)?.sync_all()?;
    }
    Ok(())
}
fn verified_snapshot(directory: &Path) -> Result<Value> {
    ensure!(
        directory.join("complete").is_file(),
        "Backup was interrupted"
    );
    let snapshot: Value = serde_json::from_slice(&fs::read(directory.join("snapshot.json"))?)?;
    let expected: BTreeMap<String, String> = serde_json::from_value(snapshot["checksums"].clone())
        .context("Backup has no integrity manifest")?;
    let mut actual = BTreeMap::new();
    snapshot_inventory(&directory.join("baked"), Path::new(""), &mut actual)?;
    ensure!(
        expected == actual,
        "Backup integrity check failed; no application data was replaced"
    );
    Ok(snapshot)
}
fn backup(home: &Path, c: &Container, db: &Database, log: &Path) -> Result<String> {
    ensure!(!running(&c.name), "Stop the container before backup");
    space(&root(home), size(&c.baked))?;
    let id = format!("{}-{}", now(), hex::encode(rand::random::<[u8; 8]>()));
    let d = root(home).join("backups").join(&id);
    fs::create_dir(&d)?;
    let dest = d.join("baked");
    fs::create_dir(&dest)?;
    for entry in fs::read_dir(&c.baked)?.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().ends_with("_workdir") {
            continue;
        }
        copy(&entry.path(), &dest.join(name), log)?;
    }

    discard_runtime_files(&dest)?;
    let mut checksums = BTreeMap::new();
    snapshot_inventory(&dest, Path::new(""), &mut checksums)?;
    fs::write(
        d.join("snapshot.json"),
        serde_json::to_vec(
            &json!({"context":c,"records":db.records.values().filter(|r|r.context==c.id).collect::<Vec<_>>(),"verified":false,"checksums":checksums}),
        )?,
    )?;
    fs::File::open(d.join("snapshot.json"))?.sync_all()?;
    fs::write(d.join("complete"), "snapshot complete")?;
    fs::File::open(d.join("complete"))?.sync_all()?;
    fs::File::open(&d)?.sync_all()?;
    Ok(id)
}
const PRESERVE_OBB: &str = r#"
set -eu
package="$1"
obb="/data/media/0/Android/obb/$package"
if [ -L "$obb" ]; then
    source="$(readlink "$obb")"
    case "$source" in /data/app/*) ;; *) echo "Unexpected OBB resource link; refusing to replace it" >&2; exit 64 ;; esac
    case "$source" in *'/../'*|*'/./'*) echo "Invalid OBB resource link" >&2; exit 64 ;; esac
    stage="${obb}.framely-preserve-$2"
    mkdir -p "$stage"
    if [ -d "$source" ]; then
        for entry in "$source"/* "$source"/.[!.]* "$source"/..?*; do
            [ -e "$entry" ] || [ -L "$entry" ] || continue
            name="${entry##*/}"
            # Lepton may link OBB to the whole APK directory. Its Android-managed
            # files are not downloaded game resources and must not be copied.
            if [ "${source##*/}" != obb ]; then
                case "$name" in *.apk|lib|oat|steam_appid.txt|UECommandLine.txt) continue ;; esac
            fi
            cp -a "$entry" "$stage/"
        done
    fi
    # Keep the old link as recovery evidence; never follow or delete its target.
    mv "$obb" "${stage}.legacy-link"
    mv "$stage" "$obb"
fi
"#;
fn install_package(c: &Container, apk: &Path, log: &Path) -> Result<()> {
    let metadata = cached_metadata(apk)?;
    podman(
        &[
            "exec",
            &format!("lepton-{}", c.name),
            "sh",
            "-c",
            PRESERVE_OBB,
            "framely-preserve-obb",
            &metadata.package,
            &hex::encode(rand::random::<[u8; 12]>()),
        ],
        Some(log),
    )?;
    let remote = format!(
        "/data/local/tmp/framely-{}.apk",
        hex::encode(rand::random::<[u8; 12]>())
    );
    podman(
        &[
            "cp",
            apk.to_str().context("Invalid APK path")?,
            &format!("lepton-{}:{remote}", c.name),
        ],
        Some(log),
    )?;
    // Bypass Lepton's single-app wrapper: it relocates every APK to the same
    // mount and rebuilds OBB links. Keep Android's independent package paths.
    let result = podman(
        &[
            "exec",
            &format!("lepton-{}", c.name),
            "cmd_real",
            "package",
            "install",
            "-r",
            "--user",
            "0",
            &remote,
        ],
        Some(log),
    );
    let _ = podman(
        &["exec", &format!("lepton-{}", c.name), "rm", "-f", &remote],
        Some(log),
    );
    let output = result?;
    ensure!(
        output.lines().any(|s| s.trim() == "Success"),
        "APK installation failed: {}",
        output.trim()
    );
    Ok(())
}
fn launch_component(a: &App) -> Result<String> {
    let name = a
        .activity
        .as_ref()
        .or(a.metadata.activities.first())
        .context("No launchable activity; choose a declared activity in advanced settings")?;
    ensure!(
        a.metadata.declared_activities.contains(name),
        "Activity is not declared by this APK"
    );
    Ok(format!("{}/{}", a.metadata.package, name))
}
fn apply_orientation(c: &Container, orientation: Option<&str>, log: &Path) -> Result<()> {
    let container = format!("lepton-{}", c.name);
    podman(&["exec", &container, "wm", "size", "reset"], Some(log))?;
    let (mode, rotation, fixed) = match orientation {
        None | Some("auto") => ("free", "0", "disabled"),
        Some("landscape") => ("lock", "0", "enabled"),
        Some("portrait") => ("lock", "0", "enabled"),
        _ => bail!("Invalid window orientation"),
    };
    podman(
        &["exec", &container, "wm", "set-fix-to-user-rotation", fixed],
        Some(log),
    )?;
    podman(
        &[
            "exec",
            &container,
            "wm",
            "set-user-rotation",
            mode,
            rotation,
        ],
        Some(log),
    )?;
    Ok(())
}
#[cfg(test)]
fn launch(c: &Container, a: &App, log: &Path) -> Result<()> {
    launch_with_started(c, a, log, || {})
}
fn launch_with_started(c: &Container, a: &App, log: &Path, started: impl FnOnce()) -> Result<()> {
    let component = launch_component(a)?;
    podman(
        &[
            "exec",
            &format!("lepton-{}", c.name),
            "setprop",
            "lepton.active_app_id",
            &a.metadata.package,
        ],
        Some(log),
    )?;
    if a.show_window.unwrap_or(!a.metadata.vr) {
        apply_orientation(c, a.orientation.as_deref(), log)?;
    }
    let runtime = format!("lepton-{}", c.name);
    let mut launch_args = vec!["exec"];
    launch_args.extend([runtime.as_str(), "am", "start", "-W", "--user", "0"]);
    if a.show_window.unwrap_or(!a.metadata.vr) {
        launch_args.extend(["--windowingMode", "1"]);
    }
    launch_args.extend(["-n", &component]);
    let s = podman(&launch_args, Some(log))?;
    ensure!(
        !s.contains("Error:")
            && !s.contains("Exception")
            && s.lines().any(|line| line.trim() == "Status: ok"),
        "Application launch failed: {s}"
    );
    if a.show_window.unwrap_or(!a.metadata.vr) {
        podman(
            &[
                "exec",
                &runtime,
                "sh",
                "/vendor/share/framely-window.sh",
                &a.metadata.package,
            ],
            Some(log),
        )?;
    }
    // Select the display mode after the requested activity is foreground.
    // Lepton's per-app HWC mode can create a zero-sized xdg_surface on Gamescope
    // and abort its display service. Full-display mode avoids that transition;
    // flat apps retain their original full-display boot window.
    let visible_app = if a.show_window.unwrap_or(!a.metadata.vr) {
        "Waydroid"
    } else {
        "none"
    };
    podman(
        &[
            "exec",
            &format!("lepton-{}", c.name),
            "setprop",
            "waydroid.active_apps",
            visible_app,
        ],
        Some(log),
    )?;
    // ActivityManager can report success before HWC or the application exits.
    started();
    lifecycle::launched(c, a);
    // Keep the caller's pending state until the initial display handoff settles.
    #[cfg(not(test))]
    let samples = 16;
    #[cfg(test)]
    let samples = 1;
    for _ in 0..samples {
        check_launch_health(c, a, log)?;
        #[cfg(not(test))]
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}
const LAUNCH_HEALTH_PROBE: &str = r#"
if [ "$1" = flat ] && ! pidof surfaceflinger >/dev/null; then
    echo FRAMELY_DISPLAY_MISSING
elif pidof "$2" >/dev/null; then
    echo FRAMELY_APP_PRESENT
else
    echo FRAMELY_APP_MISSING
fi
"#;
fn check_launch_health(c: &Container, a: &App, log: &Path) -> Result<()> {
    let container = format!("lepton-{}", c.name);
    let result = podman(
        &[
            "exec",
            &container,
            "sh",
            "-c",
            LAUNCH_HEALTH_PROBE,
            "framely-launch-health",
            if a.show_window.unwrap_or(!a.metadata.vr) {
                "flat"
            } else {
                "vr"
            },
            &a.metadata.package,
        ],
        Some(log),
    );
    let text = match result {
        Ok(text) => text,
        Err(error) => {
            ensure!(
                running(&c.name),
                "Lepton exited shortly after launch; inspect the application log"
            );
            return Err(
                error.context("Cannot verify Android process state; inspect the application log")
            );
        }
    };
    match text.trim() {
        "FRAMELY_APP_PRESENT" => return Ok(()),
        "FRAMELY_DISPLAY_MISSING" => bail!(
            "Android display service exited shortly after launch; inspect the application log"
        ),
        "FRAMELY_APP_MISSING" => {}
        _ => bail!("Cannot verify Android process state; inspect the application log"),
    }
    // Keep support for applications with a custom Android process name.
    let dump = podman(
        &["exec", &container, "dumpsys", "activity", "processes"],
        Some(log),
    )?;
    ensure!(
        dump.lines().any(|line| {
            line.split_once("packageList={")
                .and_then(|(_, names)| names.split_once('}'))
                .is_some_and(|(names, _)| {
                    names
                        .split(',')
                        .any(|name| name.trim() == a.metadata.package)
                })
        }),
        "Application exited shortly after launch; inspect the application log"
    );
    Ok(())
}
fn live_installed(c: &Container, package: &str, log: &Path) -> Result<bool> {
    let text = podman(
        &[
            "exec",
            &format!("lepton-{}", c.name),
            "pm",
            "list",
            "packages",
            "--user",
            "0",
            package,
        ],
        Some(log),
    )?;
    ensure!(
        !text.contains("Error:") && !text.contains("Exception"),
        "Cannot verify Android package state: {text}"
    );
    Ok(text
        .lines()
        .any(|line| line.trim() == format!("package:{package}")))
}
fn purge_retained(home: &Path, db: &mut Database, id: &str, log: &Path) -> Result<u64> {
    let (a, c) = app(home, db, id)?;
    ensure!(
        !a.installed && a.pending.is_none(),
        "Application was reinstalled or cannot be verified; scan again"
    );
    start(home, &c, a.show_window, log)?;
    ensure!(
        !live_installed(&c, &a.metadata.package, log)?,
        "Application was reinstalled; data cleanup refused"
    );
    let s = podman(
        &[
            "exec",
            &format!("lepton-{}", c.name),
            "pm",
            "uninstall",
            &a.metadata.package,
        ],
        Some(log),
    );
    if !s.is_ok_and(|s| s.lines().any(|s| s.trim() == "Success")) {
        let old = root(home).join("apks").join(hash(id)).join("base.apk");
        ensure!(
            old.is_file(),
            "Original APK is required to safely remove retained Android package data"
        );
        let meta = apk_metadata::read(&old)?;
        ensure!(
            meta.package == a.metadata.package,
            "Stored APK package mismatch"
        );
        install_package(&c, &old, log)?;
        let s = podman(
            &[
                "exec",
                &format!("lepton-{}", c.name),
                "pm",
                "uninstall",
                &a.metadata.package,
            ],
            Some(log),
        )?;
        ensure!(
            s.lines().any(|s| s.trim() == "Success"),
            "Retained-data removal failed: {s}"
        );
    }
    db.records.remove(id);
    save(home, db)?;
    let path = root(home).join("apks").join(hash(id));
    let freed = a.size + size(&path);
    if path.exists() {
        remove_tree(&path)?;
    }
    Ok(freed)
}
fn clean_candidates(home: &Path, db: &Database) -> Result<Vec<Value>> {
    let cs = containers(home, db);
    let (apps, warnings) = apps(home, db, &cs, true);
    let mut out = Vec::new();
    for a in apps
        .iter()
        .filter(|a| a.state_known && !a.installed && a.pending.is_none())
    {
        out.push(json!({"id":format!("data:{}",a.id),"kind":"data","name":a.metadata.name,"bytes":a.size,"sensitive":true,"app":a.id}));
    }
    for c in &cs {
        let d = c.baked.join("data_overlay/data");
        if let Ok(es) = fs::read_dir(d) {
            for e in es.flatten() {
                let package = e.file_name().to_string_lossy().into_owned();
                if apk_metadata::valid_package(&package)
                    && !package.starts_with("com.android.")
                    && !package.starts_with("android.")
                    && !apps
                        .iter()
                        .any(|a| a.context == c.id && a.metadata.package == package)
                {
                    out.push(json!({"id":format!("unknown:{}/{}",c.id,package),"kind":"unknown","name":package,"bytes":size(&e.path()),"sensitive":true,"unknown":true}));
                }
            }
        }
    }
    for kind in ["reviews", "backups", "apks", "uploads"] {
        let d = root(home).join(kind);
        if let Ok(es) = fs::read_dir(d) {
            for e in es.flatten() {
                if fs::symlink_metadata(e.path())?.file_type().is_symlink() {
                    continue;
                }
                let name = e.file_name().to_string_lossy().into_owned();
                if !safe(&name) {
                    continue;
                }
                if kind == "apks" && db.records.values().any(|r| hash(&r.id) == name) {
                    continue;
                }
                let age = now().saturating_sub(
                    fs::metadata(e.path())?
                        .modified()?
                        .duration_since(UNIX_EPOCH)?
                        .as_secs(),
                );
                if (kind == "reviews" || kind == "uploads") && age < 900 {
                    continue;
                }
                let snapshot = if kind == "backups" {
                    fs::read(e.path().join("snapshot.json"))
                        .ok()
                        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                } else {
                    None
                };
                let context = snapshot.as_ref().and_then(|s| s["context"]["id"].as_str());
                let restorable = e.path().join("complete").is_file()
                    && snapshot
                        .as_ref()
                        .is_some_and(|s| s["checksums"].is_object())
                    && context.is_some_and(|id| {
                        cs.iter().any(|c| {
                            c.id == id
                                && !c.steam
                                && snapshot
                                    .as_ref()
                                    .is_some_and(|s| s["context"]["baked"] == json!(c.baked))
                                && !warnings
                                    .iter()
                                    .any(|w| w.starts_with(&format!("{}:", c.name)))
                                && !apps.iter().any(|a| a.context == id && !a.state_known)
                                && apps
                                    .iter()
                                    .filter(|a| a.context == id && a.installed)
                                    .count()
                                    <= 1
                        })
                    })
                    && snapshot
                        .as_ref()
                        .is_some_and(|s| s["records"].as_array().is_some_and(|v| v.len() == 1));
                out.push(json!({"id":format!("{kind}:{name}"),"kind":kind,"name":name,"bytes":size(&e.path()),"sensitive":kind=="backups","ageSeconds":age,"context":context,"restorable":restorable}));
            }
        }
    }
    Ok(out)
}
pub fn cleanup_list(home: &Path) -> Result<Value> {
    Ok(json!(clean_candidates(home, &load(home)?)?))
}
pub fn logs(home: &Path, id: &str) -> Result<Value> {
    ensure!(id.len() <= 512, "Invalid application id");
    let dir = root(home).join("logs");
    let key = hash(id);
    let context = id.split_once('/').map(|p| hash(p.0));
    let mut text = String::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        let mut paths: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name().is_some_and(|s| {
                    let s = s.to_string_lossy();
                    s.starts_with(&key) || context.as_ref().is_some_and(|k| s.starts_with(k))
                })
            })
            .collect();
        paths.sort();
        for p in paths.into_iter().rev().take(10) {
            if fs::symlink_metadata(&p)?.file_type().is_symlink() {
                continue;
            }
            let mut s = String::new();
            fs::File::open(p)?
                .take(512 * 1024)
                .read_to_string(&mut s)
                .ok();
            text.push_str(&s);
        }
    }
    Ok(json!({"name":format!("framely-apk-{key}.log"),"text":text}))
}
pub fn reviews_drop(home: &Path, ticket: &str) -> Result<Value> {
    let _guard = MUTATION.lock().unwrap();
    ensure!(
        ticket.len() == 48 && ticket.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid APK review"
    );
    let d = root(home).join("reviews").join(ticket);
    if d.exists() {
        ensure!(
            !fs::symlink_metadata(&d)?.file_type().is_symlink(),
            "Invalid review path"
        );
        fs::remove_dir_all(d)?;
    }
    Ok(json!(true))
}
pub fn operate(
    home: &Path,
    kind: &str,
    p: &Value,
    cancel: Cancellation,
    progress: Arc<dyn Fn(Value) + Send + Sync>,
) -> Result<Value> {
    if kind == "launch" {
        if let Some(id) = p["app"].as_str() {
            if steam_shortcuts::ensure_registered(home, id, false)? {
                return steam_bridge::request_launch(home, id, progress, &cancel);
            }
        }
    }
    let result = operate_internal(home, kind, p, cancel, progress, None);
    if matches!(kind, "install" | "steam.retention.settings") && result.is_ok() {
        steam_shortcuts::wake_registration();
    }
    result
}
fn operate_internal(
    home: &Path,
    kind: &str,
    p: &Value,
    cancel: Cancellation,
    progress: Arc<dyn Fn(Value) + Send + Sync>,
    steam_app_id: Option<u32>,
) -> Result<Value> {
    let _guard = MUTATION
        .try_lock()
        .map_err(|_| anyhow::anyhow!("Another APK operation is running"))?;
    cancel.check()?;
    let r = init(home)?;
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(r.join("operation.lock"))?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "Another APK operation is running"
    );
    let mut db = load(home)?;
    recover_restores(home, &mut db)?;
    let id = p["app"]
        .as_str()
        .or(p["context"].as_str())
        .unwrap_or("installation");
    let log = r.join("logs").join(format!(
        "{}-{}-{}.log",
        hash(id),
        now(),
        hex::encode(rand::random::<[u8; 6]>())
    ));
    fs::write(&log, format!("{kind}\n"))?;
    progress(json!({"phase":"preparing","operation":kind}));
    let result = (|| -> Result<Value> {
        if let Some(app_id) = steam_app_id {
            ensure!(kind == "launch", "Invalid Steam wrapper operation");
            let rec = db.records.get_mut(id).context("Missing Steam APK record")?;
            ensure!(
                rec.steam_launch && steam_shortcuts::linked(home, rec),
                "Steam launch is disabled"
            );
            rec.steam_app_id = Some(app_id);
            save(home, &db)?;
        }
        match kind {
            "steam.settings" => {
                let enabled = p["enabled"]
                    .as_bool()
                    .context("Invalid Steam launch setting")?;
                let (a, c) = app(home, &db, id)?;
                ensure!(!a.steam, "This app is already managed by Steam");
                if !enabled
                    && c.name.starts_with("steamlaunch-")
                    && db.records.get(id).and_then(native::runtime_name).is_some()
                {
                    ensure!(
                        !running(&c.name),
                        "Close this native Steam application before disabling Steam launch"
                    );
                }
                record_app(&mut db, &a);
                let retain = db.retain_steam_entries;
                let rec = db.records.get_mut(id).context("Missing APK record")?;
                if enabled && retain {
                    steam_shortcuts::register(home, &a, rec)?;
                    rec.steam_launch = true;
                } else if !enabled {
                    steam_shortcuts::unregister(home, rec)?;
                    rec.steam_binding = None;
                    rec.steam_app_id = None;
                    rec.steam_launch = false;
                }
                rec.steam_preference = Some(enabled);
                rec.steam_registration_error = None;
                save(home, &db)?;
                Ok(json!(true))
            }
            "steam.retention.settings" => {
                let retain = p["enabled"]
                    .as_bool()
                    .context("Invalid Steam entry retention setting")?;
                cancel.commit(|| {
                    db.retain_steam_entries = retain;
                    save(home, &db)?;
                    Ok(json!(true))
                })
            }
            "lifecycle.settings" => {
                let auto = p["autoStopContainer"]
                    .as_bool()
                    .context("Invalid automatic container stop setting")?;
                let close = p["stopContainerOnClose"]
                    .as_bool()
                    .context("Invalid window close setting")?;
                cancel.commit(|| {
                    db.auto_stop_container = auto;
                    db.stop_container_on_close = close;
                    save(home, &db)?;
                    Ok(json!(true))
                })
            }
            "gamepad.settings" => {
                let enabled = p["enabled"].as_bool().context("Invalid gamepad setting")?;
                let source = if let Some(value) = p.get("source") {
                    value.as_str().context("Invalid gamepad input source")?
                } else {
                    &db.gamepad_source
                }
                .to_owned();
                ensure!(
                    matches!(source.as_str(), "steam" | "frame"),
                    "Invalid gamepad input source"
                );
                let rumble = if p.get("rumble").is_some() {
                    p["rumble"]
                        .as_bool()
                        .context("Invalid gamepad rumble setting")?
                } else {
                    db.gamepad_rumble
                };
                cancel.commit(|| {
                    db.gamepad_enabled = enabled;
                    db.gamepad_source = source;
                    db.gamepad_rumble = rumble;
                    save(home, &db)?;
                    crate::gamepad::set_rumble_enabled(rumble);
                    if !enabled {
                        crate::gamepad::stop();
                    }
                    Ok(json!(true))
                })
            }
            "root.add" => {
                let cs = containers(home, &db);
                let path = p["path"].as_str().context("Missing data directory")?;
                let path = validate_baked(Path::new(path))?;
                ensure!(
                    !path.starts_with("/proc")
                        && !path.starts_with("/sys")
                        && !path.starts_with("/dev"),
                    "Invalid data directory"
                );
                if let Some(context) = p["context"].as_str().filter(|s| !s.is_empty()) {
                    ensure!(
                        safe(context) && !context.starts_with("steamlaunch-"),
                        "Invalid side-loaded context name"
                    );
                    ensure!(
                        !cs.iter().any(|c| c.name == context && c.baked != path),
                        "Container name is already used by another data directory"
                    );
                    db.root_contexts.insert(path.clone(), context.into());
                }
                let name = db.root_contexts.get(&path).cloned().unwrap_or_else(|| {
                    path.parent()
                        .and_then(|p| p.file_name())
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                });
                ensure!(
                    safe(&name) && !name.starts_with("steamlaunch-"),
                    "Invalid side-loaded context name"
                );
                ensure!(
                    !cs.iter().any(|c| c.name == name && c.baked != path),
                    "Container name is already used by another data directory"
                );
                if !db.roots.contains(&path) {
                    db.roots.push(path)
                }
                cancel.commit(|| save(home, &db))?;
                Ok(json!(true))
            }
            "root.remove" => {
                let path = p["path"].as_str().context("Missing data directory")?;
                db.roots.retain(|r| r != Path::new(path));
                db.root_contexts.remove(Path::new(path));
                cancel.commit(|| save(home, &db))?;
                Ok(json!(true))
            }
            "install" => {
                ensure!(p["approve"] == true, "Confirm APK installation");
                let show_window = p
                    .get("showWindow")
                    .map(|value| value.as_bool().context("Invalid APK display mode"))
                    .transpose()?;
                let ticket = p["ticket"].as_str().context("Missing reviewed APK")?;
                let (review, metadata) = review(home, ticket)?;
                let requested = p["context"].as_str().unwrap_or("");
                let cs = containers(home, &db);
                let context = if requested.is_empty() {
                    format!("framely-{}", hash(&metadata.package))
                } else {
                    requested.into()
                };
                let app_id = format!("{context}/{}", metadata.package);
                let existing = cs.iter().find(|c| c.id == context).cloned();
                let c = existing.unwrap_or_else(|| Container {
                    id: context.clone(),
                    name: context.clone(),
                    baked: home
                        .join(".local/share/lepton/contexts")
                        .join(&context)
                        .join("baked"),
                    running: false,
                    managed: true,
                    steam: false,
                });
                ensure!(!c.steam, "Manage Steam APKs through Steam");
                ensure!(
                    !cs.iter()
                        .any(|other| other.name == c.name && other.baked != c.baked),
                    "Container name is ambiguous; correct its data location first"
                );
                ensure!(
                    c.managed || cs.iter().any(|x| x.id == context),
                    "Unknown target container"
                );
                ensure!(
                    safe(&c.name) && !c.name.starts_with("steamlaunch-"),
                    "Invalid side-loaded container name"
                );
                if !cs.iter().any(|x| x.id == context) {
                    ensure!(
                        !c.baked.parent().context("Missing context parent")?.exists(),
                        "Container already exists but could not be read; do not overwrite it"
                    );
                    if !db.owned_contexts.contains(&context) {
                        db.owned_contexts.push(context.clone());
                    }
                }
                let packages = if c.baked.exists() {
                    installed(home, &c)?
                } else {
                    Vec::new()
                };
                let previous = packages
                    .iter()
                    .find(|(m, _)| m.package == metadata.package)
                    .cloned();
                // Existing shared containers stay readable and updatable. New
                // package identities cannot reuse another app's context, even
                // when only its retained data remains. Reject before any stop,
                // backup, journal write or package-manager call.
                ensure!(previous.is_some() || (
                    !packages.iter().any(|(m, _)| m.package != metadata.package)
                    && !db.records.values().any(|record| record.context == c.id && record.metadata.package != metadata.package)
                ), "Each new application requires its own container; shared containers only support updates to their existing applications");
                if let Some((m, _)) = &previous {
                    ensure!(
                        metadata.version_code >= m.version_code,
                        "APK downgrades are not supported"
                    );
                }
                if p["app"].is_string() {
                    let (a, old) = app(home, &db, id)?;
                    ensure!(
                        a.state_known && old.id == c.id && a.metadata.package == metadata.package,
                        "APK package or target container does not match"
                    );
                }
                space(
                    &r,
                    fs::metadata(review.join("base.apk"))?.len() * 4
                        + if c.baked.exists() { size(&c.baked) } else { 0 },
                )?;
                let rec = db.records.entry(app_id.clone()).or_insert_with(|| Record {
                    id: app_id.clone(),
                    context: c.id.clone(),
                    metadata: metadata.clone(),
                    removed: previous.is_none(),
                    ..Default::default()
                });
                rec.pending =
                    Some("Installation interrupted; refresh to reconcile installed version".into());
                save(home, &db)?;
                let mut settings = db.records.get(&app_id).cloned().unwrap();
                if let Some(show) = show_window {
                    settings.show_window = Some(show);
                }
                cancel.check()?;
                // Before submission cancellation remains possible. Package-manager submission is a commit.
                cancel.commit(||{
 progress(json!({"cancellable":false}));
 let mut backup_id=None;if c.baked.exists(){progress(json!({"phase":"backing-up"}));stop(&c,&log)?;backup_id=Some(backup(home,&c,&db,&log)?);}
progress(json!({"phase":"starting"}));start(home,&c,Some(false),&log)?;
 let sdk=podman(&["exec",&format!("lepton-{}",c.name),"getprop","ro.build.version.sdk"],Some(&log))?.trim().parse::<u32>()?;ensure!(metadata.min_sdk<=sdk,"APK requires Android SDK {}, container provides {}",metadata.min_sdk,sdk);
 progress(json!({"phase":"installing","cancellable":false}));install_package(&c,&review.join("base.apk"),&log)?;
 progress(json!({"phase":"verifying"}));let path=podman(&["exec",&format!("lepton-{}",c.name),"pm","path",&metadata.package],Some(&log))?;ensure!(path.contains("package:"),"Android did not report the installed APK");let dest=r.join("apks").join(hash(&app_id));store_apk(&review.join("base.apk"),&dest)?;
 // Installation boots a headless container solely for the package manager.
 // Finish that runtime before Steam takes ownership of the first app launch.
 stop(&c,&log)?;
 let rec=db.records.get_mut(&app_id).unwrap();rec.metadata=metadata.clone();rec.removed=false;rec.pending=None;rec.show_window=settings.show_window;save(home,&db)?;fs::remove_dir_all(review)?;
 // Mirror the operation log under the stable application id for newly installed apps.
 if id!=app_id {let _=fs::copy(&log,r.join("logs").join(format!("{}-{}.log",hash(&app_id),now())));}
 Ok(json!({"app":app_id,"backup":backup_id,"launchable":!metadata.activities.is_empty()}))})
            }
            "reconcile" => {
                let cs = containers(home, &db);
                let (actual, warnings) = apps(home, &db, &cs, false);
                let a = actual
                    .iter()
                    .find(|a| a.id == id)
                    .context("APK application not found")?;
                let c = cs
                    .iter()
                    .find(|c| c.id == a.context)
                    .context("Container unavailable; add its data location first")?;
                ensure!(
                    !warnings
                        .iter()
                        .any(|w| w.starts_with(&format!("{}:", c.name))),
                    "This container cannot be read; inspect warnings before reconciliation"
                );
                ensure!(!c.steam, "Manage Steam APKs through Steam");
                ensure!(
                    !cs.iter()
                        .any(|other| other.name == c.name && other.baked != c.baked),
                    "Container name is ambiguous; correct its data location first"
                );
                cancel.commit(|| {
                    progress(json!({"cancellable":false}));
                    start(home, c, a.show_window, &log)?;
                    let installed = live_installed(c, &a.metadata.package, &log)?;
                    record_app(&mut db, a);
                    let rec = db.records.get_mut(id).unwrap();
                    rec.removed = !installed;
                    rec.pending = None;
                    if installed {
                        rec.metadata = a.metadata.clone();
                    }
                    save(home, &db)?;
                    Ok(json!({"installed":installed}))
                })
            }
            "settings" => {
                let (a, _) = app(home, &db, id)?;
                ensure!(
                    a.state_known,
                    "APK state is unknown; reconcile before changing the application"
                );
                record_app(&mut db, &a);
                let rec = db.records.get_mut(id).unwrap();
                if let Some(v) = p.get("activity") {
                    let activity = v.as_str().filter(|s| !s.is_empty());
                    ensure!(
                        activity.is_none_or(|s| a
                            .metadata
                            .declared_activities
                            .iter()
                            .any(|a| a == s)),
                        "Activity is not declared by this APK"
                    );
                    rec.activity = activity.map(str::to_owned);
                }
                if let Some(v) = p.get("showWindow") {
                    ensure!(v.is_boolean() || v.is_null(), "Invalid window setting");
                    rec.show_window = v.as_bool();
                }
                if let Some(v) = p.get("orientation") {
                    ensure!(
                        v.as_str()
                            .is_some_and(|s| matches!(s, "auto" | "landscape" | "portrait")),
                        "Invalid window orientation"
                    );
                    rec.orientation = v.as_str().filter(|s| *s != "auto").map(str::to_owned);
                }
                cancel.commit(|| save(home, &db))?;
                Ok(json!(true))
            }
            "launch" | "close" | "uninstall" | "purge" | "clear" => {
                let (a, c) = app(home, &db, id)?;
                ensure!(
                    a.state_known,
                    "APK state is unknown; reconcile before changing the application"
                );
                record_app(&mut db, &a);
                cancel.check()?;
                let delete_container = p
                    .get("deleteContainer")
                    .map(|v| v.as_bool().context("Invalid container deletion option"))
                    .transpose()?
                    .unwrap_or(false);
                if delete_container {
                    ensure!(
                        matches!(kind, "uninstall" | "purge") && p["approve"] == true,
                        "Confirm APK removal before deleting its container"
                    );
                    ensure!(
                        kind == "purge" || p["purge"] == true,
                        "Delete application data before deleting its container"
                    );
                    ensure!(
                        a.pending.is_none(),
                        "Reconcile the interrupted operation before deleting data"
                    );
                    let (inventory, warnings) = apps(home, &db, std::slice::from_ref(&c), false);
                    let members: Vec<_> = inventory.iter().filter(|a| a.context == c.id).collect();
                    ensure!(warnings.is_empty() && members.len() == 1 && members[0].id == id
                        && members[0].state_known
                        && !db.records.values().any(|r| r.context == c.id && r.id != id),
                        "This container contains other applications or unknown state; uninstall without deleting the container");
                    // Deleting the exclusive context also uninstalls its APK.
                    // Keep its binding until cleanup has removed both runtimes
                    // and the Steam entry; pm uninstall would lose that context.
                    return cancel.commit(|| {
                        progress(json!({"cancellable":false,"phase":"uninstalling"}));
                        cleanup::delete(home, &mut db, &c, &log)?;
                        Ok(json!({"containerDeleted":true}))
                    });
                }
                if !a.installed && (kind == "purge" || (kind == "uninstall" && p["purge"] == true))
                {
                    ensure!(p["approve"] == true, "Confirm APK removal");
                    return cancel.commit(|| {
                        progress(json!({"cancellable":false}));
                        if let Some(rec) = db.records.get_mut(id) {
                            steam_shortcuts::unregister(home, rec)
                                .context("APK was uninstalled, but Steam entry cleanup failed; retry to finish cleanup")?;
                            rec.steam_launch = false;
                            rec.steam_binding = None;
                            rec.steam_app_id = None;
                        }
                        save(home, &db)?;
                        Ok(json!({"freedBytes":purge_retained(home,&mut db,id,&log)?}))
                    });
                }
                if matches!(kind, "uninstall" | "purge" | "clear") {
                    ensure!(p["approve"] == true, "Confirm APK removal");
                }
                cancel.commit(|| {
                    progress(json!({"cancellable":false}));
                    match kind {
                        "launch" => {
                            ensure!(a.installed, "Application is not installed");
                            launch_component(&a)?;
                            progress(json!({"phase":"starting"}));
                            let flat = a.show_window.unwrap_or(!a.metadata.vr);
                            if flat && running(&c.name) {
                                let name = format!("lepton-{}", c.name);
                                let headless = podman(
                                    &["exec", &name, "getprop", "lepton.headless"],
                                    Some(&log),
                                )?;
                                // A hidden window does not mean the display service
                                // or container needs recreation. Only a headless
                                // boot or changed orientation needs that transition.
                                let orientation = podman(
                                    &["exec", &name, "getprop", "framely.window_orientation"],
                                    Some(&log),
                                )?;
                                let current = if orientation.trim().is_empty() {
                                    "auto"
                                } else {
                                    orientation.trim()
                                };
                                if headless.trim() == "true"
                                    || current != a.orientation.as_deref().unwrap_or("auto")
                                {
                                    stop(&c, &log)?;
                                }
                            }
                            // A running container cannot acquire a new bind-mounted
                            // input node. Reboot only if its bridge identity differs.
                            if running(&c.name) && db.gamepad_enabled {
                                let token = podman(
                                    &[
                                        "exec",
                                        &format!("lepton-{}", c.name),
                                        "getprop",
                                        "framely.gamepad.bridge",
                                    ],
                                    Some(&log),
                                )?;
                                if crate::gamepad::current(&c.name)
                                    .is_none_or(|m| m.token != token.trim())
                                {
                                    stop(&c, &log)?;
                                }
                            }
                            let gamepad = if db.gamepad_enabled {
                                Some(crate::gamepad::prepare(
                                    &root(home),
                                    &c.name,
                                    &db.gamepad_source,
                                    db.gamepad_rumble,
                                )?)
                            } else {
                                None
                            };
                            let launch_result = (|| -> Result<()> {
                                start_oriented_container(
                                    home,
                                    &c,
                                    Some(flat),
                                    !flat,
                                    if flat { a.orientation.as_deref() } else { None },
                                    gamepad.as_ref(),
                                    steam_app_id,
                                    &log,
                                )?;
                                progress(json!({"phase":"launching"}));
                                launch_with_started(&c, &a, &log, || {
                                    progress(json!({"phase":"started"}))
                                })?;
                                if gamepad.is_some() {
                                    crate::gamepad::activate(&c.name, &a.metadata.package)?;
                                }
                                Ok(())
                            })();
                            if launch_result.is_err() {
                                crate::gamepad::stop_app(&c.name, &a.metadata.package);
                            }
                            launch_result?;
                        }
                        "close" => {
                            crate::gamepad::stop_app(&c.name, &a.metadata.package);
                            if running(&c.name) {
                                let active = podman(
                                    &[
                                        "exec",
                                        &format!("lepton-{}", c.name),
                                        "getprop",
                                        "lepton.active_app_id",
                                    ],
                                    Some(&log),
                                )?;
                                if active.trim() == a.metadata.package {
                                    podman(
                                        &[
                                            "exec",
                                            &format!("lepton-{}", c.name),
                                            "setprop",
                                            "waydroid.active_apps",
                                            "none",
                                        ],
                                        Some(&log),
                                    )?;
                                }
                                podman(
                                    &[
                                        "exec",
                                        &format!("lepton-{}", c.name),
                                        "am",
                                        "force-stop",
                                        "--user",
                                        "0",
                                        &a.metadata.package,
                                    ],
                                    Some(&log),
                                )?;
                            }
                        }
                        "clear" => {
                            ensure!(
                                a.installed,
                                "Use retained-data cleanup for an uninstalled APK"
                            );
                            start(home, &c, a.show_window, &log)?;
                            let s = podman(
                                &[
                                    "exec",
                                    &format!("lepton-{}", c.name),
                                    "pm",
                                    "clear",
                                    "--user",
                                    "0",
                                    &a.metadata.package,
                                ],
                                Some(&log),
                            )?;
                            ensure!(s.contains("Success"), "Data clear failed: {s}");
                        }
                        _ => {
                            ensure!(
                                a.pending.is_none(),
                                "Reconcile the interrupted operation before deleting data"
                            );
                            if db.records.get(id).and_then(native::runtime_name).is_some() {
                                stop(&c, &log)?;
                            }
                            progress(json!({"phase":"starting"}));
                            if a.installed {
                                let dest = r.join("apks").join(hash(id));
                                if let Some((_, apk)) = installed(home, &Container { running:false, ..c.clone() })?
                                    .into_iter().find(|(m, _)| m.package == a.metadata.package) {
                                    if apk.is_file() { store_apk(&apk, &dest)?; }
                                }
                                let readiness = if kind == "uninstall" && p["purge"] != true {
                                    AndroidReadiness::PackageManager
                                } else { AndroidReadiness::RemovalWithData };
                                start_oriented_container_for(home, &c, Some(false), true, None, None, None, readiness, &log)?;
                                progress(json!({"phase":"uninstalling"}));
                                if !live_installed(&c, &a.metadata.package, &log)? {
                                    let cached = dest.join("base.apk");
                                    ensure!(cached.is_file(), "Original APK is required to recover package registration before uninstall");
                                    install_package(&c, &cached, &log)?;
                                }
                                let mut args = vec!["exec", "", "pm", "uninstall"];
                                let name = format!("lepton-{}", c.name);
                                args[1] = &name;
                                if kind == "uninstall" && p["purge"] != true {
                                    args.push("-k");
                                }
                                args.push(&a.metadata.package);
                                let s = podman(&args, Some(&log))?;
                                ensure!(
                                    s.lines().any(|s| s.trim() == "Success"),
                                    "Uninstall failed: {s}"
                                );
                            }
                            let rec = db.records.get_mut(id).unwrap();
                            rec.removed = true;
                            rec.pending = None;
                            // Persist Android success before touching Steam. If cleanup
                            // fails, retry only cleanup rather than uninstalling twice.
                            save(home, &db)?;
                            let rec = db.records.get_mut(id).unwrap();
                            steam_shortcuts::unregister(home, rec)
                                .context("APK was uninstalled, but Steam entry cleanup failed; retry to finish cleanup")?;
                            rec.steam_launch = false;
                            rec.steam_binding = None;
                            rec.steam_app_id = None;
                            if kind == "purge" || p["purge"] == true {
                                db.records.remove(id);
                                let path = r.join("apks").join(hash(id));
                                if path.exists() {
                                    remove_tree(&path)?;
                                }
                            }
                        }
                    }
                    save(home, &db)?;
                    Ok(json!(true))
                })
            }
            "container.start" | "container.stop" | "container.delete" | "restore" => {
                ensure!(p["approve"] == true, "Confirm container operation");
                let context = p["context"].as_str().context("Missing container")?;
                let cs = containers(home, &db);
                let c = cs
                    .iter()
                    .find(|c| c.id == context)
                    .cloned()
                    .context("Unknown container")?;
                ensure!(!c.steam, "Manage Steam containers through Steam");
                ensure!(
                    !cs.iter()
                        .any(|other| other.name == c.name && other.baked != c.baked),
                    "Container name is ambiguous; correct its data location first"
                );
                ensure!(
                    validate_baked(&c.baked)? == c.baked,
                    "Container data location changed"
                );
                cancel.commit(|| {
                    progress(json!({"cancellable":false}));
                    match kind {
                        "container.start" => start_container(home, &c, None, false, &log)?,
                        "container.stop" => stop(&c, &log)?,
                        "container.delete" => {
                            cleanup::delete(home, &mut db, &c, &log)?;
                        }
                        _ => {
                            let backup = p["backup"].as_str().context("Missing backup")?;
                            ensure!(safe(backup), "Invalid backup");
                            let d = r.join("backups").join(backup);
                            let snapshot = verified_snapshot(&d)?;
                            ensure!(
                                snapshot["context"]["id"] == context && snapshot["context"]["baked"] == json!(c.baked),
                                "Backup belongs to another container"
                            );
                            ensure!(
                                snapshot["records"].as_array().is_some_and(|v| v.len() == 1)
                                    && installed(home, &c)?.len() <= 1,
                                "Shared container backups cannot be restored automatically"
                            );
                            stop(&c, &log)?;
                            space(&r, size(&d.join("baked")))?;
                            let safety = backup_fn(home, &c, &db, &log)?;
                            let parent = c.baked.parent().context("Invalid context directory")?;
                            let replacement = parent.join(format!(
                                "restore-{}",
                                hex::encode(rand::random::<[u8; 8]>())
                            ));
                            copy(&d.join("baked"), &replacement, &log)?;
                            let old = parent.join(format!(
                                "previous-{}",
                                hex::encode(rand::random::<[u8; 8]>())
                            ));
                            let journal = parent.join("framely-restore.json");
                            fs::write(&journal, serde_json::to_vec(&json!({"old":old.file_name().and_then(|s|s.to_str()),"replacement":replacement.file_name().and_then(|s|s.to_str()),"records":snapshot["records"]}))?)?;
                            fs::File::open(&journal)?.sync_all()?;
                            fs::File::open(parent)?.sync_all()?;
                            fs::rename(&c.baked, &old)?;
                            fs::File::open(parent)?.sync_all()?;
                            if let Err(e) = fs::rename(&replacement, &c.baked) {
                                let _ = fs::rename(&old, &c.baked);
                                return Err(e.into());
                            }
                            fs::File::open(parent)?.sync_all()?;
                            recover_restores(home, &mut db)?;
                            return Ok(json!({"safetyBackup":safety,"verificationRequired":true}));
                        }
                    }
                    Ok(json!(true))
                })
            }
            "cleanup" => {
                ensure!(p["approve"] == true, "Confirm data cleanup");
                let ids: Vec<String> = serde_json::from_value(p["items"].clone())?;
                ensure!(ids.len() <= 512, "Too many cleanup items");
                let items = clean_candidates(home, &db)?;
                let mut removed = Vec::new();
                let mut failed = Vec::new();
                let mut freed = 0;
                cancel.commit(|| {
                    progress(json!({"cancellable":false}));
                    for id in ids {
                        let result = (|| -> Result<u64> {
                            let item = items
                                .iter()
                                .find(|i| i["id"] == id)
                                .context("Cleanup item changed; scan again")?;
                            if item["kind"] == "data" {
                                return purge_retained(
                                    home,
                                    &mut db,
                                    item["app"].as_str().context("Invalid retained-data item")?,
                                    &log,
                                );
                            }
                            let (kind, name) =
                                id.split_once(':').context("Invalid cleanup item")?;
                            ensure!(
                                ["reviews", "backups", "apks", "uploads"].contains(&kind)
                                    && safe(name),
                                "Invalid cleanup path"
                            );
                            let p = r.join(kind).join(name);
                            ensure!(
                                !fs::symlink_metadata(&p)?.file_type().is_symlink(),
                                "Cleanup path is a symlink"
                            );
                            let n = size(&p);
                            remove_tree(&p)?;
                            Ok(n)
                        })();
                        match result {
                            Ok(n) => {
                                freed += n;
                                removed.push(id)
                            }
                            Err(e) => failed.push(json!({"id":id,"error":format!("{e:#}")})),
                        }
                    }
                    Ok(json!({"removed":removed,"failed":failed,"freedBytes":freed}))
                })
            }
            _ => bail!("Unknown APK operation"),
        }
    })();
    if let Err(e) = &result {
        if let Ok(mut f) = fs::OpenOptions::new().append(true).open(&log) {
            let _ = writeln!(f, "ERROR: {e:#}");
        }
        if let Some(rec) = db.records.get_mut(id) {
            if kind == "install" {
                rec.pending = Some(format!("Update failed; refresh installed state: {e:#}"));
                let _ = save(home, &db);
            }
        }
    }
    if kind == "install" {
        if let Some(ticket) = p["ticket"].as_str() {
            if let Ok((_, metadata)) = review(home, ticket) {
                let context = p["context"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("framely-{}", hash(&metadata.package)));
                let target = format!("{context}/{}", metadata.package);
                let _ = fs::copy(
                    &log,
                    r.join("logs")
                        .join(format!("{}-{}.log", hash(&target), now())),
                );
            }
        }
    }
    result
}
fn backup_fn(home: &Path, c: &Container, db: &Database, log: &Path) -> Result<String> {
    backup(home, c, db, log)
}
pub(crate) fn gamepad_health(context: &str, package: &str) -> (Option<bool>, bool) {
    if !safe(context) || !apk_metadata::valid_package(package) {
        return (Some(false), false);
    }
    let mut cmd = crate::process::tool("podman");
    cmd.args([
        "exec",
        &format!("lepton-{context}"),
        "sh",
        "-c",
        "dumpsys activity processes && dumpsys activity activities",
        "framely-gamepad",
        package,
    ]);
    match output(cmd, Duration::from_secs(3), None) {
        Ok(text) => (
            Some(gamepad_present(&text, package)),
            gamepad_foreground(&text, package),
        ),
        Err(_) => (None, false),
    }
}
/// Read-only input/lifecycle diagnostics; excludes input events and Steam tokens.
pub fn input_status(home: &Path, id: &str) -> Result<Value> {
    let db = load(home)?;
    let (a, c) = app(home, &db, id)?;
    let sample = lifecycle::sample(&c);
    let (alive, focused) = gamepad_health(&c.name, &a.metadata.package);
    Ok(json!({
        "app": id, "container": c.name, "package": a.metadata.package,
        "alive": alive, "foreground": focused,
        "lifecycle": match sample {
            Ok(s) => json!({"activePackage":s.package,"alivePackages":s.alive}),
            Err(e) => json!({"error":format!("{e:#}")}),
        }
    }))
}
fn gamepad_present(text: &str, package: &str) -> bool {
    gamepad_foreground(text, package)
        || text.lines().any(|line| {
            line.split_once("packageList={")
                .and_then(|(_, names)| names.split_once('}'))
                .is_some_and(|(names, _)| names.split(',').any(|name| name.trim() == package))
        })
}
fn gamepad_foreground(text: &str, package: &str) -> bool {
    text.lines().any(|line| {
        (line.contains("mResumedActivity:") || line.contains("topResumedActivity="))
            && line.split_whitespace().any(|word| {
                word.strip_prefix(package)
                    .is_some_and(|rest| rest.starts_with('/'))
            })
    })
}
pub fn launch_app(home: &Path, id: &str) -> Result<()> {
    launch_app_with_progress(home, id, Arc::new(|_| {}))
}
pub fn launch_app_with_progress(
    home: &Path,
    id: &str,
    progress: Arc<dyn Fn(Value) + Send + Sync>,
) -> Result<()> {
    operate(
        home,
        "launch",
        &json!({"app":id}),
        Cancellation::default(),
        progress,
    )?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_settings_migrate_persist_and_reject_invalid_values() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        assert!(!load(home).unwrap().auto_stop_container);
        assert!(load(home).unwrap().stop_container_on_close);
        let old: Database = serde_json::from_value(json!({})).unwrap();
        assert!(old.stop_container_on_close);
        assert!(!old.auto_stop_container);
        operate(
            home,
            "lifecycle.settings",
            &json!({"autoStopContainer":true,"stopContainerOnClose":false}),
            Cancellation::default(),
            Arc::new(|_| {}),
        )
        .unwrap();
        assert!(load(home).unwrap().auto_stop_container);
        assert!(!load(home).unwrap().stop_container_on_close);
        assert!(operate(
            home,
            "lifecycle.settings",
            &json!({"autoStopContainer":"yes","stopContainerOnClose":true}),
            Cancellation::default(),
            Arc::new(|_| {})
        )
        .is_err());
        assert!(!load(home).unwrap().stop_container_on_close);
    }
    #[test]
    fn lifecycle_native_close_and_exit_stop_without_changing_application_data() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let mut monitor = lifecycle::Monitor::default();
        let n = Instant::now();
        monitor.tick(&f.home, n).unwrap();
        // A background switch leaves the window property visible and cannot stop it.
        monitor.tick(&f.home, n + Duration::from_secs(20)).unwrap();
        assert!(running("test"));
        fs::write(f.dir.path().join("closed-window"), "").unwrap();
        monitor.tick(&f.home, n + Duration::from_secs(21)).unwrap();
        monitor.tick(&f.home, n + Duration::from_secs(23)).unwrap();
        assert!(!running("test"));
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        // Automatic process-exit cleanup is independent of native window closing.
        fs::write(f.dir.path().join("running"), "true").unwrap();
        fs::remove_file(f.dir.path().join("closed-window")).unwrap();
        f.operation(
            "lifecycle.settings",
            json!({"autoStopContainer":true,"stopContainerOnClose":false}),
        )
        .unwrap();
        monitor.tick(&f.home, n + Duration::from_secs(24)).unwrap();
        fs::write(f.dir.path().join("dead-process"), "").unwrap();
        monitor.tick(&f.home, n + Duration::from_secs(25)).unwrap();
        monitor.tick(&f.home, n + Duration::from_secs(39)).unwrap();
        assert!(running("test"));
        monitor.tick(&f.home, n + Duration::from_secs(40)).unwrap();
        assert!(!running("test"));
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
    }
    #[test]
    fn lifecycle_handles_close_during_launch_and_rejects_incomplete_queries() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        f.operation("launch", json!({"app":"test/com.example.app"}))
            .unwrap();
        fs::write(f.dir.path().join("closed-window"), "").unwrap();
        let mut monitor = lifecycle::Monitor::default();
        let n = Instant::now();
        monitor.tick(&f.home, n).unwrap();
        monitor.tick(&f.home, n + Duration::from_secs(2)).unwrap();
        assert!(!running("test"));
        fs::write(f.dir.path().join("running"), "true").unwrap();
        fs::remove_file(f.dir.path().join("closed-window")).unwrap();
        f.operation(
            "lifecycle.settings",
            json!({"autoStopContainer":true,"stopContainerOnClose":false}),
        )
        .unwrap();
        monitor.tick(&f.home, n + Duration::from_secs(3)).unwrap();
        fs::write(f.dir.path().join("dead-process"), "").unwrap();
        monitor.tick(&f.home, n + Duration::from_secs(4)).unwrap();
        fs::write(f.dir.path().join("bad-probe"), "").unwrap();
        monitor.tick(&f.home, n + Duration::from_secs(30)).unwrap();
        assert!(running("test"));
        fs::remove_file(f.dir.path().join("bad-probe")).unwrap();
        monitor.tick(&f.home, n + Duration::from_secs(60)).unwrap();
        assert!(running("test"));
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
    }
    #[test]
    fn lifecycle_shared_container_and_active_operations_are_protected() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let baked = f.home.join(".local/share/lepton/contexts/test/baked");
        fs::write(baked.join("data_overlay/system/packages.xml"),"<packages><package name=\"com.example.app\" codePath=\"/data/app/a\"/><package name=\"com.example.extra\" codePath=\"/data/app/b\"/></packages>").unwrap();
        let mut monitor = lifecycle::Monitor::default();
        let n = Instant::now();
        monitor.tick(&f.home, n).unwrap();
        fs::write(f.dir.path().join("closed-window"), "").unwrap();
        fs::write(f.dir.path().join("other-process"), "").unwrap();
        monitor.tick(&f.home, n + Duration::from_secs(1)).unwrap();
        monitor.tick(&f.home, n + Duration::from_secs(30)).unwrap();
        assert!(running("test"));
        fs::remove_file(f.dir.path().join("other-process")).unwrap();
        let lock = fs::OpenOptions::new()
            .write(true)
            .open(root(&f.home).join("operation.lock"))
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        monitor.tick(&f.home, n + Duration::from_secs(31)).unwrap();
        assert!(running("test"));
        drop(lock);
        monitor.tick(&f.home, n + Duration::from_secs(32)).unwrap();
        assert!(running("test"));
    }
    #[test]
    fn gamepad_setting_defaults_off_and_round_trips() {
        let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        assert!(!load(home).unwrap().gamepad_enabled);
        for enabled in [true, false] {
            operate(
                home,
                "gamepad.settings",
                &json!({"enabled":enabled}),
                Cancellation::default(),
                Arc::new(|_| {}),
            )
            .unwrap();
            assert_eq!(load(home).unwrap().gamepad_enabled, enabled);
        }
        assert!(operate(
            home,
            "gamepad.settings",
            &json!({"enabled":"true"}),
            Cancellation::default(),
            Arc::new(|_| {})
        )
        .is_err());
        assert!(!load(home).unwrap().gamepad_enabled);
        for source in ["frame", "steam"] {
            operate(
                home,
                "gamepad.settings",
                &json!({"enabled":false,"source":source,"rumble":false}),
                Cancellation::default(),
                Arc::new(|_| {}),
            )
            .unwrap();
            let saved = load(home).unwrap();
            assert_eq!(saved.gamepad_source, source);
            assert!(!saved.gamepad_rumble);
        }
        for invalid in [
            json!({"enabled":true,"source":"physical"}),
            json!({"enabled":true,"source":7}),
            json!({"enabled":true,"rumble":"yes"}),
        ] {
            assert!(operate(
                home,
                "gamepad.settings",
                &invalid,
                Cancellation::default(),
                Arc::new(|_| {})
            )
            .is_err());
            assert!(!load(home).unwrap().gamepad_enabled);
        }
        fs::write(root(home).join("state.json"), "{\"records\":{}}").unwrap();
        assert!(!load(home).unwrap().gamepad_enabled);
        assert_eq!(load(home).unwrap().gamepad_source, "steam");
        assert!(load(home).unwrap().gamepad_rumble);
    }
    #[test]
    fn gamepad_foreground_does_not_match_another_package() {
        assert!(gamepad_foreground(
            "mResumedActivity: ActivityRecord{abc u0 com.example.game/.Main t12}",
            "com.example.game"
        ));
        assert!(gamepad_foreground(
            "topResumedActivity=ActivityRecord{abc u0 com.example.game/.Main t12}",
            "com.example.game"
        ));
        assert!(!gamepad_foreground(
            "mResumedActivity: ActivityRecord{abc u0 com.example.game.other/.Main t12}",
            "com.example.game"
        ));
        assert!(!gamepad_foreground(
            "mLastPausedActivity: ActivityRecord{abc u0 com.example.game/.Main t12}",
            "com.example.game"
        ));
    }
    #[test]
    fn gamepad_tracks_custom_processes_by_package() {
        assert!(gamepad_present(
            "packageList={com.example.other, com.example.game}",
            "com.example.game"
        ));
        assert!(!gamepad_present(
            "packageList={com.example.game.other}",
            "com.example.game"
        ));
        assert!(!gamepad_present("No running processes", "com.example.game"));
    }
    #[test]
    fn gamepad_boot_mounts_only_its_node_and_injects_android_claim() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("vendor/liblepton");
        fs::create_dir_all(&library).unwrap();
        fs::write(library.join("liblepton.sh"),r#"
function props_file() { echo "$TEST_PROPS"; }
function prefix() { echo "$TEST_PREFIX"; }
function app_wants_flatscreen() { return 0; }
function setup_props() { echo waydroid.background_start=false > "$(props_file)"; }
function setup_podman_mounts() { :; }
function podman_mount_entry() { printf '%s|%s|%s\n' "$1" "$2" "$3" >> "$TEST_MOUNTS"; }
function generate_zygote_launch_rc() { printf 'service zygote /system/bin/app_process64\n    class main\n' > "$(prefix)/init.zygote64.rc"; }
"#).unwrap();
        let script = direct_launch_script(
            r#"#!/bin/bash
set -euo pipefail
SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
source "${SCRIPT_DIR}/liblepton/liblepton.sh"
setup_podman_mounts
generate_zygote_launch_rc
setup_props
"#,
        )
        .unwrap();
        let path = dir.path().join("start.sh");
        fs::write(&path, script).unwrap();
        let status = Command::new("bash")
            .arg(&path)
            .env("FRAMELY_LEPTON_DIR", library.parent().unwrap())
            .env("TEST_PROPS", dir.path().join("props"))
            .env("TEST_PREFIX", dir.path())
            .env("TEST_MOUNTS", dir.path().join("mounts"))
            .env("FRAMELY_GAMEPAD_EVENT", "/dev/input/event17")
            .env("FRAMELY_GAMEPAD_GRAB", "/runtime/grab.so")
            .env("FRAMELY_GAMEPAD_LAYOUT", "/runtime/gamepad.kl")
            .env("FRAMELY_GAMEPAD_READY", "/private/gamepad")
            .env("FRAMELY_GAMEPAD_TOKEN", "token")
            .status()
            .unwrap();
        assert!(status.success());
        let mounts = fs::read_to_string(dir.path().join("mounts")).unwrap();
        assert_eq!(mounts.lines().count(), 4);
        assert!(mounts
            .contains("/runtime/gamepad.kl|/system/usr/keylayout/Vendor_0001_Product_f001.kl|ro"));
        assert!(mounts.contains("/dev/input/event17|/dev/input/event250|rw"));
        assert!(!mounts.contains("/dev/input|"));
        assert!(fs::read_to_string(dir.path().join("init.zygote64.rc"))
            .unwrap()
            .contains("setenv LD_PRELOAD /vendor/lib64/libframely_gamepad_grab.so"));
        assert!(fs::read_to_string(dir.path().join("props"))
            .unwrap()
            .contains("framely.gamepad.bridge=token"));
    }
    #[test]
    fn direct_boot_preserves_context_and_controls_window_visibility() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("vendor with spaces/liblepton");
        fs::create_dir_all(&library).unwrap();
        fs::write(
            library.join("liblepton.sh"),
            r#"
function props_file() { printf '%s' "$TEST_PROPS"; }
function is_app() { return 1; }
function setup_props() {
    printf 'retained.context=%s\n' "$2" > "$(props_file)"
    if app_wants_flatscreen; then
        printf 'waydroid.background_start=false\nlepton.headless=false\n' >> "$(props_file)"
    else
        printf 'lepton.headless=true\n' >> "$(props_file)"
    fi
}
"#,
        )
        .unwrap();
        let script = direct_launch_script(
            r#"#!/bin/bash
set -euo pipefail
SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
source "${SCRIPT_DIR}/liblepton/liblepton.sh"
if is_app; then exit 99; fi
setup_props "$@"
"#,
        )
        .unwrap();
        let entry = dir.path().join("launch.sh");
        fs::write(&entry, script).unwrap();
        let props = dir.path().join("props");
        for visible in [true, false] {
            let status = Command::new("bash")
                .arg(&entry)
                .args(["start", "existing-context"])
                .env("FRAMELY_LEPTON_DIR", library.parent().unwrap())
                .env("TEST_PROPS", &props)
                .env("APP_WANTS_FLATSCREEN", visible.to_string())
                .status()
                .unwrap();
            assert!(status.success());
            let props = fs::read_to_string(&props).unwrap();
            assert!(props.contains("retained.context=existing-context"));
            assert!(props.contains("waydroid.active_apps=none"));
            assert!(props.contains(if visible {
                "waydroid.background_start=true"
            } else {
                "lepton.headless=true"
            }));
            assert!(!props.contains("waydroid.background_start=false"));
        }
        for (orientation, dimensions) in [
            (
                "portrait",
                "persist.waydroid.width=1080\npersist.waydroid.height=1920",
            ),
            (
                "landscape",
                "persist.waydroid.width=1920\npersist.waydroid.height=1080",
            ),
        ] {
            assert!(Command::new("bash")
                .arg(&entry)
                .args(["start", "existing-context"])
                .env("FRAMELY_LEPTON_DIR", library.parent().unwrap())
                .env("TEST_PROPS", &props)
                .env("APP_WANTS_FLATSCREEN", "true")
                .env("FRAMELY_BACKGROUND_BOOT", "false")
                .env("FRAMELY_WINDOW_ORIENTATION", orientation)
                .status()
                .unwrap()
                .success());
            let output = fs::read_to_string(&props).unwrap();
            assert!(output.contains(dimensions));
            assert!(output.contains("waydroid.background_start=false"));
            assert!(!output.contains("waydroid.active_apps=none"));
        }
        assert!(direct_launch_script("unrecognized future entrypoint").is_err());
    }
    #[test]
    fn direct_boot_preserves_downloaded_media_and_migrates_external_storage() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("lepton/liblepton");
        fs::create_dir_all(&library).unwrap();
        fs::write(library.join("liblepton.sh"),r#"
function data_mount_path() { echo "$TEST_DATA"; }
function app_wants_flatscreen() { return 0; }
function setup_props() { :; }
function props_file() { echo "$TEST_PROPS"; }
function setup_mounts() {
    if [[ -d "${STEAM_COMPAT_DATA_PATH:-}" ]]; then mkdir -p "$STEAM_COMPAT_DATA_PATH/external"; fi
    rm -rf "$(data_mount_path)/media/0"
    mkdir -p "$(data_mount_path)/media"
    if [[ -d "${STEAM_COMPAT_DATA_PATH:-}" ]]; then ln -s "$STEAM_COMPAT_DATA_PATH/external" "$(data_mount_path)/media/0"; else mkdir -p "$(data_mount_path)/media/0"; fi
}
function setup_podman_mounts() { setup_mounts; }
"#).unwrap();
        let source = r#"#!/bin/bash
set -euo pipefail
SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
source "${SCRIPT_DIR}/liblepton/liblepton.sh"
setup_podman_mounts
"#;
        let entry = dir.path().join("entry.sh");
        fs::write(&entry, direct_launch_script(source).unwrap()).unwrap();
        let data = dir.path().join("data");
        let marker = data.join("media/0/Android/data/test/files/resource");
        fs::create_dir_all(marker.parent().unwrap()).unwrap();
        fs::write(&marker, "downloaded resource").unwrap();
        let run = |compat: Option<&Path>| {
            let mut c = Command::new("bash");
            c.arg(&entry)
                .env("FRAMELY_LEPTON_DIR", library.parent().unwrap())
                .env("TEST_DATA", &data)
                .env("TEST_PROPS", dir.path().join("props"))
                .env_remove("STEAM_COMPAT_DATA_PATH");
            if let Some(p) = compat {
                c.env("STEAM_COMPAT_DATA_PATH", p);
            }
            c.status().unwrap()
        };
        let part = marker.with_file_name("resource.part");
        let checkpoint = marker.with_file_name("update.checkpoint");
        fs::write(&part, b"unfinished download").unwrap();
        fs::write(&checkpoint, b"offset=19").unwrap();
        assert!(run(None).success());
        assert!(run(None).success());
        assert_eq!(fs::read(&part).unwrap(), b"unfinished download");
        assert_eq!(fs::read(&checkpoint).unwrap(), b"offset=19");
        assert_eq!(fs::read_to_string(&marker).unwrap(), "downloaded resource");
        let compat = dir.path().join("compat");
        fs::create_dir(&compat).unwrap();
        assert!(run(Some(&compat)).success());
        assert!(run(Some(&compat)).success());
        assert_eq!(fs::read_to_string(&marker).unwrap(), "downloaded resource");
        assert_eq!(
            fs::read_link(data.join("media/0")).unwrap(),
            compat.join("external")
        );
        let current = fs::read_to_string(library.join("liblepton.sh")).unwrap();
        fs::write(
            library.join("liblepton.sh"),
            current.replace("rm -rf ", "rm -rf -- "),
        )
        .unwrap();
        assert!(!run(Some(&compat)).success());
        assert_eq!(fs::read(&part).unwrap(), b"unfinished download");
        assert_eq!(fs::read(&checkpoint).unwrap(), b"offset=19");
    }
    #[test]
    fn direct_boot_binds_resources_and_reuses_shader_cache() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("lepton/liblepton");
        fs::create_dir_all(&library).unwrap();
        fs::write(
            library.join("liblepton.sh"),
            r#"
function data_mount_path() { echo "$TEST_DATA"; }
function props_file() { echo "$TEST_PROPS"; }
function setup_props() {
    printf 'mesa.shader.cache.disable=true\nmesa.shader.cache.dir=/unused\n' > "$(props_file)"
}
function podman_mount_entry() { printf '%s|%s|%s\n' "$1" "$2" "$3" >> "$TEST_MOUNTS"; }
function setup_mounts() {
    rm -rf "$(data_mount_path)/media/0"
    mkdir -p "$(data_mount_path)/media/0"
}
function setup_podman_mounts() { setup_mounts; }
"#,
        )
        .unwrap();
        let source = r#"#!/bin/bash
set -euo pipefail
SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
source "${SCRIPT_DIR}/liblepton/liblepton.sh"
setup_props
setup_podman_mounts
"#;
        let entry = dir.path().join("entry.sh");
        fs::write(&entry, direct_launch_script(source).unwrap()).unwrap();
        let baked = dir.path().join("baked");
        let data = baked.join("data_overlay");
        let resource = data.join("media/0/Android/data/game/files/download.part");
        fs::create_dir_all(resource.parent().unwrap()).unwrap();
        fs::write(&resource, b"partial download").unwrap();
        storage::prepare(&baked).unwrap();
        let shader = baked.join("shadercache/compiled");
        fs::write(&shader, b"compiled shader").unwrap();
        let props = dir.path().join("props");
        let mounts = dir.path().join("mounts");
        for _ in 0..2 {
            fs::write(&mounts, []).unwrap();
            let mut command = Command::new("bash");
            command
                .arg(&entry)
                .env("FRAMELY_LEPTON_DIR", library.parent().unwrap())
                .env("TEST_DATA", &data)
                .env("TEST_PROPS", &props)
                .env("TEST_MOUNTS", &mounts);
            storage::configure(&mut command, &baked, false);
            assert!(command.status().unwrap().success());
            assert_eq!(fs::read(&resource).unwrap(), b"partial download");
            assert_eq!(fs::read(&shader).unwrap(), b"compiled shader");
            assert_eq!(
                fs::read_link(data.join("media/0")).unwrap(),
                baked.join("external")
            );
            let mounted = fs::read_to_string(&mounts).unwrap();
            assert!(mounted.contains(&format!("{0}|{0}|rw\n", baked.join("external").display())));
            assert!(mounted.contains(&format!(
                "{}|/data/shaders|rw,U\n",
                baked.join("shadercache").display()
            )));
            let properties = fs::read_to_string(&props).unwrap();
            assert_eq!(properties.matches("mesa.shader.cache.disable=").count(), 1);
            assert!(properties.contains("mesa.shader.cache.disable=false\n"));
            assert!(properties.contains("mesa.shader.cache.dir=/data/shaders\n"));
        }
    }
    #[test]
    fn local_apk_review_snapshots_source_and_rejects_invalid_selections() {
        let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().unwrap();
        let source = home.path().join("selected.APK");
        crate::apk_metadata::fixture(&source, true, 7);
        let original = fs::read(&source).unwrap();
        let result = inspect_local(home.path(), &source, &Cancellation::default()).unwrap();
        let ticket = result["ticket"].as_str().unwrap();
        assert_eq!(fs::read(&source).unwrap(), original);
        fs::write(&source, "externally changed after review").unwrap();
        let (snapshot, metadata) = review(home.path(), ticket).unwrap();
        assert_eq!(metadata.version_code, 7);
        assert_eq!(fs::read(snapshot.join("base.apk")).unwrap(), original);
        let empty = home.path().join("empty.apk");
        fs::write(&empty, []).unwrap();
        assert!(inspect_local(home.path(), &empty, &Cancellation::default()).is_err());
        assert!(inspect_local(home.path(), home.path(), &Cancellation::default()).is_err());
        assert!(inspect_local(
            home.path(),
            Path::new("/etc/passwd"),
            &Cancellation::default()
        )
        .is_err());
        let invalid = home.path().join("invalid.apk");
        fs::write(&invalid, b"not an apk").unwrap();
        assert!(inspect_local(home.path(), &invalid, &Cancellation::default()).is_err());
        assert_eq!(
            fs::read_dir(root(home.path()).join("reviews"))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn legacy_obb_migration_preserves_resources_without_copying_apk_files() {
        let d = tempfile::tempdir().unwrap();
        let app = d.path().join("app/pkg");
        let obb = d.path().join("media/Android/obb");
        fs::create_dir_all(&app).unwrap();
        fs::create_dir_all(&obb).unwrap();
        fs::write(app.join("resource.obb"), "downloaded").unwrap();
        fs::write(app.join("base.apk"), "apk").unwrap();
        fs::create_dir(app.join("lib")).unwrap();
        std::os::unix::fs::symlink(&app, obb.join("com.example.app")).unwrap();
        let script = PRESERVE_OBB
            .replace("/data/media/0/", &format!("{}/media/", d.path().display()))
            .replace("/data/app/", &format!("{}/app/", d.path().display()));
        assert!(Command::new("bash")
            .args(["-c", &script, "test", "com.example.app", "token"])
            .status()
            .unwrap()
            .success());
        let result = obb.join("com.example.app");
        assert!(!result.is_symlink());
        assert_eq!(
            fs::read_to_string(result.join("resource.obb")).unwrap(),
            "downloaded"
        );
        assert!(!result.join("base.apk").exists());
        assert!(!result.join("lib").exists());
        assert!(app.join("base.apk").exists());
        assert!(obb
            .join("com.example.app.framely-preserve-token.legacy-link")
            .is_symlink());
    }
    static SERIAL: Mutex<()> = Mutex::new(());
    struct Fixture {
        dir: tempfile::TempDir,
        home: PathBuf,
        _guard: std::sync::MutexGuard<'static, ()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            steam_ui::TEST_RESULT.with(|p| *p.borrow_mut() = None);
            crate::process::TEST_TOOLS.with(|p| *p.borrow_mut() = None)
        }
    }
    impl Fixture {
        fn new() -> Self {
            let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
            steam_ui::TEST_RESULT.with(|p| *p.borrow_mut() = Some(json!(0x92345678u32)));
            let dir = tempfile::tempdir().unwrap();
            let home = dir.path().join("home");
            fs::create_dir_all(&home).unwrap();
            let tools = dir.path().join("tools");
            fs::create_dir(&tools).unwrap();
            let baked = home.join(".local/share/lepton/contexts/test/baked");
            fs::create_dir_all(baked.join("data_overlay/system/users/0")).unwrap();
            fs::create_dir_all(baked.join("app_overlay")).unwrap();
            fs::create_dir_all(baked.join("data_overlay/data/com.example.app/files")).unwrap();
            fs::write(
                baked.join("data_overlay/data/com.example.app/files/save"),
                "saved progress",
            )
            .unwrap();
            fs::write(baked.join("data_overlay/system/packages.xml"),"<packages><package name=\"com.example.app\" codePath=\"/data/app/xyz/com.example.app\" version=\"42\"/></packages>").unwrap();
            crate::apk_metadata::fixture(&baked.join("app_overlay/base.apk"), true, 42);
            let state = dir.path().join("running");
            fs::write(&state, "true").unwrap();
            let script = format!(
                r#"#!/bin/sh
printf '%s\n' "$*" >> '{commands}'
case "$1" in
 inspect) case "$(cat '{state}')" in missing) echo 'Error: no such object: lepton-test' >&2; exit 125;; query-error) echo 'Error: storage permission denied' >&2; exit 125;; esac; case "$3" in *STEAM_COMPAT_DATA_PATH*) case "$4" in lepton-steamlaunch-*) echo "fixture|{compat}|[]";; *) echo 'fixture||[{{"Source":"{baked_path}","Destination":"/data"}}]';; esac;; *SteamBridge*) echo "fixture|{state}|$(cat '{state}')|{pid}";; *StartedAt*) echo "fixture|{state}|$(cat '{state}')";; *State.Pid*) echo "$(cat '{state}')|{pid}";; *) cat '{state}';; esac;;
 container) case "$(cat '{state}')" in missing) exit 1;; query-error) exit 125;; *) exit 0;; esac;;
 ps) if [ "$(cat '{state}')" = true ]; then case "$3" in *Pid*) echo 'lepton-test|{pid}';; *) echo lepton-test;; esac; fi;;
 stop) echo false > '{state}'; echo stopped;;
 cp) /bin/cp -f "$2" '{incoming}';;
 exec)
  if [ "$3" = cmd_real ] && [ "$4" = package ]; then shift; set -- "$1" "$2" pm "$4" "$5" "$6" "$7" "$8"; elif [ "$3" = pm ] && [ "$4" = install ]; then echo 'Failure [LEPTON_SINGLE_APP_WRAPPER]'; exit 1; fi
  if [ "$3" = pm ]; then
   case "$4" in
    list) if [ -f '{apk}' ] && [ ! -f '{broken}' ] && [ ! -f '{removed}' ]; then echo package:com.example.app; fi;;
    path) if [ "$5" = android ] || [ -f '{apk}' ]; then echo package:/data/app/xyz/com.example.app/base.apk; fi;;
    install) if [ -f '{fail}' ]; then echo 'Failure [INSTALL_FAILED_UPDATE_INCOMPATIBLE]'; else /bin/cp -f '{incoming}' '{apk}'; /bin/rm -f '{broken}' '{removed}'; echo '<package-restrictions><pkg name="com.example.app" installed="true"/></package-restrictions>' > '{restrictions}'; echo Success; fi;;
    uninstall) if [ -f '{uninstall_fail}' ]; then echo 'Failure [DELETE_FAILED_INTERNAL_ERROR]'; exit 0; fi; touch '{removed}'; /bin/rm -f '{apk}'; echo '<package-restrictions><pkg name="com.example.app" installed="false"/></package-restrictions>' > '{restrictions}'; if [ "$5" != -k ]; then /bin/rm -rf '{data}'; fi; echo Success;;
    clear) echo Success;;
   esac
  elif [ "$3" = sh ]; then
   if [ "$6" = framely-lifecycle-idle ]; then
    if [ -f '{bad_probe}' ]; then echo incomplete; exit 0; fi
    echo 'ACTIVITY MANAGER RUNNING PROCESSES (dumpsys activity processes)'
    if [ ! -f '{dead}' ]; then echo 'packageList={{com.example.app}}'; fi
    if [ -f '{other}' ]; then echo 'packageList={{com.example.extra}}'; fi
    echo FRAMELY_IDLE_DONE
   elif [ "$6" = framely-lifecycle ]; then
    echo FRAMELY_ACTIVE=com.example.app
    if [ -f '{closed}' ]; then echo FRAMELY_WINDOW=none; else echo FRAMELY_WINDOW=Waydroid; fi
    if [ -f '{bad_probe}' ]; then echo incomplete; exit 0; fi
    echo FRAMELY_PROCESSES
    echo 'ACTIVITY MANAGER RUNNING PROCESSES (dumpsys activity processes)'
    if [ ! -f '{dead}' ]; then echo 'packageList={{com.example.app}}'; fi
    if [ -f '{other}' ]; then echo 'packageList={{com.example.extra}}'; fi
    echo FRAMELY_LIFECYCLE_DONE
   elif [ "$6" = framely-launch-health ]; then
    if [ "$(cat '{state}')" != true ]; then exit 1;
    elif [ "$7" = flat ] && {{ [ -f '{dead}' ] || [ -f '{headless}' ]; }}; then echo FRAMELY_DISPLAY_MISSING;
    elif [ -f '{dead}' ] || [ -f '{custom}' ]; then echo FRAMELY_APP_MISSING;
    else echo FRAMELY_APP_PRESENT; fi
   else
    if [ -f '{not_ready}' ]; then /bin/rm -f '{not_ready}'; exit 1; fi
    if [ -f '{boot_crash}' ]; then
     echo FRAMELY_BOOT=0; echo FRAMELY_PM=0;
     if [ -f '{probe_seen}' ]; then echo false > '{state}'; else touch '{probe_seen}'; fi
    elif [ -f '{blocked_storage}' ]; then
     echo FRAMELY_BOOT=1; echo FRAMELY_PM=1; echo FRAMELY_USER=RUNNING_LOCKED; echo FRAMELY_STORAGE=0;
     if [ "$7" = package ]; then echo FRAMELY_ANDROID_READY; fi
    else echo FRAMELY_ANDROID_READY; fi;
   fi;
  elif [ "$3" = pidof ]; then
   if [ -f '{dead}' ] || {{ [ "$4" = com.example.app ] && [ -f '{custom}' ]; }} || {{ [ "$4" = surfaceflinger ] && [ -f '{headless}' ]; }}; then exit 1; else echo 1050; fi;
  elif [ "$3" = dumpsys ]; then if [ -f '{custom}' ]; then echo 'packageList={{com.example.app}}'; fi;
  elif [ "$3" = getprop ]; then
    case "$4" in
      waydroid.active_apps) if [ -f '{closed}' ]; then echo none; else echo Waydroid; fi;;
      lepton.headless) echo false;;
      framely.window_orientation) echo auto;;
      *) echo 30;;
    esac
  elif [ "$3" = cmd ]; then echo com.example.app/.Main;
  elif [ "$3" = am ]; then if [ -f '{launch_fail}' ]; then echo 'Error: launch failed'; else echo 'Status: ok'; fi;
  fi;;
esac
"#,
                pid = std::process::id(),
                compat = baked.parent().unwrap().display(),
                baked_path = baked.display(),
                bad_probe = dir.path().join("bad-probe").display(),
                other = dir.path().join("other-process").display(),
                state = state.display(),
                commands = dir.path().join("commands").display(),
                not_ready = dir.path().join("boot-not-ready").display(),
                uninstall_fail = dir.path().join("uninstall-fail").display(),
                boot_crash = dir.path().join("boot-crash").display(),
                probe_seen = dir.path().join("probe-seen").display(),
                blocked_storage = dir.path().join("blocked-storage").display(),
                launch_fail = dir.path().join("launch-fail").display(),
                dead = dir.path().join("dead-process").display(),
                custom = dir.path().join("custom-process").display(),
                headless = dir.path().join("headless-display").display(),
                closed = dir.path().join("closed-window").display(),
                incoming = dir.path().join("incoming.apk").display(),
                apk = baked.join("app_overlay/base.apk").display(),
                fail = dir.path().join("fail").display(),
                broken = dir.path().join("broken").display(),
                removed = dir.path().join("removed").display(),
                restrictions = baked
                    .join("data_overlay/system/users/0/package-restrictions.xml")
                    .display(),
                data = baked.join("data_overlay/data/com.example.app").display()
            );
            fs::write(tools.join("podman"), script).unwrap();
            fs::set_permissions(tools.join("podman"), fs::Permissions::from_mode(0o755)).unwrap();
            std::os::unix::fs::symlink("/bin/cp", tools.join("cp")).unwrap();
            let lepton = home.join(".local/share/Steam/steamapps/common/Lepton");
            fs::create_dir_all(lepton.join("liblepton")).unwrap();
            fs::write(lepton.join("liblepton/liblepton.sh"), format!("function setup_props() {{ echo waydroid.background_start=false > '{}'; if app_wants_flatscreen; then echo lepton.headless=false; else echo lepton.headless=true; fi >> '{}'; }}\nfunction app_wants_flatscreen() {{ return 0; }}\nfunction props_file() {{ echo '{}'; }}\nfunction setup_podman_mounts() {{ :; }}\nfunction podman_mount_entry() {{ :; }}\n", dir.path().join("props").display(), dir.path().join("props").display(), dir.path().join("props").display())).unwrap();
            fs::write(
                lepton.join("lepton"),
                format!(
                    r#"#!/bin/bash
SCRIPT_DIR=$( cd -- "$( dirname -- "${{BASH_SOURCE[0]}}" )" &> /dev/null && pwd )
source "${{SCRIPT_DIR}}/liblepton/liblepton.sh"
setup_props
echo true > '{}'
"#,
                    state.display()
                ),
            )
            .unwrap();
            fs::set_permissions(lepton.join("lepton"), fs::Permissions::from_mode(0o755)).unwrap();
            crate::process::TEST_TOOLS.with(|p| *p.borrow_mut() = Some(tools));
            Self {
                dir,
                home,
                _guard: guard,
            }
        }
        fn review(&self, version: u32) -> String {
            let p = self.dir.path().join("upload.apk");
            crate::apk_metadata::fixture(&p, true, version);
            inspect(&self.home, &p, &Cancellation::default()).unwrap()["ticket"]
                .as_str()
                .unwrap()
                .into()
        }
        fn operation(&self, kind: &str, p: Value) -> Result<Value> {
            operate_internal(
                &self.home,
                kind,
                &p,
                Cancellation::default(),
                Arc::new(|_| {}),
                None,
            )
        }
        fn save_file(&self) -> PathBuf {
            self.home.join(".local/share/lepton/contexts/test/baked/data_overlay/data/com.example.app/files/save")
        }
    }
    #[test]
    fn unchanged_inventory_preserves_state_file_and_changes_still_persist() {
        use std::os::unix::fs::MetadataExt;
        let f = Fixture::new();
        let initial = list(&f.home).unwrap();
        assert!(!initial["apps"].as_array().unwrap().is_empty());
        let state = init(&f.home).unwrap().join("state.json");
        let before = fs::metadata(&state).unwrap();
        let content = fs::read(&state).unwrap();
        list(&f.home).unwrap();
        let after = fs::metadata(&state).unwrap();
        assert_eq!(after.ino(), before.ino());
        assert_eq!(after.modified().unwrap(), before.modified().unwrap());
        assert_eq!(fs::read(&state).unwrap(), content);

        let mut db = load(&f.home).unwrap();
        db.gamepad_enabled = !db.gamepad_enabled;
        save(&f.home, &db).unwrap();
        assert_eq!(load(&f.home).unwrap().gamepad_enabled, db.gamepad_enabled);
        assert_ne!(fs::read(&state).unwrap(), content);
        assert_ne!(fs::metadata(&state).unwrap().ino(), before.ino());
    }
    #[test]
    fn steam_adapter_preserves_context_while_forwarding_ownership() {
        let t = tempfile::tempdir().unwrap();
        let lib = t.path().join("liblepton");
        fs::create_dir_all(&lib).unwrap();
        fs::write(lib.join("liblepton.sh"),r#"function setup_props() { printf 'context=%s\nowner=%s\n' "$LEPTON_CONTEXT" "${SteamAppId:-}" > "$TEST_PROPS"; }
function props_file() { echo "$TEST_PROPS"; }
"#).unwrap();
        let source = r#"#!/bin/bash
set -euo pipefail
LEPTON_CONTEXT="${2:-}"
if [[ -n "${SteamAppId:-}" ]]; then LEPTON_CONTEXT="steamlaunch-${SteamAppId}"; fi
SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
source "${SCRIPT_DIR}/liblepton/liblepton.sh"
setup_props
"#;
        let entry = t.path().join("entry.sh");
        fs::write(&entry, direct_launch_script(source).unwrap()).unwrap();
        let props = t.path().join("props");
        assert!(Command::new("bash")
            .arg(entry)
            .args(["start", "existing-context"])
            .env_remove("SteamAppId")
            .env("FRAMELY_STEAM_APP_ID", "2452903544")
            .env("FRAMELY_LEPTON_DIR", t.path())
            .env("TEST_PROPS", &props)
            .status()
            .unwrap()
            .success());
        let text = fs::read_to_string(props).unwrap();
        assert!(text.contains("context=existing-context"));
        assert!(text.contains("owner=2452903544"));
        assert!(!text.contains("steamlaunch-"));
    }
    fn fake_steam_registry(
        f: &Fixture,
        requests: usize,
        fail_first: bool,
    ) -> std::thread::JoinHandle<Vec<String>> {
        use std::io::BufRead;
        let steam = f.home.join(".steam");
        fs::create_dir_all(&steam).unwrap();
        fs::write(steam.join("steam.pid"), std::process::id().to_string()).unwrap();
        fs::write(steam.join("steam.token"), "abcdef0123456789").unwrap();
        let path = steam.join("steam.pipe");
        let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let fifo = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
            .unwrap();
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(fifo);
            let mut registered = std::collections::BTreeSet::<String>::new();
            let mut calls = Vec::new();
            let mut fail = fail_first;
            let deadline = Instant::now() + Duration::from_secs(15);
            while calls.len() < requests {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "Missing Steam registry request");
                        std::thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    r => {
                        r.unwrap();
                    }
                }
                let u = url::Url::parse(line.trim().strip_prefix("devkit-1 ").unwrap()).unwrap();
                let q: BTreeMap<_, _> = u.query_pairs().into_owned().collect();
                let command = u.path().trim_end_matches('/').rsplit('/').next().unwrap();
                calls.push(command.to_owned());
                let response = PathBuf::from(&q["response"]);
                // Frame's Steam client does not write Devkit replies into
                // Framely's persistent home directory. Keep this fixture
                // stricter than a generic FIFO responder.
                assert_eq!(response.parent().unwrap().parent(), Some(Path::new("/tmp")));
                assert!(response
                    .parent()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .starts_with("framely-steam-rpc-"));
                assert_eq!(
                    fs::metadata(response.parent().unwrap())
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o700
                );
                match command {
                    "create-shortcut" if fail => {
                        fail = false;
                        fs::write(response.with_extension("error"), "Steam is busy").unwrap();
                    }
                    "create-shortcut" => {
                        registered.insert(q["gameid"].clone());
                        fs::write(response, "").unwrap();
                    }
                    "delete-shortcut" => {
                        registered.remove(&q["gameid"]);
                        fs::write(response, "").unwrap();
                    }
                    "list-shortcuts" => fs::write(
                        response,
                        serde_json::to_vec(&json!({"version":2,"gameids":registered})).unwrap(),
                    )
                    .unwrap(),
                    _ => panic!("Unexpected Devkit operation"),
                }
            }
            calls
        })
    }
    #[test]
    fn transient_steam_entry_is_launch_only_protects_newer_launch_and_preserves_data() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let id = "test/com.example.app";
        let mut db = load(&f.home).unwrap();
        assert!(db.retain_steam_entries);
        assert!(
            serde_json::from_value::<Database>(json!({}))
                .unwrap()
                .retain_steam_entries
        );
        db.retain_steam_entries = false;
        save(&f.home, &db).unwrap();
        assert!(f
            .operation("steam.retention.settings", json!({"enabled":"bad"}))
            .is_err());
        fs::write(f.dir.path().join("running"), "false").unwrap();
        let server = fake_steam_registry(&f, 4, false);
        steam_shortcuts::synchronize(&f.home).unwrap();
        assert!(!load(&f.home).unwrap().records[id].steam_launch);
        assert!(steam_shortcuts::ensure_registered(&f.home, id, false).unwrap());
        let first = load(&f.home).unwrap().records[id]
            .steam_transient_generation
            .clone()
            .unwrap();
        steam_shortcuts::cleanup_transient(&f.home, id, None).unwrap();
        assert!(load(&f.home).unwrap().records[id].steam_launch); // startup grace
        assert!(steam_shortcuts::ensure_registered(&f.home, id, false).unwrap());
        let second = load(&f.home).unwrap().records[id]
            .steam_transient_generation
            .clone()
            .unwrap();
        assert_ne!(first, second);
        steam_shortcuts::cleanup_transient(&f.home, id, Some(&first)).unwrap();
        assert!(load(&f.home).unwrap().records[id].steam_launch);
        let path = root(&f.home)
            .join("steam")
            .join(format!("context-{}.lock", hash("test")));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let lock = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        steam_shortcuts::cleanup_transient(&f.home, id, Some(&second)).unwrap();
        assert!(load(&f.home).unwrap().records[id].steam_launch);
        drop(lock);
        fs::write(f.dir.path().join("running"), "true").unwrap();
        steam_shortcuts::cleanup_transient(&f.home, id, Some(&second)).unwrap();
        assert!(load(&f.home).unwrap().records[id].steam_launch);
        fs::write(f.dir.path().join("running"), "false").unwrap();
        steam_shortcuts::cleanup_transient(&f.home, id, Some(&second)).unwrap();
        let db = load(&f.home).unwrap();
        assert!(!db.records[id].steam_launch);
        assert_eq!(db.records[id].steam_preference, Some(true));
        assert!(f.home.join(".local/share/lepton/contexts/test/baked/data_overlay/data/com.example.app/files/save").exists());
        let calls = server.join().unwrap();
        assert_eq!(
            calls,
            vec![
                "list-shortcuts",
                "create-shortcut",
                "list-shortcuts",
                "delete-shortcut"
            ]
        );
    }
    #[test]
    fn automatic_steam_wrapping_defaults_on_is_idempotent_and_preserves_opt_out() {
        let f = Fixture::new();
        let id = "test/com.example.app";
        let commands = f.dir.path().join("commands");
        fs::write(&commands, "").unwrap();
        let server = fake_steam_registry(&f, 6, false);
        steam_shortcuts::synchronize(&f.home).unwrap();
        let db = load(&f.home).unwrap();
        assert!(db.records[id].steam_launch);
        assert_eq!(db.records[id].steam_preference, Some(true));
        let token = db.records[id].steam_token.clone();
        steam_shortcuts::synchronize(&f.home).unwrap();
        assert_eq!(load(&f.home).unwrap().records[id].steam_token, token);
        f.operation("steam.settings", json!({"app":id,"enabled":false}))
            .unwrap();
        steam_shortcuts::synchronize(&f.home).unwrap();
        assert!(!steam_shortcuts::ensure_registered(&f.home, id, false).unwrap());
        let calls = server.join().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|s| s.as_str() == "create-shortcut")
                .count(),
            1
        );
        let db = load(&f.home).unwrap();
        assert_eq!(db.records[id].steam_preference, Some(false));
        assert!(!db.records[id].steam_launch);
        assert!(db.records[id].steam_binding.is_none());
        assert_eq!(list(&f.home).unwrap()["apps"][0]["steamLaunch"], false);
        let text = fs::read_to_string(commands).unwrap();
        assert!(!text.contains("am start") && !text.contains("stop "));
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        assert!(running("test"));
    }
    #[test]
    fn automatic_steam_registration_failure_retries_without_losing_preference_or_data() {
        let f = Fixture::new();
        let id = "test/com.example.app";
        let server = fake_steam_registry(&f, 5, true);
        steam_shortcuts::synchronize(&f.home).unwrap();
        let db = load(&f.home).unwrap();
        assert_eq!(db.records[id].steam_preference, Some(true));
        assert!(!db.records[id].steam_launch);
        assert!(db.records[id].steam_registration_error.is_some());
        let token = db.records[id].steam_token.clone();
        steam_shortcuts::synchronize(&f.home).unwrap();
        server.join().unwrap();
        let db = load(&f.home).unwrap();
        assert!(db.records[id].steam_launch);
        assert!(db.records[id].steam_registration_error.is_none());
        assert_eq!(db.records[id].steam_token, token);
        assert!(running("test"));
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
    }
    #[test]
    fn steam_registration_uses_devkit_ipc_and_preserves_existing_context() {
        use std::io::BufRead;
        let f = Fixture::new();
        let id = "test/com.example.app";
        let mut db = load(&f.home).unwrap();
        let (a, _) = app(&f.home, &db, id).unwrap();
        record_app(&mut db, &a);
        let steam = f.home.join(".steam");
        fs::create_dir_all(&steam).unwrap();
        fs::write(steam.join("steam.pid"), std::process::id().to_string()).unwrap();
        fs::write(steam.join("steam.token"), "abcdef0123456789").unwrap();
        let pipe = steam.join("steam.pipe");
        let c = std::ffi::CString::new(pipe.to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        let fifo = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pipe)
            .unwrap();
        let expected = steam_shortcuts::game_id(id);
        let expected2 = expected.clone();
        let thread = std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(fifo);
            for _ in 0..5 {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let url = url::Url::parse(line.trim().strip_prefix("devkit-1 ").unwrap()).unwrap();
                let q: BTreeMap<_, _> = url.query_pairs().into_owned().collect();
                let response = &q["response"];
                if url.path().contains("create-shortcut") || url.path().contains("delete-shortcut")
                {
                    assert_eq!(q["gameid"], expected2);
                    fs::write(response, "").unwrap();
                } else {
                    fs::write(
                        response,
                        serde_json::to_vec(&json!({"version":2,"gameids":[expected2]})).unwrap(),
                    )
                    .unwrap();
                }
            }
        });
        let rec = db.records.get_mut(id).unwrap();
        steam_shortcuts::register(&f.home, &a, rec).unwrap();
        let token = rec.steam_token.clone();
        steam_shortcuts::register(&f.home, &a, rec).unwrap();
        steam_shortcuts::unregister(&f.home, rec).unwrap();
        thread.join().unwrap();
        assert!(!f
            .home
            .join("devkit-game")
            .join(format!("{expected}-argv.json"))
            .exists());
        assert_eq!(token, rec.steam_token);
        assert_eq!(rec.steam_binding.as_ref().unwrap().game_id, expected);
        assert_eq!(rec.context, "test");
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        assert!(!f.home.join(".local/share/applications").exists());
    }
    #[test]
    fn steam_container_probe_distinguishes_removed_stopped_and_query_failures() {
        let f = Fixture::new();
        let db = load(&f.home).unwrap();
        let (_, c) = app(&f.home, &db, "test/com.example.app").unwrap();
        fs::write(f.dir.path().join("commands"), "").unwrap();
        assert!(steam_bridge::current_instance(&c).unwrap().is_some());
        for state in ["false", "missing"] {
            fs::write(f.dir.path().join("running"), state).unwrap();
            assert!(steam_bridge::current_instance(&c).unwrap().is_none());
        }
        fs::write(f.dir.path().join("running"), "query-error").unwrap();
        assert!(steam_bridge::current_instance(&c).is_err());
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        let commands = fs::read_to_string(f.dir.path().join("commands")).unwrap();
        assert!(!commands
            .lines()
            .any(|line| line.starts_with("exec ") || line.starts_with("stop ")));
    }
    #[test]
    fn steam_wrapper_launch_and_stop_preserve_data_and_reject_unowned_requests() {
        let f = Fixture::new();
        let id = "test/com.example.app";
        let mut db = load(&f.home).unwrap();
        let (a, _) = app(&f.home, &db, id).unwrap();
        record_app(&mut db, &a);
        let rec = db.records.get_mut(id).unwrap();
        rec.steam_launch = true;
        rec.steam_token = Some("test-token".into());
        rec.steam_binding = Some(steam_shortcuts::Binding {
            game_id: steam_shortcuts::game_id(id),
            native: false,
            revision: String::new(),
        });
        save(&f.home, &db).unwrap();
        let path = steam_shortcuts::wrapper(&f.home, id);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "fixture").unwrap();
        assert!(steam_bridge::test_request(&f.home, id, "wrong", "start").is_err());
        // A direct-running container must never be restarted to obtain Steam ownership.
        assert!(steam_bridge::test_request(&f.home, id, "test-token", "start").is_err());
        fs::write(f.dir.path().join("running"), "query-error").unwrap();
        assert!(steam_bridge::test_request(&f.home, id, "test-token", "start").is_err());
        // Lepton --rm leaves no Podman object after shutdown. Cold Steam launch
        // must work without requiring a previously created/stopped object.
        fs::write(f.dir.path().join("running"), "missing").unwrap();
        assert_eq!(
            steam_bridge::test_request(&f.home, id, "test-token", "start").unwrap()["started"],
            true
        );
        assert_eq!(
            load(&f.home).unwrap().records[id].steam_app_id,
            Some(0x92345678)
        );
        assert_eq!(
            steam_bridge::test_request(&f.home, id, "test-token", "status").unwrap()["owned"],
            true
        );
        assert!(steam_bridge::test_request(&f.home, id, "test-token", "start").is_err());
        fs::write(f.dir.path().join("commands"), "").unwrap();
        assert_eq!(
            steam_bridge::test_request(&f.home, id, "test-token", "focus").unwrap()["reused"],
            true
        );
        assert!(!fs::read_to_string(f.dir.path().join("commands"))
            .unwrap()
            .contains("stop "));
        // Unknown lifecycle output refuses a destructive stop.
        fs::write(f.dir.path().join("bad-probe"), "").unwrap();
        assert!(steam_bridge::test_request(&f.home, id, "test-token", "stop").is_err());
        assert!(running("test"));
        fs::remove_file(f.dir.path().join("bad-probe")).unwrap();
        fs::write(f.dir.path().join("dead-process"), "").unwrap();
        steam_bridge::test_request(&f.home, id, "test-token", "stop").unwrap();
        assert!(!running("test"));
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        assert!(f
            .home
            .join(".local/share/lepton/contexts/test/baked/app_overlay/base.apk")
            .exists());
    }
    #[test]
    fn steam_stop_protects_shared_apps_and_ignores_a_new_container_instance() {
        let f = Fixture::new();
        let id = "test/com.example.app";
        let mut db = load(&f.home).unwrap();
        let (a, _) = app(&f.home, &db, id).unwrap();
        record_app(&mut db, &a);
        db.records.get_mut(id).unwrap().steam_token = Some("token".into());
        save(&f.home, &db).unwrap();
        steam_bridge::test_adopt(&f.home, id, "token").unwrap();
        let xml = f
            .home
            .join(".local/share/lepton/contexts/test/baked/data_overlay/system/packages.xml");
        let text = fs::read_to_string(&xml).unwrap().replace(
            "</packages>",
            "<package name=\"com.example.extra\" codePath=\"/data/app/extra\"/></packages>",
        );
        fs::write(xml, text).unwrap();
        fs::write(f.dir.path().join("other-process"), "").unwrap();
        fs::write(f.dir.path().join("dead-process"), "").unwrap();
        steam_bridge::test_request(&f.home, id, "token", "stop").unwrap();
        assert!(running("test"));
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        steam_bridge::test_adopt(&f.home, id, "token").unwrap();
        let tool = f.dir.path().join("tools/podman");
        let script = fs::read_to_string(&tool)
            .unwrap()
            .replace("fixture|", "new-instance|");
        fs::write(&tool, script).unwrap();
        let commands = f.dir.path().join("commands");
        fs::write(&commands, "").unwrap();
        assert_eq!(
            steam_bridge::test_request(&f.home, id, "token", "status").unwrap()["owned"],
            false
        );
        steam_bridge::test_request(&f.home, id, "token", "stop").unwrap();
        assert!(running("test"));
        assert!(!fs::read_to_string(commands).unwrap().contains("force-stop"));
    }
    #[test]
    fn application_lookup_does_not_query_other_containers() {
        let f = Fixture::new();
        let contexts = f.home.join(".local/share/lepton/contexts");
        fs::create_dir_all(contexts.join("other")).unwrap();
        copy(
            &contexts.join("test/baked"),
            &contexts.join("other/baked"),
            &f.dir.path().join("copy.log"),
        )
        .unwrap();
        let commands = f.dir.path().join("commands");
        fs::write(&commands, "").unwrap();
        let (a, c) = app(&f.home, &load(&f.home).unwrap(), "test/com.example.app").unwrap();
        assert_eq!(a.context, "test");
        assert_eq!(c.name, "test");
        let trace = fs::read_to_string(commands).unwrap();
        assert!(trace.contains("exec lepton-test pm list"));
        assert!(!trace.contains("exec lepton-other"), "{trace}");
        assert!(
            !trace.lines().any(|line| line.starts_with("inspect ")),
            "{trace}"
        );
    }
    #[test]
    fn stale_podman_running_state_does_not_disable_installed_apps() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let path = f.dir.path().join("tools/podman");
        let script = fs::read_to_string(&path)
            .unwrap()
            .replace(&format!("|{}", std::process::id()), "|0");
        fs::write(&path, script).unwrap();
        assert!(!running("test"));
        let cs = containers(&f.home, &load(&f.home).unwrap());
        assert!(!cs.iter().find(|c| c.id == "test").unwrap().running);
        let data = list(&f.home).unwrap();
        assert_eq!(data["apps"][0]["stateKnown"], true);
        assert_eq!(data["apps"][0]["installed"], true);
        assert!(data["warnings"].as_array().unwrap().is_empty());
        assert!(launcher(&f.home)[0]["launchUnavailable"].is_null());
        assert!(f.save_file().exists());
        let log = f.dir.path().join("commands");
        fs::write(&log, "").unwrap();
        f.operation("launch", json!({"app":"test/com.example.app"}))
            .unwrap();
        // The fake stale runtime stays dead, but launch must attempt boot rather
        // than waiting for an already-exited Android instance.
        assert!(f.dir.path().join("props").exists());
        assert!(f.save_file().exists());
    }
    #[test]
    fn container_discovery_falls_back_if_bulk_status_query_fails() {
        let f = Fixture::new();
        let tool = f.dir.path().join("tools/podman");
        let script = fs::read_to_string(&tool)
            .unwrap()
            .replace(" ps) if", " ps) exit 1; if");
        fs::write(&tool, script).unwrap();
        let cs = containers(&f.home, &load(&f.home).unwrap());
        assert!(cs.iter().any(|c| c.id == "test" && c.running));
        let trace = fs::read_to_string(f.dir.path().join("commands")).unwrap();
        assert!(
            trace.lines().any(|line| line.starts_with("inspect ")),
            "{trace}"
        );
    }
    #[test]
    fn restore_refuses_ambiguous_or_escaping_live_apk_without_replacing_data() {
        let f = Fixture::new();
        let (_, c) = app(&f.home, &load(&f.home).unwrap(), "test/com.example.app").unwrap();
        let persistent = c
            .baked
            .join("data_overlay/app/xyz/com.example.app/base.apk");
        fs::create_dir_all(persistent.parent().unwrap()).unwrap();
        crate::apk_metadata::fixture_named(&persistent, true, 42, "com.example.other");
        let log = f.dir.path().join("repair.log");
        let commands = f.dir.path().join("commands");
        fs::write(&commands, "").unwrap();
        assert!(restore_package_mount(&f.home, &c, &log).is_err());
        fs::remove_file(&persistent).unwrap();
        let external = f.dir.path().join("outside.apk");
        crate::apk_metadata::fixture(&external, true, 42);
        std::os::unix::fs::symlink(&external, &persistent).unwrap();
        assert!(restore_package_mount(&f.home, &c, &log).is_err());
        assert!(!fs::read_to_string(commands)
            .unwrap()
            .contains("package install"));
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        assert!(external.is_file());
    }
    #[test]
    fn restore_reuses_live_apk_even_with_stale_legacy_source_and_newer_version() {
        let f = Fixture::new();
        let baked = f.home.join(".local/share/lepton/contexts/test/baked");
        let persistent = baked.join("data_overlay/app/xyz/com.example.app/base.apk");
        fs::create_dir_all(persistent.parent().unwrap()).unwrap();
        crate::apk_metadata::fixture(&persistent, true, 43);
        // Discovery still sees the old APK through its shared mount.
        fs::write(baked.join("data_overlay/system/packages.xml"),
            "<packages><package name=\"com.example.app\" codePath=\"/data/steam_app\" version=\"42\"/></packages>").unwrap();
        let db = load(&f.home).unwrap();
        let (_, c) = app(&f.home, &db, "test/com.example.app").unwrap();
        let commands = f.dir.path().join("commands");
        fs::write(&commands, "").unwrap();
        let log = f.dir.path().join("repair.log");
        for _ in 0..2 {
            restore_package_mount(&f.home, &c, &log).unwrap();
        }
        let trace = fs::read_to_string(&commands).unwrap();
        assert!(!trace.contains("package install"), "{trace}");
        assert!(
            !trace.lines().any(|line| line.starts_with("cp ")),
            "{trace}"
        );
        assert_eq!(cached_metadata(&persistent).unwrap().version_code, 43);
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        // A missing APK must still trigger recovery, despite a registered path.
        fs::remove_file(&persistent).unwrap();
        restore_package_mount(&f.home, &c, &log).unwrap();
        assert!(fs::read_to_string(&commands)
            .unwrap()
            .contains("package install"));
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
    }
    #[test]
    fn starting_existing_container_waits_for_android_before_package_repair() {
        let f = Fixture::new();
        let db = load(&f.home).unwrap();
        let (_, c) = app(&f.home, &db, "test/com.example.app").unwrap();
        fs::write(f.dir.path().join("boot-not-ready"), "").unwrap();
        start(&f.home, &c, None, &f.dir.path().join("launch.log")).unwrap();
        let commands = fs::read_to_string(f.dir.path().join("commands")).unwrap();
        let probes: Vec<_> = commands
            .match_indices("framely-android-ready full")
            .collect();
        assert_eq!(probes.len(), 2);
        let repair = commands
            .find("exec lepton-test cmd_real package install")
            .unwrap();
        assert!(repair > probes[1].0);
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
    }
    #[test]
    fn installation_display_mode_is_validated_and_saved_after_success() {
        let f = Fixture::new();
        let id = "test/com.example.app";
        assert!(f
            .operation("install", json!({"approve":true,"showWindow":"vr"}))
            .is_err());
        assert!(load(&f.home).unwrap().records.is_empty());
        for (version, show) in [(43, false), (44, true)] {
            let ticket = f.review(version);
            f.operation(
                "install",
                json!({"ticket":ticket,"context":"test","app":id,"approve":true,"showWindow":show}),
            )
            .unwrap();
            assert_eq!(load(&f.home).unwrap().records[id].show_window, Some(show));
            assert!(
                !running("test"),
                "installation must finish its headless runtime"
            );
            assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
            let props = fs::read_to_string(f.dir.path().join("props")).unwrap();
            assert!(props.contains("lepton.headless=true"));
            assert!(props.contains("waydroid.background_start=true"));
            assert!(props.contains("waydroid.active_apps=none"));
        }
        let ticket = f.review(45);
        fs::write(f.dir.path().join("fail"), "").unwrap();
        assert!(f
            .operation(
                "install",
                json!({"ticket":ticket,"context":"test","app":id,"approve":true,"showWindow":false})
            )
            .is_err());
        assert_eq!(load(&f.home).unwrap().records[id].show_window, Some(true));
    }
    #[test]
    fn launch_reveals_only_the_successful_target_and_respects_vr_override() {
        let f = Fixture::new();
        let db = load(&f.home).unwrap();
        let (mut a, c) = app(&f.home, &db, "test/com.example.app").unwrap();
        let commands = f.dir.path().join("commands");
        let log = f.dir.path().join("launch.log");
        a.show_window = Some(true);
        launch(&c, &a, &log).unwrap();
        let trace = fs::read_to_string(&commands).unwrap();
        let start = trace
            .find("am start -W --user 0 --windowingMode 1 -n com.example.app/")
            .unwrap();
        let reveal = trace.find("setprop waydroid.active_apps Waydroid").unwrap();
        assert!(start < reveal);
        fs::write(&commands, "").unwrap();
        a.show_window = Some(false);
        launch(&c, &a, &log).unwrap();
        assert!(fs::read_to_string(&commands)
            .unwrap()
            .contains("setprop waydroid.active_apps none"));
        assert!(!fs::read_to_string(&commands)
            .unwrap()
            .contains("--windowingMode"));
        fs::write(&commands, "").unwrap();
        a.show_window = Some(true);
        fs::write(f.dir.path().join("launch-fail"), "").unwrap();
        assert!(launch(&c, &a, &log).is_err());
        assert!(!fs::read_to_string(&commands)
            .unwrap()
            .contains("waydroid.active_apps"));
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
    }
    #[test]
    fn successful_activity_start_with_dead_display_is_reported_as_failure() {
        let f = Fixture::new();
        let db = load(&f.home).unwrap();
        let (a, c) = app(&f.home, &db, "test/com.example.app").unwrap();
        fs::write(f.dir.path().join("dead-process"), "").unwrap();
        let error = launch(&c, &a, &f.dir.path().join("launch.log")).unwrap_err();
        assert!(format!("{error:#}").contains("Android display service exited"));
        fs::remove_file(f.dir.path().join("dead-process")).unwrap();
        fs::write(f.dir.path().join("running"), "false").unwrap();
        let error = launch(&c, &a, &f.dir.path().join("launch.log")).unwrap_err();
        assert!(error
            .to_string()
            .contains("Lepton exited shortly after launch"));
    }
    #[test]
    fn launching_an_already_visible_app_reuses_running_container() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let id = "test/com.example.app";
        let media = f.home.join(".local/share/lepton/contexts/test/baked/data_overlay/media/0/Android/data/com.example.app/files/update.part");
        fs::create_dir_all(media.parent().unwrap()).unwrap();
        fs::write(&media, "partial download").unwrap();
        f.operation("launch", json!({"app":id})).unwrap();
        let trace = fs::read_to_string(f.dir.path().join("commands")).unwrap();
        assert!(
            !trace.lines().any(|line| line.starts_with("stop ")),
            "{trace}"
        );
        assert!(trace.contains("setprop waydroid.active_apps Waydroid"));
        assert_eq!(fs::read_to_string(media).unwrap(), "partial download");
    }
    #[test]
    fn hidden_application_window_reuses_running_container_and_reports_handoff_before_checks() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        fs::write(f.dir.path().join("closed-window"), "").unwrap();
        let commands = f.dir.path().join("commands");
        fs::write(&commands, "").unwrap();
        f.operation("launch", json!({"app":"test/com.example.app"}))
            .unwrap();
        let trace = fs::read_to_string(&commands).unwrap();
        assert!(
            !trace.lines().any(|line| line.starts_with("stop ")),
            "{trace}"
        );
        assert!(trace.contains("setprop waydroid.active_apps Waydroid"));
        let (a, c) = app(&f.home, &load(&f.home).unwrap(), "test/com.example.app").unwrap();
        fs::write(&commands, "").unwrap();
        let mut handed_off = false;
        launch_with_started(&c, &a, &f.dir.path().join("launch.log"), || {
            let trace = fs::read_to_string(&commands).unwrap();
            assert!(trace.contains("setprop waydroid.active_apps Waydroid"));
            assert!(!trace.contains("framely-launch-health"));
            handed_off = true;
        })
        .unwrap();
        assert!(handed_off);
        assert!(fs::read_to_string(&commands)
            .unwrap()
            .contains("framely-launch-health"));
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        fs::write(f.dir.path().join("launch-fail"), "").unwrap();
        launch_with_started(&c, &a, &f.dir.path().join("launch.log"), || {
            panic!("Failed launch must not hand off focus")
        })
        .unwrap_err();
    }
    #[test]
    fn launch_health_supports_custom_processes_and_headless_vr() {
        let f = Fixture::new();
        let db = load(&f.home).unwrap();
        let (mut a, c) = app(&f.home, &db, "test/com.example.app").unwrap();
        fs::write(f.dir.path().join("custom-process"), "").unwrap();
        launch(&c, &a, &f.dir.path().join("launch.log")).unwrap();
        fs::write(f.dir.path().join("headless-display"), "").unwrap();
        a.show_window = Some(false);
        launch(&c, &a, &f.dir.path().join("launch.log")).unwrap();
        a.show_window = Some(true);
        assert!(launch(&c, &a, &f.dir.path().join("launch.log")).is_err());
    }
    #[test]
    fn container_cleanup_removes_owned_artifacts_and_preserves_backups_and_other_apps() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let id = "test/com.example.app";
        let key = hash(id);
        let mut db = load(&f.home).unwrap();
        let rec = db.records.get_mut(id).unwrap();
        rec.steam_app_id = Some(0x92345678);
        rec.steam_binding = Some(steam_shortcuts::Binding {
            game_id: steam_shortcuts::game_id(id),
            native: true,
            revision: String::new(),
        });
        let mut other = rec.clone();
        other.id = "other/com.example.app".into();
        other.context = "other".into();
        other.steam_binding = None;
        db.records.insert(other.id.clone(), other.clone());
        let r = init(&f.home).unwrap();
        for path in [
            r.join("apks").join(&key),
            r.join("apks").join(hash(&other.id)),
            r.join("backups/keep"),
            r.join("steam/leases"),
        ] {
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("keep"), b"data").unwrap();
        }
        fs::write(r.join("steam").join(format!("native-{key}.sh")), "script").unwrap();
        fs::write(
            r.join("steam/leases").join(format!("{key}.json")),
            serde_json::to_vec(&json!({"app":id})).unwrap(),
        )
        .unwrap();
        let old_log = r.join("logs").join(format!("{key}-old.log"));
        fs::write(&old_log, "old").unwrap();
        fs::write(r.join("logs/gamepad.log"), "shared").unwrap();
        let log = r.join("logs").join(format!("{}-delete.log", hash("test")));
        fs::write(&log, "delete").unwrap();
        let registry = fake_steam_registry(&f, 1, false);
        let mut c = containers(&f.home, &db)
            .into_iter()
            .find(|c| c.id == "test")
            .unwrap();
        c.name = "steamlaunch-2452903544".into();
        cleanup::delete(&f.home, &mut db, &c, &log).unwrap();
        assert_eq!(registry.join().unwrap(), vec!["delete-shortcut"]);
        assert!(!c.baked.parent().unwrap().exists());
        assert!(!r.join("apks").join(&key).exists());
        assert!(!r.join("steam").join(format!("native-{key}.sh")).exists());
        assert!(!r.join("steam/leases").join(format!("{key}.json")).exists());
        assert!(!old_log.exists());
        assert!(r.join("backups/keep/keep").exists());
        assert!(r.join("apks").join(hash(&other.id)).join("keep").exists());
        assert!(r.join("logs/gamepad.log").exists());
        assert!(log.exists());
        assert!(r
            .join("steam")
            .join(format!("context-{}.lock", hash("test")))
            .exists());
        assert!(db.records.contains_key(&other.id));
        assert!(!db.records.contains_key(id));
        let commands = fs::read_to_string(f.dir.path().join("commands")).unwrap();
        assert!(commands.contains("lepton-test"));
        assert!(commands.contains("lepton-steamlaunch-2452903544"));
        assert_eq!(commands.lines().filter(|l| l.starts_with("rm ")).count(), 2);
    }
    #[test]
    fn container_cleanup_refuses_a_runtime_bound_to_other_storage() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let mut db = load(&f.home).unwrap();
        let c = containers(&f.home, &db)
            .into_iter()
            .find(|c| c.id == "test")
            .unwrap();
        let tool = f.dir.path().join("tools/podman");
        let source = fs::read_to_string(&tool).unwrap();
        let foreign = f.dir.path().join("foreign");
        fs::create_dir_all(foreign.join("baked")).unwrap();
        fs::write(
            &tool,
            source.replace(
                &format!("\"Source\":\"{}\"", c.baked.display()),
                &format!("\"Source\":\"{}\"", foreign.join("baked").display()),
            ),
        )
        .unwrap();
        assert!(cleanup::delete(&f.home, &mut db, &c, &f.dir.path().join("delete.log")).is_err());
        assert!(f.save_file().exists());
        assert!(running("test"));
    }
    #[test]
    fn container_cleanup_preserves_data_when_a_lease_has_another_identity() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let mut db = load(&f.home).unwrap();
        let c = containers(&f.home, &db)
            .into_iter()
            .find(|c| c.id == "test")
            .unwrap();
        let leases = root(&f.home).join("steam/leases");
        fs::create_dir_all(&leases).unwrap();
        fs::write(
            leases.join(format!("{}.json", hash("test/com.example.app"))),
            r#"{"app":"other/com.example.app"}"#,
        )
        .unwrap();
        assert!(cleanup::delete(&f.home, &mut db, &c, &f.dir.path().join("delete.log")).is_err());
        assert!(f.save_file().exists());
        assert!(running("test"));
        assert!(db.records.contains_key("test/com.example.app"));
    }
    #[test]
    fn discovery_reads_mount_layer_and_does_not_claim_existing_context() {
        let f = Fixture::new();
        let v = list(&f.home).unwrap();
        assert_eq!(v["apps"][0]["metadata"]["name"], "Example");
        assert_eq!(v["apps"][0]["installed"], true);
        assert_eq!(v["containers"][0]["managed"], false);
        assert_eq!(launcher(&f.home).len(), 1);
        assert!(f
            .operation("container.delete", json!({"context":"test"}))
            .is_err());
        assert!(f.save_file().exists());
        f.operation("container.delete", json!({"context":"test","approve":true}))
            .unwrap();
        assert!(!f.save_file().exists());
    }
    #[test]
    fn external_delete_preserves_parent_and_names_cannot_alias() {
        let f = Fixture::new();
        let existing = f.home.join(".local/share/lepton/contexts/test/baked");
        let parent = f.dir.path().join("custom");
        fs::create_dir(&parent).unwrap();
        let baked = parent.join("baked");
        copy(&existing, &baked, &f.dir.path().join("copy.log")).unwrap();
        fs::write(parent.join("keep"), "unrelated").unwrap();
        assert!(f
            .operation("root.add", json!({"path":baked,"context":"test"}))
            .is_err());
        f.operation("root.add", json!({"path":baked,"context":"custom"}))
            .unwrap();
        // The simulated custom runtime must expose its own /data mount,
        // rather than the fixture's original test container storage.
        let tool = f.dir.path().join("tools/podman");
        let source = fs::read_to_string(&tool).unwrap();
        fs::write(
            &tool,
            source.replace(
                &format!("\"Source\":\"{}\"", existing.display()),
                &format!("\"Source\":\"{}\"", baked.display()),
            ),
        )
        .unwrap();
        let context = format!("external-{}", hash(&baked.to_string_lossy()));
        f.operation(
            "container.delete",
            json!({"context":context,"approve":true}),
        )
        .unwrap();
        assert!(!baked.exists());
        assert!(parent.join("keep").exists());
        assert!(f.save_file().exists());
        assert!(load(&f.home).unwrap().roots.is_empty());
    }
    #[test]
    fn registered_external_restore_recovers_without_a_baked_directory() {
        let f = Fixture::new();
        let original = f.home.join(".local/share/lepton/contexts/test/baked");
        let parent = f.dir.path().join("external");
        fs::create_dir(&parent).unwrap();
        let baked = parent.join("baked");
        copy(&original, &baked, &f.dir.path().join("copy.log")).unwrap();
        f.operation("root.add", json!({"path":baked,"context":"external"}))
            .unwrap();
        fs::rename(&baked, parent.join("previous-test")).unwrap();
        copy(
            &parent.join("previous-test"),
            &parent.join("restore-test"),
            &f.dir.path().join("copy.log"),
        )
        .unwrap();
        fs::write(
            parent.join("framely-restore.json"),
            serde_json::to_vec(
                &json!({"old":"previous-test","replacement":"restore-test","records":[]}),
            )
            .unwrap(),
        )
        .unwrap();
        let mut db = load(&f.home).unwrap();
        recover_restores(&f.home, &mut db).unwrap();
        assert!(baked.join("app_overlay/base.apk").exists());
        assert!(!parent.join("framely-restore.json").exists());
        assert!(!parent.join("restore-test").exists());
    }
    #[test]
    fn native_launch_prepare_preserves_identity_data_and_rejects_running_or_changed_binding() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let id = "test/com.example.app";
        let mut db = load(&f.home).unwrap();
        let r = db.records.get_mut(id).unwrap();
        r.steam_launch = true;
        r.steam_app_id = Some(0x92345678);
        r.steam_binding = Some(steam_shortcuts::Binding {
            game_id: steam_shortcuts::game_id(id),
            native: true,
            revision: String::new(),
        });
        save(&f.home, &db).unwrap();
        assert!(native::prepare(&f.home, id, 0x92345678).is_err());
        fs::write(f.dir.path().join("running"), "false").unwrap();
        let lepton = runner(&f.home).unwrap();
        let source = fs::read_to_string(&lepton).unwrap();
        fs::write(
            &lepton,
            format!(
                "{source}\nfunction teardown() {{ :; }}\ncase \"${{COMMAND}}\" in\n*) :;;\nesac\n"
            ),
        )
        .unwrap();
        let baked = f.home.join(".local/share/lepton/contexts/test/baked");
        let installed_dir = baked.join("data_overlay/app/xyz/com.example.app");
        fs::create_dir_all(installed_dir.join("lib/arm64")).unwrap();
        fs::copy(
            baked.join("app_overlay/base.apk"),
            installed_dir.join("base.apk"),
        )
        .unwrap();
        fs::write(
            installed_dir.join("lib/arm64/game.so"),
            b"existing extracted library",
        )
        .unwrap();
        assert!(native::prepare(&f.home, id, 0x92345679).is_err());
        let (c, a, response, ownership) = native::prepare(&f.home, id, 0x92345678).unwrap();
        assert!(native::prepare(&f.home, id, 0x92345678).is_err());
        assert_eq!(c.id, "test");
        assert_eq!(c.name, "steamlaunch-2452903544");
        assert!(!c.steam);
        assert_eq!(a.id, id);
        assert_eq!(
            response["env"]["FRAMELY_NATIVE_APK_DIR"],
            json!(installed_dir)
        );
        assert_eq!(
            response["env"]["STEAM_COMPAT_DATA_PATH"],
            json!(baked.parent().unwrap())
        );
        assert_eq!(fs::read(f.save_file()).unwrap(), b"saved progress");
        assert_eq!(
            fs::read(installed_dir.join("lib/arm64/game.so")).unwrap(),
            b"existing extracted library"
        );
        drop(ownership);
        assert!(native::prepare(&f.home, id, 0x92345678).is_ok());

        // An idle installation runtime from an older release can be handed
        // over, but incomplete probes and another running app must not stop it.
        fs::write(f.dir.path().join("running"), "true").unwrap();
        fs::write(f.dir.path().join("dead-process"), "").unwrap();
        fs::write(f.dir.path().join("bad-probe"), "").unwrap();
        assert!(native::prepare(&f.home, id, 0x92345678).is_err());
        assert!(running("test"));
        fs::remove_file(f.dir.path().join("bad-probe")).unwrap();
        let xml = baked.join("data_overlay/system/packages.xml");
        let text = fs::read_to_string(&xml).unwrap().replace(
            "</packages>",
            "<package name=\"com.example.extra\" codePath=\"/data/app/extra\"/></packages>",
        );
        fs::write(&xml, text).unwrap();
        fs::write(f.dir.path().join("other-process"), "").unwrap();
        assert!(native::prepare(&f.home, id, 0x92345678).is_err());
        assert!(running("test"));
        fs::remove_file(f.dir.path().join("other-process")).unwrap();
        assert!(native::prepare(&f.home, id, 0x92345678).is_ok());
        assert!(!running("test"));
        assert_eq!(fs::read(f.save_file()).unwrap(), b"saved progress");

        let ticket = f.review(43);
        f.operation(
            "install",
            json!({"ticket":ticket,"context":"test","app":id,"approve":true}),
        )
        .unwrap();
        assert!(!running("test"));
        let (_, installed, _, _ownership) = native::prepare(&f.home, id, 0x92345678).unwrap();
        assert_eq!(installed.id, id);
        assert_eq!(load(&f.home).unwrap().records[id].metadata.version_code, 43);
        assert_eq!(fs::read(f.save_file()).unwrap(), b"saved progress");
    }
    #[test]
    fn unreadable_state_is_not_uninstallation_or_a_launch_target() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let packages = f
            .home
            .join(".local/share/lepton/contexts/test/baked/data_overlay/system/packages.xml");
        fs::write(&packages, [255u8]).unwrap();
        let data = list(&f.home).unwrap();
        let a = &data["apps"][0];
        assert_eq!(a["stateKnown"], false);
        assert_eq!(a["installed"], true);
        assert!(launcher(&f.home)[0]["launchUnavailable"].is_string());
        assert!(f
            .operation(
                "uninstall",
                json!({"app":"test/com.example.app","approve":true})
            )
            .is_err());
        assert!(f
            .operation(
                "clear",
                json!({"app":"test/com.example.app","approve":true})
            )
            .is_err());
        assert!(f.save_file().exists());
        assert!(!load(&f.home).unwrap().records["test/com.example.app"].removed);
    }
    #[test]
    fn shared_mount_loss_preserves_launcher_identity_until_confirmed_uninstall() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let baked = f.home.join(".local/share/lepton/contexts/test/baked");
        // Another app takes the legacy shared APK mount, while the original stays registered.
        crate::apk_metadata::fixture_named(
            &baked.join("app_overlay/base.apk"),
            true,
            1,
            "com.example.other",
        );
        for _ in 0..2 {
            list(&f.home).unwrap();
            let icons = launcher(&f.home);
            assert_eq!(icons.len(), 1);
            assert_eq!(icons[0]["id"], "test/com.example.app");
            assert!(icons[0]["launchUnavailable"].is_string());
            assert!(f
                .operation("launch", json!({"app":"test/com.example.app"}))
                .is_err());
        }
        crate::apk_metadata::fixture(&baked.join("app_overlay/base.apk"), true, 42);
        assert!(launcher(&f.home)[0]["launchUnavailable"].is_null());
        fs::write(baked.join("data_overlay/system/users/0/package-restrictions.xml"),
            r#"<package-restrictions><pkg name="com.example.app" installed="false"/></package-restrictions>"#).unwrap();
        fs::write(f.dir.path().join("running"), "false").unwrap();
        assert!(launcher(&f.home).is_empty());
    }
    #[test]
    fn existing_sideloaded_containers_do_not_accept_additional_apps() {
        let f = Fixture::new();
        let data = list(&f.home).unwrap();
        assert_eq!(data["containers"][0]["acceptsAdditionalApps"], false);
        let baked = f.home.join(".local/share/lepton/contexts/test/baked");
        let direct = baked.join("data_overlay/app/xyz/com.example.app");
        fs::create_dir_all(&direct).unwrap();
        fs::rename(baked.join("app_overlay/base.apk"), direct.join("base.apk")).unwrap();
        fs::write(f.dir.path().join("running"), "false").unwrap();
        let data = list(&f.home).unwrap();
        assert_eq!(data["containers"][0]["acceptsAdditionalApps"], false);
        assert_eq!(data["containers"][0]["managed"], false);
    }
    #[test]
    fn shared_mount_replacement_recovers_metadata_from_matching_cached_apk() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let baked = f.home.join(".local/share/lepton/contexts/test/baked");
        let apk = baked.join("app_overlay/base.apk");
        let cache = root(&f.home)
            .join("apks")
            .join(hash("test/com.example.app"));
        store_apk(&apk, &cache).unwrap();
        crate::apk_metadata::fixture_named(&apk, true, 1, "com.example.other");
        let mut db = load(&f.home).unwrap();
        db.records
            .get_mut("test/com.example.app")
            .unwrap()
            .metadata
            .activities
            .clear();
        db.records
            .get_mut("test/com.example.app")
            .unwrap()
            .metadata
            .declared_activities
            .clear();
        save(&f.home, &db).unwrap();
        let data = list(&f.home).unwrap();
        assert_eq!(
            data["apps"][0]["metadata"]["activities"][0],
            "com.example.app.Main"
        );
        assert_eq!(data["apps"][0]["installed"], true);
        fs::write(baked.join("data_overlay/system/users/0/package-restrictions.xml"), r#"<package-restrictions><pkg name="com.example.app" enabled="3"/></package-restrictions>"#).unwrap();
        assert!(launcher(&f.home)[0]["launchUnavailable"].is_string());
        // An externally upgraded package must not acquire metadata from an older cache.
        fs::write(baked.join("data_overlay/system/packages.xml"),r#"<packages><package name="com.example.app" codePath="/data/app/missing" version="43"/></packages>"#).unwrap();
        assert!(list(&f.home).unwrap()["apps"][0]["metadata"]["activities"]
            .as_array()
            .unwrap()
            .is_empty());
    }
    #[test]
    fn legacy_shared_apps_survive_restart_but_new_packages_are_rejected() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let baked = f.home.join(".local/share/lepton/contexts/test/baked");
        let mounted = baked.join("app_overlay/base.apk");
        let original = fs::read(&mounted).unwrap();
        let second = baked.join("data_overlay/app/extra/base.apk");
        fs::create_dir_all(second.parent().unwrap()).unwrap();
        let xml = f.dir.path().join("two-packages.xml");
        fs::write(&xml, r#"<packages><package name="com.example.app" codePath="/data/app/xyz/com.example.app" version="42"/><package name="com.example.extra" codePath="/data/app/extra" version="1"/></packages>"#).unwrap();
        let tools = f.dir.path().join("tools");
        fs::write(tools.join("podman"), format!(r#"#!/bin/sh
printf '%s\n' "$*" >> '{commands}'
case "$1" in
 inspect) case "$3" in *SteamBridge*) echo "fixture|{state}|$(cat '{state}')|{pid}";; *StartedAt*) echo "fixture|{state}|$(cat '{state}')";; *State.Pid*) echo "$(cat '{state}')|{pid}";; *) cat '{state}';; esac;;
 ps) if [ "$(cat '{state}')" = true ]; then case "$3" in *Pid*) echo 'lepton-test|{pid}';; *) echo lepton-test;; esac; fi;;
 stop) echo false > '{state}';;
 cp) /bin/cp -f "$2" '{incoming}';;
 exec)
  if [ "$3" = cmd_real ] && [ "$4" = package ]; then shift; set -- "$1" "$2" pm "$4" "$5" "$6" "$7" "$8"; elif [ "$3" = pm ] && [ "$4" = install ]; then echo 'Failure [LEPTON_SINGLE_APP_WRAPPER]'; exit 1; fi
  if [ "$3" = pm ]; then
   case "$4" in
    list) echo package:com.example.app; if [ -f '{second}' ]; then echo package:com.example.extra; fi;;
    path) if [ "$7" = com.example.app ]; then echo package:/data/app/xyz/com.example.app/base.apk; else echo package:/data/app/extra/base.apk; fi;;
    install) if /bin/cmp -s '{incoming}' '{mounted}'; then /bin/mkdir -p '{first_dir}'; /bin/cp -f '{incoming}' '{first}'; else /bin/cp -f '{incoming}' '{second}'; fi; /bin/cp '{xml}' '{database}'; echo Success;;
   esac
  elif [ "$3" = sh ]; then if [ "$6" = framely-launch-health ]; then echo FRAMELY_APP_PRESENT; else echo FRAMELY_ANDROID_READY; fi;
  elif [ "$3" = pidof ]; then echo 1050;
  elif [ "$3" = getprop ]; then echo 30;
  elif [ "$3" = cmd ]; then echo com.example.extra/.Main;
  elif [ "$3" = am ]; then echo 'Status: ok';
  fi;;
esac
"#, pid=std::process::id(), commands=f.dir.path().join("commands").display(), state=f.dir.path().join("running").display(),
            incoming=f.dir.path().join("incoming.apk").display(), second=second.display(), mounted=mounted.display(), first=baked.join("data_overlay/app/xyz/com.example.app/base.apk").display(), first_dir=baked.join("data_overlay/app/xyz/com.example.app").display(),
            xml=xml.display(), database=baked.join("data_overlay/system/packages.xml").display())).unwrap();
        // Simulate an older/shared installation without adding a package through
        // the new installer. Both existing apps must remain manageable.
        crate::apk_metadata::fixture_named(&second, true, 1, "com.example.extra");
        fs::copy(&xml, baked.join("data_overlay/system/packages.xml")).unwrap();
        list(&f.home).unwrap();
        let incoming = f.dir.path().join("third.apk");
        crate::apk_metadata::fixture_named(&incoming, true, 1, "com.example.third");
        let reviewed = inspect(&f.home, &incoming, &Cancellation::default()).unwrap();
        let before = fs::read(root(&f.home).join("state.json")).unwrap();
        let trace_before = fs::read_to_string(f.dir.path().join("commands")).unwrap();
        let error = f
            .operation(
                "install",
                json!({"context":"test","ticket":reviewed["ticket"],"approve":true}),
            )
            .unwrap_err();
        assert!(error.to_string().contains("own container"));
        assert_eq!(fs::read(root(&f.home).join("state.json")).unwrap(), before);
        let trace_after = fs::read_to_string(f.dir.path().join("commands")).unwrap();
        let new_calls = &trace_after[trace_before.len()..];
        assert!(
            !new_calls.contains("stop ") && !new_calls.contains("package install"),
            "{new_calls}"
        );
        assert_eq!(fs::read(&mounted).unwrap(), original);
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        assert_eq!(
            cached_metadata(&second).unwrap().package,
            "com.example.extra"
        );
        let db = load(&f.home).unwrap();
        let c = containers(&f.home, &db)
            .into_iter()
            .find(|c| c.id == "test")
            .unwrap();
        let log = f.dir.path().join("restart.log");
        stop(&c, &log).unwrap();
        // Stopped containers discover both the mounted APK and the added APK.
        assert_eq!(
            installed(
                &f.home,
                &Container {
                    running: false,
                    ..c.clone()
                }
            )
            .unwrap()
            .len(),
            2
        );
        start(&f.home, &c, Some(false), &log).unwrap();
        let data = list(&f.home).unwrap();
        for package in ["com.example.app", "com.example.extra"] {
            assert!(data["apps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a["metadata"]["package"] == package
                    && a["installed"] == true
                    && a["metadata"]["activities"]
                        .as_array()
                        .is_some_and(|a| !a.is_empty())));
        }
        f.operation("launch", json!({"app":"test/com.example.extra"}))
            .unwrap();
        assert_eq!(fs::read(&mounted).unwrap(), original);
        assert!(fs::read_to_string(f.dir.path().join("commands"))
            .unwrap()
            .contains(
            "am start -W --user 0 --windowingMode 1 -n com.example.extra/com.example.extra.Main"
        ));
    }
    #[test]
    fn review_tampering_expiry_and_cancel_are_rejected() {
        let f = Fixture::new();
        let ticket = f.review(42);
        let d = root(&f.home).join("reviews").join(&ticket);
        fs::set_permissions(d.join("base.apk"), fs::Permissions::from_mode(0o600)).unwrap();
        crate::apk_metadata::fixture(&d.join("base.apk"), true, 43);
        assert!(review(&f.home, &ticket)
            .unwrap_err()
            .to_string()
            .contains("changed"));
        let ticket = f.review(42);
        fs::write(
            root(&f.home).join("reviews").join(&ticket).join("created"),
            (now() - 901).to_string(),
        )
        .unwrap();
        assert!(review(&f.home, &ticket).is_err());
        reviews_drop(&f.home, &ticket).unwrap();
        let c = Cancellation::default();
        c.stop();
        assert!(inspect(&f.home, &f.dir.path().join("upload.apk"), &c).is_err());
    }
    #[test]
    fn window_orientation_is_validated_persisted_and_applied() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let id = "test/com.example.app";
        assert!(f
            .operation("settings", json!({"app":id,"orientation":"sideways"}))
            .is_err());
        assert!(load(&f.home).unwrap().records[id].orientation.is_none());
        f.operation("settings", json!({"app":id,"orientation":"portrait"}))
            .unwrap();
        assert_eq!(
            load(&f.home).unwrap().records[id].orientation.as_deref(),
            Some("portrait")
        );
        let (a, c) = app(&f.home, &load(&f.home).unwrap(), id).unwrap();
        assert_eq!(a.orientation.as_deref(), Some("portrait"));
        apply_orientation(&c, a.orientation.as_deref(), &f.dir.path().join("log")).unwrap();
        let trace = fs::read_to_string(f.dir.path().join("commands")).unwrap();
        assert!(trace.contains("wm size reset"));
        assert!(trace.contains("wm set-user-rotation lock 0"));
        f.operation("settings", json!({"app":id,"orientation":"auto"}))
            .unwrap();
        assert!(load(&f.home).unwrap().records[id].orientation.is_none());
    }
    #[test]
    fn update_failure_and_downgrade_preserve_package_and_data() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let ticket = f.review(41);
        assert!(f.operation("install",json!({"context":"test","app":"test/com.example.app","ticket":ticket,"approve":true})).unwrap_err().to_string().contains("downgrade"));
        let ticket = f.review(43);
        fs::write(f.dir.path().join("fail"), "").unwrap();
        assert!(f.operation("install",json!({"context":"test","app":"test/com.example.app","ticket":ticket,"approve":true})).is_err());
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        assert_eq!(
            cached_metadata(
                &f.home
                    .join(".local/share/lepton/contexts/test/baked/app_overlay/base.apk")
            )
            .unwrap()
            .version_code,
            42
        );
        assert!(clean_candidates(&f.home, &load(&f.home).unwrap())
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "backups"));
    }
    #[test]
    fn uninstall_can_delete_an_exclusive_container_and_keep_backups() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let root = init(&f.home).unwrap();
        fs::create_dir_all(root.join("backups/keep")).unwrap();
        fs::write(root.join("backups/keep/save"), "backup").unwrap();
        let result = f.operation("uninstall", json!({"app":"test/com.example.app","purge":true,"deleteContainer":true,"approve":true})).unwrap();
        assert_eq!(result["containerDeleted"], true);
        assert!(!f.save_file().exists());
        assert!(!load(&f.home)
            .unwrap()
            .records
            .contains_key("test/com.example.app"));
        assert!(root.join("backups/keep/save").exists());
        assert!(list(&f.home).unwrap()["containers"]
            .as_array()
            .unwrap()
            .is_empty());
    }
    #[test]
    fn uninstall_container_deletion_requires_data_deletion_and_confirmation() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        for params in [
            json!({"app":"test/com.example.app","deleteContainer":true,"approve":true}),
            json!({"app":"test/com.example.app","deleteContainer":true,"purge":true}),
            json!({"app":"test/com.example.app","deleteContainer":"true","purge":true,"approve":true}),
        ] {
            assert!(f.operation("uninstall", params).is_err());
            assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        }
    }
    #[test]
    fn uninstall_container_deletion_refuses_other_retained_apps() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let mut db = load(&f.home).unwrap();
        let mut other = db.records["test/com.example.app"].clone();
        other.id = "test/com.other.app".into();
        other.metadata.package = "com.other.app".into();
        other.removed = true;
        db.records.insert(other.id.clone(), other);
        save(&f.home, &db).unwrap();
        let error = f.operation("uninstall", json!({"app":"test/com.example.app","purge":true,"deleteContainer":true,"approve":true})).unwrap_err();
        assert!(
            error.to_string().contains("other applications"),
            "{error:#}"
        );
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        assert!(load(&f.home)
            .unwrap()
            .records
            .contains_key("test/com.other.app"));
    }
    #[test]
    fn retained_data_cleanup_can_delete_its_exclusive_container() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        f.operation(
            "uninstall",
            json!({"app":"test/com.example.app","approve":true}),
        )
        .unwrap();
        assert!(f.save_file().exists());
        let result = f
            .operation(
                "purge",
                json!({"app":"test/com.example.app","deleteContainer":true,"approve":true}),
            )
            .unwrap();
        assert_eq!(result["containerDeleted"], true);
        assert!(!f.save_file().exists());
    }
    #[test]
    fn retained_uninstall_does_not_wait_for_unlock_or_external_storage() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        fs::write(f.dir.path().join("blocked-storage"), "").unwrap();
        let (_, c) = app(&f.home, &load(&f.home).unwrap(), "test/com.example.app").unwrap();
        let error =
            wait_android_ready_until(&c, AndroidReadiness::Full, None, Duration::from_millis(100))
                .unwrap_err();
        assert!(error.to_string().contains("user did not unlock"), "{error}");
        f.operation(
            "uninstall",
            json!({"app":"test/com.example.app","approve":true}),
        )
        .unwrap();
        assert!(f.save_file().exists());
        assert!(load(&f.home).unwrap().records["test/com.example.app"].removed);
    }
    #[test]
    fn failed_android_start_or_uninstall_preserves_steam_bindings_and_saves() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let id = "test/com.example.app";
        let mut db = load(&f.home).unwrap();
        db.records.get_mut(id).unwrap().steam_binding = Some(steam_shortcuts::Binding {
            game_id: steam_shortcuts::game_id(id),
            native: false,
            revision: String::new(),
        });
        save(&f.home, &db).unwrap();
        let before = fs::read(root(&f.home).join("state.json")).unwrap();
        fs::write(f.dir.path().join("boot-crash"), "").unwrap();
        let error = f
            .operation("uninstall", json!({"app":id,"approve":true}))
            .unwrap_err();
        assert!(error.to_string().contains("container stopped"), "{error}");
        assert_eq!(before, fs::read(root(&f.home).join("state.json")).unwrap());
        assert!(f.save_file().exists());
        assert!(!f.dir.path().join("removed").exists());
        fs::remove_file(f.dir.path().join("boot-crash")).unwrap();
        fs::write(f.dir.path().join("running"), "true").unwrap();
        fs::write(f.dir.path().join("uninstall-fail"), "").unwrap();
        let error = f
            .operation("uninstall", json!({"app":id,"approve":true}))
            .unwrap_err();
        assert!(
            error.to_string().contains("DELETE_FAILED_INTERNAL_ERROR"),
            "{error}"
        );
        assert_eq!(before, fs::read(root(&f.home).join("state.json")).unwrap());
        assert!(f.save_file().exists());
    }
    #[test]
    fn steam_cleanup_failure_records_android_success_and_retry_does_not_uninstall_twice() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let id = "test/com.example.app";
        let mut db = load(&f.home).unwrap();
        db.records.get_mut(id).unwrap().steam_binding = Some(steam_shortcuts::Binding {
            game_id: "invalid-binding".into(),
            native: false,
            revision: String::new(),
        });
        save(&f.home, &db).unwrap();
        let error = f
            .operation("uninstall", json!({"app":id,"approve":true}))
            .unwrap_err();
        assert!(error.to_string().contains("APK was uninstalled"), "{error}");
        let mut db = load(&f.home).unwrap();
        assert!(db.records[id].removed);
        assert!(db.records[id].steam_binding.is_some());
        assert!(f.save_file().exists());
        let commands = f.dir.path().join("commands");
        let count = fs::read_to_string(&commands)
            .unwrap()
            .matches("pm uninstall")
            .count();
        db.records.get_mut(id).unwrap().steam_binding = None;
        save(&f.home, &db).unwrap();
        f.operation("uninstall", json!({"app":id,"approve":true}))
            .unwrap();
        assert_eq!(
            count,
            fs::read_to_string(&commands)
                .unwrap()
                .matches("pm uninstall")
                .count()
        );
        assert!(f.save_file().exists());
    }
    #[test]
    fn android_ready_probe_uses_operation_specific_checks_and_accepts_volume_output() {
        let dir = tempfile::tempdir().unwrap();
        let run = |mode: &str, user: &str, volume: &str| {
            let mocks = format!("getprop() {{ echo 1; }}; pm() {{ echo package:/system/framework/framework-res.apk; }}; am() {{ echo {user}; }}; sm() {{ echo '{volume}'; }};\n");
            let probe = ANDROID_READY_PROBE
                .replace("/storage/emulated/0", &dir.path().display().to_string());
            let mut command = Command::new("/bin/sh");
            command.args(["-c", &(mocks + &probe), "probe", mode]);
            output(command, Duration::from_secs(2), None).unwrap()
        };
        assert!(
            run("package", "RUNNING_LOCKED", "emulated;0 unmounted null")
                .contains("FRAMELY_ANDROID_READY")
        );
        assert!(!run("full", "RUNNING_LOCKED", "emulated;0 mounted null")
            .contains("FRAMELY_ANDROID_READY"));
        assert!(
            !run("full", "RUNNING_UNLOCKED", "emulated;0 unmounted null")
                .contains("FRAMELY_ANDROID_READY")
        );
        assert!(run("full", "RUNNING_UNLOCKED", "emulated;0 mounted null")
            .contains("FRAMELY_ANDROID_READY"));
        assert!(run("full", "RUNNING_UNLOCKED", "emulated mounted null")
            .contains("FRAMELY_ANDROID_READY"));
    }
    #[test]
    fn android_ready_errors_identify_the_blocked_stage() {
        assert!(
            android_readiness_failure("probe timed out", AndroidReadiness::Full)
                .contains("did not respond")
        );
        assert!(
            android_readiness_failure("FRAMELY_BOOT=0", AndroidReadiness::Full).contains("startup")
        );
        assert!(android_readiness_failure(
            "FRAMELY_BOOT=1\nFRAMELY_PM=0",
            AndroidReadiness::PackageManager
        )
        .contains("PackageManager"));
        assert!(android_readiness_failure(
            "FRAMELY_BOOT=1\nFRAMELY_PM=1\nFRAMELY_USER=RUNNING_UNLOCKED\nFRAMELY_STORAGE=0",
            AndroidReadiness::Full
        )
        .contains("external storage"));
    }
    #[test]
    fn updating_uninstalling_retaining_and_batch_cleanup_are_scoped() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let ticket = f.review(43);
        f.operation(
            "install",
            json!({"context":"test","app":"test/com.example.app","ticket":ticket,"approve":true}),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        f.operation(
            "uninstall",
            json!({"app":"test/com.example.app","approve":true}),
        )
        .unwrap();
        assert!(f.save_file().exists());
        let v = list(&f.home).unwrap();
        assert_eq!(v["apps"][0]["installed"], false);
        assert_eq!(v["apps"][0]["pending"], Value::Null);
        let other = f
            .save_file()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("com.other.app");
        fs::create_dir_all(&other).unwrap();
        fs::write(other.join("keep"), "other app").unwrap();
        let v = f
            .operation(
                "cleanup",
                json!({"items":["data:test/com.example.app"],"approve":true}),
            )
            .unwrap();
        assert!(v["failed"].as_array().unwrap().is_empty(), "{v}");
        assert!(!f.save_file().exists());
        assert!(other.join("keep").exists());
    }
    #[test]
    fn cleanup_rechecks_installation_and_rejects_unknown_paths() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let v = f
            .operation(
                "cleanup",
                json!({"items":["data:test/com.example.app","backups:../outside"],"approve":true}),
            )
            .unwrap();
        assert_eq!(v["failed"].as_array().unwrap().len(), 2);
        assert!(f.save_file().exists());
        assert!(f
            .operation(
                "settings",
                json!({"app":"test/com.example.app","activity":"com.other.Activity"})
            )
            .is_err());
        f.operation("settings",json!({"app":"test/com.example.app","showWindow":false,"activity":"com.example.app.Main"})).unwrap();
        let db = load(&f.home).unwrap();
        assert_eq!(db.records["test/com.example.app"].show_window, Some(false));
    }
    #[test]
    fn restore_checks_integrity_and_handles_overlay_scratch_permissions() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let mut db = load(&f.home).unwrap();
        let (found, _) = apps(&f.home, &db, &containers(&f.home, &db), false);
        record_app(&mut db, &found[0]);
        save(&f.home, &db).unwrap();
        let c = containers(&f.home, &db)
            .into_iter()
            .find(|c| c.id == "test")
            .unwrap();
        stop(&c, &f.dir.path().join("test.log")).unwrap();
        let _socket =
            std::os::unix::net::UnixListener::bind(c.baked.join("data_overlay/transient.sock"))
                .unwrap();
        let resource = c
            .baked
            .join("external/Android/data/com.example.app/files/update.part");
        fs::create_dir_all(resource.parent().unwrap()).unwrap();
        fs::write(&resource, b"unfinished download").unwrap();
        storage::prepare(&c.baked).unwrap();
        let shader = c.baked.join("shadercache/compiled");
        fs::write(&shader, b"compiled shader").unwrap();
        fs::create_dir_all(c.baked.join("data_overlay/media")).unwrap();
        std::os::unix::fs::symlink(
            c.baked.join("external"),
            c.baked.join("data_overlay/media/0"),
        )
        .unwrap();
        let id = backup(&f.home, &c, &db, &f.dir.path().join("test.log")).unwrap();
        assert!(clean_candidates(&f.home, &db)
            .unwrap()
            .iter()
            .any(|v| v["name"] == id && v["restorable"] == true));
        fs::write(f.save_file(), "changed progress").unwrap();
        fs::write(&resource, b"changed download").unwrap();
        fs::write(&shader, b"changed shader").unwrap();
        let scratch = c.baked.join("data_workdir/work");
        fs::create_dir_all(&scratch).unwrap();
        fs::set_permissions(&scratch, fs::Permissions::from_mode(0)).unwrap();
        let result = f
            .operation(
                "restore",
                json!({"context":"test","backup":id,"approve":true}),
            )
            .unwrap();
        assert_eq!(result["verificationRequired"], true);
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        assert_eq!(fs::read(&resource).unwrap(), b"unfinished download");
        assert_eq!(fs::read(&shader).unwrap(), b"compiled shader");
        assert_eq!(
            fs::read(
                c.baked
                    .join("data_overlay/media/0/Android/data/com.example.app/files/update.part")
            )
            .unwrap(),
            b"unfinished download"
        );
        let original = root(&f.home)
            .join("backups")
            .join(&id)
            .join("baked/data_overlay/data/com.example.app/files/save");
        fs::write(original, "tampered progress").unwrap();
        assert!(f
            .operation(
                "restore",
                json!({"context":"test","backup":id,"approve":true})
            )
            .unwrap_err()
            .to_string()
            .contains("integrity"));
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
    }
    #[test]
    fn rebooted_context_reregisters_same_apk_without_losing_data() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        fs::write(f.dir.path().join("broken"), "missing Android registration").unwrap();
        f.operation("reconcile", json!({"app":"test/com.example.app"}))
            .unwrap();
        f.operation("launch", json!({"app":"test/com.example.app"}))
            .unwrap();
        assert!(!f.dir.path().join("broken").exists());
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
    }
    #[test]
    fn retained_cleanup_uses_live_state_when_android_metadata_is_delayed() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        f.operation(
            "uninstall",
            json!({"app":"test/com.example.app","approve":true}),
        )
        .unwrap();
        let baked = f.home.join(".local/share/lepton/contexts/test/baked");
        crate::apk_metadata::fixture(&baked.join("app_overlay/base.apk"), true, 42);
        fs::write(baked.join("data_overlay/system/users/0/package-restrictions.xml"), r#"<package-restrictions><pkg name="com.example.app" installed="true"/></package-restrictions>"#).unwrap();
        assert_eq!(list(&f.home).unwrap()["apps"][0]["installed"], false);
        let result = f
            .operation(
                "cleanup",
                json!({"items":["data:test/com.example.app"],"approve":true}),
            )
            .unwrap();
        assert!(result["failed"].as_array().unwrap().is_empty(), "{result}");
        assert!(!f.save_file().exists());
    }
    #[test]
    fn interrupted_restore_rolls_back_missing_baked_before_next_mutation() {
        let f = Fixture::new();
        list(&f.home).unwrap();
        let mut db = load(&f.home).unwrap();
        db.owned_contexts.push("test".into());
        save(&f.home, &db).unwrap();
        let parent = f.home.join(".local/share/lepton/contexts/test");
        fs::rename(parent.join("baked"), parent.join("previous-test")).unwrap();
        fs::create_dir(parent.join("restore-test")).unwrap();
        fs::write(
            parent.join("framely-restore.json"),
            r#"{"old":"previous-test","replacement":"restore-test","records":[]}"#,
        )
        .unwrap();
        f.operation(
            "settings",
            json!({"app":"test/com.example.app","showWindow":false}),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(f.save_file()).unwrap(), "saved progress");
        assert!(!parent.join("framely-restore.json").exists());
        assert!(!parent.join("restore-test").exists());
    }
    #[test]
    fn disabled_packages_remain_visible_but_unavailable_and_symlinks_are_rejected() {
        let f = Fixture::new();
        let b = f.home.join(".local/share/lepton/contexts/test/baked");
        f.operation(
            "settings",
            json!({"app":"test/com.example.app","activity":"com.example.app.Main"}),
        )
        .unwrap();
        fs::write(b.join("data_overlay/system/users/0/package-restrictions.xml"),"<package-restrictions><pkg name=\"com.example.app\" enabled=\"3\"/></package-restrictions>").unwrap();
        assert!(launcher(&f.home)[0]["launchUnavailable"].is_string());
        let r = init(&f.home).unwrap();
        fs::remove_dir(r.join("reviews")).unwrap();
        std::os::unix::fs::symlink(f.dir.path(), r.join("reviews")).unwrap();
        assert!(upload_dir(&f.home, 100).is_err());
    }
}
