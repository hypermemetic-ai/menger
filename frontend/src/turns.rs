//! CHAT-7: a chat's turns, as a traditional agent chat draws them — the
//! dock's chat and the full-screen session alike, only narrower in the dock.
//!
//! - **Your turns** are bubbles on the right, in the selection token, with
//!   the time under them. **Others' turns** are full-width prose under one
//!   mono line: a mark (an agent square, a person round), the name, an agent
//!   tag, `via` and the time.
//! - **Code blocks** get a head ([`code_heads`]): the language, the path the
//!   fence names after it (```` ```rust kiln/src/schedule.rs ````), *copy*.
//! - **Ticket codes** become chips, and a code alone on its line a card with
//!   the ticket's title and status ([`chip_codes`]) — `/ticket CODE` posts one.
//! - A turn shows **copy · quote · link** on hover: its text to the
//!   clipboard, `> quoted` into the composer, its URL (`?chat=ID&at=MSG`).
//! - **Scrolling** — manifest's two rules (`conversationScroll.ts`), in
//!   [`Scroll`]: opening lands on the read mark (or the newest) once, with
//!   no animation, and a message arriving never moves you. More than
//!   [`OFF_BOTTOM_PX`] off the bottom, a round **↓ N new · jump to latest**
//!   shows; at the bottom, new turns appear in place and keep you there.
//!
//! What decides is pure and pinned by native tests; the rest is the DOM.

use menger_tickets_api::Message;

/// manifest's `conversation.offBottomPx`: past this, you are reading back.
pub(crate) const OFF_BOTTOM_PX: f64 = 48.0;

/// Whether a scroller is more than [`OFF_BOTTOM_PX`] off its bottom.
pub(crate) fn off_bottom(scroll_height: f64, scroll_top: f64, client_height: f64) -> bool {
    scroll_height - scroll_top - client_height > OFF_BOTTOM_PX
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
    /// Turns that arrived below while you read back.
    pub(crate) unseen: u32,
}

