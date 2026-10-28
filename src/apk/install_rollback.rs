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

// A caught failure is not an interrupted transaction. Restore the record before
// submission, or reconcile the actual package after an uncertain submission.
pub(super) fn reconcile_failed(
    home: &Path,
    db: &mut Database,
    c: &Container,
    app_id: &str,
    original: Option<Record>,
    submitted: bool,
    stopped: bool,
    error: &anyhow::Error,
) -> Result<()> {
    if !submitted {
        if let Some(record) = original {
            db.records.insert(app_id.into(), record);
        } else {
            db.records.remove(app_id);
        }
    } else {
        let mut actual = c.clone();
        actual.running = !stopped;
        match installed(home, &actual) {
            Ok(packages) => {
                if let Some(record) = db.records.get_mut(app_id) {
                    if let Some((metadata, _)) = packages
                        .into_iter()
                        .find(|(m, _)| m.package == record.metadata.package)
                    {
                        record.metadata = metadata;
                        record.removed = false;
                    } else {
                        record.removed = true;
                    }
                    record.pending = None;
                }
            }
            Err(_) => {
                if let Some(record) = db.records.get_mut(app_id) {
                    record.pending = Some(format!("Installation failed; actual package state could not be verified; reconcile before changing the application: {error:#}"));
                }
            }
        }
    }
    save(home, db)
}
