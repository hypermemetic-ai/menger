//! The `code` activation — **the one declaration**, as `tickets`, `ui` and
//! `media` are: the `Activation` impl, dispatch, the schema and the
//! methods' bodies from this `impl`, and `CodeClient<T>` for a client of it.
//! Each method is its attribute, its caller and a call into [`CodeService`]
//! ([`crate::service`]), whose answer it streams.
//!
//! **Who may** (CODE-1 › grants): `open`, `refs`, `tree`, `read` and `kept`
//! a viewer's, `fetch` an editor's, `forget` the admin's — each held
//! anywhere: a kept commit holds only what GitHub shows anyone.

use crate::caller::{Caller, Callers, CodeRequest};
use crate::events::CodeEvent;
use crate::service::CodeService;
use crate::vocab::{CommitHash, RefName, RepoName, TreePath};
use futures::Stream;
use std::sync::Arc;

/// The activation: the service, and the resolver every caller is verified by.
#[derive(Clone)]
pub struct CodeActivation {
    code: Arc<CodeService>,
    callers: Callers,
}

impl CodeActivation {
    /// Over `code`.
    pub fn new(code: Arc<CodeService>) -> Self {
        Self { code, callers: Callers }
    }

    /// The service it answers from.
    pub fn code(&self) -> &Arc<CodeService> {
        &self.code
    }
}

/// `f` over the service on a blocking thread: the store's reads.
async fn blocking(code: Arc<CodeService>, f: impl FnOnce(&CodeService) -> CodeEvent + Send + 'static) -> CodeEvent {
    tokio::task::spawn_blocking(move || f(&code)).await.unwrap_or_else(|e| CodeEvent::Error { message: e.to_string() })
}

