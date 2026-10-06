//! Steam-owned wrapper lifetime. Only the session service launches/controls APKs.
use super::steam_shortcuts as shortcuts;
use super::*;
use crate::ipc;
use std::os::unix::fs::OpenOptionsExt;
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read},
    os::unix::net::{UnixListener, UnixStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};
type Progress = Arc<dyn Fn(Value) + Send + Sync>;
struct Pending {
    ticket: u64,
    progress: Progress,
    result: mpsc::Sender<std::result::Result<Value, String>>,
}
static PENDING: Mutex<BTreeMap<String, Pending>> = Mutex::new(BTreeMap::new());
static STARTING: Mutex<std::collections::BTreeSet<String>> =
    Mutex::new(std::collections::BTreeSet::new());
static STOPPED: AtomicBool = AtomicBool::new(false);
extern "C" fn interrupted(_: libc::c_int) {
    STOPPED.store(true, Ordering::Relaxed);
}
#[derive(Serialize, Deserialize)]
struct Lease {
    app: String,
    token: String,
    pid: u32,
    signature: String,
    instance: String,
}
fn leases(home: &Path) -> PathBuf {
    root(home).join("steam/leases")
}
fn lease_path(home: &Path, id: &str) -> PathBuf {
    leases(home).join(format!("{}.json", hash(id)))
}
fn signature(pid: u32) -> Option<String> {
    if !runtime_pid_alive(&pid.to_string()) {
        return None;
    }
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let start = stat.rsplit_once(')')?.1.split_whitespace().nth(19)?;
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    Some(format!("{}:{start}", boot.trim()))
}
fn runtime() -> Result<PathBuf> {
    let uid = unsafe { libc::geteuid() };
    ensure!(uid != 0, "Steam APK wrapper must run as the session user");
    let p = PathBuf::from(format!("/run/user/{uid}/framely-apk-steam"));
    fs::create_dir_all(&p)?;
    let m = fs::symlink_metadata(&p)?;
    ensure!(
        m.is_dir() && m.uid() == uid,
        "Invalid Steam wrapper runtime directory"
    );
    fs::set_permissions(&p, fs::Permissions::from_mode(0o700))?;
    Ok(p)
}
fn socket() -> Result<PathBuf> {
    Ok(runtime()?.join("control.sock"))
}
fn read_request(stream: &mut UnixStream) -> Result<Value> {
    let mut bytes = Vec::new();
    BufReader::new(stream)
        .take(8193)
        .read_until(b'\n', &mut bytes)?;
    ensure!(
        bytes.len() <= 8192 && bytes.last() == Some(&b'\n'),
        "Invalid Steam wrapper request"
    );
    Ok(serde_json::from_slice(&bytes)?)
}
fn peer(stream: &UnixStream) -> Result<u32> {
    let mut c: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    ensure!(
        unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                &mut c as *mut _ as *mut _,
                &mut len,
            )
        } == 0
            && c.uid == unsafe { libc::geteuid() }
            && c.pid > 0,
        "Invalid Steam wrapper peer"
    );
    Ok(c.pid as u32)
}
fn record(home: &Path, id: &str, token: &str) -> Result<Record> {
    let db = load(home)?;
    let r = db.records.get(id).context("Unknown Steam APK entry")?;
    ensure!(
        r.steam_token.as_deref() == Some(token),
        "Invalid Steam APK entry token"
    );
    Ok(r.clone())
}
pub(super) fn current_instance(c: &Container) -> Result<Option<String>> {
    let mut cmd = crate::process::tool("podman");
    cmd.args([
        "inspect",
        "--format",
        "{{/*SteamBridge*/}}{{.Id}}|{{.State.StartedAt}}|{{.State.Running}}|{{.State.Pid}}",
        &format!("lepton-{}", c.name),
    ]);
    let text = match output(cmd, Duration::from_secs(3), None) {
        Ok(text) => text,
        Err(error) => {
            // Lepton removes its transient Podman object when stopped. Only
            // Podman's explicit "absent" exit code is safe to treat as stopped;
            // permission/storage/runtime errors must not authorize a new boot.
            let exists = crate::process::command_output_timeout(
                crate::process::tool("podman").args([
                    "container",
                    "exists",
                    &format!("lepton-{}", c.name),
                ]),
                Duration::from_secs(3),
                false,
            )?;
            if exists.status.code() == Some(1) {
                return Ok(None);
            }
            return Err(error.context("Cannot verify Steam APK container state"));
        }
    };
    let fields: Vec<_> = text.trim().split('|').collect();
    ensure!(
        fields.len() == 4
            && !fields[0].is_empty()
            && !fields[1].is_empty()
            && matches!(fields[2], "true" | "false"),
        "Unknown Steam container state"
    );
    let pid = fields[3]
        .parse::<u32>()
        .context("Unknown container process")?;
    if fields[2] == "false" || pid == 0 || !runtime_pid_alive(&pid.to_string()) {
        return Ok(None);
    }
    Ok(Some(fields[..3].join("|")))
}
fn instance(c: &Container) -> Result<String> {
    current_instance(c)?.context("APK container has stopped")
}
fn save_lease(home: &Path, l: &Lease) -> Result<()> {
    fs::create_dir_all(leases(home))?;
    fs::set_permissions(leases(home), fs::Permissions::from_mode(0o700))?;
    let p = lease_path(home, &l.app);
    let tmp = p.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(&serde_json::to_vec(l)?)?;
    file.sync_all()?;
    fs::rename(tmp, p)?;
    Ok(())
}
fn load_lease(home: &Path, id: &str) -> Result<Lease> {
    let p = lease_path(home, id);
    ensure!(
        fs::metadata(&p)?.len() <= 8192,
        "Invalid Steam wrapper lease"
    );
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
fn owned_status(home: &Path, l: &Lease) -> Result<Value> {
    let db = load(home)?;
    if db.records.get(&l.app).is_none_or(|r| r.removed) {
        return Ok(json!({"owned":false}));
    }
    let (_, c) = app(home, &db, &l.app)?;
    if current_instance(&c)?.as_deref() != Some(&l.instance) {
        return Ok(json!({"owned":false}));
    }
    let s = lifecycle::sample(&c)?;
    let alive = s.alive.contains(&db.records[&l.app].metadata.package);
    Ok(json!({"owned":true,"alive":alive}))
}
/// Stop only the app owned by this exact container instance. Other installed
/// applications in an old shared context retain their processes and data.
fn stop_owned(home: &Path, l: &Lease) -> Result<()> {
    let _guard = MUTATION
        .try_lock()
        .map_err(|_| anyhow::anyhow!("Another APK operation is running"))?;
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(root(home).join("operation.lock"))?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "Another APK operation is running"
    );
    let db = load(home)?;
    if db.records.get(&l.app).is_none_or(|r| r.removed) {
        return Ok(());
    }
    let r = record(home, &l.app, &l.token)?;
    ensure!(
        !db.records
            .values()
            .any(|r| r.context == db.records[&l.app].context && r.pending.is_some()),
        "APK operation is pending"
    );
    let (a, c) = app(home, &db, &l.app)?;
    if current_instance(&c)?.as_deref() != Some(&l.instance) {
        return Ok(());
    }
    let log = root(home)
        .join("logs")
        .join(format!("{}-steam-stop-{}.log", hash(&l.app), now()));
    fs::write(
        &log,
        "Steam wrapper stopped; preserving installed application data\n",
    )?;
    let before = lifecycle::sample(&c)?;
    crate::gamepad::stop_app(&c.name, &r.metadata.package);
    if before.package == a.metadata.package {
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
    let after = lifecycle::sample(&c)?;
    if after.instance == l.instance && after.alive.is_empty() {
        stop(&c, &log)?;
    }
    Ok(())
}
fn handle(home: &Path, pid: u32, v: Value) -> Result<Value> {
    let id = v["app"].as_str().context("Missing Steam APK entry")?;
    let token = v["token"]
        .as_str()
        .context("Missing Steam APK entry token")?;
    match v["method"].as_str() {
        Some("native.prepare") => {
            let r = record(home, id, token)?;
            ensure!(
                !r.removed && r.steam_launch && shortcuts::linked(home, &r),
                "Steam launch is disabled"
            );
            let app_id = v["steamAppId"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .filter(|v| *v >= 0x80000000)
                .context("Launch this APK entry through Steam")?;
            let (c, a, response, ownership) = super::native::prepare(home, id, app_id)?;
            let owner_signature = signature(pid).context("Steam launch exited")?;
            let home = home.to_owned();
            let id = id.to_owned();
            let token = token.to_owned();
            let gamepad_event = response["env"]["FRAMELY_GAMEPAD_EVENT"]
                .as_str()
                .map(PathBuf::from);
            std::thread::spawn(move || {
                let result = (|| -> Result<Value> {
                    let deadline = Instant::now() + Duration::from_secs(120);
                    let creation_deadline = Instant::now() + Duration::from_secs(45);
                    loop {
                        ensure!(
                            signature(pid).as_deref() == Some(&owner_signature),
                            "Native Lepton launch exited; inspect the APK log"
                        );
                        ensure!(
                            Instant::now() < deadline,
                            "Native Lepton startup timed out; inspect the APK log"
                        );
                        let instance = current_instance(&c)?;
                        if instance.is_none() {
                            if let Some(event) = &gamepad_event {
                                ensure!(event.exists(), "Virtual gamepad disappeared before Lepton created its container; retry the launch");
                            }
                            ensure!(Instant::now() < creation_deadline, "Lepton did not create its container; inspect the native Lepton startup log");
                        }
                        if let Some(instance) = instance {
                            if let Ok(s) = lifecycle::sample(&c) {
                                if s.package == a.metadata.package
                                    && s.alive.contains(&a.metadata.package)
                                {
                                    save_lease(
                                        &home,
                                        &Lease {
                                            app: id.clone(),
                                            token: token.clone(),
                                            pid,
                                            signature: owner_signature.clone(),
                                            instance,
                                        },
                                    )?;
                                    if crate::gamepad::current(&c.name).is_some() {
                                        crate::gamepad::activate(&c.name, &a.metadata.package)?;
                                    }
                                    return Ok(json!({"started":true,"native":true}));
                                }
                            }
                        }
                        std::thread::sleep(Duration::from_millis(500));
                    }
                })();
                // A failed Podman run can leave Lepton waiting for a create
                // event forever. End only this verified, still-uncreated launch;
                // never interrupt a container that has already booted.
                if result.is_err() && matches!(current_instance(&c), Ok(None)) {
                    if signature(pid).as_deref() == Some(&owner_signature) {
                        unsafe {
                            libc::kill(pid as i32, libc::SIGTERM);
                        }
                    }
                    crate::gamepad::stop_context(&c.name);
                }
                if let Some(p) = PENDING.lock().unwrap().remove(&id) {
                    if result.is_ok() {
                        (p.progress)(json!({"phase":"started"}));
                    }
                    let _ = p.result.send(result.map_err(|e| format!("{e:#}")));
                }
                // Preserve storage ownership after reporting startup success.
                // Steam drops inherited fds, so the service retains this lock
                // through exit and the container's eventual shutdown.
                while signature(pid).as_deref() == Some(&owner_signature)
                    || !matches!(current_instance(&c), Ok(None))
                {
                    std::thread::sleep(Duration::from_millis(500));
                }
                // The native Lepton entry may remove its container before the
                // lease watcher runs. Release its virtual device here, while
                // we still hold the context lock, so a later launch cannot be
                // affected by an old helper or have its new device removed.
                crate::gamepad::stop_context(&c.name);
                drop(ownership);
            });
            Ok(response)
        }
        Some("start") => {
            let r = record(home, id, token)?;
            ensure!(
                !r.removed && r.steam_launch && shortcuts::linked(home, &r),
                "Steam launch is disabled for this APK"
            );
            let app_id = v["steamAppId"]
                .as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .filter(|v| *v >= 0x80000000)
                .context("Launch this APK entry through Steam")?;
            if let Ok(old) = load_lease(home, id) {
                ensure!(
                    signature(old.pid).as_deref() != Some(&old.signature),
                    "This APK already has an active Steam wrapper"
                );
            }
            let entry_lock = shortcuts::wrapper(home, id).with_extension("launch.lock");
            let lock = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(entry_lock)?;
            ensure!(
                unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
                "Steam APK launch is already in progress"
            );
            let db = load(home)?;
            let (_a, c) = app(home, &db, id)?;
            ensure!(current_instance(&c)?.is_none(),"Close the running container before its first Steam launch; existing applications will not be restarted automatically");
            let progress = PENDING
                .lock()
                .unwrap()
                .get(id)
                .map(|p| p.progress.clone())
                .unwrap_or_else(|| Arc::new(|_| {}));
            let signature = signature(pid).context("Steam wrapper exited before launch")?;
            let owner_home = home.to_owned();
            let owner_app = id.to_owned();
            let owner_token = token.to_owned();
            let owner_context = c.clone();
            let marked = std::sync::atomic::AtomicBool::new(false);
            let owner_progress: Progress = Arc::new(move |v| {
                if matches!(v["phase"].as_str(), Some("launching" | "started"))
                    && !marked.load(Ordering::Relaxed)
                {
                    if let Ok(instance) = instance(&owner_context) {
                        let lease = Lease {
                            app: owner_app.clone(),
                            token: owner_token.clone(),
                            pid,
                            signature: signature.clone(),
                            instance,
                        };
                        if save_lease(&owner_home, &lease).is_ok() {
                            marked.store(true, Ordering::Relaxed);
                        }
                    }
                }
                progress(v);
            });
            let launched = operate_internal(
                home,
                "launch",
                &json!({"app":id}),
                Cancellation::default(),
                owner_progress,
                Some(app_id),
            );
            let response = launched.and_then(|_| {
                let lease =
                    load_lease(home, id).context("Cannot record the Steam-owned container")?;
                ensure!(
                    lease.pid == pid && instance(&c)? == lease.instance,
                    "Steam-owned container changed during startup"
                );
                Ok(json!({"started":true}))
            });
            if response.is_err() {
                if let Ok(lease) = load_lease(home, id) {
                    if lease.pid == pid && stop_owned(home, &lease).is_ok() {
                        let _ = fs::remove_file(lease_path(home, id));
                    }
                }
            }
            if let Some(p) = PENDING.lock().unwrap().remove(id) {
                let _ = p.result.send(
                    response
                        .as_ref()
                        .map(Clone::clone)
                        .map_err(|e| format!("{e:#}")),
                );
            }
            response
        }
        Some("status") | Some("stop") => {
            let l = load_lease(home, id)?;
            ensure!(
                l.pid == pid && l.token == token && signature(pid).as_deref() == Some(&l.signature),
                "Steam wrapper lease does not belong to this process"
            );
            if v["method"] == "stop" {
                stop_owned(home, &l)?;
                let _ = fs::remove_file(lease_path(home, id));
                Ok(json!(true))
            } else {
                owned_status(home, &l)
            }
        }
        _ => bail!("Invalid Steam wrapper method"),
    }
}
pub(super) fn start_session() -> Result<()> {
    let home = crate::steam::home()?;
    let dir = runtime()?;
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(dir.join("server.lock"))?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "Steam APK bridge is already running"
    );
    let path = socket()?;
    if path.exists() {
        ensure!(
            fs::symlink_metadata(&path)?.file_type().is_socket(),
            "Invalid Steam APK socket"
        );
        fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    std::thread::spawn(move || {
        let _lock = lock;
        let workers = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            if workers.fetch_add(1, Ordering::Relaxed) >= 8 {
                workers.fetch_sub(1, Ordering::Relaxed);
                continue;
            }
            let home = home.clone();
            let workers = workers.clone();
            std::thread::spawn(move || {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                let result = peer(&stream).and_then(|pid| {
                    read_request(&mut stream).and_then(|v| {
                        let id = v["app"].as_str().unwrap_or("").to_owned();
                        let start = v["method"] == "start" || v["method"] == "native.prepare";
                        if start {
                            record(&home, &id, v["token"].as_str().unwrap_or(""))?;
                            ensure!(
                                STARTING.lock().unwrap().insert(id.clone()),
                                "Steam APK launch is already in progress"
                            );
                        }
                        let result = handle(&home, pid, v);
                        if start {
                            if result.is_err() {
                                if let Some(p) = PENDING.lock().unwrap().remove(&id) {
                                    let _ = p
                                        .result
                                        .send(Err(format!("{:#}", result.as_ref().unwrap_err())));
                                }
                            }
                            STARTING.lock().unwrap().remove(&id);
                        }
                        result
                    })
                });
                let _ = ipc::write(&mut stream, &ipc::response(result));
                workers.fetch_sub(1, Ordering::Relaxed);
            });
        }
    });
    let home = crate::steam::home()?;
    std::thread::spawn(move || loop {
        if let Ok(files) = fs::read_dir(leases(&home)) {
            for entry in files.flatten().take(64) {
                if entry.path().extension().is_none_or(|s| s != "json")
                    || !entry
                        .metadata()
                        .is_ok_and(|m| m.is_file() && m.len() <= 8192)
                {
                    continue;
                }
                let Ok(bytes) = fs::read(entry.path()) else {
                    continue;
                };
                let Ok(l) = serde_json::from_slice::<Lease>(&bytes) else {
                    continue;
                };
                if entry.path() != lease_path(&home, &l.app) {
                    continue;
                }
                if signature(l.pid).as_deref() != Some(&l.signature) {
                    if load(&home).is_ok_and(|db| !db.records.contains_key(&l.app)) {
                        let _ = fs::remove_file(entry.path());
                        continue;
                    }
                    if stop_owned(&home, &l).is_ok() {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    });
    Ok(())
}
fn focus_owned(home: &Path, lease: &Lease, progress: &Progress) -> Result<Value> {
    let _guard = MUTATION
        .try_lock()
        .map_err(|_| anyhow::anyhow!("Another APK operation is running"))?;
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(root(home).join("operation.lock"))?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "Another APK operation is running"
    );
    let db = load(home)?;
    let (a, c) = app(home, &db, &lease.app)?;
    ensure!(
        signature(lease.pid).as_deref() == Some(&lease.signature)
            && current_instance(&c)?.as_deref() == Some(&lease.instance),
        "Previous Steam launch is closing; retry after it stops"
    );
    let log =
        root(home)
            .join("logs")
            .join(format!("{}-steam-focus-{}.log", hash(&lease.app), now()));
    launch_with_started(&c, &a, &log, || progress(json!({"phase":"started"})))?;
    Ok(json!({"started":true,"reused":true}))
}
pub(super) fn request_launch(
    home: &Path,
    id: &str,
    progress: Progress,
    cancel: &Cancellation,
) -> Result<Value> {
    let db = load(home)?;
    let r = db.records.get(id).context("APK entry not found")?;
    ensure!(
        r.steam_launch && shortcuts::linked(home, r),
        "Steam entry is missing; re-enable Steam launch in APK management"
    );
    let binding = r.steam_binding.as_ref().context("Missing Steam binding")?;
    if let Ok(lease) = load_lease(home, id) {
        if signature(lease.pid).as_deref() == Some(&lease.signature) {
            return cancel.commit(|| {
                shortcuts::rpc(home, "run-game", Some(&binding.game_id))?;
                progress(json!({"cancellable":false}));
                focus_owned(home, &lease, &progress)
            });
        }
    }

    let (tx, rx) = mpsc::channel();
    let ticket = rand::random::<u64>();
    {
        let mut p = PENDING.lock().unwrap();
        ensure!(
            !p.contains_key(id) && !STARTING.lock().unwrap().contains(id),
            "This APK is already launching through Steam"
        );
        p.insert(
            id.into(),
            Pending {
                ticket,
                progress: progress.clone(),
                result: tx,
            },
        );
    }
    let result = (|| {
        cancel.commit(|| shortcuts::rpc(home, "run-game", Some(&binding.game_id)))?;
        progress(json!({"cancellable":false}));
        progress(json!({"phase":"starting"}));
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(result) => return result.map_err(anyhow::Error::msg),
                Err(mpsc::RecvTimeoutError::Disconnected) => bail!("Steam APK launch connection closed"),
                Err(mpsc::RecvTimeoutError::Timeout) => ensure!(Instant::now()<deadline,"Steam did not start the APK entry. Check its compatibility and launch options in Steam."),
            }
        }
    })();
    {
        let mut pending = PENDING.lock().unwrap();
        if pending.get(id).is_some_and(|p| p.ticket == ticket) {
            pending.remove(id);
        }
    }
    result
}
pub(super) fn native_call(app: &str, token: &str, app_id: u32) -> Result<Value> {
    call_with_id(app, token, "native.prepare", 15, Some(app_id))
}
fn call(app: &str, token: &str, method: &str, timeout: u64) -> Result<Value> {
    call_with_id(app, token, method, timeout, None)
}
fn call_with_id(
    app: &str,
    token: &str,
    method: &str,
    timeout: u64,
    owner: Option<u32>,
) -> Result<Value> {
    let mut s = UnixStream::connect(socket()?).context("Framely UI session is unavailable")?;
    s.set_read_timeout(Some(Duration::from_secs(timeout)))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))?;
    let app_id = owner.or_else(|| {
        std::env::var("SteamAppId")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
    });
    ipc::write(
        &mut s,
        &json!({"method":method,"app":app,"token":token,"steamAppId":app_id}),
    )?;
    let r = ipc::read(&mut s)?;
    if let Some(e) = r["error"].as_str() {
        bail!("{e}")
    }
    Ok(r["result"].clone())
}
pub(crate) fn run(app: &str, token: &str) -> Result<()> {
    STOPPED.store(false, Ordering::Relaxed);
    unsafe {
        libc::signal(
            libc::SIGTERM,
            interrupted as *const () as libc::sighandler_t,
        );
        libc::signal(libc::SIGINT, interrupted as *const () as libc::sighandler_t);
        libc::signal(libc::SIGHUP, interrupted as *const () as libc::sighandler_t);
    }
    // Duplicate shortcut executions cannot interfere with an existing wrapper.
    let home = crate::steam::home()?;
    let p = shortcuts::wrapper(&home, app).with_extension("owner.lock");
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(p)?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "This APK already has an active Steam wrapper"
    );
    call(app, token, "start", 180)?;
    let mut absent = None;
    while !STOPPED.load(Ordering::Relaxed) {
        match call(app, token, "status", 5) {
            Ok(v) if v["owned"] == false => return Ok(()),
            Ok(v) if v["alive"] == false => {
                let t = *absent.get_or_insert_with(Instant::now);
                if t.elapsed() >= Duration::from_secs(15) {
                    break;
                }
            }
            _ => absent = None,
        }
        for _ in 0..20 {
            if STOPPED.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    // The session's persisted owner-PID monitor also handles SIGKILL and reconnects
    // across Framely updates. A failed stop request must not trigger a reinstall.
    let _ = call(app, token, "stop", 35);
    Ok(())
}

#[cfg(test)]
pub(super) fn test_request(home: &Path, app: &str, token: &str, method: &str) -> Result<Value> {
    if method == "focus" {
        let progress: Progress = Arc::new(|_| {});
        return focus_owned(home, &load_lease(home, app)?, &progress);
    }
    handle(
        home,
        std::process::id(),
        json!({"app":app,"token":token,"method":method,"steamAppId":0x92345678u32}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn owner_signature_is_bound_to_process_start() {
        let pid = std::process::id();
        let a = signature(pid).unwrap();
        assert_eq!(signature(pid), Some(a));
        assert!(signature(u32::MAX).is_none());
    }
    #[test]
    fn unix_peer_and_bounded_requests() {
        let (mut a, mut b) = UnixStream::pair().unwrap();
        assert_eq!(peer(&a).unwrap(), std::process::id());
        ipc::write(&mut b, &json!({"method":"status"})).unwrap();
        assert_eq!(read_request(&mut a).unwrap()["method"], "status");
        b.write_all(&vec![b'x'; 8193]).unwrap();
        assert!(read_request(&mut a).is_err());
    }
}

#[cfg(test)]
pub(super) fn test_adopt(home: &Path, id: &str, token: &str) -> Result<()> {
    let db = load(home)?;
    let (_, c) = app(home, &db, id)?;
    save_lease(
        home,
        &Lease {
            app: id.into(),
            token: token.into(),
            pid: std::process::id(),
            signature: signature(std::process::id()).unwrap(),
            instance: instance(&c)?,
        },
    )
}
