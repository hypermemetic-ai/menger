//! FILTER-27: the filter menu's **When** category — the time clauses of
//! FILTER-10's language as editable pills.
//!
//! - **A time pill** ([`Timed`]) is one group holding one time predicate
//!   (`created <window>`, `entered S [window] [by who]`, `left …`,
//!   `changed …`, `[status S | category C] for over|under D`), `not` or
//!   not. Any such group typed in the bar's `text` mode comes back as one,
//!   and a pill prints the canonical text ([`group`]): the round trip is
//!   the language's own.
//! - **Its segments** ([`segments`]): the event's words, then each part a
//!   click reopens on its own — the status (or the statuses a `for` asks
//!   of), the window (or the length), who — and the whole in words
//!   ([`words`]): "created today", "Pending for over 2 weeks", "moved into
//!   Ready in the last 2 days by owner". The lens bar reads it so.
//! - **FILTER-55**: the menu's When pane (src/when_view.rs) is gone — the
//!   builder (src/fcard.rs) sets a rule's time on its rule; what is left
//!   here reads a time pill in words, and builds one for the tests.
//! - **FILTER-30**: Hasn't changed is `not touched <window>` — the ticket's
//!   last touch (any write to it, a comment, an answer) outside the window:
//!   "hasn't changed in 2 weeks".
//! - **DEPLOY-13**: `deployed <env> [window]` is a time pill too — its
//!   environment a part of its own ([`Part::Env`]).
//! - **DEPLOY-21**: a delivery stage — `ready for <env>`, `deploying
//!   <env>`, `waiting <env>`, `failed <env>` — is a time pill too, its
//!   environment its part.
//!
//! Everything that decides is a pure function here, pinned by native tests.

use menger_tickets_api::deployments::DeliveryStage;
use menger_tickets_api::filter::{Event, Field, Filter, Held, HeldOf, History, Moment, Unit, When, Who, Window};
#[cfg(test)]
use menger_tickets_api::filter::{Group, Predicate};
#[cfg(test)]
use menger_tickets_api::Status;

/// A time pill's group: whether it is negated, and its predicate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Timed {
    pub(crate) not: bool,
    pub(crate) h: History,
}

