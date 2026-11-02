//! V2-24: **the media service** — the images a ticket's body shows, as a
//! service of v2's own (V2-1: services that coexist, each reaching the
//! others as a client through the seam).
//!
//! An **image** is kept by menger, on the wire: `media.put { ticket?, mime,
//! data }` (the bytes in base64) keeps it content-addressed — its SHA-256,
//! in hex, is its name — and `media.get { hash }` answers the bytes; the
//! shell serves the same bytes at `GET /media/<hash>` to a signed-in
//! session ([`fetch`]). A body names one as `![alt](media:<hash>)`. It
//! belongs to the ticket it was put for, and is answered to a reader who
//! can see that ticket; the same bytes put twice are one image (one row),
//! put for a second ticket they belong to both; put with no ticket (what
//! the editor pastes before a ticket exists) they are the putter's.
//!
//! **Whose it is, asked over the seam.** This service never opens the
//! tickets file: a put for a ticket asks the tickets service `tickets.may {
//! id, role: editor }` as the caller, through plexus-core's in-memory
//! transport ([`Seam`]), and a get asks `tickets.may { id, role: viewer }`
//! for a ticket the image was put for — the same route and gate a socket
//! reaches, the tickets service deciding by the caller's reach.
//!
//! **What is kept**: PNG, JPEG, GIF and WebP, each recognised by its magic
//! bytes ([`image::sniff`]) — never by the name or the type a caller says —
//! and never SVG; a declared type that is not what the bytes are is
//! refused; at most [`image::MAX_BYTES`] each. In its own SQLite file,
//! `media.db` ([`MEDIA_DB_FILE`]; `--media-db-path`), never the tickets
//! file ([`MediaStore`]).
//!
//! **EDITOR-10: any file, of any size, and links.** The service keeps any
//! file ([`kinds`]: its type and how it draws, read from its bytes), in one
//! message or in resumable chunks; its bytes live behind one trait,
//! [`BlobStore`] (the disk today, [`DiskStore`]; an object store later),
//! and every byte is asked of the uploader's [`Allowance`] first. A public
//! link is read into a card ([`unfurl`]) and a remote file kept
//! ([`unfurl::fetch`]), through [`Remote`] and its SSRF rules.
//!
//! | module | what |
//! |---|---|
//! | [`kinds`] | any file's type and kind by its bytes; the caps; the served-inline rule |
//! | [`blob`] | [`BlobStore`], the bytes' one trait; [`DiskStore`] |
//! | [`allowance`] | [`Allowance`], the one call site for the quota service; [`Unlimited`] |
//! | [`remote`] | [`Remote`]: what the server may fetch, and how |
//! | [`unfurl`] | a link's card and its providers; `fetch` |
//! | [`image`] | the vocabulary: the kept types, the cap, `sniff`/`check`, [`MediaEvent`] |
//! | [`vocab`] | the typed parameters: [`MediaHash`], [`MediaType`], [`MediaData`] |
//! | [`store`] | the file: `media` and `media_owners`, `put`, `kept`, `bytes` |
//! | [`seam`] | the tickets service as a client; the shell's [`fetch`] |
//! | [`activation`] | `media.put` and `media.get`, the one declaration |

pub mod allowance;
pub mod blob;
pub use blob::{BlobError, BlobStore, DiskStore};
pub use allowance::{Allowance, Unlimited};
pub mod image;
pub mod kinds;
pub mod remote;
pub use remote::Remote;
pub mod unfurl;
pub use image::{MediaEvent, MediaInfo};
pub mod vocab;
pub use vocab::{FileName, MediaData, MediaHash, MediaType, UploadId, WebUrl};
pub mod store;
pub use store::{default_blob_dir, MediaStore, Owner, Put, StoreError};
mod caller;
pub use caller::{Caller, MediaRequest};
pub mod seam;
pub use seam::{fetch, Fetched, Seam};
pub mod activation;
pub use activation::{MediaActivation, MediaClient};
mod streams;

/// The service's file when `--media-db-path` names none: `media.db` beside
/// the tickets file, never the same file.
pub const MEDIA_DB_FILE: &str = "media.db";
