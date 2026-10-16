//! ACCESS-9: the **capabilities** — values whose existence proves a check
//! ran (ought capability-types; ACCESS-1). Each is minted in this module
//! alone, from the one before it:
//!
//! | type | proves | minted by |
//! | --- | --- | --- |
//! | [`VerifiedUser`] | the sign-in stands, the principal parsed, its grants resolved | [`Access::verify`] |
//! | [`Permit<R>`] | the user holds a role `R` admits on the ticket it names | [`VerifiedUser::permit`] |
//! | [`AdminOf`] | the user is admin on the scope it names | [`VerifiedUser::admin_of`] |
//! | [`crate::store::System`] | the holder opened the store: it acts for no user | the store's open |
//!
//! [`Hidden`] is the one answer for a ticket that is not there and one that
//! is not the user's; [`Refusal`] every other reason a call is turned away.
//! No public field, no public constructor, no `Default`, no `Deserialize`;
//! the `for_tests` constructors are compiled out of the server. None of
//! them can be built elsewhere:
//!
//! ```compile_fail,E0451
//! // A VerifiedUser: only Access::verify makes one.
//! use menger_tickets_api::access::VerifiedUser;
//! fn forge(reach: menger_tickets_api::access::Reach) -> VerifiedUser {
//!     VerifiedUser { principal: None, name: None, reach, ctx: None, gate: None }
//! }
//! ```
//!
//! ```compile_fail,E0451
//! // A Permit: only VerifiedUser::permit makes one, for the id it checked.
//! use menger_tickets_api::access::{Editor, Permit};
//! use menger_tickets_api::address::TicketId;
//! let forged: Permit<Editor> = Permit { id: TicketId::new("MENGER-1"), _role: std::marker::PhantomData };
//! ```
//!
//! ```compile_fail,E0451
//! // An AdminOf: only VerifiedUser::admin_of makes one.
//! use menger_tickets_api::access::AdminOf;
//! let forged = AdminOf { scope: menger_tickets_api::members::Scope::Everywhere };
//! ```

use super::*;
use crate::address::TicketId;
use crate::members::Scope;
use crate::vocab::Principal as Who;
use crate::vocab::TicketRef;
use std::marker::PhantomData;

/// Why a call is turned away, when it is not [`Hidden`]: the sign-in, the
/// session's ceiling, or no grant where the call needs one. Said in words
/// as an in-band error ([`Refusal`]'s `Display`, the strings menger always
/// answered).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// No signed-in identity, where login is on.
    SignIn,
    /// A signed-in identity that names no principal (none stamped, or one
    /// the identity layer cannot read) — never its `user_id` instead.
    NoPrincipal,
    /// The sign-in's expiry has passed.
    Expired,
    /// Its session was signed out.
    SignedOut,
    /// The session's ceiling cannot be read.
    Ceiling(String),
    /// `who` holds `grant` nowhere — waiting for an admin.
    Nowhere { who: String, grant: String },
    /// A ceiling layer (`layer`, as kept) allows `grant` nowhere.
    OutsideCeiling { layer: String, grant: String },
    /// The gate's own refusal, or admin wanted and not held, in words.
    Lacks(String),
    /// A reach a read's lens cannot be made of.
    Unreadable(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignIn => f.write_str("sign in first: this menger requires it"),
            Self::NoPrincipal => f.write_str("sign in first: this identity names no principal"),
            Self::Expired => f.write_str("your sign-in has expired: sign in again"),
            Self::SignedOut => f.write_str("you have signed out: sign in again"),
            Self::Ceiling(why) | Self::Lacks(why) | Self::Unreadable(why) => f.write_str(why),
            Self::Nowhere { who, grant } => write!(f, "no access yet: {who} holds {grant} nowhere — ask an admin for a role"),
            Self::OutsideCeiling { layer, grant } => write!(f, "this session's scope ({layer}) holds no {grant}"),
        }
    }
}

impl std::error::Error for Refusal {}

