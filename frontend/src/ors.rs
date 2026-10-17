//! FILTER-39 (FILTER-38's DECIDED field, owner 2026-10-17: "ors should be
//! explicit not any of. clauses between ors are anded"): **the filter the
//! panel builds — groups joined by an explicit `or`, every clause inside a
//! group joined by `and`.** No "any of", no "all of", no join toggle.
//!
//! - **A filter, drawn** ([`Ors`]): its groups, each a row of clauses (a
//!   clause is one predicate of FILTER-3's language — `kind build`, `not
//!   ready`, `in KILN`, `under KILN`, a time clause, `saved "Name"`).
//!   [`Ors::of_filter`] reads any filter into that shape — the language's
//!   or-of-and, its lists spelled out: `kind build and status in (Ready,
//!   Coding)` is `(kind build and status Ready) or (kind build and status
//!   Coding)`, the clause the two share drawn once in each group. A filter
//!   that would spell out into more than [`MOST`] groups is not drawn (the
//!   panel says to edit it as text).
//! - **Back to the language** ([`Ors::to_groups`]): one group is its
//!   clauses as the filter's top-level groups (`a and b`); several, one
//!   `or` of them — read back through the language, so the text is
//!   canonical and what it refuses (`text` inside an `or`) is refused in
//!   its words. An empty group is a draft (`+ or`, a group emptied by its
//!   last ×): it is no part of the filter.
//! - **The panel's edits**: [`Ors::pick`] — a value ticked in a section,
//!   into the selected group: a clause there already is taken out (or
//!   turned, `is` ⇄ `is not`); a second value of a category a ticket holds
//!   one of (a second status, kind, severity, shortcode) is an alternative,
//!   so it makes **a group of its own** after the selected one, the
//!   group's other clauses kept in both — and says so ([`Picked::note`]);
//!   anything else is and-ed in. [`Ors::remove`], [`Ors::remove_group`],
//!   [`Ors::add_group`] (`+ or`), and the time clauses' [`Ors::time_at`] /
//!   [`Ors::put_time`].
//! - **What a section draws** ([`Ors::state`], [`Ors::elsewhere`]): a
//!   value's box in the selected group (on, negated, off, or covered by a
//!   host shortcode's `under`), and the other groups that hold it.
//!
//! Everything here is pure and pinned by native tests below.

use menger_tickets_api::filter::{Conj, Field, Filter, Group, Predicate};

use crate::filter::{Cat, Op, ScTree};
use crate::lens::{field_of, token_of};
use crate::when::Timed;

/// The most groups a filter spells out into and is still drawn.
pub(crate) const MOST: usize = 32;

/// A filter as the panel draws it: groups joined by `or`, each a row of
/// clauses joined by `and`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Ors {
    pub(crate) groups: Vec<Vec<Predicate>>,
}

/// Where a clause is: its group and its place in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Slot {
    pub(crate) group: usize,
    pub(crate) item: usize,
}

/// What a tick did: the group the panel now edits, and — a second value
/// made a group of its own — what to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Picked {
    pub(crate) sel: usize,
    pub(crate) note: Option<String>,
}

/// A value's box in the selected group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    Off,
    /// `is`.
    On,
    /// `is not`.
    Not,
    /// Covered by a host shortcode's `under` in the group.
    Covered,
}

impl Mark {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Mark::Off => "off",
            Mark::On => "on",
            Mark::Not => "not",
            Mark::Covered => "covered",
        }
    }
}

/// `groups` with `more` and-ed to each: every pair, joined.
fn product(a: Vec<Vec<Predicate>>, b: Vec<Vec<Predicate>>) -> Option<Vec<Vec<Predicate>>> {
    if a.len().saturating_mul(b.len()) > MOST {
        return None;
    }
    let mut out = Vec::with_capacity(a.len() * b.len());
    for x in &a {
        for y in &b {
            let mut g = x.clone();
            for p in y {
                if !g.contains(p) {
                    g.push(p.clone());
                }
            }
            out.push(g);
        }
    }
    Some(out)
}

/// A predicate, `not` folded in (`flip`), spelled out as or-of-and.
fn spell_pred(p: &Predicate, flip: bool) -> Option<Vec<Vec<Predicate>>> {
    let not = p.not != flip;
    match &p.field {
        Field::Group(g) => spell_group(g, not),
        f => Some(vec![vec![Predicate { field: f.clone(), not }]]),
    }
}

