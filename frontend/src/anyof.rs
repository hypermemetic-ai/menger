//! FILTER-31: any of — the filter bar's or-groups. The language already
//! holds the shape (FILTER-10: a filter is groups joined by `and`, each
//! group predicates joined by one `or` or one `and`; groups do not nest);
//! this is the bar building and drawing it.
//!
//! - **A group's members** ([`members`]): the pills it is made of — a
//!   category's clause (the values of one field under one operator, merged
//!   where the group's join is the clause's own: `status Ready or status
//!   Coding` is one member, Status is any of Ready, Coding), a time clause
//!   (FILTER-27's, one predicate), or what no category draws (`under X`,
//!   `category in (…)`), kept as its text. A group of two or more members is
//!   drawn as a box ([`crate::filter::Pill::Group`]); of one, as that
//!   member's pill, as before.
//! - **What fits** ([`Member::fits`]): a member sits in a group joined by
//!   `or` when it is one predicate or is itself an `or` (Status is any of …);
//!   in one joined by `and` when it is one predicate or itself an `and`
//!   (Kind is not doc, spike). Anything else would need a group inside a
//!   group, which the language refuses — so the edit is refused, with why,
//!   and nothing changes.
//! - **The edits** — each a pure function over the filter's groups: a
//!   category's clause set inside the group at `at` ([`set_cat`]: Or… on a
//!   pill, the group's `+ or`, a member's values), a time clause put there
//!   ([`put_time`]), a member removed or its operator turned
//!   ([`remove_member`], [`flip_member`]), the join flipped ([`flip`]), the
//!   group taken apart ([`ungroup`]), top-level pills made one any-of group
//!   ([`group_any`]). A group left with one member dissolves into it; with
//!   none, it goes.
//!
//! Where a pill's menu items live: `filter.rs`'s `pill_menu_view` (the ⋯
//! on a pill or a group) calls the edits here through `Lens::edit_groups`;
//! what an item offers, or why it is off, is [`or_refusal`],
//! [`flip_refusal`] and (FILTER-30) [`negate_refusal`] here; Negate turns a
//! time clause around, alone or as a member of a box ([`negate`]).

use menger_tickets_api::filter::{Conj, Field, Group, Predicate};

use crate::filter::{op_label, Cat, Clause, Op};
use crate::lens::{group_of, token_of};
use crate::when::{self, Timed};

/// One pill of a group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Member {
    /// A category's clause.
    Clause { cat: Cat, clause: Clause },
    /// A time clause (FILTER-27).
    Time(Timed),
    /// Predicates no category draws — one, or one field's values listed —
    /// as their own group: shown as its text.
    Custom(Group),
}

/// Where a time clause is: the group at `at`, or — `member` — one of that
/// group's members.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Slot {
    pub(crate) at: usize,
    pub(crate) member: Option<usize>,
}

/// A join as the bar says it.
pub(crate) fn conj_words(c: Conj) -> &'static str {
    match c {
        Conj::Any => "any of",
        Conj::All => "all of",
    }
}

/// The word between a group's members.
pub(crate) fn joiner(c: Conj) -> &'static str {
    match c {
        Conj::Any => "or",
        Conj::All => "and",
    }
}

fn other(c: Conj) -> Conj {
    match c {
        Conj::Any => Conj::All,
        Conj::All => Conj::Any,
    }
}

impl Member {
    /// The member alone, as a top-level group — what it is outside a group.
    pub(crate) fn group(&self) -> Option<Group> {
        match self {
            Member::Clause { cat, clause } => group_of(*cat, clause),
            Member::Time(t) => Some(when::group(t)),
            Member::Custom(g) => Some(g.clone()),
        }
    }

    /// Whether it can be one of a group joined by `conj`: one predicate,
    /// or itself joined so.
    pub(crate) fn fits(&self, conj: Conj) -> bool {
        self.group().is_some_and(|g| g.preds.len() <= 1 || g.conj == conj)
    }

    /// The category drawing it (When for a time clause; none for custom).
    pub(crate) fn cat(&self) -> Option<Cat> {
        match self {
            Member::Clause { cat, .. } => Some(*cat),
            Member::Time(t) => Some(crate::filter::time_cat(t)),
            Member::Custom(_) => None,
        }
    }

