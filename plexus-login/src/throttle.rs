//! AUTH-22: throttles for the self-hosted sign-in — codes sent, codes tried
//! and TOTP codes tried, each per address (or per user) and per client IP,
//! with backoff.
//!
//! Two shapes:
//!
//! - a **rate** ([`Rate`]): at most `max` events in a sliding `window`
//!   (codes sent: per address, per IP);
//! - a **backoff** ([`Backoff`]): failures are free up to `free`, then each
//!   further failure locks the key for `base · 2^(n − free)` seconds, capped
//!   at `cap`; failures older than `window` are forgotten, and a success
//!   clears the key (codes tried, TOTP tried).
//!
//! Every table is bounded ([`MAX_KEYS`]): past it, the key whose newest
//! event is oldest makes room, so a flood of fresh keys costs memory only up
//! to the bound. Keys are hashed with a per-process salt, so the tables hold
//! no address.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use crate::sessions::now;

/// The most keys one table holds.
pub const MAX_KEYS: usize = 50_000;

/// What a throttle says: go on, or wait this many seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Wait(i64),
}

impl Verdict {
    pub fn allowed(self) -> bool {
        self == Verdict::Allow
    }

    /// The longer of two waits.
    pub fn and(self, other: Verdict) -> Verdict {
        match (self, other) {
            (Verdict::Wait(a), Verdict::Wait(b)) => Verdict::Wait(a.max(b)),
            (w @ Verdict::Wait(_), _) | (_, w @ Verdict::Wait(_)) => w,
            _ => Verdict::Allow,
        }
    }
}

/// At most `max` events per `window` seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    pub max: usize,
    pub window: i64,
}

/// `free` failures in `window` seconds, then a lock that doubles from
/// `base` seconds up to `cap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    pub free: u32,
    pub base: i64,
    pub cap: i64,
    pub window: i64,
}

/// The sign-in's limits. [`Limits::default`] is what a deployment gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Codes sent to one address (WorkOS's own limit is 3 a minute).
    pub send_per_address: Rate,
    /// Codes sent, and SSO lookups, from one client IP.
    pub send_per_ip: Rate,
    /// Wrong codes for one address.
    pub try_per_address: Backoff,
    /// Wrong codes (email and TOTP) from one client IP.
    pub try_per_ip: Backoff,
    /// Wrong TOTP codes for one user.
    pub totp_per_user: Backoff,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            send_per_address: Rate { max: 3, window: 600 },
            send_per_ip: Rate { max: 20, window: 600 },
            try_per_address: Backoff { free: 5, base: 30, cap: 900, window: 3600 },
            try_per_ip: Backoff { free: 20, base: 30, cap: 900, window: 3600 },
            totp_per_user: Backoff { free: 5, base: 30, cap: 900, window: 3600 },
        }
    }
}

#[derive(Default)]
struct Entry {
    /// Event times, oldest first (rates), or failure times (backoffs).
    times: Vec<i64>,
    locked_until: i64,
}

impl Entry {
    fn newest(&self) -> i64 {
        self.times.last().copied().unwrap_or(self.locked_until)
    }
}

/// One table of keys.
struct Table {
    entries: Mutex<HashMap<[u8; 32], Entry>>,
}

impl Table {
    fn new() -> Self {
        Self { entries: Mutex::new(HashMap::new()) }
    }

    fn with<R>(&self, key: [u8; 32], f: impl FnOnce(&mut Entry) -> R) -> R {
        let mut map = self.entries.lock().unwrap();
        if !map.contains_key(&key) && map.len() >= MAX_KEYS {
            if let Some(oldest) = map.iter().min_by_key(|(_, e)| e.newest()).map(|(k, _)| *k) {
                map.remove(&oldest);
            }
        }
        f(map.entry(key).or_default())
    }

    /// Counts an event against a rate, if it fits.
    fn take(&self, key: [u8; 32], rate: Rate) -> Verdict {
        let t = now();
        self.with(key, |e| {
            e.times.retain(|&at| t - at < rate.window);
            if e.times.len() >= rate.max {
                let wait = e.times[0] + rate.window - t;
                return Verdict::Wait(wait.max(1));
            }
            e.times.push(t);
            Verdict::Allow
        })
    }

