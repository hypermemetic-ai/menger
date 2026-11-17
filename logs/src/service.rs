//! **What each method does**, beside the activation (its `impl` keeps only
//! the methods — MENGER-29's rule). [`LogsService`] holds the store, the
//! reach (the tickets service, asked as the caller), the allowance and the
//! clock; a test builds one over a fake reach and a clock of its own.
//!
//! Every method asks its log's ticket first — by the uid the log keeps, so
//! a ticket moved is still its ticket, and its id now is what is answered:
//!
//! | method | the role on the log's ticket |
//! |---|---|
//! | `define`, `extend`, `deprecate` | admin |
//! | `append` | editor |
//! | `describe`, `list` | viewer — a log on a ticket the caller cannot see is not found, and not listed |
//! | `read`, `watch` (LOGS-3) | viewer — across logs, only the logs whose tickets the caller sees; a watcher is told an entry only while it sees its log's ticket (asked again at most every [`REACH_SECS`] seconds a log) |

use crate::allowance::{Allowance, Unlimited};
use crate::caller::Caller;
use crate::events::{columns, Column, LogsEvent};
use crate::schema::{self, Field};
use crate::seam::{Answer, Reach};
use crate::store::{writable, Before, EntryRow, LogRow, LogsStore, Query, StoreError};
use menger_tickets_api::members::Role;
use serde_json::Value;
use std::sync::Arc;

/// The time, in unix milliseconds — a test's may be set.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// The most entries one `append` carries.
pub const APPEND_MAX: usize = 10_000;
/// The most tags one `append` gives its entries.
pub const TAGS_MAX: usize = 32;
/// LOGS-3: a page of `read`, unless `limit` says: 100; at most 1,000.
pub const PAGE: u32 = 100;
pub const PAGE_MAX: u32 = 1_000;
/// LOGS-3: how long a watcher's answer from the tickets service is reused, a log's ticket.
pub const REACH_SECS: u64 = 5;
/// LOGS-3: how many appends a watcher may fall behind before it is told it missed some.
pub const WATCH_BUFFER: usize = 1_024;

/// LOGS-3: what an append tells its watchers: the log, its ticket's uid,
/// and the entries kept, as kept.
#[derive(Debug, Clone)]
pub struct Published {
    pub log: String,
    pub uid: String,
    pub rows: Vec<EntryRow>,
}

/// LOGS-3: what `read` is asked, each part as the call gave it.
#[derive(Debug, Clone, Default)]
pub struct ReadArgs {
    pub log: Option<String>,
    pub filter: Option<String>,
    pub tag: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub by: Option<String>,
    pub ticket: Option<String>,
    pub limit: Option<u32>,
    pub after: Option<String>,
}

/// An entry as `read` and `watch` say it.
fn entry(log: &str, r: EntryRow) -> LogsEvent {
    LogsEvent::Entry { log: log.into(), seq: r.seq, at: r.at, by: r.by, tags: r.tags, ticket: r.ticket, entry: Value::Object(r.entry) }
}

fn bad_filter(why: impl Into<String>) -> LogsEvent {
    LogsEvent::refused("bad_filter", why)
}

fn system_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// The service: the store, and who decides.
pub struct LogsService {
    store: Arc<LogsStore>,
    reach: Arc<dyn Reach>,
    allowance: Arc<dyn Allowance>,
    clock: Clock,
    published: tokio::sync::broadcast::Sender<Arc<Published>>,
}

impl std::fmt::Debug for LogsService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogsService").field("store", &self.store.path()).field("reach", &self.reach).finish()
    }
}

