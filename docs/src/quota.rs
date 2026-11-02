//! **The meter's seam** — every byte a document's body grows by is
//! reserved before it is written and committed once it is (V2-31: the
//! quota service will meter `docs.bytes` per author; EDITOR-10 does the
//! same for images). Until that service lands, [`AllowAll`] allows every
//! write; the call sites are [`crate::crdt`]'s create and append.
//! V2-31: the quota service answers it (`src/quota_allowance.rs`'s
//! `DocsMetered`): `docs.bytes` per change, `docs.count` per document made.

/// A reservation the meter made, handed back at commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reserved {
    /// Whose bytes: the writer's principal.
    pub author: String,
    /// How many.
    pub bytes: u64,
    /// V2-31: the quota service's hold, to commit (none: nothing held).
    pub hold: Option<String>,
}

/// What meters the docs service's writes.
pub trait Quota: Send + Sync {
    /// Reserve `bytes` of `docs.bytes` for `author`; `Err` refuses the
    /// write, in words, and nothing is written.
    fn reserve(&self, author: &str, bytes: u64) -> Result<Reserved, String>;
    /// The write the reservation was for is in the file.
    fn commit(&self, reserved: Reserved);
    /// V2-31: one document made (`docs.count`) for `author`; refused like
    /// `reserve`. Every one allowed unless the meter says.
    fn reserve_count(&self, author: &str) -> Result<Reserved, String> {
        Ok(Reserved { author: author.to_string(), bytes: 0, hold: None })
    }
}

/// Every write allowed — the meter until V2-31's.
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAll;

impl Quota for AllowAll {
    fn reserve(&self, author: &str, bytes: u64) -> Result<Reserved, String> {
        Ok(Reserved { author: author.to_string(), bytes, hold: None })
    }
    fn commit(&self, _reserved: Reserved) {}
}
