use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    os::unix::fs::FileExt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
pub const CHUNK: usize = 16 * 1024 * 1024;
#[derive(Clone, Default)]
pub struct Uploads(Arc<Mutex<Option<Upload>>>);
struct Upload {
    id: String,
    package: crate::package::Staged,
    file: File,
    total: u64,
    received: u64,
    ranges: BTreeMap<u64, u64>,
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
        self.start_in(total, std::path::Path::new("/tmp"))
    }
    pub fn start_in(&self, total: u64, parent: &std::path::Path) -> Result<Value> {
        ensure!(total > 0, "Empty package");
        let mut slot = self.0.lock().unwrap();
        Self::expire(&mut slot);
        ensure!(slot.is_none(), "An upload is already running");
        let id = hex::encode(rand::random::<[u8; 24]>());
        let package = crate::package::Staged::create_in(parent)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&package.path)?;
        *slot = Some(Upload {
            id: id.clone(),
            package,
            file,
            total,
            received: 0,
            ranges: BTreeMap::new(),
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
        ensure!(
            offset <= u.total && bytes.len() as u64 <= u.total - offset,
            "Upload exceeds declared size"
        );
        let end = offset + bytes.len() as u64;
        if u.ranges.get(&offset) == Some(&end) {
            // A lost acknowledgement may be retried, but cannot change accepted bytes.
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
            // Serialize writes with cancellation so an aborted upload cannot keep writing.
            if let Err(error) = u.file.write_all_at(bytes, offset) {
                *slot = None;
                return Err(error.into());
            }
            u.ranges.insert(offset, end);
            u.received += bytes.len() as u64;
        }
        u.touched = Instant::now();
        // received acknowledges this chunk's end (also compatible with sequential clients).
        Ok(json!({"received":end,"uploadedBytes":u.received,"total":u.total}))
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
        assert!(uploads.append(&id, 3, b"a").is_err());
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
    fn out_of_order_chunks_retries_and_holes() {
        let uploads = Uploads::default();
        let start = uploads.start(9).unwrap();
        let id = start["upload"].as_str().unwrap();
        let ack = uploads.append(id, 6, b"ghi").unwrap();
        assert_eq!(ack["received"], 9);
        assert_eq!(ack["uploadedBytes"], 3);
        assert!(uploads.take(id).is_err());
        assert_eq!(uploads.append(id, 6, b"ghi").unwrap()["uploadedBytes"], 3);
        assert!(uploads.append(id, 6, b"xxx").is_err());
        assert!(uploads.append(id, 5, b"fg").is_err());
        assert!(uploads.append(id, u64::MAX, b"x").is_err());
        uploads.append(id, 0, b"abc").unwrap();
        assert!(uploads.append(id, 1, b"b").is_err());
        assert!(uploads.append(id, 0, b"ab").is_err());
        assert!(uploads.take(id).is_err());
        uploads.append(id, 3, b"def").unwrap();
        let staged = uploads.take(id).unwrap();
        assert_eq!(fs::read(&staged.path).unwrap(), b"abcdefghi");
        assert_eq!(staged.hash, crate::package::digest(b"abcdefghi"));
    }
    #[test]
    fn simultaneous_chunks_produce_a_complete_file() {
        let uploads = Uploads::default();
        let start = uploads.start(4 * CHUNK as u64).unwrap();
        let id = start["upload"].as_str().unwrap().to_owned();
        let barrier = Arc::new(std::sync::Barrier::new(4));
        let workers: Vec<_> = (0..4)
            .map(|index| {
                let uploads = uploads.clone();
                let id = id.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    uploads
                        .append(&id, (index * CHUNK) as u64, &vec![index as u8; CHUNK])
                        .unwrap();
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let staged = uploads.take(&id).unwrap();
        let bytes = fs::read(&staged.path).unwrap();
        for index in 0..4 {
            assert!(bytes[index * CHUNK..(index + 1) * CHUNK]
                .iter()
                .all(|b| *b == index as u8));
        }
    }
    #[test]
    fn persistent_uploads_are_private_and_removed_on_cancel_or_drop() {
        let parent = tempfile::tempdir().unwrap();
        let uploads = Uploads::default();
        let started = uploads.start_in(3, parent.path()).unwrap();
        let id = started["upload"].as_str().unwrap();
        uploads.append(id, 0, b"apk").unwrap();
        let staged = uploads.take(id).unwrap();
        assert!(staged.path.starts_with(parent.path()));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&staged.path).unwrap().permissions().mode() & 0o777,
            0o400
        );
        let path = staged.path.clone();
        drop(staged);
        assert!(!path.exists());
        let started = uploads.start_in(1, parent.path()).unwrap();
        uploads.abort(started["upload"].as_str().unwrap()).unwrap();
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
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
