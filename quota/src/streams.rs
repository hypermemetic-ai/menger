//! What the `quota` methods do, beside the activation (its `impl` keeps only
//! the methods — MENGER-29's rule). The ledger's work runs off the runtime
//! (`spawn_blocking`); what the other services know (a principal's teams,
//! whether it is exempt, a store's count) is asked over the [`crate::Seam`].

use crate::activation::QuotaActivation;
use crate::caller::Caller;
use crate::disk::{self, BoxState};
use crate::ledger::history::{self, Whose};
use crate::ledger::{self, Holder, Ledger, LedgerError, Line, Who};
use crate::sources::{Unit, Window};
use crate::vocab::{CapText, QuotaEvent, SourceUse};
use crate::Refusal;
use futures::Stream;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

/// Off the runtime, with the ledger.
async fn blocking<T: Send + 'static>(ledger: &Arc<Ledger>, work: impl FnOnce(&Ledger) -> T + Send + 'static) -> Result<T, String> {
    let l = Arc::clone(ledger);
    tokio::task::spawn_blocking(move || work(&l)).await.map_err(|e| format!("the quota task failed: {e}"))
}

/// Who `principal` is to the ledger: exempt (the gate's word), and the
/// teams holding an allowance it is in (asked only when there are some).
pub(crate) async fn who(act: &QuotaActivation, principal: &str) -> Who {
    let exempt = act.seam.exempt(principal);
    let teams = match exempt {
        true => Vec::new(),
        false => {
            let ledger = act.ledger.clone();
            let with = blocking(&ledger, |l| l.teams_with_allowances()).await.ok().and_then(Result::ok).unwrap_or_default();
            match with.is_empty() {
                true => Vec::new(),
                false => act.seam.teams_of(principal, &with).await,
            }
        }
    };
    Who { principal: principal.to_string(), teams, exempt }
}

/// The last time the box's warning was logged (unix seconds): once an hour.
static WARNED: AtomicI64 = AtomicI64::new(0);

/// The data disk now, and a log line for admins past the warn line.
fn box_now(act: &QuotaActivation) -> (Option<f64>, BoxState) {
    let lines = act.ledger.settings().box_lines;
    let used = act.disk.used_percent();
    let state = disk::state(used, lines);
    if matches!(state, BoxState::Warn | BoxState::Full) {
        let now = ledger::now();
        let last = WARNED.load(Ordering::Relaxed);
        if now - last >= 3600 && WARNED.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
            tracing::warn!(
                "V2-31: the data disk is {:.0}% full — capped writes are refused from {:.0}% (box_full); admins: make room or grow the disk",
                used.unwrap_or(0.0),
                lines.full_percent
            );
        }
    }
    (used, state)
}

/// KEEL-56: the seam's four — `reserve`, `commit`, `release`, `credit` —
/// move anyone's allowance, so they are an admin of `*`'s (the server's own
/// caller, which every service's seam asks as), whatever the gate admitted.
fn the_seams(act: &QuotaActivation, user: &Caller, what: &str) -> Option<QuotaEvent> {
    (!act.seam.admin_everywhere(user.context())).then(|| admins_only(what))
}

pub(crate) async fn reserve(act: QuotaActivation, user: Caller, principal: String, source: String, amount: u64) -> QuotaEvent {
    if let Some(refused) = the_seams(&act, &user, "a reservation against an allowance") {
        return refused;
    }
    let who = who(&act, &principal).await;
    let (used, _) = box_now(&act);
    let now = ledger::now();
    match blocking(&act.ledger, move |l| l.reserve(&who, &source, amount, now, used)).await {
        Ok(Ok(r)) => QuotaEvent::Reserved { ticket: r.ticket, remaining: r.remaining, expires: r.expires },
        Ok(Err(refusal)) => refusal.into(),
        Err(message) => QuotaEvent::Error { message },
    }
}

