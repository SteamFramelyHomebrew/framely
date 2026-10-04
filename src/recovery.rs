use std::time::{Duration, Instant};
/// A healthy minute resets the burst budget. Three consecutive short runs quarantine.
#[derive(Default)]
pub struct Recovery {
    pub failures: u32,
    pub started: Option<Instant>,
    pub next: Option<Instant>,
}
impl Recovery {
    pub fn started(&mut self, now: Instant) {
        self.started = Some(now);
        self.next = None;
    }
    #[cfg(test)]
    pub fn failed(&mut self, now: Instant) -> bool {
        self.failed_with_limit(now, 3)
    }
    pub fn failed_with_limit(&mut self, now: Instant, limit: u32) -> bool {
        if self
            .started
            .is_some_and(|t| now.duration_since(t) >= Duration::from_secs(60))
        {
            self.failures = 0;
        }
        self.started = None;
        self.failures += 1;
        self.next = Some(now + Duration::from_secs(1 << (self.failures.min(6) - 1)));
        self.failures >= limit
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crash_bursts_back_off_and_healthy_runs_reset() {
        let now = Instant::now();
        let mut r = Recovery::default();
        r.started(now);
        assert!(!r.failed(now));
        assert_eq!(r.next, Some(now + Duration::from_secs(1)));
        r.started(now);
        assert!(!r.failed(now));
        assert_eq!(r.next, Some(now + Duration::from_secs(2)));
        r.started(now);
        assert!(r.failed(now));
        r.started(now);
        assert!(!r.failed(now + Duration::from_secs(61)));
        assert_eq!(r.failures, 1);
    }
}
