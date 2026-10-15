//! AGS-28: **every key and gesture, declared once.** Owner, 2026-10-13:
//! "create for me a keybinding abstraction which we can then use to show on
//! the settings page".
//!
//! [`BINDINGS`] is the app's whole key map: each [`Binding`] an id (what a
//! handler names), the [`Context`]s where it applies, its chords (spelled as
//! [`Chord::parse`] reads them — `j`, `Shift+*`, `g h`, `Mod+k`, `Alt+Click`),
//! the [`Group`] it is listed under and what it does, in the reader's words.
//! Three things follow from the one table:
//!
//! - **Handlers dispatch by id.** A handler asks [`hit`] (a key),
//!   [`hit_mouse`] (a click, a right-click) or [`is`] (one id, no sequence
//!   state) what the event is *in its context*, and matches the answer —
//!   `match bindings::hit(&ev, Context::Nav) { Some("nav.expand-subtree") =>
//!   …, _ => {} }`. Nothing outside this module reads an event's key, code,
//!   button or modifiers: a source lint (the tests below, as THEME-2's
//!   colour lint) refuses it, file by file, except where a file is listed
//!   with its reason. Which ids a handler names is read from the sources
//!   too: a binding nobody dispatches fails a test, and so does a handler
//!   naming an id the table does not hold.
//! - **The documentation is the table.** The `?` cheat sheet and Settings →
//!   Keyboard (src/keys.rs's `KeyTable`) draw [`BINDINGS`] grouped, each row
//!   its chords as this platform spells them ([`Chord::show`]: on a Mac `Mod`
//!   is ⌘ and Alt ⌥), what it does and where ([`Context::words`]); Settings
//!   narrows it by a key or by words ([`search`]). `SHORTCUTS`, the list of
//!   display strings the sheet drew before, is gone.
//! - **Conflicts are a test.** Two bindings that share a chord in contexts
//!   that can be live at once ([`Context::live_with`]) fail a native test
//!   naming both; so does a key that is also the first key of a sequence.
//!
//! **Matching** ([`Step::matches`]). A letter is matched without its case,
//! Shift counted: `j` is j, `Shift+j` is J (and caps lock's J is j). A
//! symbol (`?`, `*`, `/`, `-`) ignores Shift, which the layout may need to
//! type it; `Numpad*` is the numpad's key alone. Named keys (`Escape`,
//! `ArrowDown`, `Space`, …) count Shift. `Mod` is ⌘ on a Mac and Ctrl or ⌘
//! elsewhere (MENGER-6's chord: Ctrl+K on a Mac is a field's kill-line); any
//! other Ctrl, ⌘ or Alt must be written to match. Where two chords of one
//! context both match, the one naming more wins (`Shift+Numpad*` over `*`).
//!
//! **Sequences** (`g h`) are one chord of two steps, and the time between
//! them is [`SEQUENCE_MS`], here and nowhere else. The context that owns a
//! sequence arms it on its first key ([`PREFIX`] answers that press, so the
//! handler swallows it); a second key that completes a sequence another
//! context owns is left to that context — so `h` in the nav is not
//! "collapse" when it ends `g h`, without the nav knowing `g h` exists. The
//! same keydown seen by several listeners arms once and disarms nothing.
//!
//! **Gestures are bindings.** A click, Shift+click, Ctrl/⌘+click, Alt+click
//! (FILTER-25's chip filter), a middle-click, a right-click and a
//! double-click are steps like keys ([`Button`]), listed with them — a
//! `contextmenu` event is a right-click whatever its button says, a
//! `dblclick` a double-click. AGS-29: the nav's row waits
//! [`DOUBLE_CLICK_MS`] on a plain click before it opens, so that a
//! double-click can open or close it without opening its ticket.
//!
//! **Later, rebinding** (not this ticket): matching reads the chords from
//! [`table`], which is [`BINDINGS`] with a layer of overrides applied
//! ([`table_with`]: an id and the chords that replace its own). A reader's
//! overrides kept in the settings record would be that layer; no handler
//! changes, since handlers never see a chord.

use std::cell::{OnceCell, RefCell};

/// How long the first key of a sequence (`g` of `g h`) stays armed.
pub(crate) const SEQUENCE_MS: f64 = 1500.0;

/// AGS-29: how long a plain click on a nav row with children waits for a
/// second before it opens the ticket — a double-click (`nav.toggle`) opens
/// or closes the row instead. Under the system's double-click time (~500
/// ms), so a held click feels quick; src/rowclick.rs holds the rule.
pub(crate) const DOUBLE_CLICK_MS: f64 = 250.0;

/// What [`hit`] answers for the first key of a sequence the context owns:
/// taken (the handler swallows it), nothing done yet.
pub(crate) const PREFIX: &str = "…prefix";

// ---------------------------------------------------------------------
// The vocabulary