/// Public GitHub repositories, read inside menger: a ref resolved to a commit, the commit fetched once, its tree and files answered from what is kept.
#[plexus_rpc::macros::activation(
    auth = "required",
    request = CodeRequest,
    typed_params,
    roles(
        viewer = ["open", "refs", "tree", "read", "kept"],
        editor = [viewer, "fetch"],
    )
)]
impl CodeActivation {
    /// The commit a branch, tag or hash names, and whether it is kept; yields opened { repo, commit, ref, tip, committed_unix, kept, files, bytes }.
    ///
    /// `ref` in the answer is the branch or tag resolved — `refname`, or the
    /// default branch's name when none was given.
    ///
    /// With `refname` (a branch or a tag) or neither (the default branch), the
    /// server reads GitHub's git listing of the repository — at most once a
    /// minute — and answers the ref's commit, its full hash; `tip` is true
    /// for a branch, whose tip moves. With `commit`, a hash kept already is
    /// answered with nothing asked of GitHub (a short one: the kept commit
    /// it begins); one not kept is answered as given, or as the tip it
    /// begins. `kept`: whether its tree is held — `tree` and `read` answer
    /// it; `committed_unix`, `files` and `bytes` are null when it is not.
    /// Refused in band: `not_public` (GitHub shows no public repository of
    /// that name), `no_such_ref`, `rate_limited { retry_after_secs }`,
    /// `unreachable`. A `repo` that is not GitHub's `owner/name` is refused
    /// (`bad_repo`) before anything is asked. Viewer.
    #[plexus_rpc::macros::method(
        group = "read",
        example(note = "a branch's tip", params = r#"{"repo": "hypermemetic-ai/menger", "refname": "v2"}"#),
        example(note = "a commit kept already", params = r#"{"repo": "hypermemetic-ai/menger", "commit": "b0c85cd"}"#)
    )]
    pub async fn open(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The repository on GitHub, owner/name, e.g. hypermemetic-ai/menger.
        repo: RepoName,
        /// A branch or tag, e.g. main or v0.8.0 (optional: the default branch).
        refname: Option<RefName>,
        /// A commit's hash, 7 to 40 hex digits — instead of a ref, and winning over one (optional).
        commit: Option<CommitHash>,
    ) -> impl Stream<Item = CodeEvent> + Send + 'static {
        let (_, code) = (user, self.code.clone());
        futures::stream::once(async move { code.open(repo.as_str(), refname.as_ref().map(RefName::as_str), commit.as_ref().map(CommitHash::as_str)).await })
    }

    /// A repository's branches and tags, each with its commit, and its default branch; yields refs { repo, default_branch, branches, tags }.
    ///
    /// From the same listing `open` reads (at most once a minute). Each of
    /// `branches` and `tags` is `{ name, commit }`; an annotated tag's
    /// commit is the one it points at. Refused as `open` is. Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "what to open", params = r#"{"repo": "hypermemetic-ai/menger"}"#))]
    pub async fn refs(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The repository on GitHub, owner/name.
        repo: RepoName,
    ) -> impl Stream<Item = CodeEvent> + Send + 'static {
        let (_, code) = (user, self.code.clone());
        futures::stream::once(async move { code.refs(repo.as_str()).await })
    }

    /// Read a commit's whole tree from GitHub once and keep it; yields fetched { repo, commit, files, bytes, new_bytes }.
    ///
    /// One request for the commit's archive; each file is kept once by its
    /// SHA-256 across every commit and repository, so `new_bytes` is what
    /// was new to the store — and what counts against your allowance
    /// (`code.bytes`), reserved before anything is kept. A commit kept
    /// already answers at once with `new_bytes: 0`. A short hash is read
    /// in full from the archive. Refused in band, nothing kept: `too_large`
    /// (past 100 MiB compressed, 400 MiB unpacked or 50,000 entries; `max`
    /// the cap), `bad_path` (an entry that leaves the tree), `not_public`,
    /// `no_such_commit`, `rate_limited`, `full` (the store's cap, with
    /// every other commit read within the hour), `over_allowance`. Editor.
    #[plexus_rpc::macros::method(group = "write", example(note = "a branch's tip, once opened", params = r#"{"repo": "hypermemetic-ai/menger", "commit": "b0c85cddc784cb4e7d3e118a118a09a6334d783b"}"#))]
    pub async fn fetch(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The repository on GitHub, owner/name.
        repo: RepoName,
        /// The commit's hash, as `open` answered it (7 to 40 hex digits).
        commit: CommitHash,
    ) -> impl Stream<Item = CodeEvent> + Send + 'static {
        let code = self.code.clone();
        futures::stream::once(async move { code.fetch(user.principal(), repo.as_str(), commit.as_str()).await })
    }

    /// A kept commit's tree under a directory, from what is kept; yields tree { repo, commit, path, entries, truncated }.
    ///
    /// Every entry under the directory `under` (the whole tree when none —
    /// `path` in the answer), ordered by
    /// path, each `{ path, kind, bytes, target? }` — `kind` `file`, `dir`,
    /// `link` (its `target` as text, never followed) or `module` (a
    /// submodule); at most 20,000, `truncated` past that. Nothing is asked
    /// of GitHub. `not_kept { repo, commit }` for a commit not held (fetch
    /// it), `not_found { path }` for no such directory. Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "a directory", params = r#"{"repo": "hypermemetic-ai/menger", "commit": "b0c85cd", "under": "ui/src"}"#))]
    pub async fn tree(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The repository on GitHub, owner/name.
        repo: RepoName,
        /// The kept commit's hash (7 to 40 hex digits).
        commit: CommitHash,
        /// A directory, from the tree's root, e.g. ui/src (optional: the whole tree). Not `path`: synapse makes a `--path` absolute on this machine.
        under: Option<TreePath>,
    ) -> impl Stream<Item = CodeEvent> + Send + 'static {
        let (_, code, path) = (user, self.code.clone(), under.map(TreePath::into_string).unwrap_or_default());
        futures::stream::once(blocking(code, move |c| c.tree(repo.as_str(), commit.as_str(), &path)))
    }

    /// A file of a kept commit, from what is kept; yields file { path, bytes, lines, text }, or binary, too_large, link, not_found, not_kept.
    ///
    /// The file is named by `file`, a path from the tree's root (the
    /// answer's `path`). Text of at most 2 MiB is answered whole. In band: `binary { path,
    /// bytes }` (a NUL in its first 8 KiB, or not UTF-8), `too_large {
    /// path, bytes, max }`, `link { path, target }` (a symbolic link, never
    /// followed), `not_found { path }`, `not_kept { repo, commit }`.
    /// Nothing is asked of GitHub. Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "a file", params = r#"{"repo": "hypermemetic-ai/menger", "commit": "b0c85cd", "file": "ui/routes.json"}"#))]
    pub async fn read(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The repository on GitHub, owner/name.
        repo: RepoName,
        /// The kept commit's hash (7 to 40 hex digits).
        commit: CommitHash,
        /// The file, a path from the tree's root, e.g. src/shell.rs. Not `path`: synapse makes a `--path` absolute on this machine.
        file: TreePath,
    ) -> impl Stream<Item = CodeEvent> + Send + 'static {
        let (_, code) = (user, self.code.clone());
        futures::stream::once(blocking(code, move |c| c.read(repo.as_str(), commit.as_str(), file.as_str())))
    }

    /// Every kept commit; yields kept { repo, commit, files, bytes, committed_unix, fetched_by, fetched_unix, read_unix } per commit, then store { commits, bytes, max_bytes }.
    ///
    /// The commit read longest ago first — the first to go when the store
    /// needs room (never one read in the last hour). `store`'s `bytes` is
    /// what is on disk, each file once; `max_bytes` the cap. Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "what is kept", params = r#"{}"#))]
    pub async fn kept(&self, #[from_auth(self.callers.verify)] user: Caller) -> impl Stream<Item = CodeEvent> + Send + 'static {
        let (_, code) = (user, self.code.clone());
        futures::stream::iter(code.kept())
    }

    /// Drop a kept commit, its files' bytes credited to whoever fetched them; yields forgotten { repo, commit, freed }, or not_kept.
    ///
    /// A file goes only when no other kept commit names it; `freed` is the
    /// bytes that went. Admin.
    #[plexus_rpc::macros::method(group = "write", example(note = "a commit no longer wanted", params = r#"{"repo": "hypermemetic-ai/menger", "commit": "b0c85cd"}"#))]
    pub async fn forget(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The repository on GitHub, owner/name.
        repo: RepoName,
        /// The kept commit's hash (7 to 40 hex digits).
        commit: CommitHash,
    ) -> impl Stream<Item = CodeEvent> + Send + 'static {
        let (_, code) = (user, self.code.clone());
        futures::stream::once(async move { code.forget(repo.as_str(), commit.as_str()).await })
    }
}
