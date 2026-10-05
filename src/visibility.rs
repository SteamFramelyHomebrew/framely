use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Visibility {
    pub known: bool,
    pub capture_obscured: bool,
    pub sequence: u64,
    pub session_id: String,
    pub views: BTreeMap<String, bool>,
}
pub struct Tracker {
    pub state: Visibility,
    seen: Option<Instant>,
}
impl Default for Tracker {
    fn default() -> Self {
        Self {
            state: Visibility {
                session_id: format!("{:032x}", rand::random::<u128>()),
                ..Default::default()
            },
            seen: None,
        }
    }
}
impl Tracker {
    pub fn report(&mut self, views: BTreeMap<String, bool>, now: Instant) -> Result<bool> {
        ensure!(
            views.len() <= 64
                && views.keys().all(|k| k.len() <= 256
                    && (k == "menu"
                        || k == "launcher"
                        || k == "framely.manager"
                        || k.starts_with("framely.window."))),
            "Invalid native visibility views"
        );
        self.seen = Some(now);
        let changed = !self.state.known || self.state.views != views;
        if changed {
            self.state = Visibility {
                known: true,
                capture_obscured: views.values().any(|v| *v),
                sequence: self.state.sequence + 1,
                session_id: self.state.session_id.clone(),
                views,
            };
        }
        Ok(changed)
    }
    pub fn expire(&mut self, now: Instant) -> bool {
        if self.state.known
            && self
                .seen
                .is_none_or(|t| now.duration_since(t) > Duration::from_secs(2))
        {
            self.state = Visibility {
                sequence: self.state.sequence + 1,
                session_id: self.state.session_id.clone(),
                ..Default::default()
            };
            self.seen = None;
            true
        } else {
            false
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aggregate_close_and_lease() {
        let now = Instant::now();
        let mut t = Tracker::default();
        assert!(!t.state.known);
        assert!(t
            .report(
                BTreeMap::from([("menu".into(), false), ("framely.window.x.y".into(), true)]),
                now
            )
            .unwrap());
        assert!(t.state.capture_obscured);
        assert!(!t
            .report(t.state.views.clone(), now + Duration::from_secs(1))
            .unwrap());
        assert!(!t.expire(now + Duration::from_secs(2)));
        assert!(t.expire(now + Duration::from_secs(4)));
        assert!(!t.state.known);
        assert!(!t.expire(now + Duration::from_secs(5)));
        assert!(t
            .report(BTreeMap::new(), now + Duration::from_secs(6))
            .unwrap());
        assert!(!t.state.capture_obscured);
        assert_eq!(t.state.sequence, 3);
    }
    #[test]
    fn reject_untrusted_view() {
        assert!(Tracker::default()
            .report(BTreeMap::from([("remote".into(), true)]), Instant::now())
            .is_err());
    }
}