    /// The member in words: `Status is any of Ready, Coding`, "created
    /// today", a custom group's text.
    pub(crate) fn words(&self) -> String {
        match self {
            Member::Clause { cat, clause } => {
                let values = clause.values.iter().map(|v| cat.value_label(v)).collect::<Vec<_>>().join(", ");
                format!("{} {} {values}", cat.label(), op_label(*cat, clause.op, clause.values.len()))
            }
            Member::Time(t) => when::words(t, crate::calendar::reader_h12()),
            Member::Custom(g) => g.format(),
        }
    }
}

/// The member a predicate is, alone.
fn member_of(p: &Predicate) -> Member {
    match (&p.field, token_of(&p.field)) {
        (Field::History(h), _) => Member::Time(Timed { not: p.not, h: h.clone() }),
        (_, Some((cat, None))) => {
            Member::Clause { cat, clause: Clause { op: Op::Is, values: vec![if p.not { "no" } else { "yes" }.to_string()] } }
        }
        (_, Some((cat, Some(v)))) => match Clause::new(cat, if p.not { Op::Not } else { Op::Is }, [v]) {
            Some(clause) => Member::Clause { cat, clause },
            None => Member::Custom(Group::one(p.clone())),
        },
        (_, None) => Member::Custom(Group::one(p.clone())),
    }
}

/// The join under which a predicate's values list: `or` for inclusions,
/// `and` for exclusions.
fn listing(not: bool) -> Conj {
    if not {
        Conj::All
    } else {
        Conj::Any
    }
}

/// `p` taken into `m` when they are one field's values under one operator
/// and the group's join is theirs — whether it was.
fn merge(m: &mut Member, p: &Predicate, conj: Conj) -> bool {
    if listing(p.not) != conj {
        return false;
    }
    match (m, member_of(p)) {
        (Member::Clause { cat, clause }, Member::Clause { cat: c, clause: one })
            if *cat == c && !c.is_flag() && clause.op == one.op =>
        {
            match Clause::new(c, clause.op, clause.values.iter().cloned().chain(one.values)) {
                Some(cl) => {
                    *clause = cl;
                    true
                }
                None => false,
            }
        }
        (Member::Custom(g), Member::Custom(_))
            if matches!(p.field, Field::Category(_) | Field::Under(_))
                && g.preds.iter().all(|q| q.field.name() == p.field.name() && q.not == p.not) =>
        {
            let mut preds = g.preds.clone();
            preds.push(p.clone());
            *g = if p.not { Group::all(preds) } else { Group::any(preds) };
            true
        }
        _ => false,
    }
}

/// The group's members, in the order they first appear.
pub(crate) fn members(g: &Group) -> Vec<Member> {
    let conj = if g.preds.len() > 1 { g.conj } else { listing(g.preds.first().is_some_and(|p| p.not)) };
    let mut out: Vec<Member> = Vec::new();
    for p in &g.preds {
        if !out.iter_mut().any(|m| merge(m, p, conj)) {
            out.push(member_of(p));
        }
    }
    out
}

/// The group the members make, joined by `conj` — one member is its own
/// group; none, no group. Refused when a member does not fit the join.
pub(crate) fn build(conj: Conj, ms: &[Member]) -> Result<Option<Group>, String> {
    match ms {
        [] => Ok(None),
        [m] => Ok(m.group()),
        ms => {
            if let Some(m) = ms.iter().find(|m| !m.fits(conj)) {
                return Err(misfit(m, conj));
            }
            let preds = ms.iter().filter_map(Member::group).flat_map(|g| g.preds).collect();
            Ok(Some(Group { conj, preds }))
        }
    }
}

/// Why `m` cannot be one of a group joined by `conj`.
fn misfit(m: &Member, conj: Conj) -> String {
    format!(
        "\u{201c}{}\u{201d} is itself {} its values, so it can't sit in {} group — groups do not nest; keep one value, or leave it outside the group",
        m.words(),
        conj_words(other(conj)),
        if conj == Conj::Any { "an any-of" } else { "an all-of" },
    )
}

