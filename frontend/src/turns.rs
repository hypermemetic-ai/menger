//! CHAT-7: a chat's turns, as a traditional agent chat draws them — the
//! dock's chat and the full-screen session alike, only narrower in the dock.
//!
//! - **Your turns** are bubbles on the right, in the selection token, with
//!   the time under them. **Others' turns** are full-width prose under one
//!   mono line: a mark (an agent square, a person round), the name, an agent
//!   tag, `via` and the time.
//! - **Code blocks** get a head ([`code_heads`]): the language, the path the
//!   fence names after it (```` ```rust kiln/src/schedule.rs ````), *copy*.
//! - **Ticket codes** become chips ([`chip_codes`]); a code alone on its line
//!   is drawn again under the turn as a card that acts (CHAT-56,
//!   src/chat_cards.rs) — `/ticket CODE` posts one. CHAT-57: a name in bold
//!   (`**KILN-7**`, `**ENV-2..5**`, `**A-11/12**`) is a **live chip**
//!   ([`live_chip`]): coloured by its ticket's stage once its card resolves
//!   it, a run or a set one chip that opens to its tickets' on a tap.
//! - A turn shows **copy · quote · link** on hover: its text to the
//!   clipboard, `> quoted` into the composer, its URL (`?chat=ID&at=MSG`).
//! - **Scrolling** — manifest's two rules (`conversationScroll.ts`), in
//!   [`Scroll`]: opening lands on the read mark (or the newest) once, with
//!   no animation, and a message arriving never moves you. More than
//!   [`OFF_BOTTOM_PX`] off the bottom you are reading back, and what arrives
//!   is counted; at the bottom, new turns appear in place and keep you there.
//!   CHAT-62: more than a screen off ([`far_off`]) a round **↓** fades in,
//!   docked on the composer's top-right edge (src/chat_pane.rs's
//!   [`jump_dock`](crate::chat_pane::jump_dock)) with the count as a badge,
//!   and the space it takes is the composer's, never a turn's.
//!
//! What decides is pure and pinned by native tests; the rest is the DOM.


/// manifest's `conversation.offBottomPx`: past this, you are reading back.
pub(crate) const OFF_BOTTOM_PX: f64 = 48.0;

/// Whether a scroller is more than [`OFF_BOTTOM_PX`] off its bottom.
pub(crate) fn off_bottom(scroll_height: f64, scroll_top: f64, client_height: f64) -> bool {
    scroll_height - scroll_top - client_height > OFF_BOTTOM_PX
}

/// CHAT-62: whether a scroller is more than a screen off its bottom — the
/// ↓ shows only then.
pub(crate) fn far_off(scroll_height: f64, scroll_top: f64, client_height: f64) -> bool {
    scroll_height - scroll_top - client_height > client_height.max(OFF_BOTTOM_PX)
}

/// CHAT-61: the turn the reader is looking at — of the mounted turns'
/// `(id, top, bottom)` on screen, in order, the first that begins inside
/// the view (below `view_top`, a sticky head's bottom where one covers it),
/// else the one the top cuts — and where its top is. What moves above it
/// is undone by [`held_by`]; so is a turn cut by the top changing height,
/// which moves what is below it.
pub(crate) fn first_seen(rows: &[(u64, f64, f64)], view_top: f64) -> Option<(u64, f64)> {
    let begins = rows.iter().find(|(_, top, _)| *top >= view_top - 0.5);
    let cut = || rows.iter().find(|(_, _, bottom)| *bottom > view_top + 0.5);
    begins.or_else(cut).map(|&(id, top, _)| (id, top))
}

/// CHAT-61: how far to scroll so the turn seen at `before` is back there,
/// now that it is at `after` — `None` under half a pixel. Something above
/// it grew (a message's frame drawn, an image loaded): its top went down,
/// and the scroll goes down as far; something below it changed: it did not
/// move, and nothing is done.
pub(crate) fn held_by(before: f64, after: f64) -> Option<f64> {
    let d = after - before;
    (d.abs() >= 0.5).then_some(d)
}

/// Where opening a chat lands: the first unread turn, or the newest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Landing {
    /// At this message (the read mark's *new* line).
    At(u64),
    Newest,
}

