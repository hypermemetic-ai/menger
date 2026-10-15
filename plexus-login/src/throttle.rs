//! AUTH-22: throttles for our own sign-in page — codes sent, SSO lookups,
//! guesses (codes, passwords, TOTP codes) and sign-in flows started.
//!
//! Three rules, from two adversarial reviews:
//!
//! 1. **A guess is counted before it is made** (AUTH-24 #1). An attempt
//!    reserves its place in the same critical section that checks the limit
//!    ([`Throttles::reserve_code`] …). Only a definite answer settles it: a
//!    right one is refunded, a wrong one stays counted, and only an answer
//!    that is no verdict at all (the backend down, or rate-limiting us) is
//!    given back (AUTH-25 N8). So a key gets exactly its allowance of
//!    guesses in flight, and a burst of any size gets no more.
//! 2. **No limit refuses everyone** (AUTH-25 N1–N3). There is no global
//!    ceiling. A distributed attacker is bounded by **network aggregation**:
//!    each per-client limit is kept at three levels — IPv6 /64, /48 and /32,
//!    IPv4 /32, /24 and /16 — each with its own, looser allowance. A
//!    thousand /64s from one provider share a /48 or /32 budget, and an
//!    unrelated network is untouched.
//! 3. **Bounded memory without collateral** (AUTH-25 N1). Each table drops
//!    entries idle past its *own* window. When still full, it evicts the
//!    least-recently-used entries that hold no lock, taken first from the
//!    most-represented network (the flooder's own). Locked entries survive
//!    as long as anything else can go. A new key is always admitted.
//!
//! Every table is keyed by a salted hash, so the tables hold no address.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;

use sha2::{Digest, Sha256};

use crate::sessions::now;

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

/// A per-client limit at the three network levels: the client's own
/// network (/64, /32), its neighbourhood (/48, /24) and its provider
/// (/32, /16).
pub type Tiered<T> = [T; 3];

/// The sign-in's limits. [`Limits::default`] is what a deployment gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Codes sent to one address from one neighbourhood (/48, /24).
    pub send_per_address_network: Rate,
    /// … and per day.
    pub send_per_address_network_day: Rate,
    /// Codes sent to one address per day from every network that has never
    /// signed that address in (AUTH-25 N5): a stranger's sends. The owner's
    /// own networks keep their own allowance, which strangers cannot use.
    pub send_per_address_foreign_day: Rate,
    /// Codes sent from one client, at each level.
    pub send_per_client: Tiered<Rate>,
    /// SSO lookups (an email's domain) from one client, at each level.
    pub sso_per_client: Tiered<Rate>,
    /// Wrong guesses allowed for one sent code in its flow; then that flow
    /// needs a new code (AUTH-25 N4).
    pub code_tries_per_flow: u32,
    /// Guesses (codes, passwords, TOTP codes) from one client's own network
    /// (/64, /32) — never a wider level (AUTH-26 R1: a wide guess tier would
    /// refuse a whole provider the right code). An emailed code's guesses
    /// are bounded per send already, and sends are tiered.
    pub try_per_client: Backoff,
    /// Passwords tried for one username from one neighbourhood.
    pub try_per_user_network: Backoff,
    /// TOTP codes tried for one user (reached only past the first factor).
    pub totp_per_user: Backoff,
    /// Sign-in flows started by one client, at each level.
    pub flows_per_client: Tiered<Rate>,
    /// Sign-in flows held in all; past it the most-represented network's
    /// newest flows make room — never a refusal.
    pub flows_total: usize,
    /// Keys one throttle table holds before it evicts.
    pub table_cap: usize,
    /// How long a network that signed an address in stays its owner's.
    pub known_network_secs: i64,
    /// How long a form's CSRF token lasts (a page older than this is
    /// drawn again with a fresh one).
    pub form_token_secs: i64,
    /// IPv4 prefixes of the three levels.
    pub ipv4_prefixes: [u8; 3],
    /// IPv6 prefixes of the three levels.
    pub ipv6_prefixes: [u8; 3],
}

const DAY: i64 = 86_400;