/// A group, negated when `not` (De Morgan), spelled out as or-of-and.
fn spell_group(g: &Group, not: bool) -> Option<Vec<Vec<Predicate>>> {
    let any = matches!((g.conj, not), (Conj::Any, false) | (Conj::All, true)) && g.preds.len() > 1;
    let parts: Vec<Vec<Vec<Predicate>>> = g.preds.iter().map(|p| spell_pred(p, not)).collect::<Option<_>>()?;
    if any {
        let out: Vec<Vec<Predicate>> = parts.into_iter().flatten().collect();
        return (out.len() <= MOST).then_some(out);
    }
    parts.into_iter().try_fold(vec![Vec::new()], product)
}

impl Ors {
    /// The filter as or-of-and — `None` when it spells out into more than
    /// [`MOST`] groups. The empty filter is no group.
    pub(crate) fn of_filter(f: &Filter) -> Option<Ors> {
        let mut groups = vec![Vec::new()];
        for g in &f.groups {
            groups = product(groups, spell_group(g, false)?)?;
        }
        let mut out: Vec<Vec<Predicate>> = Vec::new();
        for g in groups.into_iter().filter(|g| !g.is_empty()) {
            if !out.contains(&g) {
                out.push(g);
            }
        }
        Some(Ors { groups: out })
    }

    /// The groups that say something — drafts left out.
    pub(crate) fn live(&self) -> impl Iterator<Item = &Vec<Predicate>> {
        self.groups.iter().filter(|g| !g.is_empty())
    }

    /// The filter's groups: one group's clauses each a top-level group;
    /// several, one `or` of them — canonical, through the language.
    pub(crate) fn to_groups(&self) -> Result<Vec<Group>, String> {
        let live: Vec<&Vec<Predicate>> = self.live().collect();
        let groups: Vec<Group> = match live.as_slice() {
            [] => Vec::new(),
            [one] => one.iter().map(|p| Group::one(p.clone())).collect(),
            many => vec![Group::of(Conj::Any, many.iter().map(|g| Group::of(Conj::All, (*g).clone()).nested(false)).collect())],
        };
        let text = Filter { groups, ..Filter::default() }.format();
        Filter::parse(&text).map(|f| f.groups).map_err(|e| e.to_string())
    }

    /// How many clauses it holds.
    pub(crate) fn clauses(&self) -> usize {
        self.groups.iter().map(Vec::len).sum()
    }

    /// The group at `sel` — made, empty, when there is none.
    fn group_mut(&mut self, sel: usize) -> (usize, &mut Vec<Predicate>) {
        if self.groups.is_empty() {
            self.groups.push(Vec::new());
        }
        let sel = sel.min(self.groups.len() - 1);
        (sel, &mut self.groups[sel])
    }

    /// `+ or`: a new, empty group after the last (none made while the last
    /// is empty already) — its place.
    pub(crate) fn add_group(&mut self) -> usize {
        if self.groups.last().is_none_or(|g| !g.is_empty()) {
            self.groups.push(Vec::new());
        }
        self.groups.len() - 1
    }

    /// × on a group: it and all it holds.
    pub(crate) fn remove_group(&mut self, at: usize) {
        if at < self.groups.len() {
            self.groups.remove(at);
        }
    }

    /// × on a clause; a group left empty goes with it, unless it is the
    /// only one.
    pub(crate) fn remove(&mut self, s: Slot) {
        let Some(g) = self.groups.get_mut(s.group) else { return };
        if s.item < g.len() {
            g.remove(s.item);
        }
        if g.is_empty() && self.groups.len() > 1 {
            self.groups.remove(s.group);
        }
    }

