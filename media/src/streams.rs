//! What the `media` methods do, beside the activation (its `impl` keeps
//! only the methods — MENGER-29's rule, tickets-api/tests/sizes.rs). Each
//! takes the [`Caller`] the dispatch resolved; whose a ticket is, is asked
//! of the tickets service over the [`Seam`] as that caller — never read
//! from its store. EDITOR-10: every byte taken is asked of the allowance
//! first, for the caller ([`crate::allowance`]), and committed or released
//! after.

use crate::activation::MediaActivation;
use crate::allowance::{source, Allowance};
use crate::caller::Caller;
use crate::image::MediaEvent;
use crate::kinds;
use crate::seam::{Answer, Seam};
use crate::store::{MediaStore, Owner, StoreError, UPLOAD_TTL};
use menger_docs::seam::Answer as DocAnswer;
use menger_docs::DocId;
use crate::vocab::{FileName, MediaData, MediaHash, UploadId, WebUrl};
use menger_tickets_api::members::Role;
use menger_tickets_api::vocab::TicketRef;
use std::sync::{Arc, RwLock, Weak};

/// `not_found` for the ticket `t` names.
fn no_ticket(t: &TicketRef) -> MediaEvent {
    MediaEvent::NotFound { hash: None, ticket: Some(t.to_string()), upload: None, doc: None }
}

fn no_upload(u: &str) -> MediaEvent {
    MediaEvent::NotFound { hash: None, ticket: None, upload: Some(u.to_string()), doc: None }
}

/// A store error as the wire says it.
fn said(e: StoreError, missing: impl FnOnce() -> MediaEvent) -> MediaEvent {
    match e {
        StoreError::Refused(problem) => problem.into(),
        StoreError::NotFound(_) => missing(),
        e => MediaEvent::Error { message: e.to_string() },
    }
}

/// ACCESS-11, over the seam: what a put for `ticket` takes — the caller's
/// editor permit on it, the tickets service's word (`tickets.may { id,
/// role: editor }` as the caller); a ticket out of reach, or none, is
/// answered as one that is not there. With no ticket, the file is the
/// putter's.
async fn owner(seam: &Seam, user: &Caller, ticket: Option<&TicketRef>, doc: Option<&DocId>) -> Result<Owner, MediaEvent> {
    // V2-29: for a doc, the docs service's word (`docs.may { id, role: editor }` as the caller).
    if let Some(d) = doc {
        if ticket.is_some() {
            return Err(MediaEvent::Error { message: "a file is put for a ticket or for a doc, not both (nothing was kept)".into() });
        }
        return match seam.doc_may(user.context(), d.as_str(), Role::Editor).await {
            Ok(DocAnswer::Held(id)) => Ok(Owner::Doc(id)),
            Ok(DocAnswer::NotHeld | DocAnswer::NotFound) => Err(MediaEvent::NotFound { hash: None, ticket: None, upload: None, doc: Some(d.to_string()) }),
            Err(message) => Err(MediaEvent::Error { message: format!("{message} (nothing was kept)") }),
        };
    }
    let Some(t) = ticket else {
        return Ok(Owner::App(Some(user.principal().to_string())));
    };
    match seam.may(user.context(), t.as_str(), Role::Editor).await {
        Ok(Answer::Held(id)) => Ok(Owner::Ticket(id)),
        Ok(Answer::NotHeld | Answer::NotFound) => Err(no_ticket(t)),
        Err(message) => Err(MediaEvent::Error { message: format!("{message} (nothing was kept)") }),
    }
}

/// V2-69: commits `id` — the kept file's bytes — when the put made the file
/// the putter's (`charged`: new to them, though another may keep it too),
/// else releases it.
async fn settle(allowance: &dyn Allowance, id: &str, put: Option<&crate::store::Put>) {
    match put {
        Some(p) if p.charged => allowance.commit(id, Some(p.info.bytes)).await,
        _ => allowance.release(id).await,
    }
}

/// A store's answer to a put as the wire says it.
fn stored(put: &Result<crate::store::Put, StoreError>) -> MediaEvent {
    match put {
        Ok(p) => MediaEvent::stored(p.info.clone(), p.duplicate),
        Err(e) => said(e.clone(), || MediaEvent::Error { message: "the file vanished".into() }),
    }
}

