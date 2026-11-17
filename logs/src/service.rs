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

use crate::allowance::{Allowance, Unlimited};
use crate::caller::Caller;
use crate::events::{columns, Column, LogsEvent};
use crate::schema::{self, Field};
use crate::seam::{Answer, Reach};
use crate::store::{writable, LogRow, LogsStore, StoreError};
use menger_tickets_api::members::Role;
use serde_json::Value;
use std::sync::Arc;

/// The time, in unix milliseconds — a test's may be set.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// The most entries one `append` carries.
pub const APPEND_MAX: usize = 10_000;
/// The most tags one `append` gives its entries.
pub const TAGS_MAX: usize = 32;

fn system_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// The service: the store, and who decides.
pub struct LogsService {
    store: Arc<LogsStore>,
    reach: Arc<dyn Reach>,
    allowance: Arc<dyn Allowance>,
    clock: Clock,
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
        Self { store, reach, allowance: Arc::new(Unlimited), clock: Arc::new(system_ms) }
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

    /// The fields of the log `name` — a test's look.
    pub fn fields(&self, name: &str) -> Option<Vec<Field>> {
        self.store.log(name).ok().flatten().map(|l| l.fields)
    }
}