#[cfg(test)]
/// The group as a time pill: one time predicate, alone in its group.
pub(crate) fn of_group(g: &Group) -> Option<Timed> {
    match g.preds.as_slice() {
        [p] => match &p.field {
            Field::History(h) => Some(Timed { not: p.not, h: h.clone() }),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
/// The pill as the language's group.
pub(crate) fn group(t: &Timed) -> Group {
    let field = Field::History(t.h.clone());
    Group::one(if t.not { Predicate::is_not(field) } else { Predicate::is(field) })
}

/// What the When menu builds — one of its six rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Created,
    Into,
    OutOf,
    Held,
    Changed,
    /// FILTER-30: `touched` — built negated: hasn't changed.
    Untouched,
    /// DEPLOY-13: `deployed <env> [window]` — the filter menu's Deployed
    /// category (src/filter.rs), not When's rows.
    Deployed,
    /// DEPLOY-20: `behind <env>` — running an earlier version there; a pill
    /// (from the palette or a saved filter), never a row of a menu.
    Behind,
    /// DEPLOY-21: `<stage> <env>` — a delivery stage there; the Deployed
    /// category's rows (src/when_view.rs).
    Stage(DeliveryStage),
    /// DEPLOY-25: `requested <env>` — a pending request there would carry
    /// it; a pill (from the palette, a saved filter or a lane), never a row
    /// of a menu.
    Requested,
    /// MENGER-39: `queued [urgency]` — the owner's queue, kept by the
    /// store; a pill (from the palette or a saved filter), never a row of
    /// When's menu.
    Queued,
    /// ARCHIVE-2: `archived <window> [by who]` — a pill, never a row of
    /// When's menu (the Archive view is ARCHIVE-3's).
    Archived,
    /// ARCHIVE-2: `restored [window] [by who]` — a pill, as `archived`.
    Restored,
    /// CHAT-3: `chatting` / `unread` — the reader's chats, kept by the
    /// store; a pill (from the palette or a saved filter), never a row of
    /// When's menu. CHAT-4 draws them.
    Chats,
    /// TICKETS-9: `assigned [who]` / `unassigned` — who holds it, kept by
    /// the store; the People category's cells, never a row of When's menu.
    People,
}

impl Kind {

    pub(crate) fn of(e: &Event) -> Kind {
        match e {
            Event::Created => Kind::Created,
            Event::Entered(_) => Kind::Into,
            Event::Left(_) => Kind::OutOf,
            Event::Held(_) => Kind::Held,
            Event::Changed => Kind::Changed,
            Event::Touched => Kind::Untouched,
            Event::Deployed(_) => Kind::Deployed,
            Event::Behind(_) => Kind::Behind,
            Event::Stage(s, _) => Kind::Stage(*s),
            Event::Requested(_) => Kind::Requested,
            Event::Queued(_) => Kind::Queued,
            Event::Chatting | Event::Unread | Event::Asked | Event::InChat(_) => Kind::Chats,
            Event::Assigned(_) | Event::Unassigned => Kind::People,
            Event::Archived => Kind::Archived,
            Event::Restored => Kind::Restored,
            // FILTER-52: a rule with a time — until FILTER-54 draws it on
            // its rule, a status's is Moved into's pill, a kind's Changed's.
            Event::Became(menger_tickets_api::filter::Became::Status(_)) => Kind::Into,
            Event::Became(menger_tickets_api::filter::Became::Kind(_)) => Kind::Changed,
        }
    }

    /// Whether its clause has a window (all but In status for).
    pub(crate) fn windowed(self) -> bool {
        !matches!(self, Kind::Held | Kind::Queued | Kind::Behind | Kind::Stage(_) | Kind::Chats | Kind::People)
    }

    /// Whether its clause may say who (the transitions'; not a birth).
    pub(crate) fn has_who(self) -> bool {
        matches!(self, Kind::Into | Kind::OutOf | Kind::Changed | Kind::Archived | Kind::Restored)
    }

}

/// A part of a clause its pill's segment reopens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Part {
    Status,
    /// DEPLOY-13: the environment `deployed` names.
    Env,
    Window,
    Length,
    Who,
}

impl Part {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Part::Status => "status",
            Part::Env => "env",
            Part::Window => "window",
            Part::Length => "length",
            Part::Who => "who",
        }
    }
}

/// A window as the language reads it (`today`, `within 3d`, `before 2w
/// ago`) — `None` when it is none.
pub(crate) fn window_of(text: &str) -> Option<Window> {
    let f = Filter::parse(&format!("moved {text}")).ok()?;
    match &f.groups.first()?.preds.first()?.field {
        Field::History(h) if f.groups.len() == 1 => h.window.clone(),
        _ => None,
    }
}

#[cfg(test)]
/// A new clause of `kind` — a status's for Moved into / out of, a preset's
/// window (`None`: ever) for the rest; In status for, over a length;
/// Hasn't changed, `not touched` (in two weeks unless a window is given).
pub(crate) fn new_clause(kind: Kind, status: Option<Status>, window: Option<Window>) -> Option<Timed> {
    let event = match kind {
        Kind::Created => Event::Created,
        Kind::Into => Event::Entered(status?),
        Kind::OutOf => Event::Left(status?),
        Kind::Changed => Event::Changed,
        Kind::Held => Event::Held(Held { of: HeldOf::Any, over: true, n: 1, unit: Unit::Week }),
        Kind::Untouched => Event::Touched,
        // DEPLOY-13: names an environment first ([`new_deployed`];
        // DEPLOY-21: [`new_stage`]).
        Kind::Deployed | Kind::Stage(_) => return None,
        // MENGER-39 / DEPLOY-20: never built by When's menu.
        Kind::Queued | Kind::Behind | Kind::Chats | Kind::People | Kind::Requested => return None,
        // ARCHIVE-2: the archive's two ends.
        Kind::Archived => Event::Archived,
        Kind::Restored => Event::Restored,
    };
    let window = match kind {
        Kind::Created | Kind::Untouched => Some(window.unwrap_or_else(|| default_window(&event))),
        Kind::Held => None,
        _ => window,
    };
    Some(Timed { not: kind == Kind::Untouched, h: History { event, window, by: None } })
}

