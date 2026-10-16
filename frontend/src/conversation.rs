//! APP-22: a ticket page's **Conversation** — the ticket's comments at the
//! foot of its page, oldest first, live, and a box to comment on the ticket
//! as a whole.
//!
//! Owner, 2026-10-15: *"the ticket itself should allow me to comment."*
//! COMMENT-3 pins every comment to an element; a comment on the ticket as a
//! whole ("this isn't what I meant, do X instead") is a comment like any
//! other, anchored to the ticket's page at the path [`PATH`] — `route
//! "#/ticket/<ID>"`, `path "conversation"`, `label "Conversation"`, `ticket
//! "<ID>"` ([`anchor`]) — so `comments list --ticket <ID>` and `comments
//! watch` carry it unchanged, and `-p '{"path":"conversation"}'` lists only
//! these.
//!
//! **The thread** ([`thread`]) is every comment on the ticket (its anchor's
//! `ticket`, as `comments list --ticket` reads it), whole-ticket and pinned
//! alike, oldest first. A pinned one says what it is pinned to and links to
//! its pin ([`Place`]); a whole-ticket one is not pinned anywhere in
//! annotation mode (src/annotate.rs leaves it out), and its link back
//! (`?comment=<id>`) flashes its row here ([`row_selector`]).
//!
//! **The box**: Ctrl+Enter (⌘↩ on a Mac, AGS-28's `comment.save`) or Send
//! keeps it with `comments.add`; Esc leaves the box, the draft kept. `c` on
//! a ticket page focuses it (`comment.focus`, src/keys.rs). The copy is
//! src/comments.rs's, which `comments.watch` keeps: a comment sent from any
//! page, or by an agent, shows on every open page.
//!
//! **A change request** (APP-23: the tray's Request changes,
//! `tickets.request_changes`) is a comment here like any other, with intent
//! `changes`: its row is marked "changes requested" ([`mark`]).
//!
//! What decides — the thread, a row's place, its mark, the anchor — is pure and pinned
//! by native tests; the rest is the DOM.

use leptos::prelude::*;
use menger_tickets_api::{Anchor, Comment, CommentIntent, CommentState};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;

use crate::bindings::{self, Context};
use crate::{comments, comments_page, dom, fmt_relative, fmt_unix, md, now_unix};

/// The anchor path of a comment on the ticket as a whole.
pub(crate) const PATH: &str = "conversation";

/// Its label, in words.
pub(crate) const LABEL: &str = "Conversation";

/// The anchor of a comment on `ticket` as a whole.
pub(crate) fn anchor(ticket: &str) -> Anchor {
    Anchor {
        route: format!("#/ticket/{ticket}"),
        path: PATH.to_string(),
        label: LABEL.to_string(),
        excerpt: String::new(),
        ticket: Some(ticket.to_string()),
        version: None,
    }
}

/// Whether `c` is on its ticket as a whole (not pinned to an element).
pub(crate) fn is_whole(c: &Comment) -> bool {
    c.anchor.path == PATH
}

/// The thread on `ticket`: every comment whose anchor names it (any case),
/// whole-ticket and pinned alike, oldest first.
pub(crate) fn thread(all: &[Comment], ticket: &str) -> Vec<Comment> {
    let mut out: Vec<Comment> =
        all.iter().filter(|c| c.anchor.ticket.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(ticket))).cloned().collect();
    out.sort_by_key(|c| c.id);
    out
}

/// Where a row says it was left.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    /// On the ticket as a whole.
    Whole,
    /// Pinned to an element: what it was (its label), and the link to the pin.
    Pinned { label: String, href: String },
}

pub(crate) fn place(c: &Comment) -> Place {
    if is_whole(c) {
        Place::Whole
    } else {
        Place::Pinned { label: c.anchor.label.clone(), href: comments_page::link(c) }
    }
}

/// APP-23: the mark a row wears for what the comment is — a change request
/// says so; a comment wears none.
pub(crate) fn mark(c: &Comment) -> Option<&'static str> {
    (c.intent == CommentIntent::Changes).then_some("changes requested")
}

/// The selector of comment `id`'s row in the conversation — where its link
/// back flashes.
pub(crate) fn row_selector(id: u64) -> String {
    format!("#conversation li[data-comment-id=\"{id}\"]")
}

/// What the box says while it is empty.
pub(crate) fn placeholder(ticket: &str) -> String {
    format!("Comment on {ticket} — Ctrl+Enter sends")
}

/// `c`: the open ticket page's box focused and in view. Whether there was one.
pub(crate) fn focus_box() -> bool {
    let Some(el) = dom::window().document().and_then(|d| d.query_selector("#conversation textarea.cbox").ok().flatten()) else {
        return false;
    };
    let Ok(area) = el.dyn_into::<web_sys::HtmlTextAreaElement>() else { return false };
    let opts = web_sys::ScrollIntoViewOptions::new();
    opts.set_block(web_sys::ScrollLogicalPosition::Center);
    area.scroll_into_view_with_scroll_into_view_options(&opts);
    area.focus().is_ok()
}