/// ACCESS-13: a refusal as the frame's resolver answers it on the wire,
/// typed (MACRO-14): a sign-in that does not stand — none, no principal,
/// expired, signed out, a ceiling that cannot be read — is
/// `unauthenticated` (-32001), in its words; anything else is `forbidden`
/// (-32003). The gate refuses a grant held nowhere before the resolver
/// runs, so the second is rare: a reach no read can be made of.
impl From<Refusal> for plexus_core::plexus::PlexusError {
    fn from(r: Refusal) -> Self {
        use plexus_core::plexus::{AuthzDenyReason, PlexusError};
        match r {
            Refusal::SignIn | Refusal::NoPrincipal | Refusal::Expired | Refusal::SignedOut | Refusal::Ceiling(_) => PlexusError::Unauthenticated(r.to_string()),
            Refusal::Nowhere { .. } | Refusal::OutsideCeiling { .. } | Refusal::Lacks(_) | Refusal::Unreadable(_) => {
                PlexusError::Forbidden { reason: AuthzDenyReason::NotAccepted }
            }
        }
    }
}

impl From<Refusal> for String {
    fn from(r: Refusal) -> String {
        r.to_string()
    }
}

/// "No such ticket" and "not yours", one answer: a caller learns nothing
/// from which it was (IDENTITY-37). Not a capability — anyone may say it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hidden;

impl std::fmt::Display for Hidden {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("no such ticket")
    }
}

impl std::error::Error for Hidden {}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Viewer {}
    impl Sealed for super::Editor {}
    impl Sealed for super::Admin {}
}

/// A role a [`Permit`] is for — [`Viewer`], [`Editor`], [`Admin`]; sealed:
/// no other. `admits` says which held roles grant it: viewer is held by
/// every role (deployer reads too, DEPLOY-14), editor by editor and
/// admin, admin by admin.
pub trait RoleMark: sealed::Sealed + Send + Sync + 'static {
    /// The role it names.
    const ROLE: Role;
    /// Whether holding `held` grants it.
    fn admits(held: Role) -> bool;
}

/// The mark of a permit to read.
#[derive(Debug)]
pub enum Viewer {}
/// The mark of a permit to change.
#[derive(Debug)]
pub enum Editor {}
/// The mark of a permit to administer.
#[derive(Debug)]
pub enum Admin {}

impl RoleMark for Viewer {
    const ROLE: Role = Role::Viewer;
    fn admits(_: Role) -> bool {
        true
    }
}

impl RoleMark for Editor {
    const ROLE: Role = Role::Editor;
    fn admits(held: Role) -> bool {
        matches!(held, Role::Editor | Role::Admin)
    }
}

impl RoleMark for Admin {
    const ROLE: Role = Role::Admin;
    fn admits(held: Role) -> bool {
        held == Role::Admin
    }
}

/// Holding one proves its user holds a role `R` admits on the ticket it
/// names — checked against the kept tree when it was minted, within the
/// session's ceiling. It carries its id: a write takes the permit, not an
/// id beside it, so a write cannot check one ticket and change another
/// (ACCESS-11). `Permit<Editor>` (and `Permit<Admin>`) gives the weaker
/// permit for the same ticket.
#[derive(Debug)]
pub struct Permit<R: RoleMark> {
    id: TicketId,
    _role: PhantomData<fn() -> R>,
}

impl<R: RoleMark> Permit<R> {
    fn new(id: TicketId) -> Self {
        Self { id, _role: PhantomData }
    }

    /// The ticket it is for.
    pub fn id(&self) -> &TicketId {
        &self.id
    }

    /// A permit for a test in another module (compiled out of the server).
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_tests(id: TicketId) -> Self {
        Self::new(id)
    }
}

impl Permit<Editor> {
    /// Editor ⇒ viewer: the read permit for the same ticket.
    pub fn viewer(&self) -> Permit<Viewer> {
        Permit::new(self.id.clone())
    }
}

impl Permit<Admin> {
    /// Admin ⇒ editor.
    pub fn editor(&self) -> Permit<Editor> {
        Permit::new(self.id.clone())
    }

    /// Admin ⇒ viewer.
    pub fn viewer(&self) -> Permit<Viewer> {
        Permit::new(self.id.clone())
    }
}

/// Holding one proves its user is admin on the scope it names (the gate's
/// [`ADMINISTER`] there, ceiling included) — what a grant or a revoke on it
/// takes (ACCESS-11).
#[derive(Debug)]
pub struct AdminOf {
    scope: Scope,
    /// Login off: minted for anyone (ACCESS-11).
    anyone: bool,
}

impl AdminOf {
    /// The scope it administers.
    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    /// An `AdminOf` for a test in another module (compiled out of the server).
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_tests(scope: Scope) -> Self {
        Self { scope, anyone: false }
    }
}

impl AdminOf {
    /// ACCESS-11: whether it was minted for no one in particular — login
    /// off, where anyone reaches everything. A grant or a revoke refuses
    /// one: planted while login is off, it would take effect when login
    /// turns on (ACCESS-3 #11).
    pub fn anyone(&self) -> bool {
        self.anyone
    }
}