/// Where a binding applies. Which contexts are live at once is
/// [`Context::live_with`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Context {
    /// Even in a text field or a dialog: taken before anything sees it.
    Anywhere,
    /// Anywhere on the page but a text field, a dialog or a menu.
    Global,
    /// Focus in the nav's tree.
    Nav,
    /// The board, a row of it focused (or the board on screen).
    Board,
    /// A ticket's page, focus anywhere but the nav.
    TicketPage,
    /// On any ticket: a row, a card, a ticket's page.
    Ticket,
    /// On any chip (a code, a status, a kind, a severity, a label).
    Chip,
    /// On a link in a body, a comment or a note.
    Link,
    /// Inside a ticket's framed body (its SDK posts the keys out).
    Frame,
    /// Annotation mode is on: a click is a comment's anchor.
    Annotate,
    /// The jump palette is open.
    Palette,
    /// A dialog is open (the cheat sheet, a comment box, the leave dialog…).
    Dialog,
    /// A menu is open (a ticket's, a chip's, the filter's, the saved views').
    Menu,
    /// A date picker's grid.
    DatePicker,
    /// A time field's hours or minutes.
    TimeField,
    /// A text field with a button beside it.
    Field,
}

impl Context {
    #[cfg(test)]
    pub(crate) const ALL: [Context; 16] = [
        Context::Anywhere,
        Context::Global,
        Context::Nav,
        Context::Board,
        Context::TicketPage,
        Context::Ticket,
        Context::Chip,
        Context::Link,
        Context::Frame,
        Context::Annotate,
        Context::Palette,
        Context::Dialog,
        Context::Menu,
        Context::DatePicker,
        Context::TimeField,
        Context::Field,
    ];

    /// Where, in the reader's words — the tables' last column.
    pub(crate) fn words(self) -> &'static str {
        match self {
            Context::Anywhere => "anywhere, typing too",
            Context::Global => "everywhere",
            Context::Nav => "in the nav",
            Context::Board => "on the board",
            Context::TicketPage => "on a ticket page",
            Context::Ticket => "on any ticket",
            Context::Chip => "on any chip",
            Context::Link => "on a link in a body",
            Context::Frame => "inside a framed body",
            Context::Annotate => "in annotation mode",
            Context::Palette => "in the palette",
            Context::Dialog => "in a dialog",
            Context::Menu => "in a menu",
            Context::DatePicker => "in a date picker",
            Context::TimeField => "in a time field",
            Context::Field => "in a text field",
        }
    }

    /// A context that owns every key while it is up: the global map stands
    /// down inside it (src/keys.rs's `defer_to_focus`), so it shares no key
    /// with the page's.
    fn modal(self) -> bool {
        matches!(
            self,
            Context::Annotate | Context::Palette | Context::Dialog | Context::Menu | Context::DatePicker | Context::TimeField | Context::Field
        )
    }

    /// Whether a key could reach both at once. Itself, always; `Anywhere`,
    /// with everything; a modal context, with nothing else. Of the page's:
    /// the nav, the board, a ticket's page and a filter bar are where focus
    /// (or the pointer) is — one at a time — and a body's links are on no
    /// nav or board row, nor a filter's pill. Everything else on the page is live together.
    pub(crate) fn live_with(self, other: Context) -> bool {
        if self == other || self == Context::Anywhere || other == Context::Anywhere {
            return true;
        }
        if self.modal() || other.modal() {
            return false;
        }
        let region = |c: Context| matches!(c, Context::Nav | Context::Board | Context::TicketPage);
        if region(self) && region(other) {
            return false;
        }
        let rows = |c: Context| matches!(c, Context::Nav | Context::Board);
        !((self == Context::Link && rows(other)) || (other == Context::Link && rows(self)))
    }
}

/// What a binding is listed under, in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Group {
    Move,
    Open,
    /// ACT-10: a ticket's status, by key.
    Status,
    Select,
    Nav,
    Filter,
    Annotate,
    View,
    Dialogs,
    Forms,
    Help,
}

impl Group {
    pub(crate) const ALL: [Group; 11] = [
        Group::Move,
        Group::Open,
        Group::Status,
        Group::Select,
        Group::Nav,
        Group::Filter,
        Group::Annotate,
        Group::View,
        Group::Dialogs,
        Group::Forms,
        Group::Help,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Group::Move => "Move",
            Group::Open => "Open",
            Group::Status => "Status",
            Group::Select => "Select",
            Group::Nav => "Nav",
            Group::Filter => "Filter",
            Group::Annotate => "Annotate",
            Group::View => "View",
            Group::Dialogs => "Menus & dialogs",
            Group::Forms => "Forms",
            Group::Help => "Help",
        }
    }
}

/// One thing the app answers: a key, a sequence or a gesture, by id.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Binding {
    /// What a handler names — `nav.expand-subtree`.
    pub(crate) id: &'static str,
    /// Where it applies, the first the one the tables name first.
    pub(crate) contexts: &'static [Context],
    /// Its chords, as [`Chord::parse`] reads them.
    pub(crate) chords: &'static [&'static str],
    pub(crate) group: Group,
    /// What it does, in the reader's words.
    pub(crate) does: &'static str,
    /// The browser does it, not a handler: the row only says so (Tab).
    pub(crate) native: bool,
}

const fn b(id: &'static str, contexts: &'static [Context], chords: &'static [&'static str], group: Group, does: &'static str) -> Binding {
    Binding { id, contexts, chords, group, does, native: false }
}

