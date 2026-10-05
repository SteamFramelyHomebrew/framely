use crate::model::*;
use anyhow::{ensure, Context, Result};
use base64::engine::general_purpose::STANDARD;
#[cfg(test)]
use base64::Engine;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Read, Seek, SeekFrom, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

/// Owns a private on-disk package until all review/install users release it.
#[derive(Debug)]
pub struct Staged {
    directory: PathBuf,
    pub path: PathBuf,
    pub hash: String,
}
impl Drop for Staged {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
impl Staged {
    pub fn create() -> Result<Self> {
        Self::create_in(Path::new("/tmp"))
    }
    /// Session-local staging; privileged IPC continues to require /tmp snapshots.
    pub fn create_in(parent: &Path) -> Result<Self> {
        let directory = parent.join(format!(
            "framely-package-{}",
            hex::encode(rand::random::<[u8; 24]>())
        ));
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let staged = Self {
            path: directory.join("package"),
            directory,
            hash: String::new(),
        };
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staged.path)?;
        Ok(staged)
    }
    pub fn seal(mut self) -> Result<Arc<Self>> {
        let mut file = fs::File::open(&self.path)?;
        self.hash = digest_reader(&mut file)?;
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o400))?;
        Ok(Arc::new(self))
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Arc<Self>> {
        let staged = Self::create()?;
        fs::write(&staged.path, bytes)?;
        staged.seal()
    }
    pub fn request(&self) -> serde_json::Value {
        serde_json::json!({"packagePath":self.path,"sha256":self.hash})
    }
    pub fn manifest(&self) -> Result<Manifest> {
        verify_manifest(fs::File::open(&self.path)?)
    }
}
pub fn digest_reader(reader: &mut impl Read) -> Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(hex::encode(hash.finalize()))
}
// Only private package snapshots from the authorized session user or root can
// cross the privileged IPC boundary. Never follow user-controlled symlinks.
pub fn open_staged(request: &serde_json::Value, manager: u32) -> Result<fs::File> {
    let path = Path::new(
        request["packagePath"]
            .as_str()
            .context("Missing package path")?,
    );
    let directory = path.parent().context("Invalid package path")?;
    let name = directory
        .file_name()
        .and_then(|s| s.to_str())
        .context("Invalid package directory")?;
    let token = name
        .strip_prefix("framely-package-")
        .context("Invalid package directory")?;
    ensure!(
        directory.parent() == Some(Path::new("/tmp"))
            && path.file_name() == Some(std::ffi::OsStr::new("package"))
            && token.len() == 48
            && token.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid staged package path"
    );
    let meta = fs::symlink_metadata(directory)?;
    ensure!(
        meta.is_dir() && (meta.uid() == manager || meta.uid() == 0) && meta.mode() & 0o077 == 0,
        "Untrusted package directory"
    );
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let data = file.metadata()?;
    ensure!(
        data.is_file() && data.uid() == meta.uid() && data.mode() & 0o077 == 0,
        "Untrusted package file"
    );
    ensure!(
        digest_reader(&mut file)? == request["sha256"].as_str().context("Missing package hash")?,
        "Package changed since review"
    );
    file.seek(SeekFrom::Start(0))?;
    Ok(file)
}
pub fn request_bytes(
    request: &serde_json::Value,
    proxy: &ProxySettings,
    manager: u32,
) -> Result<Vec<u8>> {
    if request.get("packagePath").is_some() {
        let mut bytes = Vec::new();
        open_staged(request, manager)?.read_to_end(&mut bytes)?;
        Ok(bytes)
    } else {
        let staged = stage_request_proxy(request, proxy, |_, _| Ok(()))?;
        Ok(fs::read(&staged.path)?)
    }
}
/// Verify a disk archive with bounded buffers; keep only its small manifest.
pub fn verify_manifest(reader: impl Read + Seek) -> Result<Manifest> {
    let mut zip = ZipArchive::new(reader).context("Invalid plugin archive")?;
    ensure!(zip.len() <= 2049, "Too many archive entries");
    let manifest: Manifest = {
        let mut entry = zip
            .by_name("manifest.json")
            .context("Missing manifest.json")?;
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(256 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(bytes.len() <= 256 * 1024, "Manifest too large");
        serde_json::from_slice(&bytes)?
    };

    manifest.validate()?;
    let mut names = std::collections::BTreeSet::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name().to_owned();
        safe_path(&name)?;
        ensure!(!entry.is_dir(), "Directory entries are not supported");
        ensure!(
            entry.unix_mode().unwrap_or(0) & 0o170000 != 0o120000,
            "Symlinks are not allowed"
        );
        ensure!(names.insert(name.clone()), "Duplicate archive entry");
        if name == "manifest.json" {
            continue;
        }
        let expected = manifest.files.get(&name).context("Unlisted payload file")?;
        if manifest.icon.as_deref() == Some(name.as_str()) {
            ensure!(
                entry.size() >= 24 && entry.size() <= 1024 * 1024,
                "Invalid PNG icon"
            );
            let mut header = [0u8; 24];
            entry.read_exact(&mut header)?;
            ensure!(
                header.starts_with(b"\x89PNG\r\n\x1a\n") && &header[12..16] == b"IHDR",
                "Invalid PNG icon"
            );
            let w = u32::from_be_bytes(header[16..20].try_into()?);
            let h = u32::from_be_bytes(header[20..24].try_into()?);
            ensure!(
                w > 0 && h > 0 && w <= 1024 && h <= 1024,
                "Icon dimensions exceed 1024px"
            );
            // Hash the header too; entry is already advanced by 24 bytes.
            let mut chain = Cursor::new(header).chain(entry);
            ensure!(
                digest_reader(&mut chain)? == *expected,
                "File hash mismatch: {name}"
            );
        } else {
            ensure!(
                digest_reader(&mut entry)? == *expected,
                "File hash mismatch: {name}"
            );
        }
    }
    ensure!(
        names.len() == manifest.files.len() + 1,
        "Unlisted/missing payload files"
    );
    Ok(manifest)
}
pub fn stage_request_proxy(
    v: &serde_json::Value,
    proxy: &ProxySettings,
    mut progress: impl FnMut(u64, Option<u64>) -> Result<()>,
) -> Result<Arc<Staged>> {
    if let Some(encoded) = v.get("package").and_then(|v| v.as_str()) {
        let staged = Staged::create()?;
        let mut file = fs::OpenOptions::new().write(true).open(&staged.path)?;
        let mut reader = base64::read::DecoderReader::new(encoded.as_bytes(), &STANDARD);
        let received = std::io::copy(&mut reader, &mut file)?;
        file.sync_all()?;
        progress(received, Some(received))?;
        return staged.seal();
    }
    progress(0, None)?;
    let url = v["url"].as_str().context("Missing package/URL")?;
    validate_url(url, v["allowHttp"].as_bool().unwrap_or(false))?;
    let response = crate::http::get_with_proxy(
        url,
        v["allowHttp"].as_bool().unwrap_or(false),
        std::time::Duration::from_secs(60),
        &[],
        proxy,
    )
    .context("Package download failed")?;
    let total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    let staged = Staged::create()?;
    let mut file = fs::OpenOptions::new().write(true).open(&staged.path)?;
    let mut reader = response.into_reader();
    let mut buffer = [0u8; 65536];
    let mut received = 0;
    progress(0, total)?;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])?;
        received += n as u64;
        progress(received, total)?;
    }
    ensure!(total.is_none_or(|n| n == received), "Truncated download");
    file.sync_all()?;
    let staged = staged.seal()?;
    if let Some(expected) = v["sha256"].as_str() {
        ensure!(staged.hash == expected, "Download SHA256 mismatch");
    }
    Ok(staged)
}
pub struct Verified {
    pub manifest: Manifest,
    pub files: BTreeMap<String, Vec<u8>>,
}
pub fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub fn verify(bytes: &[u8]) -> Result<Verified> {
    let mut zip = ZipArchive::new(Cursor::new(bytes)).context("Invalid plugin archive")?;
    ensure!(zip.len() <= 2049, "Too many archive entries");
    let mut raw = BTreeMap::new();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let name = f.name().to_owned();
        safe_path(&name)?;
        ensure!(!f.is_dir(), "Directory entries are not supported");
        ensure!(
            f.unix_mode().unwrap_or(0) & 0o170000 != 0o120000,
            "Symlinks are not allowed"
        );
        let mut data = Vec::new();
        f.read_to_end(&mut data)?;
        ensure!(data.len() as u64 == f.size(), "Truncated archive entry");
        ensure!(raw.insert(name, data).is_none(), "Duplicate archive entry");
    }
    let manifest_bytes = raw
        .remove("manifest.json")
        .context("Missing manifest.json")?;
    ensure!(manifest_bytes.len() <= 256 * 1024, "Manifest too large");
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;

    manifest.validate()?;
    ensure!(
        raw.len() == manifest.files.len(),
        "Unlisted/missing payload files"
    );
    for (path, hash) in &manifest.files {
        ensure!(
            digest(raw.get(path).context("Missing payload file")?) == *hash,
            "File hash mismatch: {path}"
        );
    }
    if let Some(icon) = &manifest.icon {
        let image = raw.get(icon).context("Missing icon")?;
        ensure!(
            image.len() >= 24
                && image.len() <= 1024 * 1024
                && image.starts_with(b"\x89PNG\r\n\x1a\n")
                && &image[12..16] == b"IHDR",
            "Invalid PNG icon"
        );
        let w = u32::from_be_bytes(image[16..20].try_into()?);
        let h = u32::from_be_bytes(image[20..24].try_into()?);
        ensure!(
            w > 0 && h > 0 && w <= 1024 && h <= 1024,
            "Icon dimensions exceed 1024px"
        );
    }
    Ok(Verified {
        manifest,
        files: raw,
    })
}
pub fn unpack(v: &Verified, root: &Path) -> Result<()> {
    fs::create_dir(root)?;
    for (path, bytes) in &v.files {
        let p = root.join(path);
        fs::create_dir_all(p.parent().unwrap())?;
        fs::write(&p, bytes)?;
        let mode = if v.manifest.backend.as_ref().map(|b| b.entry.as_str()) == Some(path.as_str())
            || v.manifest.lifecycle.as_ref().is_some_and(|l| {
                [
                    &l.on_install,
                    &l.on_update,
                    &l.on_uninstall,
                    &l.on_crash_cleanup,
                ]
                .into_iter()
                .flatten()
                .any(|h| h.entry == *path)
            }) {
            0o755
        } else {
            0o644
        };
        fs::set_permissions(p, fs::Permissions::from_mode(mode))?;
    }
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&v.manifest)?,
    )?;
    Ok(())
}
pub fn pack(manifest_path: &Path, payload: &Path, out: &Path) -> Result<()> {
    let mut m: Manifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
    let mut files = BTreeMap::new();
    fn collect(base: &Path, p: &Path, files: &mut BTreeMap<String, Vec<u8>>) -> Result<()> {
        for e in fs::read_dir(p)? {
            let e = e?;
            let meta = fs::symlink_metadata(e.path())?;
            ensure!(!meta.file_type().is_symlink(), "Payload symlink rejected");
            if meta.is_dir() {
                collect(base, &e.path(), files)?
            } else {
                ensure!(meta.is_file(), "Unsupported payload file");
                let rel = e
                    .path()
                    .strip_prefix(base)?
                    .to_str()
                    .context("Non UTF8 path")?
                    .to_owned();
                safe_path(&rel)?;
                ensure!(rel != "manifest.json", "Reserved payload name");
                files.insert(rel, fs::read(e.path())?);
            }
        }
        Ok(())
    }
    collect(payload, payload, &mut files)?;
    m.files = files.iter().map(|(p, b)| (p.clone(), digest(b))).collect();

    m.validate()?;
    let raw = serde_json::to_vec(&m)?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    zip.start_file("manifest.json", options)?;
    zip.write_all(&raw)?;
    for (p, b) in files {
        zip.start_file(p, options)?;
        zip.write_all(&b)?;
    }
    let bytes = zip.finish()?.into_inner();
    verify(&bytes)?;
    fs::write(out, &bytes)?;
    println!("{} {}", digest(&bytes), out.display());
    Ok(())
}
#[cfg(test)]
pub fn from_request(v: &serde_json::Value) -> Result<Vec<u8>> {
    from_request_progress(v, |_, _| Ok(()))
}
#[cfg(test)]
pub fn from_request_progress(
    v: &serde_json::Value,
    progress: impl FnMut(u64, Option<u64>) -> Result<()>,
) -> Result<Vec<u8>> {
    from_request_proxy(v, &Default::default(), progress)
}
#[cfg(test)]
pub fn from_request_proxy(
    v: &serde_json::Value,
    proxy: &ProxySettings,
    mut progress: impl FnMut(u64, Option<u64>) -> Result<()>,
) -> Result<Vec<u8>> {
    if let Some(s) = v.get("package").and_then(|v| v.as_str()) {
        let bytes = STANDARD.decode(s)?;
        progress(bytes.len() as u64, Some(bytes.len() as u64))?;
        return Ok(bytes);
    }
    progress(0, None)?;
    let url = v["url"].as_str().context("Missing package/URL")?;
    validate_url(url, v["allowHttp"].as_bool().unwrap_or(false))?;
    let response = crate::http::get_with_proxy(
        url,
        v["allowHttp"].as_bool().unwrap_or(false),
        std::time::Duration::from_secs(60),
        &[],
        proxy,
    )
    .context("Package download failed")?;
    let total = response
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok());
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 65536];
    progress(0, total)?;
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..n]);
        progress(bytes.len() as u64, total)?;
    }
    ensure!(
        total.is_none_or(|t| t == bytes.len() as u64),
        "Truncated download"
    );
    if let Some(expected) = v["sha256"].as_str() {
        ensure!(digest(&bytes) == expected, "Download SHA256 mismatch");
    }
    Ok(bytes)
}
