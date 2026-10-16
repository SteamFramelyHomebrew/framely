//! Roll back only a directory atomically reserved by this installation.
use super::*;

pub(super) struct NewContext {
    parent: PathBuf,
    identity: (u64, u64),
}
impl NewContext {
    pub(super) fn reserve(home: &Path, db: &Database, c: &Container) -> Result<Self> {
        cleanup::ensure_runtime_absent(c)?;
        let base = contexts_root(home);
        let ancestor = base
            .ancestors()
            .find(|p| p.exists())
            .context("Missing context storage ancestor")?;
        ensure!(
            fs::canonicalize(ancestor)? == ancestor,
            "Container storage contains a link"
        );
        fs::create_dir_all(&base)?;
        cleanup::validate_location(home, db, c)?;
        let parent = c
            .baked
            .parent()
            .context("Missing context parent")?
            .to_path_buf();
        // create_dir, not create_dir_all: an existing/replaced context is never
        // owned by this attempt, regardless of its size or missing database.
        fs::create_dir(&parent)
            .with_context(|| format!("Cannot reserve new container {}", parent.display()))?;
        let metadata = fs::symlink_metadata(&parent)?;
        Ok(Self {
            parent,
            identity: (metadata.dev(), metadata.ino()),
        })
    }
    pub(super) fn rollback(
        self,
        home: &Path,
        db: &mut Database,
        c: &Container,
        log: &Path,
    ) -> Result<()> {
        let metadata = fs::symlink_metadata(&self.parent)?;
        ensure!(
            metadata.is_dir()
                && !metadata.file_type().is_symlink()
                && (metadata.dev(), metadata.ino()) == self.identity,
            "New container directory changed; rollback refused"
        );
        cleanup::delete_failed_install(home, db, c, log)
    }
}
