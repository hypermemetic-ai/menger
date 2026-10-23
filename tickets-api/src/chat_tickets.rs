//! CHAT-55: **a chat's tickets** — the vocabulary, wasm-safe, the one
//! declaration (the store keeps them in `store::chats::mentions`; the app
//! draws them as the strip under a chat's head).
//!
//! Owner, 2026-10-19 (chat 6): *"start the work that turns this chat into a
//! ticket surface"* — CHAT-35's B, a strip pinned in the chat. A chat's
//! tickets are every ticket a message in it names ([`codes_in`], kept at
//! post time) and every ticket filed or linked from it (`tickets create
//! --chat N`, `chats link`). A deleted message's mentions drop out with it
//! (CHAT-44); restored, they come back.
//!
//! Each is in one **stage** ([`ChatStage`]), the strip's pills: *waits on
//! you · ready · in progress · in review · on test · shipped* — worked out
//! by [`stage_of`] from its status, its open inputs and where it runs, the
//! same function on the server (`chats tickets`) and in the app (live, as a
//! status, an input or a deploy moves).

use crate::types::{Kind, Status, TicketSummary};
use crate::vocab::MessageId;
use serde::{Deserialize, Serialize};

/// A ticket's code as a chat names it: capitals (and digits after the
/// first), a dash, a number from 1 — `KILN-7`, `A2B-10`. The composer's and
/// the turn's chips' rule (frontend `composer::is_code`).
pub fn is_code(s: &str) -> bool {
    let Some((head, n)) = s.split_once('-') else { return false };
    let mut hc = head.chars();
    hc.next().is_some_and(|c| c.is_ascii_uppercase())
        && hc.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && !n.is_empty()
        && !n.starts_with('0')
        && n.chars().all(|c| c.is_ascii_digit())
}

/// CHAT-57: the most tickets one run or set names — `KILN-1..500` names
/// its first 20.
pub const EXPAND_CAP: usize = 20;

/// CHAT-57: how a name in a message is written (the notation, menger-skills
/// `chatting`): `**KILN-7**`, a run `**ENV-2..5**`, a set
/// `**HYPERFORGE-11/12**`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// One ticket: `KILN-7`.
    One,
    /// A run of numbers: `ENV-2..5` is ENV-2, ENV-3, ENV-4 and ENV-5.
    Range,
    /// A set: `HYPERFORGE-11/12` is HYPERFORGE-11 and HYPERFORGE-12.
    Set,
}

/// A ticket number as written: digits, from 1, no leading zero.
fn number(s: &str) -> Option<u64> {
    (!s.is_empty() && !s.starts_with('0') && s.chars().all(|c| c.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

/// CHAT-57: one name — `KILN-7`, `ENV-2..5` or `HYPERFORGE-11/12` — read
/// whole: its form and the codes it names, in order, each once, at most
/// [`EXPAND_CAP`]. A run that does not go up, or anything else, is none.
pub fn parse_name(s: &str) -> Option<(Form, Vec<String>)> {
    if is_code(s) {
        return Some((Form::One, vec![s.to_string()]));
    }
    if let Some((first, last)) = s.split_once("..") {
        let (head, n) = first.split_once('-')?;
        let (from, to) = (number(n)?, number(last)?);
        if !is_code(first) || to <= from {
            return None;
        }
        return Some((Form::Range, (from..=to).take(EXPAND_CAP).map(|i| format!("{head}-{i}")).collect()));
    }
    let mut parts = s.split('/');
    let first = parts.next()?;
    if !is_code(first) {
        return None;
    }
    let head = first.split_once('-')?.0;
    let mut out = vec![first.to_string()];
    for p in parts {
        let code = format!("{head}-{}", number(p)?);
        if !out.contains(&code) {
            out.push(code);
        }
    }
    (out.len() > 1).then(|| (Form::Set, out.into_iter().take(EXPAND_CAP).collect()))
}

/// Every ticket code in `text`, once each, in the order first written — a
/// word on its own (`KILN-7`, `KILN-7's`, `(KILN-7)`), never part of a
/// longer word. CHAT-57: a run (`ENV-2..5`) and a set (`A-11/12`) name each
/// of theirs ([`parse_name`]); `KILN-7/KILN-8` names both.
pub fn codes_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |c: String| {
        if !out.contains(&c) {
            out.push(c);
        }
    };
    for raw in text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'))) {
        let w = raw.trim_matches(|c| matches!(c, '-' | '.' | '/'));
        match parse_name(w) {
            Some((_, codes)) => codes.into_iter().for_each(&mut add),
            None => w.split(['.', '/']).map(|p| p.trim_matches('-')).filter(|p| is_code(p)).for_each(|p| add(p.to_string())),
        }
    }
    out
}

/// CHAT-57: a name written in bold — `**ENV-2..5**` — and the input after
/// it, `**ENV-1** · entity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    /// As written between the stars: `ENV-2..5`.
    pub text: String,
    pub form: Form,
    /// The tickets it names ([`parse_name`]).
    pub codes: Vec<String>,
    /// The input asked on it: the word after ` · `.
    pub input: Option<String>,
}