    /// Whether a backoff's key is locked now.
    fn check(&self, key: [u8; 32]) -> Verdict {
        let t = now();
        let map = self.entries.lock().unwrap();
        match map.get(&key) {
            Some(e) if e.locked_until > t => Verdict::Wait(e.locked_until - t),
            _ => Verdict::Allow,
        }
    }

    /// A failure against a backoff: the lock it now carries.
    fn fail(&self, key: [u8; 32], b: Backoff) -> Verdict {
        let t = now();
        self.with(key, |e| {
            e.times.retain(|&at| t - at < b.window);
            e.times.push(t);
            let n = e.times.len() as u32;
            if n > b.free {
                let lock = b.base.saturating_mul(1i64 << (n - b.free - 1).min(20)).min(b.cap);
                e.locked_until = e.locked_until.max(t + lock);
                return Verdict::Wait(e.locked_until - t);
            }
            Verdict::Allow
        })
    }

    fn clear(&self, key: [u8; 32]) {
        self.entries.lock().unwrap().remove(&key);
    }
}

/// Every throttle the self-hosted sign-in keeps. See the module docs.
pub struct Throttles {
    pub limits: Limits,
    salt: [u8; 32],
    sent_address: Table,
    sent_ip: Table,
    tried_address: Table,
    tried_ip: Table,
    totp_user: Table,
}

