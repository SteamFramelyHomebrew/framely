//! Check the APK against the image before creating a context, and against the
//! running Android before submitting it to PackageManager.
use super::*;

fn property<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines()
        .filter_map(|line| line.trim().split_once('='))
        .find_map(|(name, value)| (name == key).then_some(value.trim()))
}
fn check(abis: &[String], supported: &str) -> Result<()> {
    if abis.is_empty() {
        return Ok(());
    }
    let supported: Vec<_> = supported
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    ensure!(
        !supported.is_empty(),
        "Android did not report supported APK architectures"
    );
    ensure!(abis.iter().any(|abi| supported.contains(&abi.as_str())),
        "APK architectures ({}) are not supported by this Android runtime ({}). Use an APK with a supported architecture; 32-bit ARM requires a separate compatibility runtime.",
        abis.join(", "), supported.join(", "));
    Ok(())
}
pub(super) fn image(home: &Path, metadata: &Metadata) -> Result<()> {
    if metadata.abis.is_empty() {
        return Ok(());
    }
    // Lepton's image is unpacked beside the launcher. Do not guess capabilities
    // if an alternate image has no build.prop; the live check remains mandatory.
    let Ok(launcher) = runner(home) else {
        return Ok(());
    };
    let path = launcher
        .parent()
        .context("Invalid Lepton location")?
        .join("images/rootfs/system/build.prop");
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    if let Some(supported) = property(&text, "ro.product.cpu.abilist") {
        check(&metadata.abis, supported)?;
    }
    Ok(())
}
pub(super) fn live(c: &Container, metadata: &Metadata, log: &Path) -> Result<()> {
    if metadata.abis.is_empty() {
        return Ok(());
    }
    let supported = podman(
        &[
            "exec",
            &format!("lepton-{}", c.name),
            "getprop",
            "ro.product.cpu.abilist",
        ],
        Some(log),
    )?;
    check(&metadata.abis, supported.trim())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_abis_must_intersect_the_runtime_and_java_only_apps_work() {
        let arm32 = vec!["armeabi-v7a".into()];
        assert!(check(&arm32, "arm64-v8a").is_err());
        assert!(check(&arm32, "arm64-v8a,armeabi-v7a").is_ok());
        assert!(check(&["arm64-v8a".into(), "armeabi-v7a".into()], "arm64-v8a").is_ok());
        assert!(check(&arm32, "").is_err());
        assert!(check(&[], "").is_ok());
    }
}