/// A store error as the wire says it.
pub(crate) fn said(e: StoreError) -> LogsEvent {
    let message = e.to_string();
    match e {
        StoreError::Taken(_) => LogsEvent::refused("name_taken", message),
        StoreError::NoLog(log) => LogsEvent::no_log(&log),
        StoreError::NoField { field, .. } => LogsEvent::Refused { reason: "no_field".into(), message, place: None, index: None, field: Some(field) },
        StoreError::Deprecated { field, .. } => LogsEvent::Refused { reason: "deprecated".into(), message: format!("{message} (nothing was kept)"), place: None, index: None, field },
        StoreError::Misfit { index, misfit } => LogsEvent::Refused { reason: "bad_entry".into(), message: format!("{message} (nothing of this call was kept)"), place: None, index: Some(index as u64), field: misfit.field },
        StoreError::NotGrown(m) => LogsEvent::Refused { reason: "not_grown".into(), message: format!("{message} (nothing was changed)"), place: None, index: None, field: m.field },
        StoreError::BadName(_) | StoreError::Db(_) => LogsEvent::Error { message },
    }
}

fn bad_schema(r: schema::Refused) -> LogsEvent {
    LogsEvent::Refused { reason: "bad_schema".into(), message: format!("the schema {r} (nothing was made)"), place: Some(r.place), index: None, field: None }
}

fn forbidden(what: &str, role: Role, id: &str) -> LogsEvent {
    LogsEvent::refused("forbidden", format!("{what} is the {}'s on {id}, the log's ticket", role.as_str()))
}

/// Entries as a call gives them: `entry` (one JSON object), or `entries`
/// (JSON lines, or a JSON array); each must be an object, which the check
/// says. Refused, with the index, for one that is not JSON.
pub fn entries_of(entry: Option<&str>, entries: Option<&str>) -> Result<Vec<Value>, LogsEvent> {
    let bad = |index: usize, e: serde_json::Error| LogsEvent::Refused { reason: "bad_entry".into(), message: format!("entry {index}: not JSON ({e}) (nothing of this call was kept)"), place: None, index: Some(index as u64), field: None };
    let out = match (entry, entries) {
        (Some(_), Some(_)) => return Err(LogsEvent::refused("bad_entry", "give --entry or --entries, not both")),
        (None, None) => return Err(LogsEvent::refused("bad_entry", "nothing to append: give --entry (one JSON object) or --entries (JSON lines)")),
        (Some(one), None) => vec![serde_json::from_str(one).map_err(|e| bad(0, e))?],
        (None, Some(many)) if many.trim_start().starts_with('[') => match serde_json::from_str::<Value>(many).map_err(|e| bad(0, e))? {
            Value::Array(items) => items,
            v => vec![v],
        },
        (None, Some(many)) => many.lines().filter(|l| !l.trim().is_empty()).enumerate().map(|(i, l)| serde_json::from_str(l).map_err(|e| bad(i, e))).collect::<Result<_, _>>()?,
    };
    match out.len() {
        0 => Err(LogsEvent::refused("bad_entry", "nothing to append: no entry was given")),
        n if n > APPEND_MAX => Err(LogsEvent::refused("too_many", format!("at most {APPEND_MAX} entries an append; this has {n}"))),
        _ => Ok(out),
    }
}

/// What `entries` with `tags` counts against the caller: each entry's JSON,
/// and the tags' once per entry.
pub fn bytes_of(entries: &[Value], tags: &[String]) -> u64 {
    let per_entry: usize = tags.iter().map(|t| t.len() + 3).sum::<usize>() + 2;
    entries.iter().map(|e| (e.to_string().len() + per_entry) as u64).sum()
}

impl LogsService {
    /// Over `store`, `reach` deciding: every byte allowed, the system's clock.
    pub fn new(store: Arc<LogsStore>, reach: Arc<dyn Reach>) -> Self {
        Self { store, reach, allowance: Arc::new(Unlimited), clock: Arc::new(system_ms), published: tokio::sync::broadcast::channel(WATCH_BUFFER).0 }
    }

    /// Every kept byte asked of `allowance` (the quota service's meter).
    pub fn with_allowance(mut self, allowance: Arc<dyn Allowance>) -> Self {
        self.allowance = allowance;
        self
    }

    /// The time read from `clock` (a test's).
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// The store.
    pub fn store(&self) -> &Arc<LogsStore> {
        &self.store
    }

    fn now(&self) -> i64 {
        (self.clock)()
    }