/// The mark of a role on a scope a CI's key holds (DEPLOY-14): deployer,
/// held by deployer and admin — what `deployments.record` takes.
#[derive(Debug)]
pub enum Deployer {}

impl sealed::Sealed for Deployer {}

impl RoleMark for Deployer {
    const ROLE: Role = Role::Deployer;
    fn admits(held: Role) -> bool {
        matches!(held, Role::Deployer | Role::Admin)
    }
}

/// ACCESS-11: holding one proves its user holds a role `R` admits on the
/// scope it names (`*`, an organization, a shortcode — or one covering it),
/// within its ceiling: what a write that names no ticket but a scope takes
/// — a new shortcode (editor on `*` or its organization), a theme, a
/// deployment. [`AdminOf`] is the admin's.
#[derive(Debug)]
pub struct On<R: RoleMark> {
    scope: Scope,
    _role: PhantomData<fn() -> R>,
}

/// [`On`] the editor's.
pub type EditorOf = On<Editor>;
/// [`On`] the deployer's.
pub type DeployerOf = On<Deployer>;

impl<R: RoleMark> On<R> {
    /// The scope it is for.
    pub fn scope(&self) -> &Scope {
        &self.scope
    }

    /// An `On` for a test in another module (compiled out of the server).
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_tests(scope: Scope) -> Self {
        Self { scope, _role: PhantomData }
    }
}

/// ACCESS-11: a name a write may record without reading it — a blocker not
/// born yet (a forward reference). Only one who sees every ticket mints
/// one: to anyone else a name that is no ticket and a ticket out of reach
/// must be one answer ([`Hidden`]), so it names only what it may read.
#[derive(Debug)]
pub struct Forward {
    name: TicketRef,
}

impl Forward {
    /// The name, as given — ACCESS-48: a [`TicketRef`], the store records
    /// it as the [`crate::TicketName`] it settles to.
    pub fn name(&self) -> &TicketRef {
        &self.name
    }
}

/// ACCESS-11: the [`System`] — the code that opened the store, acting for
/// no user — mints any proof: the builder's seeding, the access layer's
/// own writes (`auth.grant`, `auth.revoke`, `auth.role_set`, decided by the
/// gate), migrations, tests. A permit it mints names its ticket as given
/// (any address); the write resolves it as it always did.
impl System {
    /// A permit for the ticket `id` names, unchecked.
    pub fn permit<R: RoleMark>(&self, id: impl AsRef<str>) -> Permit<R> {
        Permit::new(TicketId::new(id.as_ref().trim()))
    }

    /// A permit for each ticket `ids` names, unchecked.
    pub fn permits<R: RoleMark>(&self, ids: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<Permit<R>> {
        ids.into_iter().map(|id| self.permit(id)).collect()
    }

    /// Admin on `scope`, unchecked.
    pub fn admin_of(&self, scope: &Scope) -> AdminOf {
        AdminOf { scope: scope.clone(), anyone: false }
    }

    /// A role `R` on `scope`, unchecked.
    pub fn on<R: RoleMark>(&self, scope: &Scope) -> On<R> {
        On { scope: scope.clone(), _role: PhantomData }
    }

    /// A forward reference to `name`.
    pub fn forward(&self, name: impl AsRef<str>) -> Forward {
        Forward { name: TicketRef::stored(name.as_ref().trim()) }
    }
}

/// ACCESS-10: a [`VerifiedUser`] reading through one of its grants' reach
/// ([`VerifiedUser::reading`]) — a [`crate::store::Reader`] as the user is,
/// answered from where it holds `grant`.
#[derive(Debug, Clone)]
pub struct ReadingAs<'a> {
    user: &'a VerifiedUser,
    grant: GateGrant,
}

impl ReadingAs<'_> {
    /// The filter a store read answers it from ([`VerifiedUser::filter_for`]
    /// its grant).
    pub fn read_filter(&self) -> Result<Option<Filter>, String> {
        self.user.filter_for(&self.grant)
    }

    /// The user it reads as.
    pub fn user(&self) -> &VerifiedUser {
        self.user
    }
}

