//! What the `code` service streams. Every method returns
//! `impl Stream<Item = CodeEvent>`; a refusal is in band —
//! `refused { reason, message }` — and so is everything a read finds that
//! is not a file's text (`binary`, `too_large`, `not_found`, `not_kept`).

use serde::{Deserialize, Serialize};

/// A branch or tag and the commit it names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RefAt {
    /// Its name, `main` or `v0.8.0` (without `refs/heads/` or `refs/tags/`).
    pub name: String,
    /// The commit's full hash — for an annotated tag, the commit it points at.
    pub commit: String,
}

/// One entry of a kept tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TreeEntry {
    /// From the tree's root, `ui/src/app.svelte`.
    pub path: String,
    /// `file`, `dir`, `link` (a symbolic link, kept as its target) or
    /// `module` (a submodule: the directory `.gitmodules` names).
    pub kind: String,
    /// A file's size; 0 for anything else.
    pub bytes: u64,
    /// A link's target, as text (never followed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

/// What the `code` service streams.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CodeEvent {
    /// `open`'s answer: the commit a ref (or a hash) names, and whether it is kept.
    Opened {
        repo: String,
        /// The full hash — or, for a short hash neither kept nor a tip, the
        /// hash as asked (`fetch` reads its full hash from GitHub).
        commit: String,
        /// The branch or tag resolved (the default branch when none was
        /// named); none when a commit was asked.
        #[serde(rename = "ref")]
        reference: Option<String>,
        /// A branch's tip: it moves, and is asked of GitHub again after a minute.
        tip: bool,
        /// When the commit was made (unix seconds); none when it is not kept.
        committed_unix: Option<i64>,
        /// Whether its tree is held: `tree` and `read` answer it.
        kept: bool,
        /// Its files and their bytes; none when it is not kept.
        files: Option<u64>,
        bytes: Option<u64>,
    },
    /// `refs`' answer: every branch and tag of the listing, and the default branch.
    Refs {
        repo: String,
        default_branch: Option<String>,
        branches: Vec<RefAt>,
        tags: Vec<RefAt>,
    },
    /// `fetch`'s answer: the commit is kept; `new_bytes` were new to the
    /// store (0 when it was kept already, and nothing was asked).
    Fetched { repo: String, commit: String, files: u64, bytes: u64, new_bytes: u64 },
    /// `tree`'s answer: every entry under `path`, ordered by path; at most
    /// 20,000, `truncated` past that.
    Tree { repo: String, commit: String, path: String, entries: Vec<TreeEntry>, truncated: bool },
    /// `read`'s answer: a text file whole.
    File { path: String, bytes: u64, lines: u64, text: String },
    /// `read`: a file that is not text (a NUL in its first 8 KiB, or not UTF-8).
    Binary { path: String, bytes: u64 },
    /// `read`: a file past what `read` answers (`max`, 2 MiB).
    TooLarge { path: String, bytes: u64, max: u64 },
    /// `read`: a symbolic link, with its target (never followed).
    Link { path: String, target: String },
    /// `read` or `tree`: no such file (or directory) in the kept tree.
    NotFound { path: String },
    /// `tree`, `read` or `forget`: the commit is not kept (`fetch` it).
    NotKept { repo: String, commit: String },
    /// `kept`: one kept commit.
    Kept {
        repo: String,
        commit: String,
        files: u64,
        bytes: u64,
        committed_unix: Option<i64>,
        /// Who fetched it (a principal).
        fetched_by: Option<String>,
        fetched_unix: i64,
        /// When its tree or a file was last read (unix seconds).
        read_unix: i64,
    },
    /// `kept`'s last event: the store as a whole — its commits, the bytes
    /// on disk (each file once) and the cap.
    Store { commits: u64, bytes: u64, max_bytes: u64 },
    /// `forget`'s answer: the commit is gone, and `freed` bytes with it.
    Forgotten { repo: String, commit: String, freed: u64 },
    /// Refused, nothing kept: `not_public`, `no_such_ref`, `no_such_commit`,
    /// `rate_limited` (`retry_after_secs` when GitHub said), `too_large`
    /// (`max`, the cap passed), `bad_path`, `full`, `over_allowance` (and the
    /// quota service's other reasons), `unreachable`, `bad_archive`.
    Refused {
        reason: String,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after_secs: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max: Option<u64>,
    },
    /// The service itself failed, in words.
    Error { message: String },
}

impl CodeEvent {
    /// `refused { reason, message }`, nothing else said.
    pub fn refused(reason: &str, message: impl Into<String>) -> Self {
        CodeEvent::Refused { reason: reason.into(), message: message.into(), retry_after_secs: None, max: None }
    }
}

impl plexus_rpc::client::FromClientError for CodeEvent {
    fn from_client_error(error: plexus_rpc::client::ClientError) -> Self {
        CodeEvent::Error { message: error.to_string() }
    }
}
