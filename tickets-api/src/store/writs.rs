//! ACCESS-11: **what a write takes** — every public write of the store takes
//! the proof that its caller may make it, and the proof carries what it is
//! for: a [`Permit`] its ticket's id (the write changes that ticket, and no
//! other — *the ticket checked is the ticket written*), an [`AdminOf`] or an
//! [`On`] its scope, the [`System`] nothing (it acts for no user). The
//! writes that name more than one ticket — a birth's parent and blockers, a
//! move's three ends, an update's parent and blockers, a comment's ticket —
//! take the bundles here, each built from those proofs or by the system;
//! the store checks, under its write-serial lock, that what the write is
//! given names exactly what the proofs are for (a stale or a mismatched one
//! refuses the write, writing nothing).
//!
//! The proofs are minted by the user ([`crate::access::VerifiedUser::permit`],
//! `admin_of`, `on`, `forward`) or by the system (`System::permit`, …) — no
//! one else:
//!
//! ```compile_fail,E0308
//! // A write without a permit: an id is not one.
//! async fn write(store: &menger_tickets_api::store::Store) {
//!     let _ = store.set_status("MENGER-1", menger_tickets_api::store::Status::Ready).await;
//! }
//! ```
//!
//! ```compile_fail,E0451
//! // A permit forged for another ticket's id: only `permit()` makes one,
//! // for the id it checked.
//! use menger_tickets_api::access::{Editor, Permit};
//! use menger_tickets_api::address::TicketId;
//! let forged: Permit<Editor> = Permit { id: TicketId::new("SECRET-1"), _role: std::marker::PhantomData };
//! ```
//!
//! ```compile_fail,E0308
//! // A viewer's permit where a write needs an editor's.
//! async fn write(store: &menger_tickets_api::store::Store, p: menger_tickets_api::access::Permit<menger_tickets_api::access::Viewer>) {
//!     let _ = store.set_status(p, menger_tickets_api::store::Status::Ready).await;
//! }
//! ```

use super::*;
use crate::access::{AdminOf, Editor, Forward, On, Permit, RoleMark, Viewer};
use crate::members::Scope;

/// A name a write records as a blocker: a ticket the caller can view, or —
/// for one who would see it if it were one — a name not born yet.
#[derive(Debug)]
pub enum Blocker {
    /// A ticket the caller may view: the write records its id.
    Seen(Permit<Viewer>),
    /// A forward reference.
    Forward(Forward),
}

impl Blocker {
    /// The name it is for, as the proof holds it.
    pub fn name(&self) -> &str {
        match self {
            Self::Seen(p) => p.id().as_str(),
            Self::Forward(f) => f.name(),
        }
    }
}

/// Where a birth is filed, as its caller may file it: under a ticket it may
/// change (the parent named, else the shortcode's root), or — a shortcode
/// with neither — as a new shortcode, by an editor on `*` or on the
/// organization it is in.
#[derive(Debug)]
pub enum Under {
    Parent(Permit<Editor>),
    Founding(On<Editor>),
    /// A parent named that is no ticket — by one who sees every ticket:
    /// the store refuses it as not found ([`StoreError::ParentNotFound`]).
    Named(Forward),
}

/// What [`Store::create`] takes: the system's word, or the caller's proofs
/// — where it is filed, and every blocker it names.
#[derive(Debug)]
pub struct Filing(Option<(Under, Vec<Blocker>)>);

impl Filing {
    /// A user's: `under`, and one [`Blocker`] per `blocked_by` entry.
    pub fn new(under: Under, blockers: Vec<Blocker>) -> Self {
        Self(Some((under, blockers)))
    }
}

impl From<&System> for Filing {
    fn from(_: &System) -> Self {
        Self(None)
    }
}

/// The admin a shortcode's root takes to move it, and so re-host the
/// shortcode (whoever administers a host administers what hangs there):
/// admin on the host it leaves (on itself, when it hangs nowhere) and on
/// the host it goes to (ACCESS-3 #2).
#[derive(Debug)]
pub struct Rehost {
    pub leaving: AdminOf,
    pub joining: AdminOf,
}

/// A new parent, as its caller may give it: editor on the parent it has
/// now (`None` when it has none) and on the one it goes to — and, for a
/// shortcode's root, [`Rehost`].
#[derive(Debug)]
pub struct Parenting {
    from: Option<Permit<Editor>>,
    to: Permit<Editor>,
    rehost: Option<Rehost>,
}

