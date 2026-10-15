//! COMMENT-2: the comments vocabulary — the one declaration.
//!
//! A **comment** is a reader's note left on a part of the app: a page, an
//! element on it, the body of a ticket (COMMENT-1, decided 2026-10-08:
//! "comments are their own records in menger's store, not tickets, reached
//! as `menger comments …`"). It is kept by the server beside the tickets
//! (`store::comments`) and reached over the `comments` activation
//! ([`crate::comments_activation`]); the browser app reads *these* types,
//! as it reads the tickets' (MFE-9), so a renamed field fails its build.
//!
//! Wasm-safe, with no feature gate: the checks below ([`check`]) are the
//! server's rules for what it keeps, and the app may hold a comment box to
//! them before it sends one.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Who a comment is from when `add` names no author.
pub const DEFAULT_AUTHOR: &str = "owner";

/// The longest body a comment keeps, in bytes.
pub const BODY_MAX: usize = 65_536;

/// The longest `route`, `path`, `label` or `excerpt` an anchor keeps, in
/// bytes — an excerpt is "what was there, in words", not the element's
/// whole text: a client cuts it to fit.
pub const ANCHOR_FIELD_MAX: usize = 4_096;

/// The longest author a comment keeps, in bytes.
pub const AUTHOR_MAX: usize = 256;

/// Where a comment was left: the app page, the place on it, what was there
/// in words, and the ticket the element belongs to (when it belongs to one).
///
/// `route`, `path` and `label` are required and not blank; `excerpt` may be
/// empty (an element with no text). Each is trimmed. They are `#[serde(default)]`
/// so that one left out arrives as empty and is refused *in band*, naming
/// it — not as a transport error before the method runs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Anchor {
    /// The app page, as its route: `#/ticket/DEMAND-2`, `#/waves/RETURNS`,
    /// `#/` (the board) — always beginning `#/`.
    #[serde(default)]
    pub route: String,
    /// Where on the page: the element's path, e.g.
    /// `[data-tag="ticket DEMAND-1"]`. `list { path }` matches a prefix of it.
    #[serde(default)]
    pub path: String,
    /// What was there, in words: the element's name, e.g. `Parent chip`.
    #[serde(default)]
    pub label: String,
    /// What was there, in words: its text, e.g. `DEMAND-1` (may be empty).
    #[serde(default)]
    pub excerpt: String,
    /// The ticket the element belongs to, e.g. `DEMAND-2` — omitted when it
    /// belongs to none. It must name a ticket; kept as that ticket's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<String>,
    /// Set by the server, never by the caller: `ticket`'s `updated_unix`
    /// when the comment was added — so a reader can tell the ticket has
    /// changed since (its body may no longer hold `path`). Omitted with no
    /// `ticket`; a value sent is replaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<i64>,
}

/// Where a comment stands: `open` until it is resolved; `resolve` and
/// `reopen` flip it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentState {
    Open,
    Resolved,
}

impl CommentState {
    pub const ALL: [CommentState; 2] = [CommentState::Open, CommentState::Resolved];

    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            CommentState::Open => "open",
            CommentState::Resolved => "resolved",
        }
    }
}

impl fmt::Display for CommentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for CommentState {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "open" => Ok(Self::Open),
            "resolved" => Ok(Self::Resolved),
            other => Err(format!("unknown comment state '{other}' (expected open or resolved)")),
        }
    }
}

/// APP-23: what a comment is for — a comment (the default), or a change
/// request: the owner handing a queued ticket back to whoever queued it
/// (`tickets.request_changes`, the tray's Request changes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentIntent {
    #[default]
    Comment,
    Changes,
}

impl CommentIntent {
    pub const ALL: [CommentIntent; 2] = [CommentIntent::Comment, CommentIntent::Changes];

    /// The wire spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            CommentIntent::Comment => "comment",
            CommentIntent::Changes => "changes",
        }
    }

    /// The default: left off the wire, so a plain comment is as it was.
    pub fn is_comment(&self) -> bool {
        *self == CommentIntent::Comment
    }
}

impl FromStr for CommentIntent {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        CommentIntent::ALL
            .into_iter()
            .find(|i| i.as_str().eq_ignore_ascii_case(s.trim()))
            .ok_or_else(|| format!("unknown comment intent '{}' (expected comment or changes)", s.trim()))
    }
}

