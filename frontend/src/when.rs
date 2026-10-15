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
//! - **The menu** (src/when_view.rs): Created, Moved into ▸ status, Moved
//!   out of ▸ status, In status for, Changed, Hasn't changed — each with its
//!   presets and a Custom… window editor (src/calendar.rs's calendar and
//!   time selector). Every edit is one of the pure edits here
//!   ([`with_window`], …).
//! - **FILTER-30**: Hasn't changed is `not touched <window>` — the ticket's
//!   last touch (any write to it, a comment, an answer) outside the window:
//!   "hasn't changed in 2 weeks". And any time pill turns around with
//!   [`negate`] (its ⋯ menu's Negate; a group's, for its time members).
//! - **DEPLOY-13**: `deployed <env> [window]` is a time pill too — its
//!   environment a part of its own ([`Part::Env`]) — built from the filter
//!   menu's own Deployed category: Deployed to ▸ and Not deployed to ▸ an
//!   environment ([`new_deployed`], [`with_env`]), windows of what reached it
//!   lately ([`DEPLOYED_PRESETS`]).
//!
//! Everything that decides is a pure function here, pinned by native tests.

use menger_tickets_api::filter::{Category, Event, Field, Filter, Group, Held, HeldOf, History, Moment, Predicate, Unit, When, Who, Window};
use menger_tickets_api::Status;

/// A time pill's group: whether it is negated, and its predicate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Timed {
    pub(crate) not: bool,
    pub(crate) h: History,
}

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
    /// DEPLOY-13: `not deployed <env> [window]` — built negated, as Hasn't
    /// changed is; a pill of it is a Deployed one, turned around.
    NotDeployed,
    /// MENGER-39: `queued [urgency]` — the owner's queue, kept by the
    /// store; a pill (from the palette or a saved filter), never a row of
    /// When's menu.
    Queued,
}

impl Kind {
    /// The menu's order.
    pub(crate) const ALL: [Kind; 6] = [Kind::Created, Kind::Into, Kind::OutOf, Kind::Held, Kind::Changed, Kind::Untouched];

    /// DEPLOY-13: the Deployed category's rows.
    pub(crate) const DEPLOYED: [Kind; 2] = [Kind::Deployed, Kind::NotDeployed];

    pub(crate) fn of(e: &Event) -> Kind {
        match e {
            Event::Created => Kind::Created,
            Event::Entered(_) => Kind::Into,
            Event::Left(_) => Kind::OutOf,
            Event::Held(_) => Kind::Held,
            Event::Changed => Kind::Changed,
            Event::Touched => Kind::Untouched,
            Event::Deployed(_) => Kind::Deployed,
            Event::Queued(_) => Kind::Queued,
        }
    }

