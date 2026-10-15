//! ACCESS-9: a verified user's **reach** — the grants it holds (its own and
//! its teams'), each compiled once into a [`Held`] scope, and the session's
//! ceiling the same way, evaluated against the kept tree.
//!
//! Two questions, both answered by the store, never by filter text:
//!
//! - **does it cover this ticket** ([`Reach::covers`], a [`super::Permit`]'s
//!   question): the ticket's chain — itself, its ancestors, its shortcode,
//!   the shortcodes hosting it, its organization ([`Store::gate_chain`]) —
//!   holds a scope the user holds a role on, and every ceiling layer's;
//! - **what may a read answer from** ([`Reach::filter_for`], a read's
//!   lens): `under` each scope the call's grant is held on, as filter
//!   predicates built directly — the store's `under` is the kept tree's
//!   own lineage. (IDENTITY-21 formatted this as text and parsed it back;
//!   ACCESS-3 "filter text".)
//!
//! A higher role on a narrower scope never lowers a wider one: what is held
//! is the union of every grant.

use super::*;
use crate::address::{OrgId, ShortcodeId, TicketId};
use crate::filter::{Field, Group, Predicate};
use crate::members::Scope;

/// One scope a role is held on, compiled — the four kinds of ACCESS-37's
/// grammar and the organization ORG-4 added. A grant compiles to the first
/// three (the stored [`Scope`]; ACCESS-21 adds `under ID` and `ID` there);
/// a session's ceiling names tickets too (`editor@MENGER-12`, the ticket
/// and its subtree: [`Held::Under`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Held {
    /// `*`: every ticket.
    Everywhere,
    /// `acme:`: every ticket in the organization.
    Org(OrgId),
    /// `acme:MENGER`: the shortcode and every shortcode hosted under it.
    Shortcode { org: OrgId, id: ShortcodeId },
    /// `under ID`: the ticket and its descendants, wherever it moves.
    Under(TicketId),
    /// `ID`: the ticket itself, alone. No stored scope names one yet
    /// (ACCESS-21); a read's lens cannot narrow to it, so it adds nothing
    /// to one ([`Held::under`]) — a [`super::Permit`] reaches it.
    Ticket(TicketId),
    /// A scope minted before ORG-4 (a bare `IDENTITY`, in a ceiling or a
    /// grant the open has not rewritten yet): named as the gate names it,
    /// matched by its chain's bare identity.
    Named(String),
}

impl Held {
    /// A stored grant's scope.
    pub fn of_scope(scope: &Scope) -> Self {
        match scope {
            Scope::Everywhere => Self::Everywhere,
            Scope::Org(org) => Self::Org(org.clone()),
            Scope::Shortcode { org, id } => Self::Shortcode { org: org.clone(), id: id.clone() },
        }
    }

    /// A ceiling entry's scope, as the gate reads it: `*`; a kept scope
    /// (`acme:`, `acme:MENGER`); a ticket (`MENGER-12`, it and its
    /// subtree); else the name itself.
    fn of_ceiling(scope: &ScopeRef) -> Self {
        let ScopeRef::Named(text) = scope else { return Self::Everywhere };
        if let Some(s) = Scope::stored(text) {
            return Self::of_scope(&s);
        }
        let ticket = text.rsplit_once('-').is_some_and(|(code, n)| !code.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        match ticket {
            true => Self::Under(TicketId::new(text.trim().to_ascii_uppercase())),
            false => Self::Named(text.trim().to_string()),
        }
    }

    /// Whether it covers the ticket `key`, whose chain is `chain` (the
    /// store's [`Store::gate_chain`] of it: `org:ID` and the bare identity
    /// of each shortcode, the ticket's key and its ancestors', `org:`).
    fn covers(&self, key: &str, chain: &[String]) -> bool {
        let holds = |t: &str| chain.iter().any(|c| c == t);
        match self {
            Self::Everywhere => true,
            Self::Org(org) => holds(&Scope::Org(org.clone()).text()),
            Self::Shortcode { org, id } => holds(&Scope::Shortcode { org: org.clone(), id: id.clone() }.text()),
            Self::Under(t) => holds(t.as_str()),
            Self::Ticket(t) => t.as_str() == key,
            Self::Named(n) => holds(n),
        }
    }

    /// The name a read's `under` asks for it — `None` for `*` (no
    /// narrowing) and for one ticket alone (no `under` is it).
    fn under(&self) -> Option<String> {
        match self {
            Self::Everywhere | Self::Ticket(_) => None,
            Self::Org(org) => Some(Scope::Org(org.clone()).text()),
            Self::Shortcode { id, .. } => Some(id.as_str().to_string()),
            Self::Under(t) => Some(crate::address::folded(t.as_str())),
            Self::Named(n) => Some(under_name(n)),
        }
    }
}

/// One ceiling layer: its entries, each a role or a grant on a scope.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Layer {
    entries: Vec<(RoleOrGrant, Held)>,
    /// As the session keeps it — what a refusal names.
    text: Vec<String>,
}

