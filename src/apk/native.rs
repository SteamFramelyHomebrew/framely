//! Steam invokes its Lepton compatibility tool; a private entry adapter preserves
//! existing storage/input settings without replacing any system/Steam files.
use super::*;
use std::os::unix::process::CommandExt;

const NATIVE_HOOKS: &str = r#"
# The session service has verified the installed package before starting Steam.
# Do not let native APK/depot timestamps reinstall or clear that installation.
function is_app_baked() { return 0; }
function get_app_mount_dir() { print "${FRAMELY_NATIVE_MOUNT:?}"; }
function get_app_activity() { print "${FRAMELY_NATIVE_ACTIVITY:?}"; }
function get_app_hash() { extract_app_hash; }
function get_app_last_depot_version() { extract_app_last_depot_version; }
function clear_baked_app_data() { println 'Framely: keeping installed application data'; }
# Separate native APK overlays in older shared contexts. Keep the installed
# directory (including extracted libraries/splits), rather than hiding it with
# a directory containing only the staged base APK.
if [[ -n "${FRAMELY_NATIVE_APK_DIR:-}" ]]; then
    framely_native_mounts="$(declare -f setup_mounts)"
    framely_native_lower='APP_APK_DIR="$(dirname "$(get_apk_path)")"'
    framely_native_upper='$(get_baked_app_data_dir)/app_overlay'
    [[ "$framely_native_mounts" == *"$framely_native_lower"* && "$framely_native_mounts" == *"$framely_native_upper"* ]] || { echo 'Unsupported native APK mounts; keeping application data unchanged' >&2; exit 64; }
    framely_native_mounts="${framely_native_mounts//"$framely_native_lower"/'APP_APK_DIR="${FRAMELY_NATIVE_APK_DIR:?}"'}"
    framely_native_mounts="${framely_native_mounts//"$framely_native_upper"/'${FRAMELY_NATIVE_OVERLAY:?}/upper'}"
    function app_workdir() { print "${FRAMELY_NATIVE_OVERLAY:?}/work"; }
    eval "$framely_native_mounts"
    unset framely_native_mounts framely_native_lower framely_native_upper
fi
"#;

pub(super) fn script(source: &str) -> Result<String> {
    let adapted = direct_launch_script(source)?;
    let include = "source \"${SCRIPT_DIR}/liblepton/liblepton.sh\"";
    // Append native overrides immediately before the entry's case statement,
    // after the shared resource, orientation and optional input hooks.
    let marker = "case \"${COMMAND}\" in";
    ensure!(
        source.matches(marker).count() == 1 && source.contains("function teardown()"),
        "Unsupported native Lepton entry; keeping the existing installation unchanged"
    );
    ensure!(adapted.contains(include), "Missing native Lepton library");
    Ok(adapted.replacen(marker, &format!("{NATIVE_HOOKS}\n{marker}"), 1))
}

pub(super) fn runtime_name(r: &Record) -> Option<String> {
    r.steam_binding.as_ref().filter(|b| b.native)?;
    r.steam_app_id
        .filter(|id| *id >= 0x80000000)
        .map(|id| format!("steamlaunch-{id}"))
}