impl Default for Limits {
    fn default() -> Self {
        let rate = |max, window| Rate { max, window };
        let backoff = |free, window| Backoff { free, base: 30, cap: 900, window };
        Self {
            send_per_address_network: rate(3, 600),
            send_per_address_network_day: rate(10, DAY),
            send_per_address_foreign_day: rate(3, DAY),
            send_per_client: [rate(20, 600), rate(100, 600), rate(500, 600)],
            sso_per_client: [rate(60, 600), rate(300, 600), rate(1500, 600)],
            code_tries_per_flow: 5,
            try_per_client: backoff(30, 3600),
            try_per_user_network: Backoff { free: 5, base: 30, cap: 900, window: 3600 },
            totp_per_user: Backoff { free: 5, base: 30, cap: 900, window: 3600 },
            flows_per_client: [rate(60, 600), rate(600, 600), rate(3000, 600)],
            flows_total: 200_000,
            table_cap: 100_000,
            known_network_secs: 90 * DAY,
            form_token_secs: 900,
            ipv4_prefixes: [32, 24, 16],
            ipv6_prefixes: [64, 48, 32],
        }
    }
}

/// A client's networks, own to widest, and the neighbourhood its entries
/// are counted under when a table must evict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Net {
    pub levels: [String; 3],
    agg: u64,
}

impl Net {
    /// The client's own network (/64, /32): its key in the per-client tables.
    pub fn own(&self) -> &str {
        &self.levels[0]
    }

    /// The neighbourhood (/48, /24) a flood is grouped by.
    pub fn neighbourhood(&self) -> &str {
        &self.levels[1]
    }

    /// The neighbourhood as a number (eviction's grouping).
    pub fn agg(&self) -> u64 {
        self.agg
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Rate(Rate),
    Backoff(Backoff),
}

impl Kind {
    fn window(self) -> i64 {
        match self {
            Kind::Rate(r) => r.window,
            Kind::Backoff(b) => b.window,
        }
    }
}

struct Entry {
    /// Event (attempt) times, oldest first.
    times: Vec<i64>,
    /// The neighbourhood that made it.
    agg: u64,
}

impl Entry {
    fn last(&self) -> i64 {
        self.times.last().copied().unwrap_or(i64::MIN)
    }
}

/// One table of keys under one limit.
struct Table {
    entries: Mutex<HashMap<[u8; 32], Entry>>,
    kind: Kind,
    cap: usize,
}

impl Table {
    fn rate(r: Rate, cap: usize) -> Self {
        Self { entries: Mutex::new(HashMap::new()), kind: Kind::Rate(r), cap: cap.max(16) }
    }

    fn backoff(b: Backoff, cap: usize) -> Self {
        Self { entries: Mutex::new(HashMap::new()), kind: Kind::Backoff(b), cap: cap.max(16) }
    }

    /// Whether an entry holds a lock (at or past its limit): what eviction
    /// spares.
    fn locked(&self, e: &Entry, t: i64) -> bool {
        let w = self.kind.window();
        let live = e.times.iter().filter(|&&at| t - at < w).count();
        match self.kind {
            Kind::Rate(r) => live >= r.max,
            Kind::Backoff(b) => live >= b.free as usize,
        }
    }

