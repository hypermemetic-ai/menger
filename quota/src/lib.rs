//! V2-31: **the quota service** — per-principal allowances on every data
//! input, as a service of v2's own beside `tickets`, `ui` and `media` (V2-1:
//! services that coexist, each reaching the others as a client through the
//! seam). The owner, 2026-11-02: "we should have per user allowances for
//! just about every data source input. and be able to tabulate their total
//! storage usage. use that to measure and prevent them from exceeding their
//! cap."
//!
//! **Counted** per principal per [source](sources) — a name, a unit, a
//! window — in its own file, `quota.db` ([`QUOTA_DB_FILE`];
//! `--quota-db-path`), never the tickets file ([`Ledger`]).
//!
//! **Enforced** before a write lands: the writing service asks
//! `quota.reserve { principal, source, amount }` through the seam
//! ([`QuotaSeam`], a [`Meter`]), writes, then `commit { ticket, amount? }`
//! or `release { ticket }`; a deletion `credit`s. Over the cap the answer
//! is typed — `over_allowance { source, used, cap, asked }` — and the
//! service writes nothing. Reservations not committed expire and are swept.
//!
//! **The effective cap** of a principal on a source: their own allowance,
//! else the largest of their teams', else the default for the source, else
//! unlimited. Admins of `*` and the operator are exempt (counted, never
//! capped). An agent's key counts as itself: the tracker links no key to a
//! person (`members` holds grants and teams, nothing that says whose a key
//! is), so a key has the default of its own.
//!
//! **The defaults are data**: a fresh file takes a [plan](plan) — the
//! built-in `plans/free-1k.json`, the owner's free tier, or `--quota-plan`.
//!
//! | module | what |
//! |---|---|
//! | [`sources`] | the registry: every source, its unit and window |
//! | [`plan`] | the defaults as data; caps read (`2 GB`, `unlimited`) |
//! | [`ledger`] | the file: usage, holds, allowances; reserve/commit/release, transactional |
//! | [`meter`] | what a writing service holds: [`Meter`], [`MeterSlot`], [`Refusal`] |
//! | [`bucket`] | the write rate per principal (a token bucket) |
//! | [`disk`] | the box guard: how full the data disk is |
//! | [`seam`] | the other services as clients: teams, exemption, recount; [`QuotaSeam`] |
//! | [`activation`] | `quota.*`, the one declaration |

pub mod sources;
pub mod plan;
pub use plan::{Cap, Plan};
pub mod ledger;
pub use ledger::{Holder, Ledger, LedgerError, Who};
pub mod meter;
pub use meter::{Hold, Meter, MeterSlot, Refusal, Unmetered};
pub mod bucket;
pub use bucket::Buckets;
pub mod disk;
pub use disk::{DiskUsage, FakeDisk, Statvfs};
pub mod vocab;
pub use vocab::{QuotaEvent, SourceUse};
mod caller;
pub use caller::{Caller, QuotaRequest};
pub mod seam;
pub use seam::{QuotaSeam, Seam};
pub mod activation;
pub use activation::{QuotaActivation, QuotaClient};
pub mod streams;
pub use streams::upkeep;

/// The service's file when `--quota-db-path` names none: `quota.db` beside
/// the tickets file, never the same file.
pub const QUOTA_DB_FILE: &str = "quota.db";