/// Where a grant is held, compiled: the memberships' scopes, then each
/// ceiling layer's — every part must cover a ticket; an absent part (one
/// that holds `*`) narrows nothing. `None` from [`Reach::key_for`]:
/// everything. Compared, not formatted: what a watch holds to know its
/// reach moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReachKey {
    parts: Vec<Vec<Held>>,
}

/// What a verified user holds: the grants (direct ∪ its teams' ∪ the
/// operator's `--admin`), compiled; the session's ceiling; the grant the
/// call was admitted for; and the grants generation it was resolved at.
/// Built only by [`Access::verify`]; no `Clone` across calls, no
/// `Deserialize`, no `Default`.
pub struct Reach {
    grants: Vec<(Scope, Role)>,
    held: Vec<(Held, Role)>,
    ceiling: Vec<Layer>,
    call: Option<GateGrant>,
    /// `None`: login is off — everything, as menger always answered.
    gate: Option<Gate>,
    generation: u64,
    /// Who it is, for a refusal's words.
    who: String,
}

impl std::fmt::Debug for Reach {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Reach").field("grants", &self.grants).field("generation", &self.generation).finish()
    }
}

impl Reach {
    /// Login off: everything.
    pub(super) fn everything() -> Self {
        Self { grants: Vec::new(), held: Vec::new(), ceiling: Vec::new(), call: None, gate: None, generation: 0, who: "an anonymous caller".into() }
    }

    /// `who`'s reach as `ctx` stands: its grants as the book resolves them
    /// (direct ∪ teams' ∪ the operator's), each scope compiled; `ctx`'s
    /// ceiling layers the same way.
    pub(super) fn resolve(book: &Book, gate: &Gate, who: &str, ctx: &AuthContext) -> Result<Self, Refusal> {
        let generation = book.store().map(|s| s.grants_generation()).unwrap_or(0);
        let mut grants: Vec<(Scope, Role)> = Vec::new();
        let mut held: Vec<(Held, Role)> = Vec::new();
        for g in book.grants_of(who) {
            match Scope::stored(&g.scope) {
                Some(s) => {
                    held.push((Held::of_scope(&s), g.role));
                    grants.push((s, g.role));
                }
                // A scope in no kept form (one written before ORG-4, which
                // the store's open rewrites): matched by name, as the gate
                // matches it.
                None => held.push((Held::Named(g.scope.trim().to_string()), g.role)),
            }
        }
        let ceiling = GateCeiling::layers_of(ctx)
            .map_err(|e| Refusal::Ceiling(e.to_string()))?
            .into_iter()
            .map(|l| Layer { text: l.text(), entries: l.0.iter().map(|(rg, s)| (rg.clone(), Held::of_ceiling(s))).collect() })
            .collect();
        Ok(Self { grants, held, ceiling, call: None, gate: Some(gate.clone()), generation, who: who.to_string() })
    }

    /// The reach for the call admitted for `grant`.
    pub(super) fn for_call(mut self, grant: Option<GateGrant>) -> Self {
        self.call = grant;
        self
    }