/// The scroll policy, and nothing else: one landing per chat, and arrivals
/// that keep you at the bottom only when you were there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Scroll {
    landed: bool,
    pub(crate) at_bottom: bool,
    /// CHAT-62: more than a screen off the bottom ([`far_off`]).
    pub(crate) far: bool,
    /// Turns that arrived below while you read back.
    pub(crate) unseen: u32,
}

impl Default for Scroll {
    fn default() -> Self {
        Scroll { landed: false, at_bottom: true, far: false, unseen: 0 }
    }
}

impl Scroll {
    /// Rule 1: where to land, once per chat (`None` after the first time).
    pub(crate) fn land(&mut self, new_from: Option<u64>) -> Option<Landing> {
        if self.landed {
            return None;
        }
        self.landed = true;
        Some(new_from.map_or(Landing::Newest, Landing::At))
    }

    /// `n` turns arrived: whether to keep the view at the bottom (you were
    /// there). Off it, nothing moves; they are counted for the ↓.
    pub(crate) fn arrived(&mut self, n: u32) -> bool {
        if self.at_bottom {
            return true;
        }
        self.unseen += n;
        false
    }

    /// The reader scrolled (or the content moved under them): whether
    /// they are at the bottom, and whether more than a screen off it.
    pub(crate) fn moved(&mut self, at_bottom: bool, far: bool) {
        self.at_bottom = at_bottom;
        self.far = far && !at_bottom;
        if at_bottom {
            self.unseen = 0;
        }
    }

    /// Rule 2, CHAT-62: the ↓ while more than a screen off the bottom —
    /// `Some(n)`, the turns that came in below since (its badge when not 0).
    pub(crate) fn jump(&self) -> Option<u32> {
        (!self.at_bottom && self.far).then_some(self.unseen)
    }
}

/// CHAT-62: the ↓'s badge — the count, `99+` past it; none for 0.
pub(crate) fn jump_badge(n: u32) -> Option<String> {
    match n {
        0 => None,
        1..=99 => Some(n.to_string()),
        _ => Some("99+".to_string()),
    }
}

/// CHAT-62: what the ↓ says to a screen reader and on hover.
pub(crate) fn jump_label(n: u32) -> String {
    match n {
        0 => "jump to latest".to_string(),
        1 => "jump to latest · 1 new message".to_string(),
        n => format!("jump to latest · {n} new messages"),
    }
}

/// CHAT-29: how far the document must move one way before the head follows
/// — a finger's jitter moves nothing.
pub(crate) const HEAD_SLOP_PX: f64 = 6.0;

/// CHAT-29: on a phone the chat scrolls the document, and its head hides on
/// a scroll down and comes back on a scroll up — always shown within its own
/// height of the top. `y` the document's scroll now, `was` the last one the
/// head moved at, `by_reader` whether the reader's finger, wheel or keys
/// moved it — the view's own moves (landing, ↓, an arrival kept at the
/// bottom) leave the head as it is. The answer is whether it shows, and the
/// scroll to measure the next move from. CHAT-60: the ticket strip is part
/// of the head — `head_h` is the two together, and a strip that is `open`
/// (a stage's tickets dropped) holds both where they are.
pub(crate) fn head_shown(shown: bool, was: f64, y: f64, head_h: f64, by_reader: bool, open: bool) -> (bool, f64) {
    if open {
        return (shown, y);
    }
    if y <= head_h.max(0.0) {
        return (true, y);
    }
    if !by_reader {
        return (shown, y);
    }
    let dy = y - was;
    if dy > HEAD_SLOP_PX {
        (false, y)
    } else if dy < -HEAD_SLOP_PX {
        (true, y)
    } else {
        // Within the slop: nothing moves, and the mark stays where it was
        // unless the head already agrees with the way it is going.
        (shown, if (shown && dy < 0.0) || (!shown && dy > 0.0) { y } else { was })
    }
}

