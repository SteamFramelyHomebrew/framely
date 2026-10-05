use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

pub fn roots(home: &Path) -> Vec<(String, PathBuf)> {
    let mut roots = vec![
        ("下载".into(), home.join("Downloads")),
        ("主目录".into(), home.into()),
        ("桌面".into(), home.join("Desktop")),
        ("文档".into(), home.join("Documents")),
        ("临时目录".into(), PathBuf::from("/tmp")),
    ];
    if let Some(user) = home.file_name() {
        if let Ok(devices) = fs::read_dir(Path::new("/run/media").join(user)) {
            roots.extend(
                devices
                    .flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())),
            );
        }
    }
    roots
        .into_iter()
        .filter_map(|(name, path)| {
            path.canonicalize()
                .ok()
                .filter(|p| p.is_dir())
                .map(|p| (name, p))
        })
        .collect()
}
fn checked_roots(roots: &[(String, PathBuf)], path: &Path) -> Result<PathBuf> {
    let path = path
        .canonicalize()
        .context("文件或目录不存在，或无权访问")?;
    ensure!(
        roots.iter().any(|(_, root)| path.starts_with(root)),
        "只能选择用户目录、临时目录或已挂载存储中的文件"
    );
    Ok(path)
}
pub fn checked(home: &Path, path: &Path) -> Result<PathBuf> {
    checked_roots(&roots(home), path)
}
pub fn list(home: &Path, params: &Value) -> Result<Value> {
    let roots = roots(home);
    let default = roots.first().context("没有可访问的目录")?.1.clone();
    let path = checked_roots(
        &roots,
        Path::new(
            params["path"]
                .as_str()
                .filter(|p| !p.is_empty())
                .unwrap_or(default.to_str().context("Invalid file path")?),
        ),
    )?;
    ensure!(path.is_dir(), "请选择目录");
    let query = params["query"].as_str().unwrap_or("").to_lowercase();
    ensure!(query.len() <= 1024, "搜索内容过长");
    let hidden = params["hidden"] == true;
    let extensions: Vec<String> = params["extensions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str().map(str::to_lowercase))
        .collect();
    ensure!(
        extensions.len() <= 128
            && extensions
                .iter()
                .all(|s| s.starts_with('.') && s.len() <= 128),
        "Invalid file filters"
    );
    let offset = params["offset"].as_u64().unwrap_or(0) as usize;
    let mut entries = vec![];
    let mut skipped = 0;
    for (index, entry) in fs::read_dir(&path).context("无法读取目录")?.enumerate() {
        ensure!(index < 50000, "目录文件过多，请选择更具体的目录");
        let Ok(entry) = entry else {
            skipped += 1;
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if (!hidden && name.starts_with('.')) || !name.to_lowercase().contains(&query) {
            continue;
        }
        let Ok(target) = checked_roots(&roots, &entry.path()) else {
            skipped += 1;
            continue;
        };
        let Ok(meta) = fs::metadata(&target) else {
            skipped += 1;
            continue;
        };
        if !meta.is_file() && !meta.is_dir() {
            continue;
        }
        if !meta.is_dir()
            && (params["directoryOnly"] == true
                || (!extensions.is_empty()
                    && !extensions
                        .iter()
                        .any(|ext| name.to_lowercase().ends_with(ext))))
        {
            continue;
        }
        entries.push(json!({"name":name,"path":entry.path(),"directory":meta.is_dir(),"size":meta.len(),"modified":meta.modified().ok().and_then(|t|t.duration_since(UNIX_EPOCH).ok()).map(|t|t.as_secs())}));
    }
    entries.sort_by(|a, b| {
        b["directory"]
            .as_bool()
            .cmp(&a["directory"].as_bool())
            .then_with(|| {
                a["name"]
                    .as_str()
                    .unwrap()
                    .to_lowercase()
                    .cmp(&b["name"].as_str().unwrap().to_lowercase())
            })
            .then_with(|| a["name"].as_str().cmp(&b["name"].as_str()))
    });
    let total = entries.len();
    let parent = path.parent().and_then(|p| checked_roots(&roots, p).ok());
    Ok(
        json!({"path":path,"parent":parent,"roots":roots.iter().map(|(name,path)|json!({"name":name,"path":path})).collect::<Vec<_>>(),"entries":entries.into_iter().skip(offset).take(200).collect::<Vec<_>>(),"total":total,"offset":offset,"skipped":skipped}),
    )
}
pub fn selection(home: &Path, params: &Value) -> Result<Vec<PathBuf>> {
    let paths = params["paths"]
        .as_array()
        .context("Missing selected files")?;
    ensure!(paths.len() <= 64, "选择的文件过多");
    paths
        .iter()
        .map(|p| {
            let path = checked(
                home,
                Path::new(p.as_str().context("Invalid selected file")?),
            )?;
            if params["directory"] == true {
                ensure!(path.is_dir(), "请选择目录");
            } else {
                ensure!(path.is_file(), "请选择文件");
                fs::File::open(&path).context("无法读取所选文件")?;
            }
            Ok(path)
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browsing_filters_sorts_and_keeps_selection_within_roots() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir(home.path().join("Downloads")).unwrap();
        fs::create_dir(home.path().join("Downloads/Z folder")).unwrap();
        fs::write(home.path().join("Downloads/a.apk"), b"apk").unwrap();
        fs::write(home.path().join("Downloads/.hidden"), b"hidden").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", home.path().join("Downloads/escape")).unwrap();
        let listed = list(home.path(), &json!({})).unwrap();
        assert_eq!(listed["total"], 2);
        assert_eq!(listed["entries"][0]["name"], "Z folder");
        assert_eq!(
            list(home.path(), &json!({"query":"A.APK"})).unwrap()["total"],
            1
        );
        assert_eq!(
            list(home.path(), &json!({"hidden":true})).unwrap()["total"],
            3
        );
        assert!(selection(
            home.path(),
            &json!({"paths":[home.path().join("Downloads/escape")]})
        )
        .is_err());
        assert!(selection(
            home.path(),
            &json!({"paths":[home.path().join("Downloads/a.apk")]})
        )
        .is_ok());
        assert!(selection(
            home.path(),
            &json!({"paths":[home.path().join("Downloads")]})
        )
        .is_err());
        assert!(selection(
            home.path(),
            &json!({"paths":[home.path().join("Downloads")],"directory":true})
        )
        .is_ok());
    }
}