/// CHAT-57: the bold names on one line — `**X**` or `__X__` whose inside is
/// a name ([`parse_name`]), each with the input after it (` · entity`, the
/// word in backticks too). Inline code is not read.
pub fn names_on_line(line: &str) -> Vec<Named> {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(at) = rest.find(['*', '_', '`']) {
        let tail = &rest[at..];
        if tail.starts_with('`') {
            // Inline code: skip to its end.
            let ticks = tail.chars().take_while(|c| *c == '`').count();
            let close = tail[ticks..].find(&tail[..ticks]).map_or(tail.len(), |e| ticks + e + ticks);
            rest = &tail[close..];
            continue;
        }
        let mark = if tail.starts_with("**") { "**" } else if tail.starts_with("__") { "__" } else {
            rest = &tail[1..];
            continue;
        };
        let inner_from = &tail[2..];
        let Some(end) = inner_from.find(mark) else { break };
        let inner = inner_from[..end].trim();
        let after = &inner_from[end + 2..];
        match parse_name(inner) {
            Some((form, codes)) => {
                out.push(Named { text: inner.to_string(), form, codes, input: input_after(after) });
                rest = after;
            }
            None => rest = &tail[2..],
        }
    }
    out
}

/// The input named after a bold name: ` · entity` (or `` · `entity` ``).
fn input_after(after: &str) -> Option<String> {
    let t = after.trim_start().strip_prefix('·')?.trim_start();
    let t = t.strip_prefix('`').unwrap_or(t);
    let w: String = t.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':')).collect();
    let w = w.trim_end_matches(['.', ':', '-']);
    (!w.is_empty()).then(|| w.to_string())
}

/// Whether `line` opens or closes a fence — its mark (```` ``` ```` or `~~~`).
fn fence_mark(line: &str) -> Option<&'static str> {
    let t = line.trim_start();
    ["```", "~~~"].into_iter().find(|m| t.starts_with(m))
}

/// CHAT-57: the bold names in a body, outside fenced code blocks, in order.
pub fn bold_names(body: &str) -> Vec<Named> {
    let mut out = Vec::new();
    let mut fence: Option<&str> = None;
    for line in body.lines() {
        match (fence, fence_mark(line)) {
            (None, Some(m)) => fence = Some(m),
            (Some(f), Some(m)) if f == m && line.trim().trim_start_matches(m.chars().next().unwrap_or('`')).is_empty() => fence = None,
            (None, None) => out.extend(names_on_line(line)),
            _ => {}
        }
    }
    out
}

