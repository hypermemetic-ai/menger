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
//!   out of ▸ status, In status for, Changed — each with its presets and a
//!   Custom… window editor (src/calendar.rs's calendar and time selector).
//!   Every edit is one of the pure edits here ([`with_window`], …).
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

/// What the When menu builds — one of its five rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Created,
    Into,
    OutOf,
    Held,
    Changed,
}

impl Kind {
    /// The menu's order.
    pub(crate) const ALL: [Kind; 5] = [Kind::Created, Kind::Into, Kind::OutOf, Kind::Held, Kind::Changed];

    pub(crate) fn of(e: &Event) -> Kind {
        match e {
            Event::Created => Kind::Created,
            Event::Entered(_) => Kind::Into,
            Event::Left(_) => Kind::OutOf,
            Event::Held(_) => Kind::Held,
            Event::Changed => Kind::Changed,
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
        }
    }

    /// Whether its clause names a status first (Moved into, Moved out of).
    pub(crate) fn needs_status(self) -> bool {
        matches!(self, Kind::Into | Kind::OutOf)
    }

    /// Whether its clause has a window (all but In status for).
    pub(crate) fn windowed(self) -> bool {
        self != Kind::Held
    }

    /// Whether its clause may say who (the transitions'; not a birth).
    pub(crate) fn has_who(self) -> bool {
        matches!(self, Kind::Into | Kind::OutOf | Kind::Changed)
    }
}

/// A part of a clause its pill's segment reopens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Part {
    Status,
    Window,
    Length,
    Who,
}

impl Part {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Part::Status => "status",
            Part::Window => "window",
            Part::Length => "length",
            Part::Who => "who",
        }
    }
}

/// The window presets each windowed kind offers, as the language writes
/// them — Changed recently's five (MENGER-13) for every kind.
pub(crate) const WINDOW_PRESETS: [&str; 5] = ["today", "yesterday", "this week", "within 1h", "within 12h"];

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
/// window (`None`: ever) for the rest; In status for, over a length.
pub(crate) fn new_clause(kind: Kind, status: Option<Status>, window: Option<Window>) -> Option<Timed> {
    let event = match kind {
        Kind::Created => Event::Created,
        Kind::Into => Event::Entered(status?),
        Kind::OutOf => Event::Left(status?),
        Kind::Changed => Event::Changed,
        Kind::Held => Event::Held(Held { of: HeldOf::Any, over: true, n: 1, unit: Unit::Week }),
    };
    let window = if kind == Kind::Created { Some(window.unwrap_or(Window::Today)) } else if kind == Kind::Held { None } else { window };
    Some(Timed { not: false, h: History { event, window, by: None } })
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
        Event::Created => t.h.window = Some(w.unwrap_or(Window::Today)),
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
    if t.not {
        out.push((None, "not".into()));
    }
    match &t.h.event {
        Event::Created => out.push((None, "created".into())),
        Event::Entered(s) => {
            out.push((None, "moved into".into()));
            out.push((Some(Part::Status), s.as_str().into()));
        }
        Event::Left(s) => {
            out.push((None, "moved out of".into()));
            out.push((Some(Part::Status), s.as_str().into()));
        }
        Event::Changed => out.push((None, "changed".into())),
        Event::Held(h) => {
            out.push((Some(Part::Status), scope_words(&h.of)));
            out.push((Some(Part::Length), held_words(h)));
        }
    }
    if Kind::of(&t.h.event).windowed() {
        let w = t.h.window.as_ref().map_or_else(|| "ever".to_string(), |w| window_words(w, h12));
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

/// Whether `t`'s window is none of the presets (a custom one).
pub(crate) fn custom_window(t: &Timed) -> bool {
    t.h.window.as_ref().is_some_and(|w| !WINDOW_PRESETS.contains(&w.format().as_str()))
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

    /// A group that is not one time predicate is no time pill.
    #[test]
    fn other_groups_are_not_time_pills() {
        for text in ["status Ready", "(changed today or blocked)", "(entered Ready within 1h or left Coding within 1d)"] {
            let f = Filter::parse(text).unwrap();
            assert!(of_group(&f.groups[0]).is_none(), "{text}");
        }
    }
}