/// Holding one proves the call's identity was verified now: signed in (or
/// login off), the sign-in alive and unexpired, the principal parsed by the
/// identity layer's grammar — never its `user_id` instead — its ceiling
/// readable, and its grants resolved (direct ∪ its teams') into a
/// [`Reach`] at the grants generation of the moment. Minted only by
/// [`Access::verify`] (and [`Access::verify_call`], which says which grant
/// the call was admitted for). Every store read takes one (ACCESS-10).
pub struct VerifiedUser {
    /// `None`: login is off — anyone, reaching everything.
    principal: Option<Who>,
    name: Option<String>,
    reach: Reach,
    /// The identity verified — what the gate's own questions take.
    ctx: Option<AuthContext>,
    /// ACCESS-13: the gate's reach the call was admitted under — what an
    /// open watch asks again as the grants move ([`super::Following::of`]);
    /// `None` with login off, and for a user verified outside a dispatch.
    gate: Option<GateReach>,
}

impl std::fmt::Debug for VerifiedUser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedUser").field("principal", &self.principal).field("grant", &self.reach.call()).finish()
    }
}

impl VerifiedUser {
    /// Login off: anyone, everything.
    pub(super) fn anyone() -> Self {
        Self { principal: None, name: None, reach: Reach::everything(), ctx: None, gate: None }
    }

    pub(super) fn verified(principal: Who, name: Option<String>, reach: Reach, ctx: AuthContext) -> Self {
        Self { principal: Some(principal), name, reach, ctx: Some(ctx), gate: None }
    }

    pub(super) fn for_call(self, grant: Option<GateGrant>) -> Self {
        Self { reach: self.reach.for_call(grant), ..self }
    }

    /// ACCESS-13: the gate's reach for the call, kept.
    pub(super) fn following(self, gate: GateReach) -> Self {
        Self { gate: Some(gate), ..self }
    }

    /// ACCESS-13: the identity verified (`None` with login off).
    pub fn context(&self) -> Option<&AuthContext> {
        self.ctx.as_ref()
    }

    /// ACCESS-13: the gate's reach the call was admitted under (`None` with
    /// login off).
    pub fn gate_reach(&self) -> Option<&GateReach> {
        self.gate.as_ref()
    }

    /// A user holding `grants` (no ceiling), behind a gate over `access`'s
    /// book — for a test in another module (compiled out of the server).
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_tests(access: &Access, principal: Who, grants: Vec<(Held, Role)>) -> Self {
        let reach = Reach::for_tests(principal.as_str(), grants, access.gate().clone());
        Self { principal: Some(principal), name: None, reach, ctx: None, gate: None }
    }

    /// The principal (`None` with login off).
    pub fn principal(&self) -> Option<&Who> {
        self.principal.as_ref()
    }

    /// The principal's text, owned — what a stored row names it by.
    pub fn principal_text(&self) -> Option<String> {
        self.principal.as_ref().map(|p| p.as_str().to_string())
    }

    /// The name to show, when signed in.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// What it holds.
    pub fn reach(&self) -> &Reach {
        &self.reach
    }

    /// The grant the call was admitted for (the method's); `None` with
    /// login off.
    pub fn grant(&self) -> Option<&GateGrant> {
        self.reach.call()
    }

    fn who(&self) -> String {
        self.principal.as_ref().map(|p| p.as_str().to_string()).unwrap_or_else(|| "an anonymous caller".into())
    }

    /// A [`Permit`] for the ticket `id` at role `R` — when the store holds
    /// it and the reach covers it there (its chain, as the tree stands
    /// now). Not there and not covered are one answer, [`Hidden`].
    pub fn permit<R: RoleMark>(&self, id: &TicketId, store: &Store) -> Result<Permit<R>, Hidden> {
        if store.ticket_id(id.as_str()).as_deref() != Some(id.as_str()) {
            return Err(Hidden);
        }
        let chain = store.gate_chain(id.as_str()).ok_or(Hidden)?;
        match self.reach.covers::<R>(id.as_str(), &chain) {
            true => Ok(Permit::new(id.clone())),
            false => Err(Hidden),
        }
    }

    /// An [`AdminOf`] `scope` — when this user holds admin on a scope that
    /// covers it (`*`; its organization; the shortcode or one hosting it),
    /// and so does every ceiling layer: the reach's own question, asked of
    /// the scope's chain in the kept tree.
    pub fn admin_of(&self, scope: &Scope, store: &Store) -> Result<AdminOf, Refusal> {
        let chain = match scope {
            Scope::Everywhere => Vec::new(),
            s => store.gate_chain(&s.text()).unwrap_or_default(),
        };
        match self.reach.covers::<Admin>("", &chain) {
            true => Ok(AdminOf { scope: scope.clone(), anyone: self.reach.gate().is_none() }),
            false => Err(Refusal::Lacks(format!("{} needs admin on {scope}", self.who()))),
        }
    }

