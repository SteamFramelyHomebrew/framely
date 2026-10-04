use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub struct Verifier {
    state: Arc<Mutex<Limits>>,
}
#[derive(Default)]
struct Limits {
    active: usize,
    global: Vec<Instant>,
    peers: BTreeMap<String, Vec<Instant>>,
}
struct Permit(Verifier);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().active -= 1;
    }
}
impl Verifier {
    #[cfg(test)]
    pub(crate) fn active(&self) -> usize {
        self.state.lock().unwrap().active
    }
    fn acquire(&self, peer: &str, now: Instant) -> Result<Permit> {
        let mut state = self.state.lock().unwrap();
        state
            .global
            .retain(|t| now.duration_since(*t) < Duration::from_secs(60));
        state.peers.retain(|_, times| {
            times.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
            !times.is_empty()
        });
        ensure!(
            state.active < 2
                && state.global.len() < 12
                && state.peers.get(peer).is_none_or(|v| v.len() < 5),
            "Login rate limit exceeded"
        );
        state.global.push(now);
        state.peers.entry(peer.to_owned()).or_default().push(now);
        state.active += 1;
        Ok(Permit(self.clone()))
    }
    pub fn verify(&self, root: &Path, params: &Value) -> Result<Value> {
        let password = params["password"].as_str().context("Missing password")?;
        ensure!(password.len() <= 512, "密码过长");
        let _permit = self.acquire(params["peer"].as_str().unwrap_or("local"), Instant::now())?;
        let path = root.join("network-password.json");
        let snapshot = fs::read(&path)?;
        let saved: Value = serde_json::from_slice(&snapshot)?;
        let salt = hex::decode(saved["salt"].as_str().context("Missing salt")?)?;
        let expected = hex::decode(saved["hash"].as_str().context("Missing hash")?)?;
        ensure!(
            salt.len() == 16 && expected.len() == 32,
            "Invalid password record"
        );
        let mut actual = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), &salt, 600_000, &mut actual);
        let matches = actual
            .iter()
            .zip(&expected)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0;
        // A password changed during verification must not authenticate with the old value.
        Ok(serde_json::json!(matches && fs::read(path)? == snapshot))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_limits_bound_concurrency_peers_and_reset() {
        let v = Verifier::default();
        let now = Instant::now();
        let a = v.acquire("a", now).unwrap();
        let b = v.acquire("b", now).unwrap();
        assert!(v.acquire("c", now).is_err());
        drop(a);
        drop(b);
        for _ in 0..4 {
            drop(v.acquire("a", now).unwrap());
        }
        assert!(v.acquire("a", now).is_err());
        for _ in 0..4 {
            drop(v.acquire("b", now).unwrap());
        }
        assert!(v.acquire("b", now).is_err());
        drop(v.acquire("c", now).unwrap());
        drop(v.acquire("c", now).unwrap());
        assert!(v.acquire("d", now).is_err());
        drop(v.acquire("a", now + Duration::from_secs(61)).unwrap());
    }
}