/// The section: the thread, then the box.
#[component]
pub(crate) fn Conversation(ticket: String) -> impl IntoView {
    comments::want();
    let held = comments::signal();
    let id = ticket.clone();
    let rows = Memo::new(move |_| held.with(|h| h.as_ref().map(|h| thread(h.list(), &id))));
    // The newest id the page first drew: a row after it arrives (`fresh`).
    let first_seen = StoredValue::new(None::<u64>);
    Effect::new(move |_| {
        if let (None, Some(r)) = (first_seen.get_value(), rows.get()) {
            first_seen.set_value(Some(r.last().map_or(0, |c| c.id.get())));
        }
    });
    let body = RwSignal::new(String::new());
    let sending = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let me = ticket.clone();
    let send = move || {
        let text = body.get_untracked();
        if text.trim().is_empty() || sending.get_untracked() {
            return;
        }
        sending.set(true);
        let a = anchor(&me);
        spawn_local(async move {
            let r = comments::add(a, text).await;
            sending.try_set(false);
            match r {
                Ok(_) => {
                    body.try_set(String::new());
                    error.try_set(None);
                }
                Err(e) => {
                    error.try_set(Some(e));
                }
            }
        });
    };
    let send_key = send.clone();
    let area = NodeRef::<leptos::html::Textarea>::new();
    crate::grow::follow(area, move || body.track());
    let count = move || rows.with(|r| r.as_ref().map(|r| r.len()));
    view! {
        <section class="convsec" id="conversation" data-ticket-id=ticket.clone()>
            <h3>"Conversation"{move || count().filter(|n| *n > 0).map(|n| view! { <span class="ccn">{n}</span> })}</h3>
            {move || match rows.get() {
                None => view! { <p class="loading">"loading comments…"</p> }.into_any(),
                Some(r) if r.is_empty() => view! { <p class="none">"No comments yet."</p> }.into_any(),
                Some(r) => {
                    let since = first_seen.get_value();
                    view! {
                        <ol class="cthread">
                            {r.into_iter().map(|c| { let fresh = since.is_some_and(|s| c.id > menger_tickets_api::vocab::CommentId::new(s)); row(c, fresh) }).collect_view()}
                        </ol>
                    }
                    .into_any()
                }
            }}
            <div class="cnew">
                <textarea
                    node_ref=area
                    class="cbox grow"
                    rows="3"
                    aria-label=format!("Comment on {ticket}")
                    placeholder=placeholder(&ticket)
                    prop:value=move || body.get()
                    on:input=move |ev| body.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| match bindings::hit(&ev, Context::Field) {
                        Some("comment.save") => {
                            ev.prevent_default();
                            send_key();
                        }
                        Some("form.cancel") => {
                            ev.prevent_default();
                            if let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::HtmlElement>().ok()) {
                                let _ = el.blur();
                            }
                        }
                        _ => {}
                    }
                ></textarea>
                <div class="cnewacts">
                    {move || error.get().map(|e| view! { <span class="cerr" role="alert">{e}</span> })}
                    <button
                        class="csend"
                        disabled=move || sending.get() || body.with(|b| b.trim().is_empty())
                        on:click=move |_| send()
                    >
                        {move || if sending.get() { "Sending…" } else { "Send" }}
                    </button>
                </div>
            </div>
        </section>
    }
}

/// One comment in the thread.
fn row(c: Comment, fresh: bool) -> impl IntoView {
    let id = c.id;
    let open = c.state == CommentState::Open;
    let busy = RwSignal::new(false);
    let err = RwSignal::new(None::<String>);
    let flip = move |_| {
        busy.set(true);
        spawn_local(async move {
            let r = if open { comments::resolve(id.get()).await } else { comments::reopen(id.get()).await };
            busy.try_set(false);
            err.try_set(r.err());
        });
    };
    let html = md::render_body(&c.body).html;
    let node = NodeRef::<leptos::html::Div>::new();
    node.on_load(|el: web_sys::HtmlDivElement| crate::highlight::mount(&el));
    let place = match place(&c) {
        Place::Whole => None,
        Place::Pinned { label, href } => {
            let title = format!("pinned to {label} — {}", c.anchor.path);
            Some(view! { <a class="cpin" href=href title=title>"on "{label}</a> })
        }
    };
    view! {
        <li class="cpost" class:fresh=fresh class:pinned=!is_whole(&c) class:changes=mark(&c).is_some() data-comment-id=id.to_string() data-state=c.state.as_str()>
            // LOOK-3: the author's initials, the mockup's avatar.
            <span class="cav" aria-hidden="true">{initials(&c.author)}</span>
            <div class="chead">
                {mark(&c).map(|m| view! { <span class="cmark">{m}</span> })}
                <span class="cwho">{c.author.clone()}</span>
                <span class="cwhen" title=fmt_unix(c.at)>{fmt_relative(now_unix(), c.at)}</span>
                {place}
                {(!open).then(|| view! { <span class="cstate" data-state="resolved">"resolved"</span> })}
                <button class="cflip" data-act=if open { "resolve" } else { "reopen" } disabled=move || busy.get() on:click=flip>
                    {if open { "Resolve" } else { "Reopen" }}
                </button>
            </div>
            <div class="cbody md" data-links="comment" data-ticket=c.anchor.ticket.clone() node_ref=node inner_html=html></div>
            {move || err.get().map(|e| view! { <span class="cerr">{e}</span> })}
        </li>
    }
}

