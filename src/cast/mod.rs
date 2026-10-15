//! Built-in casting. Settings live in the core database; all media workers
//! run as the Steam session user, apart from the framebuffer export helper.
pub mod settings;
pub mod runtime;
mod recording;
pub mod region;
#[cfg(test)]
mod region_device_test;

pub mod receiver;
pub mod watch_access;
