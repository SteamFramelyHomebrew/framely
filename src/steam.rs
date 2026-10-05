//! Local Steam library discovery. No account API or network access is needed.
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
    pub id: u32,
    pub name: String,
    pub kind: &'static str,
    pub icon: Option<String>,
    pub icon_fit: &'static str,
}
#[derive(Debug)]
enum Value {
    Text(String),
    Object(BTreeMap<String, Value>),
}
fn tokens(input: &str) -> Result<Vec<String>> {
    let mut chars = input.chars().peekable();
    let mut out = Vec::new();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '{' | '}' => out.push(c.to_string()),
            '"' => {
                let mut s = String::new();
                let mut closed = false;
                while let Some(c) = chars.next() {
                    if c == '"' {
                        closed = true;
                        break;
                    }
                    if c == '\\' {
                        let e = chars.next().context("Truncated escape")?;
                        s.push(match e {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        });
                    } else {
                        s.push(c);
                    }
                }
                ensure!(closed, "Unclosed VDF string");
                out.push(s);
            }
            _ => {
                let mut s = c.to_string();
                while chars
                    .peek()
                    .is_some_and(|c| !c.is_whitespace() && *c != '{' && *c != '}')
                {
                    s.push(chars.next().unwrap());
                }
                out.push(s);
            }
        }
    }
    Ok(out)
}
fn object(
    tokens: &[String],
    i: &mut usize,
    depth: usize,
    nested: bool,
) -> Result<BTreeMap<String, Value>> {
    ensure!(depth < 32, "VDF too deep");
    let mut out = BTreeMap::new();
    while *i < tokens.len() {
        if tokens[*i] == "}" {
            ensure!(nested, "Unexpected brace");
            *i += 1;
            return Ok(out);
        }
        let key = tokens[*i].clone();
        *i += 1;
        let v = tokens.get(*i).context("Missing VDF value")?;
        *i += 1;
        let value = if v == "{" {
            Value::Object(object(tokens, i, depth + 1, true)?)
        } else {
            ensure!(v != "}", "Missing VDF value");
            Value::Text(v.clone())
        };
        out.insert(key, value);
    }
    ensure!(!nested, "Unclosed VDF object");
    Ok(out)
}
fn read(path: &Path) -> Result<BTreeMap<String, Value>> {
    let bytes = fs::read(path)?;
    ensure!(bytes.len() <= 8 * 1024 * 1024, "VDF too large");
    let t = tokens(std::str::from_utf8(&bytes)?)?;
    object(&t, &mut 0, 0, false)
}
fn text<'a>(v: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a str> {
    if let Some(Value::Text(s)) = v.get(key) {
        Some(s)
    } else {
        None
    }
}
fn root<'a>(v: &'a BTreeMap<String, Value>, key: &str) -> Option<&'a BTreeMap<String, Value>> {
    if let Some(Value::Object(v)) = v.get(key) {
        Some(v)
    } else {
        None
    }
}
pub(crate) fn library_roots(home: &Path) -> BTreeSet<PathBuf> {
    let steam = home.join(".local/share/Steam");
    let steam = if steam.exists() {
        steam
    } else {
        home.join(".steam/steam")
    };
    let mut libraries = BTreeSet::from([steam.clone()]);
    if let Ok(v) = read(&steam.join("steamapps/libraryfolders.vdf")) {
        if let Some(v) = root(&v, "libraryfolders") {
            for (key, value) in v {
                if key.parse::<u32>().is_err() {
                    continue;
                }
                let path = match value {
                    Value::Text(s) => Some(s.as_str()),
                    Value::Object(v) => text(v, "path"),
                };
                if let Some(p) = path {
                    let p = PathBuf::from(p);
                    if p.is_absolute() {
                        libraries.insert(p);
                    }
                }
            }
        }
    }
    libraries
}
pub fn discover(home: &Path) -> Vec<App> {
    let steam = home.join(".local/share/Steam");
    let steam = if steam.exists() {
        steam
    } else {
        home.join(".steam/steam")
    };
    let libraries = library_roots(home);
    let mut apps = BTreeMap::new();
    for library in libraries {
        let Ok(entries) = fs::read_dir(library.join("steamapps")) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            let Some(file) = p.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if !file.starts_with("appmanifest_") || !file.ends_with(".acf") {
                continue;
            }
            let Ok(v) = read(&p) else { continue };
            let Some(v) = root(&v, "AppState") else {
                continue;
            };
            let Some(id) = text(v, "appid")
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|v| *v > 0)
            else {
                continue;
            };
            let Some(name) = text(v, "name").filter(|n| !n.is_empty()) else {
                continue;
            };
            let flags = text(v, "StateFlags")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            if flags & 4 == 0 {
                continue;
            }
            let lower = name.to_lowercase();
            if [
                "steamvr",
                "lepton",
                "lepton development",
                "steam linux runtime",
                "proton",
                "steamworks common redistributables",
            ]
            .iter()
            .any(|s| lower == *s || lower.starts_with(&format!("{s} ")))
            {
                continue;
            }
            let dir = steam.join("appcache/librarycache").join(id.to_string());
            let mut candidates: Vec<_> = fs::read_dir(dir)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .collect();
            // Newer Steam clients keep named artwork one level below a content hash.
            let nested: Vec<_> = candidates
                .iter()
                .filter(|p| p.is_dir())
                .take(16)
                .cloned()
                .collect();
            for path in nested {
                candidates.extend(
                    fs::read_dir(path)
                        .ok()
                        .into_iter()
                        .flatten()
                        .flatten()
                        .take(32)
                        .map(|e| e.path()),
                );
            }
            for suffix in ["icon.jpg", "icon.png", "library_600x900.jpg", "header.jpg"] {
                candidates.push(
                    steam
                        .join("appcache/librarycache")
                        .join(format!("{id}_{suffix}")),
                );
            }
            let artwork_fit = |p: &Path| {
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                if name.contains("600x900") || name == "library_capsule.jpg" {
                    (0, "cover")
                } else if name.contains("header") {
                    (1, "cover")
                } else if name == "logo.png" {
                    (2, "contain")
                } else {
                    (3, "contain")
                }
            };
            candidates.sort_by_key(|p| (artwork_fit(p).0, p.clone()));
            let icon = candidates.into_iter().take(32).find_map(|p| {
                if fs::metadata(&p).ok()?.len() > 1024 * 1024 {
                    return None;
                }
                let b = fs::read(&p).ok()?;
                let mime = if b.starts_with(b"\x89PNG\r\n\x1a\n") {
                    "image/png"
                } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
                    "image/jpeg"
                } else {
                    return None;
                };
                use base64::Engine;
                Some((
                    format!(
                        "data:{mime};base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(b)
                    ),
                    artwork_fit(&p).1,
                ))
            });
            apps.insert(
                id,
                App {
                    id,
                    name: name.into(),
                    kind: if text(v, "installdir").is_some_and(|dir| {
                        fs::read_dir(library.join("steamapps/common").join(dir)).is_ok_and(
                            |entries| {
                                entries
                                    .flatten()
                                    .any(|e| e.path().extension().is_some_and(|ext| ext == "apk"))
                            },
                        )
                    }) {
                        "lepton"
                    } else {
                        "steam"
                    },
                    icon_fit: icon.as_ref().map(|v| v.1).unwrap_or("contain"),
                    icon: icon.map(|v| v.0),
                },
            );
        }
    }
    apps.into_values().collect()
}
pub fn home() -> Result<PathBuf> {
    ensure!(
        unsafe { libc::geteuid() } != 0,
        "Steam launcher must run in the user session"
    );
    Ok(crate::process::user_home(unsafe { libc::geteuid() })?.into())
}
pub fn launch(id: u32) -> Result<()> {
    launch_installed(&home()?, id)
}
fn launch_installed(home: &Path, id: u32) -> Result<()> {
    let apps = discover(home);
    ensure!(
        apps.iter().any(|a| a.id == id),
        "Steam app is no longer installed"
    );
    let output = crate::process::tool("steam")
        .arg(format!("steam://rungameid/{id}"))
        .stdin(Stdio::null())
        .output()
        .context("Cannot contact Steam")?;
    ensure!(
        output.status.success(),
        "Steam launch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_escaped_and_invalid() {
        let t = tokens("\"libraryfolders\" { \"0\" { \"path\" \"/a\\\\b\" } }").unwrap();
        let v = object(&t, &mut 0, 0, false).unwrap();
        assert!(root(&v, "libraryfolders").is_some());
        assert!(tokens("\"oops").is_err());
        assert!(object(&tokens("a { b c").unwrap(), &mut 0, 0, false).is_err());
    }
    #[test]
    fn artwork_prefers_nested_cover_then_logo() {
        let dir = tempfile::tempdir().unwrap();
        let steam = dir.path().join(".local/share/Steam");
        fs::create_dir_all(steam.join("steamapps")).unwrap();
        fs::write(
            steam.join("steamapps/appmanifest_1.acf"),
            "AppState { appid 1 name Game StateFlags 4 }",
        )
        .unwrap();
        let cache = steam.join("appcache/librarycache/1");
        fs::create_dir_all(cache.join("content-hash")).unwrap();
        let png = include_bytes!("../assets/branding/icons/32.png");
        fs::write(cache.join("logo.png"), png).unwrap();
        let cover = cache.join("content-hash/library_capsule.jpg");
        fs::write(&cover, png).unwrap();
        let app = discover(dir.path()).remove(0);
        assert_eq!(app.icon_fit, "cover");
        assert!(app.icon.unwrap().starts_with("data:image/png;base64,"));
        fs::write(&cover, b"corrupt").unwrap();
        assert_eq!(discover(dir.path()).remove(0).icon_fit, "contain");
    }
    #[test]
    fn multiple_libraries_and_tools() {
        let dir = tempfile::tempdir().unwrap();
        let steam = dir.path().join(".local/share/Steam/steamapps");
        fs::create_dir_all(&steam).unwrap();
        fs::write(
            steam.join("appmanifest_1.acf"),
            "AppState { appid 1 name Game StateFlags 4 }",
        )
        .unwrap();
        fs::write(
            steam.join("appmanifest_2.acf"),
            "AppState { appid 2 name Proton StateFlags 4 }",
        )
        .unwrap();
        fs::write(steam.join("appmanifest_3.acf"), "broken {").unwrap();
        let second = dir.path().join("second-library");
        fs::create_dir_all(second.join("steamapps")).unwrap();
        fs::write(
            second.join("steamapps/appmanifest_4.acf"),
            "AppState { appid 4 name AnotherGame StateFlags 4 }",
        )
        .unwrap();
        fs::write(
            second.join("steamapps/appmanifest_1.acf"),
            "AppState { appid 1 name Duplicate StateFlags 4 }",
        )
        .unwrap();
        fs::write(steam.join("libraryfolders.vdf"), format!("libraryfolders {{ 0 {{ path \"{}\" }} 1 {{ path \"{}\" }} 2 {{ path \"/missing-library\" }} }}", steam.parent().unwrap().display(), second.display())).unwrap();
        assert_eq!(
            discover(dir.path())
                .iter()
                .map(|a| a.id)
                .collect::<Vec<_>>(),
            vec![1, 4]
        );
    }
}
#[cfg(test)]
mod launch_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn only_installed_ids_reach_mock_steam() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join(".local/share/Steam/steamapps");
        fs::create_dir_all(&library).unwrap();
        fs::write(
            library.join("appmanifest_42.acf"),
            "AppState { appid 42 name Test StateFlags 4 }",
        )
        .unwrap();
        let tools = dir.path().join("tools");
        fs::create_dir(&tools).unwrap();
        let log = dir.path().join("args");
        fs::write(
            tools.join("steam"),
            format!("#!/bin/sh\nprintf '%s' \"$1\" > '{}'\n", log.display()),
        )
        .unwrap();
        fs::set_permissions(tools.join("steam"), fs::Permissions::from_mode(0o755)).unwrap();
        crate::process::TEST_TOOLS.with(|p| *p.borrow_mut() = Some(tools));
        assert!(launch_installed(dir.path(), 999).is_err());
        assert!(!log.exists());
        launch_installed(dir.path(), 42).unwrap();
        assert_eq!(fs::read_to_string(log).unwrap(), "steam://rungameid/42");
        crate::process::TEST_TOOLS.with(|p| *p.borrow_mut() = None);
    }
}