pub(crate) async fn commit(act: QuotaActivation, user: Caller, ticket: String, amount: Option<u64>) -> QuotaEvent {
    if let Some(refused) = the_seams(&act, &user, "counting a reservation") {
        return refused;
    }
    let t = ticket.clone();
    match blocking(&act.ledger, move |l| l.commit(&t, amount)).await {
        Ok(Ok(c)) => QuotaEvent::Committed { ticket, principal: c.principal, source: c.source, amount: c.amount, used: c.used },
        Ok(Err(e)) => QuotaEvent::Error { message: e.to_string() },
        Err(message) => QuotaEvent::Error { message },
    }
}

pub(crate) async fn release(act: QuotaActivation, user: Caller, ticket: String) -> QuotaEvent {
    if let Some(refused) = the_seams(&act, &user, "letting a reservation go") {
        return refused;
    }
    let t = ticket.clone();
    match blocking(&act.ledger, move |l| l.release(&t)).await {
        Ok(Ok((principal, source, amount))) => QuotaEvent::Released { ticket, principal, source, amount },
        Ok(Err(e)) => QuotaEvent::Error { message: e.to_string() },
        Err(message) => QuotaEvent::Error { message },
    }
}

pub(crate) async fn credit(act: QuotaActivation, user: Caller, principal: String, source: String, amount: u64) -> QuotaEvent {
    if let Some(refused) = the_seams(&act, &user, "giving usage back") {
        return refused;
    }
    let (p, s) = (principal.clone(), source.clone());
    match blocking(&act.ledger, move |l| l.credit(&p, &s, amount, ledger::now())).await {
        Ok(Ok(used)) => QuotaEvent::Credited { principal, source, used },
        Ok(Err(e)) => QuotaEvent::Error { message: e.to_string() },
        Err(message) => QuotaEvent::Error { message },
    }
}

/// A principal's lines as the wire carries them, and what it stores.
fn usage_event(who: &Who, lines: Vec<Line>) -> QuotaEvent {
    let bytes = lines.iter().filter(|l| l.source.unit == Unit::Bytes && l.source.window == Window::Total).map(|l| l.used).sum();
    let sources = lines
        .into_iter()
        .map(|l| SourceUse {
            source: l.source.name.to_string(),
            unit: l.source.unit.as_str().to_string(),
            window: l.source.window.as_str().to_string(),
            used: l.used,
            held: l.held,
            cap: l.cap.number(),
            from: l.from.to_string(),
        })
        .collect();
    QuotaEvent::Usage { principal: who.principal.clone(), exempt: who.exempt, bytes, sources }
}

async fn usage_of(act: &QuotaActivation, principal: &str) -> QuotaEvent {
    let who = who(act, principal).await;
    let w = who.clone();
    match blocking(&act.ledger, move |l| l.usage(&w, ledger::now())).await {
        Ok(Ok(lines)) => usage_event(&who, lines),
        Ok(Err(e)) => QuotaEvent::Error { message: e.to_string() },
        Err(message) => QuotaEvent::Error { message },
    }
}

fn admins_only(what: &str) -> QuotaEvent {
    QuotaEvent::Error { message: format!("{what} is an admin of *'s") }
}

pub(crate) async fn usage(act: QuotaActivation, user: Caller, principal: Option<String>) -> QuotaEvent {
    let whose = principal.unwrap_or_else(|| user.principal().to_string());
    if whose != user.principal() && !act.seam.admin_everywhere(user.context()) {
        return admins_only("another principal's usage");
    }
    usage_of(&act, &whose).await
}

/// The key a row sorts by, most first (or by principal).
fn sort_key(ev: &QuotaEvent, sort: &str) -> u64 {
    match ev {
        QuotaEvent::Usage { bytes, sources, .. } => match sort {
            "bytes" => *bytes,
            s => sources.iter().find(|u| u.source == s).map_or(0, |u| u.used),
        },
        _ => 0,
    }
}