use Context as C;
use Group as G;

/// Every key and gesture the app answers.
pub(crate) const BINDINGS: &[Binding] = &[
    // -- Move
    b("move.next", &[C::Nav, C::Board, C::TicketPage], &["j", "ArrowDown"], G::Move, "the next ticket in the nav, opened — from anywhere on a ticket page; on the board, the next row"),
    b("move.prev", &[C::Nav, C::Board, C::TicketPage], &["k", "ArrowUp"], G::Move, "the previous ticket, opened; on the nav's first ticket, its `under` filter one parent wider; on the board, the previous row"),
    b("move.first", &[C::Nav, C::Board], &["Home"], G::Move, "the first row (the board's, once one of its rows has focus)"),
    b("move.last", &[C::Nav, C::Board], &["End"], G::Move, "the last row"),
    Binding { id: "focus.tab", contexts: &[C::Global], chords: &["Tab"], group: G::Move, does: "into the nav tree, the board rows, and the controls", native: true },
    // -- Open
    b("open.focused", &[C::Nav, C::Board], &["Enter"], G::Open, "open the focused ticket (a group's row in the nav opens or closes)"),
    b("palette.open", &[C::Global], &["/"], G::Open, "jump to a ticket by its code, its title or words inside — or type `filter SC`, `filter <text>`, `deployed <env>`, `theme <name>`"),
    b("palette.toggle", &[C::Anywhere, C::Frame], &["Mod+k"], G::Open, "the jump palette from anywhere — a text field, a dialog, a framed body; again, it closes"),
    b("go.board", &[C::Global], &["g h"], G::Open, "go to the board"),
    b("go.needs", &[C::Global], &["g n"], G::Open, "the nav under the Needs you filter — what waits on you, within the header's Filter; again, the nav's default"),
    b("link.follow", &[C::Link], &["Click", "Mod+Click", "Shift+Click", "MiddleClick"], G::Open, "inside menger, go there; anywhere else, a dialog names the destination first"),
    b("menu.open", &[C::Ticket, C::Chip], &["RightClick", "ContextMenu", "Shift+F10"], G::Open, "the menu of what is under the pointer, or of the focused ticket: a ticket's actions, a chip's filters — a status, kind, severity or label chip's own values first"),
    // -- Status (ACT-10)
    b("status.up", &[C::Ticket], &["]"], G::Status, "the focused ticket (or its selection, or the ticket page's) one status on along the line: Idea → Pending → Ready → Coding → In Review → Complete"),
    b("status.down", &[C::Ticket], &["["], G::Status, "one status back along the line; off it (Blocked, Epic, Superseded, Canceled) or at an end, nothing changes and a note says why"),
    b("status.quick", &[C::Ticket], &["s"], G::Status, "the focused ticket's menu, opened at Set status, its current status focused — ↓/↑ and Enter set it"),
    // -- Select
    b("select.only", &[C::Nav, C::Board], &["Click"], G::Select, "open a ticket; the selection becomes its row — in the nav, a row with children opens after 250 ms, the double-click window"),
    b("select.range", &[C::Nav, C::Board], &["Shift+Click", "Shift+Mod+Click"], G::Select, "select from the last click to this row, in the order drawn"),
    b("select.toggle", &[C::Nav, C::Board], &["Mod+Click", "Space"], G::Select, "add or remove this row — by key, the focused one — from the selection"),
    b("select.extend-down", &[C::Nav, C::Board], &["Shift+j", "Shift+ArrowDown"], G::Select, "extend the selection a row down from the focused one"),
    b("select.extend-up", &[C::Nav, C::Board], &["Shift+k", "Shift+ArrowUp"], G::Select, "extend the selection a row up from the focused one"),
    b("select.all", &[C::Nav, C::Board], &["Mod+a"], G::Select, "select every row drawn"),
    // -- Nav
    b("nav.toggle", &[C::Nav], &["DoubleClick"], G::Nav, "open or close a row with children; its ticket does not open (a click waits 250 ms for the second)"),
    b("nav.expand", &[C::Nav], &["l"], G::Nav, "expand a group, or step into it"),
    b("nav.collapse", &[C::Nav], &["h"], G::Nav, "collapse a group, or step out to it"),
    b("nav.expand-subtree", &[C::Nav], &["*"], G::Nav, "expand the focused row's subtree, at every depth"),
    b("nav.expand-all", &[C::Nav], &["Shift+Numpad*"], G::Nav, "expand everything (or right-click › Nav › Expand all)"),
    b("nav.collapse-all", &[C::Nav], &["-"], G::Nav, "collapse everything, back to the shortcodes"),
    // -- Filter
    b("filter.view", &[C::Global, C::Nav, C::TicketPage], &["f"], G::Filter, "filter this view; on a ticket page, the nav to this ticket and its children"),
    b("filter.global", &[C::Global], &["Shift+f"], G::Filter, "filter every view (the global filter)"),
    b("chip.filter-everywhere", &[C::Chip], &["Alt+Click"], G::Filter, "the chip becomes the global filter"),
    // -- Annotate
    b("annotate.toggle", &[C::Global, C::Frame], &["a", "Shift+a"], G::Annotate, "annotation mode: outline the page, click anything to comment on it; again, it ends"),
    b("annotate.pick", &[C::Annotate], &["Click"], G::Annotate, "comment on what was clicked"),
    b("comment.save", &[C::Dialog], &["Mod+Enter"], G::Annotate, "save the comment being written"),
    // -- View
    b("view.full-screen", &[C::TicketPage, C::Frame], &["z"], G::View, "the ticket's framed body full screen, and back (from inside the body too)"),
    b("app.escape", &[C::Global, C::Frame], &["Escape"], G::View, "close the palette or this list; fold an open query tree; leave annotation mode; leave full screen; clear the selection"),
    // -- Menus & dialogs
    b("list.next", &[C::Palette, C::Menu], &["ArrowDown"], G::Dialogs, "the next item (from the last, the first)"),
    b("list.prev", &[C::Palette, C::Menu], &["ArrowUp"], G::Dialogs, "the previous item (from the first, the last)"),
    b("list.first", &[C::Palette, C::Menu], &["Home"], G::Dialogs, "the first item"),
    b("list.last", &[C::Palette, C::Menu], &["End"], G::Dialogs, "the last item"),
    b("list.choose", &[C::Palette, C::Menu], &["Enter"], G::Dialogs, "the highlighted row: open it (the palette), take it (the filter's menu)"),
    b("menu.into", &[C::Menu], &["ArrowRight"], G::Dialogs, "open the submenu, at its first item"),
    b("menu.out", &[C::Menu], &["ArrowLeft", "Backspace"], G::Dialogs, "close the submenu, back to its item (Backspace: in the filter's menu, its search empty)"),
    b("menu.leave", &[C::Menu], &["Tab"], G::Dialogs, "close the menu and move on (in the filter's menu, to its operator first)"),
    b("menu.leave-back", &[C::Menu], &["Shift+Tab"], G::Dialogs, "close the menu and move back"),
    b("dialog.close", &[C::Palette, C::Dialog, C::Menu, C::DatePicker], &["Escape"], G::Dialogs, "close it; focus goes back where it was"),
    b("dialog.next-field", &[C::Palette, C::Dialog], &["Tab"], G::Dialogs, "the next control, kept inside the dialog"),
    b("dialog.prev-field", &[C::Palette, C::Dialog], &["Shift+Tab"], G::Dialogs, "the previous control, kept inside the dialog"),
    // -- Forms
    b("form.submit", &[C::Field, C::Dialog], &["Enter"], G::Forms, "what the form's button does — Test, Save, Rename, Apply; in the leave dialog, open the link"),
    b("form.cancel", &[C::Field], &["Escape"], G::Forms, "leave the field, nothing applied"),
    b("date.prev-day", &[C::DatePicker], &["ArrowLeft"], G::Forms, "the day before"),
    b("date.next-day", &[C::DatePicker], &["ArrowRight"], G::Forms, "the day after"),
    b("date.prev-week", &[C::DatePicker], &["ArrowUp"], G::Forms, "a week earlier"),
    b("date.next-week", &[C::DatePicker], &["ArrowDown"], G::Forms, "a week later"),
    b("date.prev-month", &[C::DatePicker], &["PageUp"], G::Forms, "a month earlier"),
    b("date.next-month", &[C::DatePicker], &["PageDown"], G::Forms, "a month later"),
    b("date.prev-year", &[C::DatePicker], &["Shift+PageUp"], G::Forms, "a year earlier"),
    b("date.next-year", &[C::DatePicker], &["Shift+PageDown"], G::Forms, "a year later"),
    b("date.week-start", &[C::DatePicker], &["Home"], G::Forms, "the week's Monday"),
    b("date.week-end", &[C::DatePicker], &["End"], G::Forms, "the week's Sunday"),
    b("date.choose", &[C::DatePicker], &["Enter", "Space"], G::Forms, "choose the focused day"),
    b("time.up", &[C::TimeField], &["ArrowUp"], G::Forms, "a step up: an hour, a minute, one more of a length"),
    b("time.down", &[C::TimeField], &["ArrowDown"], G::Forms, "a step down"),
    b("time.up-ten", &[C::TimeField], &["PageUp", "Shift+ArrowUp"], G::Forms, "ten later"),
    b("time.down-ten", &[C::TimeField], &["PageDown", "Shift+ArrowDown"], G::Forms, "ten earlier"),
    // -- Help
    b("help.keys", &[C::Global], &["?"], G::Help, "this list (Settings › Keyboard has it too, with a search)"),
];

