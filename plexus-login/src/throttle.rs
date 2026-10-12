//! AUTH-22: throttles for our own sign-in page — codes sent, codes,
//! passwords and TOTP codes tried, and sign-in flows started.
//!
//! AUTH-24 #1: **a guess is counted before it is made.** Every attempt
//! reserves its place in the same critical section that checks the limit
//! ([`Throttles::reserve_code`] …): the attempt is recorded as a failure up
//! front, and a right answer refunds it ([`Throttles::refund`]). So a key has
//! exactly its allowance of guesses in flight, and a burst of any size gets
//! no more than that to the backend.
//!
//! Two shapes:
//!
//! - a **rate** ([`Rate`]): at most `max` events in a sliding `window`
//!   (codes sent, flows started);
//! - a **backoff** ([`Backoff`]): `free` attempts in `window`, then the n-th
//!   attempt past them waits `base · 2^(n−1)` seconds (capped at `cap`)
//!   after the one before it.
//!
//! AUTH-24 #3: a third party must not lock the owner of an address out.
//! Codes and passwords tried are limited per **(address, client)** — the
//! attacker's guesses lock the attacker's own client — and only a looser
//! ceiling counts every client's guesses at one address. A code *send* is
//! limited per (address, client) and per client, never per address alone,
//! so nobody can use up the owner's sends.
//!
//! AUTH-24 #6: a client is its address's network: IPv4 by /32 and IPv6 by
//! /64 unless configured ([`Limits::ipv4_prefix`], [`Limits::ipv6_prefix`]).
//!
//! Every table is bounded ([`MAX_KEYS`]) and keyed by a salted hash, so
//! the tables hold no address. A full table purges what has aged out (at
//! most every few seconds) and otherwise **refuses** a new key rather than
//! evict one: evicting would let a flood of fresh keys wipe out a lock.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use crate::sessions::now;

/// The most keys one table holds.
pub const MAX_KEYS: usize = 100_000;
/// The longest window any limit uses: an entry idle this long holds nothing.
const MAX_WINDOW: i64 = 24 * 3600;
/// How often a full table may purge.
const PURGE_EVERY: i64 = 5;
/// What a full table answers a new key.
const FULL_WAIT: i64 = 60;

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
}

/// At most `max` events per `window` seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    pub max: usize,
    pub window: i64,
}

/// `free` attempts in `window` seconds, then each further one waits
/// `base · 2^(n−1)` seconds (at most `cap`) after the one before it.
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
    /// Codes sent to one address from one client.
    pub send_per_address_client: Rate,
    /// Codes sent (and SSO lookups) from one client, any address.
    pub send_per_client: Rate,
    /// Codes sent by the whole server: our standing with the mail sender.
    pub send_total: Rate,
    /// Codes (or passwords) tried for one address from one client.
    pub try_per_address_client: Backoff,
    /// Codes (or passwords) tried for one address from every client: the
    /// looser ceiling against a distributed guess.
    pub try_per_address: Backoff,
    /// Codes, passwords and TOTP codes tried from one client.
    pub try_per_client: Backoff,
    /// TOTP codes tried for one user (reached only past the first factor).
    pub totp_per_user: Backoff,
    /// Sign-in flows a client may start.
    pub flows_per_client: Rate,
    /// Sign-in flows one client may hold at once; its oldest makes room.
    pub flows_held_per_client: usize,
    /// Sign-in flows held in all; past it new ones are refused (never
    /// someone else's evicted).
    pub flows_total: usize,
    /// A client's IPv4 network (32: the address).
    pub ipv4_prefix: u8,
    /// A client's IPv6 network (64: a subscriber's usual delegation).
    pub ipv6_prefix: u8,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            send_per_address_client: Rate { max: 3, window: 600 },
            send_per_client: Rate { max: 20, window: 600 },
            send_total: Rate { max: 2000, window: 600 },
            try_per_address_client: Backoff { free: 5, base: 30, cap: 900, window: 3600 },
            try_per_address: Backoff { free: 50, base: 60, cap: 900, window: 3600 },
            try_per_client: Backoff { free: 20, base: 30, cap: 900, window: 3600 },
            totp_per_user: Backoff { free: 5, base: 30, cap: 900, window: 3600 },
            flows_per_client: Rate { max: 60, window: 600 },
            flows_held_per_client: 20,
            flows_total: 20_000,
            ipv4_prefix: 32,
            ipv6_prefix: 64,
        }
    }
}

#[derive(Default)]
struct Entry {
    /// Event (attempt) times, oldest first.
    times: Vec<i64>,
}

/// One table of keys.
struct Table {
    entries: Mutex<(HashMap<[u8; 32], Entry>, i64)>,
    cap: usize,
}

impl Table {
    fn new() -> Self {
        Self::with_cap(MAX_KEYS)
    }