#[cfg(test)]
/// CHAT-4: `chatting` (`unread: false`) or `unread` — the filter menu's
/// Chats category builds them; no window, no who.
pub(crate) fn new_chats(unread: bool) -> Timed {
    Timed { not: false, h: History { event: if unread { Event::Unread } else { Event::Chatting }, window: None, by: None } }
}

#[cfg(test)]
/// TICKETS-9: the People category's cells — `assigned me` (`Some(Me)`),
/// `assigned "<principal>"`, `assigned` (`None`: anyone) or, `unassigned`,
/// `unassigned`; no window, no who.
pub(crate) fn new_people(who: Option<menger_tickets_api::filter::Who>, unassigned: bool) -> Timed {
    let event = if unassigned { Event::Unassigned } else { Event::Assigned(who) };
    Timed { not: false, h: History { event, window: None, by: None } }
}

/// TICKETS-9: whether a time pill is one of the People category's cells.
pub(crate) fn is_people(t: &Timed) -> bool {
    matches!(t.h.event, Event::Assigned(_) | Event::Unassigned)
}

/// CHAT-4: whether a time pill is one of the reader's chats' cells.
pub(crate) fn is_chats(t: &Timed) -> bool {
    matches!(t.h.event, Event::Chatting | Event::Unread)
}

#[cfg(test)]
/// The window a clause that must have one falls back to: a birth's today, a
/// touch's two weeks.
fn default_window(e: &Event) -> Window {
    match e {
        Event::Touched => Window::Within(2, Unit::Week),
        _ => Window::Today,
    }
}

#[cfg(test)]
/// DEPLOY-13: `deployed <env> [window]` — `not` for Not deployed to —
/// or `None` when `env` is no environment name (`prod`, `menger/prod`).
pub(crate) fn new_deployed(env: &str, not: bool, window: Option<Window>) -> Option<Timed> {
    let env = menger_tickets_api::deployments::environment_of(env).ok()?;
    Some(Timed { not, h: History { event: Event::Deployed(env), window, by: None } })
}

#[cfg(test)]
/// DEPLOY-21: `<stage> <env>` — or `None` when `env` is no environment
/// name.
pub(crate) fn new_stage(stage: DeliveryStage, env: &str) -> Option<Timed> {
    let env = menger_tickets_api::deployments::environment_of(env).ok()?;
    Some(Timed { not: false, h: History { event: Event::Stage(stage, env), window: None, by: None } })
}

#[cfg(test)]
/// DEPLOY-13: `t` with the environment its `deployed` (DEPLOY-21: or its
/// stage) names set.
pub(crate) fn with_env(t: &Timed, env: &str) -> Timed {
    let mut t = t.clone();
    if let (Event::Deployed(e) | Event::Behind(e) | Event::Stage(_, e) | Event::Requested(e), Ok(env)) = (&mut t.h.event, menger_tickets_api::deployments::environment_of(env)) {
        *e = env;
    }
    t
}

/// DEPLOY-13: the environment `t` names, when it is a `deployed`
/// (DEPLOY-21: or a stage).
pub(crate) fn env_of(t: &Timed) -> Option<&str> {
    match &t.h.event {
        Event::Deployed(e) | Event::Behind(e) | Event::Stage(_, e) | Event::Requested(e) => Some(e),
        _ => None,
    }
}

#[cfg(test)]
/// FILTER-30: `t` turned around — `created today` ⇄ `not created today`,
/// hasn't changed ⇄ touched.
pub(crate) fn negate(t: &Timed) -> Timed {
    Timed { not: !t.not, h: t.h.clone() }
}

