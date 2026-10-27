//! IDENTITY-46: what a principal's sign-in told the server about them — one
//! `principal_profiles` row each: provider, backend, how they signed in,
//! the provider's subject, email, name, picture, organization, the last
//! sign-in. Kept so an admin sees who signed in and how after a restart
//! (the waiting list is in memory, [`crate::access::WAITING_MAX`]).
//!
//! **Written off the call** by [`crate::access::Access::verify_now`], as
//! CHAT-14's names are: [`Store::knows_profile`] reads the cache, and only
//! a caller whose identity says something the row does not is a write.
//! An identity that says less (a minted session keeps no email) keeps what
//! the row holds: each field is the last sign-in's word that named it.
//! Only the fields of [`Profile`] are kept — never the metadata whole,
//! which carries the session's key.

use super::*;
use crate::access::AdminOf;
use rusqlite::params;
use crate::members::{Profile, Scope};

const TABLE: &str = "
        CREATE TABLE IF NOT EXISTS principal_profiles (
            principal      TEXT PRIMARY KEY,
            provider       TEXT,
            backend        TEXT,
            method         TEXT,
            subject        TEXT,
            email          TEXT,
            email_verified INTEGER,
            name           TEXT,
            picture        TEXT,
            org            TEXT,
            signed_in_at   INTEGER,
            first_seen     INTEGER NOT NULL,
            updated        INTEGER NOT NULL
        );
    ";

/// The longest a kept field is (a picture's URL the longest of them).
const FIELD_MAX: usize = 2048;

/// A file without the table gains it, empty.
pub(super) fn migrate(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(TABLE)?;
    Ok(())
}

/// Every profile, as stored.
pub(super) fn load(conn: &Connection) -> anyhow::Result<HashMap<String, Profile>> {
    let mut stmt = conn.prepare(
        "SELECT principal, provider, backend, method, subject, email, email_verified, name, picture, org, signed_in_at, first_seen, updated FROM principal_profiles",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Profile {
            principal: r.get(0)?,
            provider: r.get(1)?,
            backend: r.get(2)?,
            method: r.get(3)?,
            subject: r.get(4)?,
            email: r.get(5)?,
            email_verified: r.get(6)?,
            name: r.get(7)?,
            picture: r.get(8)?,
            org: r.get(9)?,
            signed_in_at: r.get(10)?,
            first_seen: r.get(11)?,
            updated: r.get(12)?,
            holds_grant: false,
        })
    })?;
    Ok(rows.map(|r| r.map(|p| (p.principal.clone(), p))).collect::<Result<_, _>>()?)
}

/// `heard` laid over `kept`: each field the identity names, else the kept
/// one; trimmed, blanks none, each at most [`FIELD_MAX`].
fn merged(kept: Option<&Profile>, heard: &Profile) -> Profile {
    let field = |new: &Option<String>, old: Option<&Option<String>>| {
        new.as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| v.chars().take(FIELD_MAX).collect())
            .or_else(|| old.cloned().flatten())
    };
    let k = |f: fn(&Profile) -> &Option<String>| kept.map(f);
    Profile {
        principal: heard.principal.clone(),
        provider: field(&heard.provider, k(|p| &p.provider)),
        backend: field(&heard.backend, k(|p| &p.backend)),
        method: field(&heard.method, k(|p| &p.method)),
        subject: field(&heard.subject, k(|p| &p.subject)),
        email: field(&heard.email, k(|p| &p.email)),
        email_verified: heard.email_verified.or(kept.and_then(|p| p.email_verified)),
        name: field(&heard.name, k(|p| &p.name)),
        picture: field(&heard.picture, k(|p| &p.picture)),
        org: field(&heard.org, k(|p| &p.org)),
        signed_in_at: heard.signed_in_at.max(kept.and_then(|p| p.signed_in_at)),
        first_seen: kept.map_or(0, |p| p.first_seen),
        updated: kept.map_or(0, |p| p.updated),
        holds_grant: false,
    }
}

impl Store {
    /// IDENTITY-46: whether the kept profile already says everything
    /// `heard` does — no write is due.
    pub fn knows_profile(&self, heard: &Profile) -> bool {
        let cache = self.tickets.read().expect("store lock poisoned");
        let kept = cache.profiles.get(&heard.principal);
        kept.is_some_and(|k| merged(Some(k), heard) == *k)
    }

    /// IDENTITY-46: `heard` kept, laid over what is kept — when it says
    /// something new. Answers whether it wrote.
    pub async fn note_profile(&self, _sys: &System, heard: Profile) -> Result<bool, StoreError> {
        if heard.principal.trim().is_empty() || self.knows_profile(&heard) {
            return Ok(false);
        }
        let serial = self.serial().await;
        let kept = self.tickets.read().expect("store lock poisoned").profiles.get(&heard.principal).cloned();
        let mut p = merged(kept.as_ref(), &heard);
        if kept.as_ref() == Some(&p) {
            return Ok(false);
        }
        let now = chrono::Utc::now().timestamp();
        p.first_seen = kept.map_or(now, |k| k.first_seen);
        p.updated = now;
        let row = p.clone();
        self.write(
            serial,
            move |tx| {
                tx.prepare_cached(
                    "INSERT INTO principal_profiles (principal, provider, backend, method, subject, email, email_verified, name, picture, org, signed_in_at, first_seen, updated)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                     ON CONFLICT (principal) DO UPDATE SET provider = excluded.provider, backend = excluded.backend, method = excluded.method,
                       subject = excluded.subject, email = excluded.email, email_verified = excluded.email_verified, name = excluded.name,
                       picture = excluded.picture, org = excluded.org, signed_in_at = excluded.signed_in_at, updated = excluded.updated",
                )?
                .execute(params![
                    row.principal, row.provider, row.backend, row.method, row.subject, row.email, row.email_verified,
                    row.name, row.picture, row.org, row.signed_in_at, row.first_seen, row.updated
                ])?;
                Ok(())
            },
            move |cache, _| {
                cache.profiles.insert(p.principal.clone(), p);
            },
            |(), (), _| (),
        )
        .await?;
        Ok(true)
    }

    /// IDENTITY-46: every kept profile, the newest sign-in first (else the
    /// newest change), to an admin of `*` (anyone else: none — every
    /// sign-in is everyone's, whatever it came for). `holds_grant` is the
    /// caller's to fill.
    pub fn profiles(&self, admin: &AdminOf) -> Vec<Profile> {
        if *admin.scope() != Scope::Everywhere {
            return Vec::new();
        }
        let cache = self.tickets.read().expect("store lock poisoned");
        let mut all: Vec<Profile> = cache.profiles.values().cloned().collect();
        all.sort_by(|a, b| {
            let at = |p: &Profile| p.signed_in_at.unwrap_or(0).max(p.updated);
            at(b).cmp(&at(a)).then_with(|| a.principal.cmp(&b.principal))
        });
        all
    }

    /// IDENTITY-46: one principal's kept profile, to an admin of `*`.
    pub fn profile(&self, admin: &AdminOf, principal: &str) -> Option<Profile> {
        if *admin.scope() != Scope::Everywhere {
            return None;
        }
        self.tickets.read().expect("store lock poisoned").profiles.get(principal).cloned()
    }
}
