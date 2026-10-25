//! Built-in casting. Settings live in the core database; all media workers
//! run as the Steam session user, apart from the framebuffer export helper.
pub mod settings;
pub mod runtime;
mod recording;
pub mod screenshot;
pub mod region;
#[cfg(test)]
mod region_device_test;

pub mod receiver;
pub mod watch_access;

// Preserve one bounded failure tail before a later start truncates the live log.
fn retain_failed_log(directory: &std::path::Path, name: &str) {
    use std::io::{Read, Seek, SeekFrom};
    let result = (|| -> std::io::Result<()> {
        let mut source = std::fs::File::open(directory.join(format!("{name}.log")))?;
        let size = source.metadata()?.len();
        source.seek(SeekFrom::Start(size.saturating_sub(256 * 1024)))?;
        let mut tail = Vec::new();
        source.take(256 * 1024).read_to_end(&mut tail)?;
        if !tail.is_empty() {
            std::fs::write(directory.join(format!("{name}.failed.log")), tail)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!("Cannot retain casting {name} failure log: {error}");
        }
    }
}
