//! Persistent, watch-only capability; deliberately separate from cast settings.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::Path};

#[derive(Default, Serialize, Deserialize)]
pub struct Grant {
    pub token: Option<String>,
}
impl Grant {
    pub fn load(root: &Path) -> Result<Self> {
        let bytes = match fs::read(root.join("cast-watch-access.json")) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        let grant: Self = serde_json::from_slice(&bytes)?;
        ensure!(grant.token.as_ref().is_none_or(|t| t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit())), "Invalid watch access record");
        Ok(grant)
    }
    pub fn save(root: &Path, enabled: bool) -> Result<Self> {
        let mut grant = Self::load(root)?;
        if enabled && grant.token.is_none() {
            grant.token = Some(crate::session::cast_random_key());
        } else if !enabled {
            grant.token = None;
        }
        let temporary = root.join("cast-watch-access.json.tmp");
        let mut file = fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(&temporary)?;
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        let result = (|| -> Result<()> {
            file.write_all(&serde_json::to_vec(&grant)?)?;
            file.sync_all()?;
            fs::rename(&temporary, root.join("cast-watch-access.json"))?;
            fs::File::open(root)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() { let _ = fs::remove_file(&temporary); }
        result?;
        Ok(grant)
    }
}

#[derive(Default)]
pub struct Access {
    pub token: Option<String>,
    pub sessions: std::collections::BTreeMap<String, String>,
    pub revoked: Vec<String>,
}
impl Access {
    pub fn accepts(&self, candidate: &str) -> bool {
        self.token.as_deref().is_some_and(|token| {
            token.len() == candidate.len() && token.bytes().zip(candidate.bytes()).fold(0u8, |diff, (a,b)| diff | (a ^ b)) == 0
        })
    }
    pub fn replace(&mut self, token: Option<String>) {
        if self.token != token {
            self.revoked.extend(std::mem::take(&mut self.sessions).into_values());
            self.token = token;
        }
    }
    pub fn path(&self) -> String {
        match &self.token {
            Some(token) => format!("/cast/watch?token={token}"),
            None => "/cast/watch".into(),
        }
    }
    pub fn disconnect_revoked(&mut self) {
        let client = ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(2)).build();
        self.revoked.retain(|target| {
            match client.delete(target).set("If-Match", "*").call() {
                Ok(_) | Err(ureq::Error::Status(404 | 410, _)) => false,
                Err(_) => true,
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn watch_access_persists_rotates_and_preserves_on_failure() {
        let root = tempfile::tempdir().unwrap();
        assert!(Grant::load(root.path()).unwrap().token.is_none());
        let first = Grant::save(root.path(), true).unwrap().token.unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(Grant::load(root.path()).unwrap().token.as_ref(), Some(&first));
        assert_eq!(Grant::save(root.path(), true).unwrap().token.as_ref(), Some(&first));
        assert_eq!(fs::metadata(root.path().join("cast-watch-access.json")).unwrap().permissions().mode() & 0o777, 0o600);
        fs::create_dir(root.path().join("cast-watch-access.json.tmp")).unwrap();
        assert!(Grant::save(root.path(), false).is_err());
        assert_eq!(Grant::load(root.path()).unwrap().token.as_ref(), Some(&first));
        fs::remove_dir(root.path().join("cast-watch-access.json.tmp")).unwrap();
        assert!(Grant::save(root.path(), false).unwrap().token.is_none());
        assert_ne!(Grant::save(root.path(), true).unwrap().token.unwrap(), first);
    }
}