// ---------------------------------------------------------------------
// Words

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `2 weeks`, `1 day`, `12 hours`.
pub(crate) fn length_words(n: u32, u: Unit) -> String {
    format!("{n} {}", u.word(n != 1))
}

/// A time of day, the reader's way: `08:30`, or `8:30 am`.
pub(crate) fn clock_words(h: u8, m: u8, h12: bool) -> String {
    if !h12 {
        return format!("{h:02}:{m:02}");
    }
    let (hh, half) = match h {
        0 => (12, "am"),
        1..=11 => (h, "am"),
        12 => (12, "pm"),
        _ => (h - 12, "pm"),
    };
    format!("{hh}:{m:02} {half}")
}

/// A calendar moment: `Oct 1, 2026`, `Oct 1, 2026, 08:00`.
pub(crate) fn moment_words(m: &Moment, h12: bool) -> String {
    let date = format!("{} {}, {}", MONTHS[(m.month as usize).saturating_sub(1) % 12], m.day, m.year);
    match m.time {
        None => date,
        Some((h, mi)) => format!("{date}, {}", clock_words(h, mi, h12)),
    }
}

/// A moment: a calendar one, or `2 weeks ago`.
pub(crate) fn when_words(w: &When, h12: bool) -> String {
    match w {
        When::At(m) => moment_words(m, h12),
        When::Ago(n, u) => format!("{} ago", length_words(*n, *u)),
    }
}

/// A window in words: `today`, `in the last hour`, `in the last 3 days`,
/// `since Oct 1, 2026`, `before 2 weeks ago`, `between … and …`.
pub(crate) fn window_words(w: &Window, h12: bool) -> String {
    match w {
        Window::Within(1, u) => format!("in the last {}", u.word(false)),
        Window::Within(n, u) => format!("in the last {}", length_words(*n, *u)),
        Window::Today => "today".into(),
        Window::Yesterday => "yesterday".into(),
        Window::ThisWeek => "this week".into(),
        Window::Since(x) => format!("since {}", when_words(x, h12)),
        Window::Before(x) => format!("before {}", when_words(x, h12)),
        Window::Between(a, b) => format!("between {} and {}", when_words(a, h12), when_words(b, h12)),
    }
}

/// Who, in words.
pub(crate) fn who_words(w: &Who) -> String {
    match w {
        Who::Me => "by me".into(),
        Who::System => "by the system".into(),
        Who::Principal(p) => format!("by {p}"),
    }
}

/// What a `for` asks of, in words: `in any status`, `Pending`, `Pending
/// or Ready`, `ready, active or review`.
pub(crate) fn scope_words(of: &HeldOf) -> String {
    let list = |v: Vec<String>| match v.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    };
    match of {
        HeldOf::Any => "in any status".into(),
        HeldOf::Statuses(v) => list(v.iter().map(|s| s.as_str().to_string()).collect()),
        HeldOf::Categories(v) => list(v.iter().map(|c| c.as_str().to_string()).collect()),
    }
}

/// A `for`'s length in words: `for over 2 weeks`, `for under 1 hour`.
pub(crate) fn held_words(h: &Held) -> String {
    format!("for {} {}", if h.over { "over" } else { "under" }, length_words(h.n, h.unit))
}