/// LOOK-3: an author's initials for the avatar — the first letters of its
/// first two words (`owner` → `OW`, `Ada Lovelace` → `AL`, `key:4b…` → `KE`).
pub(crate) fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let s: String = match words.as_slice() {
        [] => "?".to_string(),
        [one] => one.chars().take(2).collect(),
        [a, b, ..] => a.chars().take(1).chain(b.chars().take(1)).collect(),
    };
    s.to_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::{pick_single, table_with, Press};

    #[test]
    fn an_avatar_is_the_authors_initials() {
        assert_eq!(initials("owner"), "OW");
        assert_eq!(initials("Ada Lovelace"), "AL");
        assert_eq!(initials("key:4b19"), "K4");
        assert_eq!(initials(""), "?");
    }
    use crate::comments::tests::{comment, resolved};

    /// The thread is the ticket's comments — its anchor's ticket, any case —
    /// whole-ticket and pinned alike, resolved too, oldest first; another
    /// ticket's and the app's are not in it.
    #[test]
    fn the_thread_is_the_tickets_comments_oldest_first() {
        let whole = |id| {
            let mut c = comment(id, "#/ticket/D-2", PATH, Some("D-2"));
            c.anchor = anchor("D-2");
            c
        };
        let all = vec![
            whole(9),
            comment(3, "#/ticket/D-2", "#t-status", Some("D-2")),
            comment(4, "#/ticket/D-3", PATH, Some("D-3")),
            comment(5, "#/", "#annotate", None),
            resolved(whole(1)),
            comment(7, "#/board", "#board", Some("d-2")),
        ];
        let ids: Vec<u64> = thread(&all, "D-2").iter().map(|c| c.id.get()).collect();
        assert_eq!(ids, [1, 3, 7, 9]);
        assert_eq!(thread(&all, "d-2").len(), 4, "a ticket id, whatever its case");
        assert!(thread(&all, "D-9").is_empty());
    }

    /// A whole-ticket comment's anchor is the ticket's page at `conversation`
    /// — and its row says nothing of a pin; a pinned one names what it is
    /// pinned to and links to it.
    #[test]
    fn a_whole_ticket_row_and_a_pinned_one() {
        let a = anchor("APP-22");
        assert_eq!(
            (a.route.as_str(), a.path.as_str(), a.label.as_str(), a.ticket.as_deref(), a.excerpt.as_str()),
            ("#/ticket/APP-22", "conversation", "Conversation", Some("APP-22"), "")
        );
        assert!(menger_tickets_api::comments::check(a.clone(), "x", None).is_ok(), "the server keeps it as sent");
        let mut whole = comment(1, "#/ticket/APP-22", PATH, Some("APP-22"));
        whole.anchor = a;
        assert!(is_whole(&whole));
        assert_eq!(place(&whole), Place::Whole);
        let mut pinned = comment(2, "#/ticket/APP-22", "#t-status", Some("APP-22"));
        pinned.anchor.label = "Status".into();
        assert!(!is_whole(&pinned));
        assert_eq!(place(&pinned), Place::Pinned { label: "Status".into(), href: "#/ticket/APP-22?comment=2".into() });
        assert_eq!(row_selector(1), "#conversation li[data-comment-id=\"1\"]");
        // APP-23: a change request is marked; a comment is not.
        assert_eq!(mark(&whole), None);
        whole.intent = CommentIntent::Changes;
        assert_eq!(mark(&whole), Some("changes requested"));
        assert_eq!(placeholder("APP-22"), "Comment on APP-22 — Ctrl+Enter sends");
    }

    /// `c` on a ticket page is `comment.focus`; Ctrl+Enter in the box is
    /// `comment.save` and Esc leaves it.
    #[test]
    fn c_focuses_the_box_and_ctrl_enter_sends() {
        let t = table_with(&[]);
        assert_eq!(pick_single(&t, &Press::key("c"), Context::TicketPage, false), Some("comment.focus"));
        assert_eq!(pick_single(&t, &Press::key("c"), Context::Board, false), None, "a ticket page's, not the board's");
        let ctrl_enter = Press { key: "Enter".into(), ctrl: true, ..Press::default() };
        assert_eq!(pick_single(&t, &ctrl_enter, Context::Field, false), Some("comment.save"));
        assert_eq!(pick_single(&t, &Press::key("Escape"), Context::Field, false), Some("form.cancel"));
        assert_eq!(pick_single(&t, &Press::key("Enter"), Context::Field, false), Some("form.submit"), "a bare Enter is a new line's");
    }
}
