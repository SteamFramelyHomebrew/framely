//! Per-application GLES workaround. The library is inert unless Android's
//! debug-layer loader selects this exact package at process startup.
use super::*;

pub(super) const HOOKS: &str = include_str!("../../native/android/fbo-compat.sh");
const LAYER: &str = "libFramelyFBOCompat.so";

pub(super) fn library(required: bool) -> Result<Option<PathBuf>> {
    let exe = std::env::current_exe()?;
    let path = exe
        .parent()
        .and_then(Path::parent)
        .context("Missing Framely runtime directory")?
        .join("lib/openvr")
        .join(LAYER);
    ensure!(
        !required || path.is_file(),
        "Framebuffer compatibility runtime is missing; install a complete Framely build"
    );
    Ok(path.is_file().then_some(path))
}
pub(super) fn configure_command(cmd: &mut Command, enabled: bool, package: &str) -> Result<()> {
    // Do not inherit a launch configuration from another application/process.
    cmd.env_remove("FRAMELY_FBO_LIBRARY")
        .env_remove("FRAMELY_FBO_PACKAGE");
    if let Some(path) = library(enabled)? {
        cmd.env("FRAMELY_FBO_LIBRARY", path);
    }
    if enabled {
        ensure!(apk_metadata::valid_package(package), "Invalid APK package");
        cmd.env("FRAMELY_FBO_PACKAGE", package);
    }
    Ok(())
}
pub(super) fn configure_environment(env: &mut Value, enabled: bool, package: &str) -> Result<()> {
    // Empty values also prevent inherited Steam environment settings from
    // enabling a layer for an application whose preference is off.
    env["FRAMELY_FBO_LIBRARY"] = json!(library(enabled)?);
    if env["FRAMELY_FBO_LIBRARY"].is_null() {
        env["FRAMELY_FBO_LIBRARY"] = json!("");
    }
    ensure!(
        !enabled || apk_metadata::valid_package(package),
        "Invalid APK package"
    );
    env["FRAMELY_FBO_PACKAGE"] = json!(if enabled { package } else { "" });
    Ok(())
}
pub(super) fn mounted(c: &Container, log: &Path) -> Result<bool> {
    Ok(podman(
        &[
            "exec",
            &format!("lepton-{}", c.name),
            "sh",
            "-c",
            "if [ -r /data/local/debug/gles/libFramelyFBOCompat.so ]; then echo ready; fi",
        ],
        Some(log),
    )?
    .trim()
        == "ready")
}
pub(super) fn apply(c: &Container, a: &App, log: &Path) -> Result<()> {
    ensure!(
        apk_metadata::valid_package(&a.metadata.package),
        "Invalid APK package"
    );
    // Reusing a container is safe: restart only the selected app if its loader
    // configuration changed, and clear only our own layer when disabling it.
    let script = r#"
package=$1
enabled=$2
layer=$(settings get global gpu_debug_layers_gles)
target=$(settings get global gpu_debug_app)
if [ "$enabled" = true ]; then
    [ -r /data/local/debug/gles/libFramelyFBOCompat.so ] || exit 64
    active=$(settings get global enable_gpu_debug_layers)
    if [ "$layer" != libFramelyFBOCompat.so ] || [ "$target" != "$package" ] || [ "$active" != 1 ]; then
        am force-stop --user 0 "$package" || exit
    fi
    settings put global enable_gpu_debug_layers 1 || exit
    settings put global gpu_debug_app "$package" || exit
    settings put global gpu_debug_layers_gles libFramelyFBOCompat.so || exit
    settings put global gpu_debug_layer_app "$package" || exit
elif [ "$layer" = libFramelyFBOCompat.so ]; then
    if [ "$target" = "$package" ]; then am force-stop --user 0 "$package" || exit; fi
    settings delete global gpu_debug_layers_gles || exit
    settings delete global gpu_debug_layer_app || exit
fi
"#;
    podman(
        &[
            "exec",
            &format!("lepton-{}", c.name),
            "sh",
            "-c",
            script,
            "framely-fbo",
            &a.metadata.package,
            if a.framebuffer_compatibility {
                "true"
            } else {
                "false"
            },
        ],
        Some(log),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compiled_layer_preserves_calls_and_handles_missing_symbols() {
        let dir = tempfile::tempdir().unwrap();
        let base = Path::new(env!("CARGO_MANIFEST_DIR"));
        let library = dir.path().join("libFramelyFBOCompat.so");
        let tester = dir.path().join("test-layer");
        assert!(Command::new("gcc")
            .args([
                "-shared",
                "-fPIC",
                "-nostdlib",
                "-fno-stack-protector",
                "-fvisibility=hidden",
                "-Wl,-z,defs",
                "-Wall",
                "-Wextra",
                "-Werror"
            ])
            .arg(base.join("native/fbo_compat.c"))
            .arg("-o")
            .arg(&library)
            .status()
            .unwrap()
            .success());
        assert!(Command::new("gcc")
            .arg(base.join("tests/fbo-layer.c"))
            .args(["-ldl", "-o"])
            .arg(&tester)
            .status()
            .unwrap()
            .success());
        assert!(Command::new(tester)
            .arg(library)
            .status()
            .unwrap()
            .success());
    }
    #[test]
    fn boot_hooks_scope_layer_and_clear_stale_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("test.sh");
        fs::write(
            &script,
            format!(
                r#"set -euo pipefail
function setup_podman_mounts() {{ echo original-mounts; }}
function podman_mount_entry() {{ printf '%s|%s|%s\n' "$1" "$2" "$3"; }}
function generate_vulkan_settings_script() {{ echo original-vulkan-settings; }}
{HOOKS}
setup_podman_mounts
generate_vulkan_settings_script
"#
            ),
        )
        .unwrap();
        for enabled in [false, true] {
            let result = Command::new("bash")
                .arg(&script)
                .env("FRAMELY_FBO_LIBRARY", "/runtime/layer.so")
                .env(
                    "FRAMELY_FBO_PACKAGE",
                    if enabled { "com.example.game" } else { "" },
                )
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            let output = String::from_utf8(result.stdout).unwrap();
            assert!(output.contains("original-mounts"));
            assert!(output
                .contains("/runtime/layer.so|/data/local/debug/gles/libFramelyFBOCompat.so|ro"));
            assert!(output.contains("settings delete global gpu_debug_layers_gles"));
            assert!(output.contains("original-vulkan-settings"));
            assert_eq!(
                output.contains("settings put global gpu_debug_app com.example.game"),
                enabled
            );
            assert_eq!(
                output.contains("settings put global gpu_debug_layers_gles libFramelyFBOCompat.so"),
                enabled
            );
        }
        let result = Command::new("bash")
            .arg(&script)
            .env("FRAMELY_FBO_LIBRARY", "/runtime/layer.so")
            .env("FRAMELY_FBO_PACKAGE", "bad;command")
            .output()
            .unwrap();
        assert!(!result.status.success());
    }
}
