pub mod discovery;
pub mod instance;
pub mod maintenance;
pub mod release;
pub mod remote;
pub const DEFAULT_REPO: &str = "SteamFramelyHomebrew/framely";
pub const BOOTSTRAP: &[u8] = include_bytes!("../../tools/bootstrap.py");
