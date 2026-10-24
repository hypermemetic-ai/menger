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
//! - FILTER-55: the menu's edits of these groups (a tick, `+ or`, a box's
//!   state) went with the menu; the builder (src/fcard.rs) edits the
//!   filter's tree itself.
//!
//! Everything here is pure and pinned by native tests below.

use menger_tickets_api::filter::{Conj, Field, Filter, Group, Predicate};

use crate::filter::Cat;
use crate::lens::token_of;
use crate::when::Timed;

/// The most groups a filter spells out into and is still drawn.
pub(crate) const MOST: usize = 32;

/// A filter as the panel draws it: groups joined by `or`, each a row of
/// clauses joined by `and`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Ors {
    pub(crate) groups: Vec<Vec<Predicate>>,
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

    #[cfg(test)]
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
pub(crate) mod tests {
    use super::*;

    fn f(text: &str) -> Filter {
        Filter::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
    }

    /// Tickets enough to tell two filters apart: every status, kind,
    /// severity, flag, label, shortcode and a few histories.
    pub(crate) fn grid() -> Vec<menger_tickets_api::filter::Facts> {
        use menger_tickets_api::{Kind, Severity, Status};
        let mut out = Vec::new();
        let statuses = [Status::Ready, Status::Coding, Status::Pending, Status::Complete, Status::InReview];
        let kinds = [Kind::Build, Kind::Gap, Kind::Spike, Kind::Doc];
        let sevs = [None, Some(Severity::High), Some(Severity::Low)];
        let hists: [&[&str]; 3] = [&[], &["changed today"], &["created today"]];
        let mut n = 0;
        for st in statuses {
            for k in kinds {
                for sev in sevs {
                    for flags in 0..8u8 {
                        for (hi, h) in hists.iter().enumerate() {
                            n += 1;
                            let epic = if n % 3 == 0 { "MNG" } else { "WAVE" };
                            out.push(menger_tickets_api::filter::Facts {
                                id: format!("{epic}-{n}"),
                                epic: epic.into(),
                                title: if n % 5 == 0 { "the menger envelope".into() } else { "a title".into() },
                                status: st,
                                kind: k,
                                severity: sev,
                                blocked: flags & 1 != 0,
                                has_commits: flags & 2 != 0,
                                menger_doc: false,
                                asking: flags & 4 != 0,
                                labels: if hi == 1 { vec!["needs-you".into()] } else { Vec::new() },
                                archived: false,
                                frozen: false,
                                lineage: vec![format!("{epic}-{n}"), epic.into(), "PLEXUS-1".into()],
                                history: h.iter().map(|s| s.to_string()).collect(),
                            });
                        }
                    }
                }
            }
        }
        out
    }

    /// Whether two filters admit the same tickets of [`grid`].
    pub(crate) fn same_meaning(a: &Filter, b: &Filter) -> bool {
        grid().iter().all(|t| a.matches(t) == b.matches(t))
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
        let six = "status in (Ready, Coding, Pending, Blocked, Draft, Epic)";
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
        o.groups.push(Vec::new());
        assert_eq!(back(&o), "kind build");
        assert_eq!(o.groups.len(), 2);
        // What the language refuses is refused in its words.
        let o = Ors { groups: vec![vec![Predicate::is(Field::Text("x".into()))], vec![Predicate::is(Field::Ready)]] };
        assert!(o.to_groups().unwrap_err().contains("text"));
    }
}
