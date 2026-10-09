//! Descriptor-relative upload writes: never follow nested destination links.
use super::*;
use std::{
    ffi::CString,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::ffi::OsStrExt,
    },
};
fn name(p: &Path) -> Result<CString> {
    Ok(CString::new(p.as_os_str().as_bytes())?)
}
fn open_directory(fd: i32, part: &Path) -> Result<fs::File> {
    let part = name(part)?;
    let next = unsafe {
        libc::openat(
            fd,
            part.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if next < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { fs::File::from_raw_fd(next) })
}
pub(super) fn relative(value: &str) -> Result<&Path> {
    let p = Path::new(value);
    ensure!(
        !value.is_empty()
            && !p.is_absolute()
            && p.components().all(|c| matches!(c, Component::Normal(_))),
        "Invalid upload path"
    );
    for c in p.components() {
        valid_name(c.as_os_str().to_str().context("Invalid upload filename")?)?;
    }
    Ok(p)
}
pub(super) struct Anchor {
    pub root: PathBuf,
    pub directory: PathBuf,
    root_fd: fs::File,
    pub fd: fs::File,
}
impl Anchor {
    pub fn root(p: &Value) -> Result<Self> {
        let requested = PathBuf::from(strparam(p, "directory")?);
        ensure!(requested.is_absolute(), "Use an absolute file path");
        let root = requested.canonicalize()?;
        let fd = open_directory(libc::AT_FDCWD, &root)?;
        let m = fd.metadata()?;
        if let Some(expected) = p.get("rootIdentity") {
            ensure!(
                expected["dev"].as_u64() == Some(m.dev())
                    && expected["ino"].as_u64() == Some(m.ino()),
                "Upload destination changed; confirm its path again"
            );
        }
        Ok(Self {
            directory: root.clone(),
            root,
            root_fd: fd.try_clone()?,
            fd,
        })
    }
    pub fn identity(&self) -> Result<Value> {
        let m = self.root_fd.metadata()?;
        Ok(json!({"dev":m.dev(),"ino":m.ino()}))
    }
    pub fn descend(mut self, relative: &Path, create: bool) -> Result<Self> {
        for c in relative.components() {
            let part = Path::new(c.as_os_str());
            match open_directory(self.fd.as_raw_fd(), part) {
                Ok(next) => self.fd = next,
                Err(e)
                    if create
                        && e.downcast_ref::<std::io::Error>()
                            .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                {
                    let n = name(part)?;
                    let r = unsafe { libc::mkdirat(self.fd.as_raw_fd(), n.as_ptr(), 0o755) };
                    if r < 0
                        && std::io::Error::last_os_error().kind()
                            != std::io::ErrorKind::AlreadyExists
                    {
                        return Err(std::io::Error::last_os_error().into());
                    }
                    self.fd = open_directory(self.fd.as_raw_fd(), part)?;
                }
                Err(e) => {
                    return Err(e.context("Upload directory is unavailable or is a symbolic link"))
                }
            }
            self.directory.push(part);
        }
        self.check()?;
        Ok(self)
    }
    pub fn check(&self) -> Result<()> {
        let root = open_directory(libc::AT_FDCWD, &self.root)?;
        let a = root.metadata()?;
        let b = self.root_fd.metadata()?;
        ensure!(
            a.dev() == b.dev() && a.ino() == b.ino(),
            "Upload destination moved or changed"
        );
        let mut fd = root;
        for c in self.directory.strip_prefix(&self.root)?.components() {
            fd = open_directory(fd.as_raw_fd(), Path::new(c.as_os_str()))?;
        }
        let a = fd.metadata()?;
        let b = self.fd.metadata()?;
        ensure!(
            a.dev() == b.dev() && a.ino() == b.ino(),
            "Upload destination moved or changed"
        );
        Ok(())
    }
    pub fn metadata(&self, leaf: &Path) -> Result<Option<libc::stat>> {
        let mut st = std::mem::MaybeUninit::uninit();
        let leaf = name(leaf)?;
        let r = unsafe {
            libc::fstatat(
                self.fd.as_raw_fd(),
                leaf.as_ptr(),
                st.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if r == 0 {
            return Ok(Some(unsafe { st.assume_init() }));
        }
        let e = std::io::Error::last_os_error();
        if e.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(e.into())
        }
    }
    pub fn file_conflict(&self, leaf: &Path) -> Result<bool> {
        if let Some(st) = self.metadata(leaf)? {
            ensure!(
                matches!(st.st_mode & libc::S_IFMT, libc::S_IFREG | libc::S_IFLNK),
                "Upload file conflicts with a directory or special file"
            );
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn temporary(&self, leaf: &Path) -> Result<fs::File> {
        let leaf = name(leaf)?;
        let fd = unsafe {
            libc::openat(
                self.fd.as_raw_fd(),
                leaf.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(unsafe { fs::File::from_raw_fd(fd) })
    }
    pub fn unlink(&self, leaf: &Path) {
        if let Ok(n) = name(leaf) {
            unsafe {
                libc::unlinkat(self.fd.as_raw_fd(), n.as_ptr(), 0);
            }
        }
    }
    pub fn finish(
        &self,
        temp: &Path,
        leaf: &Path,
        policy: &str,
        approve: bool,
    ) -> Result<Option<PathBuf>> {
        self.check()?;
        if policy == "overwrite" {
            ensure!(approve, "Confirm overwriting files");
        }
        for i in 0..10000 {
            let candidate = if i == 0 {
                leaf.to_path_buf()
            } else {
                let stem = leaf.file_stem().unwrap_or_default().to_string_lossy();
                let ext = leaf
                    .extension()
                    .map(|v| format!(".{}", v.to_string_lossy()))
                    .unwrap_or_default();
                PathBuf::from(format!("{stem} ({i}){ext}"))
            };
            let exists = self.file_conflict(&candidate)?;
            if exists && policy == "skip" {
                return Ok(None);
            }
            if exists && policy == "keep" {
                continue;
            }
            ensure!(
                !exists || policy == "overwrite",
                "A file with that name already exists"
            );
            let src = name(temp)?;
            let dest = name(&candidate)?;
            let r = unsafe {
                libc::renameat2(
                    self.fd.as_raw_fd(),
                    src.as_ptr(),
                    self.fd.as_raw_fd(),
                    dest.as_ptr(),
                    if policy == "overwrite" {
                        0
                    } else {
                        libc::RENAME_NOREPLACE
                    },
                )
            };
            if r == 0 {
                return Ok(Some(self.directory.join(candidate)));
            }
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                if policy == "skip" {
                    return Ok(None);
                }
                if policy == "keep" {
                    continue;
                }
            }
            return Err(e.into());
        }
        bail!("Too many filename conflicts")
    }
}