pub(super) fn prepare(
    home: &Path,
    id: &str,
    app_id: u32,
) -> Result<(Container, App, Value, fs::File)> {
    let _guard = MUTATION
        .try_lock()
        .map_err(|_| anyhow::anyhow!("Another APK operation is running"))?;
    let _lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(init(home)?.join("operation.lock"))?;
    ensure!(
        unsafe { libc::flock(_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "Another APK operation is running"
    );
    let mut db = load(home)?;
    let r = db.records.get(id).context("Unknown APK entry")?;
    ensure!(
        r.steam_launch
            && r.steam_binding.as_ref().is_some_and(|b| b.native)
            && r.steam_app_id == Some(app_id),
        "Steam APK binding changed; refresh the entry and retry"
    );
    // Steam's launcher closes inherited descriptors. Keep this lock in the
    // session service, which owns it until the launch and container have ended.
    fs::create_dir_all(root(home).join("steam"))?;
    let ownership = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(
            root(home)
                .join("steam")
                .join(format!("context-{}.lock", hash(&r.context))),
        )?;
    ensure!(
        unsafe { libc::flock(ownership.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
        "Another application is starting or running in this container"
    );
    let (a, mut c) = app(home, &db, id)?;
    ensure!(
        a.installed && a.state_known && a.pending.is_none(),
        "APK installation is unavailable or an operation is pending"
    );
    let component = launch_component(&a)?;
    ensure!(!running(&c.name),"Close the running container before native Steam launch; it will not be restarted automatically");
    // Also guard the original name: registration can occur while the old launch
    // is still running, and two runtime names must never share an overlay.
    let original = if c.id.starts_with("external-") {
        db.root_contexts
            .get(&c.baked)
            .cloned()
            .context("Unknown external container name")?
    } else {
        c.id.clone()
    };
    ensure!(
        !running(&original),
        "Close the running container before native Steam launch"
    );
    c.name = format!("steamlaunch-{app_id}");
    c.steam = false;
    let xml = fs::read_to_string(c.baked.join("data_overlay/system/packages.xml"))?;
    let mount = xml
        .split('<')
        .filter(|t| t.starts_with("package "))
        .find(|t| attr(t, "name") == Some(&a.metadata.package))
        .and_then(|t| attr(t, "codePath"))
        .filter(|p| p.starts_with("/data/") && !p.split('/').any(|x| x == ".."))
        .context("Cannot verify the installed APK mount; no application data was changed")?;
    let runner = runner(home)?;
    let path = root(home)
        .join("steam")
        .join(format!("native-{}.sh", hash(id)));
    fs::create_dir_all(path.parent().unwrap())?;
    steam_shortcuts::write_owned(
        &path,
        script(&fs::read_to_string(&runner)?)?.as_bytes(),
        0o700,
    )?;
    storage::prepare(&c.baked)?;
    let media = c.baked.join("data_overlay/media/0");
    let adopted = c.baked.parent().unwrap().join("external");
    let external = if fs::read_link(&media).is_ok_and(|target| target == adopted) {
        ensure!(!adopted.is_symlink(), "Unexpected external media link");
        adopted
    } else {
        c.baked.join("external")
    };
    let flat = a.show_window.unwrap_or(!a.metadata.vr);
    let mut env = json!({
        "FRAMELY_LEPTON_DIR":runner.parent().context("Missing Lepton directory")?,
        "STEAM_COMPAT_DATA_PATH":c.baked.parent().context("Missing application data")?,
        "FRAMELY_NATIVE_MOUNT":mount,
        "FRAMELY_NATIVE_ACTIVITY":component.split_once('/').context("Invalid component")?.1,
        "FRAMELY_NATIVE_LAUNCH":"true",
        "FRAMELY_EXTERNAL_MEDIA_DIR":external,
        "FRAMELY_SHADER_CACHE_DIR":c.baked.join("shadercache"),
        "FRAMELY_BACKGROUND_BOOT":"false",
        "FRAMELY_WINDOW_ORIENTATION":a.orientation.as_deref().unwrap_or("auto"),
        "APP_WANTS_FLATSCREEN":if flat {"true"}else{"false"},
        "LEPTON_NO_CLEANUP":"true", "TERM":"dumb"
    });
    let installed_dir = c
        .baked
        .join("data_overlay")
        .join(mount.trim_start_matches("/data/"));
    if installed_dir.is_dir() {
        let installed_dir = fs::canonicalize(installed_dir)?;
        ensure!(
            installed_dir.starts_with(fs::canonicalize(c.baked.join("data_overlay"))?),
            "Installed APK directory escapes its data location"
        );
        env["FRAMELY_NATIVE_APK_DIR"] = json!(installed_dir);
        env["FRAMELY_NATIVE_OVERLAY"] = json!(c
            .baked
            .join(format!("native-apk-{}", hash(&a.metadata.package))));
    }
    if db.gamepad_enabled {
        let m =
            crate::gamepad::prepare(&root(home), &c.name, &db.gamepad_source, db.gamepad_rumble)?;
        crate::gamepad::select_target(&c.name, &a.metadata.package)?;
        for (k, v) in [
            ("FRAMELY_GAMEPAD_EVENT", json!(m.event)),
            ("FRAMELY_GAMEPAD_GRAB", json!(m.grab)),
            ("FRAMELY_GAMEPAD_LAYOUT", json!(m.layout)),
            ("FRAMELY_GAMEPAD_READY", json!(m.ready)),
            ("FRAMELY_GAMEPAD_TOKEN", json!(m.token)),
        ] {
            env[k] = v;
        }
    }
    db.records.get_mut(id).unwrap().steam_app_id = Some(app_id);
    save(home, &db)?;
    Ok((
        c,
        a,
        json!({"script":path,"env":env,"transientGeneration":db.records[id].steam_transient_generation}),
        ownership,
    ))
}

pub(super) fn run(app: &str, token: &str, command: &[String]) -> Result<()> {
    ensure!(
        unsafe { libc::geteuid() } != 0,
        "Native APK launch must run as the Steam session user"
    );
    let home = crate::steam::home()?;
    let expected = runner(&home)?;
    let entry = command
        .len()
        .checked_sub(4)
        .context("Unexpected native Lepton command")?;
    ensure!(
        fs::canonicalize(&command[entry])? == fs::canonicalize(&expected)?
            && command[entry + 1] == "waitforexitandrun"
            && command[entry + 2] == "--",
        "Unexpected native Lepton command"
    );
    if entry != 0 {
        // %command% includes Steam's launch wrapper and reaper. Retain both;
        // replacing the whole command would lose native lifecycle ownership.
        ensure!(
            entry == 8
                && Path::new(&command[0])
                    .file_name()
                    .is_some_and(|s| s == "steam-launch-wrapper")
                && Path::new(&command[4])
                    .file_name()
                    .is_some_and(|s| s == "reaper")
                && command[1] == "--oom-score-adjust"
                && command[2].parse::<i32>().is_ok()
                && command[3] == "--"
                && command[5] == "SteamLaunch"
                && command[6].starts_with("AppId=")
                && command[7] == "--",
            "Unsupported Steam launch wrapper; keeping application data unchanged"
        );
        for i in [0, 4] {
            let path = fs::canonicalize(&command[i])?;
            ensure!(
                path.starts_with(fs::canonicalize(home.join(".local/share/Steam"))?),
                "Unexpected Steam launch runtime"
            );
        }
    }
    let r = load(&home)?
        .records
        .get(app)
        .cloned()
        .context("Unknown APK entry")?;
    ensure!(
        r.steam_token.as_deref() == Some(token),
        "Invalid native APK token"
    );
    let apk = steam_shortcuts::native_apk(&home, app);
    ensure!(
        fs::canonicalize(&command[entry + 3])? == fs::canonicalize(&apk)?,
        "Unexpected APK launch target"
    );
    let response = steam_bridge::native_call(
        app,
        token,
        r.steam_app_id.context("Missing Steam APK App ID")?,
    )?;
    let path = response["script"]
        .as_str()
        .context("Missing native entry")?;
    ensure!(
        Path::new(path)
            == root(&home)
                .join("steam")
                .join(format!("native-{}.sh", hash(app))),
        "Invalid native entry path"
    );
    let mut arguments = command.to_vec();
    arguments[entry] = path.to_owned();
    let mut cmd = Command::new(&arguments[0]);
    cmd.args(&arguments[1..])
        .env_remove("FRAMELY_STEAM_APP_ID")
        .env("SteamAppId", r.steam_app_id.unwrap().to_string());
    for (k, v) in response["env"]
        .as_object()
        .context("Missing native environment")?
    {
        cmd.env(k, v.as_str().context("Invalid native environment value")?);
    }
    Err(cmd.exec().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_shared_context_uses_complete_installed_directory_and_separate_overlay() {
        let source = format!(
            r#"
function print() {{ printf '%s\n' "$*"; }}
function setup_mounts() {{
    APP_APK_DIR="$(dirname "$(get_apk_path)")"
    local APP_OVERLAY_UPPERDIR="$(get_baked_app_data_dir)/app_overlay"
    print "$APP_APK_DIR|$APP_OVERLAY_UPPERDIR|$(app_workdir)"
}}
{NATIVE_HOOKS}
setup_mounts
"#
        );
        for package in ["one", "two"] {
            let out = Command::new("bash")
                .args(["-c", &source])
                .env(
                    "FRAMELY_NATIVE_APK_DIR",
                    format!("/baked/data_overlay/app/{package}"),
                )
                .env("FRAMELY_NATIVE_OVERLAY", format!("/baked/native-{package}"))
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(String::from_utf8(out.stdout).unwrap().trim(), format!("/baked/data_overlay/app/{package}|/baked/native-{package}/upper|/baked/native-{package}/work"));
        }
    }
    #[test]
    fn native_hooks_keep_storage_and_selected_activity_after_early_exit() {
        let t = tempfile::tempdir().unwrap();
        let lib = t.path().join("lepton/liblepton");
        fs::create_dir_all(&lib).unwrap();
        fs::write(
            lib.join("liblepton.sh"),
            r#"
function data_mount_path() { echo "$TEST_DATA"; }
function setup_props() { :; }
function props_file() { echo "$TEST_PROPS"; }
function setup_mounts() {
    rm -rf "$(data_mount_path)/media/0"
    mkdir -p "$(data_mount_path)/media"
    ln -s "${STEAM_COMPAT_DATA_PATH}/external" "$(data_mount_path)/media/0"
}
function setup_podman_mounts() { setup_mounts; }
function podman_mount_entry() { :; }
function print() { printf '%s\n' "$*"; }
function println() { print "$@"; }
"#,
        )
        .unwrap();
        let source = r#"#!/bin/bash
set -euo pipefail
COMMAND=waitforexitandrun
SCRIPT_DIR=$( cd -- "$( dirname -- "${BASH_SOURCE[0]}" )" &> /dev/null && pwd )
source "${SCRIPT_DIR}/liblepton/liblepton.sh"
function teardown() { clear_baked_app_data 'early exit'; }
case "${COMMAND}" in
waitforexitandrun)
    is_app_baked
    setup_podman_mounts
    get_app_mount_dir
    get_app_activity
    teardown
;;
esac
"#;
        let entry = t.path().join("entry.sh");
        fs::write(&entry, script(source).unwrap()).unwrap();
        let compat = t.path().join("context");
        let data = compat.join("baked/data_overlay");
        let download = data.join("media/0/Android/data/test/files/download.part");
        fs::create_dir_all(download.parent().unwrap()).unwrap();
        fs::write(&download, b"resume-me").unwrap();
        let save = data.join("data/test/save");
        fs::create_dir_all(save.parent().unwrap()).unwrap();
        fs::write(&save, b"saved game").unwrap();
        for _ in 0..2 {
            let out = Command::new("bash")
                .arg(&entry)
                .env("FRAMELY_LEPTON_DIR", lib.parent().unwrap())
                .env("STEAM_COMPAT_DATA_PATH", &compat)
                .env("FRAMELY_EXTERNAL_MEDIA_DIR", compat.join("baked/external"))
                .env("FRAMELY_SHADER_CACHE_DIR", compat.join("baked/shadercache"))
                .env("FRAMELY_NATIVE_LAUNCH", "true")
                .env("FRAMELY_NATIVE_MOUNT", "/data/app/test")
                .env("FRAMELY_NATIVE_ACTIVITY", "test.Selected")
                .env("TEST_DATA", &data)
                .env("TEST_PROPS", t.path().join("props"))
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(String::from_utf8_lossy(&out.stdout).contains("test.Selected"));
            assert_eq!(fs::read(&save).unwrap(), b"saved game");
            assert_eq!(fs::read(&download).unwrap(), b"resume-me");
        }
        assert!(script("unknown future native entry").is_err());
    }
}
