use anyhow::{ensure, Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, ffi::CString, fs, io::Read, os::unix::fs::{MetadataExt, FileTypeExt, symlink, PermissionsExt}, path::{Path, PathBuf}, process::Command};

pub fn bytes(path: &Path) -> Result<u64> {
    if !path.exists() { return Ok(0); }
    let root = if fs::symlink_metadata(path)?.file_type().is_symlink() { path.canonicalize()? } else { path.to_path_buf() };
    let mut total = 0u64;
    fn walk(path: &Path, total: &mut u64) -> Result<()> {
        let m = fs::symlink_metadata(path)?;
        if m.is_file() { *total = total.saturating_add(m.len()); }
        if m.is_dir() && !m.file_type().is_symlink() {
            for entry in fs::read_dir(path)? { walk(&entry?.path(), total)?; }
        }
        Ok(())
    }
    walk(&root, &mut total)?;
    Ok(total)
}

fn inventory(path: &Path) -> Result<BTreeMap<PathBuf, String>> {
    let mut files = BTreeMap::new();
    fn walk(root: &Path, rel: &Path, files: &mut BTreeMap<PathBuf, String>) -> Result<()> {
        let path = root.join(rel);
        let meta = fs::symlink_metadata(&path)?;
        let value = if meta.file_type().is_symlink() {
            format!("link:{}:{}:{}", fs::read_link(&path)?.display(), meta.uid(), meta.gid())
        } else if meta.is_dir() {
            for entry in fs::read_dir(&path)? { walk(root, &rel.join(entry?.file_name()), files)?; }
            if rel.as_os_str().is_empty() { "directory".to_owned() }
            else { format!("directory:{}:{}:{}", meta.uid(), meta.gid(), meta.mode() & 0o7777) }
        } else if meta.is_file() {
            let mut file = fs::File::open(path)?;
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 65536];
            loop { let n = file.read(&mut buffer)?; if n == 0 { break; } hash.update(&buffer[..n]); }
            format!("sha256:{}:{}:{}:{}", hex::encode(hash.finalize()), meta.uid(), meta.gid(), meta.mode() & 0o7777)
        } else if meta.file_type().is_char_device() && meta.rdev() == 0 {
            "overlay-whiteout".to_owned()
        } else { anyhow::bail!("存储中含有无法迁移的文件：{}", path.display()); };
        files.insert(rel.to_path_buf(), value);
        Ok(())
    }
    walk(path, Path::new(""), &mut files)?;
    Ok(files)
}
pub fn verified_equal(source: &Path, target: &Path) -> Result<bool> {
    Ok(source.is_dir() && target.is_dir() && inventory(source)? == inventory(target)?)
}
fn selected_source(source: &Path, name: &str, follow_root_links: bool) -> Result<PathBuf> {
    let path = source.join(name);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(path),
        Err(error) => return Err(error.into()),
    };
    ensure!(follow_root_links || !meta.file_type().is_symlink(), "存储目录不能是链接：{}", path.display());
    let resolved = if meta.file_type().is_symlink() { path.canonicalize()? } else { path };
    ensure!(resolved.is_dir(), "存储项目不是目录：{}", resolved.display());
    Ok(resolved)
}
pub fn verified_selected(source: &Path, target: &Path, names: &[String], follow_root_links: bool) -> Result<bool> {
    if !target.is_dir() { return Ok(false); }
    let mut expected = std::collections::BTreeSet::new();
    for name in names {
        if selected_source(source, name, follow_root_links)?.is_dir() { expected.insert(name.clone()); }
    }
    let actual: std::collections::BTreeSet<String> = fs::read_dir(target)?.map(|entry| entry.map(|e|e.file_name().to_string_lossy().to_string())).collect::<std::io::Result<_>>()?;
    if actual != expected { return Ok(false); }
    for name in expected {
        if !verified_equal(&selected_source(source, &name, follow_root_links)?, &target.join(name))? { return Ok(false); }
    }
    Ok(true)
}
pub fn write_journal(path: &Path, value: &Value) -> Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, serde_json::to_vec(value)?)?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    fs::File::open(&temporary)?.sync_all()?;
    fs::rename(temporary, path)?;
    fs::File::open(path.parent().context("缺少存储状态目录")?)?.sync_all()?;
    Ok(())
}

