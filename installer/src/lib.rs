pub mod discovery;
#[path = "../../src/http/github_fallback.rs"]
pub(crate) mod github_fallback;
pub mod instance;
pub mod maintenance;
pub mod progress;
pub mod proxy;
pub mod release;
pub mod remote;
pub const DEFAULT_REPO: &str = "SteamFramelyHomebrew/framely";
pub const BOOTSTRAP: &[u8] = include_bytes!("../../tools/bootstrap.py");