/// One comment, as kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Comment {
    /// Its id: 1, 2, … in the order comments were added, never reused.
    pub id: crate::vocab::CommentId,
    pub state: CommentState,
    /// Who left it (`owner` unless `add` named someone).
    pub author: String,
    /// When it was added, unix seconds.
    pub at: i64,
    pub anchor: Anchor,
    /// What it says (markdown, as written).
    pub body: String,
    /// IDENTITY-21: the Plexus principal of who left it, when they were
    /// signed in — `author` is only the name they go by.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal: Option<String>,
    /// APP-23: `changes` for a change request; omitted for a comment.
    #[serde(default, skip_serializing_if = "CommentIntent::is_comment")]
    pub intent: CommentIntent,
}

/// What the `comments` activation's methods yield. In-band errors are
/// `Error { message }`, as every menger method's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CommentsEvent {
    /// One comment as it is now, its fields beside `type` — `add`'s,
    /// `resolve`'s and `reopen`'s answer, and each of `list`'s rows.
    /// [`CommentsEvent::into_comment`] gives the [`Comment`].
    Comment {
        id: u64,
        state: CommentState,
        author: String,
        at: i64,
        anchor: Anchor,
        body: String,
        /// IDENTITY-21: the signer's principal, when signed in.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        principal: Option<String>,
        /// APP-23: `changes` for a change request; omitted for a comment.
        #[serde(default, skip_serializing_if = "CommentIntent::is_comment")]
        intent: CommentIntent,
    },
    /// Where a `list` stands, once, after its rows: `next` is the cursor to
    /// pass as the next call's `after` (a comment id), `null` when nothing
    /// more matches.
    Page { next: Option<u64> },
    /// `watch`: a comment was added.
    Added { comment: Comment },
    /// `watch`: a comment was resolved.
    Resolved { comment: Comment },
    /// `watch`: a resolved comment was reopened.
    Reopened { comment: Comment },
    /// `watch`: this subscriber fell behind and `missed` changes were
    /// dropped — `list` again to catch up.
    Lagged { missed: u64 },
    /// ACCESS-12 (`watch`): the watch ended — its caller, verified again
    /// when the grants moved or its sign-in ended, was refused. Last.
    WatchClosed { reason: crate::types::WatchEnd, message: String },
    /// A refusal, naming the problem; nothing was kept or changed.
    Error { message: String },
}

impl From<Comment> for CommentsEvent {
    fn from(c: Comment) -> Self {
        let Comment { id, state, author, at, anchor, body, principal, intent } = c;
        CommentsEvent::Comment { id: id.get(), state, author, at, anchor, body, principal, intent }
    }
}

impl CommentsEvent {
    /// The comment this event carries — a `comment` row's fields, or what
    /// `added`, `resolved` and `reopened` carry — and `None` for the rest.
    pub fn into_comment(self) -> Option<Comment> {
        match self {
            CommentsEvent::Comment { id, state, author, at, anchor, body, principal, intent } => {
                Some(Comment { id: crate::vocab::CommentId::new(id), state, author, at, anchor, body, principal, intent })
            }
            CommentsEvent::Added { comment } | CommentsEvent::Resolved { comment } | CommentsEvent::Reopened { comment } => {
                Some(comment)
            }
            CommentsEvent::Page { .. } | CommentsEvent::Lagged { .. } | CommentsEvent::WatchClosed { .. } | CommentsEvent::Error { .. } => None,
        }
    }
}

/// Why `add` keeps nothing — each names the problem. (A `ticket` naming no
/// ticket is the store's to say: only it knows the tickets.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The body is empty, or only whitespace.
    EmptyBody,
    /// The body is longer than [`BODY_MAX`].
    BodyTooLong { bytes: usize },
    /// An author was given and is blank.
    BlankAuthor,
    /// The author is longer than [`AUTHOR_MAX`].
    AuthorTooLong { bytes: usize },
    /// The anchor's `field` is malformed — `why`.
    Anchor { field: &'static str, why: String },
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Problem::EmptyBody => f.write_str("the comment's body is empty"),
            Problem::BodyTooLong { bytes } => write!(f, "the comment's body is {bytes} bytes, past the {BODY_MAX} a comment keeps"),
            Problem::BlankAuthor => f.write_str("author is blank (leave it out for `owner`)"),
            Problem::AuthorTooLong { bytes } => write!(f, "author is {bytes} bytes, past the {AUTHOR_MAX} a comment keeps"),
            Problem::Anchor { field, why } => write!(f, "malformed anchor: {field} {why}"),
        }
    }
}