    /// Makes room in a full table (see the module docs). Takes it down to
    /// nine tenths, so a pass runs once per tenth of the cap.
    fn evict(&self, map: &mut HashMap<[u8; 32], Entry>, t: i64) {
        let w = self.kind.window();
        map.retain(|_, e| t.saturating_sub(e.last()) < w);
        let target = self.cap - self.cap / 10;
        if map.len() < self.cap {
            return;
        }
        // ACCESS-45: the top neighbourhood is the largest, and among equals
        // the one most recently active — a spread flood is the newest, so a
        // tie never falls to an owner's older network by the map's order.
        let mut per_agg: HashMap<u64, (usize, i64)> = HashMap::new();
        for e in map.values() {
            let slot = per_agg.entry(e.agg).or_insert((0, i64::MIN));
            slot.0 += 1;
            slot.1 = slot.1.max(e.last());
        }
        let top = per_agg.into_iter().max_by_key(|&(a, (n, last))| (n, last, a)).map(|(a, _)| a).unwrap_or(0);
        // Unlocked from the top neighbourhood, then unlocked anywhere, then
        // the top neighbourhood's locks, then anything — **newest first**
        // (AUTH-26 R3): a flood is the newest, and an owner's remembered
        // network (older) is the last to go.
        for pass in 0..4 {
            if map.len() <= target {
                return;
            }
            let mut victims: Vec<([u8; 32], i64)> = map
                .iter()
                .filter(|(_, e)| match pass {
                    0 => e.agg == top && !self.locked(e, t),
                    1 => !self.locked(e, t),
                    2 => e.agg == top,
                    _ => true,
                })
                .map(|(k, e)| (*k, e.last()))
                .collect();
            victims.sort_unstable_by_key(|&(_, last)| std::cmp::Reverse(last));
            for (k, _) in victims.into_iter().take(map.len() - target) {
                map.remove(&k);
            }
        }
    }

    fn with<R>(&self, key: [u8; 32], agg: u64, f: impl FnOnce(&mut Entry) -> R) -> R {
        let mut map = self.entries.lock().unwrap();
        if !map.contains_key(&key) && map.len() >= self.cap {
            self.evict(&mut map, now());
        }
        f(map.entry(key).or_insert_with(|| Entry { times: Vec::new(), agg }))
    }

    /// Counts an event against the rate, if it fits: when, or the wait.
    fn take(&self, key: [u8; 32], agg: u64) -> Result<i64, i64> {
        let Kind::Rate(rate) = self.kind else { unreachable!("a rate table") };
        let t = now();
        self.with(key, agg, |e| {
            e.times.retain(|&at| t - at < rate.window);
            if e.times.len() >= rate.max {
                return Err((e.times[0] + rate.window - t).max(1));
            }
            e.times.push(t);
            Ok(t)
        })
    }

    /// Reserves an attempt against the backoff — checked and counted in one
    /// step: when, or the wait.
    fn reserve(&self, key: [u8; 32], agg: u64) -> Result<i64, i64> {
        let Kind::Backoff(b) = self.kind else { unreachable!("a backoff table") };
        let t = now();
        self.with(key, agg, |e| {
            e.times.retain(|&at| t - at < b.window);
            let n = e.times.len() as u32;
            if n >= b.free {
                let last = *e.times.last().expect("n ≥ free > 0");
                let wait = b.base.saturating_mul(1i64 << (n - b.free).min(20)).min(b.cap);
                if t < last + wait {
                    return Err(last + wait - t);
                }
            }
            e.times.push(t);
            Ok(t)
        })
    }

    /// Whether a key has a live event (membership tables).
    fn holds(&self, key: [u8; 32]) -> bool {
        let w = self.kind.window();
        let t = now();
        self.entries.lock().unwrap().get(&key).is_some_and(|e| t.saturating_sub(e.last()) < w)
    }

    fn refund(&self, key: [u8; 32], at: i64) {
        if let Some(e) = self.entries.lock().unwrap().get_mut(&key) {
            if let Some(i) = e.times.iter().rposition(|&x| x == at) {
                e.times.remove(i);
            }
        }
    }

    fn clear(&self, key: [u8; 32]) {
        self.entries.lock().unwrap().remove(&key);
    }

    fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

/// Attempts reserved for one guess, settled by its answer.
#[derive(Debug)]
#[must_use = "a reservation is settled by the answer; dropping it keeps the attempt counted"]
pub struct Reservation {
    slots: Vec<(usize, [u8; 32], i64)>,
    /// Keys a right answer clears.
    forgive: Vec<(usize, [u8; 32])>,
}

/// How a guess ended, for [`Throttles::settle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Definitely right.
    Right,
    /// Wrong — or any answer that is not definitely right or no verdict
    /// (a 401/403 included: AUTH-25 N8).
    Wrong,
    /// No verdict: the backend unreachable, failing, or rate-limiting us.
    NoVerdict,
}

// Table indices.
const SEND_ADDR_NET: usize = 0;
const SEND_ADDR_NET_DAY: usize = 1;
const SEND_ADDR_FOREIGN: usize = 2;
const SEND_CLIENT: usize = 3; // 3, 4, 5
const SSO_CLIENT: usize = 6; // 6, 7, 8
const TRY_CLIENT: usize = 9;
const TRY_USER_NET: usize = 10;
const TOTP_USER: usize = 11;
const FLOW_CLIENT: usize = 12; // 12, 13, 14
const KNOWN: usize = 15;

/// Every throttle the sign-in page keeps. See the module docs.
pub struct Throttles {
    pub limits: Limits,
    salt: [u8; 32],
    tables: Vec<Table>,
}

impl Throttles {
    pub fn new(limits: Limits) -> Self {
        let mut salt = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut salt);
        let c = limits.table_cap;
        let mut tables = vec![
            Table::rate(limits.send_per_address_network, c),
            Table::rate(limits.send_per_address_network_day, c),
            Table::rate(limits.send_per_address_foreign_day, c),
        ];
        tables.extend(limits.send_per_client.iter().map(|r| Table::rate(*r, c)));
        tables.extend(limits.sso_per_client.iter().map(|r| Table::rate(*r, c)));
        tables.push(Table::backoff(limits.try_per_client, c));
        tables.push(Table::backoff(limits.try_per_user_network, c));
        tables.push(Table::backoff(limits.totp_per_user, c));
        tables.extend(limits.flows_per_client.iter().map(|r| Table::rate(*r, c)));
        tables.push(Table::rate(Rate { max: 1, window: limits.known_network_secs }, c));
        Self { limits, salt, tables }
    }