    fn with_cap(cap: usize) -> Self {
        Self { entries: Mutex::new((HashMap::new(), 0)), cap }
    }

    /// `f` over the key's entry; `None` when the table is full of live keys.
    fn with<R>(&self, key: [u8; 32], f: impl FnOnce(&mut Entry) -> R) -> Option<R> {
        let mut guard = self.entries.lock().unwrap();
        let (map, purged) = &mut *guard;
        if !map.contains_key(&key) && map.len() >= self.cap {
            let t = now();
            if t - *purged >= PURGE_EVERY {
                *purged = t;
                map.retain(|_, e| e.times.last().is_some_and(|&l| t - l < MAX_WINDOW));
            }
            if map.len() >= self.cap {
                return None;
            }
        }
        Some(f(map.entry(key).or_default()))
    }

    /// Counts an event against a rate, if it fits.
    fn take(&self, key: [u8; 32], rate: Rate) -> Verdict {
        match self.take_at(key, rate) {
            Ok(_) => Verdict::Allow,
            Err(w) => Verdict::Wait(w),
        }
    }

    /// [`Table::take`], the time it was counted at (to refund).
    fn take_at(&self, key: [u8; 32], rate: Rate) -> Result<i64, i64> {
        let t = now();
        self.with(key, |e| {
            e.times.retain(|&at| t - at < rate.window);
            if e.times.len() >= rate.max {
                return Err((e.times[0] + rate.window - t).max(1));
            }
            e.times.push(t);
            Ok(t)
        })
        .unwrap_or(Err(FULL_WAIT))
    }

    /// Reserves an attempt against a backoff — checked and counted in one
    /// step: the time it was counted at, or the wait.
    fn reserve(&self, key: [u8; 32], b: Backoff) -> Result<i64, i64> {
        let t = now();
        self.with(key, |e| {
            e.times.retain(|&at| t - at < b.window);
            let n = e.times.len() as u32;
            if n >= b.free {
                let last = *e.times.last().expect("n ≥ free ≥ … has a last");
                let wait = b.base.saturating_mul(1i64 << (n - b.free).min(20)).min(b.cap);
                if t < last + wait {
                    return Err(last + wait - t);
                }
            }
            e.times.push(t);
            Ok(t)
        })
        .unwrap_or(Err(FULL_WAIT))
    }

    /// Takes back one reserved attempt.
    fn refund(&self, key: [u8; 32], at: i64) {
        if let Some(e) = self.entries.lock().unwrap().0.get_mut(&key) {
            if let Some(i) = e.times.iter().rposition(|&x| x == at) {
                e.times.remove(i);
            }
        }
    }

    fn clear(&self, key: [u8; 32]) {
        self.entries.lock().unwrap().0.remove(&key);
    }
}

/// Which table a reserved slot is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    AddressClient,
    Address,
    Client,
    TotpUser,
}

/// Attempts reserved for one guess: refunded when it was right (or never
/// reached a verdict).
#[derive(Debug)]
#[must_use = "a reservation is refunded on success; dropping it keeps the attempt counted"]
pub struct Reservation {
    slots: Vec<(Which, [u8; 32], i64)>,
    /// The (address, client) key a right answer clears.
    forgive: Option<[u8; 32]>,
    forgive_totp: Option<[u8; 32]>,
}

/// Every throttle the sign-in page keeps. See the module docs.
pub struct Throttles {
    pub limits: Limits,
    salt: [u8; 32],
    sent_address_client: Table,
    sent_client: Table,
    sent_total: Table,
    tried_address_client: Table,
    tried_address: Table,
    tried_client: Table,
    totp_user: Table,
    flows_client: Table,
}