    /// Its row in the menu.
    pub(crate) fn label(self) -> &'static str {
        match self {
            Kind::Created => "Created",
            Kind::Into => "Moved into",
            Kind::OutOf => "Moved out of",
            Kind::Held => "In status for",
            Kind::Changed => "Changed",
            Kind::Untouched => "Hasn't changed",
            Kind::Deployed => "Deployed to",
            Kind::NotDeployed => "Not deployed to",
            Kind::Queued => "Queued",
        }
    }

    /// The DOM's `data-when`.
    pub(crate) fn key(self) -> &'static str {
        match self {
            Kind::Created => "created",
            Kind::Into => "into",
            Kind::OutOf => "outof",
            Kind::Held => "held",
            Kind::Changed => "changed",
            Kind::Untouched => "untouched",
            Kind::Deployed => "deployed",
            Kind::NotDeployed => "notdeployed",
            Kind::Queued => "queued",
        }
    }

    /// Whether its clause names a status first (Moved into, Moved out of).
    pub(crate) fn needs_status(self) -> bool {
        matches!(self, Kind::Into | Kind::OutOf)
    }

    /// DEPLOY-13: whether its clause names an environment first (Deployed
    /// to, Not deployed to).
    pub(crate) fn needs_env(self) -> bool {
        matches!(self, Kind::Deployed | Kind::NotDeployed)
    }

    /// Whether its clause has a window (all but In status for).
    pub(crate) fn windowed(self) -> bool {
        !matches!(self, Kind::Held | Kind::Queued)
    }

    /// Whether its clause may say who (the transitions'; not a birth).
    pub(crate) fn has_who(self) -> bool {
        matches!(self, Kind::Into | Kind::OutOf | Kind::Changed)
    }

    /// Whether its window may be "ever" (a birth and a touch always have
    /// one).
    pub(crate) fn has_ever(self) -> bool {
        !matches!(self, Kind::Created | Kind::Untouched)
    }

    /// The window presets its menu offers (FILTER-30: Hasn't changed, the
    /// lengths a stale ticket is measured in).
    pub(crate) fn presets(self) -> &'static [&'static str] {
        match self {
            Kind::Untouched => &UNTOUCHED_PRESETS,
            Kind::Deployed | Kind::NotDeployed => &DEPLOYED_PRESETS,
            _ => &WINDOW_PRESETS,
        }
    }

    /// A preset's chip, in words: `in the last 3 days`; Hasn't changed's,
    /// the length alone (`3 days`).
    pub(crate) fn preset_words(self, w: &Window, h12: bool) -> String {
        match (self, w) {
            (Kind::Untouched, Window::Within(n, u)) => length_words(*n, *u),
            _ => window_words(w, h12),
        }
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

/// The window presets each windowed kind offers, as the language writes
/// them — Changed recently's five (MENGER-13) for every kind.
pub(crate) const WINDOW_PRESETS: [&str; 5] = ["today", "yesterday", "this week", "within 1h", "within 12h"];

/// FILTER-30: the windows Hasn't changed offers: 1d · 3d · 1w · 2w · 1mo.
pub(crate) const UNTOUCHED_PRESETS: [&str; 5] = ["within 1d", "within 3d", "within 1w", "within 2w", "within 1mo"];

/// DEPLOY-13: the windows Deployed to offers — what reached an environment
/// lately.
pub(crate) const DEPLOYED_PRESETS: [&str; 5] = ["within 1h", "within 24h", "today", "this week", "within 2w"];

/// The lengths In status for offers: over 1d · 3d · 1w · 2w · 1mo.
pub(crate) const LENGTH_PRESETS: [(u32, Unit); 5] = [(1, Unit::Day), (3, Unit::Day), (1, Unit::Week), (2, Unit::Week), (1, Unit::Month)];

/// The statuses Moved into / out of offer, the menu's order.
pub(crate) const STATUSES: [Status; 11] = [
    Status::Pending,
    Status::Ready,
    Status::Coding,
    Status::InReview,
    Status::Complete,
    Status::Blocked,
    Status::Idea,
    Status::Epic,
    Status::Superseded,
    Status::Canceled,
    Status::Doc,
];

/// The categories In status for offers (a doc never moves: no `static`).
pub(crate) const CATEGORIES: [Category; 6] =
    [Category::Open, Category::Ready, Category::Active, Category::Review, Category::Done, Category::Dropped];

/// A window as the language reads it (`today`, `within 3d`, `before 2w
/// ago`) — `None` when it is none.
pub(crate) fn window_of(text: &str) -> Option<Window> {
    let f = Filter::parse(&format!("changed {text}")).ok()?;
    match &f.groups.first()?.preds.first()?.field {
        Field::History(h) if f.groups.len() == 1 => h.window.clone(),
        _ => None,
    }
}

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
        // DEPLOY-13: names an environment first ([`new_deployed`]).
        Kind::Deployed | Kind::NotDeployed => return None,
        // MENGER-39: never built by When's menu.
        Kind::Queued => return None,
    };
    let window = match kind {
        Kind::Created | Kind::Untouched => Some(window.unwrap_or_else(|| default_window(&event))),
        Kind::Held => None,
        _ => window,
    };
    Some(Timed { not: kind == Kind::Untouched, h: History { event, window, by: None } })
}

/// The window a clause that must have one falls back to: a birth's today, a
/// touch's two weeks.
fn default_window(e: &Event) -> Window {
    match e {
        Event::Touched => Window::Within(2, Unit::Week),
        _ => Window::Today,
    }
}

/// DEPLOY-13: `deployed <env> [window]` — `not` for Not deployed to —
/// or `None` when `env` is no environment name (`prod`, `menger/prod`).
pub(crate) fn new_deployed(env: &str, not: bool, window: Option<Window>) -> Option<Timed> {
    let env = menger_tickets_api::deployments::environment_of(env).ok()?;
    Some(Timed { not, h: History { event: Event::Deployed(env), window, by: None } })
}