impl Parenting {
    pub fn new(from: Option<Permit<Editor>>, to: Permit<Editor>, rehost: Option<Rehost>) -> Self {
        Self { from, to, rehost }
    }

    /// The parent it goes to.
    pub fn to(&self) -> &str {
        self.to.id().as_str()
    }
}

/// What [`Store::set_parent`] takes: the child and its [`Parenting`] — or
/// the system's move, whose parent now is not asked.
#[derive(Debug)]
pub struct Move {
    child: Permit<Editor>,
    by: MoveBy,
}

#[derive(Debug)]
enum MoveBy {
    System(Permit<Editor>),
    User(Parenting),
}

impl Move {
    pub fn new(child: Permit<Editor>, parenting: Parenting) -> Self {
        Self { child, by: MoveBy::User(parenting) }
    }
}

/// What [`Store::update`] takes: the ticket, and — for what it names
/// beyond itself — a [`Parenting`] for a new parent and a [`Blocker`] per
/// id it blocks or unblocks; or the system's, which names what it likes.
#[derive(Debug)]
pub struct Updating {
    ticket: Permit<Editor>,
    by: Option<(Option<Parenting>, Vec<Blocker>)>,
}

impl Updating {
    pub fn new(ticket: Permit<Editor>, parent: Option<Parenting>, blockers: Vec<Blocker>) -> Self {
        Self { ticket, by: Some((parent, blockers)) }
    }

    /// The ticket it is for.
    pub fn id(&self) -> &str {
        self.ticket.id().as_str()
    }
}

/// What a comment write takes: the system's word, a permit on the ticket
/// its anchor names (viewer to add; editor to resolve or reopen — and on
/// the ticket whose page its route is, when the caller names that too), or,
/// for a comment on the app (no ticket), nothing more than the call's own
/// grant.
#[derive(Debug)]
pub struct OnComment<R: RoleMark>(CommentBy<R>);

#[derive(Debug)]
enum CommentBy<R: RoleMark> {
    System,
    Tickets(Vec<Permit<R>>),
}

impl<R: RoleMark> OnComment<R> {
    /// A user's: the permits for the tickets the comment names (none: on
    /// the app).
    pub fn new(tickets: Vec<Permit<R>>) -> Self {
        Self(CommentBy::Tickets(tickets))
    }
}

impl<R: RoleMark> From<&System> for OnComment<R> {
    fn from(_: &System) -> Self {
        Self(CommentBy::System)
    }
}

/// What a write on a scope takes — a grant or a revoke, a role's edit, an
/// organization's making, a theme, a deployment: the system's word, or a
/// user's [`AdminOf`] or [`On`] that scope.
#[derive(Debug)]
pub struct Authority(AuthorityBy);

#[derive(Debug)]
enum AuthorityBy {
    System,
    Admin(AdminOf),
    Editor(On<Editor>),
    Deployer(On<crate::access::Deployer>),
}

impl From<&System> for Authority {
    fn from(_: &System) -> Self {
        Self(AuthorityBy::System)
    }
}

impl From<AdminOf> for Authority {
    fn from(a: AdminOf) -> Self {
        Self(AuthorityBy::Admin(a))
    }
}

impl From<On<Editor>> for Authority {
    fn from(a: On<Editor>) -> Self {
        Self(AuthorityBy::Editor(a))
    }
}

impl From<On<crate::access::Deployer>> for Authority {
    fn from(a: On<crate::access::Deployer>) -> Self {
        Self(AuthorityBy::Deployer(a))
    }
}

/// A proof given for one thing and a write asked of another: a bug in the
/// caller, said in words — never a ticket's existence (the caller minted
/// the proof from what it asked).
fn mismatch(what: &str) -> StoreError {
    StoreError::Invalid(format!("{what}: the write was not given the proof it needs (nothing written)"))
}

/// The key `text` names in `cache`, as a write resolves it (`None`: none).
pub(super) fn key_of(cache: &Cache, text: &str) -> Option<String> {
    match cache.find_ticket(text, shortcodes::OrgReach::All) {
        shortcodes::Found::One(key) => Some(key),
        _ => None,
    }
}

