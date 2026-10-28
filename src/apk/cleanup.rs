//! Delete only storage and runtime objects attributed to one APK context.
use super::*;
use std::collections::BTreeSet;

fn absent(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotFound
}
fn directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) => ensure!(
            m.is_dir() && m.uid() == unsafe { libc::geteuid() },
            "Invalid cleanup directory"
        ),
        Err(e) if absent(&e) => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
// A failed first boot has no packages.xml. Only explicit Framely ownership
// plus an exact, non-symlink managed path may replace that validation.
pub(super) fn validate_location(home: &Path, db: &Database, c: &Container) -> Result<()> {
    if let Ok(path) = validate_baked(&c.baked) {
        ensure!(path == c.baked, "Container data location changed");
        return Ok(());
    }
    ensure!(
        c.managed
            && !c.steam
            && c.id == c.name
            && safe(&c.id)
            && !c.id.starts_with("steamlaunch-")
            && db.owned_contexts.contains(&c.id),
        "Unreadable containers can only be cleaned when owned by Framely"
    );
    let base = contexts_root(home);
    ensure!(
        base.is_absolute() && fs::canonicalize(&base)? == base,
        "Managed container location changed"
    );
    let parent = base.join(&c.id);
    ensure!(
        c.baked == parent.join("baked"),
        "Container data location changed"
    );
    for path in [&base, &parent, &c.baked] {
        directory(path)?;
        if path.exists() {
            ensure!(
                fs::canonicalize(path)? == *path,
                "Container cleanup path contains a link"
            );
        }
    }
    Ok(())
}
pub(super) fn ensure_runtime_absent(c: &Container) -> Result<()> {
    ensure!(
        runtime(&c.name, &c.baked, true)?.is_none(),
        "Container runtime already exists; refusing to replace it"
    );
    Ok(())
}
fn identity(path: &Path) -> Result<Option<(u64, u64)>> {
    match fs::symlink_metadata(path) {
        Ok(m) => Ok(Some((m.dev(), m.ino()))),
        Err(e) if absent(&e) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn remove_owned(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) => {
            ensure!(
                m.uid() == unsafe { libc::geteuid() },
                "Cleanup file belongs to another user"
            );
            remove_tree(path)?;
        }
        Err(e) if absent(&e) => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}
fn data_mount_owned(mounts: &[Value], baked: &Path) -> bool {
    if mounts
        .iter()
        .filter(|m| m["Destination"] == "/data")
        .count()
        != 1
    {
        return false;
    }
    let Ok(baked) = fs::canonicalize(baked) else {
        return false;
    };
    let exact = |value: &str, relative: &str| {
        let expected = baked.join(relative);
        fs::canonicalize(value).is_ok_and(|path| path == expected)
            && fs::canonicalize(&expected).is_ok_and(|path| path == expected)
    };
    mounts.iter().any(|mount| {
        if mount["Destination"] != "/data" {
            return false;
        }
        // Normal Lepton uses a shared sysbake as the read-only lower layer.
        // Ownership comes from BOTH private writable OverlayFS directories,
        // never from the shared Source or the runtime name alone.
        let overlay_options = mount["Options"].as_array().is_some_and(|options| {
            options
                .iter()
                .filter_map(Value::as_str)
                .any(|s| s.starts_with("upperdir=") || s.starts_with("workdir="))
        });
        if mount["Type"] == "overlay" || overlay_options {
            let Some(options) = mount["Options"].as_array() else {
                return false;
            };
            let option = |name: &str| -> Option<&str> {
                let values: Vec<_> = options
                    .iter()
                    .filter_map(Value::as_str)
                    .filter_map(|s| s.strip_prefix(name))
                    .collect();
                (values.len() == 1).then(|| values[0])
            };
            option("upperdir=").is_some_and(|p| exact(p, "data_overlay"))
                && option("workdir=").is_some_and(|p| exact(p, "data_workdir"))
        } else {
            mount["Source"]
                .as_str()
                .is_some_and(|p| exact(p, "data_overlay") || exact(p, ""))
        }
    })
}

fn runtime(name: &str, baked: &Path, original: bool) -> Result<Option<String>> {
    ensure!(safe(name), "Invalid container name");
    let result = crate::process::command_output_timeout(
        crate::process::tool("podman").args([
            "inspect",
            "--format",
            "{{.Id}}|{{ index .Config.Labels \"STEAM_COMPAT_DATA_PATH\" }}|{{json .Mounts}}",
            &format!("lepton-{name}"),
        ]),
        Duration::from_secs(10),
        true,
    )?;
    if !result.status.success() {
        let error = String::from_utf8_lossy(&result.stderr).to_lowercase();
        ensure!(
            error.contains("no such") || error.contains("does not exist"),
            "Cannot verify container ownership"
        );
        return Ok(None);
    }
    let text = String::from_utf8(result.stdout)?;
    let mut fields = text.trim().splitn(3, '|');
    let id = fields.next().context("Unknown container identity")?;
    let location = fields
        .next()
        .context("Invalid container ownership response")?;
    let mounts = fields.next().context("Missing container mounts")?;
    ensure!(!id.is_empty(), "Unknown container identity");
    if location.is_empty() || location == "<no value>" {
        // Older direct runtimes have no compatdata label. Verify their actual
        // Android data mount as well as the unambiguous original name.
        ensure!(original, "Cannot verify Steam container storage");
        let mounts: Vec<Value> = serde_json::from_str(mounts)?;
        ensure!(data_mount_owned(&mounts, baked),
            "Container data mount belongs to another location; expected private data under {}; observed /data mounts: {}",
            baked.display(), json!(mounts.iter().filter(|mount| mount["Destination"] == "/data").collect::<Vec<_>>()));
    } else {
        ensure!(
            fs::canonicalize(Path::new(location).join("baked"))? == baked,
            "Container belongs to another data location"
        );
    }
    Ok(Some(id.to_owned()))
}

pub(super) fn delete(home: &Path, db: &mut Database, c: &Container, log: &Path) -> Result<()> {
    delete_internal(home, db, c, log, false)
}
pub(super) fn delete_failed_install(
    home: &Path,
    db: &mut Database,
    c: &Container,
    log: &Path,
) -> Result<()> {
    delete_internal(home, db, c, log, true)
}
fn delete_internal(
    home: &Path,
    db: &mut Database,
    c: &Container,
    log: &Path,
    preserve_logs: bool,
) -> Result<()> {
    validate_location(home, db, c)?;
    let records: Vec<_> = db
        .records
        .values()
        .filter(|r| r.context == c.id)
        .cloned()
        .collect();
    let steam = root(home).join("steam");
    directory(&steam)?;
    directory(&steam.join("leases"))?;
    for rec in &records {
        let lease = steam.join("leases").join(format!("{}.json", hash(&rec.id)));
        if let Ok(m) = fs::symlink_metadata(&lease) {
            if m.is_file() {
                ensure!(
                    m.len() <= 8192 && m.uid() == unsafe { libc::geteuid() },
                    "Invalid Steam lease file"
                );
                let value: Value = serde_json::from_slice(&fs::read(&lease)?)?;
                ensure!(
                    value["app"] == rec.id,
                    "Steam lease belongs to another application"
                );
            }
        }
    }
    let original = if c.id.starts_with("external-") {
        db.root_contexts
            .get(&c.baked)
            .cloned()
            .unwrap_or_else(|| c.name.clone())
    } else {
        c.id.clone()
    };
    let standard = (if db.owned_contexts.contains(&c.id) {
        super::contexts_root(home)
    } else {
        home.join(".local/share/lepton/contexts")
    })
    .join(&c.id);
    let default_location = !c.id.starts_with("external-")
        && !fs::symlink_metadata(&standard).is_ok_and(|m| m.file_type().is_symlink())
        && fs::canonicalize(&standard).is_ok_and(|p| p == standard && p.join("baked") == c.baked);
    let target = if default_location {
        standard
    } else {
        c.baked.clone()
    };
    let target_identity = identity(&target)?;
    let mut names = BTreeSet::from([original.clone(), c.name.clone()]);
    names.extend(records.iter().filter_map(native::runtime_name));
    // Resolve every name before stopping anything. A recycled Steam App ID
    // pointing at different storage must never be removed.
    let mut runtimes = Vec::new();
    for name in &names {
        if let Some(id) = runtime(name, &c.baked, *name == original)? {
            runtimes.push((name.clone(), id));
        }
    }
    for (name, _) in &runtimes {
        let mut owned = c.clone();
        owned.name = name.clone();
        stop(&owned, log)?;
    }
    fs::create_dir_all(&steam)?;
    let ownership = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(steam.join(format!("context-{}.lock", hash(&c.id))))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while unsafe { libc::flock(ownership.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        ensure!(
            Instant::now() < deadline,
            "Steam launch still owns this container; wait for it to close and retry"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    for rec in &records {
        steam_shortcuts::unregister(home, rec)?;
    }
    validate_location(home, db, c)?;
    ensure!(
        identity(&target)? == target_identity,
        "Container directory changed during deletion"
    );
    ensure!(
        !names.iter().any(|name| wrapper_active(name)),
        "A container launcher is still running; wait for it to close"
    );
    for (name, identity) in &runtimes {
        if let Some(current) = runtime(name, &c.baked, *name == original)? {
            ensure!(
                current == *identity && !running(name),
                "Container changed during deletion"
            );
            podman(&["rm", identity], Some(log))?;
        }
    }
    let mut keys = BTreeSet::from([hash(&c.id)]);
    for rec in &records {
        let key = hash(&rec.id);
        keys.insert(key.clone());
        remove_owned(&root(home).join("apks").join(&key))?;
        remove_owned(&steam.join(format!("native-{key}.sh")))?;
        let lease = steam.join("leases").join(format!("{key}.json"));
        remove_owned(&lease)?;
    }
    for entry in fs::read_dir(root(home).join("logs"))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !preserve_logs
            && entry.path() != log
            && name.ends_with(".log")
            && keys.iter().any(|key| name.starts_with(&format!("{key}-")))
        {
            remove_owned(&entry.path())?;
        }
    }
    validate_location(home, db, c)?;
    ensure!(
        identity(&target)? == target_identity,
        "Container directory changed during deletion"
    );
    remove_owned(&target)?;
    db.roots.retain(|p| p != &c.baked);
    db.root_contexts.remove(&c.baked);
    db.records.retain(|_, r| r.context != c.id);
    db.owned_contexts.retain(|id| id != &c.id);
    save(home, db)?;
    // Keep the context lock inode: unlinking it can split mutual exclusion.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlay_ownership_uses_private_upper_and_work_dirs_not_shared_sysbake() {
        let dir = tempfile::tempdir().unwrap();
        let baked = dir.path().join("baked");
        for path in ["data_overlay", "data_workdir"] {
            fs::create_dir_all(baked.join(path)).unwrap();
        }
        let mount = json!({"Type":"overlay", "Source":"/shared/lepton/sysbake", "Destination":"/data",
            "Options":[format!("upperdir={}", baked.join("data_overlay").display()), format!("workdir={}", baked.join("data_workdir").display())]});
        assert!(data_mount_owned(&[mount.clone()], &baked));
        assert!(!data_mount_owned(&[mount.clone(), mount.clone()], &baked));
        // Podman inspect serializes OCI OverlayFS mounts as Type=bind.
        let mut inspected = mount.clone();
        inspected["Type"] = json!("bind");
        inspected["Source"] = json!("/podman/overlay/merged");
        assert!(data_mount_owned(&[inspected], &baked));
        for (key, value) in [
            ("Destination", json!("/other")),
            (
                "Options",
                json!([
                    "upperdir=/other/data_overlay",
                    "workdir=/other/data_workdir"
                ]),
            ),
        ] {
            let mut other = mount.clone();
            other[key] = value;
            assert!(!data_mount_owned(&[other], &baked));
        }
        let mut other = mount.clone();
        other["Options"][1] = json!("workdir=/other/data_workdir");
        assert!(!data_mount_owned(&[other], &baked));
        let mut duplicate = mount.clone();
        duplicate["Options"]
            .as_array_mut()
            .unwrap()
            .push(json!("upperdir=/other"));
        assert!(!data_mount_owned(&[duplicate], &baked));
        let outside = dir.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::remove_dir(baked.join("data_overlay")).unwrap();
        std::os::unix::fs::symlink(&outside, baked.join("data_overlay")).unwrap();
        assert!(!data_mount_owned(&[mount], &baked));
    }
}
