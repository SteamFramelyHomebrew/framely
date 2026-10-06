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
static MUTATION: Mutex<()> = Mutex::new(());
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    id: String,
    context: String,
    metadata: Metadata,
    removed: bool,
    #[serde(default)]
    pending: Option<String>,
    #[serde(default)]
    activity: Option<String>,
    #[serde(default)]
    show_window: Option<bool>,
}
#[derive(Default, Serialize, Deserialize)]
struct Database {
    #[serde(default)]
    owned_contexts: Vec<String>,
    #[serde(default)]
    root_contexts: BTreeMap<PathBuf, String>,
    #[serde(default)]
    roots: Vec<PathBuf>,
    #[serde(default)]
    records: BTreeMap<String, Record>,
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
    let tmp = r.join(format!(
        "state-{}.tmp",
        hex::encode(rand::random::<[u8; 12]>())
    ));
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    f.write_all(&serde_json::to_vec(db)?)?;
    f.sync_all()?;
    fs::rename(tmp, r.join("state.json"))?;
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
            "{{.State.Running}}",
            &format!("lepton-{name}"),
        ],
        None,
    )
    .is_ok_and(|s| s.trim() == "true")
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
                    running: running(&name),
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
                    running: running(id),
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
    if let Ok(s) = podman(&["ps", "--format", "{{.Names}}"], None) {
        for n in s.lines().filter_map(|s| s.strip_prefix("lepton-")) {
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
                        c.steam |= n.starts_with("steamlaunch-");
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
fn installed(c: &Container) -> Result<Vec<(Metadata, PathBuf)>> {
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
            .unwrap_or(direct);
        if apk.exists() {
            ensure!(
                fs::canonicalize(&apk)?.starts_with(&c.baked),
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
        match installed(c) {
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
        json!({"apps":apps,"containers":cs.iter().map(|c| { let occupied=apps.iter().any(|a| a.context==c.id&&a.installed); let known=!warnings.iter().any(|w|w.starts_with(&format!("{}:",c.name)))&&!apps.iter().any(|a|a.context==c.id&&!a.state_known); let shared_mount=c.baked.join("app_overlay/base.apk").exists()||c.baked.join("app_lowerdir/base.apk").exists(); let mut v=serde_json::to_value(c).unwrap(); v["acceptsAdditionalApps"]=json!(!c.steam&&known&&!(occupied&&shared_mount)); v }).collect::<Vec<_>>(),"warnings":warnings,"roots":db.roots,"available":runner(home).is_ok()}),
    )
}
pub fn launcher(home: &Path) -> Vec<Value> {
    let Ok(db) = load(home) else { return vec![] };
    let cs = containers(home, &db);
    let (apps, _) = apps(home, &db, &cs, false);
    apps.into_iter()
        .filter(|a| {
            a.state_known
                && a.installed
                && !a.steam
                && (!a.metadata.activities.is_empty() || a.activity.is_some())
        })
        .map(|a| json!({"id":a.id,"kind":"lepton","name":a.metadata.name,"icon":a.metadata.icon}))
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
pub fn inspect(home: &Path, staged: &Path, cancel: &Cancellation) -> Result<Value> {
    let _guard = MUTATION.lock().unwrap();
    cancel.check()?;
    let r = init(home)?;
    space(&r, fs::metadata(staged)?.len() * 3)?;
    let metadata = apk_metadata::read(staged)?;
    let ticket = hex::encode(rand::random::<[u8; 24]>());
    let d = r.join("reviews").join(&ticket);
    fs::create_dir(&d)?;
    fs::copy(staged, d.join("base.apk"))?;
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
    Ok(json!({"ticket":ticket,"metadata":metadata,"bytes":fs::metadata(staged)?.len()}))
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
        "APK review expired; upload again"
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
    // A development context mounts its single APK at /data/steam_app on each boot.
    // Android's remembered randomized /data/app path then has no mounted APK.
    // Re-register the same signed APK with replacement enabled, preserving all data.
    let apk = [
        c.baked.join("app_overlay/base.apk"),
        c.baked.join("app_lowerdir/base.apk"),
    ]
    .into_iter()
    .find(|p| p.is_file());
    let Some(apk) = apk else {
        return Ok(());
    };
    let metadata = cached_metadata(&apk)?;
    if load(home)?
        .records
        .get(&format!("{}/{}", c.id, metadata.package))
        .is_some_and(|r| r.removed)
    {
        return Ok(());
    }
    if restriction(c, &metadata.package).is_some_and(|r| !r.0) {
        return Ok(());
    }
    if !live_installed(c, &metadata.package, log)? {
        install_package(c, &apk, log)?;
        ensure!(
            live_installed(c, &metadata.package, log)?,
            "Android package registration did not recover"
        );
    }
    Ok(())
}
// Adapt only the entry script in a private temporary file. Keep Lepton's original
// libraries and dev-context data handling; app mode can clear existing baked data.
fn direct_launch_script(source: &str) -> Result<String> {
    let directory =
        "SCRIPT_DIR=$( cd -- \"$( dirname -- \"${BASH_SOURCE[0]}\" )\" &> /dev/null && pwd )";
    let include = "source \"${SCRIPT_DIR}/liblepton/liblepton.sh\"";
    ensure!(
        source.matches(directory).count() == 1 && source.matches(include).count() == 1,
        "Unsupported Lepton entry script; cannot start an APK without showing the desktop"
    );
    let hooks = r#"
# Framely: boot without a desktop window, retaining the original dev context.
# Do not use is_app: Lepton's app bake path can reset existing application data.
eval "$(declare -f setup_props | sed '1s/setup_props/framely_original_setup_props/')"
function app_wants_flatscreen() {
    [[ "${APP_WANTS_FLATSCREEN:-true}" == true ]]
}
function setup_props() {
    framely_original_setup_props "$@"
    sed -i 's/^waydroid.background_start=false$/waydroid.background_start=true/' "$(props_file)"
    printf '\nwaydroid.active_apps=none\n' >> "$(props_file)"
}
"#;
    Ok(source
        .replace(directory, "SCRIPT_DIR=\"${FRAMELY_LEPTON_DIR:?}\"")
        .replace(include, &format!("{include}\n{hooks}")))
}
fn start(home: &Path, c: &Container, show: Option<bool>, log: &Path) -> Result<()> {
    start_container(home, c, show, false, log)
}
fn start_container(
    home: &Path,
    c: &Container,
    show: Option<bool>,
    direct: bool,
    log: &Path,
) -> Result<()> {
    if running(&c.name) {
        return restore_package_mount(home, c, log);
    }
    let runner = runner(home)?;
    let mut temporary = None;
    let mut cmd = if direct {
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
    } else {
        Command::new(&runner)
    };
    cmd.args(["start", &c.name])
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
    if c.id.starts_with("external-") {
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
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(60) {
        if podman(
            &[
                "exec",
                &format!("lepton-{}", c.name),
                "pm",
                "path",
                "android",
            ],
            None,
        )
        .is_ok_and(|s| s.contains("package:"))
        {
            return restore_package_mount(home, c, log);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    bail!("Lepton did not become ready; inspect the container log")
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
                r.metadata = a.metadata.clone();
                r.removed = false;
            }
        })
        .or_insert_with(|| Record {
            id: a.id.clone(),
            context: a.context.clone(),
            metadata: a.metadata.clone(),
            removed: !a.installed,
            pending: None,
            activity: a.activity.clone(),
            show_window: a.show_window,
        });
}
fn app(home: &Path, db: &Database, id: &str) -> Result<(App, Container)> {
    let cs = containers(home, db);
    let (apps, _) = apps(home, db, &cs, false);
    let a = apps
        .into_iter()
        .find(|a| a.id == id)
        .context("APK application not found")?;
    ensure!(!a.steam, "Manage Steam APKs through Steam");
    let c = cs
        .iter()
        .find(|c| c.id == a.context)
        .cloned()
        .context("Container unavailable; add its data location first")?;
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
fn install_package(c: &Container, apk: &Path, log: &Path) -> Result<()> {
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
    let result = podman(
        &[
            "exec",
            &format!("lepton-{}", c.name),
            "pm",
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
fn launch(c: &Container, a: &App, log: &Path) -> Result<()> {
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
    let s = podman(
        &[
            "exec",
            &format!("lepton-{}", c.name),
            "am",
            "start",
            "-W",
            "--user",
            "0",
            "-n",
            &component,
        ],
        Some(log),
    )?;
    ensure!(
        !s.contains("Error:")
            && !s.contains("Exception")
            && s.lines().any(|line| line.trim() == "Status: ok"),
        "Application launch failed: {s}"
    );
    // Reveal the target only after ActivityManager has completed the launch.
    // The compositor's app mode avoids exposing the Android home screen.
    if a.show_window.unwrap_or(!a.metadata.vr) {
        podman(
            &[
                "exec",
                &format!("lepton-{}", c.name),
                "setprop",
                "waydroid.active_apps",
                &a.metadata.package,
            ],
            Some(log),
        )?;
    }
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
    progress(json!({"phase":"preparing"}));
    let result = (|| -> Result<Value> {
        match kind {
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
                if c.baked.exists() {
                    let packages = installed(&c)?;
                    ensure!(packages.iter().all(|(m,_)|m.package==metadata.package)||!(c.baked.join("app_overlay/base.apk").exists()||c.baked.join("app_lowerdir/base.apk").exists()),"This Lepton context uses one shared APK mount; install the new app in an independent container");
                }
                let previous = if c.baked.exists() {
                    installed(&c)?
                        .into_iter()
                        .find(|(m, _)| m.package == metadata.package)
                } else {
                    None
                };
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
                let settings = db.records.get(&app_id).cloned().unwrap();
                cancel.check()?;
                // Before submission cancellation remains possible. Package-manager submission is a commit.
                cancel.commit(||{
 progress(json!({"cancellable":false}));
 progress(json!({"phase":"backing-up"}));let mut backup_id=None;if c.baked.exists(){stop(&c,&log)?;backup_id=Some(backup(home,&c,&db,&log)?);}
 progress(json!({"phase":"starting"}));start(home,&c,settings.show_window.or(Some(!metadata.vr)),&log)?;
 let sdk=podman(&["exec",&format!("lepton-{}",c.name),"getprop","ro.build.version.sdk"],Some(&log))?.trim().parse::<u32>()?;ensure!(metadata.min_sdk<=sdk,"APK requires Android SDK {}, container provides {}",metadata.min_sdk,sdk);
 progress(json!({"phase":"installing","cancellable":false}));install_package(&c,&review.join("base.apk"),&log)?;
 progress(json!({"phase":"verifying"}));let path=podman(&["exec",&format!("lepton-{}",c.name),"pm","path",&metadata.package],Some(&log))?;ensure!(path.contains("package:"),"Android did not report the installed APK");let dest=r.join("apks").join(hash(&app_id));store_apk(&review.join("base.apk"),&dest)?;
 let rec=db.records.get_mut(&app_id).unwrap();rec.metadata=metadata.clone();rec.removed=false;rec.pending=None;save(home,&db)?;fs::remove_dir_all(review)?;
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
                if kind == "purge" && !a.installed {
                    ensure!(p["approve"] == true, "Confirm APK removal");
                    return cancel.commit(|| {
                        progress(json!({"cancellable":false}));
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
                            start_container(
                                home,
                                &c,
                                a.show_window.or(Some(!a.metadata.vr)),
                                true,
                                &log,
                            )?;
                            launch(&c, &a, &log)?;
                        }
                        "close" => {
                            if running(&c.name) {
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
                            progress(json!({"phase":"uninstalling"}));
                            start(home, &c, a.show_window, &log)?;
                            if a.installed {
                                if let Some((_, apk)) = installed(&c)?
                                    .into_iter()
                                    .find(|(m, _)| m.package == a.metadata.package)
                                {
                                    let dest = r.join("apks").join(hash(id));
                                    if apk.is_file() {
                                        store_apk(&apk, &dest)?;
                                    }
                                }
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
                            let rec = db.records.get_mut(id).unwrap();
                            rec.removed = true;
                            rec.pending = None;
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
                        "container.start" => start(home, &c, None, &log)?,
                        "container.stop" => stop(&c, &log)?,
                        "container.delete" => {
                            let standard = home.join(".local/share/lepton/contexts").join(&c.name);
                            let default_location = !fs::symlink_metadata(&standard).is_ok_and(|m|m.file_type().is_symlink()) && fs::canonicalize(&standard).is_ok_and(|p| p.join("baked") == c.baked);
                            let target = if default_location { standard } else { c.baked.clone() };
                            // An external compatdata parent can contain unrelated files. Delete only baked.
                            stop(&c, &log)?;
                            ensure!(validate_baked(&c.baked)? == c.baked, "Container data location changed");
                            if crate::process::command_output_timeout(crate::process::tool("podman").args(["inspect", "--format", "{{.Id}}", &format!("lepton-{}", c.name)]), Duration::from_secs(10), false)?.status.success() {
                                podman(&["rm", &format!("lepton-{}",c.name)],Some(&log))?;
                            }
                            remove_tree(&target)?;
                            db.roots.retain(|p| p != &c.baked);
                            db.root_contexts.remove(&c.baked);
                            db.records.retain(|_, r| r.context != context);
                            db.owned_contexts.retain(|id| id != context);
                            save(home, &db)?;
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
                                    && installed(&c)?.len() <= 1,
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
pub fn launch_app(home: &Path, id: &str) -> Result<()> {
    operate(
        home,
        "launch",
        &json!({"app":id}),
        Cancellation::default(),
        Arc::new(|_| {}),
    )?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(direct_launch_script("unrecognized future entrypoint").is_err());
    }
    static SERIAL: Mutex<()> = Mutex::new(());
    struct Fixture {
        dir: tempfile::TempDir,
        home: PathBuf,
        _guard: std::sync::MutexGuard<'static, ()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            crate::process::TEST_TOOLS.with(|p| *p.borrow_mut() = None)
        }
    }
    impl Fixture {
        fn new() -> Self {
            let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
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
 inspect) cat '{state}';;
 ps) if [ "$(cat '{state}')" = true ]; then echo lepton-test; fi;;
 stop) echo false > '{state}'; echo stopped;;
 cp) /bin/cp "$2" '{incoming}';;
 exec)
  if [ "$3" = pm ]; then
   case "$4" in
    list) if [ -f '{apk}' ] && [ ! -f '{broken}' ] && [ ! -f '{removed}' ]; then echo package:com.example.app; fi;;
    path) if [ "$5" = android ] || [ -f '{apk}' ]; then echo package:/data/app/xyz/com.example.app/base.apk; fi;;
    install) if [ -f '{fail}' ]; then echo 'Failure [INSTALL_FAILED_UPDATE_INCOMPATIBLE]'; else /bin/cp '{incoming}' '{apk}'; /bin/rm -f '{broken}' '{removed}'; echo '<package-restrictions><pkg name="com.example.app" installed="true"/></package-restrictions>' > '{restrictions}'; echo Success; fi;;
    uninstall) touch '{removed}'; /bin/rm -f '{apk}'; echo '<package-restrictions><pkg name="com.example.app" installed="false"/></package-restrictions>' > '{restrictions}'; if [ "$5" != -k ]; then /bin/rm -rf '{data}'; fi; echo Success;;
    clear) echo Success;;
   esac
  elif [ "$3" = getprop ]; then echo 30;
  elif [ "$3" = cmd ]; then echo com.example.app/.Main;
  elif [ "$3" = am ]; then if [ -f '{launch_fail}' ]; then echo 'Error: launch failed'; else echo 'Status: ok'; fi;
  fi;;
esac
"#,
                state = state.display(),
                commands = dir.path().join("commands").display(),
                launch_fail = dir.path().join("launch-fail").display(),
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
            fs::create_dir_all(&lepton).unwrap();
            fs::write(
                lepton.join("lepton"),
                format!("#!/bin/sh\necho true > '{}'\n", state.display()),
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
            operate(
                &self.home,
                kind,
                &p,
                Cancellation::default(),
                Arc::new(|_| {}),
            )
        }
        fn save_file(&self) -> PathBuf {
            self.home.join(".local/share/lepton/contexts/test/baked/data_overlay/data/com.example.app/files/save")
        }
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
            .find("am start -W --user 0 -n com.example.app/")
            .unwrap();
        let reveal = trace
            .find("setprop waydroid.active_apps com.example.app")
            .unwrap();
        assert!(start < reveal);
        fs::write(&commands, "").unwrap();
        a.show_window = Some(false);
        launch(&c, &a, &log).unwrap();
        assert!(!fs::read_to_string(&commands)
            .unwrap()
            .contains("waydroid.active_apps"));
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
        assert!(launcher(&f.home).is_empty());
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
    fn additional_apps_policy_depends_on_shared_mount_not_creator() {
        let f = Fixture::new();
        let data = list(&f.home).unwrap();
        assert_eq!(data["containers"][0]["acceptsAdditionalApps"], false);
        let baked = f.home.join(".local/share/lepton/contexts/test/baked");
        let direct = baked.join("data_overlay/app/xyz/com.example.app");
        fs::create_dir_all(&direct).unwrap();
        fs::rename(baked.join("app_overlay/base.apk"), direct.join("base.apk")).unwrap();
        fs::write(f.dir.path().join("running"), "false").unwrap();
        let data = list(&f.home).unwrap();
        assert_eq!(data["containers"][0]["acceptsAdditionalApps"], true);
        assert_eq!(data["containers"][0]["managed"], false);
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
        let id = backup(&f.home, &c, &db, &f.dir.path().join("test.log")).unwrap();
        assert!(clean_candidates(&f.home, &db)
            .unwrap()
            .iter()
            .any(|v| v["name"] == id && v["restorable"] == true));
        fs::write(f.save_file(), "changed progress").unwrap();
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
    fn disabled_packages_are_hidden_from_launcher_and_symlinks_are_rejected() {
        let f = Fixture::new();
        let b = f.home.join(".local/share/lepton/contexts/test/baked");
        f.operation(
            "settings",
            json!({"app":"test/com.example.app","activity":"com.example.app.Main"}),
        )
        .unwrap();
        fs::write(b.join("data_overlay/system/users/0/package-restrictions.xml"),"<package-restrictions><pkg name=\"com.example.app\" enabled=\"3\"/></package-restrictions>").unwrap();
        assert!(launcher(&f.home).is_empty());
        let r = init(&f.home).unwrap();
        fs::remove_dir(r.join("reviews")).unwrap();
        std::os::unix::fs::symlink(f.dir.path(), r.join("reviews")).unwrap();
        assert!(upload_dir(&f.home, 100).is_err());
    }
}