/// The binding with this id.
#[cfg(test)]
pub(crate) fn binding(id: &str) -> Option<&'static Binding> {
    BINDINGS.iter().find(|b| b.id == id)
}

// ---------------------------------------------------------------------
// Chords

/// A mouse button, as a step names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Button {
    Click,
    MiddleClick,
    RightClick,
    /// AGS-29: a `dblclick`.
    DoubleClick,
}

/// What a step presses.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Key {
    /// A printable character — a letter in lower case.
    Char(char),
    /// A named key, by `KeyboardEvent.key` (`Space` for `" "`).
    Named(&'static str),
    /// A numpad key by its character (`*`: `NumpadMultiply`).
    Numpad(char),
    Mouse(Button),
}

/// The keys a chord may name, by their `KeyboardEvent.key`.
const NAMED: &[&str] = &[
    "Escape", "Enter", "Tab", "Space", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End", "PageUp", "PageDown",
    "Backspace", "Delete", "ContextMenu", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
];

/// One press: a key or a button, and the modifiers held.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Step {
    pub(crate) key: Key,
    /// `Mod`: ⌘ on a Mac, Ctrl or ⌘ elsewhere.
    pub(crate) cmd: bool,
    pub(crate) ctrl: bool,
    pub(crate) meta: bool,
    pub(crate) alt: bool,
    pub(crate) shift: bool,
}