pub fn validate_target(source: &Path, target: &Path, allow_root: bool) -> Result<PathBuf> {
    ensure!(target.is_absolute() && target != Path::new("/"), "请选择绝对目录路径");
    let parent = target.parent().context("缺少目标目录的上级目录")?.canonicalize()?;
    ensure!(parent.is_dir(), "目标目录的上级目录不存在");
    let target = parent.join(target.file_name().context("无效目录名称")?);
    ensure!(!target.starts_with("/tmp") && !target.starts_with("/proc") && !target.starts_with("/sys") && !target.starts_with("/dev") && !target.starts_with("/run/framely"), "不能在临时目录或系统目录保存数据");
    ensure!(allow_root || target.starts_with("/home") || target.starts_with("/run/media"), "请选择用户目录或已挂载存储");
    let source = source.canonicalize().unwrap_or_else(|_| source.to_path_buf());
    ensure!(source != target && !source.starts_with(&target) && !target.starts_with(&source), "目标目录不能位于原目录内部或包含原目录");
    if let Ok(meta) = fs::symlink_metadata(&target) {
        ensure!(meta.is_dir() && !meta.file_type().is_symlink(), "目标必须是空目录，不能是链接");
        ensure!(fs::read_dir(&target)?.next().is_none(), "目标目录必须为空");
    }
    Ok(target)
}
fn prepare_space(parent: &Path, needed: u64) -> Result<()> {
    let c_path = CString::new(parent.as_os_str().as_encoded_bytes())?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    ensure!(unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } == 0, "无法检查目标存储空间");
    let available = stat.f_bavail.saturating_mul(stat.f_frsize);
    ensure!(available >= needed.saturating_add(128 * 1024 * 1024), "目标存储空间不足");
    let probe = parent.join(format!(".framely-probe-{:016x}", rand::random::<u64>()));
    fs::create_dir(&probe)?;
    let result = (|| -> Result<()> {
        let file = probe.join("file");
        fs::write(&file, b"framely")?;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600))?;
        ensure!(fs::metadata(&file)?.permissions().mode() & 0o777 == 0o600, "目标文件系统不支持 Linux 文件权限");
        fs::hard_link(&file, probe.join("hardlink"))?;
        symlink("file", probe.join("symlink"))?;
        ensure!(fs::read_link(probe.join("symlink"))? == Path::new("file"), "目标文件系统不支持符号链接");
        Ok(())
    })();
    let _ = fs::remove_dir_all(probe);
    result
}

