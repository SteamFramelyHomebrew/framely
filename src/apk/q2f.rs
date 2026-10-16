//! Optional Quest APK conversion. The runtime is versioned separately from Framely.
use super::{apk_metadata, init, review, space, Cancellation, Metadata, MUTATION};
use crate::model::ProxySettings;
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

fn verify_framebridge(
    apk: &Path,
    runtime: &Path,
    libraries_version: &str,
    abis: &[String],
) -> Result<()> {
    let mut archive = zip::ZipArchive::new(fs::File::open(apk)?)?;
    let mut found = false;
    for abi in abis
        .iter()
        .filter(|abi| abi.as_str() == "arm64-v8a" || abi.as_str() == "armeabi-v7a")
    {
        found = true;
        let library = runtime
            .join("libraries")
            .join(libraries_version)
            .join("lib")
            .join(abi);
        for (apk_name, source_name) in [
            ("libopenxr_loader_generic.so", "libopenxr_loader_generic.so"),
            (
                "libopenxr_loader_original.so",
                "libopenxr_loader_original.so",
            ),
            ("libframe_settings.so", "libframe_settings.so"),
        ] {
            let expected = fs::read(library.join(source_name))?;
            let mut entry = archive.by_name(&format!("lib/{abi}/{apk_name}"))?;
            ensure!(
                entry.size() == expected.len() as u64,
                "Quest adapter file size mismatch: {abi}/{apk_name}"
            );
            let mut actual = Vec::with_capacity(expected.len());
            entry.read_to_end(&mut actual)?;
            ensure!(
                actual == expected,
                "Quest adapter file mismatch: {abi}/{apk_name}"
            );
        }
    }
    ensure!(found, "Quest APK has no supported FrameBridge architecture");
    Ok(())
}

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
    use std::io::Write;

    #[test]
    fn converted_apk_requires_the_exact_wrapped_loader_and_settings() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = temp.path().join("runtime");
        let lib = runtime.join("libraries/test/lib/arm64-v8a");
        fs::create_dir_all(&lib).unwrap();
        let apk = temp.path().join("converted.apk");
        let mut writer = zip::ZipWriter::new(fs::File::create(&apk).unwrap());
        for (name, data) in [
            ("libopenxr_loader_generic.so", b"framebridge".as_slice()),
            ("libopenxr_loader_original.so", b"overport".as_slice()),
            ("libframe_settings.so", b"controller_fix=1\n".as_slice()),
        ] {
            fs::write(lib.join(name), data).unwrap();
            writer
                .start_file(
                    format!("lib/arm64-v8a/{name}"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
        let abis = vec!["arm64-v8a".to_string()];
        verify_framebridge(&apk, &runtime, "test", &abis).unwrap();
        fs::write(lib.join("libframe_settings.so"), b"controller_fix=0\n").unwrap();
        assert!(verify_framebridge(&apk, &runtime, "test", &abis).is_err());
    }

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

    #[test]
    fn runtime_upgrade_keeps_per_app_signing_keys() {
        let temp = tempfile::tempdir().unwrap();
        let storage = temp.path();
        for id in ["old", "framebridge"] {
            let runtime = storage.join("q2f/runtimes").join(id);
            fs::create_dir_all(runtime.join("libraries")).unwrap();
            fs::write(runtime.join("overport-installed.json"), "{}").unwrap();
            fs::write(runtime.join("libraries/probe"), id).unwrap();
            workspace(storage, &runtime).unwrap();
            if id == "old" {
                fs::write(
                    storage.join("q2f/workspace/signatures/game.keystore"),
                    "original key",
                )
                .unwrap();
            }
        }
        assert_eq!(
            fs::read_to_string(storage.join("q2f/workspace/signatures/game.keystore")).unwrap(),
            "original key"
        );
        assert_eq!(
            fs::read_to_string(storage.join("q2f/workspace/libraries/probe")).unwrap(),
            "framebridge"
        );
    }
}

pub(super) fn convert(
    home: &Path,
    ticket: &str,
    cancel: &Cancellation,
    progress: impl Fn(Value) -> Result<()>,
    proxy: &ProxySettings,
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
    prepare.env("FRAMELY_GITHUB_AUTO_FALLBACK", if proxy.github_auto_fallback {"1"} else {"0"});
    prepare.env("FRAMELY_GITHUB_PROXY", if proxy.github_enabled {&proxy.github} else {""});
    prepare.env("FRAMELY_HTTP_PROXY", if proxy.http_enabled {&proxy.http} else {""});
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
        let mut verifier = Command::new(runtime.join("java/bin/java"));
        verifier
            .arg("-cp")
            .arg(format!(
                "{}:{}",
                runtime.join("overportcli.jar").display(),
                runtime.display()
            ))
            .arg("FramelyQ2FVerify")
            .arg(&converted);
        run(verifier, &log, cancel)?;
        let after: Metadata = apk_metadata::read(&converted)?;
        ensure!(
            after.package == before.package && after.version_code == before.version_code,
            "Quest conversion changed APK package or version"
        );
        verify_framebridge(&converted, &runtime, version, &after.abis)?;
        cancel.check()?;
        // Create a fresh review after conversion so the user has the full
        // review window even when patching takes several minutes.
        drop(_guard);
        let inspected = super::inspect(home, &converted, cancel)?;
        let reviewed = storage.join("reviews").join(
            inspected["ticket"]
                .as_str()
                .context("Converted review has no ticket")?,
        );
        fs::write(
            reviewed.join("q2f-version"),
            requirement["id"].as_str().context("Invalid q2f identity")?,
        )?;
        Ok(inspected)
    })();
    let _ = fs::remove_dir_all(&output);
    let next = result?;
    Ok(
        json!({"ticket":next["ticket"],"metadata":next["metadata"],"bytes":next["bytes"],
        "q2fVersion":requirement["id"],"originalBytes":original_bytes}),
    )
}