/// The scope a shortcode is — `org:ID`, as a grant on it is stored.
pub(super) fn scope_of(cache: &Cache, sc: &ShortcodeId) -> Scope {
    let r = &cache.shortcodes;
    Scope::Shortcode { org: r.org(sc).cloned().unwrap_or_else(|| r.first_org()), id: sc.clone() }
}

impl Filing {
    /// ACCESS-11: under the write lock, whether these proofs file a birth
    /// under `parent` (resolved: the named parent, else the shortcode's
    /// root; `None`: neither — a new shortcode, in `org`) with
    /// `blocked_by` (each as `name_ticket` resolved it).
    pub(super) fn check(&self, cache: &Cache, parent: Option<&str>, org: Option<&crate::address::OrgId>, blocked_by: &[TicketName]) -> Result<(), StoreError> {
        let Some((under, blockers)) = &self.0 else { return Ok(()) };
        match (under, parent) {
            (Under::Parent(p), Some(parent)) if key_of(cache, p.id()).as_deref() == Some(parent) => {}
            (Under::Founding(on), None) => {
                let ok = match on.scope() {
                    Scope::Everywhere => true,
                    Scope::Org(o) => org.is_some_and(|w| cache.shortcodes.canonical(w) == cache.shortcodes.canonical(o)),
                    // ACCESS-21: a ticket's scope founds no shortcode.
                    Scope::Shortcode { .. } | Scope::Under(_) | Scope::Ticket(_) => false,
                };
                if !ok {
                    return Err(mismatch("a new shortcode needs editor on * or on its organization"));
                }
            }
            _ => return Err(mismatch("create: the parent it is filed under")),
        }
        let mut held: Vec<TicketName> = Vec::with_capacity(blockers.len());
        for b in blockers {
            match b {
                Blocker::Seen(p) => held.push(key_of(cache, p.id()).map(TicketName::new).ok_or_else(|| mismatch("blocked_by"))?),
                Blocker::Forward(f) => held.push(cache.name_ticket(f.name())?),
            }
        }
        if blocked_by.iter().any(|b| !held.contains(b)) {
            return Err(mismatch("blocked_by"));
        }
        Ok(())
    }
}

impl Parenting {
    /// ACCESS-11: under the write lock, whether these proofs move `child`
    /// (its row as it stands) under `parent` (resolved): `from` its parent
    /// now, `to` the one named, and a root's [`Rehost`] the hosts it
    /// leaves and joins. A parent that moved since it was asked refuses.
    pub(super) fn check(&self, cache: &Cache, child: &Row, parent: &str) -> Result<(), StoreError> {
        if key_of(cache, self.to.id()).as_deref() != Some(parent) {
            return Err(mismatch("set_parent: the parent it goes to"));
        }
        let now = child.parent.as_ref().map(|p| p.as_str().to_string());
        let asked = self.from.as_ref().and_then(|f| key_of(cache, f.id()));
        if now != asked {
            return Err(StoreError::Invalid(format!("{}'s parent moved since it was asked: ask again (nothing written)", child.id)));
        }
        let root = cache.rollups.root(&child.shortcode) == Some(child.id.as_str());
        if !root {
            return Ok(());
        }
        let Some(Rehost { leaving, joining }) = &self.rehost else {
            return Err(StoreError::Invalid(format!(
                "{} is {}'s root: moving it re-hosts the shortcode, and takes admin on the host it leaves and on the one it joins (nothing written)",
                child.id,
                cache.shortcodes.code(&child.shortcode)
            )));
        };
        let host = |p: Option<&str>| p.and_then(|p| cache.get(p)).map(|r| r.shortcode.clone());
        let left = host(now.as_deref()).unwrap_or_else(|| child.shortcode.clone());
        let joined = host(Some(parent)).ok_or_else(|| mismatch("set_parent: the host it joins"))?;
        if leaving.scope() != &scope_of(cache, &left) || joining.scope() != &scope_of(cache, &joined) {
            return Err(mismatch("set_parent: the hosts a root moves between"));
        }
        Ok(())
    }
}

impl Move {
    /// The child, and the parent it goes to, as the proofs name them.
    pub(super) fn ids(&self) -> (&str, &str) {
        let to = match &self.by {
            MoveBy::System(to) => to.id().as_str(),
            MoveBy::User(p) => p.to(),
        };
        (self.child.id().as_str(), to)
    }