pub fn copy_verified(source: &Path, target: &Path, mut progress: impl FnMut(&str)) -> Result<()> {
    let target = validate_target(source, target, false)?;
    let parent = target.parent().unwrap();
    prepare_space(parent, bytes(source)?)?;
    let stage = parent.join(format!(".framely-storage-{:016x}", rand::random::<u64>()));
    fs::create_dir(&stage)?;
    let result = (|| -> Result<()> {
        progress("copying");
        if source.exists() {
            let output = Command::new("cp").arg("-a").arg("--reflink=auto")
                .arg("--").arg(source.join(".")).arg(&stage).output()?;
            ensure!(output.status.success(), "复制存储失败：{}", String::from_utf8_lossy(&output.stderr));
        }
        progress("verifying");
        let source_files = if source.exists() { inventory(source)? } else { BTreeMap::new() };
        let target_files = inventory(&stage)?;
        ensure!(source_files == target_files || (!source.exists() && target_files.len() == 1), "迁移后的文件校验失败，仍使用原位置");
        progress("switching");
        if target.exists() { fs::remove_dir(&target)?; }
        fs::rename(&stage, &target)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_dir_all(&stage); }
    result
}

pub fn copy_selected(source: &Path, target: &Path, names: &[String], follow_root_links: bool, mut progress: impl FnMut(&str)) -> Result<()> {
    let target = validate_target(source, target, false)?;
    let parent = target.parent().unwrap();
    let mut needed = 0u64;
    for name in names { needed = needed.saturating_add(bytes(&selected_source(source, name, follow_root_links)?)?); }
    prepare_space(parent, needed)?;
    let stage = parent.join(format!(".framely-storage-{:016x}", rand::random::<u64>()));
    fs::create_dir(&stage)?;
    let result = (|| -> Result<()> {
        progress("copying");
        for name in names {
            ensure!(!name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\\'), "无效容器名称");
            let from = selected_source(source, name, follow_root_links)?;
            if !from.exists() { continue; }
            let output = Command::new("cp").args(["-a", "--reflink=auto", "--"])
                .arg(&from).arg(stage.join(name)).output()?;
            ensure!(output.status.success(), "复制容器失败：{}", String::from_utf8_lossy(&output.stderr));
        }
        progress("verifying");
        for name in names {
            let from = selected_source(source, name, follow_root_links)?;
            if from.exists() { ensure!(inventory(&from)? == inventory(&stage.join(name))?, "容器迁移校验失败：{name}"); }
        }
        progress("switching");
        if target.exists() { fs::remove_dir(&target)?; }
        fs::rename(&stage, &target)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_dir_all(&stage); }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copy_and_verify_preserve_data_and_reject_unsafe_targets() {
        let temp = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let source = temp.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("saved.apk"), b"apk data").unwrap();
        symlink("saved.apk", source.join("current")).unwrap();
        let target = temp.path().join("moved");
        let mut steps = Vec::new();
        copy_verified(&source, &target, |step| steps.push(step.to_owned())).unwrap();
        assert_eq!(steps, ["copying", "verifying", "switching"]);
        assert_eq!(fs::read(target.join("saved.apk")).unwrap(), b"apk data");
        assert_eq!(fs::read_link(target.join("current")).unwrap(), Path::new("saved.apk"));
        assert_eq!(fs::read(source.join("saved.apk")).unwrap(), b"apk data");
        assert!(validate_target(&source, &source.join("inside"), false).is_err());
        assert!(copy_verified(&source, &target, |_| {}).is_err());
        assert!(prepare_space(temp.path(), u64::MAX).is_err());
        assert_eq!(fs::read(source.join("saved.apk")).unwrap(), b"apk data");
    }
    #[test]
    fn selected_copy_excludes_external_containers() {
        let temp = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let source = temp.path().join("contexts");
        fs::create_dir(&source).unwrap();
        for name in ["managed", "external"] {
            fs::create_dir(source.join(name)).unwrap();
            fs::write(source.join(name).join("state"), name).unwrap();
        }
        let target = temp.path().join("android");
        copy_selected(&source, &target, &["managed".into()], false, |_| {}).unwrap();
        assert_eq!(fs::read_to_string(target.join("managed/state")).unwrap(), "managed");
        assert!(!target.join("external").exists());
        assert!(source.join("external/state").is_file());
    }
    #[test]
    fn plugin_roots_copy_real_data_instead_of_storage_links() {
        let temp = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let source = temp.path().join("state");
        let physical = temp.path().join("persistent-plugins");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&physical).unwrap();
        fs::write(physical.join("manifest.json"), b"plugin").unwrap();
        symlink(&physical, source.join("plugins")).unwrap();
        let target = temp.path().join("moved");
        assert!(copy_selected(&source,&target,&["plugins".into()],false,|_|{}).is_err());
        copy_selected(&source,&target,&["plugins".into()],true,|_|{}).unwrap();
        assert!(target.join("plugins").is_dir());
        assert!(!fs::symlink_metadata(target.join("plugins")).unwrap().file_type().is_symlink());
        assert_eq!(fs::read(target.join("plugins/manifest.json")).unwrap(),b"plugin");
        assert!(verified_selected(&source,&target,&["plugins".into()],true).unwrap());
    }
}