    /// `f` over the store on a blocking thread.
    async fn blocking<T: Send + 'static>(&self, f: impl FnOnce(&LogsStore) -> Result<T, StoreError> + Send + 'static) -> Result<T, StoreError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || f(&store)).await.unwrap_or_else(|e| Err(StoreError::Db(e.to_string())))
    }

    /// The log `name` and its ticket's id now, when `caller` holds `role`
    /// on that ticket; else what to answer — `not_found` for a log on a
    /// ticket the caller cannot see, as for no log at all.
    async fn reached(&self, caller: &Caller, name: &str, role: Role, what: &str) -> Result<(LogRow, String), LogsEvent> {
        let n = name.to_string();
        let log = match self.blocking(move |s| s.log(&n)).await {
            Ok(Some(l)) => l,
            Ok(None) => return Err(LogsEvent::no_log(name)),
            Err(e) => return Err(said(e)),
        };
        match self.reach.may(caller, &log.uid, role).await {
            Ok(Answer::Held(id)) => Ok((log, id)),
            Ok(Answer::NotFound) => Err(LogsEvent::no_log(name)),
            Ok(Answer::NotHeld) => match self.reach.may(caller, &log.uid, Role::Viewer).await {
                Ok(Answer::Held(id)) => Err(forbidden(what, role, &id)),
                Ok(_) => Err(LogsEvent::no_log(name)),
                Err(message) => Err(LogsEvent::Error { message }),
            },
            Err(message) => Err(LogsEvent::Error { message }),
        }
    }

    /// `logs.define`: the log `name` made from `schema` on the ticket `on`,
    /// by its admin there.
    pub async fn define(&self, caller: &Caller, name: &str, on: &str, schema: &str) -> LogsEvent {
        let fields = match schema::parse(schema) {
            Ok(f) => f,
            Err(r) => return bad_schema(r),
        };
        let seen = match self.reach.seen(caller, on).await {
            Ok(Some(s)) => s,
            Ok(None) => return LogsEvent::no_ticket(on),
            Err(message) => return LogsEvent::Error { message },
        };
        let id = match self.reach.may(caller, &seen.uid, Role::Admin).await {
            Ok(Answer::Held(id)) => id,
            Ok(Answer::NotHeld) => return forbidden("defining a log", Role::Admin, &seen.id),
            Ok(Answer::NotFound) => return LogsEvent::no_ticket(on),
            Err(message) => return LogsEvent::Error { message },
        };
        let (n, uid, text, f, by, now) = (name.to_string(), seen.uid.clone(), schema.to_string(), fields.clone(), caller.principal().to_string(), self.now());
        match self.blocking(move |s| s.define(&n, &uid, &text, &f, &by, now)).await {
            Ok(()) => LogsEvent::Defined { log: name.into(), ticket: id, uid: seen.uid, columns: columns(&fields) },
            Err(e) => said(e),
        }
    }

    /// `logs.append`: `entries` kept in the log `name`, each with `tags` and
    /// the ticket `ticket` names, by an editor of its ticket — all or none,
    /// reserved against the caller first.
    pub async fn append(&self, caller: &Caller, name: &str, entries: Vec<Value>, mut tags: Vec<String>, ticket: Option<&str>) -> LogsEvent {
        let mut once = std::collections::HashSet::new();
        tags.retain(|t| once.insert(t.clone()));
        if tags.len() > TAGS_MAX {
            return LogsEvent::refused("too_many", format!("at most {TAGS_MAX} tags an append; this has {}", tags.len()));
        }
        let (log, _) = match self.reached(caller, name, Role::Editor, "appending to a log").await {
            Ok(r) => r,
            Err(ev) => return ev,
        };
        if let Err(e) = writable(&log, &entries) {
            return said(e);
        }
        let ticket = match ticket {
            None => None,
            Some(t) => match self.reach.seen(caller, t).await {
                Ok(Some(s)) => Some(s.uid),
                Ok(None) => return LogsEvent::no_ticket(t),
                Err(message) => return LogsEvent::Error { message },
            },
        };
        let (by, bytes) = (caller.principal().to_string(), bytes_of(&entries, &tags));
        let hold = match self.allowance.reserve(&by, bytes).await {
            Ok(id) => id,
            Err(d) => return LogsEvent::refused(&d.reason, format!("{} (nothing was kept)", d.message)),
        };
        let (n, count, now) = (name.to_string(), entries.len() as u64, self.now());
        match self.blocking(move |s| s.append(&n, &entries, &tags, ticket.as_deref(), &by, now)).await {
            Ok(kept) => {
                self.allowance.commit(&hold, bytes).await;
                self.publish(name, &log.uid, kept.first, kept.last).await;
                LogsEvent::Appended { log: name.into(), seq: kept.first, last: kept.last, count, bytes }
            }
            Err(e) => {
                self.allowance.release(&hold).await;
                said(e)
            }
        }
    }

    /// `logs.describe`: the log `name`, to a caller who sees its ticket.
    pub async fn describe(&self, caller: &Caller, name: &str) -> LogsEvent {
        match self.reached(caller, name, Role::Viewer, "reading a log").await {
            Ok((log, ticket)) => LogsEvent::Described {
                schema: serde_json::from_str(&log.schema).unwrap_or(Value::Null),
                columns: columns(&log.fields),
                log: log.name,
                ticket,
                uid: log.uid,
                entries: log.entries,
                created_by: log.created_by,
                created_unix_ms: log.created_ms,
                deprecated: log.deprecated,
            },
            Err(ev) => ev,
        }
    }

    /// `logs.list`: every log whose ticket `caller` sees, by name.
    pub async fn list(&self, caller: &Caller) -> Vec<LogsEvent> {
        let logs = match self.blocking(|s| s.logs()).await {
            Ok(l) => l,
            Err(e) => return vec![said(e)],
        };
        let asked = futures::future::join_all(logs.iter().map(|l| self.reach.may(caller, &l.uid, Role::Viewer))).await;
        let mut out = Vec::new();
        for (log, answer) in logs.into_iter().zip(asked) {
            match answer {
                Ok(Answer::Held(ticket)) => out.push(LogsEvent::Log {
                    deprecated_fields: log.fields.iter().filter(|f| f.deprecated.is_some()).map(|f| f.name.clone()).collect(),
                    fields: log.fields.len() as u64,
                    log: log.name,
                    ticket,
                    uid: log.uid,
                    entries: log.entries,
                    deprecated: log.deprecated,
                }),
                Ok(_) => {}
                Err(message) => return vec![LogsEvent::Error { message }],
            }
        }
        out
    }

    /// `logs.extend`: the log `name`'s schema grown to `schema`, by its
    /// ticket's admin — optional fields added, nothing else changed.
    pub async fn extend(&self, caller: &Caller, name: &str, schema: &str) -> LogsEvent {
        let next = match schema::parse(schema) {
            Ok(f) => f,
            Err(r) => return bad_schema(r),
        };
        if let Err(ev) = self.reached(caller, name, Role::Admin, "extending a log").await {
            return ev;
        }
        let (n, text) = (name.to_string(), schema.to_string());
        match self.blocking(move |s| s.extend(&n, &text, next)).await {
            Ok((now, added)) => LogsEvent::Extended { log: name.into(), added: added.iter().map(Column::from).collect(), columns: columns(&now) },
            Err(e) => said(e),
        }
    }

    /// `logs.deprecate`: the log `name` (or its `field`) closed to new
    /// entries for `message`, by its ticket's admin. No method reopens it.
    pub async fn deprecate(&self, caller: &Caller, name: &str, field: Option<&str>, message: &str) -> LogsEvent {
        if let Err(ev) = self.reached(caller, name, Role::Admin, "deprecating a log").await {
            return ev;
        }
        let (n, fd, m, by, now) = (name.to_string(), field.map(str::to_string), message.to_string(), caller.principal().to_string(), self.now());
        match self.blocking(move |s| s.deprecate(&n, fd.as_deref(), &m, &by, now)).await {
            Ok(()) => LogsEvent::Deprecated { log: name.into(), field: field.map(str::to_string), message: message.into() },
            Err(e) => said(e),
        }
    }

    /// LOGS-3: the entries `first` to `last` of the log `name`, told every
    /// watcher — read back only when one is watching.
    async fn publish(&self, name: &str, uid: &str, first: u64, last: u64) {
        if self.published.receiver_count() == 0 {
            return;
        }
        let n = name.to_string();
        match self.blocking(move |s| s.kept(&n, first, last)).await {
            Ok(rows) => {
                let _ = self.published.send(Arc::new(Published { log: name.into(), uid: uid.into(), rows }));
            }
            Err(e) => tracing::warn!("logs:      {name} {first}–{last} kept, and not told its watchers: {e}"),
        }
    }

    /// LOGS-3: `logs.read` — one log's entries (newest first), or every
    /// visible log's (newest first by `at`), as `args` filters them, a page
    /// at a time; then `page { count, next? }`.
    pub async fn read(&self, caller: &Caller, args: ReadArgs) -> Vec<LogsEvent> {
        let limit = args.limit.unwrap_or(PAGE).clamp(1, PAGE_MAX) as usize;
        let filter = match args.filter.as_deref().map(crate::filter::parse).transpose() {
            Ok(f) => f,
            Err(why) => return vec![bad_filter(format!("--filter: {why}"))],
        };
        let now = self.now();
        let at = |w: &Option<String>| w.as_deref().and_then(|w| crate::vocab::when_ms(w, now));
        let ticket = match args.ticket.as_deref() {
            None => None,
            Some(t) => match self.reach.seen(caller, t).await {
                Ok(Some(s)) => Some(s.uid),
                Ok(None) => return vec![LogsEvent::no_ticket(t)],
                Err(message) => return vec![LogsEvent::Error { message }],
            },
        };
        let q = Query { since: at(&args.since), until: at(&args.until), by: args.by, ticket, tag: args.tag, filter, whose: String::new(), envelope_only: args.log.is_none(), limit };
        match args.log.as_deref() {
            Some(name) => self.read_one(caller, name, q, args.after.as_deref()).await,
            None => self.read_across(caller, q, args.after.as_deref()).await,
        }
    }

    async fn read_one(&self, caller: &Caller, name: &str, mut q: Query, after: Option<&str>) -> Vec<LogsEvent> {
        let before = match after.map(|a| a.parse::<u64>()) {
            None => Before::Start,
            Some(Ok(seq)) => Before::Seq(seq),
            Some(Err(_)) => return vec![LogsEvent::refused("bad_cursor", "this `after` is a page across logs; give back the `next` a read of this log answered")],
        };
        let log = match self.reached(caller, name, Role::Viewer, "reading a log").await {
            Ok((log, _)) => log,
            Err(ev) => return vec![ev],
        };
        q.whose = format!("the log `{name}`");
        let limit = q.limit;
        let rows = match self.blocking(move |s| s.read(&log, &q, &before)).await {
            Ok(Ok(rows)) => rows,
            Ok(Err(why)) => return vec![bad_filter(format!("--filter: {why}"))],
            Err(e) => return vec![said(e)],
        };
        let next = (rows.len() == limit).then(|| rows.last().map(|r| r.seq.to_string())).flatten();
        let count = rows.len() as u64;
        let mut out: Vec<LogsEvent> = rows.into_iter().map(|r| entry(name, r)).collect();
        out.push(LogsEvent::Page { count, next });
        out
    }

    /// The logs `caller` sees: each, by name.
    async fn visible(&self, caller: &Caller) -> Result<Vec<LogRow>, LogsEvent> {
        let logs = self.blocking(|s| s.logs()).await.map_err(said)?;
        let asked = futures::future::join_all(logs.iter().map(|l| self.reach.may(caller, &l.uid, Role::Viewer))).await;
        let mut out = Vec::new();
        for (log, answer) in logs.into_iter().zip(asked) {
            match answer {
                Ok(Answer::Held(_)) => out.push(log),
                Ok(_) => {}
                Err(message) => return Err(LogsEvent::Error { message }),
            }
        }
        Ok(out)
    }

    async fn read_across(&self, caller: &Caller, mut q: Query, after: Option<&str>) -> Vec<LogsEvent> {
        let before = match after.map(|a| a.split('.').collect::<Vec<_>>()) {
            None => Before::Start,
            Some(p) if p.len() == 3 => match (p[0].parse(), p[1].parse()) {
                (Ok(at), Ok(seq)) => Before::At { at, log: p[2].to_string(), seq },
                _ => return vec![LogsEvent::refused("bad_cursor", "give back the `next` a read across logs answered")],
            },
            Some(_) => return vec![LogsEvent::refused("bad_cursor", "this `after` is one log's page; give back the `next` a read across logs answered")],
        };
        let logs = match self.visible(caller).await {
            Ok(l) => l,
            Err(ev) => return vec![ev],
        };
        q.whose = "the envelope — a read across logs filters on seq, at, by and ticket alone; name a log to filter on its fields".into();
        let limit = q.limit;
        let read = self.blocking(move |s| {
            let mut all: Vec<(String, EntryRow)> = Vec::new();
            for log in &logs {
                match s.read(log, &q, &before)? {
                    Ok(rows) => all.extend(rows.into_iter().map(|r| (log.name.clone(), r))),
                    Err(why) => return Ok(Err(why)),
                }
            }
            all.sort_by(|(la, a), (lb, b)| b.at.cmp(&a.at).then(la.cmp(lb)).then(b.seq.cmp(&a.seq)));
            all.truncate(limit);
            Ok(Ok(all))
        });
        let rows = match read.await {
            Ok(Ok(rows)) => rows,
            Ok(Err(why)) => return vec![bad_filter(format!("--filter: {why}"))],
            Err(e) => return vec![said(e)],
        };
        let next = (rows.len() == limit).then(|| rows.last().map(|(l, r)| format!("{}.{}.{l}", r.at, r.seq))).flatten();
        let count = rows.len() as u64;
        let mut out: Vec<LogsEvent> = rows.into_iter().map(|(l, r)| entry(&l, r)).collect();
        out.push(LogsEvent::Page { count, next });
        out
    }

    /// LOGS-3: `logs.watch` — `watching`, then each entry appended from now
    /// on to `log` (every log when none) carrying `tag` (any when none),
    /// told `caller` only while it sees the log's ticket; `lagged` when it
    /// fell behind. Open until the caller closes it.
    pub fn watch(self: Arc<Self>, caller: Caller, log: Option<String>, tag: Option<String>) -> impl futures::Stream<Item = LogsEvent> + Send + 'static {
        async_stream::stream! {
            // Subscribed before anything is asked: nothing appended after
            // `watching` is said goes by unseen.
            let mut rx = self.published.subscribe();
            if let Some(name) = &log {
                if let Err(ev) = self.reached(&caller, name, Role::Viewer, "reading a log").await {
                    yield ev;
                    return;
                }
            }
            yield LogsEvent::Watching { log: log.clone(), tag: tag.clone() };
            let mut sees: std::collections::HashMap<String, (std::time::Instant, bool)> = std::collections::HashMap::new();
            loop {
                let p = match rx.recv().await {
                    Ok(p) => p,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                        yield LogsEvent::Lagged { missed };
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                if log.as_ref().is_some_and(|l| *l != p.log) {
                    continue;
                }
                let rows: Vec<&EntryRow> = p.rows.iter().filter(|r| tag.as_ref().is_none_or(|t| r.tags.contains(t))).collect();
                if rows.is_empty() {
                    continue;
                }
                let fresh = sees.get(&p.uid).filter(|(at, _)| at.elapsed().as_secs() < REACH_SECS).map(|(_, ok)| *ok);
                let ok = match fresh {
                    Some(ok) => ok,
                    None => {
                        let ok = matches!(self.reach.may(&caller, &p.uid, Role::Viewer).await, Ok(Answer::Held(_)));
                        sees.insert(p.uid.clone(), (std::time::Instant::now(), ok));
                        ok
                    }
                };
                if ok {
                    for r in rows {
                        yield entry(&p.log, r.clone());
                    }
                }
            }
        }
    }

    /// The fields of the log `name` — a test's look.
    pub fn fields(&self, name: &str) -> Option<Vec<Field>> {
        self.store.log(name).ok().flatten().map(|l| l.fields)
    }
}