/// A comment as `add` would keep it, before the store fills in its id, its
/// time and the ticket's `version`: the anchor trimmed (its `version`
/// cleared, its `ticket` upper-cased), the author (`owner` for none,
/// trimmed), the body as given — or the first problem, naming it.
pub fn check(mut anchor: Anchor, body: &str, author: Option<&str>) -> Result<(Anchor, String), Problem> {
    for (field, value, required) in [
        ("route", &mut anchor.route, true),
        ("path", &mut anchor.path, true),
        ("label", &mut anchor.label, true),
        ("excerpt", &mut anchor.excerpt, false),
    ] {
        let trimmed = value.trim();
        if required && trimmed.is_empty() {
            return Err(Problem::Anchor { field, why: "is missing or empty".into() });
        }
        if trimmed.len() > ANCHOR_FIELD_MAX {
            return Err(Problem::Anchor {
                field,
                why: format!("is {} bytes, past the {ANCHOR_FIELD_MAX} an anchor keeps", trimmed.len()),
            });
        }
        if trimmed.len() != value.len() {
            *value = trimmed.to_string();
        }
    }
    if !anchor.route.starts_with("#/") {
        return Err(Problem::Anchor {
            field: "route",
            why: format!("'{}' is not an app page: a route begins #/ (e.g. #/ticket/DEMAND-2, or #/ for the board)", anchor.route),
        });
    }
    if let Some(ticket) = anchor.ticket.as_mut() {
        let id = ticket.trim().to_ascii_uppercase();
        if id.is_empty() {
            return Err(Problem::Anchor {
                field: "ticket",
                why: "is empty (leave it out when the element belongs to no ticket)".into(),
            });
        }
        *ticket = id;
    }
    anchor.version = None;
    if body.trim().is_empty() {
        return Err(Problem::EmptyBody);
    }
    if body.len() > BODY_MAX {
        return Err(Problem::BodyTooLong { bytes: body.len() });
    }
    let author = match author {
        None => DEFAULT_AUTHOR.to_string(),
        Some(a) if a.trim().is_empty() => return Err(Problem::BlankAuthor),
        Some(a) if a.trim().len() > AUTHOR_MAX => return Err(Problem::AuthorTooLong { bytes: a.trim().len() }),
        Some(a) => a.trim().to_string(),
    };
    Ok((anchor, author))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn anchor() -> Anchor {
        Anchor {
            route: "#/ticket/DEMAND-2".into(),
            path: "[data-tag=\"ticket DEMAND-1\"]".into(),
            label: "Parent chip".into(),
            excerpt: "DEMAND-1".into(),
            ticket: Some("DEMAND-2".into()),
            version: None,
        }
    }

    /// The wire shape the ticket's example shows: a `comment` event is the
    /// comment's own fields beside `type`, in that order; an anchor without
    /// a ticket omits `ticket` and `version`; the state is lowercase.
    #[test]
    fn a_comment_event_is_the_comment_beside_its_type() {
        let c = Comment {
            id: crate::vocab::CommentId::new(1),
            state: CommentState::Open,
            author: "owner".into(),
            at: 1_791_510_000,
            anchor: Anchor { version: Some(1_791_500_000), ..anchor() },
            body: "show the parent's title too".into(),
            principal: None,
            intent: CommentIntent::Comment,
        };
        let ev = serde_json::to_value(CommentsEvent::from(c.clone())).unwrap();
        assert_eq!(
            ev,
            json!({
                "type": "comment", "id": 1, "state": "open", "author": "owner", "at": 1_791_510_000,
                "anchor": {
                    "route": "#/ticket/DEMAND-2", "path": "[data-tag=\"ticket DEMAND-1\"]", "label": "Parent chip",
                    "excerpt": "DEMAND-1", "ticket": "DEMAND-2", "version": 1_791_500_000
                },
                "body": "show the parent's title too"
            })
        );
        let mut keys: Vec<&str> = ev.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["anchor", "at", "author", "body", "id", "state", "type"], "the comment's fields and its type, nothing else");
        let back: CommentsEvent = serde_json::from_value(ev).unwrap();
        assert_eq!(back.clone().into_comment(), Some(c.clone()));
        let added = serde_json::to_value(CommentsEvent::Added { comment: c.clone() }).unwrap();
        assert_eq!((added["type"].as_str(), added["comment"]["id"].as_u64()), (Some("added"), Some(1)));
        assert_eq!(serde_json::to_value(CommentsEvent::Page { next: None }).unwrap(), json!({"type": "page", "next": null}));
        let bare = Anchor { ticket: None, ..anchor() };
        assert_eq!(serde_json::to_value(&bare).unwrap().as_object().unwrap().len(), 4, "no ticket, no version");
        // APP-23: a change request says so; a comment says nothing of it.
        let changes = serde_json::to_value(CommentsEvent::from(Comment { intent: CommentIntent::Changes, ..c.clone() })).unwrap();
        assert_eq!(changes["intent"], json!("changes"));
        assert_eq!("Changes".parse::<CommentIntent>(), Ok(CommentIntent::Changes));
        assert!("praise".parse::<CommentIntent>().unwrap_err().contains("comment or changes"));
        let resolved = serde_json::to_value(CommentState::Resolved).unwrap();
        assert_eq!(resolved, json!("resolved"));
    }

    /// A field left out of an anchor arrives empty — and is refused in
    /// band by [`check`], naming it.
    #[test]
    fn a_missing_anchor_field_arrives_empty_and_is_named() {
        let a: Anchor = serde_json::from_value(json!({"path": "p", "label": "l"})).unwrap();
        assert_eq!(a.route, "");
        assert_eq!(
            check(a, "x", None).unwrap_err().to_string(),
            "malformed anchor: route is missing or empty"
        );
    }

    #[test]
    fn check_keeps_a_good_comment_trimmed_and_names_every_problem() {
        let (a, author) = check(
            Anchor {
                route: " #/ticket/DEMAND-2 ".into(),
                ticket: Some(" demand-2 ".into()),
                version: Some(7),
                excerpt: "  ".into(),
                ..anchor()
            },
            "  a note  ",
            None,
        )
        .unwrap();
        assert_eq!((a.route.as_str(), a.ticket.as_deref(), a.version, a.excerpt.as_str()), ("#/ticket/DEMAND-2", Some("DEMAND-2"), None, ""));
        assert_eq!(author, "owner");
        assert_eq!(check(anchor(), "x", Some(" agent-7 ")).unwrap().1, "agent-7");

        let refused = |a: Anchor, body: &str, author: Option<&str>| check(a, body, author).unwrap_err().to_string();
        assert_eq!(refused(anchor(), " \n\t", None), "the comment's body is empty");
        assert!(refused(anchor(), &"x".repeat(BODY_MAX + 1), None).contains("65537 bytes"));
        assert_eq!(refused(anchor(), "x", Some("  ")), "author is blank (leave it out for `owner`)");
        assert!(refused(anchor(), "x", Some(&"a".repeat(AUTHOR_MAX + 1))).starts_with("author is 257 bytes"));
        assert_eq!(refused(Anchor { path: " ".into(), ..anchor() }, "x", None), "malformed anchor: path is missing or empty");
        assert_eq!(refused(Anchor { label: String::new(), ..anchor() }, "x", None), "malformed anchor: label is missing or empty");
        assert!(refused(Anchor { route: "ticket/DEMAND-2".into(), ..anchor() }, "x", None)
            .starts_with("malformed anchor: route 'ticket/DEMAND-2' is not an app page"));
        assert!(refused(Anchor { excerpt: "e".repeat(ANCHOR_FIELD_MAX + 1), ..anchor() }, "x", None)
            .starts_with("malformed anchor: excerpt is 4097 bytes"));
        assert!(refused(Anchor { ticket: Some(" ".into()), ..anchor() }, "x", None).starts_with("malformed anchor: ticket is empty"));
        // The anchor is checked before the body: the first problem is named.
        assert_eq!(refused(Anchor { route: String::new(), ..anchor() }, "", None), "malformed anchor: route is missing or empty");
    }

    #[test]
    fn states_parse_whatever_their_case_and_name_the_two() {
        assert_eq!("Open".parse::<CommentState>(), Ok(CommentState::Open));
        assert_eq!(" RESOLVED ".parse::<CommentState>(), Ok(CommentState::Resolved));
        assert_eq!(
            "closed".parse::<CommentState>().unwrap_err(),
            "unknown comment state 'closed' (expected open or resolved)"
        );
        for s in CommentState::ALL {
            assert_eq!(s.as_str().parse::<CommentState>(), Ok(s));
        }
    }
}
