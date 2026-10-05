//! User-session applications: XDG desktop entries and installed Lepton packages.
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    pub id: String,
    pub name: String,
    pub kind: &'static str,
    pub icon: Option<String>,
    #[serde(skip)]
    target: Target,
}
#[derive(Clone, Debug)]
enum Target {
    Desktop(PathBuf),
    Lepton {
        runner: PathBuf,
        context: String,
        package: String,
    },
}
fn entries(input: &str) -> BTreeMap<String, String> {
    let mut active = false;
    let mut out = BTreeMap::new();
    for line in input.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            active = line == "[Desktop Entry]";
            continue;
        }
        if active && !line.starts_with('#') {
            if let Some((k, v)) = line.split_once('=') {
                out.insert(k.trim().into(), v.trim().into());
            }
        }
    }
    out
}
fn data_home(home: &Path) -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"))
}
fn data_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![data_home(home)];
    dirs.extend(
        std::env::var("XDG_DATA_DIRS")
            .unwrap_or_else(|_| "/usr/local/share:/usr/share".into())
            .split(':')
            .map(PathBuf::from)
            .filter(|p| p.is_absolute()),
    );
    for p in [
        home.join(".local/share/flatpak/exports/share"),
        PathBuf::from("/var/lib/flatpak/exports/share"),
    ] {
        if !dirs.contains(&p) {
            dirs.push(p);
        }
    }
    dirs
}
fn icon_file(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() > 1024 * 1024 {
        return None;
    }
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if path.extension().is_some_and(|e| e == "svg")
        && std::str::from_utf8(&bytes).ok()?.contains("<svg")
    {
        "image/svg+xml"
    } else {
        return None;
    };
    use base64::Engine;
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}
fn icon(home: &Path, name: &str, dirs: &[PathBuf]) -> Option<String> {
    let path = Path::new(name);
    if path.is_absolute() {
        return icon_file(path);
    }
    if name.contains('/') || name.is_empty() {
        return None;
    }
    for base in std::iter::once(home.join(".icons")).chain(dirs.iter().map(|d| d.join("icons"))) {
        for theme in ["hicolor", "breeze", "breeze-dark", "Adwaita"] {
            for size in ["256x256", "128x128", "64x64", "48x48", "scalable", "32x32"] {
                for ext in ["png", "svg"] {
                    if let Some(icon) = icon_file(
                        &base
                            .join(theme)
                            .join(size)
                            .join("apps")
                            .join(format!("{name}.{ext}")),
                    ) {
                        return Some(icon);
                    }
                }
            }
        }
    }
    dirs.iter().find_map(|d| {
        ["png", "svg", "xpm"]
            .iter()
            .find_map(|ext| icon_file(&d.join("pixmaps").join(format!("{name}.{ext}"))))
    })
}
fn files(base: &Path, dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
    if depth > 8 {
        return;
    }
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                files(base, &p, depth + 1, out);
            } else if p.extension().is_some_and(|e| e == "desktop") {
                if let Ok(relative) = p.strip_prefix(base) {
                    out.push((relative.to_string_lossy().replace('/', "-"), p));
                }
            }
        }
    }
}
fn desktop_apps(home: &Path, dirs: &[PathBuf], steam: &BTreeSet<u32>) -> Vec<App> {
    let mut seen = BTreeSet::new();
    let mut apps = Vec::new();
    let desktops = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    for dir in dirs {
        let base = dir.join("applications");
        let mut candidates = Vec::new();
        files(&base, &base, 0, &mut candidates);
        candidates.sort();
        for (id, path) in candidates {
            if !seen.insert(id.clone()) {
                continue;
            }
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            if content.len() > 128 * 1024 {
                continue;
            }
            let v = entries(&content);
            let get = |k: &str| v.get(k).map(String::as_str).unwrap_or("");
            if get("Type") != "Application"
                || get("Hidden") == "true"
                || get("NoDisplay") == "true"
                || get("Exec").is_empty()
            {
                continue;
            }
            let matches = |list: &str| {
                list.split(';')
                    .any(|item| !item.is_empty() && desktops.split(':').any(|d| d == item))
            };
            if !get("OnlyShowIn").is_empty() && !matches(get("OnlyShowIn"))
                || matches(get("NotShowIn"))
            {
                continue;
            }
            if !get("TryExec").is_empty() {
                let t = Path::new(get("TryExec"));
                let exists = if t.is_absolute() {
                    t.is_file()
                } else {
                    std::env::var_os("PATH").is_some_and(|paths| {
                        std::env::split_paths(&paths).any(|d| d.join(t).is_file())
                    })
                };
                if !exists {
                    continue;
                }
            }
            // Steam already owns its game shortcuts and helper runtimes.
            if get("Exec").split_whitespace().any(|word| {
                word.trim_matches('"')
                    .strip_prefix("steam://rungameid/")
                    .and_then(|s| s.parse::<u32>().ok())
                    .is_some_and(|id| steam.contains(&id))
            }) || id == "valve-steamvr.desktop"
                || id == "Lepton Development.desktop"
            {
                continue;
            }
            let locale = std::env::var("LC_MESSAGES")
                .or_else(|_| std::env::var("LANG"))
                .unwrap_or_default();
            let locale = locale.split('.').next().unwrap_or("");
            let language = locale.split('_').next().unwrap_or("");
            let name = v
                .get(&format!("Name[{locale}]"))
                .or_else(|| v.get(&format!("Name[{language}]")))
                .or_else(|| v.get("Name"))
                .filter(|n| !n.is_empty());
            let Some(name) = name else {
                continue;
            };
            apps.push(App {
                id,
                name: name.clone(),
                kind: "desktop",
                icon: icon(home, get("Icon"), dirs),
                target: Target::Desktop(path),
            });
        }
    }
    apps
}
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let rest = tag.split_once(&format!("{name}=\""))?.1;
    Some(rest.split_once('"')?.0)
}
fn safe_component(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn lepton_apps(home: &Path) -> Vec<App> {
    let runner = [
        home.join(".local/share/Steam/steamapps/common/Lepton/lepton"),
        home.join(".steam/steam/steamapps/common/Lepton/lepton"),
    ]
    .into_iter()
    .find(|p| p.is_file());
    let Some(runner) = runner else {
        return Vec::new();
    };
    let mut apps = Vec::new();
    let root = data_home(home).join("lepton/contexts");
    let Ok(contexts) = fs::read_dir(root) else {
        return apps;
    };
    for context in contexts.flatten() {
        let name = context.file_name().to_string_lossy().into_owned();
        if !safe_component(&name) || name.starts_with("steamlaunch-") {
            continue;
        }
        let data = context.path().join("baked/data_overlay");
        let Ok(xml) = fs::read_to_string(data.join("system/packages.xml")) else {
            continue;
        };
        if xml.len() > 8 * 1024 * 1024 {
            continue;
        }
        for tag in xml.split('<').filter(|t| t.starts_with("package ")) {
            let Some(package) = attr(tag, "name").filter(|s| safe_component(s)) else {
                continue;
            };
            let Some(code) = attr(tag, "codePath").and_then(|s| s.strip_prefix("/data/")) else {
                continue;
            };
            // Android system packages are not launcher apps. A removed package's stale
            // metadata is ignored; validate that its installed APK is still present.
            let path = data.join(code);
            if code.split('/').any(|s| s == "..") || !path.join("base.apk").is_file() {
                continue;
            }
            let icon = icon_file(
                &data
                    .join("system_ce/0/shortcut_service/bitmaps")
                    .join(format!("{package}.png")),
            );
            apps.push(App {
                id: format!("{name}/{package}"),
                name: package.rsplit('.').next().unwrap_or(package).into(),
                kind: "lepton",
                icon,
                target: Target::Lepton {
                    runner: runner.clone(),
                    context: name.clone(),
                    package: package.into(),
                },
            });
        }
    }
    apps
}
pub fn discover(home: &Path) -> Vec<App> {
    let ids = crate::steam::discover(home)
        .into_iter()
        .map(|a| a.id)
        .collect();
    let mut apps = desktop_apps(home, &data_dirs(home), &ids);
    apps.extend(lepton_apps(home));
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    apps
}
pub fn launch(home: &Path, id: &str) -> Result<()> {
    let app = discover(home)
        .into_iter()
        .find(|a| format!("{}:{}", a.kind, a.id) == id)
        .context("Application is no longer installed")?;
    launch_target(app.target)
}
fn launch_target(target: Target) -> Result<()> {
    match target {
        Target::Desktop(path) => {
            crate::process::launch_handoff(
                crate::process::tool("gio").arg("launch").arg(path),
                std::time::Duration::from_secs(10),
            )
            .map_err(|error| anyhow::anyhow!("Desktop launch failed: {error:#}"))?;
        }
        Target::Lepton {
            runner,
            context,
            package,
        } => {
            // Use the same session-owned context and Android launcher intent; never
            // reinstall the APK or accept an arbitrary shell command from the UI.
            let running = crate::process::command_output_timeout(
                crate::process::tool("podman").args([
                    "inspect",
                    "--format",
                    "{{.State.Running}}",
                    &format!("lepton-{context}"),
                ]),
                std::time::Duration::from_secs(5),
                true,
            )
            .context("Cannot inspect Lepton context")?;
            let running = running.status.success()
                && String::from_utf8_lossy(&running.stdout).trim() == "true";
            let child = if running {
                None
            } else {
                Some(
                    std::process::Command::new(&runner)
                        .args(["start", &context])
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .context("Cannot start Lepton context")?,
                )
            };
            if let Some(mut child) = child {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            let start = std::time::Instant::now();
            let mut ready = false;
            while start.elapsed() < std::time::Duration::from_secs(35) {
                let output = crate::process::command_output_timeout(
                    crate::process::tool("podman").args([
                        "exec",
                        &format!("lepton-{context}"),
                        "monkey",
                        "-p",
                        &package,
                        "-c",
                        "android.intent.category.LAUNCHER",
                        "1",
                    ]),
                    std::time::Duration::from_secs(35)
                        .saturating_sub(start.elapsed())
                        .min(std::time::Duration::from_secs(5)),
                    false,
                )
                .context("Lepton launch handoff failed")?;
                if output.status.success() {
                    ready = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            ensure!(
                ready,
                "Lepton application did not become ready; retry after the context starts"
            );
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn xdg_priority_hidden_overrides_and_steam_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("first");
        let b = dir.path().join("second");
        for p in [&a, &b] {
            fs::create_dir_all(p.join("applications")).unwrap();
        }
        fs::write(
            a.join("applications/hidden.desktop"),
            "[Desktop Entry]\nHidden=true",
        )
        .unwrap();
        fs::write(
            b.join("applications/hidden.desktop"),
            "[Desktop Entry]\nType=Application\nName=Hidden\nExec=test",
        )
        .unwrap();
        fs::write(
            a.join("applications/game.desktop"),
            "[Desktop Entry]\nType=Application\nName=Game\nExec=steam steam://rungameid/42",
        )
        .unwrap();
        fs::write(a.join("applications/terminal.desktop"),"[Other]\nName=Wrong\n[Desktop Entry]\nType=Application\nName=Terminal\nExec=terminal %U\nTerminal=true").unwrap();
        let apps = desktop_apps(dir.path(), &[a, b], &BTreeSet::from([42]));
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].id, "terminal.desktop");
    }
    #[test]
    fn lepton_only_installed_third_party_packages() {
        let dir = tempfile::tempdir().unwrap();
        let runner = dir
            .path()
            .join(".local/share/Steam/steamapps/common/Lepton");
        fs::create_dir_all(&runner).unwrap();
        fs::write(runner.join("lepton"), "").unwrap();
        let data = dir
            .path()
            .join(".local/share/lepton/contexts/test/baked/data_overlay");
        fs::create_dir_all(data.join("system")).unwrap();
        fs::create_dir_all(data.join("app/example")).unwrap();
        fs::write(data.join("app/example/base.apk"), "apk").unwrap();
        fs::write(data.join("system/packages.xml"),"<packages><package name=\"com.example.app\" codePath=\"/data/app/example\"/><package name=\"com.android.system\" codePath=\"/system/app/system\"/><package name=\"com.removed.app\" codePath=\"/data/app/missing\"/></packages>").unwrap();
        let apps = lepton_apps(dir.path());
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].id, "test/com.example.app");
    }
    #[test]
    fn launch_handoffs_use_literal_arguments_and_reuse_live_lepton_context() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let tools = dir.path().join("tools");
        fs::create_dir(&tools).unwrap();
        for name in ["gio", "podman"] {
            fs::write(tools.join(name),format!("#!/bin/sh\nif [ \"$1\" = inspect ]; then echo true; exit 0; fi\nprintf '%s\\n' \"$@\" > '{}'/{}-args\n",dir.path().display(),name)).unwrap();
            fs::set_permissions(tools.join(name), fs::Permissions::from_mode(0o755)).unwrap();
        }
        crate::process::TEST_TOOLS.with(|p| *p.borrow_mut() = Some(tools));
        let path = dir.path().join("app; literal.desktop");
        launch_target(Target::Desktop(path.clone())).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("gio-args")).unwrap(),
            format!("launch\n{}\n", path.display())
        );
        launch_target(Target::Lepton {
            runner: dir.path().join("must-not-start"),
            context: "existing".into(),
            package: "com.example.app".into(),
        })
        .unwrap();
        assert_eq!(fs::read_to_string(dir.path().join("podman-args")).unwrap(),"exec\nlepton-existing\nmonkey\n-p\ncom.example.app\n-c\nandroid.intent.category.LAUNCHER\n1\n");
        crate::process::TEST_TOOLS.with(|p| *p.borrow_mut() = None);
    }
    #[test]
    fn stalled_lepton_inspect_and_launch_are_bounded() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let podman = dir.path().join("podman");
        for inspect in [true, false] {
            fs::write(&podman, if inspect { "#!/bin/sh\nexec sleep 30\n" } else { "#!/bin/sh\nif [ \"$1\" = inspect ]; then echo true; exit 0; fi\nexec sleep 30\n" }).unwrap();
            fs::set_permissions(&podman, fs::Permissions::from_mode(0o755)).unwrap();
            crate::process::TEST_TOOLS.with(|p| *p.borrow_mut() = Some(dir.path().into()));
            let started = std::time::Instant::now();
            let result = launch_target(Target::Lepton {
                runner: dir.path().join("must-not-start"),
                context: "existing".into(),
                package: "com.example.app".into(),
            });
            crate::process::TEST_TOOLS.with(|p| *p.borrow_mut() = None);
            assert!(format!("{:#}", result.unwrap_err()).contains("timed out"));
            assert!(started.elapsed() < std::time::Duration::from_secs(7));
        }
    }
    #[test]
    fn unknown_id_never_launches() {
        let dir = tempfile::tempdir().unwrap();
        assert!(launch(dir.path(), "desktop:../../arbitrary").is_err());
    }
}