pub(crate) fn table(act: QuotaActivation, user: Caller, org: Option<String>, sort: Option<String>) -> impl Stream<Item = QuotaEvent> + Send + 'static {
    async_stream::stream! {
        if !act.seam.admin_everywhere(user.context()) {
            yield admins_only("the table of everyone's usage");
            return;
        }
        let lines = act.ledger.settings().box_lines;
        let (used, state) = box_now(&act);
        yield QuotaEvent::Box { used_percent: used, warn_percent: lines.warn_percent, full_percent: lines.full_percent, state };
        let principals = match org {
            Some(o) => act.seam.principals_in(&o).await,
            None => blocking(&act.ledger, |l| l.principals()).await.and_then(|r| r.map_err(|e| e.to_string())),
        };
        let principals = match principals {
            Ok(p) => p,
            Err(message) => {
                yield QuotaEvent::Error { message };
                return;
            }
        };
        let mut rows = Vec::new();
        for p in &principals {
            rows.push(usage_of(&act, p).await);
        }
        let sort = sort.unwrap_or_else(|| "bytes".into());
        match sort.as_str() {
            "principal" => {}
            s => rows.sort_by(|a, b| sort_key(b, s).cmp(&sort_key(a, s))),
        }
        let mut total: Vec<SourceUse> = Vec::new();
        let (mut bytes, mut n) = (0u64, 0u32);
        for row in rows {
            if let QuotaEvent::Usage { bytes: b, sources, .. } = &row {
                n += 1;
                bytes += b;
                for s in sources {
                    match total.iter_mut().find(|t| t.source == s.source) {
                        Some(t) => {
                            t.used += s.used;
                            t.held += s.held;
                        }
                        None => total.push(SourceUse { cap: None, from: String::new(), ..s.clone() }),
                    }
                }
            }
            yield row;
        }
        yield QuotaEvent::Total { principals: n, bytes, sources: total };
    }
}

pub(crate) async fn set(act: QuotaActivation, user: Caller, who: (Option<String>, Option<String>, bool), source: String, cap: CapText) -> QuotaEvent {
    if !act.seam.admin_everywhere(user.context()) {
        return admins_only("setting an allowance");
    }
    let holder = match who {
        (Some(p), None, false) => Holder::Principal(p),
        (None, Some(t), false) => Holder::Team(t),
        (None, None, true) => Holder::Default,
        _ => return QuotaEvent::Error { message: "name exactly one of principal, team, or default: true".into() },
    };
    let label = match &holder {
        Holder::Principal(p) => format!("principal:{p}"),
        Holder::Team(t) => format!("team:{t}"),
        Holder::Default => "default".into(),
    };
    let (c, by, s) = (cap.cap(), user.principal().to_string(), source.clone());
    match blocking(&act.ledger, move |l| l.set(&holder, &s, c, &by)).await {
        Ok(Ok(())) => {
            tracing::info!("V2-31: {label}'s allowance of {source} set to {} by {}", c.map_or("(cleared)".into(), |c| c.to_string()), user.principal());
            QuotaEvent::Allowance { holder: label, source, cap: c.and_then(|c| c.number()), cleared: c.is_none() }
        }
        Ok(Err(e)) => QuotaEvent::Error { message: e.to_string() },
        Err(message) => QuotaEvent::Error { message },
    }
}