    /// [`Parenting::check`], the system's move asking nothing.
    pub(super) fn check(&self, cache: &Cache, child: &Row, parent: &str) -> Result<(), StoreError> {
        match &self.by {
            MoveBy::System(_) => Ok(()),
            MoveBy::User(p) => p.check(cache, child, parent),
        }
    }
}

impl Updating {
    /// The system's: `id`, whatever it names.
    pub(super) fn system(ticket: Permit<Editor>) -> Self {
        Self { ticket, by: None }
    }

    /// ACCESS-11: under the write lock, whether these proofs cover what
    /// `update` names beyond its ticket (`row`, as it stands): a new
    /// parent, by [`Parenting::check`]; every id it blocks or unblocks, by
    /// a [`Blocker`] for it.
    pub(super) fn check(&self, cache: &Cache, row: &Row, update: &Update) -> Result<(), StoreError> {
        let Some((parenting, blockers)) = &self.by else { return Ok(()) };
        // A parent that names no ticket: the update's own refusal says so.
        if let Some(parent) = update.parent.as_deref().map(str::trim).filter(|p| !p.is_empty()).and_then(|p| key_of(cache, p)) {
            match parenting {
                Some(p) => p.check(cache, row, &parent)?,
                None => return Err(mismatch("update: the parent")),
            }
        }
        let held: Vec<String> = blockers.iter().map(|b| key_of(cache, b.name()).unwrap_or_else(|| b.name().to_ascii_uppercase())).collect();
        for b in update.block.iter().chain(&update.unblock).map(|b| b.trim()).filter(|b| !b.is_empty()) {
            let named = key_of(cache, b).unwrap_or_else(|| b.to_ascii_uppercase());
            let ok = held.iter().zip(blockers).any(|(h, given)| *h == named && (matches!(given, Blocker::Forward(_)) || cache.get(&named).is_some()));
            if !ok {
                return Err(mismatch("update: a blocker"));
            }
        }
        Ok(())
    }

    /// The ticket's permit, its id.
    pub(super) fn ticket(&self) -> &Permit<Editor> {
        &self.ticket
    }
}

impl<R: RoleMark> OnComment<R> {
    /// ACCESS-11: under the write lock, whether these proofs cover the
    /// comment's ticket `ticket` (resolved; `None`: a comment on the app).
    pub(super) fn check(&self, cache: &Cache, ticket: Option<&str>) -> Result<(), StoreError> {
        let CommentBy::Tickets(permits) = &self.0 else { return Ok(()) };
        match ticket {
            // On the app, or on a ticket gone (deleted): nothing to hold.
            None => Ok(()),
            Some(t) if cache.get(t).is_none() => Ok(()),
            Some(t) if permits.iter().any(|p| key_of(cache, p.id()).as_deref() == Some(t)) => Ok(()),
            Some(_) => Err(mismatch("a comment on a ticket")),
        }
    }
}

impl Authority {
    /// ACCESS-11: whether it may grant or revoke on the stored scope
    /// `scope`: the system, or admin minted on exactly it while login is on
    /// (ACCESS-3 #11: one minted with login off, for anyone, is refused —
    /// it would take effect when login turns on).
    pub(super) fn grants_on(&self, scope: &Scope) -> Result<(), StoreError> {
        match &self.0 {
            AuthorityBy::System => Ok(()),
            AuthorityBy::Admin(a) if a.anyone() => Err(StoreError::Invalid(
                "login is off: a grant made now would take effect when login turns on — sign in to grant or revoke (nothing written)".into(),
            )),
            AuthorityBy::Admin(a) if a.scope() == scope => Ok(()),
            _ => Err(mismatch("a grant or a revoke needs admin on its scope")),
        }
    }

    /// ACCESS-22: whether it is admin on some scope (minted while login is
    /// on), or the system — what making a team takes.
    pub(super) fn administers_somewhere(&self, what: &str) -> Result<(), StoreError> {
        match &self.0 {
            AuthorityBy::System => Ok(()),
            AuthorityBy::Admin(a) if !a.anyone() => Ok(()),
            _ => Err(mismatch(what)),
        }
    }

    /// Whether it is admin on exactly `scope`, or the system.
    pub(super) fn admin_on(&self, scope: &Scope, what: &str) -> Result<(), StoreError> {
        match &self.0 {
            AuthorityBy::System => Ok(()),
            AuthorityBy::Admin(a) if a.scope() == scope => Ok(()),
            _ => Err(mismatch(what)),
        }
    }

