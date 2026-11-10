//! The `media` activation — **the one declaration**, as `tickets` and `ui`
//! are: the `Activation` impl, dispatch, the schema and the methods'
//! bodies from this `impl`, and `MediaClient<T>` — one typed method per
//! `#[method]`, over [`crate::image`]'s types — for a client of it (the
//! shell's `GET /media/<hash>`, a test).
//!
//! The images a ticket's body shows, kept by the service's own store
//! ([`MediaStore`], `media.db`): `put` keeps one for a ticket (or for its
//! putter, with none), `get` answers its bytes. **Under login**: `get` is a
//! viewer's, answering an image only to a reader the tickets service says
//! sees a ticket it was put for (ACCESS-10); `put` is an editor's, and the
//! editor permit on the ticket is the tickets service's word, asked over
//! the seam ([`Seam`]) as the caller — never read from its store (V2-1).
//! Without a ticket the image is the putter's, and the admins' of `*`.

use crate::allowance::{Allowance, Unlimited};
use crate::caller::{Caller, Callers, MediaRequest};
use crate::image::MediaEvent;
use crate::remote::Remote;
use crate::seam::Seam;
use crate::store::MediaStore;
use crate::vocab::{FileName, MediaData, MediaHash, MediaType, UploadId, WebUrl};
use futures::Stream;
use menger_docs::DocId;
use menger_tickets_api::vocab::TicketRef;
use std::sync::Arc;

/// The activation: the store over `media.db`, the seam to the tickets
/// service, the resolver every caller is verified by — and (EDITOR-10) the
/// fetcher links are read through, the allowance every byte is asked of,
/// and the server's own cap on one upload, when it names one.
#[derive(Clone)]
pub struct MediaActivation {
    pub(crate) store: Arc<MediaStore>,
    pub(crate) seam: Seam,
    callers: Callers,
    pub(crate) remote: Remote,
    /// Shared with the hourly sweep, so an allowance set after `new` is the one it releases to.
    pub(crate) allowance: Arc<std::sync::RwLock<Arc<dyn Allowance>>>,
    pub(crate) max_upload: Option<u64>,
}

impl MediaActivation {
    /// Over `store`; every caller the one the service's gate admitted; links
    /// read under every rule of [`Remote`]; every byte allowed ([`Unlimited`])
    /// and no cap on an upload. Within a runtime, abandoned uploads are swept
    /// hourly ([`crate::streams::sweep`]).
    pub fn new(store: Arc<MediaStore>) -> Self {
        let me = Self { store, seam: Seam::default(), callers: Callers, remote: Remote::new(), allowance: Arc::new(std::sync::RwLock::new(Arc::new(Unlimited))), max_upload: None };
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            rt.spawn(crate::streams::sweeping(Arc::downgrade(&me.store), me.allowance.clone()));
        }
        me
    }

    /// EDITOR-10: the allowance every byte is asked of (V2-31's quota
    /// service, once it lands) in place of [`Unlimited`].
    pub fn with_allowance(self, allowance: Arc<dyn Allowance>) -> Self {
        *self.allowance.write().unwrap_or_else(|p| p.into_inner()) = allowance;
        self
    }

    /// The allowance asked now.
    pub(crate) fn allowance(&self) -> Arc<dyn Allowance> {
        self.allowance.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// EDITOR-10: the server's cap on one upload (`--media-max-bytes`);
    /// none: the uploader's allowance alone.
    pub fn with_max_upload(mut self, max: Option<u64>) -> Self {
        self.max_upload = max;
        self
    }

    /// EDITOR-10: links read through `remote` — a test's, letting its local
    /// server through.
    pub fn with_remote(mut self, remote: Remote) -> Self {
        self.remote = remote;
        self
    }

    /// The seam the builder attaches its running service to, once built —
    /// how a put asks the tickets service for the ticket's permit.
    pub fn seam(&self) -> &Seam {
        &self.seam
    }

    /// The store it keeps.
    pub fn store(&self) -> &Arc<MediaStore> {
        &self.store
    }
}