/// The stores `recount` asks — each for every principal, as the server —
/// and which of their counts is which source: (method, its params, the
/// source its `count` is, the source its `bytes` is). Each yields `usage {
/// principal, count?, files?, bytes? }` per principal.
pub const COUNTERS: &[(&str, &str, Option<&str>, Option<&str>)] = &[
    ("tickets.revisions.usage", "{}", Some("tickets.count"), Some("tickets.bytes")),
    ("media.usage", r#"{"everyone": true}"#, None, Some("media.bytes")),
    ("docs.usage", r#"{"everyone": true}"#, Some("docs.count"), Some("docs.bytes")),
];

pub(crate) fn recount(act: QuotaActivation, user: Option<Caller>, principal: Option<String>) -> impl Stream<Item = QuotaEvent> + Send + 'static {
    async_stream::stream! {
        if let Some(u) = &user {
            if !act.seam.admin_everywhere(u.context()) {
                yield admins_only("a recount");
                return;
            }
        }
        let (mut counted_sources, mut not_counted) = (Vec::new(), Vec::new());
        let (mut seen, mut drifted) = (std::collections::BTreeSet::new(), 0u32);
        for (method, params, count_src, bytes_src) in COUNTERS {
            let counted = match act.seam.counted(method, params).await {
                Ok((c, unattributed)) => {
                    not_counted.extend(unattributed);
                    c
                }
                Err(why) => {
                    not_counted.push(format!("{method}: {why}"));
                    continue;
                }
            };
            for (src, pick) in [(count_src, 0), (bytes_src, 1)] {
                let Some(src) = src else { continue };
                let src = src.to_string();
                counted_sources.push(src.clone());
                let s = src.clone();
                let mut who: std::collections::BTreeSet<String> = blocking(&act.ledger, move |l| l.totals(&s)).await.ok().and_then(Result::ok).unwrap_or_default().into_iter().map(|(p, _)| p).collect();
                who.extend(counted.keys().cloned());
                if let Some(p) = &principal {
                    who.retain(|w| w == p);
                }
                for p in who {
                    let c = counted.get(&p).and_then(|c| if pick == 0 { c.count } else { c.bytes }).unwrap_or(0);
                    let (pp, ss) = (p.clone(), src.clone());
                    let was = match blocking(&act.ledger, move |l| l.replace_total(&pp, &ss, c)).await {
                        Ok(Ok(w)) => w,
                        Ok(Err(e)) => { yield QuotaEvent::Error { message: e.to_string() }; return; }
                        Err(message) => { yield QuotaEvent::Error { message }; return; }
                    };
                    seen.insert(p.clone());
                    if was != c {
                        drifted += 1;
                        yield QuotaEvent::Drift { principal: p, source: src.clone(), was, counted: c };
                    }
                }
            }
        }
        if drifted > 0 {
            tracing::warn!("V2-31: recount — {drifted} total(s) drifted from what the stores count, now set to the stores'");
        }
        let (backfilled, unread) = backfill(&act, principal.as_deref()).await;
        not_counted.extend(unread);
        yield QuotaEvent::Recounted { principals: seen.len() as u32, drifted, sources: counted_sources, not_counted, backfilled };
    }
}

/// V2-70: the history's past, laid back from each store by day (only the
/// days before a principal's first counted point of a source; an
/// estimate): how many points were written, and the stores that could not
/// be asked, with why.
async fn backfill(act: &QuotaActivation, only: Option<&str>) -> (u32, Vec<String>) {
    let (mut written, mut unread) = (0u32, Vec::new());
    let now = ledger::now();
    for (method, params, count_src, bytes_src) in COUNTERS {
        let rows = match act.seam.daily(method, params).await {
            Ok(r) => r,
            Err(why) => {
                unread.push(format!("{method} by day (history not backfilled): {why}"));
                continue;
            }
        };
        for (src, pick) in [(count_src, 0), (bytes_src, 1)] {
            let Some(src) = src else { continue };
            for (p, cumulative) in cumulative(&rows, pick, only) {
                let s = src.to_string();
                if let Ok(Ok(n)) = blocking(&act.ledger, move |l| l.backfill(&p, &s, &cumulative, now)).await {
                    written += n as u32;
                }
            }
        }
    }
    (written, unread)
}

/// Each principal's (day, running total) from its per-day amounts (`pick`
/// 0: the count, 1: the bytes), days ascending.
fn cumulative(rows: &[(String, i64, u64, u64)], pick: u8, only: Option<&str>) -> std::collections::BTreeMap<String, Vec<(i64, u64)>> {
    let mut by: std::collections::BTreeMap<String, std::collections::BTreeMap<i64, u64>> = std::collections::BTreeMap::new();
    for (p, day, count, bytes) in rows.iter().filter(|r| only.is_none_or(|o| o == r.0)) {
        *by.entry(p.clone()).or_default().entry(*day).or_default() += if pick == 0 { *count } else { *bytes };
    }
    by.into_iter()
        .map(|(p, days)| {
            let mut sum = 0u64;
            (p, days.into_iter().map(|(d, n)| (d, { sum += n; sum })).collect())
        })
        .collect()
}

/// V2-70: `quota.history` — a point per source per day, then `end`; another
/// principal's, or everyone's (the system's sum), an admin of `*`'s.
pub(crate) fn history(act: QuotaActivation, user: Caller, (principal, everyone): (Option<String>, bool), source: Option<String>, days: u32) -> impl Stream<Item = QuotaEvent> + Send + 'static {
    async_stream::stream! {
        if everyone && principal.is_some() {
            yield QuotaEvent::Error { message: "name a principal or everyone: true, not both".into() };
            return;
        }
        let other = everyone || principal.as_deref().is_some_and(|p| p != user.principal());
        if other && !act.seam.admin_everywhere(user.context()) {
            yield admins_only("another principal's history, or everyone's,");
            return;
        }
        let whose = match everyone {
            true => Whose::Everyone,
            false => Whose::One(principal.unwrap_or_else(|| user.principal().to_string())),
        };
        let (now, w) = (ledger::now(), whose.clone());
        match blocking(&act.ledger, move |l| l.history(&w, source.as_deref(), days, now)).await {
            Ok(Ok(points)) => {
                let (from, to) = (points.first().map(|p| p.day), points.last().map(|p| p.day));
                for p in points {
                    yield QuotaEvent::Point { day: history::date(p.day), source: p.source.to_string(), used: p.used, estimate: p.estimate };
                }
                let today = now.div_euclid(history::DAY);
                let (from, to) = (history::date(from.unwrap_or(today)), history::date(to.unwrap_or(today)));
                let principal = match whose { Whose::One(p) => Some(p), Whose::Everyone => None };
                yield QuotaEvent::End { principal, everyone, from, to, days };
            }
            Ok(Err(e)) => yield e.into(),
            Err(message) => yield QuotaEvent::Error { message },
        }
    }
}

