//! CHAT-66: **replying to a message** — the owner, chat 6: *"reply to a
//! message (ideally not quoting the whole thing but entering the id into the
//! message much like a screenshot is …) in the chat case as a typical chat
//! app would."*
//!
//! - **Asking**: a turn's *reply* (its hover menu on a desk, its long press
//!   on a phone) or a swipe right on it ([`swipe`]) puts a bar over the
//!   composer — *↩ replying to ada: the first words…* ×. Esc or × takes it
//!   off; Send prepends the token ([`outgoing`]): the stored body is
//!   `[↩](message:<id>) …` (`menger_tickets_api::chat_reply`), nothing of
//!   the original copied.
//! - **Drawing**: a reply's turn shows a compact quoted header — the
//!   original's author and first words, from the message's `quote` the
//!   server answers with it ([`header`]); "a deleted message" when it was
//!   deleted, "a message you can't see" when none came. A tap on it scrolls
//!   to the original (fetching the window around it when it is out of view,
//!   CHAT-8) and flashes it.
//! - **Elsewhere** a `message:` link is a chip, `↩ message N` (src/md.rs).
//!
//! Which chat is replying to what is held here, per chat, for the pane and
//! its composer alike: [`set`], [`get`], [`clear`].

use std::collections::HashMap;

use leptos::prelude::*;
use menger_tickets_api::chat_reply;
use menger_tickets_api::chats::Quote;

/// What the composer of a chat is replying to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Replying {
    pub(crate) id: u64,
    pub(crate) who: String,
    pub(crate) snippet: String,
}

thread_local! {
    static REPLYING: ArcRwSignal<HashMap<u64, Replying>> = ArcRwSignal::new(HashMap::new());
}

fn signal() -> ArcRwSignal<HashMap<u64, Replying>> {
    REPLYING.with(Clone::clone)
}

/// Chat `chat`'s composer now replies to `r`.
pub(crate) fn set(chat: u64, r: Replying) {
    signal().update(|m| {
        m.insert(chat, r);
    });
}

/// What chat `chat`'s composer replies to (tracked).
pub(crate) fn get(chat: u64) -> Option<Replying> {
    signal().with(|m| m.get(&chat).cloned())
}

/// What chat `chat`'s composer replies to, untracked.
pub(crate) fn get_untracked(chat: u64) -> Option<Replying> {
    signal().with_untracked(|m| m.get(&chat).cloned())
}

/// Chat `chat`'s composer replies to nothing.
pub(crate) fn clear(chat: u64) {
    signal().update(|m| {
        m.remove(&chat);
    });
}

/// The bar over the composer.
pub(crate) fn bar_text(r: &Replying) -> String {
    if r.snippet.is_empty() {
        format!("replying to {}", r.who)
    } else {
        format!("replying to {}: {}", r.who, r.snippet)
    }
}

/// What Send posts: the box's text, replying to `to` when it is set.
pub(crate) fn outgoing(to: Option<u64>, text: &str) -> String {
    match to {
        Some(id) => chat_reply::replying(id, text),
        None => text.to_string(),
    }
}

/// A reply's quoted header: who, and what it says of the original —
/// `name` names a principal.
pub(crate) fn header(reply_to: Option<u64>, quote: Option<&Quote>, name: &impl Fn(&str) -> String) -> Option<(String, String)> {
    reply_to?;
    Some(match quote {
        Some(q) if q.deleted => (String::new(), "a deleted message".to_string()),
        Some(q) => (q.name.clone().unwrap_or_else(|| name(&q.author)), q.snippet.clone()),
        None => (String::new(), "a message you can't see".to_string()),
    })
}

/// How far a finger travels right before a swipe replies, in px.
pub(crate) const SWIPE_PX: f64 = 64.0;

/// What a touch that moved `dx`, `dy` from where it landed does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Swipe {
    /// Nothing yet (or a scroll: it went up or down first).
    None,
    /// The turn follows the finger this far right.
    Follow(f64),
    /// Far enough: lifted here, it replies.
    Reply(f64),
}

/// A swipe right: mostly sideways, rightwards; the turn follows it at most
/// [`SWIPE_PX`] plus a little, and past that it replies.
pub(crate) fn swipe(dx: f64, dy: f64) -> Swipe {
    if dx <= 8.0 || dy.abs() > dx * 0.6 {
        return Swipe::None;
    }
    let shown = dx.min(SWIPE_PX + 16.0);
    if dx >= SWIPE_PX {
        Swipe::Reply(shown)
    } else {
        Swipe::Follow(shown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::vocab::MessageId;

    fn q(deleted: bool) -> Quote {
        Quote { id: MessageId::new(512), author: "key:54a9".into(), name: None, snippet: if deleted { String::new() } else { "Expired events: archive?".into() }, deleted }
    }

    #[test]
    fn the_header_says_who_and_what_or_why_not() {
        let name = |p: &str| if p == "key:54a9" { "builder".to_string() } else { p.to_string() };
        assert_eq!(header(None, None, &name), None, "not a reply");
        assert_eq!(header(Some(512), Some(&q(false)), &name), Some(("builder".into(), "Expired events: archive?".into())));
        let mut named = q(false);
        named.name = Some("Ada".into());
        assert_eq!(header(Some(512), Some(&named), &name).unwrap().0, "Ada", "the name it wrote under");
        assert_eq!(header(Some(512), Some(&q(true)), &name), Some((String::new(), "a deleted message".into())));
        assert_eq!(header(Some(512), None, &name), Some((String::new(), "a message you can't see".into())));
    }

    #[test]
    fn send_prepends_the_token_and_only_the_token() {
        assert_eq!(outgoing(Some(512), "archive"), "[↩](message:512) archive");
        assert_eq!(outgoing(None, "archive"), "archive");
        assert_eq!(chat_reply::reply_to_in(&outgoing(Some(9), "x")), Some(9));
        let r = Replying { id: 512, who: "builder".into(), snippet: "Expired events".into() };
        assert_eq!(bar_text(&r), "replying to builder: Expired events");
        assert_eq!(bar_text(&Replying { snippet: String::new(), ..r }), "replying to builder");
    }

    #[test]
    fn a_swipe_right_replies_a_scroll_does_not() {
        assert_eq!(swipe(4.0, 0.0), Swipe::None, "a tap");
        assert_eq!(swipe(30.0, 2.0), Swipe::Follow(30.0));
        assert_eq!(swipe(70.0, 10.0), Swipe::Reply(70.0));
        assert_eq!(swipe(200.0, 10.0), Swipe::Reply(SWIPE_PX + 16.0), "it follows only so far");
        assert_eq!(swipe(40.0, 60.0), Swipe::None, "mostly down: a scroll");
        assert_eq!(swipe(-80.0, 0.0), Swipe::None, "leftwards");
    }
}