/// What the menu says while it adds to `g` (Or…, `+ or`): what a pick
/// joins.
pub(crate) fn into_words(g: &Group) -> String {
    let ms = members(g);
    match ms.as_slice() {
        [m] => format!("Or \u{2014} what you pick joins \u{201c}{}\u{201d}", m.words()),
        ms => {
            let words = ms.iter().map(Member::words).collect::<Vec<_>>().join(&format!(" {} ", joiner(g.conj)));
            format!("Adding to {}: {words}", conj_words(g.conj))
        }
    }
}

/// The members of the group at `at`, and the join an edit keeps: its own
/// with two or more members, else any of (a lone pill becoming a group).
fn parts(groups: &[Group], at: usize) -> Option<(Conj, Vec<Member>)> {
    let g = groups.get(at)?;
    let ms = members(g);
    Some((if ms.len() > 1 { g.conj } else { Conj::Any }, ms))
}

/// The group at `at` rewritten as `ms` under `conj` (gone, with none).
fn write(groups: &mut Vec<Group>, at: usize, conj: Conj, ms: &[Member]) -> Result<(), String> {
    match build(conj, ms)? {
        Some(g) => groups[at] = g,
        None => {
            groups.remove(at);
        }
    }
    Ok(())
}

/// Whether the group at `at` is a box — two members or more.
pub(crate) fn is_group(g: &Group) -> bool {
    members(g).len() > 1
}

/// `c`'s clause among the members of the group at `at` (its first).
pub(crate) fn clause_in(groups: &[Group], at: usize, c: Cat) -> Option<Clause> {
    groups.get(at).and_then(|g| {
        members(g).into_iter().find_map(|m| match m {
            Member::Clause { cat, clause } if cat == c => Some(clause),
            _ => None,
        })
    })
}

/// `c`'s clause inside the group at `at` set: its member replaced, added as
/// one more (a lone pill becomes an any-of group), or — none — taken out.
pub(crate) fn set_cat(groups: &mut Vec<Group>, at: usize, c: Cat, cl: Option<Clause>) -> Result<(), String> {
    let Some((conj, mut ms)) = parts(groups, at) else { return Err("that group is gone".into()) };
    let i = ms.iter().position(|m| matches!(m, Member::Clause { cat, .. } if *cat == c));
    match (i, cl) {
        (Some(i), Some(clause)) => ms[i] = Member::Clause { cat: c, clause },
        (Some(i), None) => {
            ms.remove(i);
        }
        (None, Some(clause)) => ms.push(Member::Clause { cat: c, clause }),
        (None, None) => return Ok(()),
    }
    write(groups, at, conj, &ms)
}

/// The time clause at `slot`.
pub(crate) fn time_in(groups: &[Group], slot: Slot) -> Option<Timed> {
    let g = groups.get(slot.at)?;
    match slot.member {
        None => when::of_group(g),
        Some(i) => match members(g).into_iter().nth(i)? {
            Member::Time(t) => Some(t),
            _ => None,
        },
    }
}

/// `t` written at `slot`, or — no slot — added: as a member of the group
/// at `into` (a lone pill becoming an any-of group), else as a group of its
/// own, last. Where it now is.
pub(crate) fn put_time(groups: &mut Vec<Group>, into: Option<usize>, slot: Option<Slot>, t: &Timed) -> Result<Slot, String> {
    let at = slot.map(|s| s.at).or(into);
    let member = match slot {
        Some(s) => s.member,
        None => into.map(|_| usize::MAX),
    };
    match (at.filter(|a| *a < groups.len()), member) {
        (Some(at), None) => {
            groups[at] = when::group(t);
            Ok(Slot { at, member: None })
        }
        (Some(at), Some(i)) => {
            let (conj, mut ms) = parts(groups, at).ok_or("that group is gone")?;
            let i = if i < ms.len() {
                ms[i] = Member::Time(t.clone());
                i
            } else {
                ms.push(Member::Time(t.clone()));
                ms.len() - 1
            };
            write(groups, at, conj, &ms)?;
            // One member is no box: the clause is the group itself.
            Ok(Slot { at, member: (ms.len() > 1).then_some(i) })
        }
        (None, _) => {
            groups.push(when::group(t));
            Ok(Slot { at: groups.len() - 1, member: None })
        }
    }
}