    /// ACCESS-11: an [`On<R>`] `scope` — when this user holds a role `R`
    /// admits on a scope that covers it, within every ceiling layer: as
    /// [`VerifiedUser::admin_of`] asks for admin.
    pub fn on<R: RoleMark>(&self, scope: &Scope, store: &Store) -> Result<On<R>, Refusal> {
        let chain = match scope {
            Scope::Everywhere => Vec::new(),
            s => store.gate_chain(&s.text()).unwrap_or_default(),
        };
        match self.reach.covers::<R>("", &chain) {
            true => Ok(On { scope: scope.clone(), _role: PhantomData }),
            false => Err(Refusal::Lacks(format!("{} needs {} on {scope}", self.who(), R::ROLE))),
        }
    }

    /// ACCESS-11: a [`Forward`] reference to `name` — a name that is no
    /// ticket (yet), for a user who would see it if it were one: one who
    /// sees every ticket (login off, or a role on `*`), or holds a role on
    /// the whole shortcode the name is in (or an organization or a host
    /// covering it). To anyone else [`Hidden`], as a ticket out of reach
    /// is: a name that is no ticket and a hidden ticket, one answer.
    pub fn forward(&self, name: &str, store: &Store) -> Result<Forward, Hidden> {
        let ok = match self.sight_filter() {
            Ok(None) => true,
            Ok(Some(_)) => {
                let scope = store.shortcode_of(name.trim()).map(|sc| store.scope_of_shortcode(&sc)).ok_or(Hidden)?;
                let chain = store.gate_chain(&scope.text()).unwrap_or_default();
                self.reach.covers::<Viewer>("", &chain)
            }
            Err(_) => false,
        };
        // ACCESS-48: a name no ticket could have is no forward reference.
        match (ok, name.parse::<TicketRef>()) {
            (true, Ok(name)) => Ok(Forward { name }),
            _ => Err(Hidden),
        }
    }

    /// The filter `under` the scopes this user holds `grant` on (within
    /// its ceiling): `None` everywhere, `Err` nowhere. Built as predicates
    /// ([`Reach::filter_for`]).
    pub fn filter_for(&self, grant: &GateGrant) -> Result<Option<Filter>, String> {
        self.reach.filter_for(grant).map_err(String::from)
    }

    /// [`VerifiedUser::filter_for`] the grant the call was admitted for.
    pub fn reach_filter(&self) -> Result<Option<Filter>, String> {
        match self.grant() {
            Some(g) => self.filter_for(g),
            None => Ok(None),
        }
    }

    /// ACCESS-10: what a store read answers this user from — where the
    /// call's grant is held (the method's, as the gate admitted it), or,
    /// for a user verified outside a call, where [`SEES`] is: the filter
    /// `under` those scopes, `None` everywhere, `Err` nowhere. What
    /// [`crate::store::Reader`] reads.
    pub fn read_filter(&self) -> Result<Option<Filter>, String> {
        match self.grant() {
            Some(g) => self.filter_for(g),
            None => self.sight_filter(),
        }
    }

