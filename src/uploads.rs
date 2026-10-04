use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
pub const CHUNK: usize = 512 * 1024;
#[derive(Clone, Default)]
pub struct Uploads(Arc<Mutex<Option<Upload>>>);
struct Upload {
    id: String,
    package: crate::package::Staged,
    file: File,
    total: u64,
    received: u64,
    touched: Instant,
}
impl Uploads {
    fn expire(slot: &mut Option<Upload>) {
        if slot
            .as_ref()
            .is_some_and(|u| u.touched.elapsed() > Duration::from_secs(900))
        {
            *slot = None;
        }
    }
    pub fn start(&self, total: u64) -> Result<Value> {
        ensure!(total > 0, "Empty package");
        let mut slot = self.0.lock().unwrap();
        Self::expire(&mut slot);
        ensure!(slot.is_none(), "An upload is already running");
        let id = hex::encode(rand::random::<[u8; 24]>());
        let package = crate::package::Staged::create()?;
        let file = OpenOptions::new().write(true).open(&package.path)?;
        *slot = Some(Upload {
            id: id.clone(),
            package,
            file,
            total,
            received: 0,
            touched: Instant::now(),
        });
        Ok(json!({"upload":id,"chunkSize":CHUNK}))
    }
    pub fn append(&self, id: &str, offset: u64, bytes: &[u8]) -> Result<Value> {
        ensure!(
            !bytes.is_empty() && bytes.len() <= CHUNK,
            "Invalid upload chunk size"
        );
        let mut slot = self.0.lock().unwrap();
        Self::expire(&mut slot);
        let u = slot.as_mut().context("Upload expired")?;
        ensure!(u.id == id, "Unknown upload");
        ensure!(offset == u.received, "Unexpected upload offset");
        ensure!(
            bytes.len() as u64 <= u.total - u.received,
            "Upload exceeds declared size"
        );
        // A failed write invalidates the partial file rather than allowing retry at a stale offset.
        if let Err(error) = u.file.write_all(bytes) {
            *slot = None;
            return Err(error.into());
        }
        u.received += bytes.len() as u64;
        u.touched = Instant::now();
        Ok(json!({"received":u.received,"total":u.total}))
    }
    pub fn abort(&self, id: &str) -> Result<Value> {
        let mut slot = self.0.lock().unwrap();
        if slot.as_ref().is_some_and(|u| u.id == id) {
            *slot = None;
        }
        Ok(json!({"cancelled":true}))
    }
    pub fn cleanup(&self) {
        Self::expire(&mut self.0.lock().unwrap());
    }
    pub fn take(&self, id: &str) -> Result<Arc<crate::package::Staged>> {
        let mut slot = self.0.lock().unwrap();
        Self::expire(&mut slot);
        let u = slot.as_ref().context("Upload expired")?;
        ensure!(u.id == id, "Unknown upload");
        ensure!(u.received == u.total, "Incomplete upload");
        let u = slot.take().unwrap();
        u.file.sync_all()?;
        u.package.seal()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn ordered_complete_upload_and_cleanup() {
        let uploads = Uploads::default();
        let id = uploads.start(3).unwrap()["upload"]
            .as_str()
            .unwrap()
            .to_owned();
        let directory = uploads
            .0
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .package
            .path
            .parent()
            .unwrap()
            .to_owned();
        assert!(uploads.start(1).is_err());
        assert!(uploads.append("wrong", 0, b"a").is_err());
        assert!(uploads.append(&id, 1, b"a").is_err());
        assert!(uploads.append(&id, 0, b"abcd").is_err());
        uploads.append(&id, 0, b"ab").unwrap();
        assert!(uploads.take(&id).is_err());
        uploads.append(&id, 2, b"c").unwrap();
        let package = uploads.take(&id).unwrap();
        assert_eq!(fs::read(&package.path).unwrap(), b"abc");
        assert!(directory.exists());
        drop(package);
        assert!(!directory.exists());
        assert!(uploads.take(&id).is_err());
    }
    #[test]
    fn cancellation_expiry_and_chunk_bound() {
        let uploads = Uploads::default();
        let id = uploads.start((CHUNK + 1) as u64).unwrap()["upload"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(uploads.append(&id, 0, &vec![0; CHUNK + 1]).is_err());
        uploads.abort(&id).unwrap();
        assert!(uploads.append(&id, 0, b"x").is_err());
        uploads.start(1).unwrap();
        let directory = {
            let mut s = uploads.0.lock().unwrap();
            let u = s.as_mut().unwrap();
            u.touched = Instant::now() - Duration::from_secs(901);
            u.package.path.parent().unwrap().to_owned()
        };
        uploads.cleanup();
        assert!(!directory.exists());
        uploads.start(1).unwrap();
    }
}
