//! Mirrors only a pending Android runtime-permission dialog into a VR window.
//! The user's click is delivered to Android's own PermissionController, so
//! Android retains its one-time/while-in-use/denial semantics and callbacks.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{DynamicImage, ImageFormat};
use std::collections::BTreeSet;
use std::io::{Cursor, Seek, SeekFrom};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Pending {
    pub id: String,
    pub app_name: String,
    pub package: String,
    runtime: String,
    instance: String,
}

fn active_permission(runtime: &str) -> Result<bool> {
    let mut command = crate::process::tool("podman");
    command.args([
            "exec", &format!("lepton-{runtime}"), "sh", "-c",
            "dumpsys activity activities | grep -E 'topResumedActivity=|mResumedActivity:'; printf '\\nFRAMELY_PERMISSION_DONE\\n'",
        ]);
    let text = output(command, Duration::from_secs(3), None)?;
    ensure!(text.ends_with("FRAMELY_PERMISSION_DONE\n"), "Incomplete permission probe");
    Ok(text.lines().any(|line| {
        (line.contains("topResumedActivity=") || line.contains("mResumedActivity:"))
            && line.contains("com.android.permissioncontroller/")
            && line.contains("GrantPermissionsActivity")
    }))
}

pub(crate) fn pending(home: &Path) -> Result<Option<Pending>> {
    let db = load(home)?;
    let mut command = crate::process::tool("podman");
    command.args(["ps", "--format", "{{.Names}}"]);
    let running = output(command, Duration::from_secs(3), None)?;
    let running: BTreeSet<_> = running.lines().collect();
    let contexts = containers(home, &db);
    for record in db.records.values().filter(|r| !r.removed && r.metadata.vr && !r.show_window.unwrap_or(false)) {
        let Some(base) = contexts.iter().find(|c| c.id == record.context) else { continue };
        let runtime = native::runtime_name(record).filter(|name| running.contains(format!("lepton-{name}").as_str()))
            .unwrap_or_else(|| base.name.clone());
        if !running.contains(format!("lepton-{runtime}").as_str()) { continue; }
        let mut probe = crate::process::tool("podman");
        probe.args(["exec", &format!("lepton-{runtime}"), "getprop", "lepton.active_app_id"]);
        let Ok(package) = output(probe, Duration::from_secs(3), None) else { continue };
        if package.trim() != record.metadata.package { continue; }
        if !active_permission(&runtime)? { continue; }
        let mut inspect = crate::process::tool("podman");
        inspect.args(["inspect", "--format", "{{.Id}}|{{.State.StartedAt}}|{{.State.Running}}", &format!("lepton-{runtime}")]);
        let Ok(instance) = output(inspect, Duration::from_secs(3), None) else { continue };
        let instance = instance.trim().to_owned();
        if !instance.ends_with("|true") || instance.split('|').count() != 3 { continue; }
        let id = hash(&format!("{}|{}|{}", runtime, instance, package.trim()));
        return Ok(Some(Pending {
            id,
            app_name: record.metadata.name.clone(),
            package: package.trim().to_owned(),
            runtime,
            instance,
        }));
    }
    Ok(None)
}

fn current(home: &Path, id: &str) -> Result<Pending> {
    let request = pending(home)?.context("Android permission request has ended")?;
    ensure!(request.id == id, "Android permission request has changed");
    Ok(request)
}

fn screenshot(runtime: &str) -> Result<Vec<u8>> {
    let mut file = tempfile::tempfile()?;
    let mut child = crate::process::tool("podman")
        .args(["exec", &format!("lepton-{runtime}"), "screencap", "-p"])
        .stdin(Stdio::null()).stdout(Stdio::from(file.try_clone()?)).stderr(Stdio::null())
        .spawn()?;
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? { break status; }
        if start.elapsed() >= Duration::from_secs(3) {
            let _ = child.kill(); let _ = child.wait();
            bail!("Android permission capture timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    ensure!(status.success(), "Android permission capture failed");
    ensure!(file.metadata()?.len() <= 8 * 1024 * 1024, "Android permission capture too large");
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    ensure!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"), "Android permission capture is not PNG");
    Ok(bytes)
}

pub(crate) fn frame(home: &Path, id: &str) -> Result<Value> {
    let request = current(home, id)?;
    let image = image::load_from_memory_with_format(&screenshot(&request.runtime)?, ImageFormat::Png)?;
    let (width, height) = (image.width(), image.height());
    ensure!((320..=4096).contains(&width) && (240..=4096).contains(&height), "Unexpected Android display size");
    let crop_width = width * 11 / 20;
    let crop_height = height * 11 / 20;
    let left = (width - crop_width) / 2;
    let top = (height - crop_height) / 2;
    let cropped = image.crop_imm(left, top, crop_width, crop_height);
    let mut png = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(cropped.into_rgba8()).write_to(&mut png, ImageFormat::Png)?;
    ensure!(current(home, id)?.instance == request.instance, "Android permission request has changed");
    Ok(json!({"id":id,"appName":request.app_name,"package":request.package,
        "image":format!("data:image/png;base64,{}",STANDARD.encode(png.into_inner())),
        "left":left,"top":top,"width":crop_width,"height":crop_height}))
}

const INPUT: &str = r#"
set -eu
for entry in $(cat /proc/$(pidof system_server)/environ | tr '\0' '\n' | grep -E '^(ANDROID_(ROOT|DATA|ART_ROOT|I18N_ROOT|TZDATA_ROOT)|BOOTCLASSPATH)='); do
    export "$entry"
done
exec input touchscreen tap "$1" "$2"
"#;

pub(crate) fn click(home: &Path, id: &str, x: u32, y: u32) -> Result<Value> {
    let request = current(home, id)?;
    ensure!(x < 4096 && y < 4096, "Invalid Android input position");
    let image = image::load_from_memory_with_format(&screenshot(&request.runtime)?, ImageFormat::Png)?;
    ensure!(x < image.width() && y < image.height(), "Android input position outside display");
    ensure!(current(home, id)?.instance == request.instance, "Android permission request has changed");
    let mut command = crate::process::tool("podman");
    command.args([
        "exec", &format!("lepton-{}", request.runtime), "sh", "-c", INPUT,
        "framely-permission", &x.to_string(), &y.to_string(),
    ]);
    output(command, Duration::from_secs(3), None)?;
    Ok(json!(true))
}