/// A chord: one step, or a sequence of two (`g h`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Chord {
    pub(crate) steps: Vec<Step>,
}

impl Step {
    fn parse(spec: &str) -> Result<Step, String> {
        let mut parts: Vec<&str> = spec.split('+').collect();
        // `Shift++` would name `+`: none of ours does, so a trailing empty
        // part is an error like any other.
        let key = parts.pop().filter(|k| !k.is_empty()).ok_or_else(|| format!("{spec:?}: no key"))?;
        let mut s = Step { key: Key::Char(' '), cmd: false, ctrl: false, meta: false, alt: false, shift: false };
        for m in parts {
            let flag = match m {
                "Mod" => &mut s.cmd,
                "Ctrl" => &mut s.ctrl,
                "Meta" => &mut s.meta,
                "Alt" => &mut s.alt,
                "Shift" => &mut s.shift,
                _ => return Err(format!("{spec:?}: {m:?} is no modifier (Mod, Ctrl, Meta, Alt, Shift)")),
            };
            if *flag {
                return Err(format!("{spec:?}: {m} twice"));
            }
            *flag = true;
        }
        let mut chars = key.chars();
        s.key = match (chars.next(), chars.next()) {
            (Some(c), None) if c.is_ascii_uppercase() => return Err(format!("{spec:?}: a letter is lower case — Shift+{}", c.to_ascii_lowercase())),
            (Some(c), None) => Key::Char(c),
            _ => match key {
                "Click" => Key::Mouse(Button::Click),
                "MiddleClick" => Key::Mouse(Button::MiddleClick),
                "RightClick" => Key::Mouse(Button::RightClick),
                "DoubleClick" => Key::Mouse(Button::DoubleClick),
                k if k.starts_with("Numpad") && k.chars().count() == 7 => Key::Numpad(k.chars().last().unwrap_or('*')),
                k => Key::Named(NAMED.iter().find(|n| **n == k).ok_or_else(|| format!("{spec:?}: {k:?} names no key"))?),
            },
        };
        Ok(s)
    }

    /// How many things the step names besides its key — the more, the more
    /// specific ([`best`]).
    fn weight(&self) -> u8 {
        [self.cmd, self.ctrl, self.meta, self.alt, self.shift].iter().filter(|f| **f).count() as u8 + u8::from(matches!(self.key, Key::Numpad(_)))
    }

    /// Whether `p` is this step (module doc, **Matching**).
    pub(crate) fn matches(&self, p: &Press, mac: bool) -> bool {
        if self.cmd {
            let held = if mac { p.meta } else { p.ctrl || p.meta };
            if !held {
                return false;
            }
        } else if p.ctrl != self.ctrl || p.meta != self.meta {
            return false;
        }
        if p.alt != self.alt {
            return false;
        }
        match &self.key {
            Key::Mouse(b) => p.button == Some(*b) && p.shift == self.shift,
            _ if p.button.is_some() => false,
            Key::Char(c) => {
                let mut k = p.key.chars();
                let (Some(k), None) = (k.next(), k.next()) else { return false };
                if c.is_ascii_alphabetic() {
                    k.to_ascii_lowercase() == *c && p.shift == self.shift
                } else {
                    k == *c && (p.shift || !self.shift)
                }
            }
            Key::Named("Space") => p.key == " " && p.shift == self.shift,
            Key::Named(n) => p.key == *n && p.shift == self.shift,
            Key::Numpad(c) => p.code == numpad_code(*c) && p.shift == self.shift,
        }
    }