/// Files for tickets and docs — any file kept by its hash (one message, or in chunks for any size), and links read into cards.
#[plexus_rpc::macros::activation(
    auth = "required",
    request = MediaRequest,
    typed_params,
    roles(
        viewer = ["get", "info", "usage", "unfurl"],
        editor = [viewer, "put", "begin", "chunk", "finish", "fetch"],
    )
)]
impl MediaActivation {
    /// Keep a file in one message (any type, at most 7 MiB, base64); yields stored { hash, mime, kind, name, bytes, width, height, duplicate }.
    ///
    /// The file is named by its SHA-256 (`hash`, 64 hex digits); a body
    /// shows an image as `![name](media:<hash>)` and any other file as
    /// `[name](media:<hash>)`, and the page draws it from `/media/<hash>`
    /// by its `kind`: `image` inline, `video` and `audio` as players, `pdf`
    /// in a viewer, `file` as a card with a download. Its type is read from
    /// its own magic bytes, never from `mime` or `name` (PNG, JPEG, GIF,
    /// WebP; MP4, QuickTime, WebM; MP3, Ogg, WAV, M4A, FLAC; PDF; anything
    /// else `application/octet-stream`); an image's width and height from
    /// its header. At most 7 MiB in one message — larger files go up with
    /// `begin`, `chunk` and `finish`. Yields exactly one event: `stored` —
    /// `duplicate: true` when the same bytes were kept already — or
    /// `refused { reason, message, bytes?, max? }` (`too_large`,
    /// `truncated`, `bad_data`, `over_allowance`), with nothing kept;
    /// `not_found { ticket }` for a ticket that is none, or that the tickets
    /// service says you may not edit. V2-29: with `doc` instead (a document
    /// of the docs service), your editor permit on it is asked of `docs.may`
    /// as you, the file answered to whoever reads that document, and
    /// `not_found { doc }` for one you may not write. The bytes count against your
    /// allowance. Through synapse the data is a file (`--data @shot.b64`)
    /// or stdin, never `"$(base64 …)"`. Editor.
    #[plexus_rpc::macros::method(
        group = "write",
        example(note = "a 1×1 PNG for a ticket", params = r#"{"ticket": "KILN-4", "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==", "name": "dot.png"}"#)
    )]
    pub async fn put(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The ticket it is for, e.g. KILN-4 (optional: with none and no doc, the file is yours).
        ticket: Option<TicketRef>,
        /// V2-29: the document it is for, e.g. 7k3mq2xa — instead of a ticket (optional).
        doc: Option<DocId>,
        /// What the client says it is (optional, a hint only: the bytes decide).
        mime: Option<MediaType>,
        /// The bytes, standard base64 (`base64 -w0 shot.png`); a `data:image/png;base64,` prefix is read past.
        data: MediaData,
        /// Its name as it came, e.g. clip.mp4 (optional).
        name: Option<FileName>,
    ) -> impl Stream<Item = MediaEvent> + Send + 'static {
        let _ = mime;
        futures::stream::once(crate::streams::put(self.clone(), user, ticket, doc, data, name))
    }

    /// A file's bytes by its hash, up to 7 MiB; yields media { hash, mime, kind, name, bytes, width, height, data } (data base64), or not_found.
    ///
    /// Yields exactly one event: `media` — the file as kept, its bytes in
    /// standard base64 — or `not_found { hash }` for a hash that names no
    /// file, and alike for one put only for tickets you cannot see
    /// (ACCESS-10: not found ≡ not yours) or put with no ticket by someone
    /// else. A file past 7 MiB is `refused { reason: too_large }`: read it
    /// at `GET /media/<hash>` on the shell, by range. Viewer.
    #[plexus_rpc::macros::method(
        group = "read",
        example(note = "an image a body shows", params = r#"{"hash": "9f2c6ae2b4d1e1a8f7c3b0d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718"}"#)
    )]
    pub async fn get(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The file's hash, as `put` answered it (a `media:` before it is read past).
        hash: MediaHash,
    ) -> impl Stream<Item = MediaEvent> + Send + 'static {
        futures::stream::once(crate::streams::get(self.clone(), user, hash))
    }

    /// What a kept file is, without its bytes; yields info { hash, mime, kind, name, bytes, width, height }, or not_found.
    ///
    /// The same answer as `get` without the bytes, for a file of any size —
    /// what the shell asks before it serves `GET /media/<hash>`, and what a
    /// page asks to draw a file's card. `not_found { hash }` alike for none
    /// and for one you may not see. Viewer.
    #[plexus_rpc::macros::method(
        group = "read",
        example(note = "what a file in a body is", params = r#"{"hash": "9f2c6ae2b4d1e1a8f7c3b0d4e5f60718293a4b5c6d7e8f90a1b2c3d4e5f60718"}"#)
    )]
    pub async fn info(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The file's hash (a `media:` before it is read past).
        hash: MediaHash,
    ) -> impl Stream<Item = MediaEvent> + Send + 'static {
        futures::stream::once(crate::streams::info(self.clone(), user, hash))
    }

    /// What you have put: yields usage { principal, files, bytes } — each file once, however many tickets it is for; an admin of * names anyone, or every keeper (then unattributed { files, bytes }).
    ///
    /// The storage that counts against your allowance: every file you put
    /// or uploaded, by its bytes. Viewer. V2-31: an admin of `*` names
    /// anyone (`principal`), or asks `everyone: true` — one `usage` per
    /// keeper, then `unattributed { files, bytes }`. V2-69: each file
    /// counted once against each principal who put or fetched it (a
    /// duplicate of another's file is charged to both), as the quota
    /// service charged it (what `quota.recount` reads); unattributed is
    /// what no one keeps — a link's preview, or a ticket's file from
    /// before uploaders were recorded.
    #[plexus_rpc::macros::method(group = "read", example(note = "my storage", params = r#"{}"#), example(note = "every uploader (admin of *)", params = r#"{"everyone": true}"#))]
    pub async fn usage(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// Whose (optional: yours; anyone else's is an admin of *'s).
        principal: Option<menger_tickets_api::vocab::Principal>,
        /// true: every keeper, then what no one is charged for (an admin of *'s).
        everyone: Option<bool>,
    ) -> impl Stream<Item = MediaEvent> + Send + 'static {
        use futures::StreamExt;
        let asked = (principal.map(|p| p.to_string()), everyone.unwrap_or(false));
        futures::stream::once(crate::streams::usage(self.clone(), user, asked)).flat_map(futures::stream::iter)
    }

    /// Start (or resume) an upload of any size in chunks; yields upload { upload, size, received, chunk }.
    ///
    /// For a file too large for `put`. Say its `size` in bytes and its
    /// `name`; the answer is the upload's id, the bytes it has (`received`,
    /// 0 for a new one) and the most one chunk may carry (`chunk`, bytes
    /// before base64). Then `chunk { upload, offset, data }` in order and
    /// `finish { upload }`. After a dropped connection, `begin { upload }`
    /// with the same id answers what it has, and the next chunk goes from
    /// there. The size is held against your allowance from the start
    /// (`refused { reason: over_allowance }` when it does not fit; the
    /// server may also cap one upload: `too_large`). An upload untouched
    /// for a day is dropped. `not_found { ticket }` for a ticket you may
    /// not edit; `not_found { upload }` to resume one that is not yours or
    /// gone. Editor.
    #[plexus_rpc::macros::method(
        group = "write",
        example(note = "a 300 MB video for a ticket", params = r#"{"ticket": "KILN-4", "name": "walkthrough.mp4", "size": 314572800}"#)
    )]
    pub async fn begin(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The ticket it is for (optional: with none and no doc, the file is yours).
        ticket: Option<TicketRef>,
        /// V2-29: the document it is for, e.g. 7k3mq2xa — instead of a ticket (optional).
        doc: Option<DocId>,
        /// Its name as it came, e.g. walkthrough.mp4 (optional).
        name: Option<FileName>,
        /// What the client says it is (optional, a hint only: the bytes decide).
        mime: Option<MediaType>,
        /// Its size in bytes (for a new upload).
        size: Option<u64>,
        /// An upload of yours to resume, by the id `begin` answered.
        upload: Option<UploadId>,
    ) -> impl Stream<Item = MediaEvent> + Send + 'static {
        let _ = mime;
        futures::stream::once(crate::streams::begin(self.clone(), user, ticket, doc, name, size, upload))
    }

    /// One chunk of an upload, at its byte offset; yields chunked { upload, received, size, duplicate }.
    ///
    /// `offset` is where the chunk starts: the `received` the last answer
    /// gave. A chunk sent again is `duplicate` and written once; one that
    /// leaves a gap is `refused { reason: out_of_order }` naming the offset
    /// expected; past the size said, `past_size`. At most `chunk` bytes
    /// (as `begin` said) of base64. Editor.
    #[plexus_rpc::macros::method(
        group = "write",
        example(note = "the first chunk", params = r#"{"upload": "0f1e2d3c4b5a69788796a5b4c3d2e1f0", "offset": 0, "data": "AAAAIGZ0eXBpc29t"}"#)
    )]
    pub async fn chunk(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The upload, as `begin` answered it.
        upload: UploadId,
        /// The byte this chunk starts at.
        offset: u64,
        /// Its bytes, standard base64.
        data: MediaData,
    ) -> impl Stream<Item = MediaEvent> + Send + 'static {
        futures::stream::once(crate::streams::chunk(self.clone(), user, upload, offset, data))
    }

    /// Finish an upload whose every byte came; yields stored { hash, mime, kind, name, bytes, width, height, duplicate }.
    ///
    /// The file is hashed, its type read from its first bytes, and kept as
    /// `put` keeps one: `stored`, or `refused { reason: incomplete }` while
    /// bytes are missing (send them, then finish again). Editor.
    #[plexus_rpc::macros::method(group = "write", example(note = "the upload done", params = r#"{"upload": "0f1e2d3c4b5a69788796a5b4c3d2e1f0"}"#))]
    pub async fn finish(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The upload, as `begin` answered it.
        upload: UploadId,
    ) -> impl Stream<Item = MediaEvent> + Send + 'static {
        futures::stream::once(crate::streams::finish(self.clone(), user, upload))
    }

    /// Read a public link into a card; yields unfurled { url, kind, title, description, site, image, provider, embed }.
    ///
    /// The server reads the page — public http(s) addresses only, never a
    /// private, loopback, link-local or metadata one (checked again at
    /// every redirect, at most 3), 5 s and 5 MiB at most — and answers its
    /// OpenGraph and Twitter tags; YouTube, Vimeo, X, GitHub and Figma by
    /// their oEmbed, the first four with a player address (`embed`) built
    /// on their own player host. The preview image is fetched and kept
    /// (`image`, a hash). `kind`: `link`, `video`, `tweet`, `repo`,
    /// `design` or `image`. Kept a day. A refusal names why (`bad_url`,
    /// `blocked_address`, `too_many_redirects`, `timeout`, `too_large`,
    /// `bad_type`, `http_status`, `unreachable`). Counts one call against
    /// your allowance. Viewer.
    #[plexus_rpc::macros::method(group = "read", example(note = "a pasted link", params = r#"{"url": "https://github.com/hypermemetic-ai/menger"}"#))]
    pub async fn unfurl(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The page's address, http:// or https://.
        url: WebUrl,
    ) -> impl Stream<Item = MediaEvent> + Send + 'static {
        futures::stream::once(crate::streams::unfurl(self.clone(), user, url))
    }

    /// Keep a remote image or file as a file of your own; yields stored, as put does.
    ///
    /// What a pasted page's images and an image dragged from another tab
    /// become: the server reads it under the same rules as `unfurl` (10 s
    /// and 7 MiB at most; a page is refused as `bad_type` — unfurl it) and
    /// keeps it as `put` would, never hot-linked. Counts one call and its
    /// bytes against your allowance. Editor.
    #[plexus_rpc::macros::method(group = "write", example(note = "an image from a pasted page", params = r#"{"url": "https://example.com/figure.png", "ticket": "KILN-4"}"#))]
    pub async fn fetch(
        &self,
        #[from_auth(self.callers.verify)] user: Caller,
        /// The file's address, http:// or https://.
        url: WebUrl,
        /// The ticket it is for (optional: with none and no doc, the file is yours).
        ticket: Option<TicketRef>,
        /// V2-29: the document it is for, e.g. 7k3mq2xa — instead of a ticket (optional).
        doc: Option<DocId>,
        /// Its name (optional: the address's last segment).
        name: Option<FileName>,
    ) -> impl Stream<Item = MediaEvent> + Send + 'static {
        futures::stream::once(crate::streams::fetch(self.clone(), user, url, ticket, doc, name))
    }
}
