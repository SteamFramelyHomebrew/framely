//! Steam's current connected-host inventory; no account scraping or media reads.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex, OnceLock,
};
use std::time::{Duration, Instant};
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct RemoteApp {
    pub id: u32,
    pub name: String,
    pub client: String,
    #[serde(default, rename = "deviceName")]
    pub device_name: String,
}
fn query(request: serde_json::Value) -> Result<serde_json::Value> {
    crate::apk::steam_ui::evaluate(&format!("({})({request})", include_str!("remote.js")))
}
fn inventory(value: serde_json::Value) -> Result<Vec<RemoteApp>> {
    let rows: Vec<RemoteApp> = serde_json::from_value(value)?;
    ensure!(rows.len() <= 10000, "Remote library is too large");
    Ok(rows
        .into_iter()
        .filter(|a| {
            a.id > 0 && !a.name.trim().is_empty() && a.name.len() <= 1024 && valid_client(&a.client)
        })
        .collect())
}
fn valid_client(client: &str) -> bool {
    !client.is_empty()
        && client.len() <= 20
        && client.bytes().all(|b| b.is_ascii_digit())
        && client != "0"
}
static CACHE: OnceLock<Mutex<Option<(Instant, Vec<RemoteApp>)>>> = OnceLock::new();
static REFRESHING: AtomicBool = AtomicBool::new(false);
pub(super) fn cached() -> Vec<RemoteApp> {
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let snapshot = cache.lock().unwrap().clone();
    if snapshot
        .as_ref()
        .is_none_or(|(at, _)| at.elapsed() >= Duration::from_secs(3))
        && REFRESHING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        std::thread::spawn(|| {
            let rows = query(serde_json::json!({"mode":"list"}))
                .and_then(inventory)
                .unwrap_or_default();
            *CACHE.get().unwrap().lock().unwrap() = Some((Instant::now(), rows));
            REFRESHING.store(false, Ordering::Release);
        });
    }
    snapshot
        .filter(|(at, _)| at.elapsed() < Duration::from_secs(15))
        .map(|(_, v)| v)
        .unwrap_or_default()
}
pub(super) fn launch(id: u32, client: &str) -> Result<()> {
    ensure!(id > 0 && valid_client(client), "Invalid remote Steam game");
    ensure!(
        query(serde_json::json!({"mode":"launch","id":id,"client":client}))? == true,
        "Remote game or host is no longer available"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_remote_entries_and_clients_are_rejected() {
        let rows=inventory(serde_json::json!([{ "id":1,"name":"Game","client":"123"},{"id":0,"name":"Invalid","client":"12"},{"id":2,"name":"Unsafe","client":"1);evil()"}])).unwrap();
        assert_eq!(rows.len(), 1);
        assert!(launch(42, "0").is_err());
        assert!(launch(42, r#"1"2"#).is_err());
    }
}
