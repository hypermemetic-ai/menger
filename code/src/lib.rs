//! CODE-2: **the code service** — a public GitHub repository read inside
//! menger, as a service of v2's own beside `tickets`, `media`, `notes` and
//! `quota` (V2-1). The page's Content-Security-Policy keeps the browser off
//! GitHub and test and prod hold no checkout, so a view of code (CODE-3)
//! reads only what this service answers.
//!
//! **A ref is resolved** by GitHub's own git listing,
//! `https://github.com/<owner>/<name>.git/info/refs?service=git-upload-pack`
//! (protocol v0 pkt-lines: every branch and tag with its hash, and the
//! default branch as `symref=HEAD:refs/heads/…`), a listing reused for a
//! minute ([`github`]).
//!
//! **A commit is fetched once**, in one request,
//! `https://codeload.github.com/<owner>/<name>/tar.gz/<commit>`: its full
//! hash read from the archive's pax header (`comment=<hash>`), its entries
//! (under one top directory, dropped) unpacked under caps — 100 MiB
//! compressed, 400 MiB unpacked, 50,000 entries — and a path that leaves
//! the tree refused ([`unpack`]). Each file is kept once by its SHA-256 on
//! disk (`--code-blob-dir`), the tree as rows in the service's own file,
//! `code.db` ([`CODE_DB_FILE`]; `--code-db-path`) ([`store`]). A commit
//! never changes, so what is kept for one is right for good.
//!
//! **Every new byte is reserved** for whoever fetched it through the quota
//! seam (`code.bytes`, [`Allowance`]); the store stays under its cap
//! (`--code-max-bytes`, 5 GiB) by dropping the commit read longest ago
//! (never one read in the last hour), its files' bytes going only when no
//! kept commit names them, and credited to whoever they were reserved for.
//!
//! **The tree and a file are answered from what is kept**, and nothing
//! else: `code.tree` and `code.read` never ask GitHub.
//!
//! | module | what |
//! |---|---|
//! | [`vocab`] | the typed parameters: [`RepoName`], [`RefName`], [`CommitHash`], [`TreePath`] |
//! | [`events`] | [`CodeEvent`], what every method streams |
//! | [`github`] | the two addresses, the pkt-line listing, the archive, through media's `Remote` |
//! | [`unpack`] | a commit's archive read into a staged tree, under the caps |
//! | [`store`] | the file and the blobs: keep, evict, read, forget |
//! | [`allowance`] | [`Allowance`], the one call site for the quota service; [`Unlimited`] |
//! | [`service`] | [`CodeService`], what each method does |
//! | [`activation`] | `code.*`, the one declaration |

pub mod vocab;
pub use vocab::{CommitHash, RefName, RepoName, TreePath};
pub mod events;
pub use events::{CodeEvent, RefAt, TreeEntry};
pub mod allowance;
pub use allowance::{Allowance, Denied, Unlimited};
pub mod github;
pub use github::GitHub;
pub mod unpack;
pub use unpack::Caps;
pub mod store;
pub use store::{default_blob_dir, CodeStore};
pub mod service;
pub use service::{Clock, CodeService};
mod caller;
pub use caller::{Caller, CodeRequest};
pub mod activation;
pub use activation::{CodeActivation, CodeClient};

/// The service's file when `--code-db-path` names none: `code.db` beside
/// the tickets file, never the same file.
pub const CODE_DB_FILE: &str = "code.db";

/// The kept bytes' cap when `--code-max-bytes` names none: 5 GiB.
pub const DEFAULT_MAX_BYTES: u64 = 5 * 1024 * 1024 * 1024;