/// The member `i` of the group at `at` taken out — the group dissolving
/// into the one left.
pub(crate) fn remove_member(groups: &mut Vec<Group>, at: usize, i: usize) {
    if let Some((conj, mut ms)) = parts(groups, at) {
        if i < ms.len() {
            ms.remove(i);
            // One member left is its own group; it always fits.
            let _ = write(groups, at, conj, &ms);
        }
    }
}

/// Member `i`'s operator turned (a flag's value), as a pill's toggle turns
/// it — refused when it would no longer fit the group's join.
pub(crate) fn flip_member(groups: &mut Vec<Group>, at: usize, i: usize) -> Result<(), String> {
    let Some((conj, mut ms)) = parts(groups, at) else { return Ok(()) };
    let Some(Member::Clause { cat, clause }) = ms.get(i).cloned() else { return Ok(()) };
    // A flag's `is not` is its other value (Clause::new turns it).
    let op = if cat.is_flag() || clause.op == Op::Is { Op::Not } else { Op::Is };
    if let Some(clause) = Clause::new(cat, op, clause.values) {
        ms[i] = Member::Clause { cat, clause };
    }
    write(groups, at, conj, &ms)
}

/// Why the group at `at` cannot flip its join (`None`: it can).
pub(crate) fn flip_refusal(g: &Group) -> Option<String> {
    let to = other(g.conj);
    members(g).iter().find(|m| !m.fits(to)).map(|m| misfit(m, to))
}

/// The group's join flipped: any of ⇄ all of.
pub(crate) fn flip(groups: &mut [Group], at: usize) -> Result<(), String> {
    let Some(g) = groups.get(at) else { return Ok(()) };
    if let Some(why) = flip_refusal(g) {
        return Err(why);
    }
    let ms = members(g);
    if ms.len() > 1 {
        if let Some(next) = build(other(g.conj), &ms)? {
            groups[at] = next;
        }
    }
    Ok(())
}

/// The group at `at` taken apart: each member a top-level pill, in its
/// place, joined by `and`.
pub(crate) fn ungroup(groups: &mut Vec<Group>, at: usize) {
    let Some(g) = groups.get(at) else { return };
    let parts: Vec<Group> = members(g).iter().filter_map(Member::group).collect();
    groups.splice(at..=at, parts);
}

/// FILTER-30: why the clause at `slot` cannot be negated from its menu
/// (`None`: it can) — only a time clause turns around there; a category's
/// pill turns with its operator.
pub(crate) fn negate_refusal(groups: &[Group], slot: Slot) -> Option<String> {
    time_in(groups, slot).is_none().then(|| "only a time clause is negated here — a category's pill turns with its operator".into())
}

/// FILTER-30: the time clauses Negate offers on the pill or group `g` at
/// `at`: the pill itself, or each time member of a box — where each is,
/// with its words.
pub(crate) fn negatable(g: &Group, at: usize) -> Vec<(Slot, Timed)> {
    if let Some(t) = when::of_group(g) {
        return vec![(Slot { at, member: None }, t)];
    }
    if !is_group(g) {
        return Vec::new();
    }
    members(g)
        .into_iter()
        .enumerate()
        .filter_map(|(i, m)| match m {
            Member::Time(t) => Some((Slot { at, member: Some(i) }, t)),
            _ => None,
        })
        .collect()
}

/// FILTER-30: the time clause at `slot` turned around (`created today` ⇄
/// `not created today`), where it is — a member of a box too: one
/// predicate either way, it fits any join.
pub(crate) fn negate(groups: &mut Vec<Group>, slot: Slot) -> Result<(), String> {
    if let Some(why) = negate_refusal(groups, slot) {
        return Err(why);
    }
    let t = time_in(groups, slot).ok_or("that clause is gone")?;
    put_time(groups, None, Some(slot), &when::negate(&t)).map(|_| ())
}

/// Why the pill or group `g` cannot take an alternative (`None`: it can) —
/// Or… on its menu.
pub(crate) fn or_refusal(g: &Group) -> Option<String> {
    let ms = members(g);
    match ms.as_slice() {
        [m] if !m.fits(Conj::Any) => Some(misfit(m, Conj::Any)),
        ms if ms.len() > 1 && g.conj == Conj::All => Some("an all-of group takes no alternative — flip it to any of first".into()),
        _ => None,
    }
}

