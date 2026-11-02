//! **The write rate** — a token bucket per principal: `rate` writes a second
//! sustained, `burst` at once after a pause. The owner's free tier (V2-31):
//! about 20 writes a second sustained. The server's call layer
//! (`src/calls.rs`) asks it before every write; a call past it is answered
//! `too_many_writes` with how long to wait. Pure, with the clock given, so
//! its tests need no sleep.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The owner's sustained rate: 20 writes a second.
pub const WRITES_PER_SECOND: f64 = 20.0;
/// What a principal may spend at once after a pause: two seconds' worth.
pub const WRITE_BURST: f64 = 40.0;
/// Buckets idle this long are full again and forgotten.
const FORGET_AFTER: Duration = Duration::from_secs(600);

/// Every principal's bucket.
#[derive(Debug)]
pub struct Buckets {
    rate: f64,
    burst: f64,
    held: Mutex<HashMap<String, (f64, Instant)>>,
}

impl Buckets {
    /// `rate` tokens a second, at most `burst` held. A rate of 0 lets every call by.
    pub fn new(rate: f64, burst: f64) -> Self {
        Self { rate: rate.max(0.0), burst: burst.max(1.0), held: Mutex::new(HashMap::new()) }
    }

    /// Whether `principal` may write now (`Ok`, a token spent), or how long
    /// until it may (`Err`).
    pub fn take(&self, principal: &str, now: Instant) -> Result<(), Duration> {
        if self.rate <= 0.0 {
            return Ok(());
        }
        let mut held = self.held.lock().unwrap_or_else(|p| p.into_inner());
        if held.len() > 4096 {
            held.retain(|_, (_, at)| now.saturating_duration_since(*at) < FORGET_AFTER);
        }
        let (tokens, at) = held.entry(principal.to_string()).or_insert((self.burst, now));
        let refill = now.saturating_duration_since(*at).as_secs_f64() * self.rate;
        *tokens = (*tokens + refill).min(self.burst);
        *at = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            Ok(())
        } else {
            Err(Duration::from_secs_f64((1.0 - *tokens) / self.rate))
        }
    }

    /// Its sustained rate, writes a second.
    pub fn rate(&self) -> f64 {
        self.rate
    }
}

impl Default for Buckets {
    fn default() -> Self {
        Self::new(WRITES_PER_SECOND, WRITE_BURST)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_then_the_sustained_rate() {
        let b = Buckets::new(20.0, 40.0);
        let t0 = Instant::now();
        for i in 0..40 {
            assert!(b.take("key:a", t0).is_ok(), "write {i} of the burst");
        }
        let wait = b.take("key:a", t0).expect_err("the 41st at once waits");
        assert!(wait <= Duration::from_millis(50) && wait > Duration::ZERO, "{wait:?}");
        // Another principal has its own bucket.
        assert!(b.take("key:b", t0).is_ok());
        // Sustained: over 10 seconds, 20 a second pass and no more.
        let mut passed = 0;
        for ms in (0..10_000).step_by(10) {
            if b.take("key:a", t0 + Duration::from_millis(ms + 10)).is_ok() {
                passed += 1;
            }
        }
        assert!((199..=201).contains(&passed), "{passed} in 10 s at 20/s");
    }

    #[test]
    fn a_pause_fills_it_again_never_past_the_burst() {
        let b = Buckets::new(20.0, 40.0);
        let t0 = Instant::now();
        for _ in 0..40 {
            b.take("p", t0).unwrap();
        }
        let later = t0 + Duration::from_secs(3600);
        let n = (0..100).filter(|_| b.take("p", later).is_ok()).count();
        assert_eq!(n, 40, "the burst, not an hour's worth");
    }

    #[test]
    fn rate_zero_lets_everything_by() {
        let b = Buckets::new(0.0, 1.0);
        let t0 = Instant::now();
        assert!((0..10_000).all(|_| b.take("p", t0).is_ok()));
    }
}
