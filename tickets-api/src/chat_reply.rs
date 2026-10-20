//! CHAT-66: **a reply is a reference, not a quote** (owner, chat 6: *"not
//! quoting the whole thing but entering the id into the message much like a
//! screenshot is"*).
//!
//! A message names the one it answers as a token, written first in its
//! body, the way an image is `![alt](media:<hash>)`: `[↩](message:<id>)`.
//! The stored body keeps only the token; the server reads it off
//! ([`reply_to_in`]) as CHAT-55 reads a body's codes, refuses one naming a
//! message of another chat, and answers each message with its
//! [`crate::chats::Quote`] — who wrote the original and its first words
//! ([`snippet`]) — so no client parses it twice. Anywhere else (a comment,
//! a ticket body) the same token is a link to the message ([`message_ref`]).
//!
//! Wasm-safe: the server, the app and the bridge's tests read one rule.

/// A message's link scheme: `message:<id>`.
pub const SCHEME: &str = "message:";

/// How many characters of the original a quote carries.
pub const SNIPPET_CHARS: usize = 80;

/// The token a reply to message `id` starts with.
pub fn token(id: u64) -> String {
    format!("[↩]({SCHEME}{id})")
}

/// The message a link's destination names: `message:<id>`, an id of
/// digits (1 or more), nothing else.
pub fn message_ref(dest: &str) -> Option<u64> {
    let n = dest.trim().strip_prefix(SCHEME)?;
    (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then(|| n.parse().ok()).flatten().filter(|n| *n > 0)
}

/// The leading token of `body`: the id it names and the bytes it spans
/// (with the blanks before it and the one line break or spaces after it).
fn lead(body: &str) -> Option<(u64, usize)> {
    let start = body.len() - body.trim_start().len();
    let rest = body[start..].strip_prefix('[')?;
    let close = rest.find(']')?;
    // The link's text is one line (`↩` as written, any other kept as is).
    if rest[..close].contains('\n') {
        return None;
    }
    let after = rest[close + 1..].strip_prefix('(')?;
    let end = after.find(')')?;
    let id = message_ref(&after[..end])?;
    let mut at = start + 1 + close + 1 + 1 + end + 1;
    let tail = &body[at..];
    let blanks = tail.len() - tail.trim_start_matches([' ', '\t']).len();
    at += blanks;
    if body[at..].starts_with("\r\n") {
        at += 2;
    } else if body[at..].starts_with('\n') {
        at += 1;
    }
    Some((id, at))
}

/// The message `body` answers: its leading `[↩](message:<id>)` — a token
/// anywhere else is a link, not a reply.
pub fn reply_to_in(body: &str) -> Option<u64> {
    lead(body).map(|(id, _)| id)
}

/// `body` without its leading token — what a turn draws under its quote.
pub fn without_token(body: &str) -> &str {
    lead(body).map_or(body, |(_, at)| &body[at..])
}

/// `body` answering message `id`: the token first, then the text.
pub fn replying(id: u64, body: &str) -> String {
    format!("{} {}", token(id), without_token(body))
}

/// What a quote says of a message: its first line of words, at most
/// [`SNIPPET_CHARS`] characters (`…` past them) — or, with no words,
/// `drawing`, `image` or `components`. Its own reply token, fences' marks
/// and markdown's leading marks are not words.
pub fn snippet(body: &str) -> String {
    let body = without_token(body);
    let mut images = Vec::new();
    let text = strip_images(body, &mut images);
    let line = text
        .lines()
        .map(|l| l.trim().trim_start_matches(['#', '>', '-', '*', ' ']).trim())
        .find(|l| !l.is_empty() && !l.starts_with("```") && !l.starts_with("~~~") && !l.starts_with('<'));
    match line {
        Some(l) => {
            let words = l.split_whitespace().collect::<Vec<_>>().join(" ");
            if words.chars().count() > SNIPPET_CHARS {
                let cut: String = words.chars().take(SNIPPET_CHARS - 1).collect();
                format!("{}…", cut.trim_end())
            } else {
                words
            }
        }
        None if body.contains('<') => "components".to_string(),
        None if !images.is_empty() && images.iter().all(|a| a == "drawing") => "drawing".to_string(),
        None if !images.is_empty() => "image".to_string(),
        None => String::new(),
    }
}

/// `body` with each `![alt](…)` taken out, its alt kept in `alts`.
fn strip_images(body: &str, alts: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(i) = rest.find("![") {
        let after = &rest[i + 2..];
        let Some(close) = after.find("](") else { break };
        let Some(end) = after[close + 2..].find(')') else { break };
        out.push_str(&rest[..i]);
        alts.push(after[..close].trim().to_string());
        rest = &after[close + 2 + end + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leading_token_is_the_reply() {
        assert_eq!(reply_to_in("[↩](message:512) yes, archive"), Some(512));
        assert_eq!(reply_to_in("  \n[↩](message:7)\nyes"), Some(7));
        assert_eq!(reply_to_in("[re](message:9) any text"), Some(9), "the link's text is free");
        assert_eq!(reply_to_in("yes [↩](message:512)"), None, "a token later is a link, not a reply");
        assert_eq!(reply_to_in("[↩](message:x)"), None);
        assert_eq!(reply_to_in("[↩](message:0)"), None);
        assert_eq!(reply_to_in("[↩](message:)"), None);
        assert_eq!(reply_to_in("[↩](media:abc)"), None);
        assert_eq!(reply_to_in("[a\nb](message:3)"), None);
        assert_eq!(reply_to_in("plain"), None);
    }

    #[test]
    fn the_token_comes_off_for_drawing() {
        assert_eq!(without_token("[↩](message:512) yes, archive"), "yes, archive");
        assert_eq!(without_token("[↩](message:512)\nyes"), "yes");
        assert_eq!(without_token("[↩](message:512)\n\n# head"), "\n# head");
        assert_eq!(without_token("no token"), "no token");
        assert_eq!(replying(4, "hi"), "[↩](message:4) hi");
        assert_eq!(replying(4, "[↩](message:3) hi"), "[↩](message:4) hi", "one reply at a time");
        assert_eq!(reply_to_in(&replying(41, "x")), Some(41));
        assert_eq!(message_ref("message:12"), Some(12));
        assert_eq!(message_ref("message:12/x"), None);
    }

    #[test]
    fn a_snippet_is_its_first_words_or_what_it_holds() {
        assert_eq!(snippet("[↩](message:3) yes\nmore"), "yes");
        assert_eq!(snippet("## The plan\n\nbody"), "The plan");
        assert_eq!(snippet("```rust\nfn x() {}\n```"), "fn x() {}");
        let long = "word ".repeat(40);
        let s = snippet(&long);
        assert_eq!(s.chars().count(), SNIPPET_CHARS);
        assert!(s.ends_with('…'));
        let h = "a".repeat(64);
        assert_eq!(snippet(&format!("![drawing](media:{h})")), "drawing");
        assert_eq!(snippet(&format!("![kiln curve](media:{h})")), "image");
        assert_eq!(snippet(&format!("![drawing](media:{h}) look here")), "look here");
        assert_eq!(snippet("<m-stat value=\"3\">done</m-stat>"), "components");
        assert_eq!(snippet(""), "");
    }
}