    fn key(&self, kind: &str, value: &str) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.salt);
        h.update(kind.as_bytes());
        h.update([0]);
        h.update(value.as_bytes());
        h.finalize().into()
    }

    /// A client's networks (AUTH-24 #6, AUTH-25): `unknown` when there is
    /// no address.
    pub fn net(&self, ip: Option<IpAddr>) -> Net {
        let levels: [String; 3] = match ip {
            Some(IpAddr::V4(v4)) => self.limits.ipv4_prefixes.map(|p| {
                let p = p.min(32);
                let mask = if p == 0 { 0 } else { u32::MAX << (32 - p) };
                format!("{}/{p}", std::net::Ipv4Addr::from(u32::from(v4) & mask))
            }),
            Some(IpAddr::V6(v6)) => self.limits.ipv6_prefixes.map(|p| {
                let p = p.min(128);
                let mask = if p == 0 { 0 } else { u128::MAX << (128 - p) };
                format!("{}/{p}", std::net::Ipv6Addr::from(u128::from(v6) & mask))
            }),
            None => ["unknown".into(), "unknown".into(), "unknown".into()],
        };
        let agg = u64::from_le_bytes(self.key("agg", &levels[1])[..8].try_into().expect("8 bytes"));
        Net { levels, agg }
    }

    /// The client's own network (its throttle key, its flow grouping).
    pub fn client(&self, ip: Option<IpAddr>) -> String {
        self.net(ip).levels[0].clone()
    }

    /// Takes from every (table, key) or none: the longest wait otherwise.
    fn take_all(&self, wanted: &[(usize, [u8; 32])], agg: u64) -> Verdict {
        let mut got = Vec::new();
        for &(i, k) in wanted {
            match self.tables[i].take(k, agg) {
                Ok(at) => got.push((i, k, at)),
                Err(w) => {
                    for (i, k, at) in got {
                        self.tables[i].refund(k, at);
                    }
                    return Verdict::Wait(w);
                }
            }
        }
        Verdict::Allow
    }

    fn reserve_all(&self, wanted: &[(usize, [u8; 32])], agg: u64) -> Result<Vec<(usize, [u8; 32], i64)>, i64> {
        let mut got = Vec::new();
        for &(i, k) in wanted {
            match self.tables[i].reserve(k, agg) {
                Ok(at) => got.push((i, k, at)),
                Err(w) => {
                    for (i, k, at) in got {
                        self.tables[i].refund(k, at);
                    }
                    return Err(w);
                }
            }
        }
        Ok(got)
    }

    fn tiers(&self, base: usize, kind: &str, net: &Net) -> [(usize, [u8; 32]); 3] {
        [0, 1, 2].map(|l| (base + l, self.key(kind, &net.levels[l])))
    }

    /// The guess table's key for a client: its own network only.
    fn own_try(&self, net: &Net) -> (usize, [u8; 32]) {
        (TRY_CLIENT, self.key("try", net.own()))
    }

    /// Whether `ip`'s neighbourhood has signed `address` in before: the
    /// owner's network, whose sends no stranger can use up.
    pub fn known(&self, address: &str, ip: Option<IpAddr>) -> bool {
        let net = self.net(ip);
        self.tables[KNOWN].holds(self.key("known", &format!("{address}\u{0}{}", net.neighbourhood())))
    }

    /// `address` signed in from `ip`: its neighbourhood is the owner's now.
    pub fn signed_in(&self, address: &str, ip: Option<IpAddr>) {
        let net = self.net(ip);
        let k = self.key("known", &format!("{address}\u{0}{}", net.neighbourhood()));
        self.tables[KNOWN].clear(k);
        let _ = self.tables[KNOWN].take(k, net.agg);
    }

    /// A code send for `address` from `ip`: counted against the client at
    /// every level and against (address, neighbourhood) — and, from a
    /// network that never signed the address in, against the address's
    /// strangers' day — or the wait (nothing counted then).
    pub fn send(&self, address: &str, ip: Option<IpAddr>) -> Verdict {
        let net = self.net(ip);
        let an = format!("{address}\u{0}{}", net.neighbourhood());
        let mut wanted: Vec<(usize, [u8; 32])> = self.tiers(SEND_CLIENT, "send", &net).to_vec();
        wanted.push((SEND_ADDR_NET, self.key("addr-net", &an)));
        wanted.push((SEND_ADDR_NET_DAY, self.key("addr-net", &an)));
        if !self.known(address, ip) {
            wanted.push((SEND_ADDR_FOREIGN, self.key("addr-foreign", address)));
        }
        self.take_all(&wanted, net.agg)
    }

    /// An SSO lookup from `ip` (AUTH-25 N3: apart from sends).
    pub fn sso(&self, ip: Option<IpAddr>) -> Verdict {
        let net = self.net(ip);
        self.take_all(&self.tiers(SSO_CLIENT, "sso", &net), net.agg)
    }

    /// A sign-in flow started by `ip`.
    pub fn flow(&self, ip: Option<IpAddr>) -> Verdict {
        let net = self.net(ip);
        self.take_all(&self.tiers(FLOW_CLIENT, "flow", &net), net.agg)
    }

    /// Reserves a guess of an emailed code from `ip` (the code's own budget
    /// is its flow's: [`Limits::code_tries_per_flow`]).
    pub fn reserve_code(&self, ip: Option<IpAddr>) -> Result<Reservation, i64> {
        let net = self.net(ip);
        let slots = self.reserve_all(&[self.own_try(&net)], net.agg)?;
        Ok(Reservation { slots, forgive: Vec::new() })
    }

    /// Reserves a password guess for `user` from `ip`.
    pub fn reserve_password(&self, user: &str, ip: Option<IpAddr>) -> Result<Reservation, i64> {
        let net = self.net(ip);
        let un = self.key("user-net", &format!("{user}\u{0}{}", net.neighbourhood()));
        let wanted = vec![(TRY_USER_NET, un), self.own_try(&net)];
        let slots = self.reserve_all(&wanted, net.agg)?;
        Ok(Reservation { slots, forgive: vec![(TRY_USER_NET, un)] })
    }

    /// Reserves a TOTP guess for `user` from `ip`.
    pub fn reserve_totp(&self, user: &str, ip: Option<IpAddr>) -> Result<Reservation, i64> {
        let net = self.net(ip);
        let u = self.key("user", user);
        let wanted = vec![(TOTP_USER, u), self.own_try(&net)];
        let slots = self.reserve_all(&wanted, net.agg)?;
        Ok(Reservation { slots, forgive: vec![(TOTP_USER, u)] })
    }

    /// Settles a reservation by the guess's outcome (see [`Outcome`]).
    pub fn settle(&self, r: Reservation, outcome: Outcome) {
        match outcome {
            Outcome::Wrong => {}
            Outcome::NoVerdict | Outcome::Right => {
                for (i, k, at) in r.slots {
                    self.tables[i].refund(k, at);
                }
                if outcome == Outcome::Right {
                    for (i, k) in r.forgive {
                        self.tables[i].clear(k);
                    }
                }
            }
        }
    }

    /// The largest table's size (tests).
    pub fn largest_table(&self) -> usize {
        self.tables.iter().map(Table::len).max().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> Option<IpAddr> {
        Some(s.parse().unwrap())
    }

    #[test]
    fn a_burst_gets_exactly_its_allowance() {
        let t = Throttles::new(Limits { totp_per_user: Backoff { free: 3, base: 10, cap: 25, window: 600 }, ..Limits::default() });
        let passed = (0..200).filter_map(|i| t.reserve_totp("user_1", ip(&format!("10.{}.0.1", i % 250))).ok()).count();
        assert_eq!(passed, 3, "counted before the guess is made");
    }

    #[test]
    fn only_a_definite_answer_settles_a_guess() {
        let t = Throttles::new(Limits { totp_per_user: Backoff { free: 2, base: 10, cap: 25, window: 600 }, ..Limits::default() });
        let r = t.reserve_totp("u", ip("1.1.1.1")).unwrap();
        t.settle(r, Outcome::NoVerdict);
        let r = t.reserve_totp("u", ip("1.1.1.1")).unwrap();
        t.settle(r, Outcome::Wrong);
        let r = t.reserve_totp("u", ip("1.1.1.1")).unwrap();
        t.settle(r, Outcome::Wrong);
        assert!(t.reserve_totp("u", ip("1.1.1.1")).is_err(), "two wrong: locked");
        let t2 = Throttles::new(t.limits);
        let r = t2.reserve_totp("u", ip("1.1.1.1")).unwrap();
        t2.settle(r, Outcome::Wrong);
        let r = t2.reserve_totp("u", ip("1.1.1.1")).unwrap();
        t2.settle(r, Outcome::Right);
        for _ in 0..2 {
            let _ = t2.reserve_totp("u", ip("1.1.1.1")).expect("forgiven");
        }
    }

    #[test]
    fn networks_are_aggregated_at_three_levels() {
        let t = Throttles::new(Limits::default());
        let a = t.net(ip("2001:db8:1:2::1"));
        assert_eq!(a.levels, ["2001:db8:1:2::/64", "2001:db8:1::/48", "2001:db8::/32"].map(String::from));
        assert_eq!(t.net(ip("192.0.2.77")).levels, ["192.0.2.77/32", "192.0.2.0/24", "192.0.0.0/16"].map(String::from));
        assert_eq!(a.agg(), t.net(ip("2001:db8:1:ffff::9")).agg(), "one neighbourhood");
        assert_ne!(a.agg(), t.net(ip("2001:db8:2::1")).agg());
    }

    #[test]
    fn a_thousand_slash_64s_share_their_provider_s_budget_and_others_are_untouched() {
        let limits = Limits { send_per_client: [Rate { max: 20, window: 600 }, Rate { max: 50, window: 600 }, Rate { max: 80, window: 600 }], ..Limits::default() };
        let t = Throttles::new(limits);
        let sent = (0..1000u32).filter(|i| t.send(&format!("x{i}@junk.example"), ip(&format!("2001:db8:1:{:x}::1", i))).allowed()).count();
        assert_eq!(sent, 50, "one /48's budget");
        assert!(t.send("owner@example.com", ip("203.0.113.5")).allowed(), "an unrelated network is untouched");
    }

    #[test]
    fn the_owner_s_network_keeps_its_own_sends() {
        let t = Throttles::new(Limits::default());
        t.signed_in("owner@x", ip("203.0.113.5"));
        for i in 0..10 {
            let _ = t.send("owner@x", ip(&format!("198.51.{i}.66")));
        }
        assert!(!t.send("owner@x", ip("192.0.2.1")).allowed(), "strangers' day is used up");
        assert!(t.send("owner@x", ip("203.0.113.9")).allowed(), "the owner's neighbourhood is not a stranger");
    }

    #[test]
    fn a_full_table_evicts_the_flooder_s_unlocked_keys_and_admits_newcomers() {
        let t = Table::rate(Rate { max: 2, window: 600 }, 1000);
        // A lock somebody earned.
        let locked = [7u8; 32];
        t.take(locked, 99).unwrap();
        t.take(locked, 99).unwrap();
        assert!(t.take(locked, 99).is_err());
        // A flood of fresh keys from one neighbourhood.
        for i in 0..100_000u64 {
            let mut k = [0u8; 32];
            k[..8].copy_from_slice(&i.to_le_bytes());
            let _ = t.take(k, 1);
        }
        assert!(t.len() <= 1000);
        assert!(t.take(locked, 99).is_err(), "the lock survived");
        assert!(t.take([9u8; 32], 5).is_ok(), "a newcomer is served");
    }

    #[test]
    fn a_hundred_thousand_key_flood_leaves_new_visitors_served() {
        // AUTH-25 N1, its probe: 100,050 clients from distinct /64s (in
        // distinct /48s) fill the flow tables; a fresh visitor still starts.
        let t = Throttles::new(Limits { table_cap: 10_000, ..Limits::default() });
        for i in 0..100_050u64 {
            let _ = t.flow(ip(&format!("2001:db8:{:x}:{:x}::1", (i >> 16) & 0xffff, i & 0xffff)));
        }
        assert!(t.largest_table() <= 10_000);
        assert!(t.flow(ip("192.0.2.200")).allowed(), "a new visitor is served");
        assert!(t.send("someone@example.com", ip("198.51.100.7")).allowed());
    }

    #[test]
    fn churn_cannot_push_out_an_owner_s_remembered_network() {
        // AUTH-26 R3, its probe: an owner signs in, then a flood of
        // sign-ins of the attacker's own addresses from spread networks.
        let t = Throttles::new(Limits { table_cap: 64, ..Limits::default() });
        t.signed_in("owner@x", ip("203.0.113.5"));
        std::thread::sleep(std::time::Duration::from_millis(1100));
        for i in 0..150u32 {
            t.signed_in(&format!("a{i}@evil"), ip(&format!("10.{}.{}.1", i % 200, i / 200)));
        }
        assert!(t.known("owner@x", ip("203.0.113.9")), "the owner's network is remembered");
    }

    #[test]
    fn guesses_count_at_the_client_s_own_network_only() {
        // AUTH-26 R1: 20 clients of one /16 spend their guesses; a bystander
        // elsewhere in that /16 can still guess (once).
        let t = Throttles::new(Limits::default());
        for i in 0..20u32 {
            for _ in 0..40 {
                if let Ok(r) = t.reserve_code(ip(&format!("10.7.{i}.1"))) {
                    t.settle(r, Outcome::Wrong);
                }
            }
        }
        assert!(t.reserve_code(ip("10.7.200.5")).is_ok());
        for i in 0..20u32 {
            for _ in 0..40 {
                if let Ok(r) = t.reserve_code(ip(&format!("2001:db8:{i:x}::1"))) {
                    t.settle(r, Outcome::Wrong);
                }
            }
        }
        assert!(t.reserve_code(ip("2001:db8:ff00:1::7")).is_ok());
    }

    #[test]
    fn an_idle_entry_goes_by_its_table_s_own_window() {
        let t = Table::rate(Rate { max: 1, window: 1 }, 16);
        for i in 0..16u8 {
            let _ = t.take([i; 32], 1);
        }
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let _ = t.take([200u8; 32], 2);
        assert_eq!(t.len(), 1, "everything past its 1 s window went");
    }
}
