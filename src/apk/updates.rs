//! App-originated APK updates are queued, never installed without review.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
const HOOKS: &str = include_str!("../../native/android/apk-updates-hooks.sh");
const MAX_AGE: u64 = 7 * 24 * 3600;
static QUEUE_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Bridge {
    context: String,
}
#[derive(Deserialize)]
struct Captured {
    package: Option<String>,
    bytes: Option<u64>,
    error: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Request {
    pub id: String,
    pub app: String,
    pub name: String,
    pub previous: String,
    pub version: String,
    pub bytes: u64,
    pub created: u64,
    pub error: Option<String>,
    #[serde(default)]
    sha256: String,
    #[serde(default)]
    revision: u64,
    #[serde(default)]
    presented_revision: Option<u64>,
    // Old records have no trustworthy delivery history and stay manually accessible.
    #[serde(default)]
    presentation_tracked: bool,
}
/// One confirmation at a time; cancelling retains the APK without immediately reopening it.
#[derive(Default)]
pub(crate) struct Windows {
    active: Option<String>,
    shown: BTreeMap<String, u64>,
    sent_at: Option<std::time::Instant>,
    attempts: u8,
    failed: Option<String>,
}
impl Windows {
    pub(crate) fn next(&mut self, requests: &[Request]) -> Option<String> {
        self.next_at(requests, std::time::Instant::now())
    }
    fn next_at(&mut self, requests: &[Request], now: std::time::Instant) -> Option<String> {
        self.shown.retain(|id, _| requests.iter().any(|r| &r.id == id));
        if let Some(active) = &self.active {
            // Coalesce repeats while their confirmation is already open.
            if let Some(r) = requests.iter().find(|r| &r.id == active) {
                self.shown.insert(r.id.clone(), r.revision);
                if r.presented_revision != Some(r.revision)
                    && self.sent_at.is_some_and(|sent| now.saturating_duration_since(sent) >= Duration::from_secs(5)) {
                    if self.attempts < 3 {
                        self.attempts += 1;
                        self.sent_at = Some(now);
                        return Some(r.id.clone());
                    }
                    if self.attempts == 3 {
                        self.failed = Some(r.id.clone());
                        self.attempts = 4;
                    }
                }
                return None;
            }
            // Installed/dismissed requests must not block the next application.
            self.active = None;
        }
        let request = requests
            .iter()
            .filter(|r| r.error.is_none() && r.presentation_tracked && r.presented_revision != Some(r.revision) && self.shown.get(&r.id) != Some(&r.revision))
            .min_by_key(|r| r.created)?;
        let id = request.id.clone();
        self.shown.insert(id.clone(), request.revision);
        self.active = Some(id.clone());
        self.sent_at = Some(now);
        self.attempts = 1;
        Some(id)
    }
    pub(crate) fn take_failure(&mut self) -> Option<String> { self.failed.take() }
    pub(crate) fn close(&mut self, id: &str) {
        if self.active.as_deref() == Some(id) {
            self.active = None;
        }
    }
    pub(crate) fn reopen(&mut self, id: &str) -> Result<()> {
        ensure!(key(id), "Invalid APK update request");
        ensure!(
            self.active.as_deref().is_none_or(|active| active == id),
            "Finish the current APK update first"
        );
        self.active = Some(id.to_owned());
        self.sent_at = Some(std::time::Instant::now());
        self.attempts = 1;
        self.shown.entry(id.to_owned()).or_insert(0);
        Ok(())
    }
}
fn directory(home: &Path) -> PathBuf {
    root(home).join("updates")
}
fn key(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit())
}
fn private(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    ensure!(
        !fs::symlink_metadata(path)?.file_type().is_symlink(),
        "Invalid APK update directory"
    );
    ensure!(
        fs::metadata(path)?.uid() == unsafe { libc::geteuid() },
        "APK update directory belongs to another user"
    );
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}
fn write(path: &Path, value: &impl Serialize) -> Result<()> {
    steam_shortcuts::write_owned(path, &serde_json::to_vec(value)?, 0o600)
}
pub(super) fn hooks() -> &'static str {
    HOOKS
}
pub(super) fn prepare(home: &Path, c: &Container) -> Result<Option<Value>> {
    let db = load(home)?;
    let packages: BTreeSet<_> = db
        .records
        .values()
        .filter(|r| r.context == c.id && (!r.removed || r.pending.is_some()))
        .map(|r| r.metadata.package.as_str())
        .filter(|p| apk_metadata::valid_package(p))
        .collect();
    if packages.is_empty() {
        return Ok(None);
    }
    let base = directory(home);
    private(&base)?;
    let runtime = base.join("runtime");
    private(&runtime)?;
    let dir = base.join(format!("bridge-{}", hash(&c.id)));
    private(&dir)?;
    write(
        &dir.join("context.json"),
        &Bridge {
            context: c.id.clone(),
        },
    )?;
    let pipe_root = std::env::temp_dir().join(format!("framely-apk-updates-{}", unsafe {
        libc::geteuid()
    }));
    private(&pipe_root)?;
    let fifo = pipe_root.join(format!("{}.pipe", hash(&c.id)));
    match fs::symlink_metadata(&fifo) {
        Ok(m) => ensure!(m.file_type().is_fifo(), "APK update pipe is not a FIFO"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes())?;
            ensure!(
                unsafe { libc::mkfifo(name.as_ptr(), 0o600) } == 0,
                "Cannot create APK update pipe"
            );
        }
        Err(e) => return Err(e.into()),
    }
    let dex = runtime.join("framely-updates.dex");
    let script = runtime.join("updates.sh");
    steam_shortcuts::write_owned(
        &dex,
        include_bytes!("../../native/android/framely-updates.dex"),
        0o600,
    )?;
    steam_shortcuts::write_owned(
        &script,
        include_bytes!("../../native/android/apk-updates.sh"),
        0o600,
    )?;
    let allowlist = dir.join("packages");
    steam_shortcuts::write_owned(
        &allowlist,
        packages
            .into_iter()
            .collect::<Vec<_>>()
            .join("\n")
            .as_bytes(),
        0o600,
    )?;
    let _ = fs::remove_file(dir.join("ready"));
    Ok(Some(
        json!({"FRAMELY_UPDATE_DIR":dir,"FRAMELY_UPDATE_PIPE":fifo,"FRAMELY_UPDATE_DEX":dex,
        "FRAMELY_UPDATE_SCRIPT":script,"FRAMELY_UPDATE_PACKAGES":allowlist}),
    ))
}
pub(super) fn configure(cmd: &mut Command, home: &Path, c: &Container) -> Result<()> {
    for key in [
        "FRAMELY_UPDATE_DIR",
        "FRAMELY_UPDATE_PIPE",
        "FRAMELY_UPDATE_DEX",
        "FRAMELY_UPDATE_SCRIPT",
        "FRAMELY_UPDATE_PACKAGES",
    ] {
        cmd.env_remove(key);
    }
    if let Some(env) = prepare(home, c)? {
        for (k, v) in env.as_object().unwrap() {
            cmd.env(k, v.as_str().context("Invalid APK update environment")?);
        }
    }
    Ok(())
}
fn bridge_dirs(home: &Path) -> Vec<PathBuf> {
    fs::read_dir(directory(home))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("bridge-"))
                && e.file_type().is_ok_and(|t| t.is_dir())
        })
        .map(|e| e.path())
        .collect()
}
fn regular(path: &Path, max: u64) -> Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let f = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    ensure!(
        f.metadata()?.is_file() && f.metadata()?.len() <= max,
        "Invalid APK update file"
    );
    Ok(f)
}
fn request(home: &Path, id: &str) -> Result<(PathBuf, Request)> {
    ensure!(key(id), "Invalid APK update request");
    for dir in bridge_dirs(home) {
        if let Ok(f) = regular(&dir.join(format!("{id}.request")), 16384) {
            let r: Request = serde_json::from_reader(f)?;
            ensure!(r.id == id, "APK update request changed");
            return Ok((dir, r));
        }
    }
    bail!("APK update request has ended; retry from the application")
}
fn remove(dir: &Path, id: &str) -> Result<()> {
    ensure!(key(id), "Invalid APK update request");
    for ext in ["json", "apk", "apk.part", "request", "json.part"] {
        let p = dir.join(format!("{id}.{ext}"));
        match fs::remove_file(p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
/// Called by the session worker. Metadata parsing happens off the UI request thread.
pub(crate) fn poll(home: &Path) -> Result<Vec<Request>> {
    let _gate = QUEUE_GATE.lock().unwrap();
    let db = load(home)?;
    let mut notices = Vec::new();
    for dir in bridge_dirs(home) {
        let bridge: Bridge = serde_json::from_reader(regular(&dir.join("context.json"), 4096)?)?;
        for entry in fs::read_dir(&dir)?.filter_map(|e| e.ok()) {
            let name = entry.file_name();
            let Some(id) = name
                .to_str()
                .and_then(|n| n.strip_suffix(".json"))
                .filter(|n| key(n))
            else {
                continue;
            };
            let id = id.to_owned();
            if dir.join(format!("{id}.request")).exists() {
                continue;
            }
            let captured: Result<Captured> =
                (|| Ok(serde_json::from_reader(regular(&entry.path(), 16384)?)?))();
            let result = (|| -> Result<Request> {
                let captured = captured?;
                if let Some(error) = captured.error {
                    bail!("{error}");
                }
                let package = captured
                    .package
                    .context("Update package identity missing")?;
                let rec = db
                    .records
                    .values()
                    .find(|r| {
                        r.context == bridge.context && !r.removed && r.metadata.package == package
                    })
                    .context("Update does not match an installed application")?;
                let apk = dir.join(format!("{id}.apk"));
                let f = regular(&apk, 8 * 1024 * 1024 * 1024)?;
                ensure!(
                    captured.bytes == Some(f.metadata()?.len()),
                    "Extracted APK size changed"
                );
                let metadata = apk_metadata::read(&apk)?;
                ensure!(
                    metadata.package == package
                        && metadata.version_code >= rec.metadata.version_code,
                    "Update package mismatch or downgrade"
                );
                let sha256 = crate::package::digest_reader(&mut fs::File::open(&apk)?)?;
                // Retain one APK, but treat a fresh installer request as a new prompt.
                for previous in list(home)?.as_array().into_iter().flatten() {
                    if previous["app"] == rec.id
                        && previous["version"] == metadata.version
                        && previous["sha256"] == sha256
                    {
                        let (previous_dir, mut previous) = request(home, previous["id"].as_str().context("Missing retained update identity")?)?;
                        previous.presentation_tracked = true;
                        previous.revision = previous.revision.checked_add(1).context("APK update request revision exhausted")?;
                        write(&previous_dir.join(format!("{}.request", previous.id)), &previous)?;
                        eprintln!("APK update request={} revision={} repeated request received", previous.id, previous.revision);
                        remove(&dir, &id)?;
                        return Err(anyhow::anyhow!("duplicate"));
                    }
                }
                Ok(Request {
                    id: id.clone(),
                    app: rec.id.clone(),
                    name: rec.metadata.name.clone(),
                    previous: rec.metadata.version.clone(),
                    version: metadata.version,
                    bytes: captured.bytes.unwrap(),
                    created: now(),
                    error: None,
                    sha256,
                    revision: 0,
                    presented_revision: None,
                    presentation_tracked: true,
                })
            })();
            let value = match result {
                Ok(r) => r,
                Err(e) if e.to_string() == "duplicate" => continue,
                Err(e) => Request {
                    id: id.clone(),
                    app: String::new(),
                    name: String::new(),
                    previous: String::new(),
                    version: String::new(),
                    bytes: 0,
                    created: now(),
                    error: Some(e.to_string()),
                    sha256: String::new(),
                    revision: 0,
                    presented_revision: None,
                    presentation_tracked: true,
                },
            };
            write(&dir.join(format!("{id}.request")), &value)?;
            eprintln!("APK update request={id} revision=0 captured valid={}", value.error.is_none());
            notices.push(value);
        }
    }
    let _ = notices;
    Ok(serde_json::from_value(list(home)?)?)
}
pub(crate) fn list(home: &Path) -> Result<Value> {
    let mut values = Vec::new();
    for dir in bridge_dirs(home) {
        for entry in fs::read_dir(&dir)?.filter_map(|e| e.ok()) {
            let name = entry.file_name();
            let Some(id) = name
                .to_str()
                .and_then(|n| n.strip_suffix(".request"))
                .filter(|n| key(n))
            else {
                continue;
            };
            let value: Request = serde_json::from_reader(regular(&entry.path(), 16384)?)?;
            if now().saturating_sub(value.created) > MAX_AGE {
                remove(&dir, id)?;
            } else {
                values.push(value);
            }
        }
    }
    values.sort_by_key(|r| r.created);
    Ok(json!(values))
}
pub(crate) fn presented(home: &Path, id: &str) -> Result<()> {
    let _gate = QUEUE_GATE.lock().unwrap();
    let (dir, mut value) = request(home, id)?;
    if value.presented_revision != Some(value.revision) {
        value.presentation_tracked = true;
        value.presented_revision = Some(value.revision);
        write(&dir.join(format!("{id}.request")), &value)?;
        eprintln!("APK update request={id} revision={} confirmation presented", value.revision);
    }
    Ok(())
}
pub(crate) fn review_request(home: &Path, id: &str, cancel: &Cancellation) -> Result<Value> {
    let (dir, r) = request(home, id)?;
    ensure!(
        r.error.is_none(),
        "Update could not be extracted; retry from the application"
    );
    let db = load(home)?;
    let rec = db
        .records
        .get(&r.app)
        .context("Application no longer exists")?;
    ensure!(!rec.removed, "Application was uninstalled");
    let source = dir.join(format!("{id}.apk"));
    regular(&source, 8 * 1024 * 1024 * 1024)?;
    let review = inspect(home, &source, cancel)?;
    if review["metadata"]["package"] != rec.metadata.package {
        let _ = reviews_drop(home, review["ticket"].as_str().unwrap());
        bail!("APK update package changed");
    }
    Ok(review)
}
pub(crate) fn dismiss(home: &Path, id: &str) -> Result<Value> {
    let (dir, _) = request(home, id)?;
    remove(&dir, id)?;
    Ok(json!(true))
}
pub(super) fn validate_install(home: &Path, p: &Value) -> Result<()> {
    let Some(id) = p["updateRequest"].as_str() else {
        return Ok(());
    };
    let (_, r) = request(home, id)?;
    ensure!(
        p["app"] == r.app && p["context"].as_str() == r.app.split_once('/').map(|v| v.0),
        "APK update target changed"
    );
    Ok(())
}
pub(super) fn installed(home: &Path, p: &Value) {
    if let Some(id) = p["updateRequest"].as_str() {
        let _ = dismiss(home, id);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_do_not_escape_queue() {
        for id in ["../x", "", "abc", "0123456789abcdef0123456789abcde/ff"] {
            assert!(!key(id));
        }
        assert!(key("0123456789abcdef0123456789abcdef"));
    }
    #[test]
    fn dismiss_preserves_other_requests() {
        let t = tempfile::tempdir().unwrap();
        let a = "0123456789abcdef0123456789abcdef";
        let b = "abcdef0123456789abcdef0123456789";
        fs::write(t.path().join(format!("{a}.apk")), b"test").unwrap();
        fs::write(t.path().join(format!("{b}.apk")), b"keep").unwrap();
        remove(t.path(), a).unwrap();
        assert!(!t.path().join(format!("{a}.apk")).exists());
        assert!(t.path().join(format!("{b}.apk")).exists());
    }
    #[test]
    fn no_follow_rejects_symlinks() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("data"), b"test").unwrap();
        std::os::unix::fs::symlink(t.path().join("data"), t.path().join("link")).unwrap();
        assert!(regular(&t.path().join("link"), 100).is_err());
    }
    #[test]
    fn boot_hook_preserves_steam_forwarding_and_starts_helper_only_inside_container() {
        let t = tempfile::tempdir().unwrap();
        let script = format!(
            r#"
function setup_podman_mounts() {{ PODMAN_CMDLINE+=(--mount 'type=bind,source=/original/steam.pipe,destination=/lepton/steam.pipe,rw'); }}
function generate_zygote_launch_rc() {{ echo '# original zygote' > "$TEST_RC/init.zygote64.rc"; }}
function prefix() {{ echo "$TEST_RC"; }}
function podman_mount_entry() {{ PODMAN_CMDLINE+=(--mount "type=bind,source=$1,destination=$2,$3"); }}
PODMAN_CMDLINE=()
{HOOKS}
setup_podman_mounts
generate_zygote_launch_rc
printf '%s\n' "${{PODMAN_CMDLINE[@]}}"
"#
        );
        let out = Command::new("bash")
            .args(["-euc", &script])
            .env("TEST_RC", t.path())
            .env("FRAMELY_UPDATE_DIR", "/private/updates")
            .env("FRAMELY_UPDATE_PIPE", "/private/fifo")
            .env("FRAMELY_UPDATE_DEX", "/private/helper.dex")
            .env("FRAMELY_UPDATE_SCRIPT", "/private/helper.sh")
            .env("FRAMELY_UPDATE_PACKAGES", "/private/packages")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mounts = String::from_utf8(out.stdout).unwrap();
        assert!(mounts
            .contains("source=/original/steam.pipe,destination=/lepton/framely-original.pipe,rw"));
        assert!(mounts.contains("source=/private/fifo,destination=/lepton/steam.pipe,rw"));
        assert!(mounts.contains("destination=/vendor/share/framely-updates.dex,ro"));
        let rc = fs::read_to_string(t.path().join("init.zygote64.rc")).unwrap();
        assert!(rc.starts_with("# original zygote"));
        assert!(rc.contains("on property:sys.boot_completed=1\n    start framely-apk-updates"));
    }
    #[test]
    fn confirmations_are_automatic_sequential_and_cancel_does_not_reopen() {
        let request = |id: &str, created| Request {
            id: id.into(),
            app: "test/com.example.app".into(),
            name: "test".into(),
            previous: "1".into(),
            version: "2".into(),
            bytes: 100,
            created,
            error: None,
            sha256: String::new(),
            revision: 0,
            presented_revision: None,
            presentation_tracked: true,
        };
        let a = request("0123456789abcdef0123456789abcdef", 1);
        let b = request("abcdef0123456789abcdef0123456789", 2);
        let mut legacy = serde_json::to_value(&a).unwrap();
        legacy.as_object_mut().unwrap().remove("presentedRevision");
        legacy.as_object_mut().unwrap().remove("presentationTracked");
        legacy.as_object_mut().unwrap().remove("revision");
        let legacy: Request = serde_json::from_value(legacy).unwrap();
        assert_eq!(Windows::default().next(&[legacy]), None);
        let mut state = Windows::default();
        let queue = vec![b.clone(), a.clone()];
        assert_eq!(state.next(&queue), Some(a.id.clone()));
        assert_eq!(state.next(&queue), None);
        state.close(&b.id);
        assert_eq!(state.next(&queue), None);
        state.close(&a.id);
        assert_eq!(state.next(&queue), Some(b.id.clone()));
        state.close(&b.id);
        assert_eq!(state.next(&queue), None);
        state.reopen(&a.id).unwrap();
        assert_eq!(state.next(&queue), None);
        state.close(&a.id);
        // A new request for the same retained APK reopens after cancellation.
        let mut repeated = a.clone();
        repeated.revision = 1;
        assert_eq!(state.next(&[repeated.clone()]), Some(a.id.clone()));
        assert_eq!(state.next(&[repeated.clone()]), None);
        // Repeats received while already visible do not reopen after closing.
        repeated.revision = 2;
        assert_eq!(state.next(&[repeated.clone()]), None);
        state.close(&a.id);
        assert_eq!(state.next(&[repeated]), None);
        let mut removed = Windows::default();
        assert_eq!(removed.next(&[a.clone()]), Some(a.id.clone()));
        assert_eq!(removed.next(&[b.clone()]), Some(b.id.clone()));
        // A dropped open command is retried, and missing presentation is surfaced.
        let now = std::time::Instant::now();
        let mut delivery = Windows::default();
        assert_eq!(delivery.next_at(&[a.clone()], now), Some(a.id.clone()));
        assert_eq!(delivery.next_at(&[a.clone()], now + Duration::from_secs(4)), None);
        assert_eq!(delivery.next_at(&[a.clone()], now + Duration::from_secs(5)), Some(a.id.clone()));
        assert_eq!(delivery.next_at(&[a.clone()], now + Duration::from_secs(10)), Some(a.id.clone()));
        assert_eq!(delivery.next_at(&[a.clone()], now + Duration::from_secs(15)), None);
        assert_eq!(delivery.take_failure(), Some(a.id.clone()));
        assert_eq!(delivery.next_at(&[a.clone()], now + Duration::from_secs(20)), None);
        let mut delivered = a.clone();
        delivered.presented_revision = Some(delivered.revision);
        let mut delivery = Windows::default();
        assert_eq!(delivery.next_at(&[a.clone()], now), Some(a.id.clone()));
        assert_eq!(delivery.next_at(&[delivered], now + Duration::from_secs(20)), None);
        assert_eq!(delivery.take_failure(), None);
        // Installation/session restart must not reopen retained requests.
        let mut presented_queue = queue.clone();
        for r in &mut presented_queue { r.presented_revision = Some(r.revision); }
        let mut resumed = Windows::default();
        assert_eq!(resumed.next(&presented_queue), None);
        assert_eq!(Windows::default().next(&queue), Some(a.id.clone()));
        // Manual review remains possible, without automatic reopening on close.
        resumed.reopen(&a.id).unwrap();
        resumed.close(&a.id);
        assert_eq!(resumed.next(&presented_queue), None);
        // A new capture prompts even with older retained requests in the queue.
        let mut fresh = a.clone();
        fresh.id = "1123456789abcdef0123456789abcdef".into();
        let mut pending = presented_queue.clone();
        pending.push(fresh.clone());
        assert_eq!(resumed.next(&pending), Some(fresh.id.clone()));
        resumed.close(&fresh.id);
        // A newly submitted identical APK must also prompt after restart.
        let mut repeated = a.clone();
        repeated.revision += 1;
        assert_eq!(resumed.next(&[repeated]), Some(a.id.clone()));
    }
}