/// The groups at `ats` made one any-of group, where the first of them was;
/// where it now is. Refused (nothing changed) when fewer than two are
/// named, or one cannot be an alternative.
pub(crate) fn group_any(groups: &mut Vec<Group>, ats: &[usize]) -> Result<usize, String> {
    let mut ats: Vec<usize> = ats.iter().copied().filter(|a| *a < groups.len()).collect();
    ats.sort_unstable();
    ats.dedup();
    if ats.len() < 2 {
        return Err("pick two pills or more to group".into());
    }
    let mut ms = Vec::new();
    for &a in &ats {
        if let Some(why) = or_refusal(&groups[a]) {
            return Err(why);
        }
        ms.extend(members(&groups[a]));
    }
    // Re-read as one group, so one field's values merge.
    let Some(built) = build(Conj::Any, &ms)? else { return Err("nothing to group".into()) };
    let ms = members(&built);
    let g = build(if ms.len() > 1 { Conj::Any } else { built.conj }, &ms)?.unwrap_or(built);
    let first = ats[0];
    for &a in ats.iter().rev() {
        groups.remove(a);
    }
    groups.insert(first, g);
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use menger_tickets_api::filter::Filter;

    fn groups(text: &str) -> Vec<Group> {
        Filter::parse(text).unwrap_or_else(|e| panic!("{text}: {e}")).groups
    }

    fn text(gs: &[Group]) -> String {
        Filter { groups: gs.to_vec(), ..Filter::default() }.format()
    }

    fn cl(op: Op, v: &[&str]) -> Clause {
        Clause { op, values: v.iter().map(|s| s.to_string()).collect() }
    }

    #[test]
    fn a_group_is_its_members() {
        let g = &groups("(changed today or created today)")[0];
        let ms = members(g);
        assert_eq!(ms.len(), 2);
        assert!(matches!(&ms[0], Member::Time(_)) && matches!(&ms[1], Member::Time(_)));
        // One field's values under the group's join are one member.
        let ms = members(&groups("(status Ready or status Coding or kind gap)")[0]);
        assert_eq!(
            ms,
            vec![
                Member::Clause { cat: Cat::Status, clause: cl(Op::Is, &["Coding", "Ready"]) },
                Member::Clause { cat: Cat::Kind, clause: cl(Op::Is, &["gap"]) },
            ]
        );
        // An exclusion's values list under `and`, not under `or`.
        let ms = members(&groups("(not kind doc and not kind spike and status Ready)")[0]);
        assert_eq!(ms[0], Member::Clause { cat: Cat::Kind, clause: cl(Op::Not, &["spike", "doc"]) });
        let ms = members(&groups("(not status Ready or not status Coding)")[0]);
        assert_eq!(ms.len(), 2);
        // Flags, labels, what no category draws.
        let ms = members(&groups("(blocked or not ready or label needs-you or under PLEXUS-1 or category ready or category active)")[0]);
        assert_eq!(ms.len(), 5, "{ms:?}");
        assert_eq!(ms[0], Member::Clause { cat: Cat::Blocked, clause: cl(Op::Is, &["yes"]) });
        assert_eq!(ms[1], Member::Clause { cat: Cat::Ready, clause: cl(Op::Is, &["no"]) });
        assert_eq!(ms[2], Member::Clause { cat: Cat::Label, clause: cl(Op::Is, &["needs-you"]) });
        assert_eq!(ms[4], Member::Custom(groups("category in (ready, active)")[0].clone()));
        // A single pill is one member.
        assert_eq!(members(&groups("not kind in (doc, spike)")[0]).len(), 1);
        assert_eq!(members(&groups("status in (Ready, Coding)")[0]).len(), 1);
    }

    #[test]
    fn every_shape_the_language_holds_round_trips() {
        for form in [
            "(changed today or created today)",
            "(changed today or created today) and kind build",
            "(status Ready or kind gap)",
            "(status Ready and kind gap)",
            "(blocked or status Blocked)",
            "(not status Ready or not kind gap)",
            "(status Ready or status Coding or created within 3d)",
            "(not kind doc and not kind spike and not blocked)",
            "(label needs-you or asking)",
            "(entered Ready today by owner or status Pending for over 2w)",
            "(not created today or has commits)",
            "(in MNG or in WAVE or under PLEXUS-1)",
            "(severity high or severity critical or ready)",
        ] {
            let gs = groups(form);
            for g in &gs {
                let ms = members(g);
                let conj = if ms.len() > 1 { g.conj } else { Conj::All };
                let back = build(conj, &ms).unwrap().unwrap();
                // The same text, and the same members read back.
                assert_eq!(Filter::parse(&text(std::slice::from_ref(&back))).unwrap().groups[0], back, "{form}");
                assert_eq!(members(&back), ms, "{form}");
            }
        }
    }

    #[test]
    fn or_on_a_pill_makes_an_any_of_group() {
        // changed today, Or… → When › Created › today.
        let mut gs = groups("changed today and kind build");
        let t = when::of_group(&groups("created today")[0]).unwrap();
        let at = put_time(&mut gs, Some(0), None, &t).unwrap();
        assert_eq!(at, Slot { at: 0, member: Some(1) });
        assert_eq!(text(&gs), "(changed today or created today) and kind build");
        // + or, Status Ready; then Coding joins that member.
        set_cat(&mut gs, 0, Cat::Status, Some(cl(Op::Is, &["Ready"]))).unwrap();
        assert_eq!(text(&gs), "(changed today or created today or status Ready) and kind build");
        set_cat(&mut gs, 0, Cat::Status, Clause::new(Cat::Status, Op::Is, ["Ready".into(), "Coding".into()])).unwrap();
        assert_eq!(text(&gs), "(changed today or created today or status Coding or status Ready) and kind build");
        assert_eq!(clause_in(&gs, 0, Cat::Status), Some(cl(Op::Is, &["Coding", "Ready"])));
        // The time clause inside is edited where it is.
        let y = when::of_group(&groups("created yesterday")[0]).unwrap();
        assert_eq!(time_in(&gs, Slot { at: 0, member: Some(1) }), Some(t.clone()));
        put_time(&mut gs, None, Some(Slot { at: 0, member: Some(1) }), &y).unwrap();
        assert_eq!(text(&gs), "(changed today or created yesterday or status Coding or status Ready) and kind build");
        // A lone category pill, Or… → Kind gap.
        let mut gs = groups("status Ready");
        set_cat(&mut gs, 0, Cat::Kind, Some(cl(Op::Is, &["gap"]))).unwrap();
        assert_eq!(text(&gs), "(status Ready or kind gap)");
        // Its own category again: the values join it (the same `or`).
        let mut gs = groups("status Ready");
        set_cat(&mut gs, 0, Cat::Status, Clause::new(Cat::Status, Op::Is, ["Ready".into(), "Coding".into()])).unwrap();
        assert_eq!(text(&gs), "status in (Coding, Ready)");
    }

    #[test]
    fn what_cannot_be_an_alternative_is_refused() {
        let mut gs = groups("not kind in (doc, spike)");
        assert!(or_refusal(&gs[0]).is_some());
        let before = gs.clone();
        assert!(set_cat(&mut gs, 0, Cat::Status, Some(cl(Op::Is, &["Ready"]))).is_err());
        assert_eq!(gs, before);
        // A member turned to `is not` of two values in an any of.
        let mut gs = groups("(status Ready or status Coding or kind gap)");
        assert!(gs.len() == 1 && is_group(&gs[0]));
        assert!(flip_member(&mut gs, 0, 0).is_err());
        assert!(flip_member(&mut gs, 0, 1).is_ok());
        assert_eq!(text(&gs), "(status Coding or status Ready or not kind gap)");
        // An all of takes no alternative.
        assert!(or_refusal(&groups("(status Ready and kind gap)")[0]).is_some());
        assert!(or_refusal(&groups("status Ready")[0]).is_none());
        assert!(or_refusal(&groups("changed today")[0]).is_none());
    }

    #[test]
    fn the_joiner_flips_and_a_group_comes_apart() {
        let mut gs = groups("(status Ready or kind gap) and in MNG");
        flip(&mut gs, 0).unwrap();
        assert_eq!(text(&gs), "(status Ready and kind gap) and in MNG");
        flip(&mut gs, 0).unwrap();
        assert_eq!(text(&gs), "(status Ready or kind gap) and in MNG");
        // Two values of one field can't flip to all of.
        let mut gs = groups("(status Ready or status Coding or kind gap)");
        assert!(flip_refusal(&gs[0]).is_some());
        assert!(flip(&mut gs, 0).is_err());
        // Ungroup: each member a top-level pill, in its place.
        let mut gs = groups("kind build and (changed today or created today or status Ready) and in MNG");
        ungroup(&mut gs, 1);
        assert_eq!(text(&gs), "kind build and changed today and created today and status Ready and in MNG");
    }

    #[test]
    fn the_last_member_but_one_leaving_dissolves_the_group() {
        let mut gs = groups("(changed today or created today) and kind build");
        remove_member(&mut gs, 0, 0);
        assert_eq!(text(&gs), "created today and kind build");
        let mut gs = groups("(status Ready or kind gap)");
        set_cat(&mut gs, 0, Cat::Kind, None).unwrap();
        assert_eq!(text(&gs), "status Ready");
        set_cat(&mut gs, 0, Cat::Status, None).unwrap();
        assert!(gs.is_empty());
    }

    #[test]
    fn picked_pills_group_as_any_of() {
        let mut gs = groups("changed today and kind build and created today");
        assert_eq!(group_any(&mut gs, &[2, 0]).unwrap(), 0);
        assert_eq!(text(&gs), "(changed today or created today) and kind build");
        // A group picked with a pill: its members join.
        let mut gs = groups("(changed today or created today) and status Ready and label needs-you");
        group_any(&mut gs, &[0, 2]).unwrap();
        assert_eq!(text(&gs), "(changed today or created today or label needs-you) and status Ready");
        // One field's values merge.
        let mut gs = groups("status Ready and not status Coding and status Pending");
        group_any(&mut gs, &[0, 2]).unwrap();
        assert_eq!(text(&gs), "status in (Ready, Pending) and not status Coding");
        // Refused: one pill; a pill that is itself all of.
        let mut gs = groups("status Ready and not kind in (doc, spike)");
        assert!(group_any(&mut gs, &[0]).is_err());
        assert!(group_any(&mut gs, &[0, 1]).is_err());
        assert_eq!(text(&gs), "status Ready and not kind in (doc, spike)");
    }

    /// FILTER-30: Negate turns a time pill around — alone, or a member of an
    /// any-of group, where it is — and back; a category's pill is refused,
    /// nothing changed.
    #[test]
    fn negate_turns_a_time_clause_around_wherever_it_is() {
        let mut gs = groups("kind build and created today and (touched within 2w or status Ready)");
        assert_eq!(negatable(&gs[0], 0), []);
        assert_eq!(negatable(&gs[1], 1).len(), 1);
        let inside = negatable(&gs[2], 2);
        assert_eq!(inside.iter().map(|(s, _)| *s).collect::<Vec<_>>(), [Slot { at: 2, member: Some(0) }]);
        negate(&mut gs, Slot { at: 1, member: None }).unwrap();
        assert_eq!(text(&gs), "kind build and not created today and (touched within 2w or status Ready)");
        negate(&mut gs, inside[0].0).unwrap();
        assert_eq!(text(&gs), "kind build and not created today and (not touched within 2w or status Ready)");
        negate(&mut gs, inside[0].0).unwrap();
        negate(&mut gs, Slot { at: 1, member: None }).unwrap();
        assert_eq!(text(&gs), "kind build and created today and (touched within 2w or status Ready)", "and back");
        let before = gs.clone();
        assert!(negate_refusal(&gs, Slot { at: 0, member: None }).is_some());
        assert!(negate(&mut gs, Slot { at: 0, member: None }).is_err());
        assert!(negate(&mut gs, Slot { at: 2, member: Some(1) }).is_err(), "a category member");
        assert_eq!(gs, before, "nothing changed");
        // In an all-of group too.
        let mut gs = groups("(changed today and not touched within 1w)");
        negate(&mut gs, Slot { at: 0, member: Some(1) }).unwrap();
        assert_eq!(text(&gs), "(changed today and touched within 1w)");
    }
}
