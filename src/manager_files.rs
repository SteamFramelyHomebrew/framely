//! Filesystem operations run exclusively inside the unprivileged session service.
use crate::jobs::Cancellation;
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    os::unix::fs::{symlink, FileExt, MetadataExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
pub fn token() -> String {
    hex::encode(rand::random::<[u8; 16]>())
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn strparam<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    p[key].as_str().with_context(|| format!("Missing {key}"))
}
fn path(p: &Value, key: &str) -> Result<PathBuf> {
    let v = PathBuf::from(strparam(p, key)?);
    ensure!(v.is_absolute(), "Use an absolute file path");
    ensure!(v != Path::new("/"), "Cannot modify the filesystem root");
    let parent = v.parent().context("Missing parent")?.canonicalize()?;
    Ok(parent.join(v.file_name().context("Missing filename")?))
}
fn valid_name(s: &str) -> Result<()> {
    ensure!(
        !s.is_empty()
            && s != "."
            && s != ".."
            && !s.contains('/')
            && !s.contains('\0')
            && s.len() <= 255,
        "Invalid filename"
    );
    Ok(())
}
fn state(home: &Path) -> Result<PathBuf> {
    let dir = home.join(".local/share/framely/file-manager");
    fs::create_dir_all(&dir)?;
    let meta = fs::symlink_metadata(&dir)?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink() && meta.uid() == unsafe { libc::geteuid() },
        "Invalid file manager state directory"
    );
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}
fn save_json(path: &Path, v: &Value) -> Result<()> {
    let temp = path.with_extension(format!("{}.tmp", token()));
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    f.write_all(&serde_json::to_vec(v)?)?;
    f.sync_all()?;
    fs::rename(&temp, path)?;
    Ok(())
}
use std::os::unix::fs::OpenOptionsExt;
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ViewPreferences {
    #[serde(default = "default_file_view")]
    view: String,
    #[serde(default)]
    folders: BTreeMap<String, FolderSort>,
}
fn default_file_view() -> String {
    "list".into()
}
#[derive(Serialize, Deserialize)]
struct FolderSort {
    sort: String,
    descending: bool,
}
fn view_preferences(home: &Path, p: &Value) -> Result<Value> {
    let file = state(home)?.join("view-preferences.json");
    let mut preferences: ViewPreferences = if file.exists() {
        serde_json::from_slice(&fs::read(&file)?)?
    } else {
        ViewPreferences {
            view: default_file_view(),
            ..Default::default()
        }
    };
    let mut changed = false;
    if let Some(view) = p.get("view") {
        let view = view.as_str().context("Invalid file view")?;
        ensure!(
            matches!(
                view,
                "list" | "icons-small" | "icons-medium" | "icons-large"
            ),
            "Invalid file view"
        );
        preferences.view = view.into();
        changed = true;
    }
    if p.get("path").is_some() {
        let dir = PathBuf::from(strparam(p, "path")?);
        ensure!(dir.is_absolute(), "Use an absolute file path");
        let dir = dir.canonicalize()?;
        ensure!(dir.is_dir(), "Select a directory");
        let sort = strparam(p, "sort")?;
        ensure!(
            matches!(sort, "name" | "size" | "modified"),
            "Invalid file sort"
        );
        let descending = p["descending"]
            .as_bool()
            .context("Invalid file sort direction")?;
        preferences.folders.insert(
            dir.to_str().context("Invalid directory path")?.into(),
            FolderSort {
                sort: sort.into(),
                descending,
            },
        );
        changed = true;
    }
    let value = serde_json::to_value(preferences)?;
    if changed {
        save_json(&file, &value)?;
    }
    Ok(value)
}
#[derive(Default)]
pub struct Files {
    preferences: Mutex<()>,
    uploads: Mutex<BTreeMap<String, Upload>>,
    downloads: Mutex<BTreeMap<String, Download>>,
}
struct Upload {
    temp: PathBuf,
    target: PathBuf,
    size: u64,
    received: u64,
    file: fs::File,
    ranges: BTreeMap<u64, u64>,
    policy: String,
    created: u64,
}
impl Drop for Upload {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.temp);
    }
}
struct Download {
    path: PathBuf,
    inline: bool,
    owned: bool,
    expires: u64,
}
impl Drop for Download {
    fn drop(&mut self) {
        if self.owned {
            let _ = fs::remove_file(&self.path);
        }
    }
}
fn entry(p: &Path) -> Result<Value> {
    let m = fs::symlink_metadata(p)?;
    let link = m.file_type().is_symlink();
    let dir = if link {
        fs::metadata(p).is_ok_and(|m| m.is_dir())
    } else {
        m.is_dir()
    };
    // Revision uses target metadata (including nanoseconds), never media bytes.
    let media_revision = if !dir && media_type(p).is_some() {
        let target = if link {
            fs::metadata(p).ok()
        } else {
            Some(m.clone())
        };
        target.filter(|m| m.is_file()).map(|m| {
            format!(
                "{}:{}:{}:{}:{}",
                m.dev(),
                m.ino(),
                m.len(),
                m.mtime(),
                m.mtime_nsec()
            )
        })
    } else {
        None
    };
    Ok(
        json!({"name":p.file_name().unwrap_or_default().to_string_lossy(),"path":p,"directory":dir,"symlink":link,"size":m.len(),"mediaRevision":media_revision,"modified":m.modified().ok().and_then(|t|t.duration_since(UNIX_EPOCH).ok()).map(|t|t.as_secs()),"mode":m.mode()&0o7777,"uid":m.uid(),"gid":m.gid(),"linkTarget":if link{fs::read_link(p).ok()}else{None}}),
    )
}
fn list(p: &Value, home: &Path) -> Result<Value> {
    let dir = PathBuf::from(
        p["path"]
            .as_str()
            .unwrap_or(home.to_str().context("Invalid home")?),
    )
    .canonicalize()?;
    ensure!(dir.is_dir(), "Select a directory");
    let query = p["query"].as_str().unwrap_or("").to_lowercase();
    let mut entries = Vec::new();
    let mut skipped = 0;
    for item in fs::read_dir(&dir)? {
        let Ok(item) = item else {
            skipped += 1;
            continue;
        };
        let name = item.file_name().to_string_lossy().into_owned();
        if (!p["hidden"].as_bool().unwrap_or(false) && name.starts_with('.'))
            || !name.to_lowercase().contains(&query)
        {
            continue;
        }
        match entry(&item.path()) {
            Ok(v) => entries.push(v),
            Err(_) => skipped += 1,
        }
        ensure!(
            entries.len() <= 50000,
            "Directory has too many files; refine the search"
        );
    }
    let sort = p["sort"].as_str().unwrap_or("name");
    let reverse = p["descending"] == true;
    entries.sort_by(|a, b| {
        b["directory"]
            .as_bool()
            .cmp(&a["directory"].as_bool())
            .then_with(|| {
                let order = match sort {
                    "size" => a["size"].as_u64().cmp(&b["size"].as_u64()),
                    "modified" => a["modified"].as_u64().cmp(&b["modified"].as_u64()),
                    _ => a["name"]
                        .as_str()
                        .unwrap()
                        .to_lowercase()
                        .cmp(&b["name"].as_str().unwrap().to_lowercase()),
                };
                if reverse {
                    order.reverse()
                } else {
                    order
                }
            })
    });
    let total = entries.len();
    let offset = p["offset"].as_u64().unwrap_or(0) as usize;
    let mut roots = vec![
        json!({"name":"Home","path":home}),
        json!({"name":"Downloads","path":home.join("Downloads")}),
        json!({"name":"Filesystem","path":"/"}),
    ];
    let media = PathBuf::from("/run/media").join(home.file_name().unwrap_or_default());
    if let Ok(devices) = fs::read_dir(media) {
        for device in devices.flatten() {
            if device.path().is_dir() {
                roots.push(
                    json!({"name":device.file_name().to_string_lossy(),"path":device.path()}),
                );
            }
        }
    }
    Ok(
        json!({"path":dir,"parent":dir.parent(),"entries":entries.into_iter().skip(offset).take(200).collect::<Vec<_>>(),"total":total,"offset":offset,"skipped":skipped,"roots":roots}),
    )
}
fn fingerprint(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn binary(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .any(|byte| (*byte < 0x20 && !matches!(*byte, b'\t' | b'\r' | b'\n')) || *byte == 0x7f)
}
fn read_text(p: &Path) -> Result<Value> {
    let m = fs::metadata(p)?;
    ensure!(
        m.is_file() && m.len() <= 5 * 1024 * 1024,
        "Text editor supports files up to 5 MiB"
    );
    let mut bytes = Vec::new();
    fs::File::open(p)?
        .take(5 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 5 * 1024 * 1024,
        "Text editor supports files up to 5 MiB"
    );
    ensure!(!binary(&bytes), "Binary files cannot be edited as text");
    let text = std::str::from_utf8(&bytes).context("Only UTF-8 text can be edited")?;
    Ok(json!({"text":text,"revision":fingerprint(&bytes)}))
}
fn remove(p: &Path) -> Result<()> {
    let m = fs::symlink_metadata(p)?;
    if m.is_dir() && !m.file_type().is_symlink() {
        fs::remove_dir_all(p)?;
    } else {
        fs::remove_file(p)?;
    }
    Ok(())
}
fn size(p: &Path, cancel: &Cancellation) -> Result<u64> {
    cancel.check()?;
    let m = fs::symlink_metadata(p)?;
    if m.is_dir() {
        let mut bytes = 0;
        for e in fs::read_dir(p)? {
            bytes += size(&e?.path(), cancel)?;
        }
        Ok(bytes)
    } else if m.is_file() {
        Ok(m.len())
    } else {
        Ok(0)
    }
}
fn target(dest: &Path, policy: &str, approve: bool) -> Result<Option<PathBuf>> {
    if fs::symlink_metadata(dest).is_err() {
        return Ok(Some(dest.into()));
    }
    match policy {
        "skip" => Ok(None),
        "keep" => {
            let stem = dest.file_stem().unwrap_or_default().to_string_lossy();
            let ext = dest
                .extension()
                .map(|v| format!(".{}", v.to_string_lossy()))
                .unwrap_or_default();
            for i in 1..10000 {
                let p = dest.with_file_name(format!("{stem} ({i}){ext}"));
                if fs::symlink_metadata(&p).is_err() {
                    return Ok(Some(p));
                }
            }
            bail!("Too many filename conflicts")
        }
        "overwrite" => {
            ensure!(approve, "Confirm overwriting files");
            Ok(Some(dest.into()))
        }
        _ => bail!("A file with that name already exists"),
    }
}
fn copy(
    src: &Path,
    dest: &Path,
    cancel: &Cancellation,
    progress: &dyn Fn(Value),
    done: &mut u64,
    total: u64,
) -> Result<()> {
    cancel.check()?;
    let m = fs::symlink_metadata(src)?;
    if m.file_type().is_symlink() {
        symlink(fs::read_link(src)?, dest)?;
    } else if m.is_dir() {
        fs::create_dir(dest)?;
        for e in fs::read_dir(src)? {
            let e = e?;
            copy(
                &e.path(),
                &dest.join(e.file_name()),
                cancel,
                progress,
                done,
                total,
            )?;
        }
        fs::set_permissions(dest, m.permissions())?;
    } else if m.is_file() {
        let mut input = fs::File::open(src)?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(m.mode() & 0o777)
            .open(dest)?;
        let mut bytes = vec![0u8; 65536];
        loop {
            cancel.check()?;
            let n = input.read(&mut bytes)?;
            if n == 0 {
                break;
            }
            output.write_all(&bytes[..n])?;
            *done += n as u64;
            progress(json!({"phase":"copying","bytes":*done,"totalBytes":total,"current":src}));
        }
        output.sync_all()?;
    } else {
        bail!("Special files cannot be copied");
    }
    let after = fs::symlink_metadata(src)?;
    ensure!(
        m.dev() == after.dev()
            && m.ino() == after.ino()
            && m.len() == after.len()
            && m.mtime() == after.mtime()
            && m.mtime_nsec() == after.mtime_nsec(),
        "Source changed during transfer; original retained"
    );
    Ok(())
}
#[derive(Serialize, Deserialize)]
struct Trash {
    id: String,
    original: PathBuf,
    stored: PathBuf,
    deleted: u64,
}
fn trash_info(home: &Path) -> Result<PathBuf> {
    let dir = state(home)?.join("trash-info");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}
fn read_trash(home: &Path) -> Result<Vec<Trash>> {
    let mut items = vec![];
    for e in fs::read_dir(trash_info(home)?)? {
        let e = e?;
        if e.path().extension().is_some_and(|v| v == "json") {
            let bytes = fs::read(e.path())?;
            if let Ok(t) = serde_json::from_slice(&bytes) {
                items.push(t);
            }
        }
    }
    Ok(items)
}
fn trash(home: &Path, src: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(src)?;
    let home_trash = state(home)?.join("trash");
    fs::create_dir_all(&home_trash)?;
    let root = if metadata.dev() == fs::metadata(&home_trash)?.dev() {
        home_trash
    } else {
        let mut mount = src.parent().context("Missing parent")?.to_path_buf();
        while let Some(parent) = mount.parent() {
            if fs::metadata(parent)?.dev() != metadata.dev() {
                break;
            }
            mount = parent.into();
        }
        let trash = mount.join(format!(".framely-trash-{}", unsafe { libc::geteuid() }));
        fs::create_dir_all(&trash).context(
            "This storage cannot use the recycle bin; choose permanent deletion explicitly",
        )?;
        trash
    };
    let root_meta = fs::symlink_metadata(&root)?;
    ensure!(
        root_meta.is_dir()
            && !root_meta.file_type().is_symlink()
            && root_meta.uid() == unsafe { libc::geteuid() },
        "Invalid recycle bin directory"
    );
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    ensure!(
        metadata.dev() == fs::metadata(&root)?.dev(),
        "This storage cannot use the Framely recycle bin; choose permanent deletion explicitly"
    );
    let id = token();
    let stored = root.join(&id);
    let item = Trash {
        id: id.clone(),
        original: src.into(),
        stored: stored.clone(),
        deleted: now(),
    };
    let info = trash_info(home)?.join(format!("{id}.json"));
    save_json(&info, &serde_json::to_value(&item)?)?;
    if let Err(e) = fs::rename(src, &stored) {
        let _ = fs::remove_file(info);
        return Err(e.into());
    }
    Ok(())
}
fn checked_trash(home: &Path, id: &str) -> Result<Trash> {
    ensure!(
        id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid trash entry"
    );
    let t: Trash =
        serde_json::from_slice(&fs::read(trash_info(home)?.join(format!("{id}.json")))?)?;
    let root = t.stored.parent().context("Invalid trash record")?;
    let normal = state(home)?.join("trash");
    let external = format!(".framely-trash-{}", unsafe { libc::geteuid() });
    ensure!(
        t.id == id
            && t.stored.file_name().is_some_and(|n| n == id)
            && (root == normal || root.file_name().is_some_and(|n| n == external.as_str())),
        "Invalid trash record"
    );
    let meta = fs::symlink_metadata(root)?;
    ensure!(
        meta.is_dir() && !meta.file_type().is_symlink() && meta.uid() == unsafe { libc::geteuid() },
        "Recycle bin is unavailable or invalid"
    );
    Ok(t)
}
impl Files {
    pub fn maintain(&self, home: &Path) -> Result<()> {
        let _guard = self.preferences.lock().unwrap();
        self.uploads
            .lock()
            .unwrap()
            .retain(|_, u| now().saturating_sub(u.created) < 3600);
        self.downloads
            .lock()
            .unwrap()
            .retain(|_, d| d.expires > now());
        for t in read_trash(home)? {
            if now().saturating_sub(t.deleted) >= 30 * 86400 {
                let Ok(t) = checked_trash(home, &t.id) else {
                    continue;
                };
                if fs::symlink_metadata(&t.stored).is_ok() {
                    remove(&t.stored)?;
                }
                fs::remove_file(trash_info(home)?.join(format!("{}.json", t.id)))?;
            }
        }
        Ok(())
    }
    pub fn api(&self, home: &Path, p: &Value) -> Result<Value> {
        ensure!(
            unsafe { libc::geteuid() } != 0,
            "Files must run as the Steam session user"
        );
        let operation = p["operation"].as_str().unwrap_or("list");
        match operation {
            "list" => list(p, home),"properties" => entry(&path(p,"path")?),
            "view.preferences" => {
                let _guard = self.preferences.lock().unwrap();
                view_preferences(home, p)
            },
            "preferences" => {
                let _guard = self.preferences.lock().unwrap();
                let file = state(home)?.join("preferences.json");
                if let Some(v) = p.get("value"){
                    let list = v.as_array().context("Invalid bookmarks")?;
                    ensure!(list.len()<=64,"Too many bookmarks");
                    for b in list{
                        valid_name(strparam(b,"name")?)?;
                        ensure!(Path::new(strparam(b,"path")?).is_absolute(),"Invalid bookmark path");
                    }
                    save_json(&file, v)?;
                }
                Ok(if file.exists(){
                    serde_json::from_slice(&fs::read(file)?)?
                } else {
                    json!([])
                })
            },
            "read" => read_text(&path(p,"path")?),
            "write" => {
                let file = path(p,"path")?.canonicalize()?;
                let current = read_text(&file)?;
                ensure!(current["revision"]==p["revision"]||p["approve"]==true,"File changed outside the editor; reload or confirm overwrite");
                let text = strparam(p,"text")?;
                ensure!(text.len()<=5*1024*1024&&!binary(text.as_bytes()),"Text is too large or contains binary data");
                let temp = file.with_file_name(format!(".framely-write-{}", token()));
                let _guard = Temporary(temp.clone());
                let mut f = fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(&temp)?;
                f.write_all(text.as_bytes())?;
                f.sync_all()?;
                fs::set_permissions(&temp, fs::metadata(&file)?.permissions())?;
                ensure!(read_text(&file)?["revision"]==current["revision"],"File changed outside the editor; reload or confirm overwrite");
                fs::rename(&temp, &file)?;
                read_text(&file)
            },
            "create" => {
                let parent = PathBuf::from(strparam(p,"directory")?).canonicalize()?;
                let name = strparam(p,"name")?;
                valid_name(name)?;
                let file = parent.join(name);
                if p["folder"]==true{
                    fs::create_dir(file)?;
                } else {
                    fs::OpenOptions::new().write(true).create_new(true).mode(0o644).open(file)?;
                }
                Ok(json!(true))
            },
            "rename" => {
                let src = path(p,"path")?;
                let name = strparam(p,"name")?;
                valid_name(name)?;
                let dest = src.with_file_name(name);
                ensure!(fs::symlink_metadata(&dest).is_err(),"A file with that name already exists");
                fs::rename(src, dest)?;
                Ok(json!(true))
            },
            "chmod" => {
                let file = path(p,"path")?;
                ensure!(!fs::symlink_metadata(&file)?.file_type().is_symlink(),"Cannot change symlink permissions");
                let mode = p["mode"].as_u64().context("Missing file permissions")?;
                ensure!(mode<=0o777,"Only normal user permission bits can be changed");
                fs::set_permissions(file, fs::Permissions::from_mode(mode as u32))?;
                Ok(json!(true))
            },
            "trash.list" => Ok(json!(read_trash(home)?.into_iter().map(|t|json!({
                "id":t.id,"name":t.original.file_name().unwrap_or_default().to_string_lossy(),"path":t.original,"deleted":t.deleted,"expires":t.deleted+30*86400
            })).collect::<Vec<_>>())),
            "trash.restore" => {
                let _guard = self.preferences.lock().unwrap();
                let t = checked_trash(home, strparam(p,"id")?)?;
                let parent = t.original.parent().context("Missing original directory")?.canonicalize()?;
                let dest = parent.join(t.original.file_name().unwrap());
                let dest = target(&dest, p["conflict"].as_str().unwrap_or("error"), p["approve"]==true)?.context("Restore skipped")?;
                ensure!(fs::symlink_metadata(&dest).is_err(),"Restore to an unused filename");
                fs::rename(&t.stored, dest)?;
                fs::remove_file(trash_info(home)?.join(format!("{}.json", t.id)))?;
                Ok(json!(true))
            },
            "trash.purge" => {
                ensure!(p["approve"]==true,"Confirm permanent deletion");
                let _guard = self.preferences.lock().unwrap();
                let ids:Vec<String>=if p["all"]==true{
                    read_trash(home)?.into_iter().map(|t|t.id).collect()
                } else {
                    vec![strparam(p,"id")?.into()]
                }
                ;
                for id in ids{
                    let t = checked_trash(home, &id)?;
                    if fs::symlink_metadata(&t.stored).is_ok(){
                        remove(&t.stored)?;
                    }
                    fs::remove_file(trash_info(home)?.join(format!("{id}.json")))?;
                }
                Ok(json!(true))
            },
            "download"|"preview" => {
                let file = path(p,"path")?.canonicalize()?;
                ensure!(file.is_file(),"Select a regular file");
                let inline = p["operation"]=="preview";
                if inline{
                    ensure!(media_type(&file).is_some(),"This format cannot be previewed; download it instead");
                }
                let id = token();
                self.downloads.lock().unwrap().insert(id.clone(), Download{
                    path:file, inline, owned:false, expires:now()+3600
                });
                Ok(json!({
                    "url":format!("/manager-api/file-content/{id}"),"token":if inline{Some(id)}else{None}
                }))
            },
            "preview.release" => {
                let id = strparam(p,"token")?;
                let mut downloads = self.downloads.lock().unwrap();
                if downloads.get(id).is_some_and(|d| d.inline && !d.owned) { downloads.remove(id); }
                Ok(json!(true))
            },
            "upload.start" => {
                let parent = PathBuf::from(strparam(p,"directory")?).canonicalize()?;
                let relative = Path::new(strparam(p,"name")?);
                ensure!(!relative.is_absolute()&&relative.components().all(|c|matches!(c, Component::Normal(_))),"Invalid upload path");
                let target = parent.join(relative);
                let folder = target.parent().context("Missing upload directory")?;
                let mut ancestor = folder;
                while !ancestor.exists(){
                    ancestor = ancestor.parent().context("Invalid upload parent")?;
                }
                ensure!(ancestor.canonicalize()?.starts_with(&parent),"Upload path escapes its destination");
                fs::create_dir_all(folder)?;
                ensure!(folder.canonicalize()?.starts_with(&parent),"Upload path escapes its destination");
                ensure!(self.uploads.lock().unwrap().len()<64,"Too many simultaneous uploads");
                let size = p["size"].as_u64().context("Missing upload size")?;
                ensure!(size<=64*1024*1024*1024,"Upload too large");
                let id = token();
                let temp = folder.join(format!(".framely-upload-{id}"));
                let file = fs::OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(&temp)?;
                self.uploads.lock().unwrap().insert(id.clone(), Upload{
                    temp, target, size, received:0, file, ranges:BTreeMap::new(), policy:p["conflict"].as_str().unwrap_or("error").into(), created:now()
                });
                Ok(json!({
                    "id":id,"chunkSize":crate::uploads::CHUNK
                }))
            },
            "upload.cancel" => {
                self.uploads.lock().unwrap().remove(strparam(p,"id")?);
                Ok(json!(true))
            },
            "upload.finish" => {
                let id = strparam(p,"id")?;
                let mut uploads = self.uploads.lock().unwrap();
                let u = uploads.get(id).context("Upload expired")?;
                ensure!(u.size==u.received,"Upload is incomplete");
                u.file.sync_all()?;
                let dest = target(&u.target, &u.policy, p["approve"]==true)?;
                if let Some(dest) = dest{
                    ensure!(!fs::symlink_metadata(&dest).is_ok_and(|m|m.is_dir()),"Cannot overwrite a directory with a file");
                    fs::rename(&u.temp, dest)?;
                }
                uploads.remove(id);
                Ok(json!(true))
            },
            _ => bail!("Unknown file operation")
        }
    }

    pub fn append(&self, id: &str, offset: u64, bytes: &[u8]) -> Result<Value> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= crate::uploads::CHUNK,
            "Invalid upload chunk size"
        );
        let mut uploads = self.uploads.lock().unwrap();
        let u = uploads.get_mut(id).context("Upload expired")?;
        ensure!(
            offset <= u.size && bytes.len() as u64 <= u.size - offset,
            "Upload exceeds declared size"
        );
        let end = offset + bytes.len() as u64;
        if u.ranges.get(&offset) == Some(&end) {
            let mut existing = vec![0; bytes.len()];
            u.file.read_exact_at(&mut existing, offset)?;
            ensure!(existing == bytes, "Upload retry content mismatch");
        } else {
            ensure!(
                u.ranges
                    .range(..=offset)
                    .next_back()
                    .is_none_or(|(_, e)| *e <= offset)
                    && u.ranges
                        .range(offset..)
                        .next()
                        .is_none_or(|(s, _)| *s >= end),
                "Overlapping upload chunk"
            );
            if let Err(error) = u.file.write_all_at(bytes, offset) {
                uploads.remove(id);
                return Err(error.into());
            }
            u.ranges.insert(offset, end);
            u.received += bytes.len() as u64;
        }
        u.created = now();
        Ok(json!({"received":end,"uploadedBytes":u.received,"total":u.size}))
    }

    pub fn content(&self, id: &str) -> Result<(PathBuf, bool)> {
        let downloads = self.downloads.lock().unwrap();
        let d = downloads
            .get(id)
            .filter(|d| d.expires > now())
            .context("Download expired")?;
        Ok((d.path.clone(), d.inline))
    }
    pub fn task(
        &self,
        home: &Path,
        p: &Value,
        cancel: Cancellation,
        progress: Arc<dyn Fn(Value) + Send + Sync>,
    ) -> Result<Value> {
        let op = strparam(p, "operation")?;
        if op == "search" {
            let base = PathBuf::from(strparam(p, "path")?).canonicalize()?;
            let query = strparam(p, "query")?.to_lowercase();
            ensure!(!query.is_empty(), "Enter a search term");
            let mut found = vec![];
            let mut stack = vec![base];
            let mut visited = 0;
            while let Some(dir) = stack.pop() {
                cancel.check()?;
                let Ok(entries) = fs::read_dir(dir) else {
                    continue;
                };
                for e in entries.flatten() {
                    cancel.check()?;
                    visited += 1;
                    ensure!(
                        visited <= 500000,
                        "Search limit reached; choose a smaller directory"
                    );
                    if let Ok(m) = fs::symlink_metadata(e.path()) {
                        if m.is_dir() {
                            stack.push(e.path());
                        }
                        if e.file_name()
                            .to_string_lossy()
                            .to_lowercase()
                            .contains(&query)
                        {
                            if let Ok(v) = entry(&e.path()) {
                                found.push(v);
                            }
                        }
                    }
                    if found.len() >= 5000 {
                        break;
                    }
                }
                progress(json!({"phase":"searching","visited":visited,"matches":found.len()}));
                if found.len() >= 5000 {
                    break;
                }
            }
            return Ok(json!({"entries":found,"visited":visited}));
        }
        if matches!(op, "compress" | "extract" | "download.batch") {
            return self.archive_task(home, p, cancel, progress);
        }
        let sources = p["paths"].as_array().context("Select files")?;
        ensure!(
            !sources.is_empty() && sources.len() <= 5000,
            "Select between 1 and 5000 files"
        );
        let paths = sources
            .iter()
            .map(|s| path(&json!({"path":s}), "path"))
            .collect::<Result<Vec<_>>>()?;
        let policy = p["conflict"].as_str().unwrap_or("error");
        let approve = p["approve"] == true;
        let mut results = vec![];
        let mut done = 0;
        let mut total = 0;
        if matches!(op, "copy" | "move") {
            for src in &paths {
                match size(src, &cancel) {
                    Ok(bytes) => total += bytes,
                    Err(error) => {
                        cancel.check()?;
                        let _ = error;
                    }
                }
            }
        }
        for src in paths {
            cancel.check()?;
            let result = (|| -> Result<Value> {
                match op {
                    "delete" => {
                        ensure!(
                            p["permanent"] != true || approve,
                            "Confirm permanent deletion"
                        );
                        if p["permanent"] == true {
                            remove(&src)?;
                        } else {
                            let _guard = self.preferences.lock().unwrap();
                            trash(home, &src)?;
                        }
                    }
                    "copy" | "move" => {
                        let parent = PathBuf::from(strparam(p, "destination")?).canonicalize()?;
                        ensure!(
                            !fs::symlink_metadata(&src)?.is_dir()
                                || !parent.starts_with(src.canonicalize().unwrap_or(src.clone())),
                            "Cannot copy or move a folder into itself"
                        );
                        let dest = parent.join(src.file_name().context("Missing filename")?);
                        let Some(dest) = target(&dest, policy, approve)? else {
                            return Ok(json!({"skipped":true}));
                        };
                        ensure!(dest != src, "Source and destination are identical");
                        let stage = parent.join(format!(".framely-transfer-{}", token()));
                        let guard = Temporary(stage.clone());
                        let copied =
                            copy(&src, &stage, &cancel, progress.as_ref(), &mut done, total);
                        if let Err(e) = copied {
                            let _ = remove(&stage);
                            return Err(e);
                        }
                        cancel.commit(|| replace(&stage, &dest))?;
                        drop(guard);
                        if op == "move" {
                            cancel.commit(|| remove(&src))?;
                        }
                    }
                    _ => bail!("Unknown file task"),
                };
                Ok(json!({"success":true}))
            })();
            results.push(match result {
                Ok(v) => json!({"path":src,"result":v}),
                Err(e) => json!({"path":src,"error":e.to_string()}),
            });
            progress(json!({"phase":"working","completed":results.len(),"total":sources.len()}));
        }
        Ok(json!({"items":results}))
    }
}
pub fn media_type(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "avif" => Some("image/avif"),
        "bmp" => Some("image/bmp"),
        "mp4" | "m4v" => Some("video/mp4"),
        "webm" => Some("video/webm"),
        "mov" => Some("video/quicktime"),
        _ => None,
    }
}
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.0).is_ok() {
            let _ = remove(&self.0);
        }
    }
}
fn archive_paths(
    src: &Path,
    name: &Path,
    files: &mut Vec<(PathBuf, PathBuf)>,
    cancel: &Cancellation,
) -> Result<()> {
    cancel.check()?;
    let m = fs::symlink_metadata(src)?;
    ensure!(
        !m.file_type().is_symlink() && (m.is_file() || m.is_dir()),
        "Archive operations do not support symbolic links or special files"
    );
    files.push((src.into(), name.into()));
    ensure!(files.len() <= 50000, "Archive contains too many entries");
    if m.is_dir() {
        for e in fs::read_dir(src)? {
            let e = e?;
            archive_paths(&e.path(), &name.join(e.file_name()), files, cancel)?;
        }
    }
    Ok(())
}
fn archive_name(p: &Path) -> Result<()> {
    ensure!(
        !p.as_os_str().is_empty()
            && !p.is_absolute()
            && p.components()
                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir)),
        "Archive entry escapes the destination directory"
    );
    Ok(())
}
fn stream_copy(
    input: &mut dyn Read,
    output: &mut dyn Write,
    cancel: &Cancellation,
    limit: u64,
    progress: &dyn Fn(Value),
    done: &mut u64,
) -> Result<u64> {
    let mut bytes = [0u8; 65536];
    let mut count = 0;
    loop {
        cancel.check()?;
        let n = input.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        count += n as u64;
        ensure!(count <= limit, "Archive entry exceeds its declared size");
        output.write_all(&bytes[..n])?;
        *done += n as u64;
        progress(json!({"phase":"working","bytes":*done}));
    }
    Ok(count)
}
impl Files {
    fn archive_task(
        &self,
        home: &Path,
        p: &Value,
        cancel: Cancellation,
        progress: Arc<dyn Fn(Value) + Send + Sync>,
    ) -> Result<Value> {
        let op = strparam(p, "operation")?;
        if op == "extract" {
            let src = path(p, "path")?;
            let dest = PathBuf::from(strparam(p, "destination")?).canonicalize()?;
            let staging = dest.join(format!(".framely-extract-{}", token()));
            fs::create_dir(&staging)?;
            fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))?;
            let guard = Temporary(staging.clone());
            let mut done = 0;
            let mut count = 0;
            let limit = 64u64 * 1024 * 1024 * 1024;
            if src
                .extension()
                .and_then(|v| v.to_str())
                .is_some_and(|v| v.eq_ignore_ascii_case("zip"))
            {
                let mut zip = zip::ZipArchive::new(fs::File::open(&src)?)?;
                ensure!(zip.len() <= 50000, "Archive has too many entries");
                for i in 0..zip.len() {
                    cancel.check()?;
                    let mut e = zip.by_index(i)?;
                    let name = PathBuf::from(e.name());
                    archive_name(&name)?;
                    ensure!(
                        e.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
                        "Archive symbolic links are not supported"
                    );
                    ensure!(
                        e.unix_mode()
                            .is_none_or(|mode| matches!(mode & 0o170000, 0 | 0o100000 | 0o040000)),
                        "Archive links and special files are not supported"
                    );
                    let target = staging.join(name);
                    if e.is_dir() {
                        fs::create_dir_all(target)?;
                    } else {
                        ensure!(e.size() <= limit - done, "Expanded archive is too large");
                        fs::create_dir_all(target.parent().unwrap())?;
                        let mut out = fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .mode(e.unix_mode().unwrap_or(0o600) & 0o777)
                            .open(target)?;
                        let size = e.size();
                        stream_copy(
                            &mut e,
                            &mut out,
                            &cancel,
                            size,
                            progress.as_ref(),
                            &mut done,
                        )?;
                    }
                    count += 1;
                }
            } else {
                let file = fs::File::open(&src)?;
                let compressed = src
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .ends_with(".tar.gz")
                    || src
                        .extension()
                        .and_then(|v| v.to_str())
                        .is_some_and(|v| v.eq_ignore_ascii_case("tgz"));
                let input: Box<dyn Read> = if compressed {
                    Box::new(flate2::read::GzDecoder::new(file))
                } else {
                    Box::new(file)
                };
                let mut archive = tar::Archive::new(input);
                for e in archive.entries()? {
                    cancel.check()?;
                    let mut e = e?;
                    count += 1;
                    ensure!(count <= 50000, "Archive has too many entries");
                    let name = e.path()?.into_owned();
                    archive_name(&name)?;
                    let kind = e.header().entry_type();
                    let mode = e.header().mode()? & 0o777;
                    ensure!(
                        kind.is_file() || kind.is_dir(),
                        "Archive links and special files are not supported"
                    );
                    let target = staging.join(name);
                    if kind.is_dir() {
                        fs::create_dir_all(target)?;
                    } else {
                        let size = e.size();
                        ensure!(size <= limit - done, "Expanded archive is too large");
                        fs::create_dir_all(target.parent().unwrap())?;
                        let mut out = fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .mode(mode)
                            .open(target)?;
                        stream_copy(
                            &mut e,
                            &mut out,
                            &cancel,
                            size,
                            progress.as_ref(),
                            &mut done,
                        )?;
                    }
                }
            }
            let mut results = vec![];
            for e in fs::read_dir(&staging)? {
                cancel.check()?;
                let e = e?;
                let target = dest.join(e.file_name());
                let resolved = target_resolution(&target, p)?;
                if let Some(target) = resolved {
                    cancel.commit(|| replace(&e.path(), &target))?;
                    results.push(json!({"path":target,"success":true}));
                } else {
                    results.push(json!({"path":target,"skipped":true}));
                }
            }
            drop(guard);
            return Ok(json!({"entries":count,"bytes":done,"items":results}));
        }
        let paths = p["paths"].as_array().context("Select files")?;
        ensure!(!paths.is_empty() && paths.len() <= 5000, "Select files");
        let mut files = vec![];
        let mut names = std::collections::BTreeSet::new();
        for item in paths {
            let src = path(&json!({"path":item}), "path")?;
            let name = src.file_name().context("Missing filename")?;
            ensure!(
                names.insert(name.to_os_string()),
                "Archive contains duplicate top-level names"
            );
            archive_paths(&src, Path::new(name), &mut files, &cancel)?;
        }
        let output = if op == "download.batch" {
            state(home)?.join(format!("download-{}.zip", token()))
        } else {
            path(p, "path")?
        };
        let parent = output.parent().unwrap();
        let temporary = parent.join(format!(".framely-archive-{}", token()));
        let guard = Temporary(temporary.clone());
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        let format = if op == "download.batch" {
            "zip"
        } else {
            p["format"].as_str().unwrap_or("zip")
        };
        let mut done = 0;
        if format == "zip" {
            let mut zip = zip::ZipWriter::new(file);
            for (src, name) in &files {
                cancel.check()?;
                let meta = fs::metadata(src)?;
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .unix_permissions(meta.mode() & 0o777);
                if meta.is_dir() {
                    zip.add_directory(name.to_string_lossy(), options)?;
                } else {
                    zip.start_file(name.to_string_lossy(), options)?;
                    stream_copy(
                        &mut fs::File::open(src)?,
                        &mut zip,
                        &cancel,
                        meta.len(),
                        progress.as_ref(),
                        &mut done,
                    )?;
                }
            }
            zip.finish()?.sync_all()?;
        } else {
            ensure!(
                matches!(format, "tar" | "tar.gz"),
                "Unsupported archive format"
            );
            if format == "tar.gz" {
                let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
                write_tar(encoder, &files, &cancel, progress.as_ref(), &mut done)?
                    .finish()?
                    .sync_all()?;
            } else {
                write_tar(file, &files, &cancel, progress.as_ref(), &mut done)?.sync_all()?;
            }
        }
        cancel.check()?;
        let output = target_resolution(&output, p)?.context("Archive creation skipped")?;
        ensure!(
            !fs::symlink_metadata(&output).is_ok_and(|m| m.is_dir()),
            "Cannot overwrite a directory with an archive"
        );
        replace(&temporary, &output)?;
        drop(guard);
        if op == "download.batch" {
            let id = token();
            self.downloads.lock().unwrap().insert(
                id.clone(),
                Download {
                    path: output,
                    inline: false,
                    owned: true,
                    expires: now() + 3600,
                },
            );
            Ok(json!({"url":format!("/manager-api/file-content/{id}")}))
        } else {
            Ok(json!({"path":output}))
        }
    }
}
fn target_resolution(path: &Path, p: &Value) -> Result<Option<PathBuf>> {
    target(
        path,
        p["conflict"].as_str().unwrap_or("error"),
        p["approve"] == true,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn media_revisions_track_replacements_and_symlink_targets() {
        let temp = tempfile::tempdir().unwrap();
        let image = temp.path().join("sample.png");
        fs::write(&image, b"generated").unwrap();
        let first = entry(&image).unwrap()["mediaRevision"].clone();
        let replacement = temp.path().join("replacement.png");
        fs::write(&replacement, b"different").unwrap();
        fs::rename(&replacement, &image).unwrap();
        let second = entry(&image).unwrap()["mediaRevision"].clone();
        assert_ne!(first, second);
        let link = temp.path().join("link.png");
        symlink(&image, &link).unwrap();
        assert_eq!(entry(&link).unwrap()["mediaRevision"], second);
        fs::write(temp.path().join("other.png"), b"different").unwrap();
        fs::remove_file(&link).unwrap();
        symlink(temp.path().join("other.png"), &link).unwrap();
        assert_ne!(entry(&link).unwrap()["mediaRevision"], second);
        fs::write(temp.path().join("notes.txt"), b"generated").unwrap();
        assert!(entry(&temp.path().join("notes.txt")).unwrap()["mediaRevision"].is_null());
    }
    #[test]
    fn thumbnail_grants_can_be_released_without_revoking_downloads() {
        let temp = tempfile::tempdir().unwrap();
        let image = temp.path().join("sample.png");
        fs::write(&image, b"generated").unwrap();
        let files = Files::default();
        let preview = files
            .api(temp.path(), &json!({"operation":"preview","path":image}))
            .unwrap();
        let token = preview["token"].as_str().unwrap();
        assert!(files.content(token).unwrap().1);
        files
            .api(
                temp.path(),
                &json!({"operation":"preview.release","token":token}),
            )
            .unwrap();
        assert!(files.content(token).is_err());
        let download = files
            .api(temp.path(), &json!({"operation":"download","path":image}))
            .unwrap();
        let token = download["url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap();
        files
            .api(
                temp.path(),
                &json!({"operation":"preview.release","token":token}),
            )
            .unwrap();
        assert!(!files.content(token).unwrap().1);
        let unsafe_file = temp.path().join("active.svg");
        fs::write(&unsafe_file, b"generated").unwrap();
        assert!(files
            .api(
                temp.path(),
                &json!({"operation":"preview","path":unsafe_file})
            )
            .is_err());
    }
    #[test]
    fn centered_paths_and_link_safety() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        fs::write(root.join("target"), "keep").unwrap();
        symlink(root.join("target"), root.join("link")).unwrap();
        remove(&root.join("link")).unwrap();
        assert!(root.join("target").exists());
        assert!(archive_name(Path::new("../escape")).is_err());
        assert!(archive_name(Path::new("/absolute")).is_err());
        assert!(archive_name(Path::new("a/b")).is_ok());
    }
    #[test]
    fn conflicts_and_no_silent_overwrite() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("test.txt");
        fs::write(&file, "original").unwrap();
        assert!(target(&file, "overwrite", false).is_err());
        assert_eq!(target(&file, "skip", false).unwrap(), None);
        assert_eq!(
            target(&file, "keep", false)
                .unwrap()
                .unwrap()
                .file_name()
                .unwrap(),
            "test (1).txt"
        );
        assert_eq!(fs::read_to_string(file).unwrap(), "original");
    }
    #[test]
    fn archive_rejects_links_before_touching_target() {
        let temp = tempfile::tempdir().unwrap();
        let zip = temp.path().join("bad.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&zip).unwrap());
        writer
            .start_file("../outside", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"bad").unwrap();
        writer.finish().unwrap();
        let dest = temp.path().join("destination");
        fs::create_dir(&dest).unwrap();
        let files = Files::default();
        assert!(files
            .archive_task(
                temp.path(),
                &json!({"operation":"extract","path":zip,"destination":dest}),
                Cancellation::default(),
                Arc::new(|_| {})
            )
            .is_err());
        assert!(!temp.path().join("outside").exists());
        assert_eq!(fs::read_dir(dest).unwrap().count(), 0);
    }
    #[test]
    fn utf8_editor_detects_binary_and_changes() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("hello.txt");
        fs::write(&file, "hello").unwrap();
        let original = read_text(&file).unwrap();
        fs::write(&file, "changed").unwrap();
        assert_ne!(original["revision"], read_text(&file).unwrap()["revision"]);
        fs::write(&file, [255, 0]).unwrap();
        assert!(read_text(&file).is_err());
        fs::write(&file, b"no nul but binary \x01").unwrap();
        assert!(read_text(&file).is_err());
    }
}
fn replace(src: &Path, dest: &Path) -> Result<()> {
    if fs::symlink_metadata(dest).is_err() {
        fs::rename(src, dest)?;
        return Ok(());
    }
    let backup = dest.with_file_name(format!(".framely-replaced-{}", token()));
    fs::rename(dest, &backup)?;
    if let Err(error) = fs::rename(src, dest) {
        fs::rename(&backup, dest).context("Cannot restore the previous file")?;
        return Err(error.into());
    }
    remove(&backup)?;
    Ok(())
}
struct ProgressReader<'a, R> {
    input: R,
    cancel: &'a Cancellation,
    progress: &'a dyn Fn(Value),
    done: &'a mut u64,
}
impl<R: Read> Read for ProgressReader<'_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.cancel.check().map_err(std::io::Error::other)?;
        let n = self.input.read(bytes)?;
        *self.done += n as u64;
        (self.progress)(json!({"phase":"working","bytes":*self.done}));
        Ok(n)
    }
}
fn write_tar<W: Write>(
    output: W,
    files: &[(PathBuf, PathBuf)],
    cancel: &Cancellation,
    progress: &dyn Fn(Value),
    done: &mut u64,
) -> Result<W> {
    let mut archive = tar::Builder::new(output);
    for (src, name) in files {
        cancel.check()?;
        let m = fs::metadata(src)?;
        let mut header = tar::Header::new_gnu();
        header.set_metadata(&m);
        header.set_mode(m.mode() & 0o777);
        header.set_cksum();
        if m.is_dir() {
            archive.append_data(&mut header, name, std::io::empty())?;
        } else {
            archive.append_data(
                &mut header,
                name,
                ProgressReader {
                    input: fs::File::open(src)?,
                    cancel,
                    progress,
                    done,
                },
            )?;
        }
    }
    archive.finish()?;
    Ok(archive.into_inner()?)
}
#[cfg(test)]
mod operation_tests {
    use super::*;
    fn progress() -> Arc<dyn Fn(Value) + Send + Sync> {
        Arc::new(|_| {})
    }
    #[test]
    fn zip_tar_and_gzip_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let source = home.join("目录 space");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("中文 space.txt"), "hello 世界\n").unwrap();
        fs::set_permissions(
            source.join("中文 space.txt"),
            fs::Permissions::from_mode(0o751),
        )
        .unwrap();
        let files = Files::default();
        for format in ["zip", "tar", "tar.gz"] {
            let output = home.join(format!("test.{}", format.to_uppercase()));
            files
                .archive_task(
                    home,
                    &json!({"operation":"compress","paths":[source],"path":output,"format":format}),
                    Cancellation::default(),
                    progress(),
                )
                .unwrap();
            let dest = home.join(format!("out-{format}"));
            fs::create_dir(&dest).unwrap();
            files
                .archive_task(
                    home,
                    &json!({"operation":"extract","path":output,"destination":dest}),
                    Cancellation::default(),
                    progress(),
                )
                .unwrap();
            assert_eq!(
                fs::read_to_string(dest.join("目录 space/中文 space.txt")).unwrap(),
                "hello 世界\n"
            );
            assert_eq!(
                fs::metadata(dest.join("目录 space/中文 space.txt"))
                    .unwrap()
                    .mode()
                    & 0o777,
                0o751
            );
        }
    }
    #[test]
    fn deeply_nested_folder_copy_keeps_transfer_buffers_off_the_stack() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&destination).unwrap();
        let mut relative = PathBuf::new();
        for _ in 0..48 {
            relative.push("nested");
            fs::create_dir(source.join(&relative)).unwrap();
        }
        relative.push("fixture.txt");
        fs::write(source.join(&relative), "generated data").unwrap();
        let value = Files::default()
            .task(
                root.path(),
                &json!({"operation":"copy","paths":[source],"destination":destination}),
                Cancellation::default(),
                progress(),
            )
            .unwrap();
        assert!(value["items"][0]["error"].is_null());
        assert_eq!(
            fs::read_to_string(destination.join("source").join(relative)).unwrap(),
            "generated data"
        );
    }
    #[test]
    fn cancellation_does_not_create_archive_or_replace_files() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("file");
        fs::write(&source, "content").unwrap();
        let output = temp.path().join("archive.zip");
        let cancellation = Cancellation::default();
        cancellation.stop();
        assert!(Files::default()
            .archive_task(
                temp.path(),
                &json!({"operation":"compress","paths":[source],"path":output}),
                cancellation,
                progress()
            )
            .is_err());
        assert!(!output.exists());
        assert!(source.exists());
    }
    #[test]
    fn copy_and_move_preserve_links_and_report_item_failures() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("src");
        let dest = temp.path().join("dest");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&dest).unwrap();
        fs::write(source.join("file"), "keep").unwrap();
        symlink(source.join("file"), source.join("link")).unwrap();
        let files = Files::default();
        let result=files.task(temp.path(),&json!({"operation":"copy","paths":[source.join("link"),source.join("missing")],"destination":dest}),Cancellation::default(),progress()).unwrap();
        assert!(fs::symlink_metadata(dest.join("link"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(result["items"][1]["error"].is_string());
        files
            .task(
                temp.path(),
                &json!({"operation":"move","paths":[source.join("file")],"destination":dest}),
                Cancellation::default(),
                progress(),
            )
            .unwrap();
        assert!(!source.join("file").exists());
        assert_eq!(fs::read_to_string(dest.join("file")).unwrap(), "keep");
    }
    #[test]
    fn trash_restore_expiry_only_removes_registered_items() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let original = home.join("important.txt");
        fs::write(&original, "restore").unwrap();
        trash(home, &original).unwrap();
        let items = read_trash(home).unwrap();
        assert_eq!(items.len(), 1);
        assert!(!original.exists());
        let item = checked_trash(home, &items[0].id).unwrap();
        fs::rename(&item.stored, &item.original).unwrap();
        fs::remove_file(trash_info(home).unwrap().join(format!("{}.json", item.id))).unwrap();
        assert_eq!(fs::read_to_string(&original).unwrap(), "restore");
        trash(home, &original).unwrap();
        let mut item = read_trash(home).unwrap().remove(0);
        item.deleted = now() - 31 * 86400;
        save_json(
            &trash_info(home).unwrap().join(format!("{}.json", item.id)),
            &serde_json::to_value(&item).unwrap(),
        )
        .unwrap();
        let unrelated = state(home).unwrap().join("trash/unregistered");
        fs::write(&unrelated, "keep").unwrap();
        Files::default().maintain(home).unwrap();
        assert!(!item.stored.exists());
        assert!(unrelated.exists());
    }
    #[test]
    fn upload_is_atomic_and_offsets_are_checked() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let file = home.join("target");
        fs::write(&file, "original").unwrap();
        let manager = Files::default();
        let id=manager.api(home,&json!({"operation":"upload.start","directory":home,"name":"target","size":6,"conflict":"overwrite"})).unwrap()["id"].as_str().unwrap().to_owned();
        assert!(manager.append(&id, 5, b"new").is_err());
        manager.append(&id, 3, b"est").unwrap();
        assert_eq!(manager.append(&id, 3, b"est").unwrap()["uploadedBytes"], 3);
        assert!(manager.append(&id, 3, b"bad").is_err());
        assert!(manager.append(&id, 2, b"over").is_err());
        assert!(manager.append(&id, u64::MAX, b"x").is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "original");
        assert!(manager
            .api(
                home,
                &json!({"operation":"upload.finish","id":id,"approve":true})
            )
            .is_err());
        manager.append(&id, 0, b"new").unwrap();
        manager
            .api(
                home,
                &json!({"operation":"upload.finish","id":id,"approve":true}),
            )
            .unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "newest");
        assert!(manager
            .api(
                home,
                &json!({"operation":"upload.start","directory":home,"name":"../escape","size":0})
            )
            .is_err());
    }
    #[test]
    fn concurrent_file_upload_and_cancellation_preserve_targets() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let manager = Arc::new(Files::default());
        let start = manager.api(home.path(), &json!({"operation":"upload.start","directory":home.path(),"name":"nested/file.bin","size":4 * crate::uploads::CHUNK,"conflict":"keep"})).unwrap();
        assert_eq!(start["chunkSize"], crate::uploads::CHUNK);
        let id = start["id"].as_str().unwrap().to_owned();
        let barrier = Arc::new(std::sync::Barrier::new(4));
        let workers: Vec<_> = (0..4)
            .map(|index| {
                let manager = manager.clone();
                let id = id.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    manager
                        .append(
                            &id,
                            (index * crate::uploads::CHUNK) as u64,
                            &vec![index as u8; crate::uploads::CHUNK],
                        )
                        .unwrap();
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let target = home.path().join("nested/file.bin");
        assert!(!target.exists());
        manager
            .api(home.path(), &json!({"operation":"upload.finish","id":id}))
            .unwrap();
        let bytes = fs::read(&target).unwrap();
        for index in 0..4 {
            assert!(
                bytes[index * crate::uploads::CHUNK..(index + 1) * crate::uploads::CHUNK]
                    .iter()
                    .all(|b| *b == index as u8)
            );
        }
        let start = manager.api(home.path(), &json!({"operation":"upload.start","directory":home.path(),"name":"nested/file.bin","size":6,"conflict":"overwrite"})).unwrap();
        let id = start["id"].as_str().unwrap();
        let temp = manager.uploads.lock().unwrap()[id].temp.clone();
        manager.append(id, 3, b"end").unwrap();
        manager
            .api(home.path(), &json!({"operation":"upload.cancel","id":id}))
            .unwrap();
        assert!(!temp.exists());
        assert!(manager.append(id, 0, b"new").is_err());
        assert_eq!(fs::read(target).unwrap(), bytes);
        let start = manager.api(home.path(), &json!({"operation":"upload.start","directory":home.path(),"name":"empty","size":0,"conflict":"keep"})).unwrap();
        manager
            .api(
                home.path(),
                &json!({"operation":"upload.finish","id":start["id"]}),
            )
            .unwrap();
        assert_eq!(fs::metadata(home.path().join("empty")).unwrap().len(), 0);
    }
    #[test]
    fn archive_link_and_absolute_entries_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let src = temp.path().join("links.tar");
        let mut archive = tar::Builder::new(fs::File::create(&src).unwrap());
        let mut h = tar::Header::new_gnu();
        h.set_entry_type(tar::EntryType::Symlink);
        h.set_size(0);
        h.set_mode(0o777);
        h.set_link_name("/tmp").unwrap();
        h.set_cksum();
        archive
            .append_data(&mut h, "link", std::io::empty())
            .unwrap();
        archive.finish().unwrap();
        let dest = temp.path().join("dest");
        fs::create_dir(&dest).unwrap();
        assert!(Files::default()
            .archive_task(
                temp.path(),
                &json!({"operation":"extract","path":src,"destination":dest}),
                Cancellation::default(),
                progress()
            )
            .is_err());
        assert_eq!(fs::read_dir(dest).unwrap().count(), 0);
    }
    #[test]
    fn folder_sort_and_global_view_preferences_persist_independently() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let a = home.path().join("a");
        let b = home.path().join("b");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        let manager = Files::default();
        let defaults = manager
            .api(home.path(), &json!({"operation":"view.preferences"}))
            .unwrap();
        assert_eq!(defaults, json!({"view":"list","folders":{}}));
        manager
            .api(
                home.path(),
                &json!({"operation":"view.preferences","path":a,"sort":"size","descending":true}),
            )
            .unwrap();
        manager.api(home.path(), &json!({"operation":"view.preferences","path":b,"sort":"modified","descending":false})).unwrap();
        manager
            .api(
                home.path(),
                &json!({"operation":"view.preferences","view":"icons-large"}),
            )
            .unwrap();
        // Canonical paths avoid separate preferences for aliases/trailing slashes.
        let alias = home.path().join("alias");
        symlink(&a, &alias).unwrap();
        manager.api(home.path(), &json!({"operation":"view.preferences","path":alias,"sort":"name","descending":false})).unwrap();
        let reloaded = Files::default()
            .api(home.path(), &json!({"operation":"view.preferences"}))
            .unwrap();
        assert_eq!(reloaded["view"], "icons-large");
        assert_eq!(reloaded["folders"].as_object().unwrap().len(), 2);
        assert_eq!(
            reloaded["folders"][a.to_str().unwrap()],
            json!({"sort":"name","descending":false})
        );
        assert_eq!(
            reloaded["folders"][b.to_str().unwrap()],
            json!({"sort":"modified","descending":false})
        );
        for bad in [
            json!({"view":"giant"}),
            json!({"path":a,"sort":"invalid","descending":false}),
            json!({"path":a,"sort":"size","descending":"true"}),
            json!({"path":"relative","sort":"name","descending":false}),
        ] {
            let mut p = bad;
            p["operation"] = json!("view.preferences");
            assert!(manager.api(home.path(), &p).is_err());
        }
        assert_eq!(
            manager
                .api(home.path(), &json!({"operation":"view.preferences"}))
                .unwrap(),
            reloaded
        );
        assert_eq!(
            manager
                .api(home.path(), &json!({"operation":"preferences"}))
                .unwrap(),
            json!([])
        );
    }
    #[test]
    fn bookmarks_survive_missing_directories_and_editor_requires_revision() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path();
        let manager = Files::default();
        let b = json!([{"name":"Offline","path":home.join("missing") }]);
        manager
            .api(home, &json!({"operation":"preferences","value":b}))
            .unwrap();
        assert_eq!(
            manager
                .api(home, &json!({"operation":"preferences"}))
                .unwrap(),
            b
        );
        let path = home.join("note.txt");
        fs::write(&path, "first").unwrap();
        let revision = read_text(&path).unwrap()["revision"].clone();
        fs::write(&path, "external").unwrap();
        assert!(manager
            .api(
                home,
                &json!({"operation":"write","path":path,"text":"new","revision":revision})
            )
            .is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "external");
    }
}