impl Throttles {
    pub fn new(limits: Limits) -> Self {
        let mut salt = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut salt);
        Self {
            limits,
            salt,
            sent_address_client: Table::new(),
            sent_client: Table::new(),
            sent_total: Table::new(),
            tried_address_client: Table::new(),
            tried_address: Table::new(),
            tried_client: Table::new(),
            totp_user: Table::new(),
            flows_client: Table::new(),
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

    /// A client: its address's network (AUTH-24 #6), or `unknown`.
    pub fn client(&self, ip: Option<IpAddr>) -> String {
        match ip {
            Some(IpAddr::V4(v4)) => {
                let p = self.limits.ipv4_prefix.min(32);
                let mask = if p == 0 { 0 } else { u32::MAX << (32 - p) };
                format!("{}/{p}", std::net::Ipv4Addr::from(u32::from(v4) & mask))
            }
            Some(IpAddr::V6(v6)) => {
                let p = self.limits.ipv6_prefix.min(128);
                let mask = if p == 0 { 0 } else { u128::MAX << (128 - p) };
                format!("{}/{p}", std::net::Ipv6Addr::from(u128::from(v6) & mask))
            }
            None => "unknown".into(),
        }
    }

    fn table(&self, w: Which) -> &Table {
        match w {
            Which::AddressClient => &self.tried_address_client,
            Which::Address => &self.tried_address,
            Which::Client => &self.tried_client,
            Which::TotpUser => &self.totp_user,
        }
    }

    fn backoff(&self, w: Which) -> Backoff {
        match w {
            Which::AddressClient => self.limits.try_per_address_client,
            Which::Address => self.limits.try_per_address,
            Which::Client => self.limits.try_per_client,
            Which::TotpUser => self.limits.totp_per_user,
        }
    }

    /// Reserves in every table or none: the reservation, or the longest
    /// wait.
    fn reserve_all(&self, wanted: &[(Which, [u8; 32])]) -> Result<Vec<(Which, [u8; 32], i64)>, i64> {
        let mut got = Vec::new();
        for &(w, k) in wanted {
            match self.table(w).reserve(k, self.backoff(w)) {
                Ok(at) => got.push((w, k, at)),
                Err(wait) => {
                    for (w, k, at) in got {
                        self.table(w).refund(k, at);
                    }
                    return Err(wait);
                }
            }
        }
        Ok(got)
    }

    /// A code send for `address` from `ip`: counted against the client's
    /// sends to that address, the client's sends, and the server's, or the
    /// wait (nothing counted then).
    pub fn send(&self, address: &str, ip: Option<IpAddr>) -> Verdict {
        let client = self.client(ip);
        let steps = [
            (&self.sent_address_client, self.key("addr-client", &format!("{address}\u{0}{client}")), self.limits.send_per_address_client),
            (&self.sent_client, self.key("client", &client), self.limits.send_per_client),
            (&self.sent_total, self.key("total", ""), self.limits.send_total),
        ];
        let mut taken = Vec::new();
        for (table, key, rate) in steps {
            match table.take_at(key, rate) {
                Ok(at) => taken.push((table, key, at)),
                Err(w) => {
                    for (table, key, at) in taken {
                        table.refund(key, at);
                    }
                    return Verdict::Wait(w);
                }
            }
        }
        Verdict::Allow
    }

    /// A sign-in flow started by `ip`.
    pub fn flow(&self, ip: Option<IpAddr>) -> Verdict {
        self.flows_client.take(self.key("flow", &self.client(ip)), self.limits.flows_per_client)
    }

    /// Reserves a guess of `address`'s code (or `address`'s password) from
    /// `ip`, or the wait.
    pub fn reserve_code(&self, address: &str, ip: Option<IpAddr>) -> Result<Reservation, i64> {
        let client = self.client(ip);
        let ac = self.key("addr-client", &format!("{address}\u{0}{client}"));
        let slots = self.reserve_all(&[(Which::AddressClient, ac), (Which::Address, self.key("addr", address)), (Which::Client, self.key("client", &client))])?;
        Ok(Reservation { slots, forgive: Some(ac), forgive_totp: None })
    }

    /// Reserves a guess of `user`'s TOTP code from `ip`, or the wait.
    pub fn reserve_totp(&self, user: &str, ip: Option<IpAddr>) -> Result<Reservation, i64> {
        let client = self.client(ip);
        let u = self.key("user", user);
        let slots = self.reserve_all(&[(Which::TotpUser, u), (Which::Client, self.key("client", &client))])?;
        Ok(Reservation { slots, forgive: None, forgive_totp: Some(u) })
    }

    /// The guess was right (or never reached a verdict): its attempts are
    /// taken back, and a right one forgives its (address, client) or user.
    pub fn refund(&self, r: Reservation, right: bool) {
        for (w, k, at) in r.slots {
            self.table(w).refund(k, at);
        }
        if right {
            if let Some(k) = r.forgive {
                self.tried_address_client.clear(k);
            }
            if let Some(k) = r.forgive_totp {
                self.totp_user.clear(k);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Option<IpAddr> {
        Some(s.parse().unwrap())
    }

    fn tight() -> Limits {
        Limits {
            try_per_address_client: Backoff { free: 3, base: 10, cap: 25, window: 600 },
            try_per_address: Backoff { free: 8, base: 10, cap: 25, window: 600 },
            try_per_client: Backoff { free: 100, base: 10, cap: 25, window: 600 },
            ..Limits::default()
        }
    }

    #[test]
    fn a_burst_gets_exactly_its_allowance() {
        let t = Throttles::new(tight());
        let passed = (0..200).filter_map(|_| t.reserve_code("a@x", ip("1.1.1.1")).ok()).count();
        assert_eq!(passed, 3, "counted before the guess is made");
    }

    #[test]
    fn a_right_answer_refunds_and_forgives() {
        let t = Throttles::new(tight());
        for _ in 0..2 {
            let _ = t.reserve_code("a@x", ip("1.1.1.1")).unwrap();
        }
        let r = t.reserve_code("a@x", ip("1.1.1.1")).unwrap();
        t.refund(r, true);
        for _ in 0..3 {
            let _ = t.reserve_code("a@x", ip("1.1.1.1")).expect("forgiven");
        }
        assert!(t.reserve_code("a@x", ip("1.1.1.1")).is_err());
    }

    #[test]
    fn past_the_allowance_each_guess_waits_longer() {
        let t = Throttles::new(Limits { try_per_address_client: Backoff { free: 1, base: 10, cap: 25, window: 600 }, ..tight() });
        let _ = t.reserve_code("a@x", ip("1.1.1.1")).unwrap();
        assert_eq!(t.reserve_code("a@x", ip("1.1.1.1")).unwrap_err(), 10);
        // Another client is not held up by it.
        assert!(t.reserve_code("a@x", ip("2.2.2.2")).is_ok());
    }

    #[test]
    fn another_client_cannot_lock_the_owner_out() {
        let t = Throttles::new(tight());
        for _ in 0..6 {
            let _ = t.reserve_code("victim@x", ip("198.51.100.66"));
        }
        assert!(t.reserve_code("victim@x", ip("198.51.100.66")).is_err(), "the attacker is locked");
        assert!(t.reserve_code("victim@x", ip("203.0.113.5")).is_ok(), "the owner is not");
        // Only many clients reach the address's own ceiling.
        for i in 0..10 {
            let _ = t.reserve_code("victim@x", ip(&format!("192.0.2.{i}")));
        }
        assert!(t.reserve_code("victim@x", ip("203.0.113.9")).is_err(), "the looser ceiling");
    }

    #[test]
    fn sends_are_per_address_and_client_per_client_and_in_all() {
        let t = Throttles::new(Limits { send_per_address_client: Rate { max: 2, window: 600 }, send_per_client: Rate { max: 3, window: 600 }, send_total: Rate { max: 5, window: 600 }, ..Limits::default() });
        assert!(t.send("a@x", ip("1.1.1.1")).allowed());
        assert!(t.send("a@x", ip("1.1.1.1")).allowed());
        assert!(!t.send("a@x", ip("1.1.1.1")).allowed(), "per (address, client)");
        assert!(t.send("a@x", ip("2.2.2.2")).allowed(), "the owner can still ask");
        assert!(t.send("b@x", ip("1.1.1.1")).allowed());
        assert!(!t.send("c@x", ip("1.1.1.1")).allowed(), "per client");
        assert!(t.send("d@x", ip("3.3.3.3")).allowed());
        assert!(!t.send("e@x", ip("4.4.4.4")).allowed(), "in all");
    }

    #[test]
    fn ipv6_is_bucketed_by_its_network() {
        let t = Throttles::new(Limits::default());
        assert_eq!(t.client(ip("2001:db8:1:2::1")), t.client(ip("2001:db8:1:2:ffff::28")));
        assert_ne!(t.client(ip("2001:db8:1:2::1")), t.client(ip("2001:db8:1:3::1")));
        assert_eq!(t.client(ip("192.0.2.1")), "192.0.2.1/32");
        let wide = Throttles::new(Limits { ipv4_prefix: 24, ipv6_prefix: 48, ..Limits::default() });
        assert_eq!(wide.client(ip("192.0.2.77")), "192.0.2.0/24");
        assert_eq!(wide.client(ip("2001:db8:1:2::1")), wide.client(ip("2001:db8:1:ffff::1")));
    }

    #[test]
    fn totp_is_reserved_per_user() {
        let t = Throttles::new(Limits { totp_per_user: Backoff { free: 2, base: 5, cap: 5, window: 600 }, ..Limits::default() });
        let passed = (0..50).filter_map(|i| t.reserve_totp("user_1", ip(&format!("10.0.0.{i}"))).ok()).count();
        assert_eq!(passed, 2);
    }

    #[test]
    fn a_full_table_refuses_new_keys_and_never_evicts_a_lock() {
        let table = Table::with_cap(100);
        let b = Backoff { free: 1, base: 60, cap: 60, window: 600 };
        let locked = [7u8; 32];
        let _ = table.reserve(locked, b).unwrap();
        assert!(table.reserve(locked, b).is_err());
        for i in 0..500u64 {
            let mut k = [0u8; 32];
            k[..8].copy_from_slice(&i.to_le_bytes());
            let _ = table.take(k, Rate { max: 1, window: 60 });
        }
        assert_eq!(table.entries.lock().unwrap().0.len(), 100);
        assert!(table.reserve(locked, b).is_err(), "the lock survived the flood");
        assert_eq!(table.take([9u8; 32], Rate { max: 1, window: 60 }), Verdict::Wait(FULL_WAIT));
    }
}