/// The service's upkeep, for the life of the process: expired reservations
/// swept every minute; the stores recounted every night (drift logged).
pub fn upkeep(act: QuotaActivation) {
    let sweeping = act.ledger.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            match blocking(&sweeping, |l| l.sweep(ledger::now())).await {
                Ok(Ok(n)) if n > 0 => tracing::info!("V2-31: swept {n} expired quota reservation(s) (uploads abandoned)"),
                Ok(Err(e)) => tracing::warn!("V2-31: the quota sweep: {e}"),
                _ => {}
            }
        }
    });
    tokio::spawn(async move {
        use futures::StreamExt;
        loop {
            let now = ledger::now();
            let next = (now.div_euclid(86_400) + 1) * 86_400 + 3 * 3600;
            let wait = (next - now).rem_euclid(86_400).max(60);
            tokio::time::sleep(std::time::Duration::from_secs(wait as u64)).await;
            let mut s = Box::pin(recount(act.clone(), None, None));
            while let Some(ev) = s.next().await {
                if let QuotaEvent::Recounted { principals, drifted, not_counted, .. } = ev {
                    tracing::info!("V2-31: nightly recount — {principals} principal(s), {drifted} drifted; not counted: {}", if not_counted.is_empty() { "none".into() } else { not_counted.join("; ") });
                }
            }
        }
    });
}

impl From<LedgerError> for QuotaEvent {
    fn from(e: LedgerError) -> Self {
        Refusal::from(e).into()
    }
}