/// `media.put`: the file checked and kept by the store, for the ticket the
/// caller may edit — or, with none, as the caller's own; its bytes asked
/// of the caller's allowance first.
pub(crate) async fn put(me: MediaActivation, user: Caller, ticket: Option<TicketRef>, doc: Option<DocId>, data: MediaData, name: Option<FileName>) -> MediaEvent {
    let on = match owner(&me.seam, &user, ticket.as_ref(), doc.as_ref()).await {
        Ok(on) => on,
        Err(ev) => return ev,
    };
    let data = data.into_string();
    // base64's size, a bound on the bytes: the reservation is for no more.
    let bound = (data.len() as u64 / 4 + 1) * 3;
    let allowance = me.allowance();
    let held = match allowance.reserve(user.principal(), source::BYTES, bound.min(kinds::PUT_MAX as u64)).await {
        Ok(r) => r,
        Err(d) => return d.into(),
    };
    let put = me.store.put(on, data, name.map(FileName::into_string), Some(user.principal().to_string())).await;
    settle(allowance.as_ref(), &held.id, put.as_ref().ok()).await;
    stored(&put)
}

/// ACCESS-10, over the seam: whether `user` sees a file kept for `owners`
/// — a ticket it was put for that the tickets service says the caller sees
/// (`tickets.may { id, role: viewer }` as the caller), or, put with no
/// ticket, its putter, and anyone holding `media.get` on `*`; a public
/// page's preview, anyone signed in.
async fn sees(seam: &Seam, user: &Caller, owners: &[Owner]) -> Result<bool, String> {
    for owner in owners {
        let seen = match owner {
            Owner::Anyone => true,
            Owner::App(p) => p.as_deref() == Some(user.principal()) || seam.held_everywhere(user.context(), "media.get") == Some(true),
            Owner::Ticket(id) => matches!(seam.may(user.context(), id, Role::Viewer).await?, Answer::Held(_)),
            Owner::Doc(id) => matches!(seam.doc_may(user.context(), id, Role::Viewer).await?, DocAnswer::Held(_)),
        };
        if seen {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The file `hash` names, if `user` may see it — else the one answer for
/// none and for not theirs.
async fn visible(store: &Arc<MediaStore>, seam: &Seam, user: &Caller, hash: &MediaHash) -> Result<crate::MediaInfo, MediaEvent> {
    let not_found = || MediaEvent::no_hash(hash.as_str());
    let kept = match store.kept(hash.as_str()).await {
        Ok(k) => k,
        Err(e) => return Err(said(e, not_found)),
    };
    match sees(seam, user, &kept.owners).await {
        Ok(true) => Ok(kept.info),
        Ok(false) => Err(not_found()),
        Err(message) => Err(MediaEvent::Error { message }),
    }
}

/// `media.get`: the file and its bytes, to a reader who sees a ticket it is
/// for; `not_found` alike for none and for one out of reach; past one
/// message, `too_large` (read it by range at `/media/<hash>`).
pub(crate) async fn get(me: MediaActivation, user: Caller, hash: MediaHash) -> MediaEvent {
    use base64::Engine;
    let info = match visible(&me.store, &me.seam, &user, &hash).await {
        Ok(i) => i,
        Err(ev) => return ev,
    };
    if info.bytes > kinds::PUT_MAX as u64 {
        return MediaEvent::Refused {
            reason: "too_large".into(),
            message: format!("the file is {}; get answers at most {} — read it at /media/{}", crate::image::size(info.bytes), crate::image::size(kinds::PUT_MAX as u64), info.hash),
            bytes: Some(info.bytes),
            max: Some(kinds::PUT_MAX as u64),
        };
    }
    match me.store.read_at(hash.as_str(), 0, info.bytes as usize).await {
        Ok(bytes) => {
            let data = base64::engine::general_purpose::STANDARD.encode(bytes);
            let crate::image::MediaInfo { hash, mime, bytes, width, height, kind, name } = info;
            MediaEvent::Media { hash, mime, bytes, width, height, kind, name, data }
        }
        Err(e) => said(e, || MediaEvent::no_hash(hash.as_str())),
    }
}

/// `media.info`: what the file is, to a reader who may see it.
pub(crate) async fn info(me: MediaActivation, user: Caller, hash: MediaHash) -> MediaEvent {
    match visible(&me.store, &me.seam, &user, &hash).await {
        Ok(i) => MediaEvent::info(i),
        Err(ev) => ev,
    }
}

/// `media.usage`: what the caller has put.
pub(crate) async fn usage(me: MediaActivation, user: Caller, (principal, everyone, daily): (Option<String>, bool, bool)) -> Vec<MediaEvent> {
    // V2-31: another's, or every uploader's, is an admin's of `*` — asked as
    // a grant no role but admin holds (the quota service's own).
    let mine = principal.as_deref().is_none_or(|p| p == user.principal()) && !everyone;
    if !mine && me.seam.held_everywhere(user.context(), "quota.set") != Some(true) {
        return vec![MediaEvent::Error { message: "another's usage, or every uploader's, is an admin of *'s".into() }];
    }
    if everyone && daily {
        return match me.store.usage_daily().await {
            Ok(rows) => rows.into_iter().map(|(principal, day, files, bytes)| MediaEvent::UsageDay { principal, day, files, bytes }).collect(),
            Err(e) => vec![MediaEvent::Error { message: e.to_string() }],
        };
    }
    if everyone {
        return match (me.store.usage_charged().await, me.store.unattributed().await) {
            (Ok(rows), Ok((files, bytes))) => {
                let mut out: Vec<MediaEvent> = rows.into_iter().map(|(principal, files, bytes)| MediaEvent::Usage { principal, files, bytes }).collect();
                out.push(MediaEvent::Unattributed { files, bytes });
                out
            }
            (Err(e), _) | (_, Err(e)) => vec![MediaEvent::Error { message: e.to_string() }],
        };
    }
    let whose = principal.unwrap_or_else(|| user.principal().to_string());
    vec![match me.store.usage(&whose).await {
        Ok((files, bytes)) => MediaEvent::Usage { principal: whose, files, bytes },
        Err(e) => MediaEvent::Error { message: e.to_string() },
    }]
}

/// `media.begin`: a new upload, its size held against the caller's
/// allowance — or the caller's own upload again, from what it has.
pub(crate) async fn begin(me: MediaActivation, user: Caller, ticket: Option<TicketRef>, doc: Option<DocId>, name: Option<FileName>, size: Option<u64>, upload: Option<UploadId>) -> MediaEvent {
    let answer = |b: crate::store::Begun| MediaEvent::Upload { upload: b.upload, size: b.size, received: b.received, chunk: kinds::CHUNK_MAX as u64 };
    if let Some(u) = upload {
        return match me.store.resume(u.as_str(), user.principal()).await {
            Ok(b) => answer(b),
            Err(e) => said(e, || no_upload(u.as_str())),
        };
    }
    let Some(size) = size else {
        return MediaEvent::refused("bad_data", "say the file's size (or the upload to resume)");
    };
    let on = match owner(&me.seam, &user, ticket.as_ref(), doc.as_ref()).await {
        Ok(on) => on,
        Err(ev) => return ev,
    };
    if let Some(max) = me.max_upload.filter(|m| size > *m) {
        return crate::image::Problem::TooLarge { bytes: size, max }.into();
    }
    let allowance = me.allowance();
    let held = match allowance.reserve(user.principal(), source::BYTES, size).await {
        Ok(r) => r,
        Err(d) => return d.into(),
    };
    match me.store.begin(on, name.map(FileName::into_string), size, me.max_upload, user.principal().to_string(), Some(held.id.clone())).await {
        Ok(b) => answer(b),
        Err(e) => {
            allowance.release(&held.id).await;
            said(e, || MediaEvent::Error { message: "the upload could not begin".into() })
        }
    }
}

/// `media.chunk`: the next bytes of the caller's upload.
pub(crate) async fn chunk(me: MediaActivation, user: Caller, upload: UploadId, offset: u64, data: MediaData) -> MediaEvent {
    let bytes = match crate::image::decode(data.as_str()) {
        Ok(b) => b,
        Err(p) => return p.into(),
    };
    if bytes.len() > kinds::CHUNK_MAX {
        return crate::image::Problem::TooLarge { bytes: bytes.len() as u64, max: kinds::CHUNK_MAX as u64 }.into();
    }
    match me.store.chunk(upload.as_str(), user.principal(), offset, bytes).await {
        Ok(c) => MediaEvent::Chunked { upload: upload.into_string(), received: c.received, size: c.size, duplicate: c.duplicate },
        Err(e) => said(e, || no_upload(upload.as_str())),
    }
}

/// `media.finish`: the caller's upload kept; its reservation committed when
/// the bytes are new, released when they were kept already or refused.
pub(crate) async fn finish(me: MediaActivation, user: Caller, upload: UploadId) -> MediaEvent {
    let allowance = me.allowance();
    match me.store.finish(upload.as_str(), user.principal()).await {
        Ok(done) => {
            if let Some(id) = &done.reservation {
                settle(allowance.as_ref(), id, Some(&done.put)).await;
            }
            MediaEvent::stored(done.put.info, done.put.duplicate)
        }
        // An incomplete upload keeps its reservation; any other refusal
        // dropped the upload, and so its hold.
        Err(StoreError::Refused(p @ crate::image::Problem::Incomplete { .. })) => p.into(),
        Err(e) => said(e, || no_upload(upload.as_str())),
    }
}

/// `media.unfurl`: one call of the caller's allowance, then the card.
pub(crate) async fn unfurl(me: MediaActivation, user: Caller, url: WebUrl) -> MediaEvent {
    let allowance = me.allowance();
    let held = match allowance.reserve(user.principal(), source::FETCH_CALLS, 1).await {
        Ok(r) => r,
        Err(d) => return d.into(),
    };
    let ev = crate::unfurl::unfurl(&me.remote, &me.store, url.as_str()).await;
    match ev {
        MediaEvent::Unfurled { .. } => allowance.commit(&held.id, None).await,
        _ => allowance.release(&held.id).await,
    }
    ev
}

/// `media.fetch`: one call and the bytes it reads, of the caller's day; the
/// file it keeps, V2-69, of their storage (`media.bytes`) as a put's is —
/// the bytes read first, then held at their size, committed when the file
/// is new to them.
pub(crate) async fn fetch(me: MediaActivation, user: Caller, url: WebUrl, ticket: Option<TicketRef>, doc: Option<DocId>, name: Option<FileName>) -> MediaEvent {
    let on = match owner(&me.seam, &user, ticket.as_ref(), doc.as_ref()).await {
        Ok(on) => on,
        Err(ev) => return ev,
    };
    let allowance = me.allowance();
    let call = match allowance.reserve(user.principal(), source::FETCH_CALLS, 1).await {
        Ok(r) => r,
        Err(d) => return d.into(),
    };
    let read = match allowance.reserve(user.principal(), source::FETCH_BYTES, kinds::PUT_MAX as u64).await {
        Ok(r) => r,
        Err(d) => {
            allowance.release(&call.id).await;
            return d.into();
        }
    };
    let got = crate::unfurl::fetch_bytes(&me.remote, url.as_str(), name.map(FileName::into_string)).await;
    let (bytes, name) = match got {
        Ok(g) => g,
        Err(ev) => {
            allowance.release(&call.id).await;
            allowance.release(&read.id).await;
            return ev;
        }
    };
    allowance.commit(&call.id, None).await;
    let size = bytes.len() as u64;
    let kept = match allowance.reserve(user.principal(), source::BYTES, size).await {
        Ok(r) => r,
        Err(d) => {
            allowance.release(&read.id).await;
            return d.into();
        }
    };
    let put = me.store.put_bytes(on, bytes, name, Some(user.principal().to_string()), kinds::PUT_MAX).await;
    match &put {
        Ok(_) => allowance.commit(&read.id, Some(size)).await,
        Err(_) => allowance.release(&read.id).await,
    }
    settle(allowance.as_ref(), &kept.id, put.as_ref().ok()).await;
    stored(&put)
}

/// EDITOR-10: every hour while the store lives, uploads untouched for a day
/// are dropped and their holds released.
pub(crate) async fn sweeping(store: Weak<MediaStore>, allowance: Arc<RwLock<Arc<dyn Allowance>>>) {
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(3600));
    loop {
        tick.tick().await;
        let Some(store) = store.upgrade() else { return };
        match store.sweep(UPLOAD_TTL).await {
            Ok(gone) => {
                let allowance = allowance.read().unwrap_or_else(|p| p.into_inner()).clone();
                for d in &gone {
                    if let Some(id) = &d.reservation {
                        allowance.release(id).await;
                    }
                }
                if !gone.is_empty() {
                    tracing::info!("media:     {} abandoned upload(s) dropped", gone.len());
                }
            }
            Err(e) => tracing::warn!("media: the sweep of abandoned uploads failed: {e}"),
        }
    }
}
