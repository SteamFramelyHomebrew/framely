use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions, TryLockError},
    path::Path,
};

/// Keep this handle alive until the application exits. Never unlink the lock:
/// another launch may already hold an open handle to the same file.
pub struct InstanceGuard {
    _file: File,
}
impl InstanceGuard {
    pub fn acquire() -> Result<Option<Self>> {
        let user_dir = directories::BaseDirs::new().context("无法确定用户目录")?;
        let identity = hex::encode(Sha256::digest(
            user_dir.home_dir().as_os_str().to_string_lossy().as_bytes(),
        ));
        Self::at(&std::env::temp_dir().join(format!("framely-installer-{identity}.lock")))
    }
    fn at(path: &Path) -> Result<Option<Self>> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path).context("无法创建安装器实例锁")?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error).context("无法锁定安装器实例"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_second_instance_and_releases_on_exit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("instance.lock");
        let first = InstanceGuard::at(&path).unwrap().unwrap();
        assert!(InstanceGuard::at(&path).unwrap().is_none());
        drop(first);
        assert!(InstanceGuard::at(&path).unwrap().is_some());
    }
}