    /// The step as this platform spells it.
    fn show(&self, mac: bool) -> String {
        let letter = matches!(self.key, Key::Char(c) if c.is_ascii_alphabetic());
        // Shift and a letter alone is the capital: `F`, `J`.
        let shifted_letter = letter && self.shift && !self.cmd && !self.ctrl && !self.meta && !self.alt;
        let plain = !self.shift && !self.cmd && !self.ctrl && !self.meta && !self.alt;
        let key = match &self.key {
            Key::Char(c) if letter && !plain => c.to_ascii_uppercase().to_string(),
            Key::Char(c) => c.to_string(),
            Key::Numpad(c) => format!("{c} (numpad)"),
            Key::Named(n) => match *n {
                "ArrowUp" => "↑",
                "ArrowDown" => "↓",
                "ArrowLeft" => "←",
                "ArrowRight" => "→",
                "Escape" => "Esc",
                "PageUp" => "Page Up",
                "PageDown" => "Page Down",
                "ContextMenu" => "Menu key",
                n => n,
            }
            .to_string(),
            Key::Mouse(b) => {
                let word = match b {
                    Button::Click => "click",
                    Button::MiddleClick => "middle-click",
                    Button::RightClick => "right-click",
                    Button::DoubleClick => "double-click",
                };
                if plain {
                    let mut w = word.to_string();
                    w[..1].make_ascii_uppercase();
                    return w;
                }
                word.to_string()
            }
        };
        if plain || shifted_letter {
            return key;
        }
        let mouse = matches!(self.key, Key::Mouse(_));
        let mut mods: Vec<&str> = Vec::new();
        if mac {
            // Apple's order: ⌃ ⌥ ⇧ ⌘.
            if self.ctrl {
                mods.push("⌃");
            }
            if self.alt {
                mods.push("⌥");
            }
            if self.shift {
                mods.push("⇧");
            }
            if self.cmd || self.meta {
                mods.push("⌘");
            }
            let sep = if mouse { "-" } else { "" };
            format!("{}{sep}{key}", mods.concat())
        } else {
            if self.cmd || self.ctrl {
                mods.push("Ctrl");
            }
            if self.meta && !self.cmd {
                mods.push("Meta");
            }
            if self.alt {
                mods.push("Alt");
            }
            if self.shift {
                mods.push("Shift");
            }
            mods.push(&key);
            mods.join("+")
        }
    }
}

/// `NumpadMultiply` for `*`, as `KeyboardEvent.code` names the numpad.
fn numpad_code(c: char) -> &'static str {
    match c {
        '*' => "NumpadMultiply",
        '+' => "NumpadAdd",
        '-' => "NumpadSubtract",
        '/' => "NumpadDivide",
        '.' => "NumpadDecimal",
        _ => "",
    }
}

impl Chord {
    /// `j`, `Shift+*`, `Mod+k`, `Shift+Numpad*`, `Alt+Click`, `g h`: steps
    /// apart by a space (two at most), modifiers before the key by `+`.
    pub(crate) fn parse(spec: &str) -> Result<Chord, String> {
        let steps = spec.split(' ').map(Step::parse).collect::<Result<Vec<_>, _>>()?;
        match steps.len() {
            1 => Ok(Chord { steps }),
            2 if steps.iter().all(|s| !matches!(s.key, Key::Mouse(_))) => Ok(Chord { steps }),
            _ => Err(format!("{spec:?}: one step, or a sequence of two keys")),
        }
    }

    /// The chord as this platform spells it: `Ctrl+K` or `⌘K`, `Alt+click`
    /// or `⌥-click`, `g h`.
    pub(crate) fn show(&self, mac: bool) -> String {
        self.steps.iter().map(|s| s.show(mac)).collect::<Vec<_>>().join(" ")
    }
}

// ---------------------------------------------------------------------
// The table matching reads

/// A binding and its chords, parsed — overridden, later ([`table_with`]).
#[derive(Clone, Debug)]
pub(crate) struct Entry {
    pub(crate) binding: &'static Binding,
    pub(crate) chords: Vec<Chord>,
}

/// [`BINDINGS`] with `overrides` laid over it: an id and the chords that
/// replace its own (module doc, **Later, rebinding**). An override naming
/// no binding, or a chord that does not parse, is left out.
pub(crate) fn table_with(overrides: &[(&str, Vec<String>)]) -> Vec<Entry> {
    BINDINGS
        .iter()
        .map(|b| {
            let specs: Vec<String> = match overrides.iter().find(|(id, _)| *id == b.id) {
                Some((_, mine)) => mine.clone(),
                None => b.chords.iter().map(|c| c.to_string()).collect(),
            };
            Entry { binding: b, chords: specs.iter().filter_map(|s| Chord::parse(s).ok()).collect() }
        })
        .collect()
}

thread_local! {
    static TABLE: OnceCell<Vec<Entry>> = const { OnceCell::new() };
    static MAC: OnceCell<bool> = const { OnceCell::new() };
    /// The first step of a sequence, armed: when, and by which keydown.
    static ARMED: RefCell<Option<Armed>> = const { RefCell::new(None) };
}

/// The table the app matches against: [`BINDINGS`], no overrides yet.
pub(crate) fn table() -> Vec<Entry> {
    TABLE.with(|t| t.get_or_init(|| table_with(&[])).clone())
}

fn with_table<R>(f: impl FnOnce(&[Entry]) -> R) -> R {
    TABLE.with(|t| f(t.get_or_init(|| table_with(&[]))))
}

/// Whether the browser runs on a Mac (or an iPhone, an iPad).
pub(crate) fn is_mac() -> bool {
    MAC.with(|m| {
        *m.get_or_init(|| {
            let p = crate::dom::window().navigator().platform().unwrap_or_default();
            p.contains("Mac") || p.contains("iPhone") || p.contains("iPad")
        })
    })
}

// ---------------------------------------------------------------------
// Matching (pure — what the native tests hold)