    /// A reach of `grants` alone, for a test in another module: no ceiling,
    /// every role as tickets declares it.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn for_tests(who: &str, grants: Vec<(Held, Role)>, gate: Gate) -> Self {
        let scopes = grants
            .iter()
            .filter_map(|(h, r)| match h {
                Held::Everywhere => Some((Scope::Everywhere, *r)),
                Held::Org(o) => Some((Scope::Org(o.clone()), *r)),
                Held::Shortcode { org, id } => Some((Scope::Shortcode { org: org.clone(), id: id.clone() }, *r)),
                _ => None,
            })
            .collect();
        Self { grants: scopes, held: grants, ceiling: Vec::new(), call: None, gate: Some(gate), generation: 0, who: who.to_string() }
    }

    /// The grants it holds, as kept.
    pub fn grants(&self) -> &[(Scope, Role)] {
        &self.grants
    }

    /// The grants generation it was resolved at — a watch that finds the
    /// store's moved past it verifies again (ACCESS-12).
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The grant the call was admitted for (`None` with login off).
    pub fn call(&self) -> Option<&GateGrant> {
        self.call.as_ref()
    }

    /// The gate, with login on.
    pub(super) fn gate(&self) -> Option<&Gate> {
        self.gate.as_ref()
    }

    /// Whether `role` lists `grant` here.
    fn lists(&self, role: &str, grant: &GateGrant) -> bool {
        self.gate.as_ref().is_none_or(|g| g.lists(&RoleName::new(role), grant))
    }

    /// Where `grant` is held, compiled ([`ReachKey`]) — `None` everywhere;
    /// refused when it is held nowhere, or a ceiling layer allows it
    /// nowhere.
    pub fn key_for(&self, grant: &GateGrant) -> Result<Option<ReachKey>, Refusal> {
        if self.gate.is_none() {
            return Ok(None);
        }
        let mut parts = Vec::new();
        let held: Vec<Held> = self.held.iter().filter(|(_, r)| self.lists(r.as_str(), grant)).map(|(h, _)| h.clone()).collect();
        if held.is_empty() {
            return Err(Refusal::Nowhere { who: self.who.clone(), grant: grant.to_string() });
        }
        if !held.contains(&Held::Everywhere) {
            parts.push(held);
        }
        for layer in &self.ceiling {
            let allowed: Vec<Held> = layer
                .entries
                .iter()
                .filter(|(rg, _)| match rg {
                    RoleOrGrant::Role(r) => self.lists(r.as_str(), grant),
                    RoleOrGrant::Grant(g) => g == grant,
                })
                .map(|(_, h)| h.clone())
                .collect();
            if allowed.is_empty() {
                return Err(Refusal::OutsideCeiling { layer: layer.text.join(", "), grant: grant.to_string() });
            }
            if !allowed.contains(&Held::Everywhere) {
                parts.push(allowed);
            }
        }
        Ok((!parts.is_empty()).then_some(ReachKey { parts }))
    }

    /// The filter a read answers under for `grant`: each part `under` its
    /// scopes, or'd, the parts and'd — built as predicates, never as text.
    /// `None`: everywhere.
    pub fn filter_for(&self, grant: &GateGrant) -> Result<Option<Filter>, Refusal> {
        let Some(key) = self.key_for(grant)? else { return Ok(None) };
        let mut groups = Vec::with_capacity(key.parts.len());
        for part in &key.parts {
            let mut names: Vec<String> = part.iter().filter_map(Held::under).collect();
            names.dedup();
            if names.is_empty() {
                return Err(Refusal::Unreadable(format!("{} holds {grant} only on single tickets, which a read cannot narrow to yet", self.who)));
            }
            groups.push(Group::any(names.into_iter().map(|n| Predicate::is(Field::Under(n))).collect()));
        }
        Ok(Some(Filter { groups, display: Default::default() }))
    }

    /// Whether a role `R` admits covers the ticket `key` whose chain is
    /// `chain`: some grant of such a role on a scope in it, and in every
    /// ceiling layer an entry naming such a role (a ceiling's single grant,
    /// `tickets.get@X`, is no role) on a scope in it. Login off: always.
    pub fn covers<R: RoleMark>(&self, key: &str, chain: &[String]) -> bool {
        if self.gate.is_none() {
            return true;
        }
        let by_role = |r: &str| r.parse::<Role>().is_ok_and(R::admits);
        self.held.iter().any(|(h, r)| R::admits(*r) && h.covers(key, chain))
            && self.ceiling.iter().all(|l| {
                l.entries.iter().any(|(rg, h)| matches!(rg, RoleOrGrant::Role(r) if by_role(r.as_str())) && h.covers(key, chain))
            })
    }

    /// ORG-4: the organizations it holds a role in, sorted — `None`: every
    /// one (a role on `*`, or login off).
    pub fn orgs(&self) -> Option<Vec<OrgId>> {
        self.gate.as_ref()?;
        let mut out: Vec<OrgId> = Vec::new();
        for (s, _) in &self.grants {
            match s.org() {
                None => return None,
                Some(o) if !out.contains(o) => out.push(o.clone()),
                Some(_) => {}
            }
        }
        out.sort();
        Some(out)
    }
}