/// Each fenced block's language and path, in order — what a fence's info
/// string says past the language is the head's path.
pub(crate) fn fence_infos(src: &str) -> Vec<(Option<String>, Option<String>)> {
    let mut out = Vec::new();
    let mut open: Option<(char, usize)> = None;
    for line in src.lines() {
        let t = line.trim_start();
        let fence = t.chars().next().filter(|c| *c == '`' || *c == '~').map(|c| (c, t.chars().take_while(|x| *x == c).count()));
        match (open, fence) {
            (None, Some((c, n))) if n >= 3 => {
                let mut words = t[n * c.len_utf8()..].split_whitespace();
                let lang = words.next().map(str::to_string);
                let path = words.next().map(str::to_string);
                out.push((lang, path));
                open = Some((c, n));
            }
            (Some((c, n)), Some((c2, n2))) if c == c2 && n2 >= n && t.trim_end().chars().all(|x| x == c) => open = None,
            _ => {}
        }
    }
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The rendered body with a head over each `<pre>`: the language, the
/// fence's path, *copy* (`data-act="copy-code"`).
pub(crate) fn code_heads(html: &str, infos: &[(Option<String>, Option<String>)]) -> String {
    let mut out = String::with_capacity(html.len() + 128);
    let mut rest = html;
    let mut i = 0;
    while let Some(at) = rest.find("<pre>") {
        out.push_str(&rest[..at]);
        let after = &rest[at..];
        let end = after.find("</pre>").map_or(after.len(), |e| e + "</pre>".len());
        let (lang, path) = infos.get(i).cloned().unwrap_or_default();
        let lang = lang.or_else(|| after.split_once("class=\"language-").and_then(|(_, r)| r.split_once('"')).map(|(l, _)| l.to_string()));
        out.push_str("<div class=\"cblock\"><div class=\"chead\">");
        out.push_str(&format!("<span class=\"clang\">{}</span>", escape(lang.as_deref().unwrap_or("text"))));
        if let Some(p) = path {
            out.push_str(&format!("<span class=\"cpath\">{}</span>", escape(&p)));
        }
        out.push_str("<button type=\"button\" class=\"ccopy\" data-act=\"copy-code\">copy</button></div>");
        out.push_str(&after[..end]);
        out.push_str("</div>");
        rest = &after[end..];
        i += 1;
    }
    out.push_str(rest);
    out
}

/// The rendered body with each ticket code in its text a chip (a code alone
/// on its line is a card under the turn, CHAT-56). Nothing inside `<pre>`,
/// `<code>` or `<a>` is touched.
pub(crate) fn chip_codes(html: &str) -> String {
    let mut out = String::with_capacity(html.len() + 64);
    let mut skip = 0usize;
    let mut rest = html;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('<') {
            let end = stripped.find('>').map_or(rest.len(), |e| e + 2);
            let tag = &rest[..end];
            // CHAT-57: `<strong>ENV-2..5</strong>` — a name in bold.
            if skip == 0 && tag == "<strong>" {
                if let Some(close) = rest[end..].find("</strong>") {
                    let inner = &rest[end..end + close];
                    if let Some(chip) = (!inner.contains('<')).then(|| live_chip(inner.trim())).flatten() {
                        out.push_str(&chip);
                        rest = &rest[end + close + "</strong>".len()..];
                        continue;
                    }
                }
            }
            let name: String = tag.trim_start_matches(['<', '/']).chars().take_while(char::is_ascii_alphanumeric).collect();
            if matches!(name.as_str(), "pre" | "code" | "a") {
                if tag.starts_with("</") {
                    skip = skip.saturating_sub(1);
                } else {
                    skip += 1;
                }
            }
            out.push_str(tag);
            rest = &rest[end..];
            continue;
        }
        let end = rest.find('<').unwrap_or(rest.len());
        let text = &rest[..end];
        if skip == 0 {
            out.push_str(&chip_text(text));
        } else {
            out.push_str(text);
        }
        rest = &rest[end..];
    }
    out
}

/// CHAT-57: a name in bold as a live chip — `data-ticket` for the card to
/// colour by its stage (`data-stage`); a run or a set a chip whose tap
/// (`data-act="expand"`) shows its tickets' chips beside it. None when
/// `name` is not one.
pub(crate) fn live_chip(name: &str) -> Option<String> {
    use menger_tickets_api::chat_tickets::{parse_name, Form};
    let (form, codes) = parse_name(name)?;
    // TICKETS-36: a name the message wrote, resolved on open.
    let one = |c: &str| format!("<a class=\"tchip live\" data-ticket=\"{c}\" href=\"{}\">{c}</a>", crate::links::name_href(c));
    Some(match form {
        Form::One => one(name),
        Form::Range | Form::Set => {
            let all: Vec<String> = codes.iter().map(|c| one(c)).collect();
            format!(
                "<span class=\"trun\"><button type=\"button\" class=\"tchip live run\" data-act=\"expand\" aria-expanded=\"false\" title=\"{}\">{name}<span class=\"trn\">{}</span></button><span class=\"trx\" hidden>{}</span></span>",
                codes.join(" · "),
                codes.len(),
                all.join(" ")
            )
        }
    })
}