/// An event, as matching reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Press {
    /// `KeyboardEvent.key` (empty for a mouse press).
    pub(crate) key: String,
    /// `KeyboardEvent.code`.
    pub(crate) code: String,
    /// The mouse button, for a mouse press.
    pub(crate) button: Option<Button>,
    pub(crate) ctrl: bool,
    pub(crate) meta: bool,
    pub(crate) alt: bool,
    pub(crate) shift: bool,
    /// The event's `timeStamp` — the same keydown seen by two listeners.
    pub(crate) at: f64,
}

impl Press {
    /// A bare key, as a frame posts it (`a`, `A`, `z`, `Escape`) or a widget
    /// hands it on: a capital letter is Shift and its letter.
    pub(crate) fn key(key: &str) -> Press {
        let shift = key.chars().count() == 1 && key.chars().all(|c| c.is_ascii_uppercase());
        Press { key: key.to_string(), shift, ..Press::default() }
    }
}

/// A sequence's first step, armed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Armed {
    step: Step,
    since: f64,
    by: f64,
}

/// What a press does to the armed sequence.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Next {
    Keep,
    Disarm,
    Arm(Armed),
}

/// The binding of `ctx` a single step of which `p` is — the most specific,
/// when two match.
fn best<'a>(table: &'a [Entry], p: &Press, ctx: Context, mac: bool) -> Option<&'a Entry> {
    table
        .iter()
        .filter(|e| e.binding.contexts.contains(&ctx))
        .filter_map(|e| {
            let w = e.chords.iter().filter(|c| c.steps.len() == 1 && c.steps[0].matches(p, mac)).map(|c| c.steps[0].weight()).max()?;
            Some((w, e))
        })
        .max_by_key(|(w, _)| *w)
        .map(|(_, e)| e)
}

/// What `p` is in `ctx`, the sequence `armed` (if any) at `now`; and what it
/// does to the sequence (module doc, **Sequences**).
pub(crate) fn pick(table: &[Entry], p: &Press, ctx: Context, mac: bool, armed: Option<&Armed>, now: f64) -> (Option<&'static str>, Next) {
    let mut next = Next::Keep;
    // The same keydown seen again (another listener): not a second step.
    if let Some(a) = armed.filter(|a| now - a.since < SEQUENCE_MS && a.by != p.at) {
        let completes = table.iter().find(|e| {
            e.chords.iter().any(|c| c.steps.len() == 2 && c.steps[0] == a.step && c.steps[1].matches(p, mac))
        });
        match completes {
            Some(e) if e.binding.contexts.contains(&ctx) => return (Some(e.binding.id), Next::Disarm),
            // Another context's to complete — `h` after `g`, in the nav.
            Some(e) if e.binding.contexts.iter().any(|c| c.live_with(ctx)) => return (None, Next::Keep),
            _ => next = Next::Disarm,
        }
    }
    if let Some(e) = best(table, p, ctx, mac) {
        return (Some(e.binding.id), next);
    }
    let first = table.iter().filter(|e| e.binding.contexts.contains(&ctx)).find_map(|e| {
        e.chords.iter().find(|c| c.steps.len() == 2 && c.steps[0].matches(p, mac)).map(|c| c.steps[0].clone())
    });
    match first {
        Some(step) => (Some(PREFIX), Next::Arm(Armed { step, since: now, by: p.at })),
        None => (None, next),
    }
}

/// The single-step binding of `ctx` that `p` is, without touching a
/// sequence.
pub(crate) fn pick_single(table: &[Entry], p: &Press, ctx: Context, mac: bool) -> Option<&'static str> {
    best(table, p, ctx, mac).map(|e| e.binding.id)
}

// ---------------------------------------------------------------------
// The handlers' doors

fn press_of(ev: &web_sys::KeyboardEvent) -> Press {
    Press {
        key: ev.key(),
        code: ev.code(),
        button: None,
        ctrl: ev.ctrl_key(),
        meta: ev.meta_key(),
        alt: ev.alt_key(),
        shift: ev.shift_key(),
        at: ev.time_stamp(),
    }
}

/// What this keydown is in `ctx` — a binding's id, [`PREFIX`] for a
/// sequence's first key, or `None`. Arms and ends sequences; never answers
/// mid-composition.
pub(crate) fn hit(ev: &web_sys::KeyboardEvent, ctx: Context) -> Option<&'static str> {
    if ev.is_composing() {
        return None;
    }
    let p = press_of(ev);
    let now = js_sys::Date::now();
    let armed = ARMED.with(|a| a.borrow().clone());
    let (id, next) = with_table(|t| pick(t, &p, ctx, is_mac(), armed.as_ref(), now));
    match next {
        Next::Keep => {}
        Next::Disarm => ARMED.with(|a| *a.borrow_mut() = None),
        Next::Arm(new) => ARMED.with(|a| *a.borrow_mut() = Some(new)),
    }
    id
}

/// Whether this keydown is binding `id` in `ctx` — single steps only; no
/// sequence is armed or ended by asking.
pub(crate) fn is(ev: &web_sys::KeyboardEvent, ctx: Context, id: &str) -> bool {
    !ev.is_composing() && with_table(|t| pick_single(t, &press_of(ev), ctx, is_mac())) == Some(id)
}

