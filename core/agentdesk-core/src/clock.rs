//! Injected time source so escalation, re-scoring and the simulator can be
//! tested in compressed time.

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// Manually advanced clock. Cloning shares the same underlying time.
#[derive(Debug, Clone)]
pub struct VirtualClock(Arc<Mutex<DateTime<Utc>>>);

impl VirtualClock {
    pub fn new(start: DateTime<Utc>) -> Self {
        VirtualClock(Arc::new(Mutex::new(start)))
    }

    /// A fixed, recognisable epoch for tests and golden files.
    pub fn at_epoch() -> Self {
        Self::new("2026-01-01T00:00:00Z".parse().expect("valid epoch"))
    }

    pub fn advance(&self, by: Duration) {
        *self.0.lock().expect("clock poisoned") += by;
    }

    pub fn set(&self, to: DateTime<Utc>) {
        *self.0.lock().expect("clock poisoned") = to;
    }
}

impl Clock for VirtualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().expect("clock poisoned")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_clock_advances_and_is_shared() {
        let a = VirtualClock::at_epoch();
        let b = a.clone();
        a.advance(Duration::seconds(90));
        assert_eq!(b.now() - a.now(), Duration::zero());
        assert_eq!(b.now().to_rfc3339(), "2026-01-01T00:01:30+00:00");
    }
}
