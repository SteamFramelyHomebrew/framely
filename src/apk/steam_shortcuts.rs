//! Valve Devkit IPC registration; never edits Steam's live shortcuts database.
use super::*;
use std::os::unix::fs::OpenOptionsExt;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Binding {
    pub game_id: String,
    #[serde(default)]
    pub native: bool,
    #[serde(default)]
    pub revision: String,
}
pub(super) fn game_id(id: &str) -> String {
    format!("framely{}", hash(id))
}
pub(super) fn native_apk(home: &Path, id: &str) -> PathBuf {
    home.join("devkit-game").join(game_id(id)).join("base.apk")
}
pub(super) fn wrapper(home: &Path, id: &str) -> PathBuf {
    home.join("devkit-game").join(game_id(id)).join("launch.sh")
}
pub(super) fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
pub(super) fn write_owned(p: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let parent = p.parent().context("Missing entry parent")?;
    ensure!(
        fs::symlink_metadata(parent)?.is_dir(),
        "Invalid Devkit entry directory"
    );
    if let Ok(m) = fs::symlink_metadata(p) {
        ensure!(
            m.is_file() && !m.file_type().is_symlink(),
            "Invalid Devkit entry file"
        );
    }
    let tmp = parent.join(format!(
        ".framely-{}.tmp",
        hex::encode(rand::random::<[u8; 16]>())
    ));
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, p)?;
    Ok(())
}
/// Same authenticated local pipe protocol as Valve's steam-devkit-rpc.
/// Token and complete command must never be logged.
pub(super) fn rpc(home: &Path, command: &str, id: Option<&str>) -> Result<String> {
    ensure!(
        matches!(
            command,
            "create-shortcut" | "list-shortcuts" | "run-game" | "delete-shortcut"
        ),
        "Invalid Devkit command"
    );
    if let Some(id) = id {
        ensure!(
            !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "Invalid Devkit game ID"
        );
    }
    let steam = home.join(".steam");
    let token_path = steam.join("steam.token");
    ensure!(
        fs::metadata(&token_path)?.len() <= 256,
        "Invalid Steam authentication file"
    );
    let token = fs::read_to_string(token_path)?;
    let token = token.trim();
    ensure!(
        !token.is_empty() && token.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid Steam authentication token"
    );
    let pid = fs::read_to_string(steam.join("steam.pid"))?;
    ensure!(runtime_pid_alive(pid.trim()), "Steam is not running");
    // Steam's Devkit response writer accepts /tmp paths, but silently ignores
    // responses in the application's persistent home directory on Frame.
    // Match Valve's helper, using a private directory independent of TMPDIR.
    let directory = tempfile::Builder::new()
        .prefix("framely-steam-rpc-")
        .tempdir_in("/tmp")?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    let response = directory.path().join("response");
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    q.append_pair(
        "response",
        response.to_str().context("Invalid response path")?,
    );
    if let Some(id) = id {
        q.append_pair("gameid", id);
    }
    q.append_pair(
        "directory",
        home.join("devkit-game")
            .to_str()
            .context("Invalid Devkit path")?,
    );
    let pipe_path = steam.join("steam.pipe");
    ensure!(
        fs::metadata(&pipe_path)?.file_type().is_fifo(),
        "Steam IPC pipe is unavailable"
    );
    let mut pipe = fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(pipe_path)
        .context("Steam IPC is unavailable")?;
    let command = format!(
        "devkit-1 steam://devkit-1/{token}/{command}/?{}\n",
        q.finish()
    );
    ensure!(command.len() < 4096, "Devkit request too large");
    pipe.write_all(command.as_bytes())?;
    drop(pipe);
    let error = response.with_extension("error");
    let lock = response.with_extension("lock");
    let result = (|| {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if error.exists() {
                ensure!(
                    fs::metadata(&error)?.len() <= 8192,
                    "Invalid Steam response"
                );
                bail!("Steam Devkit: {}", fs::read_to_string(&error)?.trim());
            }
            if response.exists() && !lock.exists() {
                ensure!(
                    fs::metadata(&response)?.len() <= 65536,
                    "Steam response too large"
                );
                return Ok(fs::read_to_string(&response)?);
            }
            ensure!(
                Instant::now() < deadline,
                "Steam did not acknowledge the Devkit request; keep Steam awake and retry"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    })();
    for p in [&response, &error, &lock] {
        let _ = fs::remove_file(p);
    }
    result
}
pub(super) fn linked(home: &Path, r: &Record) -> bool {
    r.steam_binding.as_ref().is_some_and(|b| {
        b.game_id == game_id(&r.id)
            && if b.native {
                native_apk(home, &r.id).is_file()
            } else {
                wrapper(home, &r.id).is_file()
            }
    })
}
pub(super) fn register(home: &Path, a: &App, r: &mut Record) -> Result<()> {
    ensure!(
        a.installed && a.state_known && !a.steam,
        "Only installed sideloaded APKs can use a Steam entry"
    );
    let token = r
        .steam_token
        .get_or_insert_with(|| hex::encode(rand::random::<[u8; 32]>()))
        .clone();
    let game = game_id(&a.id);
    let p = wrapper(home, &a.id);
    let dir = p.parent().unwrap();
    fs::create_dir_all(dir)?;
    ensure!(
        fs::symlink_metadata(dir)?.is_dir(),
        "Invalid Steam entry directory"
    );
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    let db = load(home)?;
    let (_, c) = app(home, &db, &a.id)?;
    let apk = installed(home, &c)?
        .into_iter()
        .find(|(m, _)| m.package == a.metadata.package)
        .map(|(_, p)| p)
        .context("Installed APK is unavailable; existing data was not changed")?;
    store_apk(&apk, dir)?;
    // A stable APK target lets Steam own the actual Lepton compatibility launch.
    for (suffix, value) in [
        ("argv", json!(["./base.apk"])),
        ("env", json!({"LEPTON_NO_CLEANUP":"true"})),
        (
            "settings",
            json!({"compat_tool":"fauxdroid","steam_play":"0"}),
        ),
    ] {
        write_owned(
            &home
                .join("devkit-game")
                .join(format!("{game}-{suffix}.json")),
            &serde_json::to_vec(&value)?,
            0o600,
        )?;
    }
    let marker = dir.join("lepton-show-flatscreen");
    if a.show_window.unwrap_or(!a.metadata.vr) {
        write_owned(&marker, b"", 0o600)?;
    } else if marker.exists() {
        fs::remove_file(&marker)?;
    }
    rpc(home, "create-shortcut", Some(&game))?;
    let v: Value = serde_json::from_str(&rpc(home, "list-shortcuts", None)?)?;
    ensure!(
        v["version"] == 2
            && v["gameids"]
                .as_array()
                .is_some_and(|v| v.iter().any(|v| v == &game)),
        "Steam did not register the APK entry"
    );
    let launch = format!(
        "/var/lib/framely/current/bin/framely apk-native --app {} --token {} -- %command%",
        quote(&a.id),
        quote(&token)
    );
    let icon = icon_file(dir, &a.metadata)?;
    r.steam_app_id = Some(super::steam_ui::configure(
        &game,
        r.steam_app_id,
        &a.metadata.name,
        icon.as_deref(),
        &launch,
        &native_apk(home, &a.id),
    )?);
    r.steam_binding = Some(Binding {
        game_id: game,
        native: true,
        revision: revision(a),
    });
    Ok(())
}
pub(super) fn unregister(home: &Path, r: &Record) -> Result<()> {
    if let Some(b) = &r.steam_binding {
        ensure!(b.game_id == game_id(&r.id), "Invalid Steam APK binding");
        rpc(home, "delete-shortcut", Some(&b.game_id))?;
        // Remove only our fixed configuration files, so the Devkit sync cannot
        // re-register a disabled entry. Never recursively remove user content.
        for suffix in ["argv", "settings", "env"] {
            let p = home
                .join("devkit-game")
                .join(format!("{}-{suffix}.json", b.game_id));
            if p.exists() {
                fs::remove_file(p)?;
            }
        }
        let p = wrapper(home, &r.id);
        if p.exists() {
            fs::remove_file(&p)?;
        }
        for name in [
            "base.apk",
            "icon.png",
            "icon.webp",
            "icon.jpg",
            "lepton-show-flatscreen",
        ] {
            let file = p.parent().unwrap().join(name);
            if file.exists() {
                ensure!(
                    fs::symlink_metadata(&file)?.is_file(),
                    "Invalid owned Steam file"
                );
                fs::remove_file(file)?;
            }
        }
        let _ = fs::remove_dir(p.parent().unwrap());
    }
    Ok(())
}
fn revision(a: &App) -> String {
    hash(&json!({"metadata":a.metadata,"showWindow":a.show_window}).to_string())
}
fn icon_file(dir: &Path, metadata: &Metadata) -> Result<Option<PathBuf>> {
    use base64::Engine;
    let Some(icon) = metadata.icon.as_deref() else {
        return Ok(None);
    };
    let Some((prefix, data)) = icon.split_once(",") else {
        return Ok(None);
    };
    ensure!(
        matches!(
            prefix,
            "data:image/png;base64" | "data:image/jpeg;base64" | "data:image/webp;base64"
        ),
        "Unsupported APK icon"
    );
    ensure!(data.len() <= 8 * 1024 * 1024, "APK icon is too large");
    let bytes = base64::engine::general_purpose::STANDARD.decode(data)?;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let picture = reader.decode()?.thumbnail(512, 512);
    let path = dir.join("icon.png");
    let mut png = std::io::Cursor::new(Vec::new());
    picture.write_to(&mut png, image::ImageFormat::Png)?;
    write_owned(&path, &png.into_inner(), 0o600)?;
    Ok(Some(path))
}
// Preference and registration are separate: Steam can be asleep while the
// default-on choice remains enabled. An explicit opt-out is never auto-enabled.
pub(super) fn wanted(r: &Record) -> bool {
    r.steam_preference
        .unwrap_or(r.steam_launch || r.steam_token.is_none())
}
pub(super) fn ensure_registered(home: &Path, id: &str, force: bool) -> Result<bool> {
    let _guard = MUTATION
        .try_lock()
        .map_err(|_| anyhow::anyhow!("Another APK operation is running"))?;
    let directory = init(home)?;
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(directory.join("operation.lock"))?;
    ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "Another APK operation is running"
    );
    let mut db = load(home)?;
    if db.records.get(id).is_some_and(|r| !wanted(r)) {
        return Ok(false);
    }
    let (a, _) = app(home, &db, id)?;
    if a.steam {
        return Ok(false);
    }
    ensure!(
        a.installed && a.state_known && a.pending.is_none(),
        "Application state is unavailable for Steam registration"
    );
    ensure!(
        !a.metadata.activities.is_empty() || a.activity.is_some(),
        "No launch activity is available for Steam registration"
    );
    record_app(&mut db, &a);
    let rec = db.records.get_mut(id).context("Missing APK record")?;
    rec.steam_preference = Some(true);
    if !force
        && rec.steam_launch
        && linked(home, rec)
        && rec
            .steam_binding
            .as_ref()
            .is_some_and(|b| b.native && b.revision == revision(&a))
    {
        save(home, &db)?;
        return Ok(true);
    }
    let result = register(home, &a, rec);
    rec.steam_launch = result.is_ok();
    rec.steam_registration_error = result.as_ref().err().map(|e| format!("{e:#}"));
    save(home, &db)?;
    result?;
    Ok(true)
}
static AUTO_WAKE: std::sync::OnceLock<std::sync::mpsc::SyncSender<()>> = std::sync::OnceLock::new();
pub(super) fn wake_registration() {
    if let Some(sender) = AUTO_WAKE.get() {
        let _ = sender.try_send(());
    }
}
pub(super) fn synchronize(home: &Path) -> Result<()> {
    // Query Steam once per pass, before inventory scanning; unavailable Steam
    // defers registration without blocking UI startup or starting any container.
    let v: Value = serde_json::from_str(&rpc(home, "list-shortcuts", None)?)?;
    ensure!(v["version"] == 2, "Unknown Steam Devkit registry");
    let registered: std::collections::BTreeSet<String> =
        serde_json::from_value(v["gameids"].clone())?;
    let db = load(home)?;
    let cs = containers(home, &db);
    let (inventory, _) = apps(home, &db, &cs, false);
    for a in inventory {
        if !a.installed
            || !a.state_known
            || a.steam
            || a.pending.is_some()
            || (a.metadata.activities.is_empty() && a.activity.is_none())
        {
            continue;
        }
        if db.records.get(&a.id).is_some_and(|r| !wanted(r)) {
            continue;
        }
        let force = !registered.contains(&game_id(&a.id));
        if !force
            && db.records.get(&a.id).is_some_and(|r| {
                r.steam_preference == Some(true)
                    && r.steam_launch
                    && linked(home, r)
                    && r.steam_binding
                        .as_ref()
                        .is_some_and(|b| b.native && b.revision == revision(&a))
            })
        {
            continue;
        }
        let _ = ensure_registered(home, &a.id, force);
    }
    Ok(())
}
pub(super) fn start_auto_registration() {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    if AUTO_WAKE.set(tx).is_err() {
        return;
    }
    std::thread::spawn(move || loop {
        if let Ok(home) = crate::steam::home() {
            let _ = synchronize(&home);
        }
        let _ = rx.recv_timeout(Duration::from_secs(20));
        for _ in 0..64 {
            if rx.try_recv().is_err() {
                break;
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferences_migrate_default_on_and_preserve_legacy_explicit_disables() {
        let mut r = Record::default();
        assert!(wanted(&r));
        let mut value = serde_json::to_value(&r).unwrap();
        value.as_object_mut().unwrap().remove("steamPreference");
        assert!(wanted(&serde_json::from_value::<Record>(value).unwrap()));
        r.steam_token = Some("legacy".into());
        assert!(!wanted(&r));
        r.steam_launch = true;
        assert!(wanted(&r));
        r.steam_preference = Some(false);
        assert!(!wanted(&r));
        r.steam_preference = Some(true);
        r.steam_launch = false;
        assert!(wanted(&r));
    }
    #[test]
    fn devkit_ids_are_stable_safe_and_unique() {
        let a = game_id("ctx/com.test.app");
        assert_eq!(a, game_id("ctx/com.test.app"));
        assert_ne!(a, game_id("other/com.test.app"));
        assert!(a.bytes().all(|b| b.is_ascii_alphanumeric()));
    }
    #[test]
    fn owned_entry_rejects_symlinks() {
        let t = tempfile::tempdir().unwrap();
        let target = t.path().join("data");
        fs::write(&target, b"preserve").unwrap();
        let p = t.path().join("launch.sh");
        std::os::unix::fs::symlink(&target, &p).unwrap();
        assert!(write_owned(&p, b"replace", 0o700).is_err());
        assert_eq!(fs::read(target).unwrap(), b"preserve");
    }
}