    /// A value of `c` ticked in the selected group — see the module's
    /// words. `token` is the category's value (a flag's `yes` / `no`); `op`
    /// the section's switch; `tree` the shortcodes, for a host's `under`.
    pub(crate) fn pick(&mut self, sel: usize, c: Cat, token: &str, op: Op, tree: Option<&ScTree>) -> Result<Picked, String> {
        let p = clause(c, token, op, tree).ok_or_else(|| format!("\u{201c}{token}\u{201d} is not one of {}'s values", c.label()))?;
        let (sel, g) = self.group_mut(sel);
        if let Some(i) = g.iter().position(|q| q.field == p.field) {
            if g[i].not == p.not {
                g.remove(i);
            } else {
                g[i] = p;
            }
            return Ok(Picked { sel, note: None });
        }
        let one_of = matches!(c, Cat::Status | Cat::Kind | Cat::Severity | Cat::Shortcode);
        let same = g.iter().position(|q| !q.not && cat_of(q) == Some(c));
        match (one_of && !p.not, same) {
            (true, Some(j)) => {
                let was = g[j].clone();
                let rest: Vec<String> = g.iter().filter(|q| **q != was).map(Predicate::format).collect();
                let mut twin = g.clone();
                twin[j] = p.clone();
                self.groups.insert(sel + 1, twin);
                let note = format!(
                    "A ticket has one {}: {} and {} are alternatives, so {} is group {} \u{2014} or \u{2014}{}",
                    c.label().to_ascii_lowercase(),
                    was.format(),
                    p.format(),
                    p.format(),
                    sel + 2,
                    match rest.is_empty() {
                        true => ".".to_string(),
                        false => format!(" with {} kept in both.", rest.join(" and ")),
                    }
                );
                Ok(Picked { sel, note: Some(note) })
            }
            _ => {
                g.push(p);
                Ok(Picked { sel, note: None })
            }
        }
    }

    /// A saved filter ticked: in the selected group when it is empty (or
    /// there already: taken out), else a group of its own after it.
    pub(crate) fn pick_saved(&mut self, sel: usize, name: &str) -> Picked {
        let p = Predicate::is(Field::Saved(name.to_string()));
        let (sel, g) = self.group_mut(sel);
        if let Some(i) = g.iter().position(|q| *q == p) {
            g.remove(i);
            return Picked { sel, note: None };
        }
        if g.is_empty() {
            g.push(p);
            return Picked { sel, note: None };
        }
        self.groups.insert(sel + 1, vec![p]);
        Picked { sel: sel + 1, note: Some(format!("\u{201c}{name}\u{201d} joined as group {} \u{2014} or \u{2014} beside the others.", sel + 2)) }
    }

    /// The box `c`'s `token` draws in group `sel`.
    pub(crate) fn state(&self, sel: usize, c: Cat, token: &str, tree: Option<&ScTree>) -> Mark {
        let Some(g) = self.groups.get(sel) else { return Mark::Off };
        if c.is_flag() {
            let Some(f) = field_of(c, "yes") else { return Mark::Off };
            return match g.iter().find(|q| q.field == f) {
                Some(q) if q.not == (token == "no") => Mark::On,
                _ => Mark::Off,
            };
        }
        let fields = fields_of(c, token, tree);
        if let Some(q) = g.iter().find(|q| fields.contains(&q.field)) {
            return if q.not { Mark::Not } else { Mark::On };
        }
        if c == Cat::Shortcode {
            if let Some(t) = tree {
                let code = token.to_ascii_uppercase();
                let covered = g.iter().any(|q| !q.not && matches!(&q.field, Field::Under(h) if *h != code && t.subtree(h).contains(&code)));
                if covered {
                    return Mark::Covered;
                }
            }
        }
        Mark::Off
    }

    /// The other groups (numbered from 1) holding `c`'s `token`, as `sel`'s
    /// sections mark it.
    pub(crate) fn elsewhere(&self, sel: usize, c: Cat, token: &str, tree: Option<&ScTree>) -> Vec<usize> {
        (0..self.groups.len()).filter(|i| *i != sel && self.state(*i, c, token, tree) != Mark::Off).map(|i| i + 1).collect()
    }

    /// The time clause at `s`.
    pub(crate) fn time_at(&self, s: Slot) -> Option<Timed> {
        let p = self.groups.get(s.group)?.get(s.item)?;
        match &p.field {
            Field::History(h) => Some(Timed { not: p.not, h: h.clone() }),
            _ => None,
        }
    }

    /// `t` written at `slot`, or — none — added last to group `sel`; where
    /// it now is.
    pub(crate) fn put_time(&mut self, sel: usize, slot: Option<Slot>, t: &Timed) -> Slot {
        let p = Predicate { field: Field::History(t.h.clone()), not: t.not };
        if let Some(s) = slot {
            if let Some(q) = self.groups.get_mut(s.group).and_then(|g| g.get_mut(s.item)) {
                *q = p;
                return s;
            }
        }
        let (sel, g) = self.group_mut(sel);
        g.push(p);
        Slot { group: sel, item: g.len() - 1 }
    }