/// DEPLOY-13: `t` with the environment its `deployed` names set.
pub(crate) fn with_env(t: &Timed, env: &str) -> Timed {
    let mut t = t.clone();
    if let (Event::Deployed(e), Ok(env)) = (&mut t.h.event, menger_tickets_api::deployments::environment_of(env)) {
        *e = env;
    }
    t
}

/// DEPLOY-13: the environment `t` names, when it is a `deployed`.
pub(crate) fn env_of(t: &Timed) -> Option<&str> {
    match &t.h.event {
        Event::Deployed(e) => Some(e),
        _ => None,
    }
}

/// FILTER-30: `t` turned around — `created today` ⇄ `not created today`,
/// hasn't changed ⇄ touched.
pub(crate) fn negate(t: &Timed) -> Timed {
    Timed { not: !t.not, h: t.h.clone() }
}

/// `t` with the status its event names set (Moved into / out of).
pub(crate) fn with_status(t: &Timed, s: Status) -> Timed {
    let mut t = t.clone();
    t.h.event = match t.h.event {
        Event::Entered(_) => Event::Entered(s),
        Event::Left(_) => Event::Left(s),
        e => e,
    };
    t
}

/// `t` with its window set — `None`: ever (a birth keeps one: today).
pub(crate) fn with_window(t: &Timed, w: Option<Window>) -> Timed {
    let mut t = t.clone();
    match t.h.event {
        Event::Held(_) => {}
        Event::Created | Event::Touched => t.h.window = Some(w.unwrap_or_else(|| default_window(&t.h.event))),
        _ => t.h.window = w,
    }
    t
}

/// `t` with who set — `None`: anyone. A birth and a `for` say no one.
pub(crate) fn with_by(t: &Timed, who: Option<Who>) -> Timed {
    let mut t = t.clone();
    if Kind::of(&t.h.event).has_who() {
        t.h.by = who;
    }
    t
}

/// `t`'s `for` with its length set.
pub(crate) fn with_length(t: &Timed, over: bool, n: u32, unit: Unit) -> Timed {
    let mut t = t.clone();
    if let Event::Held(h) = &mut t.h.event {
        *h = Held { over, n: n.max(1), unit, ..h.clone() };
    }
    t
}

/// `t`'s `for` asking of `of`.
pub(crate) fn with_scope(t: &Timed, of: HeldOf) -> Timed {
    let mut t = t.clone();
    if let Event::Held(h) = &mut t.h.event {
        h.of = match of {
            HeldOf::Statuses(v) if v.is_empty() => HeldOf::Any,
            HeldOf::Categories(v) if v.is_empty() => HeldOf::Any,
            of => of,
        };
    }
    t
}

/// `of` with status `s` added or taken away (statuses replace categories).
pub(crate) fn toggle_status(of: &HeldOf, s: Status) -> HeldOf {
    let mut v = match of {
        HeldOf::Statuses(v) => v.clone(),
        _ => Vec::new(),
    };
    match v.iter().position(|x| *x == s) {
        Some(i) => {
            v.remove(i);
        }
        None => v.push(s),
    }
    v.sort_by_key(|x| STATUSES.iter().position(|y| y == x));
    HeldOf::Statuses(v)
}

