//! Built-in casting. Settings live in the core database; all media workers
//! run as the Steam session user, apart from the framebuffer export helper.
pub mod settings;
pub mod runtime;

pub mod receiver;