/// The tickets a message's body names, as a chat keeps them: [`codes_in`]
/// its text outside fenced code blocks (a log or a test's output names
/// tickets it is not about); inline code counts. CHAT-57: all four forms of
/// the notation — `**KILN-7**`, `**ENV-2..5**`, `**A-11/12**`, `**ENV-1** ·
/// entity` — and a plain id.
pub fn mentions_in(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut fence: Option<&str> = None;
    for line in body.lines() {
        match (fence, fence_mark(line)) {
            (None, Some(m)) => fence = Some(m),
            (Some(f), Some(m)) if f == m && line.trim().trim_start_matches(m.chars().next().unwrap_or('`')).is_empty() => fence = None,
            (None, None) => {
                for c in codes_in(line) {
                    if !out.contains(&c) {
                        out.push(c);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// How a ticket came into a chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MentionHow {
    /// A message names it.
    Mentioned,
    /// Filed from the chat: `tickets create --chat N`.
    Filed,
    /// Linked to the chat: `chats link`.
    Linked,
}

impl MentionHow {
    pub const ALL: [MentionHow; 3] = [MentionHow::Mentioned, MentionHow::Filed, MentionHow::Linked];

    /// The wire and stored spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            MentionHow::Mentioned => "mentioned",
            MentionHow::Filed => "filed",
            MentionHow::Linked => "linked",
        }
    }

    /// The stored spelling read back.
    pub fn of(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|h| h.as_str() == s)
    }
}

/// CHAT-55: where a chat's ticket stands, as the strip groups it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChatStage {
    /// Pending (or an Draft) until the owner promotes it, or any open work
    /// with an open input.
    WaitsOnYou,
    /// Ready, or Blocked: not started.
    Ready,
    /// Coding.
    InProgress,
    /// In Review, running nowhere yet.
    InReview,
    /// In Review or Complete, running on an environment and not yet on the
    /// one that promotes from it (DEPLOY-21's ready for, deploying, failed).
    OnTest,
    /// Complete, and running where it goes (or nowhere to go).
    Shipped,
    /// Canceled or Superseded — not drawn.
    Closed,
    /// Not work: a scope or a doc — not drawn.
    Other,
}

impl ChatStage {
    /// The six the strip draws, in its order.
    pub const STRIP: [ChatStage; 6] = [ChatStage::WaitsOnYou, ChatStage::Ready, ChatStage::InProgress, ChatStage::InReview, ChatStage::OnTest, ChatStage::Shipped];

    /// The pill's words.
    pub const fn words(self) -> &'static str {
        match self {
            ChatStage::WaitsOnYou => "waits on you",
            ChatStage::Ready => "ready",
            ChatStage::InProgress => "in progress",
            ChatStage::InReview => "in review",
            ChatStage::OnTest => "on test",
            ChatStage::Shipped => "shipped",
            ChatStage::Closed => "closed",
            ChatStage::Other => "other",
        }
    }

    /// The wire spelling — a class and a `data-stage`.
    pub const fn as_str(self) -> &'static str {
        match self {
            ChatStage::WaitsOnYou => "waits_on_you",
            ChatStage::Ready => "ready",
            ChatStage::InProgress => "in_progress",
            ChatStage::InReview => "in_review",
            ChatStage::OnTest => "on_test",
            ChatStage::Shipped => "shipped",
            ChatStage::Closed => "closed",
            ChatStage::Other => "other",
        }
    }
}

/// CHAT-55: a ticket's stage from its `kind` (a scope or a doc is not
/// work: [`ChatStage::Other`]), its `status`, whether it has an open
/// input (`asking`), whether it is staged somewhere (`staged`: running on
/// an environment, not yet on one that promotes from it — DEPLOY-21's ready
/// for, deploying or failed) and whether it runs anywhere (`running`).
pub fn stage_of(kind: Kind, status: Status, asking: bool, staged: bool, running: bool) -> ChatStage {
    match status {
        Status::Canceled | Status::Superseded => ChatStage::Closed,
        _ if matches!(kind, Kind::Scope | Kind::Doc) => ChatStage::Other,
        Status::Epic | Status::Doc => ChatStage::Other,
        _ if asking && status != Status::Complete => ChatStage::WaitsOnYou,
        Status::Pending | Status::Draft => ChatStage::WaitsOnYou,
        Status::Ready | Status::Blocked => ChatStage::Ready,
        Status::Coding => ChatStage::InProgress,
        Status::InReview if staged || running => ChatStage::OnTest,
        Status::InReview => ChatStage::InReview,
        Status::Complete if staged => ChatStage::OnTest,
        Status::Complete => ChatStage::Shipped,
    }
}

/// CHAT-55: one of `chats tickets`' rows — a ticket the chat names, as the
/// caller reads it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChatTicket {
    pub summary: TicketSummary,
    pub stage: ChatStage,
    /// How it first came into the chat.
    pub how: MentionHow,
    /// The first message that named it (none: filed or linked without one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<MessageId>,
    /// When it first came in, unix seconds.
    pub at: i64,
    /// Its open inputs.
    #[serde(default)]
    pub asking: u32,
}