impl Throttles {
    pub fn new(limits: Limits) -> Self {
        let mut salt = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut salt);
        Self {
            limits,
            salt,
            sent_address: Table::new(),
            sent_ip: Table::new(),
            tried_address: Table::new(),
            tried_ip: Table::new(),
            totp_user: Table::new(),
        }
    }

    fn key(&self, kind: &str, value: &str) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.salt);
        h.update(kind.as_bytes());
        h.update([0]);
        h.update(value.as_bytes());
        h.finalize().into()
    }

    fn ip(ip: Option<IpAddr>) -> String {
        ip.map(|i| i.to_string()).unwrap_or_else(|| "unknown".into())
    }

    /// A code (or an SSO lookup) for `address` from `ip`: counted against
    /// both, or the wait. Nothing is counted when either refuses.
    pub fn send(&self, address: &str, ip: Option<IpAddr>) -> Verdict {
        let by_ip = self.sent_ip.take(self.key("ip", &Self::ip(ip)), self.limits.send_per_ip);
        if !by_ip.allowed() {
            return by_ip;
        }
        self.sent_address.take(self.key("addr", address), self.limits.send_per_address)
    }

    /// Whether a code for `address` may be tried from `ip` now.
    pub fn may_try(&self, address: &str, ip: Option<IpAddr>) -> Verdict {
        self.tried_address.check(self.key("addr", address)).and(self.tried_ip.check(self.key("ip", &Self::ip(ip))))
    }

    /// A wrong code for `address` from `ip`.
    pub fn tried_wrong(&self, address: &str, ip: Option<IpAddr>) -> Verdict {
        let a = self.tried_address.fail(self.key("addr", address), self.limits.try_per_address);
        a.and(self.tried_ip.fail(self.key("ip", &Self::ip(ip)), self.limits.try_per_ip))
    }

    /// A right code for `address`: its failures forgotten (the IP's stay).
    pub fn tried_right(&self, address: &str) {
        self.tried_address.clear(self.key("addr", address));
    }

    /// Whether a TOTP code for `user` may be tried from `ip` now.
    pub fn may_try_totp(&self, user: &str, ip: Option<IpAddr>) -> Verdict {
        self.totp_user.check(self.key("user", user)).and(self.tried_ip.check(self.key("ip", &Self::ip(ip))))
    }

    /// A wrong TOTP code for `user` from `ip`.
    pub fn totp_wrong(&self, user: &str, ip: Option<IpAddr>) -> Verdict {
        let u = self.totp_user.fail(self.key("user", user), self.limits.totp_per_user);
        u.and(self.tried_ip.fail(self.key("ip", &Self::ip(ip)), self.limits.try_per_ip))
    }

    /// A right TOTP code for `user`.
    pub fn totp_right(&self, user: &str) {
        self.totp_user.clear(self.key("user", user));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Option<IpAddr> {
        Some(s.parse().unwrap())
    }

    #[test]
    fn sends_are_limited_per_address_and_per_ip() {
        let t = Throttles::new(Limits { send_per_address: Rate { max: 2, window: 60 }, send_per_ip: Rate { max: 3, window: 60 }, ..Limits::default() });
        assert!(t.send("a@x", ip("1.1.1.1")).allowed());
        assert!(t.send("a@x", ip("2.2.2.2")).allowed());
        assert!(matches!(t.send("a@x", ip("3.3.3.3")), Verdict::Wait(w) if w > 0 && w <= 60), "per address, whatever the IP");
        assert!(t.send("b@x", ip("1.1.1.1")).allowed());
        assert!(t.send("c@x", ip("1.1.1.1")).allowed());
        assert!(!t.send("d@x", ip("1.1.1.1")).allowed(), "per IP, whatever the address");
    }

    #[test]
    fn wrong_codes_back_off_and_a_right_one_clears_the_address() {
        let t = Throttles::new(Limits { try_per_address: Backoff { free: 2, base: 10, cap: 25, window: 600 }, ..Limits::default() });
        let from = ip("1.1.1.1");
        assert!(t.tried_wrong("a@x", from).allowed());
        assert!(t.tried_wrong("a@x", from).allowed());
        assert!(t.may_try("a@x", from).allowed());
        assert_eq!(t.tried_wrong("a@x", from), Verdict::Wait(10));
        assert!(!t.may_try("a@x", from).allowed());
        assert!(!t.may_try("a@x", ip("9.9.9.9")).allowed(), "locked per address from anywhere");
        assert!(t.may_try("b@x", from).allowed(), "another address is not locked");
        assert_eq!(t.tried_wrong("a@x", from), Verdict::Wait(20), "doubles");
        assert_eq!(t.tried_wrong("a@x", from), Verdict::Wait(25), "capped");
        t.tried_right("a@x");
        assert!(t.may_try("a@x", from).allowed());
    }

    #[test]
    fn wrong_codes_from_one_ip_lock_that_ip_for_every_address() {
        let t = Throttles::new(Limits { try_per_ip: Backoff { free: 3, base: 10, cap: 60, window: 600 }, ..Limits::default() });
        let from = ip("1.1.1.1");
        for a in ["a@x", "b@x", "c@x"] {
            assert!(t.tried_wrong(a, from).allowed());
        }
        assert!(!t.tried_wrong("d@x", from).allowed());
        assert!(!t.may_try("e@x", from).allowed(), "a fresh address from that IP waits");
        assert!(t.may_try("e@x", ip("2.2.2.2")).allowed());
        assert!(!t.may_try_totp("user_1", from).allowed(), "TOTP shares the IP's tries");
    }

    #[test]
    fn totp_backs_off_per_user() {
        let t = Throttles::new(Limits { totp_per_user: Backoff { free: 1, base: 5, cap: 5, window: 600 }, ..Limits::default() });
        assert!(t.totp_wrong("user_1", ip("1.1.1.1")).allowed());
        assert!(!t.totp_wrong("user_1", ip("2.2.2.2")).allowed());
        assert!(!t.may_try_totp("user_1", ip("3.3.3.3")).allowed());
        t.totp_right("user_1");
        assert!(t.may_try_totp("user_1", ip("3.3.3.3")).allowed());
    }

    #[test]
    fn a_table_is_bounded() {
        let table = Table::new();
        for i in 0..(MAX_KEYS + 10) {
            let mut k = [0u8; 32];
            k[..8].copy_from_slice(&(i as u64).to_le_bytes());
            table.take(k, Rate { max: 1, window: 60 });
        }
        assert_eq!(table.entries.lock().unwrap().len(), MAX_KEYS);
    }
}