/// What a bare key (a frame's post, a widget's hand-off) is in `ctx`. A
/// frame's `palette` is the palette's chord pressed inside it (MENGER-6).
pub(crate) fn key_hit(key: &str, ctx: Context) -> Option<&'static str> {
    let mac = is_mac();
    let p = if key == "palette" { Press { key: "k".into(), ctrl: !mac, meta: mac, ..Press::default() } } else { Press::key(key) };
    with_table(|t| pick_single(t, &p, ctx, mac))
}

/// The mouse press an event is: a `contextmenu` a right-click whatever its
/// button, a `dblclick` a double-click (of the main button); `auxclick` and
/// the rest by their button.
fn mouse_press(ev: &web_sys::MouseEvent) -> Press {
    let button = if ev.type_() == "contextmenu" {
        Button::RightClick
    } else if ev.type_() == "dblclick" && ev.button() == 0 {
        Button::DoubleClick
    } else {
        match ev.button() {
            1 => Button::MiddleClick,
            2 => Button::RightClick,
            _ => Button::Click,
        }
    };
    let button = if ev.button() > 2 { None } else { Some(button) };
    Press { button, ctrl: ev.ctrl_key(), meta: ev.meta_key(), alt: ev.alt_key(), shift: ev.shift_key(), at: ev.time_stamp(), ..Press::default() }
}

/// Whether a mouse event is the middle button's.
pub(crate) fn is_middle(ev: &web_sys::MouseEvent) -> bool {
    mouse_press(ev).button == Some(Button::MiddleClick)
}

/// What this click (mousedown, right-click…) is in `ctx`.
pub(crate) fn hit_mouse(ev: &web_sys::MouseEvent, ctx: Context) -> Option<&'static str> {
    let p = mouse_press(ev);
    p.button?;
    with_table(|t| pick_single(t, &p, ctx, is_mac()))
}

/// Whether the key pressed types a character (`j`, not `ArrowDown`) — the
/// board's one distinction within a binding: a letter with no row focused
/// starts at the near end, an arrow is the page's own scroll.
pub(crate) fn typed(ev: &web_sys::KeyboardEvent) -> bool {
    ev.key().chars().count() == 1
}

/// Whether a key event (any event with a `key`) is the one that moves focus
/// — src/gesture.rs's "how did focus get here", which answers no key.
pub(crate) fn moves_focus(ev: &web_sys::Event) -> bool {
    js_sys::Reflect::get(ev, &wasm_bindgen::JsValue::from_str("key")).ok().and_then(|k| k.as_string()).is_some_and(|k| {
        with_table(|t| pick_single(t, &Press::key(&k), Context::Global, false)) == Some("focus.tab")
    })
}

// ---------------------------------------------------------------------
// What the tables draw

/// The chords of a binding as this platform spells them, apart by ` · `.
pub(crate) fn keys_text(e: &Entry, mac: bool) -> String {
    e.chords.iter().map(|c| c.show(mac)).collect::<Vec<_>>().join(" · ")
}

/// Where a binding applies, in words.
pub(crate) fn where_text(b: &Binding) -> String {
    b.contexts.iter().map(|c| c.words()).collect::<Vec<_>>().join(" · ")
}

/// Whether the chord `c` is what `q` spells: its spec, or its spelling on
/// either platform, without case — `esc`, `ctrl+k`, `⌘k`, `g h`, `*`.
fn spells(c: &Chord, q: &str) -> bool {
    let shown = [c.show(false), c.show(true)];
    // One character is a key as typed: `j` is not `J`.
    if q.chars().count() == 1 {
        return shown.iter().any(|s| s == q);
    }
    Chord::parse(q.trim()).is_ok_and(|p| p == *c) || shown.iter().any(|s| s.to_lowercase() == q.to_lowercase())
}

/// The table's rows `query` keeps, in the table's order. Empty: every row.
/// A key — the query spells a chord some row has (`j`, `Esc`, `Ctrl+K`,
/// `⌘K`, `g h`, `Alt+click`), or is one character — keeps the rows with
/// that chord; otherwise every word of the query is in the row's text: what
/// it does, its id, its group, where, its keys.
pub(crate) fn search<'a>(table: &'a [Entry], query: &str, mac: bool) -> Vec<&'a Entry> {
    let q = query.trim();
    if q.is_empty() {
        return table.iter().collect();
    }
    let by_key: Vec<&Entry> = table.iter().filter(|e| e.chords.iter().any(|c| spells(c, q))).collect();
    if !by_key.is_empty() || q.chars().count() == 1 {
        return by_key;
    }
    let words: Vec<String> = q.to_lowercase().split_whitespace().map(str::to_string).collect();
    table
        .iter()
        .filter(|e| {
            let b = e.binding;
            let text = format!("{} {} {} {} {}", b.does, b.id, b.group.label(), where_text(b), keys_text(e, mac)).to_lowercase();
            words.iter().all(|w| text.contains(w.as_str()))
        })
        .collect()
}

#[cfg(test)]
#[path = "bindings_tests.rs"]
mod tests;