/// `of` with category `c` added or taken away (categories replace statuses).
pub(crate) fn toggle_category(of: &HeldOf, c: Category) -> HeldOf {
    let mut v = match of {
        HeldOf::Categories(v) => v.clone(),
        _ => Vec::new(),
    };
    match v.iter().position(|x| *x == c) {
        Some(i) => {
            v.remove(i);
        }
        None => v.push(c),
    }
    v.sort();
    HeldOf::Categories(v)
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
        Event::Touched => out.push((None, "touched".into())),
        Event::Entered(s) => {
            out.push((None, "moved into".into()));
            out.push((Some(Part::Status), s.as_str().into()));
        }
        Event::Left(s) => {
            out.push((None, "moved out of".into()));
            out.push((Some(Part::Status), s.as_str().into()));
        }
        Event::Changed => out.push((None, "changed".into())),
        Event::Deployed(env) => {
            out.push((None, "deployed to".into()));
            out.push((Some(Part::Env), env.clone()));
        }
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

/// The window a pill's preset row stands for is `t`'s — what the menu
/// marks checked.
pub(crate) fn has_window(t: &Timed, preset: &str) -> bool {
    t.h.window.as_ref().map(Window::format).as_deref() == Some(preset)
}

/// Whether `t`'s window is none of its kind's presets (a custom one).
pub(crate) fn custom_window(t: &Timed) -> bool {
    let presets = Kind::of(&t.h.event).presets();
    t.h.window.as_ref().is_some_and(|w| !presets.contains(&w.format().as_str()))
}

/// The `for` of `t`, when it is one.
pub(crate) fn held(t: &Timed) -> Option<&Held> {
    match &t.h.event {
        Event::Held(h) => Some(h),
        _ => None,
    }
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
            ("changed before 1w ago", "changed before 1 week ago"),
            ("changed within 1h", "changed in the last hour"),
            ("changed today by me", "changed today by me"),
            ("entered Canceled by system", "moved into Canceled by the system"),
            ("category in (ready, active, review) for over 1w", "ready, active or review for over 1 week"),
            ("not created today", "not created today"),
            ("changed between 2026-10-01T08:00 and 2026-10-02T17:30", "changed between Oct 1, 2026, 08:00 and Oct 2, 2026, 17:30"),
        ] {
            let t = timed(text);
            let back = Filter { groups: vec![group(&t)], display: Default::default() };
            assert_eq!(back.format(), Filter::parse(text).unwrap().format(), "{text}");
            assert_eq!(words(&t, false), said, "{text}");
        }
        assert_eq!(words(&timed("changed since 2026-10-01T14:05"), true), "changed since Oct 1, 2026, 2:05 pm");
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

    /// The menu's edits, each the clause the language would read.
    #[test]
    fn the_menus_edits_build_every_form() {
        let text = |t: &Timed| Filter { groups: vec![group(t)], display: Default::default() }.format();
        let into = new_clause(Kind::Into, Some(Status::Ready), None).unwrap();
        assert_eq!(text(&into), "entered Ready");
        let into = with_window(&into, window_of("within 2d"));
        let into = with_by(&into, Some(Who::Principal("owner".into())));
        assert_eq!(text(&into), "entered Ready within 2d by owner");
        assert_eq!(text(&with_status(&into, Status::Pending)), "entered Pending within 2d by owner");
        assert_eq!(text(&with_by(&into, None)), "entered Ready within 2d");
        let created = new_clause(Kind::Created, None, None).unwrap();
        assert_eq!(text(&created), "created today");
        assert_eq!(text(&with_window(&created, None)), "created today", "a birth keeps a window");
        assert_eq!(text(&with_by(&created, Some(Who::Me))), "created today", "a birth names no one");
        let between = Window::Between(
            When::At(Moment { year: 2026, month: 10, day: 1, time: Some((8, 0)) }),
            When::At(Moment { year: 2026, month: 10, day: 7, time: Some((17, 30)) }),
        );
        assert_eq!(text(&with_window(&created, Some(between))), "created between 2026-10-01T08:00 and 2026-10-07T17:30");
        assert_eq!(text(&with_window(&created, Some(Window::Before(When::Ago(2, Unit::Week))))), "created before 2w ago");
        let held = new_clause(Kind::Held, None, None).unwrap();
        assert_eq!(text(&held), "for over 1w");
        let held = with_length(&held, true, 2, Unit::Week);
        let held = with_scope(&held, toggle_status(&HeldOf::Any, Status::Pending));
        assert_eq!(text(&held), "status Pending for over 2w");
        let held = with_scope(&held, toggle_status(&HeldOf::Statuses(vec![Status::Pending]), Status::Pending));
        assert_eq!(text(&held), "for over 2w", "no status left: any");
        let mut of = HeldOf::Any;
        for c in [Category::Review, Category::Ready, Category::Active] {
            of = toggle_category(&of, c);
        }
        assert_eq!(text(&with_scope(&with_length(&held, true, 1, Unit::Week), of)), "category in (ready, active, review) for over 1w");
        assert_eq!(text(&with_length(&held, false, 1, Unit::Hour)), "for under 1h");
        let changed = new_clause(Kind::Changed, None, window_of("before 1w ago")).unwrap();
        assert_eq!(text(&changed), "changed before 1w ago");
        assert!(new_clause(Kind::Into, None, None).is_none(), "moved into names a status");
        assert!(has_window(&timed("changed today"), "today") && !custom_window(&timed("changed today")));
        assert!(custom_window(&timed("changed since 3d ago")));
    }

    /// FILTER-30: Hasn't changed builds `not touched <window>` from its
    /// presets (two weeks by default), reads "hasn't changed in 2 weeks",
    /// and its window reopens from the pill; Negate turns any time pill
    /// around and back.
    #[test]
    fn hasnt_changed_and_negate() {
        let text = |t: &Timed| Filter { groups: vec![group(t)], display: Default::default() }.format();
        assert!(Kind::ALL.contains(&Kind::Untouched) && Kind::Untouched.label() == "Hasn't changed");
        let t = new_clause(Kind::Untouched, None, window_of("within 2w")).unwrap();
        assert_eq!(text(&t), "not touched within 2w");
        assert_eq!(words(&t, false), "hasn't changed in 2 weeks");
        assert_eq!(new_clause(Kind::Untouched, None, None), Some(t.clone()), "two weeks unless told");
        let parts: Vec<_> = segments(&t, false).into_iter().map(|(p, w)| (p.map(Part::key), w)).collect();
        assert_eq!(parts, [(None, "hasn't changed".to_string()), (Some("window"), "in 2 weeks".to_string())]);
        for p in UNTOUCHED_PRESETS {
            let t = new_clause(Kind::Untouched, None, window_of(p)).unwrap();
            assert_eq!(text(&t), format!("not touched {p}"));
            assert!(has_window(&t, p) && !custom_window(&t), "{p}");
        }
        assert_eq!(Kind::Untouched.preset_words(&window_of("within 3d").unwrap(), false), "3 days");
        let since = with_window(&t, window_of("since 2026-10-01"));
        assert_eq!(text(&since), "not touched since 2026-10-01");
        assert_eq!(words(&since, false), "hasn't changed since Oct 1, 2026");
        assert!(custom_window(&since));
        assert_eq!(text(&with_window(&t, None)), "not touched within 2w", "a touch keeps a window");
        assert_eq!(text(&with_by(&t, Some(Who::Me))), "not touched within 2w", "a touch names no one");
        assert!(!Kind::Untouched.has_ever() && !Kind::Untouched.has_who() && Kind::Untouched.windowed());
        // Negate: every time pill turns around, and back.
        for s in [
            "created today",
            "entered Ready within 2d by owner",
            "left Pending this week",
            "status Pending for over 2w",
            "changed before 1w ago",
            "touched today",
            "not touched within 2w",
        ] {
            let t = timed(s);
            let n = negate(&t);
            assert_eq!(text(&n), if let Some(rest) = s.strip_prefix("not ") { rest.to_string() } else { format!("not {s}") }, "{s}");
            assert_eq!(negate(&n), t, "{s} and back");
        }
        assert_eq!(words(&negate(&timed("created today")), false), "not created today");
        assert_eq!(words(&negate(&t), false), "touched in the last 2 weeks");
        assert_eq!(words(&timed("touched today"), false), "touched today");
        // The saved suggestion reads back as the same pill (FILTER-6).
        let f = Filter::parse("not touched within 2w and not category done and not kind doc").unwrap();
        assert_eq!(of_group(&f.groups[0]), Some(t));
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
        assert_eq!(text(&with_by(&t, Some(Who::Me))), "deployed prod", "a deployment names no one");
        let parts: Vec<_> = segments(&t, false).into_iter().map(|(p, w)| (p.map(Part::key), w)).collect();
        assert_eq!(parts, [(None, "deployed to".to_string()), (Some("env"), "prod".to_string()), (Some("window"), "ever".to_string())]);
        assert!(Kind::Deployed.needs_env() && Kind::NotDeployed.needs_env() && !Kind::Deployed.has_who() && Kind::Deployed.has_ever());
        for p in DEPLOYED_PRESETS {
            let t = new_deployed("prod", false, window_of(p)).unwrap();
            assert!(has_window(&t, p) && !custom_window(&t), "{p}");
        }
        assert!(!Kind::ALL.contains(&Kind::Deployed), "When's rows are When's");
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