/// The pill's segments, in order: each its words, and the part a click on
/// it reopens (none: the event's own words).
pub(crate) fn segments(t: &Timed, h12: bool) -> Vec<(Option<Part>, String)> {
    let mut out: Vec<(Option<Part>, String)> = Vec::new();
    // FILTER-30: a touch negated says it in words of its own.
    let untouched = t.not && t.h.event == Event::Touched;
    if t.not && !untouched {
        out.push((None, "not".into()));
    }
    match &t.h.event {
        Event::Created => out.push((None, "created".into())),
        Event::Touched if untouched => out.push((None, "hasn't changed".into())),
        Event::Touched => out.push((None, "changed".into())),
        Event::Entered(s) => {
            out.push((None, "moved into".into()));
            out.push((Some(Part::Status), s.as_str().into()));
        }
        Event::Left(s) => {
            out.push((None, "moved out of".into()));
            out.push((Some(Part::Status), s.as_str().into()));
        }
        Event::Changed => out.push((None, "moved".into())),
        Event::Deployed(env) => {
            out.push((None, "deployed to".into()));
            out.push((Some(Part::Env), env.clone()));
        }
        Event::Behind(env) => {
            out.push((None, "behind on".into()));
            out.push((Some(Part::Env), env.clone()));
        }
        // DEPLOY-21: "ready for prod", "deploying to test", "waiting for
        // prod", "failed on prod".
        Event::Stage(s, env) => {
            let words = match s {
                DeliveryStage::ReadyFor => "ready for",
                DeliveryStage::Deploying => "deploying to",
                DeliveryStage::Waiting => "waiting for",
                DeliveryStage::Failed => "failed on",
            };
            out.push((None, words.into()));
            out.push((Some(Part::Env), env.clone()));
        }
        // DEPLOY-25: "requested for prod".
        Event::Requested(env) => {
            out.push((None, "requested for".into()));
            out.push((Some(Part::Env), env.clone()));
        }
        Event::Archived => out.push((None, "archived".into())),
        Event::Restored => out.push((None, "restored".into())),
        // FILTER-52: "became status Complete", then its window and who.
        Event::Became(b) => out.push((None, format!("became {}", b.format()))),
        Event::Chatting => out.push((None, "chatting".into())),
        Event::Unread => out.push((None, "unread".into())),
        Event::Asked => out.push((None, "asked".into())),
        Event::InChat(n) => out.push((None, format!("in chat {n}"))),
        // TICKETS-9: "assigned to me", "assigned to <principal>", "assigned", "unassigned".
        Event::Assigned(None) => out.push((None, "assigned".into())),
        Event::Assigned(Some(menger_tickets_api::filter::Who::Me)) => out.push((None, "assigned to me".into())),
        Event::Assigned(Some(w)) => out.push((None, format!("assigned to {}", crate::chats::short(&w.format().replace('"', ""))))),
        Event::Unassigned => out.push((None, "unassigned".into())),
        Event::Queued(u) => {
            out.push((None, "queued".into()));
            if let Some(u) = u {
                out.push((None, u.as_str().into()));
            }
        }
        Event::Held(h) => {
            out.push((Some(Part::Status), scope_words(&h.of)));
            out.push((Some(Part::Length), held_words(h)));
        }
    }
    if Kind::of(&t.h.event).windowed() {
        let w = match &t.h.window {
            None => "ever".to_string(),
            // "hasn't changed in 2 weeks"
            Some(Window::Within(n, u)) if untouched => format!("in {}", length_words(*n, *u)),
            Some(w) => window_words(w, h12),
        };
        out.push((Some(Part::Window), w));
    }
    if Kind::of(&t.h.event).has_who() {
        out.push((Some(Part::Who), t.h.by.as_ref().map_or_else(|| "by anyone".to_string(), who_words)));
    }
    out
}