    /// Whether it is editor (or admin) on exactly `scope`, or the system.
    pub(super) fn editor_on(&self, scope: &Scope, what: &str) -> Result<(), StoreError> {
        match &self.0 {
            AuthorityBy::System => Ok(()),
            AuthorityBy::Editor(a) if a.scope() == scope => Ok(()),
            AuthorityBy::Admin(a) if a.scope() == scope => Ok(()),
            _ => Err(mismatch(what)),
        }
    }

    /// Whether it is a deployer (or admin) on exactly `scope`, or the system.
    pub(super) fn deployer_on(&self, scope: &Scope, what: &str) -> Result<(), StoreError> {
        match &self.0 {
            AuthorityBy::System => Ok(()),
            AuthorityBy::Deployer(a) if a.scope() == scope => Ok(()),
            AuthorityBy::Admin(a) if a.scope() == scope => Ok(()),
            _ => Err(mismatch(what)),
        }
    }
}

/// ACCESS-11: of many permits, whether they are for exactly the tickets of
/// `ids` (each once, any order) — what a write over many tickets checks of
/// the proofs its caller minted for the tickets it selected. An id that
/// names no ticket has no permit: the write refuses it itself, by name.
pub(super) fn covers_exactly<'p, R: RoleMark>(cache: &Cache, permits: impl IntoIterator<Item = &'p Permit<R>>, ids: &[String]) -> Result<(), StoreError> {
    // A permit naming no ticket (the system's, for an id that is none)
    // writes nothing either: each side, the tickets it names.
    let mut held: Vec<String> = permits.into_iter().filter_map(|p| key_of(cache, p.id())).collect();
    let mut want: Vec<String> = ids.iter().filter_map(|id| key_of(cache, id)).collect();
    held.sort();
    want.sort();
    match held == want {
        true => Ok(()),
        false => Err(mismatch("a write over many tickets needs a permit for each")),
    }
}

impl System {
    /// ACCESS-11: the system's move of `child` under `to`.
    pub fn moving(&self, child: impl AsRef<str>, to: impl AsRef<str>) -> Move {
        Move { child: self.permit(child), by: MoveBy::System(self.permit(to)) }
    }

    /// ACCESS-11: the system's update of `id`.
    pub fn updating(&self, id: impl AsRef<str>) -> Updating {
        Updating::system(self.permit(id))
    }
}

impl Store {
    /// ACCESS-11: the parent the ticket `id` (a key) has now — what a move
    /// asks a permit for. From the cache.
    pub(crate) fn parent_of(&self, id: &str) -> Option<crate::address::TicketId> {
        self.tickets.read().expect("store lock poisoned").get(id).and_then(|r| r.parent.clone())
    }

    /// ACCESS-11: whether the ticket `id` (a key) is its shortcode's root.
    pub(crate) fn is_root(&self, id: &str) -> bool {
        let cache = self.tickets.read().expect("store lock poisoned");
        cache.get(id).is_some_and(|r| cache.rollups.root(&r.shortcode) == Some(r.id.as_str()))
    }

    /// ACCESS-11: the scope the shortcode `sc` is — `org:ID`, as a grant on
    /// it is stored.
    pub(crate) fn scope_of_shortcode(&self, sc: &ShortcodeId) -> Scope {
        scope_of(&self.tickets.read().expect("store lock poisoned"), sc)
    }

    /// ACCESS-11: the scope of the shortcode the ticket `id` (a key) is in
    /// — what admin on its host is asked of.
    pub(crate) fn shortcode_scope_of(&self, id: &str) -> Option<Scope> {
        let cache = self.tickets.read().expect("store lock poisoned");
        cache.get(id).map(|r| scope_of(&cache, &r.shortcode))
    }

    /// ACCESS-11: the scope of the shortcode a deleted ticket's key was
    /// minted in (a deleted ticket never moves) — `None` for text that is
    /// no key.
    pub(crate) fn deleted_scope(&self, id: &str) -> Option<Scope> {
        let (sc, _) = id.trim().rsplit_once('-')?;
        let cache = self.tickets.read().expect("store lock poisoned");
        Some(scope_of(&cache, &ShortcodeId::new(&sc.to_ascii_uppercase())))
    }
}
