//! **The meter's seam** — every byte a document's body grows by is
//! reserved before it is written and committed once it is (V2-31: the
//! quota service will meter `docs.bytes` per author; EDITOR-10 does the
//! same for images). Until that service lands, [`AllowAll`] allows every
//! write; the one call site is [`crate::crdt`]'s append.

/// A reservation the meter made, handed back at commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reserved {
    /// Whose bytes: the writer's principal.
    pub author: String,
    /// How many.
    pub bytes: u64,
}

/// What meters the docs service's writes.
pub trait Quota: Send + Sync {
    /// Reserve `bytes` of `docs.bytes` for `author`; `Err` refuses the
    /// write, in words, and nothing is written.
    fn reserve(&self, author: &str, bytes: u64) -> Result<Reserved, String>;
    /// The write the reservation was for is in the file.
    fn commit(&self, reserved: Reserved);
}

/// Every write allowed — the meter until V2-31's.
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAll;

impl Quota for AllowAll {
    fn reserve(&self, author: &str, bytes: u64) -> Result<Reserved, String> {
        Ok(Reserved { author: author.to_string(), bytes })
    }
    fn commit(&self, _reserved: Reserved) {}
}
