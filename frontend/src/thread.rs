//! COMMENT-8: **a comment's thread** — its replies under it, oldest first,
//! live, and a Reply box.
//!
//! Owner, 2026-10-18: *"we need replies to comments"* — the builder answers
//! a comment from synapse (`comments add --reply-to N`) and the owner sees
//! the answer under it, on a phone. A reply is a comment of its own whose
//! `reply_to` names its thread's root (the server keeps threads one level
//! deep), on the root's anchor — so it is never pinned or counted apart:
//! wherever a root is drawn (a ticket's Conversation, `#/comments`, the
//! annotation popover) [`Replies`] draws its thread under it, from
//! src/comments.rs's copy, which `comments.watch` keeps.
//!
//! **The box** opens from Reply (or by itself, when a draft is kept for this
//! thread): Ctrl+Enter or Send keeps it, Esc closes it, the draft kept.
//! Nothing is lost (COMMENT-9, src/drafts.rs): the draft is kept by its
//! thread ([`drafts::reply_key`]) until the server answers; a reply not
//! kept says why, with a Retry. Replying to a resolved comment reopens it
//! (the server's, in the same write).
//!
//! What decides — which comments are a thread's, their order — is pure and
//! pinned by native tests in src/comments.rs; the rest is the DOM.

use leptos::prelude::*;
use menger_tickets_api::Comment;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::bindings::{self, Context};
use crate::conversation::initials;
use crate::{comments, drafts, fmt_relative, fmt_unix, md, now_unix};

/// What the Reply box says while it is empty.
pub(crate) fn placeholder(author: &str) -> String {
    format!("Reply to {author} — Ctrl+Enter sends")
}

/// The replies under `root`, then its Reply button or box.
#[component]
pub(crate) fn Replies(root: Comment) -> impl IntoView {
    let held = comments::signal();
    let id = root.id;
    let replies = Memo::new(move |_| held.with(|h| h.as_ref().map(|h| h.replies(id)).unwrap_or_default()));
    let kept = drafts::reply_key(id.get());
    let body = RwSignal::new(drafts::load(&kept).unwrap_or_default());
    // A kept draft opens the box by itself: it is where it was left.
    let open = RwSignal::new(body.with_untracked(|b| !b.trim().is_empty()));
    let sending = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let landed = drafts::landed_signal();
    {
        let kept = kept.clone();
        Effect::new(move |_| body.with(|b| drafts::save(&kept, b)));
    }
    {
        let kept = kept.clone();
        Effect::new(move |_| {
            if let Some((k, b)) = landed.get() {
                if k == kept && body.with_untracked(|t| t.trim() == b.trim()) {
                    body.set(String::new());
                    error.set(None);
                    open.set(false);
                }
            }
        });
    }
    let to = root.clone();
    let send = move || {
        let text = body.get_untracked();
        if text.trim().is_empty() || sending.get_untracked() {
            return;
        }
        sending.set(true);
        let to = to.clone();
        spawn_local(async move {
            let r = drafts::send_reply(&to, text).await;
            sending.try_set(false);
            match r {
                Ok(_) => {
                    body.try_set(String::new());
                    error.try_set(None);
                    open.try_set(false);
                }
                Err(e) => {
                    error.try_set(Some(e));
                }
            }
        });
    };
    let area = NodeRef::<leptos::html::Textarea>::new();
    crate::grow::follow(area, move || body.track());
    // Opened by Reply: the box focused, in view above a phone's keyboard.
    Effect::new(move |_| {
        if open.get() {
            if let Some(el) = area.get() {
                let _ = el.focus();
            }
        }
    });
    let attach = crate::media::Attach::new(body, area, root.anchor.ticket.clone());
    // APP-70: a drawing on the pad goes first.
    let send = attach.then(send);
    let send_key = send.clone();
    let retry = send.clone();
    let who = root.author.clone();
    view! {
        <div class="creplies" data-thread=id.to_string()>
            {move || {
                let r = replies.get();
                (!r.is_empty()).then(|| view! { <ol class="crlist">{r.into_iter().map(reply_row).collect_view()}</ol> })
            }}
            {move || if open.get() {
                let retry = retry.clone();
                let send = send.clone();
                let send_key = send_key.clone();
                view! {
                    <div class="crnew">
                        <crate::draw::PadView a=attach />
                        <textarea
                            node_ref=area
                            class="crbox grow"
                            rows="2"
                            aria-label=format!("Reply to comment {id}")
                            placeholder=placeholder(&who)
                            prop:value=move || body.get()
                            on:input=move |ev| body.set(event_target_value(&ev))
                            on:paste=move |ev| attach.paste(ev)
                            on:dragover=move |ev| attach.dragover(ev)
                            on:drop=move |ev| attach.dropped(ev)
                            on:keydown=move |ev: web_sys::KeyboardEvent| match bindings::hit(&ev, Context::Field) {
                                Some("comment.save") => {
                                    ev.prevent_default();
                                    send_key();
                                }
                                Some("form.cancel") => {
                                    ev.prevent_default();
                                    ev.stop_propagation();
                                    open.set(false);
                                    if let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                                        let _ = el.blur();
                                    }
                                }
                                _ => {}
                            }
                        ></textarea>
                        <div class="cnewacts">
                            <crate::media::Attachments a=attach />
                            {move || error.get().map(|e| {
                                let retry = retry.clone();
                                view! {
                                    <span class="cerr" role="alert">{drafts::words(&e)}</span>
                                    <button class="cretry" disabled=move || sending.get() on:click=move |_| retry()>"Retry"</button>
                                }
                            })}
                            <button class="crcancel" on:click=move |_| open.set(false)>"Cancel"</button>
                            <button
                                class="csend crsend"
                                disabled=move || sending.get() || attach.busy() || (!attach.drawn() && body.with(|b| b.trim().is_empty()))
                                on:click=move |_| send()
                            >
                                {move || if sending.get() { "Sending…" } else { "Reply" }}
                            </button>
                        </div>
                    </div>
                }
                .into_any()
            } else {
                view! { <button class="creply" on:click=move |_| open.set(true)>"Reply"</button> }.into_any()
            }}
        </div>
    }
}

/// One reply: who, when, what — no resolve of its own (its thread's root
/// is resolved or reopened).
fn reply_row(c: Comment) -> impl IntoView {
    let html = md::render_comment(&c.body).html;
    let node = NodeRef::<leptos::html::Div>::new();
    node.on_load(|el: web_sys::HtmlDivElement| {
        crate::highlight::mount(&el);
        crate::media::hydrate(&el);
    });
    view! {
        <li class="creply-row" data-comment-id=c.id.to_string()>
            <span class="crav" aria-hidden="true">{initials(&c.author)}</span>
            <div class="chead">
                <span class="cwho">{c.author.clone()}</span>
                <span class="cwhen" title=fmt_unix(c.at)>{fmt_relative(now_unix(), c.at)}</span>
            </div>
            <div class="cbody md" data-links="comment" data-ticket=c.anchor.ticket.clone() node_ref=node inner_html=html></div>
        </li>
    }
}
