//! Optional Quest APK conversion. The runtime is versioned separately from Framely.
use super::{apk_metadata, init, review, space, Cancellation, Metadata, MUTATION};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

fn release_root() -> Result<PathBuf> {
    Ok(std::env::current_exe()?
        .parent()
        .context("Missing Framely binary directory")?
        .parent()
        .context("Missing Framely release directory")?
        .to_path_buf())
}

fn requirement_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("FRAMELY_Q2F_DESCRIPTOR") {
        return Ok(PathBuf::from(path));
    }
    Ok(release_root()?.join("Q2F_RUNTIME.json"))
}

fn tool_path() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("FRAMELY_Q2F_TOOL") {
        return Ok(PathBuf::from(path));
    }
    Ok(release_root()?.join("tools/q2f-runtime.py"))
}

pub(super) fn status(home: &Path) -> Result<Value> {
    let requirement: Value = serde_json::from_slice(&fs::read(requirement_path()?)?)?;
    let identity = requirement["id"]
        .as_str()
        .context("Invalid q2f runtime identity")?;
    ensure!(
        !identity.is_empty()
            && identity
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b))
            && !identity.contains(".."),
        "Invalid q2f runtime identity"
    );
    let cached = super::root(home).join("q2f/runtimes").join(identity);
    Ok(json!({"id":identity,"bytes":requirement["size"],
        "cached":fs::read_to_string(cached.join("VERSION")).is_ok_and(|v|v.trim()==identity)}))
}

fn run(mut command: Command, log: &Path, cancel: &Cancellation) -> Result<()> {
    let output = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(log)?;
    let errors = output.try_clone()?;
    let mut child = command
        .stdout(Stdio::from(output))
        .stderr(Stdio::from(errors))
        .spawn()?;
    loop {
        if let Err(error) = cancel.check() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            let detail = fs::read_to_string(log).unwrap_or_default();
            let tail = detail
                .chars()
                .rev()
                .take(3000)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>();
            anyhow::bail!("Quest conversion failed ({}): {}", status, tail.trim());
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn workspace(storage: &Path, runtime: &Path) -> Result<PathBuf> {
    let workspace = storage.join("q2f/workspace");
    fs::create_dir_all(workspace.join("signatures"))?;
    fs::set_permissions(
        workspace.join("signatures"),
        fs::Permissions::from_mode(0o700),
    )?;
    let libraries = workspace.join("libraries");
    if libraries.is_symlink() || libraries.exists() {
        if !libraries.is_symlink() {
            anyhow::bail!("Unexpected q2f libraries path");
        }
        fs::remove_file(&libraries)?;
    }
    let identity = runtime
        .file_name()
        .context("Missing q2f runtime identity")?;
    // Keep this relative so APK storage migration preserves the link.
    symlink(
        Path::new("../runtimes").join(identity).join("libraries"),
        &libraries,
    )?;
    fs::copy(
        runtime.join("overport-installed.json"),
        workspace.join("installed.json"),
    )?;
    Ok(workspace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_library_link_survives_apk_storage_move() {
        let temp = tempfile::tempdir().unwrap();
        let old = temp.path().join("old");
        let runtime = old.join("q2f/runtimes/test-id");
        fs::create_dir_all(runtime.join("libraries")).unwrap();
        fs::write(runtime.join("overport-installed.json"), "{}").unwrap();
        fs::write(runtime.join("libraries/probe"), "ok").unwrap();
        let work = workspace(&old, &runtime).unwrap();
        assert_eq!(
            fs::read_to_string(work.join("libraries/probe")).unwrap(),
            "ok"
        );
        let new = temp.path().join("new");
        fs::rename(old, &new).unwrap();
        assert_eq!(
            fs::read_to_string(new.join("q2f/workspace/libraries/probe")).unwrap(),
            "ok"
        );
    }
}

pub(super) fn convert(
    home: &Path,
    ticket: &str,
    cancel: &Cancellation,
    progress: impl Fn(Value) -> Result<()>,
) -> Result<Value> {
    let _guard = MUTATION.lock().unwrap();
    cancel.check()?;
    let storage = init(home)?;
    let (source, before) = review(home, ticket)?;
    let source_apk = source.join("base.apk");
    let original_bytes = fs::metadata(&source_apk)?.len();
    // The observed OVR Port peak was roughly four source APK sizes, plus the
    // original review. Leave a fixed reserve for unrelated Android writes.
    space(
        &storage,
        original_bytes
            .saturating_mul(5)
            .saturating_add(1024 * 1024 * 1024),
    )?;
    let descriptor = requirement_path()?;
    ensure!(
        descriptor.is_file(),
        "This Framely build has no q2f runtime requirement"
    );
    let requirement: Value = serde_json::from_slice(&fs::read(&descriptor)?)?;
    let version = requirement["librariesVersion"]
        .as_str()
        .context("Invalid q2f runtime version")?;
    ensure!(
        version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b)),
        "Invalid q2f runtime version"
    );
    progress(json!({"phase":"q2f-preparing","operation":"convert"}))?;
    let log = storage.join("logs").join(format!("q2f-{ticket}.log"));
    let mut prepare = Command::new("python3");
    prepare.arg(tool_path()?).arg(&descriptor).arg(&storage);
    run(prepare, &log, cancel)?;
    let runtime = PathBuf::from(
        fs::read_to_string(&log)?
            .lines()
            .last()
            .context("q2f runtime path missing")?,
    );
    ensure!(
        runtime.is_dir() && runtime.starts_with(storage.join("q2f/runtimes")),
        "Invalid q2f runtime path"
    );
    let work = workspace(&storage, &runtime)?;
    let output = source.join("converted");
    fs::create_dir_all(&output)?;
    progress(json!({"phase":"q2f-converting","operation":"convert","cancellable":true}))?;
    let mut java = Command::new(runtime.join("java/bin/java"));
    java.arg("-Xmx3g")
        .arg("-jar")
        .arg(runtime.join("overportcli.jar"))
        .arg("patch")
        .arg(format!("--input={}", source_apk.display()))
        .arg(format!("--output={}", output.display()))
        .arg("--output-name=base.apk")
        .arg(format!("--workspace={}", work.display()))
        .arg(format!("--version={version}"));
    let conversion = run(java, &log, cancel);
    let _ = fs::remove_dir_all(work.join("dec"));
    if let Err(error) = conversion {
        let _ = fs::remove_dir_all(&output);
        return Err(error);
    }
    let converted = output.join("base.apk");
    let result = (|| -> Result<Value> {
        ensure!(converted.is_file(), "Quest converter produced no APK");
        progress(json!({"phase":"q2f-verifying","operation":"convert"}))?;
        let after: Metadata = apk_metadata::read(&converted)?;
        ensure!(
            after.package == before.package && after.version_code == before.version_code,
            "Quest conversion changed APK package or version"
        );
        cancel.check()?;
        // Create a fresh review after conversion so the user has the full
        // review window even when patching takes several minutes.
        drop(_guard);
        super::inspect(home, &converted, cancel)
    })();
    let _ = fs::remove_dir_all(&output);
    let next = result?;
    let _ = super::reviews_drop(home, ticket);
    Ok(
        json!({"ticket":next["ticket"],"metadata":next["metadata"],"bytes":next["bytes"],
        "q2fVersion":requirement["id"],"originalBytes":original_bytes}),
    )
}