    /// ACCESS-10: this user reading through `grant`'s reach instead of the
    /// call's — a write naming tickets it must see ([`SEES`]), never wider
    /// than this user holds: the reach is still its own.
    pub fn reading(&self, grant: GateGrant) -> ReadingAs<'_> {
        ReadingAs { user: self, grant }
    }

    /// ACCESS-10: this user reading through [`SEES`] — what it can see of
    /// the tickets outside a read of its own (a write's refusal names only
    /// those).
    pub fn sight(&self) -> ReadingAs<'_> {
        self.reading(GateGrant::new(SEES))
    }

    /// [`VerifiedUser::filter_for`] [`SEES`]: what this user can see of the
    /// tickets, outside a read of its own.
    pub fn sight_filter(&self) -> Result<Option<Filter>, String> {
        if self.reach.gate().is_none() {
            return Ok(None);
        }
        self.filter_for(&GateGrant::new(SEES))
    }

    /// Where the call's grant is held, compiled — what a stream compares at
    /// each event to know the reach moved (compared, never formatted).
    pub fn reach_key(&self) -> Result<Option<ReachKey>, String> {
        match self.grant() {
            Some(g) => self.reach.key_for(g).map_err(String::from),
            None => Ok(None),
        }
    }

    /// A read's filter text, ANDed with where this user holds the call's
    /// grant: what the method answers under instead — the caller's text
    /// parsed, the reach's predicates added (never formatted and read
    /// back). Unchanged when it holds it everywhere; `Err` when nowhere.
    pub fn scoped(&self, text: Option<&str>) -> Result<Option<Filter>, String> {
        // ACCESS-29: the caller's own text is held to its size first.
        text.map(crate::limits::check_filter).transpose()?;
        let asked = match text.map(str::trim).filter(|t| !t.is_empty()) {
            Some(t) => Some(Filter::parse(t).map_err(|e| e.to_string())?),
            None => None,
        };
        let Some(reach) = self.reach_filter()? else {
            // As written: an empty text is no filter's groups, but a filter.
            return Ok(match (asked, text) {
                (Some(a), _) => Some(a),
                (None, Some(_)) => Some(Filter::default()),
                (None, None) => None,
            });
        };
        Ok(Some(match asked {
            Some(a) => a.and(&reach),
            None => reach,
        }))
    }

    /// [`VerifiedUser::scoped`] for a filter that must be given (`""`: none).
    pub fn scoped_text(&self, text: &str) -> Result<Filter, String> {
        Ok(self.scoped(Some(text))?.unwrap_or_default())
    }

    /// Whether this user holds `grant` on every one of `ids` — by the
    /// tickets' place in the tree, as a read sees them — `Err` naming the
    /// first it does not, and the shortcode it would be needed on.
    pub fn require_for(&self, grant: &GateGrant, ids: &[&str], store: &Store) -> Result<(), String> {
        if self.filter_for(grant).map_err(|_| self.lacks(grant, ids.first().copied()))?.is_none() {
            return Ok(());
        }
        // ACCESS-10: asked of the store as this user reading through `grant`.
        let owned: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        match store.member(&self.reading(grant.clone()), &owned, &Lens::all()).iter().position(|ok| !ok) {
            None => Ok(()),
            Some(i) => Err(self.lacks(grant, Some(ids[i]))),
        }
    }

    /// [`VerifiedUser::require_for`] the call's own grant.
    pub fn require(&self, ids: &[&str], store: &Store) -> Result<(), String> {
        match self.grant().cloned() {
            Some(g) => self.require_for(&g, ids, store),
            None => Ok(()),
        }
    }

    /// Why this user may not use `grant` on the ticket `id` (or anything
    /// here). IDENTITY-37: it names only the id's own shortcode, read from
    /// the id, so it is the same whether the ticket exists or not.
    pub fn lacks(&self, grant: &GateGrant, id: Option<&str>) -> String {
        match id {
            Some(id) => {
                // ORG-3: the shortcode the address names it in, by its
                // letters — never the text before a `-` (an org's name).
                let shortcode = crate::address::Address::parse(id)
                    .map(|a| a.code.as_str().to_string())
                    .unwrap_or_else(|_| {
                        // ORG-9: a namesake's key (`SYNAPSE-CC.2-3`) splits at its last `-`.
                        let code = crate::address::split_key(id).map(|(c, _)| c);
                        code.unwrap_or_else(|| id.split(['-', '/']).next().unwrap_or(id)).to_ascii_uppercase()
                    });
                format!("{} needs {grant} on {shortcode} for {id}", self.who())
            }
            None => format!("{} needs {grant} here", self.who()),
        }
    }

    /// The gate's own question for `grant` on `target` (`*` for
    /// everywhere), as it stands now: `Err` is its refusal, said in words
    /// (`needs tickets.create on MENGER`).
    pub fn check(&self, grant: &GateGrant, target: &str) -> Result<(), String> {
        let Some(gate) = self.reach.gate() else { return Ok(()) };
        gate.check(self.ctx.as_ref(), grant, Some(target)).map(|_| ()).map_err(|r| r.to_string())
    }

    /// Whether this user is admin on `target` (`*`: everywhere), within
    /// its ceiling ([`ADMINISTER`]).
    pub fn administers(&self, target: &str) -> bool {
        self.check(&GateGrant::new(ADMINISTER), target).is_ok()
    }

    /// ORG-4: the organizations this user holds a role in — what a short
    /// form resolves in, an ambiguous answer names candidates from, and
    /// `orgs.list` lists. `None`: every one (a role on `*`, or login off).
    pub fn orgs(&self) -> Option<Vec<crate::address::OrgId>> {
        self.reach.orgs()
    }
}