impl Default for Scroll {
    fn default() -> Self {
        Scroll { landed: false, at_bottom: true, unseen: 0 }
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
    /// there). Off it, nothing moves; they are counted for the arrow.
    pub(crate) fn arrived(&mut self, n: u32) -> bool {
        if self.at_bottom {
            return true;
        }
        self.unseen += n;
        false
    }

    /// The reader scrolled (or the content moved under them).
    pub(crate) fn moved(&mut self, at_bottom: bool) {
        self.at_bottom = at_bottom;
        if at_bottom {
            self.unseen = 0;
        }
    }

    /// Rule 2: the arrow's words while off the bottom.
    pub(crate) fn arrow(&self) -> Option<String> {
        (!self.at_bottom).then(|| match self.unseen {
            0 => "↓ jump to latest".to_string(),
            n => format!("↓ {n} new · jump to latest"),
        })
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

/// The rendered body with each ticket code in its text a chip, and a code
/// alone in its paragraph a card — `card(code)` gives the card's title and
/// status when the app holds the ticket. Nothing inside `<pre>`, `<code>`
/// or `<a>` is touched.
pub(crate) fn chip_codes(html: &str, card: &dyn Fn(&str) -> Option<(String, String)>) -> String {
    let mut out = String::with_capacity(html.len() + 64);
    let mut skip = 0usize;
    let mut rest = html;
    while !rest.is_empty() {
        if let Some(stripped) = rest.strip_prefix('<') {
            let end = stripped.find('>').map_or(rest.len(), |e| e + 2);
            let tag = &rest[..end];
            let name: String = tag.trim_start_matches(['<', '/']).chars().take_while(char::is_ascii_alphanumeric).collect();
            if matches!(name.as_str(), "pre" | "code" | "a") {
                if tag.starts_with("</") {
                    skip = skip.saturating_sub(1);
                } else {
                    skip += 1;
                }
            }
            // `<p>CODE</p>`: a card.
            if tag == "<p>" && skip == 0 {
                if let Some(close) = rest[end..].find("</p>") {
                    let inner = rest[end..end + close].trim();
                    if crate::composer::is_code(inner) {
                        out.push_str(&card_html(inner, card(inner)));
                        rest = &rest[end + close + "</p>".len()..];
                        continue;
                    }
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
            out.push_str(&format!("<a class=\"tchip\" href=\"#/ticket/{w}\">{w}</a>"));
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

fn card_html(code: &str, held: Option<(String, String)>) -> String {
    let (title, status) = held.unwrap_or_default();
    let status_html = if status.is_empty() { String::new() } else { format!("<span class=\"tcstatus st st-{}\">{}</span>", escape(&status.to_ascii_lowercase().replace(' ', "-")), escape(&status)) };
    format!("<a class=\"tcard\" href=\"#/ticket/{code}\"><span class=\"tchip\">{code}</span><span class=\"tctitle\">{}</span>{status_html}</a>", escape(&title))
}

/// A message's body as a turn draws it: rendered, code heads, chips.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn body_html(m: &Message, card: &dyn Fn(&str) -> Option<(String, String)>) -> String {
    source_html(&m.body, card)
}

/// Markdown as a turn draws it — a whole body, or a long one's head or a
/// chunk of it (CHAT-8).
pub(crate) fn source_html(src: &str, card: &dyn Fn(&str) -> Option<(String, String)>) -> String {
    let html = crate::md::render_body(src).html;
    chip_codes(&code_heads(&html, &fence_infos(src)), card)
}

/// The text a turn's *quote* puts in the composer.
pub(crate) fn quoted(body: &str) -> String {
    let mut q: String = body.trim_end().lines().map(|l| if l.is_empty() { ">".to_string() } else { format!("> {l}") }).collect::<Vec<_>>().join("\n");
    q.push_str("\n\n");
    q
}

/// A turn's link: the ticket with the chat open at the message.
pub(crate) fn turn_link(ticket: &str, chat: u64, message: u64) -> String {
    format!("#/ticket/{ticket}?chat={chat}&at={message}")
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
    /// bottom; ↓ appears past 48 px with what came in since.
    #[test]
    fn the_scroll_rules() {
        let mut s = Scroll::default();
        assert_eq!(s.land(Some(7)), Some(Landing::At(7)), "opening lands on the read mark");
        assert_eq!(s.land(None), None, "once");
        let mut n = Scroll::default();
        assert_eq!(n.land(None), Some(Landing::Newest), "or the newest");
        // At the bottom: arrivals keep you there.
        assert!(s.arrived(1));
        assert_eq!(s.arrow(), None);
        // Read back: an arrival moves nothing, and is counted.
        s.moved(false);
        assert_eq!(s.arrow().as_deref(), Some("↓ jump to latest"));
        assert!(!s.arrived(2));
        assert!(!s.arrived(1));
        assert_eq!(s.arrow().as_deref(), Some("↓ 3 new · jump to latest"));
        s.moved(true);
        assert_eq!((s.unseen, s.arrow()), (0, None), "back at the bottom: the arrow goes");
        assert!(!off_bottom(1000.0, 552.0, 400.0), "48 px off is still the bottom");
        assert!(off_bottom(1000.0, 551.0, 400.0), "49 px is reading back");
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
    fn codes_become_chips_and_a_code_alone_a_card() {
        let card = |c: &str| (c == "KILN-7").then(|| ("Cone 6 glaze firing".to_string(), "In Review".to_string()));
        let html = "<p>Fixed KILN-7's test; see <code>KILN-3</code> and <a href=\"x\">KILN-4</a>.</p>\n<p>KILN-7</p>\n<p>KILN-9</p>\n";
        let out = chip_codes(html, &card);
        assert!(out.contains("Fixed <a class=\"tchip\" href=\"#/ticket/KILN-7\">KILN-7</a>'s test"), "{out}");
        assert!(out.contains("<code>KILN-3</code>") && out.contains(">KILN-4</a>"), "code and links untouched: {out}");
        assert!(out.contains("<a class=\"tcard\" href=\"#/ticket/KILN-7\"><span class=\"tchip\">KILN-7</span><span class=\"tctitle\">Cone 6 glaze firing</span><span class=\"tcstatus st st-in-review\">In Review</span></a>"), "{out}");
        assert!(out.contains("<a class=\"tcard\" href=\"#/ticket/KILN-9\"><span class=\"tchip\">KILN-9</span><span class=\"tctitle\"></span></a>"), "a ticket not held: the code alone");
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