/// [`crate::chats::ChatsEvent`] is `Eq`; a row's only float is a placement's
/// rank, never NaN.
impl Eq for ChatTicket {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_names_its_codes_outside_fences() {
        let body = "Filed **CHAT-55** and CHAT-56 (after `APP-74`).\n\n```text\nKILN-7 failed\n```\nthen KILN-9, CHAT-55 again; nor A-01, nor KILN-7x.";
        assert_eq!(mentions_in(body), ["CHAT-55", "CHAT-56", "APP-74", "KILN-9"]);
        assert_eq!(mentions_in("~~~\nA-1\n~~~\nB-2"), ["B-2"]);
        assert_eq!(codes_in("(KILN-7)'s KILN-7"), ["KILN-7"]);
    }

    #[test]
    fn the_notation_reads_its_four_forms() {
        assert_eq!(parse_name("HYPERFORGE-9"), Some((Form::One, vec!["HYPERFORGE-9".to_string()])));
        assert_eq!(parse_name("ENV-2..5").map(|(f, c)| (f, c.join(","))), Some((Form::Range, "ENV-2,ENV-3,ENV-4,ENV-5".to_string())));
        assert_eq!(parse_name("HYPERFORGE-11/12").map(|(f, c)| (f, c.join(","))), Some((Form::Set, "HYPERFORGE-11,HYPERFORGE-12".to_string())));
        assert_eq!(parse_name("A-1/3/3/7").map(|(_, c)| c.join(",")), Some("A-1,A-3,A-7".to_string()));
        assert_eq!(parse_name("KILN-1..500").map(|(_, c)| c.len()), Some(EXPAND_CAP), "capped");
        for no in ["ENV-5..2", "ENV-2..2", "ENV-2..05", "A-1/", "A-1/x", "a-1..3", "ENV-2..ENV-5", "A-1/1"] {
            assert_eq!(parse_name(no), None, "{no}");
        }
        let body = "Filed **HYPERFORGE-9** (the hub), **ENV-2..5** and **HYPERFORGE-11/12**.\n\n**ENV-1** · entity — and `**KILN-1**`, **not a name**, plain CHAT-55.\n```\n**KILN-2**\n```";
        let names = bold_names(body);
        let got: Vec<(&str, Form, usize, Option<&str>)> = names.iter().map(|n| (n.text.as_str(), n.form, n.codes.len(), n.input.as_deref())).collect();
        assert_eq!(got, [("HYPERFORGE-9", Form::One, 1, None), ("ENV-2..5", Form::Range, 4, None), ("HYPERFORGE-11/12", Form::Set, 2, None), ("ENV-1", Form::One, 1, Some("entity"))]);
        assert_eq!(names_on_line("__A-2__ · `fire`.")[0].input.as_deref(), Some("fire"));
        assert_eq!(
            mentions_in(body),
            ["HYPERFORGE-9", "ENV-2", "ENV-3", "ENV-4", "ENV-5", "HYPERFORGE-11", "HYPERFORGE-12", "ENV-1", "KILN-1", "CHAT-55"],
            "every form expanded; inline code counts, a fence does not"
        );
        assert_eq!(codes_in("see KILN-7/KILN-8, then KILN-9... and A-3..4."), ["KILN-7", "KILN-8", "KILN-9", "A-3", "A-4"]);
    }

    #[test]
    fn the_stage_follows_status_inputs_and_deploys() {
        use ChatStage::*;
        assert_eq!(stage_of(Kind::Build, Status::Pending, false, false, false), WaitsOnYou);
        assert_eq!(stage_of(Kind::Build, Status::Coding, true, false, false), WaitsOnYou, "an open input waits on you");
        assert_eq!(stage_of(Kind::Build, Status::Ready, false, false, false), Ready);
        assert_eq!(stage_of(Kind::Build, Status::Coding, false, false, false), InProgress);
        assert_eq!(stage_of(Kind::Build, Status::InReview, false, false, false), InReview);
        assert_eq!(stage_of(Kind::Build, Status::InReview, false, false, true), OnTest, "merged and deployed to test");
        assert_eq!(stage_of(Kind::Build, Status::Complete, false, true, true), OnTest, "ready for prod");
        assert_eq!(stage_of(Kind::Build, Status::Complete, false, false, true), Shipped);
        assert_eq!(stage_of(Kind::Build, Status::Complete, true, false, false), Shipped, "a done ticket's leftover input waits on no one");
        assert_eq!(stage_of(Kind::Build, Status::Canceled, false, false, false), Closed);
        assert_eq!(stage_of(Kind::Doc, Status::Doc, false, false, false), Other);
        assert_eq!(stage_of(Kind::Scope, Status::Pending, false, false, false), Other, "a scope is not work");
    }
}