    /// The time clauses group `sel` holds of category `c` (When, Deployed,
    /// Chats), with where each is.
    pub(crate) fn times_in(&self, sel: usize, c: Cat) -> Vec<(Slot, Timed)> {
        let Some(g) = self.groups.get(sel) else { return Vec::new() };
        g.iter()
            .enumerate()
            .filter_map(|(item, _)| {
                let s = Slot { group: sel, item };
                self.time_at(s).filter(|t| crate::filter::time_cat(t) == c).map(|t| (s, t))
            })
            .collect()
    }
}

/// The category a clause is drawn under — none for one no category draws
/// (`category`, `ticket`, `text`, a saved name).
pub(crate) fn cat_of(p: &Predicate) -> Option<Cat> {
    match &p.field {
        Field::Under(_) => Some(Cat::Shortcode),
        Field::History(h) => Some(crate::filter::time_cat(&Timed { not: p.not, h: h.clone() })),
        f => token_of(f).map(|(c, _)| c),
    }
}

/// The fields `c`'s `token` may be in a group: a shortcode by `in` or, as
/// a host, `under`.
fn fields_of(c: Cat, token: &str, _tree: Option<&ScTree>) -> Vec<Field> {
    match c {
        Cat::Shortcode => {
            let code = token.to_ascii_uppercase();
            vec![Field::In(code.clone()), Field::Under(code)]
        }
        _ => field_of(c, token).into_iter().collect(),
    }
}

/// The clause a tick of `c`'s `token` under `op` makes: a flag's `yes` /
/// `no` is it or its negation; a shortcode that hosts others is `under` it
/// (it and every shortcode hosted under it), any other `in` it.
pub(crate) fn clause(c: Cat, token: &str, op: Op, tree: Option<&ScTree>) -> Option<Predicate> {
    if c.is_flag() {
        let field = field_of(c, "yes")?;
        return Some(Predicate { field, not: token == "no" });
    }
    let field = match c {
        Cat::Shortcode => {
            let code = token.to_ascii_uppercase();
            match tree.is_some_and(|t| t.subtree(&code).len() > 1) {
                true => Field::Under(code),
                false => Field::In(code),
            }
        }
        _ => field_of(c, token)?,
    };
    Some(Predicate { field, not: op == Op::Not })
}