/// The clause in words, as the lens bar reads it: `created today`,
/// `Pending for over 2 weeks`, `moved into Ready in the last 2 days by
/// owner` — a part left at its default (ever, anyone) unsaid.
pub(crate) fn words(t: &Timed, h12: bool) -> String {
    let segs = segments(t, h12);
    segs.into_iter()
        .filter(|(p, w)| !(matches!(p, Some(Part::Window)) && w == "ever") && !(matches!(p, Some(Part::Who)) && w == "by anyone"))
        .map(|(_, w)| w)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::filter::Filter;

    fn timed(text: &str) -> Timed {
        let f = Filter::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"));
        assert_eq!(f.groups.len(), 1, "{text}");
        of_group(&f.groups[0]).unwrap_or_else(|| panic!("{text} is a time pill"))
    }

    /// FILTER-27 acceptance 3: every form of the language block comes back
    /// from text as a time pill, and the pill prints the canonical text —
    /// the round trip; and each reads in words.
    #[test]
    fn every_form_round_trips_and_reads_in_words() {
        for (text, said) in [
            ("created today", "created today"),
            ("created within 3d", "created in the last 3 days"),
            ("created this week", "created this week"),
            ("created before 2w ago", "created before 2 weeks ago"),
            ("created between 2026-10-01 and 2026-10-07", "created between Oct 1, 2026 and Oct 7, 2026"),
            ("entered Pending today", "moved into Pending today"),
            ("left Pending this week", "moved out of Pending this week"),
            ("entered Ready within 2d by owner", "moved into Ready in the last 2 days by owner"),
            ("status Pending for over 2w", "Pending for over 2 weeks"),
            ("status \"In Review\" for over 3d", "In Review for over 3 days"),
            ("category active for under 1h", "active for under 1 hour"),
            ("for over 2w", "in any status for over 2 weeks"),
            // FILTER-52: `changed <window>` is the last write; `moved` any transition.
            ("changed before 1w ago", "changed before 1 week ago"),
            ("moved within 1h", "moved in the last hour"),
            ("changed today by me", "moved today by me"),
            ("entered Canceled by system", "moved into Canceled by the system"),
            ("category in (ready, active, review) for over 1w", "ready, active or review for over 1 week"),
            ("not created today", "not created today"),
            ("moved between 2026-10-01T08:00 and 2026-10-02T17:30", "moved between Oct 1, 2026, 08:00 and Oct 2, 2026, 17:30"),
            ("status Complete after 2026-10-01", "became status Complete since Oct 1, 2026"),
        ] {
            let t = timed(text);
            let back = Filter { groups: vec![group(&t)], display: Default::default() };
            assert_eq!(back.format(), Filter::parse(text).unwrap().format(), "{text}");
            assert_eq!(words(&t, false), said, "{text}");
        }
        assert_eq!(words(&timed("moved since 2026-10-01T14:05"), true), "moved since Oct 1, 2026, 2:05 pm");
    }

    /// A pill's segments: each part reopens its own part of the menu.
    #[test]
    fn a_pill_is_its_parts() {
        let parts = |text: &str| segments(&timed(text), false).into_iter().map(|(p, w)| (p.map(Part::key), w)).collect::<Vec<_>>();
        assert_eq!(
            parts("entered Ready within 2d by owner"),
            [
                (None, "moved into".to_string()),
                (Some("status"), "Ready".to_string()),
                (Some("window"), "in the last 2 days".to_string()),
                (Some("who"), "by owner".to_string())
            ]
        );
        assert_eq!(parts("status Pending for over 2w"), [(Some("status"), "Pending".to_string()), (Some("length"), "for over 2 weeks".to_string())]);
        assert_eq!(parts("created today"), [(None, "created".to_string()), (Some("window"), "today".to_string())]);
        assert_eq!(parts("changed")[1], (Some("window"), "ever".to_string()));
    }

    /// FILTER-30: Hasn't changed builds `not touched <window>` from its
    /// presets (two weeks by default), reads "hasn't changed in 2 weeks",
    /// and its window reopens from the pill; Negate turns any time pill
    /// around and back.
    #[test]
    fn hasnt_changed_and_negate() {
        let text = |t: &Timed| Filter { groups: vec![group(t)], display: Default::default() }.format();
        let t = new_clause(Kind::Untouched, None, window_of("within 2w")).unwrap();
        assert_eq!(text(&t), "not changed within 2w");
        assert_eq!(words(&t, false), "hasn't changed in 2 weeks");
        assert_eq!(new_clause(Kind::Untouched, None, None), Some(t.clone()), "two weeks unless told");
        let parts: Vec<_> = segments(&t, false).into_iter().map(|(p, w)| (p.map(Part::key), w)).collect();
        assert_eq!(parts, [(None, "hasn't changed".to_string()), (Some("window"), "in 2 weeks".to_string())]);
        // Negate: every time pill turns around, and back.
        for s in [
            "created today",
            "entered Ready within 2d by owner",
            "status left Pending this week",
            "status Pending for over 2w",
            "changed before 1w ago",
            "moved today",
            "not changed within 2w",
        ] {
            let t = timed(s);
            let n = negate(&t);
            assert_eq!(text(&n), if let Some(rest) = s.strip_prefix("not ") { rest.to_string() } else { format!("not {s}") }, "{s}");
            assert_eq!(negate(&n), t, "{s} and back");
        }
        assert_eq!(words(&negate(&timed("created today")), false), "not created today");
        assert_eq!(words(&negate(&t), false), "changed in the last 2 weeks");
        assert_eq!(words(&timed("touched today"), false), "changed today");
        // The saved suggestion reads back as the same pill (FILTER-6).
        let f = Filter::parse("not touched within 2w and not category done and not kind doc").unwrap();
        assert_eq!(of_group(&f.groups[0]), Some(t));
    }

    /// DEPLOY-21: the stage rows build `<stage> <env>`, read in words, an
    /// environment their one part; no window, no who.
    #[test]
    fn stage_pills_build_and_read() {
        let text = |t: &Timed| Filter { groups: vec![group(t)], display: Default::default() }.format();
        for (text, said) in [
            ("ready for prod", "ready for prod"),
            ("deploying test", "deploying to test"),
            ("waiting menger/prod", "waiting for menger/prod"),
            ("failed prod", "failed on prod"),
        ] {
            let f = Filter::parse(text).unwrap();
            let t = of_group(&f.groups[0]).unwrap();
            assert_eq!(words(&t, false), said);
            assert!(!Kind::of(&t.h.event).windowed() && !Kind::of(&t.h.event).has_who());
        }
        let t = new_stage(DeliveryStage::ReadyFor, "Prod").unwrap();
        assert_eq!(text(&t), "ready for prod");
        assert_eq!(text(&with_env(&t, "staging")), "ready for staging");
        assert_eq!(env_of(&t), Some("prod"));
        assert!(new_stage(DeliveryStage::Failed, "no such env!").is_none());
    }

    /// DEPLOY-13: Deployed to and Not deployed to build `deployed <env>`
    /// and `not deployed <env>`, a window from their presets; each reads in
    /// words, its environment a part of its own; the pill turns around.
    #[test]
    fn deployed_pills_build_read_and_turn_around() {
        let text = |t: &Timed| Filter { groups: vec![group(t)], display: Default::default() }.format();
        for (text_in, said) in [
            ("deployed prod", "deployed to prod"),
            ("deployed menger/prod within 24h", "deployed to menger/prod in the last 24 hours"),
            ("not deployed prod", "not deployed to prod"),
            ("deployed test today", "deployed to test today"),
        ] {
            let t = timed(text_in);
            assert_eq!(words(&t, false), said, "{text_in}");
            assert_eq!(text(&t), Filter::parse(text_in).unwrap().format(), "{text_in}");
        }
        let t = new_deployed("Prod", false, None).unwrap();
        assert_eq!(text(&t), "deployed prod");
        assert_eq!(text(&new_deployed("prod", true, window_of("within 24h")).unwrap()), "not deployed prod within 24h");
        assert!(new_deployed("no such env!", false, None).is_none());
        assert_eq!(text(&with_env(&t, "staging")), "deployed staging");
        assert_eq!(env_of(&with_env(&t, "bad env!")), Some("prod"), "a bad name changes nothing");
        assert_eq!(text(&negate(&t)), "not deployed prod");
        let parts: Vec<_> = segments(&t, false).into_iter().map(|(p, w)| (p.map(Part::key), w)).collect();
        assert_eq!(parts, [(None, "deployed to".to_string()), (Some("env"), "prod".to_string()), (Some("window"), "ever".to_string())]);
    }

    /// A group that is not one time predicate is no time pill.
    #[test]
    fn other_groups_are_not_time_pills() {
        for text in ["status Ready", "(changed today or blocked)", "(entered Ready within 1h or left Coding within 1d)"] {
            let f = Filter::parse(text).unwrap();
            assert!(of_group(&f.groups[0]).is_none(), "{text}");
        }
    }
}