fn chip_text(text: &str) -> String {
    let codes = crate::composer::codes_in(text);
    if codes.is_empty() {
        return text.to_string();
    }
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        let w = word.trim_matches('-');
        if !w.is_empty() && crate::composer::is_code(w) {
            let lead = &word[..word.find(w).unwrap_or(0)];
            let tail = &word[word.find(w).unwrap_or(0) + w.len()..];
            out.push_str(lead);
            out.push_str(&format!("<a class=\"tchip\" href=\"{}\">{w}</a>", crate::links::name_href(w)));
            out.push_str(tail);
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Markdown as a turn draws it — a whole body, or a long one's head or a
/// chunk of it (CHAT-8). CHAT-14: a comment's images (COMMENT-5) — a
/// `media:` image is drawn from `media.get`, another origin's is its alt.
pub(crate) fn source_html(src: &str) -> String {
    let html = crate::md::render_comment(src).html;
    chip_codes(&code_heads(&html, &fence_infos(src)))
}

/// The text a turn's *quote* puts in the composer.
pub(crate) fn quoted(body: &str) -> String {
    let mut q: String = body.trim_end().lines().map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") }).collect::<Vec<_>>().join("\n");
    q.push_str("\n\n");
    q
}

/// A turn's link: the ticket with the chat open at the message.
pub(crate) fn turn_link(ticket: &str, chat: u64, message: u64) -> String {
    format!("{}?chat={chat}&at={message}", crate::links::ticket_href(ticket))
}

/// The waiting line under the last turn: who waits on `await_message`, and
/// — AGENT-3, not built — how long its claim has left (`None` until claims
/// carry a lease the chat can read).
pub(crate) fn waiting_line(who: &str, claim_left_min: Option<u32>) -> String {
    match claim_left_min {
        Some(m) => format!("{who} is waiting for your next message · {m} min left on its claim"),
        None => format!("{who} is waiting for your next message"),
    }
}

/// AGENT-3's stub: minutes left on `principal`'s claim. Claims aren't
/// built, so there is never a lease to read.
pub(crate) fn claim_left(_principal: &str) -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// manifest's rules: land once; an arrival never moves you off the
    /// bottom and is counted past 48 px; CHAT-62: the ↓ shows only more
    /// than a screen off, with what came in since.
    #[test]
    fn the_scroll_rules() {
        let mut s = Scroll::default();
        assert_eq!(s.land(Some(7)), Some(Landing::At(7)), "opening lands on the read mark");
        assert_eq!(s.land(None), None, "once");
        let mut n = Scroll::default();
        assert_eq!(n.land(None), Some(Landing::Newest), "or the newest");
        // At the bottom: arrivals keep you there.
        assert!(s.arrived(1));
        assert_eq!(s.jump(), None);
        // Read back a little: an arrival moves nothing, and is counted, but
        // the ↓ waits until you are a screen away.
        s.moved(false, false);
        assert_eq!(s.jump(), None, "under a screen off: no ↓");
        assert!(!s.arrived(2));
        s.moved(false, true);
        assert_eq!(s.jump(), Some(2), "a screen off: the ↓, with what came in");
        assert!(!s.arrived(1));
        assert_eq!(s.jump(), Some(3));
        s.moved(true, true);
        assert_eq!((s.unseen, s.jump(), s.far), (0, None, false), "back at the bottom: the ↓ goes");
        s.moved(false, true);
        assert_eq!(s.jump(), Some(0), "read back again: ↓ with no badge");
        assert!(!off_bottom(1000.0, 552.0, 400.0), "48 px off is still the bottom");
        assert!(off_bottom(1000.0, 551.0, 400.0), "49 px is reading back");
        assert!(!far_off(3000.0, 2200.0, 400.0), "400 px off with a 400 px screen: not yet");
        assert!(far_off(3000.0, 2199.0, 400.0), "past a screen: the ↓");
        assert!(!far_off(100.0, 30.0, 30.0), "a tiny screen still needs 48 px");
    }

    /// CHAT-62: the badge is the count (none for 0, 99+ past it) and the
    /// label says it in words.
    #[test]
    fn the_jump_badge_and_label() {
        assert_eq!(jump_badge(0), None);
        assert_eq!(jump_badge(3).as_deref(), Some("3"));
        assert_eq!(jump_badge(140).as_deref(), Some("99+"));
        assert_eq!(jump_label(0), "jump to latest");
        assert_eq!(jump_label(1), "jump to latest · 1 new message");
        assert_eq!(jump_label(3), "jump to latest · 3 new messages");
    }

    /// CHAT-61: the turn in view is held where it was — a frame growing
    /// above it scrolls by as much; one growing below it moves nothing.
    #[test]
    fn what_grows_above_the_view_never_moves_what_is_seen() {
        // Three turns; the view's top at 100 (a head covers 0..100).
        let rows = [(1, -300.0, 40.0), (2, 40.0, 140.0), (3, 140.0, 600.0)];
        assert_eq!(first_seen(&rows, 100.0), Some((3, 140.0)), "the first turn that begins below the head");
        assert_eq!(first_seen(&rows, 0.0), Some((2, 40.0)), "not the turn the top cuts: its own height moves what is below");
        assert_eq!(first_seen(&rows, 40.0), Some((2, 40.0)), "a turn beginning at the top");
        assert_eq!(first_seen(&rows[..2], 100.0), Some((2, 40.0)), "a turn taller than the view: the one the top cuts");
        assert_eq!(first_seen(&[], 0.0), None);
        // Turn 1 (a message's frame) drew 220 px taller: turn 2 went down 220.
        let grown = [(1, -300.0, 260.0), (2, 260.0, 360.0), (3, 360.0, 820.0)];
        let (id, before) = first_seen(&rows, 0.0).unwrap();
        let after = grown.iter().find(|r| r.0 == id).unwrap().1;
        assert_eq!(held_by(before, after), Some(220.0), "the scroll goes down as far");
        // …and shrank (a guess too tall): back up as far.
        assert_eq!(held_by(40.0, -20.0), Some(-60.0));
        // Turn 3 grew below: turn 2 did not move, nothing is done.
        assert_eq!(held_by(40.0, 40.0), None);
        assert_eq!(held_by(40.0, 40.4), None, "under half a pixel is rounding");
    }

    /// CHAT-29: the head hides going down, returns going up, always shows
    /// near the top, and a jitter inside the slop moves nothing.
    #[test]
    fn the_head_follows_the_scroll() {
        let h = 50.0;
        assert_eq!(head_shown(true, 0.0, 30.0, h, true, false), (true, 30.0), "within its height of the top: shown");
        assert_eq!(head_shown(true, 100.0, 140.0, h, true, false), (false, 140.0), "down: hidden");
        assert_eq!(head_shown(false, 140.0, 300.0, h, true, false), (false, 300.0), "further down: still hidden");
        assert_eq!(head_shown(false, 300.0, 296.0, h, true, false), (false, 300.0), "a jitter up: nothing");
        assert_eq!(head_shown(false, 300.0, 290.0, h, true, false), (true, 290.0), "up past the slop: back");
        assert_eq!(head_shown(true, 290.0, 293.0, h, true, false), (true, 290.0), "a jitter down: nothing");
        assert_eq!(head_shown(true, 290.0, 288.0, h, true, false), (true, 288.0), "going up while shown keeps the mark with it");
        assert_eq!(head_shown(false, 400.0, 20.0, h, true, false), (true, 20.0), "back at the top: shown");
        assert_eq!(head_shown(true, 0.0, 3000.0, h, false, false), (true, 3000.0), "the view's own move (landing) leaves it shown");
        assert_eq!(head_shown(false, 3000.0, 2900.0, h, false, false), (false, 2900.0), "and hidden");
    }

    /// CHAT-60: the strip goes with the head — its height counts as the
    /// head's near the top, and while it is open neither moves.
    #[test]
    fn the_strip_goes_with_the_head() {
        let (head, strip) = (50.0, 44.0);
        assert_eq!(head_shown(true, 0.0, 80.0, head + strip, true, false), (true, 80.0), "within the head and the strip of the top: shown");
        assert_eq!(head_shown(true, 100.0, 140.0, head + strip, true, false), (false, 140.0), "down past both: hidden, together");
        assert_eq!(head_shown(false, 300.0, 290.0, head + strip, true, false), (true, 290.0), "up: back, together");
        assert_eq!(head_shown(true, 100.0, 400.0, head + strip, true, true), (true, 400.0), "the strip open: nothing slides away");
        assert_eq!(head_shown(false, 400.0, 300.0, head + strip, true, true), (false, 300.0), "and nothing comes back either");
    }

    #[test]
    fn a_fence_names_its_language_and_path() {
        let src = "see\n```rust kiln/src/schedule.rs · 41-46\nfn a() {}\n```\n\n~~~\nplain\n~~~\n";
        assert_eq!(fence_infos(src), [(Some("rust".into()), Some("kiln/src/schedule.rs".into())), (None, None)]);
        let html = "<p>see</p>\n<pre><code class=\"language-rust\">fn a() {}\n</code></pre>\n<pre><code>plain\n</code></pre>\n";
        let out = code_heads(html, &fence_infos(src));
        assert!(out.contains("<div class=\"cblock\"><div class=\"chead\"><span class=\"clang\">rust</span><span class=\"cpath\">kiln/src/schedule.rs</span><button type=\"button\" class=\"ccopy\" data-act=\"copy-code\">copy</button></div><pre><code class=\"language-rust\">"), "{out}");
        assert!(out.contains("<span class=\"clang\">text</span>"), "an unlabeled fence");
        assert_eq!(out.matches("cblock").count(), 2);
    }

    #[test]
    fn codes_become_chips_and_a_code_alone_a_chip_too() {
        let html = "<p>Fixed KILN-7's test; see <code>KILN-3</code> and <a href=\"x\">KILN-4</a>.</p>\n<p>KILN-7</p>\n<p>KILN-9</p>\n";
        let out = chip_codes(html);
        assert!(out.contains("Fixed <a class=\"tchip\" href=\"#/ticket/KILN-7\">KILN-7</a>'s test"), "{out}");
        assert!(out.contains("<code>KILN-3</code>") && out.contains(">KILN-4</a>"), "code and links untouched: {out}");
        assert!(out.contains("<p><a class=\"tchip\" href=\"#/ticket/KILN-9\">KILN-9</a></p>"), "alone: a chip; its card is drawn under the turn: {out}");
    }

    #[test]
    fn a_name_in_bold_is_a_live_chip() {
        let out = chip_codes("<p>Filed <strong>HYPERFORGE-9</strong>, <strong>ENV-2..5</strong> and <strong>A-11/12</strong>; <strong>ENV-1</strong> · entity. <strong>not one</strong> <code><strong>KILN-1</strong></code></p>");
        assert!(out.contains("Filed <a class=\"tchip live\" data-ticket=\"HYPERFORGE-9\" href=\"#/ticket/HYPERFORGE-9\">HYPERFORGE-9</a>,"), "{out}");
        assert!(out.contains("data-act=\"expand\" aria-expanded=\"false\" title=\"ENV-2 · ENV-3 · ENV-4 · ENV-5\">ENV-2..5<span class=\"trn\">4</span></button><span class=\"trx\" hidden>"), "{out}");
        assert_eq!(out.matches("data-ticket=\"ENV-").count(), 5, "the run's four, hidden until tapped, and ENV-1: {out}");
        assert!(out.contains("data-ticket=\"A-12\""), "{out}");
        assert!(out.contains("</a> · entity."), "the input's word stays: {out}");
        assert!(out.contains("<strong>not one</strong>") && out.contains("<code><strong>KILN-1</strong></code>"), "{out}");
    }

    #[test]
    fn quote_link_and_the_waiting_line() {
        assert_eq!(quoted("1240 is allowed.\n\nFix the test."), "> 1240 is allowed.\n>\n> Fix the test.\n\n");
        assert_eq!(turn_link("KILN-7", 41, 9), "#/ticket/KILN-7?chat=41&at=9");
        assert_eq!(waiting_line("builder", None), "builder is waiting for your next message");
        assert_eq!(waiting_line("builder", Some(13)), "builder is waiting for your next message · 13 min left on its claim");
        assert_eq!(claim_left("agent:builder"), None, "AGENT-3: no claims yet");
    }
}