/// A clause as the panel and the chips write it — the language's words, a
/// time clause in words, a saved name by name.
pub(crate) fn words(p: &Predicate) -> String {
    match &p.field {
        Field::History(h) => crate::when::words(&Timed { not: p.not, h: h.clone() }, crate::calendar::reader_h12()),
        Field::Saved(n) => format!("{}saved \u{201c}{n}\u{201d}", if p.not { "not " } else { "" }),
        _ => p.format(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(text: &str) -> Filter {
        Filter::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
    }

    fn texts(o: &Ors) -> Vec<String> {
        o.groups.iter().map(|g| g.iter().map(Predicate::format).collect::<Vec<_>>().join(" and ")).collect()
    }

    fn back(o: &Ors) -> String {
        Filter { groups: o.to_groups().expect("reads"), ..Filter::default() }.format()
    }

    /// One group is the filter's own and; lists are spelled out, the clause
    /// they share drawn in each group; a negated list stays one group of
    /// nots; De Morgan turns a negated and.
    #[test]
    fn a_filter_reads_as_groups_joined_by_or() {
        assert_eq!(texts(&Ors::of_filter(&f("")).unwrap()), Vec::<String>::new());
        assert_eq!(texts(&Ors::of_filter(&f("ready and kind build and in SCALE")).unwrap()), ["ready and kind build and in SCALE"]);
        assert_eq!(
            texts(&Ors::of_filter(&f("kind build and status in (Ready, Coding)")).unwrap()),
            ["kind build and status Ready", "kind build and status Coding"]
        );
        assert_eq!(texts(&Ors::of_filter(&f("not status in (Ready, Coding)")).unwrap()), ["not status Ready and not status Coding"]);
        assert_eq!(
            texts(&Ors::of_filter(&f("(kind build and status Ready) or (kind gap and severity high)")).unwrap()),
            ["kind build and status Ready", "kind gap and severity high"]
        );
        assert_eq!(texts(&Ors::of_filter(&f("not (ready and blocked)")).unwrap()), ["not ready", "not blocked"]);
        assert_eq!(
            texts(&Ors::of_filter(&f("status in (Ready, Coding) and kind in (build, gap)")).unwrap()),
            ["status Ready and kind build", "status Ready and kind gap", "status Coding and kind build", "status Coding and kind gap"]
        );
        assert_eq!(texts(&Ors::of_filter(&f("saved \"Hot gaps\" or in KILN")).unwrap()), ["saved \"Hot gaps\"", "in KILN"]);
    }

    /// Past [`MOST`] groups a filter is not drawn.
    #[test]
    fn a_filter_too_wide_is_not_drawn() {
        let six = "status in (Ready, Coding, Pending, Blocked, Idea, Epic)";
        assert!(Ors::of_filter(&f(&format!("{six} and kind in (build, gap, spike, scope, execution, doc)"))).is_none());
        assert!(Ors::of_filter(&f(&format!("{six} and kind in (build, gap, spike, scope, execution)"))).is_some());
    }

    /// Back to the language: canonical, and the round trip holds for every
    /// filter already in or-of-and's shape.
    #[test]
    fn groups_go_back_to_the_language() {
        for text in [
            "",
            "ready and kind build and in SCALE",
            "((kind build and status Ready) or (kind gap and severity high))",
            "status in (Ready, Coding)",
            "not status Ready and not status Coding",
            "(saved \"Hot gaps\" or (status Coding and in KILN))",
            "not menger doc",
            "under KILN and kind build",
        ] {
            let o = Ors::of_filter(&f(text)).unwrap();
            assert_eq!(back(&o), text, "{text}");
        }
        // A draft group is no part of it.
        let mut o = Ors::of_filter(&f("kind build")).unwrap();
        o.add_group();
        assert_eq!(back(&o), "kind build");
        assert_eq!(o.groups.len(), 2);
        // What the language refuses is refused in its words.
        let o = Ors { groups: vec![vec![Predicate::is(Field::Text("x".into()))], vec![Predicate::is(Field::Ready)]] };
        assert!(o.to_groups().unwrap_err().contains("text"));
    }

    /// FILTER-38's mockup *The panel open*: a second kind is an alternative
    /// — a group of its own, the status kept in both, and the panel says
    /// so; a tick on a clause there takes it out; `is not` turns it; a
    /// label and-s in.
    #[test]
    fn a_second_value_of_one_category_is_another_group() {
        let mut o = Ors::of_filter(&f("kind build and status Ready")).unwrap();
        let p = o.pick(0, Cat::Kind, "gap", Op::Is, None).unwrap();
        assert_eq!(texts(&o), ["kind build and status Ready", "kind gap and status Ready"]);
        assert_eq!(p.sel, 0);
        let note = p.note.unwrap();
        assert!(note.contains("kind build and kind gap are alternatives") && note.contains("group 2") && note.contains("status Ready kept in both"), "{note}");
        assert_eq!(back(&o), "((kind build and status Ready) or (kind gap and status Ready))");
        // In group 2 now: a label is and-ed.
        o.pick(1, Cat::Label, "frontend", Op::Is, None).unwrap();
        assert_eq!(texts(&o)[1], "kind gap and status Ready and label frontend");
        // `is not` on a status there turns it.
        o.pick(1, Cat::Status, "Ready", Op::Not, None).unwrap();
        assert_eq!(texts(&o)[1], "kind gap and not status Ready and label frontend");
        // Ticked again under the same switch: taken out.
        o.pick(1, Cat::Status, "Ready", Op::Not, None).unwrap();
        assert_eq!(texts(&o)[1], "kind gap and label frontend");
        // Two `is not`s of one category are and-ed, never split.
        o.pick(0, Cat::Status, "Pending", Op::Not, None).unwrap();
        assert_eq!(texts(&o)[0], "kind build and status Ready and not status Pending");
        assert_eq!(o.groups.len(), 2);
        // A flag: `no` is its negation, `yes` turns it back, again takes it out.
        o.pick(0, Cat::Ready, "no", Op::Is, None).unwrap();
        assert_eq!(o.state(0, Cat::Ready, "no", None), Mark::On);
        o.pick(0, Cat::Ready, "yes", Op::Is, None).unwrap();
        assert_eq!(o.state(0, Cat::Ready, "yes", None), Mark::On);
        o.pick(0, Cat::Ready, "yes", Op::Is, None).unwrap();
        assert_eq!(o.state(0, Cat::Ready, "yes", None), Mark::Off);
        // An empty filter: the first tick makes group 1.
        let mut e = Ors::default();
        e.pick(0, Cat::Status, "Ready", Op::Is, None).unwrap();
        e.pick(0, Cat::Status, "Coding", Op::Is, None).unwrap();
        assert_eq!(back(&e), "status in (Ready, Coding)");
        assert_eq!(texts(&e), ["status Ready", "status Coding"]);
    }

    /// The boxes: on, not, off; the other groups that hold a value.
    #[test]
    fn a_sections_boxes_are_the_selected_groups() {
        let o = Ors::of_filter(&f("(kind build and not status Pending) or (kind gap)")).unwrap();
        assert_eq!(o.state(0, Cat::Kind, "build", None), Mark::On);
        assert_eq!(o.state(0, Cat::Status, "Pending", None), Mark::Not);
        assert_eq!(o.state(0, Cat::Kind, "gap", None), Mark::Off);
        assert_eq!(o.elsewhere(0, Cat::Kind, "gap", None), [2]);
        assert_eq!(o.elsewhere(1, Cat::Kind, "build", None), [1]);
        assert_eq!(o.state(5, Cat::Kind, "gap", None), Mark::Off);
    }

    /// `+ or` adds a draft group (one at a time); × on a clause, a group;
    /// a saved filter fills an empty group or joins as one of its own.
    #[test]
    fn groups_are_added_and_taken_out() {
        let mut o = Ors::of_filter(&f("kind build and status Ready")).unwrap();
        assert_eq!(o.add_group(), 1);
        assert_eq!(o.add_group(), 1, "one draft at a time");
        let p = o.pick_saved(1, "Hot gaps");
        assert_eq!((p.sel, p.note), (1, None));
        assert_eq!(back(&o), "((kind build and status Ready) or saved \"Hot gaps\")");
        let p = o.pick_saved(0, "Needs you");
        assert_eq!(p.sel, 1);
        assert_eq!(texts(&o), ["kind build and status Ready", "saved \"Needs you\"", "saved \"Hot gaps\""]);
        o.remove(Slot { group: 1, item: 0 });
        assert_eq!(texts(&o), ["kind build and status Ready", "saved \"Hot gaps\""]);
        o.remove_group(0);
        assert_eq!(back(&o), "saved \"Hot gaps\"");
        o.remove(Slot { group: 0, item: 0 });
        assert_eq!(o.groups, vec![Vec::<Predicate>::new()], "the last group stays, empty");
        assert_eq!(back(&o), "");
    }

    /// A host shortcode is `under` it — it and what it hosts, one clause;
    /// a hosted one reads as covered by it.
    #[test]
    fn a_host_shortcode_is_under_it() {
        let t = crate::filter::tests::kiln_tree();
        let mut o = Ors::default();
        o.pick(0, Cat::Shortcode, "KILN", Op::Is, Some(&t)).unwrap();
        assert_eq!(back(&o), "under KILN");
        assert_eq!(o.state(0, Cat::Shortcode, "KILN", Some(&t)), Mark::On);
        assert_eq!(o.state(0, Cat::Shortcode, "GLAZE", Some(&t)), Mark::Covered);
        o.pick(0, Cat::Shortcode, "PANTRY", Op::Is, Some(&t)).unwrap();
        assert_eq!(back(&o), "(under KILN or in PANTRY)");
    }

    /// The time clauses: added to the selected group, rewritten where they
    /// are, found by category.
    #[test]
    fn time_clauses_live_in_their_group() {
        let mut o = Ors::of_filter(&f("kind build or kind gap")).unwrap();
        let t = crate::when::new_clause(crate::when::Kind::Created, None, crate::when::window_of("today")).unwrap();
        let s = o.put_time(1, None, &t);
        assert_eq!(s, Slot { group: 1, item: 1 });
        assert_eq!(back(&o), "(kind build or (kind gap and created today))");
        assert_eq!(o.times_in(1, Cat::When).len(), 1);
        assert!(o.times_in(0, Cat::When).is_empty());
        let t2 = crate::when::negate(&t);
        assert_eq!(o.put_time(1, Some(s), &t2), s);
        assert_eq!(back(&o), "(kind build or (kind gap and not created today))");
    }
}
