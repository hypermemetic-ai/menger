use crate::store::SYS;
use super::*;
use crate::types::Kind;
use serde_json::json;
use std::collections::HashSet;

#[derive(Default)]
struct Live(Mutex<HashSet<String>>);
impl SessionCheck for Live {
    fn alive(&self, key: &str) -> bool {
        self.0.lock().unwrap().contains(key)
    }
    fn key(&self) -> &str {
        SESSION_KEY
    }
}

/// The session table's metadata key, as menger's names it.
const SESSION_KEY: &str = "menger_session";

const ALICE: &str = "idp:c952acd3-60aa-4534-b2e3-83634591fee1";

/// Alice's identity, stamped with her principal as every adapter stamps it.
fn ctx(meta: serde_json::Value) -> AuthContext {
    let mut meta = meta;
    if let Some(m) = meta.as_object_mut() {
        m.entry("principal").or_insert(ALICE.into());
    }
    AuthContext::new(ALICE.into(), "s".into(), vec![], meta)
}

fn grant(principal: &str, scope: &str, role: Role) -> Grant {
    Grant { principal: principal.parse().unwrap(), scope: kept_scope(scope), role, granted_by: crate::vocab::GrantedBy::AuthGrant, at: 1 }
}

/// ACCESS-48: a scope as kept — a bare shortcode (`IDENTITY`) as ORG-4's
/// open keeps one from before organizations, in the first organization.
fn kept_scope(text: &str) -> crate::members::Scope {
    let kept = match text.contains(':') || text == "*" {
        true => text.to_string(),
        false => format!("hypermemetic:{text}"),
    };
    crate::members::Scope::stored(&kept).unwrap_or_else(|| panic!("a test's scope: {text}"))
}

/// The default roles, as tickets declares them (a part of them).
fn declared() -> BTreeMap<String, Vec<String>> {
    let viewer = vec!["tickets.get".to_string(), "tickets.list".to_string()];
    let mut editor = viewer.clone();
    editor.push("tickets.set_status".into());
    BTreeMap::from([("viewer".to_string(), viewer), ("editor".to_string(), editor)])
}

/// Two shortcodes, IDENTITY hosted under PLEXUS-1, and OUGHT apart.
async fn store() -> Arc<Store> {
    let s = Arc::new(Store::new());
    let mk = |e: &str, parent: Option<&str>| (e.to_string(), parent.map(str::to_string));
    let new = |epic: &str, kind: Kind, parent: Option<String>| crate::store::NewTicket {
        epic: epic.into(),
        title: format!("{epic} {kind:?}"),
        kind,
        severity: None,
        blocked_by: Vec::new(),
        parent,
        path: None,
        body: String::new(),
    };
    for (epic, parent) in [mk("PLEXUS", None), mk("IDENTITY", Some("PLEXUS-1")), mk("OUGHT", None)] {
        s.create(SYS, new(&epic, Kind::Scope, parent)).await.unwrap();
        s.create(SYS, new(&epic, Kind::Build, Some(format!("{epic}-1")))).await.unwrap();
    }
    s
}

fn login(admins: Vec<String>) -> Access {
    let a = Access::with_login(Arc::new(Live::default()), true, admins);
    a.book().seed(declared());
    a
}

fn reach_of(a: &Access, c: &AuthContext, grant: &str) -> GateReach {
    a.gate().check(Some(c), &GateGrant::new(grant), None).unwrap_or_else(|_| GateReach::everywhere())
}

#[test]
fn without_login_everyone_reaches_everything() {
    let c = Access::open().verify_call(None, &GateReach::everywhere()).unwrap();
    assert!(c.administers("*"));
    assert_eq!(c.scoped(Some("status Ready")).unwrap().map(|f| f.format()).as_deref(), Some("status Ready"));
}

#[test]
fn an_anonymous_call_is_refused() {
    let a = login(vec![]);
    assert!(a.verify_call(None, &GateReach::everywhere()).is_err());
    assert!(a.verify_call(Some(&AuthContext::anonymous()), &GateReach::everywhere()).is_err());
}

#[test]
fn a_session_signed_out_or_expired_is_refused() {
    let live = Arc::new(Live::default());
    live.0.lock().unwrap().insert("k1".into());
    let a = Access::with_login(live.clone(), true, vec![]);
    let ok = ctx(json!({ SESSION_KEY: "k1", "expires_at": now() + 60, "name": "alice" }));
    assert_eq!(a.verify_call(Some(&ok), &GateReach::everywhere()).unwrap().name(), Some("alice"));
    live.0.lock().unwrap().clear();
    assert!(a.verify_call(Some(&ok), &GateReach::everywhere()).unwrap_err().contains("signed out"));
    let expired = ctx(json!({ "expires_at": now() - 1 }));
    assert!(a.verify_call(Some(&expired), &GateReach::everywhere()).unwrap_err().contains("expired"));
}

#[tokio::test]
async fn no_grant_waits() {
    let a = login(vec![]);
    a.attach(store().await);
    let c = a.verify_call(Some(&ctx(json!({ "name": "alice", "provider": "plexus-idp" }))), &GateReach::everywhere()).unwrap();
    assert!(c.filter_for(&GateGrant::new("tickets.list")).unwrap_err().contains("ask an admin"));
    assert_eq!(a.waiting()[0].principal, ALICE);
}

#[tokio::test]
async fn reads_are_anded_with_where_the_grant_is_held() {
    let a = login(vec![]);
    let s = store().await;
    s.grant(SYS, grant(ALICE, "IDENTITY", Role::Editor)).await.unwrap();
    s.grant(SYS, grant(ALICE, "OUGHT", Role::Viewer)).await.unwrap();
    a.attach(s);
    let me = ctx(json!({}));
    let c = a.verify_call(Some(&me), &reach_of(&a, &me, "tickets.list")).unwrap();
    // The language's canonical spelling of (under IDENTITY or under OUGHT).
    assert_eq!(c.scoped(None).unwrap().unwrap().format(), "under in (IDENTITY, OUGHT)");
    assert_eq!(c.scoped(Some("status Ready")).unwrap().unwrap().format(), "status Ready and under in (IDENTITY, OUGHT)");
    // set_status is held on IDENTITY alone.
    assert_eq!(c.filter_for(&GateGrant::new("tickets.set_status")).unwrap().unwrap().format(), "under IDENTITY");
    assert!(a.waiting().is_empty(), "a member does not wait");
}

#[tokio::test]
async fn the_gate_resolves_a_ticket_through_its_shortcode_and_hosts() {
    let a = login(vec![]);
    let s = store().await;
    s.grant(SYS, grant(ALICE, "PLEXUS", Role::Editor)).await.unwrap();
    a.attach(s.clone());
    let g = a.gate();
    let me = ctx(json!({}));
    let set = GateGrant::new("tickets.set_status");
    assert!(g.check(Some(&me), &set, Some("IDENTITY-2")).is_ok(), "IDENTITY is hosted under PLEXUS-1");
    assert!(g.check(Some(&me), &set, Some("IDENTITY-99")).is_ok(), "a missing ticket resolves by its shortcode");
    // ORG-4: a ticket target is refused by what was written — the same
    // words for a ticket out of reach as for none.
    let refused = g.check(Some(&me), &set, Some("OUGHT-2")).unwrap_err();
    assert_eq!(refused.to_string(), "needs tickets.set_status on OUGHT-2");
    assert_eq!(g.check(Some(&me), &set, Some("OUGHT-99")).unwrap_err().to_string(), "needs tickets.set_status on OUGHT-99");
    assert_eq!(
        s.gate_chain("identity-2").unwrap(),
        ["IDENTITY-2", "hypermemetic:IDENTITY", "IDENTITY", "IDENTITY-1", "PLEXUS-1", "hypermemetic:PLEXUS", "PLEXUS", "hypermemetic:"]
            .map(String::from)
            .to_vec()
    );
    assert_eq!(s.gate_chain("hypermemetic:").unwrap(), vec!["hypermemetic:".to_string()]);
    assert_eq!(
        s.scope_chain("identity-2").unwrap(),
        ["IDENTITY", "IDENTITY-2", "IDENTITY-1", "PLEXUS-1", "PLEXUS"].map(String::from).to_vec()
    );
    assert_eq!(s.scope_chain("IDENTITY/_MENGER/board").unwrap(), ["IDENTITY", "PLEXUS"].map(String::from).to_vec());
    assert!(s.scope_chain("a b").is_none());
}

#[tokio::test]
async fn the_waiting_list_is_capped_and_forgets_the_longest_waiting() {
    let a = login(vec![]);
    a.attach(store().await);
    let as_ = |p: String| AuthContext::new(p.clone(), "s".into(), vec![], json!({ "principal": p }));
    for i in 0..WAITING_MAX + 5 {
        a.verify_call(Some(&as_(format!("apikey:user-{i}"))), &GateReach::everywhere()).unwrap();
    }
    let w = a.waiting();
    assert_eq!(w.len(), WAITING_MAX);
    assert!(w.iter().any(|w| w.principal == format!("apikey:user-{}", WAITING_MAX + 4)), "the newest waits");
    a.verify_call(Some(&as_(format!("apikey:user-{}", WAITING_MAX + 4))), &GateReach::everywhere()).unwrap();
    assert_eq!(a.waiting().len(), WAITING_MAX);
}

#[tokio::test]
async fn an_operator_admin_is_admin_everywhere() {
    let a = login(vec![ALICE.into()]);
    a.attach(store().await);
    let me = ctx(json!({}));
    let c = a.verify_call(Some(&me), &reach_of(&a, &me, "tickets.list")).unwrap();
    assert!(c.administers("*"));
    assert_eq!(a.grants_of(ALICE)[0].granted_by, crate::vocab::GrantedBy::Operator);
    assert_eq!(c.reach_filter().unwrap(), None);
}

// ── IDENTITY-42: a session's ceiling ─────────────────────────────

#[derive(Default)]
struct Mint(Mutex<Vec<AuthContext>>);
impl SessionMint for Mint {
    fn mint(&self, ctx: AuthContext, ttl: i64) -> (String, i64) {
        self.0.lock().unwrap().push(ctx);
        ("mgs_t".into(), now() + ttl)
    }
    fn end_key(&self, _: &str) -> bool {
        true
    }
}

fn ceiling(layers: &[&[&str]]) -> serde_json::Value {
    json!(layers.iter().map(|l| l.to_vec()).collect::<Vec<_>>())
}

#[test]
fn a_ceiling_entry_is_role_or_grant_at_scope() {
    let a = login(vec![]);
    assert_eq!(a.ceiling_entry("editor@identity"), Ok("editor@IDENTITY".into()));
    assert_eq!(a.ceiling_entry(" viewer@* "), Ok("viewer@*".into()));
    assert_eq!(a.ceiling_entry("Admin@menger-12"), Ok("admin@MENGER-12".into()));
    assert_eq!(a.ceiling_entry("tickets.get@menger"), Ok("tickets.get@MENGER".into()));
    // ORG-9: a code may hold hyphens (X-1A is one); only the last `-N` is a number.
    assert_eq!(a.ceiling_entry("viewer@synapse-cc-1"), Ok("viewer@SYNAPSE-CC-1".into()));
    assert_eq!(a.ceiling_entry("viewer@x-1a"), Ok("viewer@X-1A".into()));
    for bad in ["editor", "owner@X", "editor@", "editor@a b", "editor@X-", "editor@X--1a", "editor@X-1-2", "editor@*-1", "editor@under X", "a.b c@X"] {
        assert!(a.ceiling_entry(bad).is_err(), "{bad}");
    }
}

#[tokio::test]
async fn every_check_needs_the_memberships_and_the_ceiling() {
    let a = login(vec![ALICE.into()]);
    let s = store().await;
    a.attach(s.clone());
    let g = a.gate();
    let set = GateGrant::new("tickets.set_status");
    // Admin on * from the operator, held to editor on IDENTITY.
    let me = ctx(json!({ CEILING_KEY: ceiling(&[&["editor@IDENTITY"]]) }));
    assert!(g.check(Some(&me), &set, Some("IDENTITY-2")).is_ok());
    assert!(g.check(Some(&me), &set, Some("OUGHT-2")).is_err(), "elsewhere");
    let c = a.verify_call(Some(&me), &reach_of(&a, &me, "tickets.list")).unwrap();
    assert!(!c.administers("IDENTITY"), "no more than editor");
    assert_eq!(c.scoped(Some("status Ready")).unwrap().unwrap().format(), "status Ready and under IDENTITY");
    // One ticket: it and its descendants.
    let me = ctx(json!({ CEILING_KEY: ceiling(&[&["editor@IDENTITY-2"]]) }));
    assert!(g.check(Some(&me), &set, Some("IDENTITY-2")).is_ok());
    assert!(g.check(Some(&me), &set, Some("IDENTITY-1")).is_err());
    // `*` in the ceiling is no ceiling at that role.
    let me = ctx(json!({ CEILING_KEY: ceiling(&[&["admin@*"]]) }));
    assert!(a.verify_call(Some(&me), &GateReach::everywhere()).unwrap().administers("*"));
}

#[tokio::test]
async fn a_ceiling_wider_than_the_memberships_gives_no_more() {
    let a = login(vec![]);
    let s = store().await;
    s.grant(SYS, grant(ALICE, "OUGHT", Role::Viewer)).await.unwrap();
    a.attach(s.clone());
    let g = a.gate();
    let me = ctx(json!({ CEILING_KEY: ceiling(&[&["admin@*"]]) }));
    assert!(g.check(Some(&me), &GateGrant::new("tickets.get"), Some("OUGHT-2")).is_ok());
    assert!(g.check(Some(&me), &GateGrant::new("tickets.set_status"), Some("OUGHT-2")).is_err());
    assert!(g.check(Some(&me), &GateGrant::new("tickets.get"), Some("IDENTITY-2")).is_err());
}

#[tokio::test]
async fn layered_ceilings_all_hold_and_an_unreadable_one_refuses() {
    let a = login(vec![ALICE.into()]);
    let s = store().await;
    a.attach(s.clone());
    let g = a.gate();
    let me = ctx(json!({ CEILING_KEY: ceiling(&[&["editor@PLEXUS"], &["admin@IDENTITY", "editor@OUGHT"]]) }));
    let set = GateGrant::new("tickets.set_status");
    assert!(g.check(Some(&me), &set, Some("IDENTITY-2")).is_ok(), "IDENTITY is hosted under PLEXUS");
    assert!(g.check(Some(&me), &GateGrant::new(ADMINISTER), Some("IDENTITY-2")).is_err(), "the first holds editor");
    assert!(g.check(Some(&me), &GateGrant::new("tickets.get"), Some("OUGHT-2")).is_err(), "the first does not reach OUGHT");
    assert!(g.check(Some(&me), &GateGrant::new("tickets.get"), Some("PLEXUS-2")).is_err(), "the second does not reach PLEXUS-2");
    for bad in [json!("editor@X"), json!([[1]])] {
        assert!(a.verify_call(Some(&ctx(json!({ CEILING_KEY: bad.clone() }))), &GateReach::everywhere()).is_err(), "{bad}");
    }
}

#[tokio::test]
async fn a_minted_session_keeps_the_caller_and_never_widens() {
    let mint = Arc::new(Mint::default());
    let a = login(vec![ALICE.into()]).minting(mint.clone());
    a.attach(store().await);
    let agent = ctx(json!({ "agent": "worker-3", "name": "worker-3", "provider": "key", "auth_method": "key", "expires_at": now() + 300 }));
    let m = a.mint(Some(&agent), None, Some(vec!["editor@identity".into()])).unwrap();
    // ORG-4: the shortcode as the store keeps a scope, in its organization.
    assert_eq!((m.principal.as_str(), m.agent.as_deref(), m.scope.clone()), (ALICE, Some("worker-3"), vec!["editor@hypermemetic:IDENTITY".to_string()]));
    assert!(m.expires_at >= now() + MINT_TTL_DEFAULT_SECS as i64 - 5);
    let minted = mint.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!(minted.metadata["agent"], "worker-3");
    assert_eq!(minted.metadata["provider"], "key");
    assert_eq!(minted.metadata["principal"], ALICE);
    assert_eq!(minted.metadata[CEILING_KEY], json!([["editor@hypermemetic:IDENTITY"]]));
    // The gate reads it as one layer.
    assert_eq!(GateCeiling::layers_of(&minted).unwrap().len(), 1);

    // From that session: its ceiling stays, the new one is added.
    let m2 = a.mint(Some(&minted), Some(60), Some(vec!["admin@*".into()])).unwrap();
    assert_eq!(m2.scope, vec!["admin@*".to_string()]);
    let again = mint.0.lock().unwrap().last().cloned().unwrap();
    assert_eq!(again.metadata[CEILING_KEY], json!([["editor@hypermemetic:IDENTITY"], ["admin@*"]]));
    assert!(!a.verify_call(Some(&again), &GateReach::everywhere()).unwrap().administers("*"), "still held to IDENTITY");
    // Asking nothing inherits it, and says so.
    assert_eq!(a.mint(Some(&minted), None, None).unwrap().scope, vec!["editor@hypermemetic:IDENTITY".to_string()]);
    assert!(a.mint(Some(&agent), None, None).unwrap().scope.is_empty(), "no ceiling");

    for (ttl, scope) in [(Some(0), None), (Some(MINT_TTL_MAX_SECS + 1), None), (None, Some(vec![])), (None, Some(vec!["owner@X".into()]))] {
        assert!(a.mint(Some(&agent), ttl, scope.clone()).is_err(), "{ttl:?} {scope:?}");
    }
    assert!(a.mint(Some(&AuthContext::anonymous()), None, None).is_err());
    assert!(login(vec![]).mint(Some(&agent), None, None).is_err(), "nothing to mint in");
    assert!(a.logout(Some(&agent)).is_err(), "a proof-opened connection holds no session");
    assert!(a.logout(Some(&ctx(json!({ SESSION_KEY: "k" })))).is_ok());
}

#[test]
fn an_edited_role_is_kept_in_the_file_across_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let rt = tokio::runtime::Runtime::new().unwrap();
    {
        let s = Store::open(&path).unwrap();
        rt.block_on(s.set_role(SYS, "viewer".into(), vec!["tickets.get".into(), "comments.list".into()])).unwrap();
        rt.block_on(s.set_role(SYS, "viewer".into(), vec!["tickets.get".into()])).unwrap();
    }
    let s = Store::open(&path).unwrap();
    assert_eq!(s.role_edits(), BTreeMap::from([("viewer".to_string(), vec!["tickets.get".to_string()])]));
}

#[tokio::test]
async fn an_admin_s_edit_wins_over_the_declared_role_and_survives_in_the_store() {
    let a = login(vec![]);
    let s = store().await;
    a.attach(s.clone());
    let book = a.book();
    let viewer = RoleName::new("viewer");
    assert!(book.grants(&viewer).unwrap().contains(&GateGrant::new("tickets.list")));
    RoleStore::set(&*book, &viewer, GrantSet::listed(["tickets.get"])).await.unwrap();
    assert!(!book.grants(&viewer).unwrap().contains(&GateGrant::new("tickets.list")));
    assert_eq!(s.role_edits()["viewer"], vec!["tickets.get".to_string()]);
    assert!(book.names().iter().any(|r| r.as_str() == "editor"));
}

// ── ACCESS-9: the capabilities ───────────────────────────────────

use crate::address::{OrgId, ShortcodeId, TicketId};
use crate::members::Scope;

fn org() -> OrgId {
    OrgId::new(OrgId::HYPERMEMETIC).unwrap()
}

fn sc(id: &str) -> Held {
    Held::Shortcode { org: org(), id: ShortcodeId::new(id) }
}

fn t(id: &str) -> TicketId {
    TicketId::new(id)
}

fn alice() -> crate::vocab::Principal {
    ALICE.parse().unwrap()
}

/// [`store`], and PLEXUS-3 under PLEXUS-2: a subtree to hold.
async fn deeper() -> Arc<Store> {
    let s = store().await;
    s.create(SYS, crate::store::NewTicket {
        epic: "PLEXUS".into(),
        title: "below".into(),
        kind: Kind::Build,
        severity: None,
        blocked_by: Vec::new(),
        parent: Some("PLEXUS-2".into()),
        path: None,
        body: String::new(),
    })
    .await
    .unwrap();
    s
}

const EVERY: [&str; 7] = ["PLEXUS-1", "PLEXUS-2", "PLEXUS-3", "IDENTITY-1", "IDENTITY-2", "OUGHT-1", "OUGHT-2"];

/// Of [`EVERY`], those `u` holds a viewer's permit for.
fn permitted(u: &VerifiedUser, s: &Store) -> Vec<&'static str> {
    EVERY.into_iter().filter(|id| u.permit::<Viewer>(&t(id), s).is_ok()).collect()
}

/// Of [`EVERY`], those a read of `tickets.get` answers for `u` — its lens,
/// built from the reach, asked of the cells.
fn readable(u: &VerifiedUser, s: &Store) -> Vec<&'static str> {
    let ids: Vec<String> = EVERY.iter().map(|s| s.to_string()).collect();
    let lens = match u.filter_for(&GateGrant::new(SEES)).unwrap() {
        None => Lens::all(),
        Some(f) => Lens::of(&f).unwrap(),
    };
    EVERY.into_iter().zip(s.member(SYS, &ids, &lens)).filter(|(_, ok)| *ok).map(|(id, _)| id).collect()
}

#[tokio::test]
async fn each_scope_kind_admits_exactly_its_tickets() {
    let a = login(vec![]);
    let s = deeper().await;
    a.attach(s.clone());
    let user = |held: Held| VerifiedUser::for_tests(&a, alice(), vec![(held, Role::Viewer)]);
    let cases: [(Held, &[&str]); 6] = [
        (Held::Everywhere, &EVERY),
        (Held::Org(org()), &EVERY),
        // A shortcode covers the shortcodes hosted under it.
        (sc("PLEXUS"), &["PLEXUS-1", "PLEXUS-2", "PLEXUS-3", "IDENTITY-1", "IDENTITY-2"]),
        (sc("IDENTITY"), &["IDENTITY-1", "IDENTITY-2"]),
        // `under ID`: it and its descendants; `ID`: itself only.
        (Held::Under(t("PLEXUS-2")), &["PLEXUS-2", "PLEXUS-3"]),
        (Held::Ticket(t("PLEXUS-2")), &["PLEXUS-2"]),
    ];
    for (held, admits) in cases {
        let u = user(held.clone());
        assert_eq!(permitted(&u, &s), admits.to_vec(), "{held:?}: permits");
        if !matches!(held, Held::Ticket(_)) {
            assert_eq!(readable(&u, &s), admits.to_vec(), "{held:?}: a read's lens agrees");
        }
    }
    // One ticket alone is no `under`: a read is refused, never widened.
    assert!(user(Held::Ticket(t("PLEXUS-2"))).filter_for(&GateGrant::new(SEES)).is_err());
    // A higher role on a narrower scope never lowers a wider one.
    let u = VerifiedUser::for_tests(&a, alice(), vec![(sc("PLEXUS"), Role::Viewer), (sc("IDENTITY"), Role::Editor)]);
    assert_eq!(permitted(&u, &s).len(), 5);
    assert!(u.permit::<Editor>(&t("IDENTITY-2"), &s).is_ok());
    assert!(u.permit::<Editor>(&t("PLEXUS-2"), &s).is_err());
}

#[tokio::test]
async fn a_missing_ticket_and_one_out_of_reach_are_one_answer() {
    let a = login(vec![]);
    let s = store().await;
    a.attach(s.clone());
    let u = VerifiedUser::for_tests(&a, alice(), vec![(sc("IDENTITY"), Role::Admin)]);
    assert_eq!(u.permit::<Viewer>(&t("OUGHT-2"), &s).unwrap_err(), Hidden);
    assert_eq!(u.permit::<Viewer>(&t("IDENTITY-99"), &s).unwrap_err(), Hidden, "in reach by its shortcode, but not there");
    assert_eq!(u.permit::<Viewer>(&t("NOWHERE-1"), &s).unwrap_err(), Hidden);
    // Even everywhere, a ticket that is not there has no permit.
    let all = VerifiedUser::for_tests(&a, alice(), vec![(Held::Everywhere, Role::Admin)]);
    assert_eq!(all.permit::<Admin>(&t("IDENTITY-99"), &s).unwrap_err(), Hidden);
    let p = u.permit::<Admin>(&t("IDENTITY-2"), &s).unwrap();
    assert_eq!(p.id().as_str(), "IDENTITY-2");
    assert_eq!(p.editor().viewer().id(), p.id(), "admin ⇒ editor ⇒ viewer, for the same ticket");
}

#[tokio::test]
async fn a_permit_needs_a_role_its_mark_admits() {
    let a = login(vec![]);
    let s = store().await;
    a.attach(s.clone());
    let id = t("OUGHT-2");
    for (role, viewer, editor, admin) in [
        (Role::Viewer, true, false, false),
        (Role::Deployer, true, false, false),
        (Role::Editor, true, true, false),
        (Role::Admin, true, true, true),
    ] {
        let u = VerifiedUser::for_tests(&a, alice(), vec![(sc("OUGHT"), role)]);
        assert_eq!(
            (u.permit::<Viewer>(&id, &s).is_ok(), u.permit::<Editor>(&id, &s).is_ok(), u.permit::<Admin>(&id, &s).is_ok()),
            (viewer, editor, admin),
            "{role}"
        );
    }
    let editor = VerifiedUser::for_tests(&a, alice(), vec![(sc("OUGHT"), Role::Editor)]);
    assert_eq!(editor.permit::<Editor>(&id, &s).unwrap().viewer().id(), &id, "editor ⇒ viewer");
}

#[tokio::test]
async fn a_grant_on_a_ticket_that_moves_follows_it() {
    let a = login(vec![]);
    let s = deeper().await;
    a.attach(s.clone());
    let under = VerifiedUser::for_tests(&a, alice(), vec![(Held::Under(t("PLEXUS-2")), Role::Editor)]);
    let shortcode = VerifiedUser::for_tests(&a, alice(), vec![(sc("PLEXUS"), Role::Editor)]);
    assert!(under.permit::<Editor>(&t("PLEXUS-1"), &s).is_err());
    // A ticket filed elsewhere, then moved below the granted one: held.
    let four = s
        .create(SYS, crate::store::NewTicket {
            epic: "PLEXUS".into(),
            title: "elsewhere".into(),
            kind: Kind::Build,
            severity: None,
            blocked_by: Vec::new(),
            parent: Some("PLEXUS-1".into()),
            path: None,
            body: String::new(),
        })
        .await
        .unwrap()
        .id;
    assert!(under.permit::<Editor>(&four, &s).is_err());
    s.set_parent(SYS.moving(&four, "PLEXUS-3")).await.unwrap();
    assert!(under.permit::<Editor>(&four, &s).is_ok(), "below PLEXUS-2 now");
    // The granted ticket itself moved (under the one it held): the grant
    // goes with it, its subtree too, and not to its new parent.
    s.set_parent(SYS.moving(&four, "PLEXUS-1")).await.unwrap();
    s.set_parent(SYS.moving("PLEXUS-2", &four)).await.unwrap();
    assert_eq!(permitted(&under, &s), vec!["PLEXUS-2", "PLEXUS-3"]);
    assert!(under.permit::<Editor>(&four, &s).is_err(), "its new parent is not below it");
    // A shortcode's root moved under PLEXUS-2: the shortcode is hosted
    // there now, and a grant on PLEXUS (or under PLEXUS-2) covers it.
    assert!(shortcode.permit::<Editor>(&t("OUGHT-2"), &s).is_err());
    s.set_parent(SYS.moving("OUGHT-1", "PLEXUS-2")).await.unwrap();
    assert!(shortcode.permit::<Editor>(&t("OUGHT-2"), &s).is_ok(), "hosted under PLEXUS now");
    assert!(under.permit::<Editor>(&t("OUGHT-2"), &s).is_ok(), "below PLEXUS-2 now");
    assert!(readable(&under, &s).contains(&"OUGHT-2"), "a read follows it too");
}

#[tokio::test]
async fn team_grants_add_to_direct_ones() {
    let a = login(vec![]);
    let s = store().await;
    s.grant(SYS, grant(ALICE, "hypermemetic:OUGHT", Role::Viewer)).await.unwrap();
    s.grant(SYS, grant("team:crew", "hypermemetic:IDENTITY", Role::Editor)).await.unwrap();
    a.attach(s.clone());
    let me = ctx(json!({}));
    let u = a.verify_now(&me).unwrap();
    assert!(u.permit::<Viewer>(&t("OUGHT-2"), &s).is_ok());
    assert!(u.permit::<Viewer>(&t("IDENTITY-2"), &s).is_err(), "not in the team yet");
    let crew = crate::vocab::TeamName::stored("crew");
    s.set_team_for_tests(ALICE, crew.clone(), true);
    let u = a.verify_now(&me).unwrap();
    assert!(u.permit::<Editor>(&t("IDENTITY-2"), &s).is_ok(), "the team's grant");
    assert!(u.permit::<Viewer>(&t("OUGHT-2"), &s).is_ok(), "and still the direct one");
    assert!(u.permit::<Editor>(&t("OUGHT-2"), &s).is_err(), "each at its own role");
    assert_eq!(u.reach().grants().len(), 2);
    s.set_team_for_tests(ALICE, crew, false);
    assert!(a.verify_now(&me).unwrap().permit::<Viewer>(&t("IDENTITY-2"), &s).is_err(), "left the team");
}

#[tokio::test]
async fn the_generation_moves_on_every_grant_revoke_and_team_change() {
    let a = login(vec![]);
    let s = store().await;
    a.attach(s.clone());
    let me = ctx(json!({}));
    let mut seen = vec![a.verify_now(&me).unwrap().reach().generation()];
    let mut moved = |why: &str| {
        let g = a.verify_now(&me).unwrap().reach().generation();
        assert!(!seen.contains(&g), "{why}: {g} after {seen:?}");
        assert_eq!(g, a.generation());
        seen.push(g);
    };
    s.grant(SYS, grant(ALICE, "hypermemetic:OUGHT", Role::Viewer)).await.unwrap();
    moved("a grant");
    s.grant(SYS, grant(ALICE, "hypermemetic:OUGHT", Role::Editor)).await.unwrap();
    moved("a grant replacing a role");
    s.grant(SYS, grant("idp:00000000-0000-0000-0000-000000000000", "*", Role::Viewer)).await.unwrap();
    moved("someone else's grant");
    s.revoke(SYS, ALICE.parse().unwrap(), kept_scope("hypermemetic:OUGHT")).await.unwrap();
    moved("a revoke");
    s.set_team_for_tests(ALICE, crate::vocab::TeamName::stored("crew"), true);
    moved("a team joined");
    s.set_team_for_tests(ALICE, crate::vocab::TeamName::stored("crew"), false);
    moved("a team left");
    // Nothing that changes no one's grants moves it.
    let before = a.generation();
    assert!(!s.revoke(SYS, ALICE.parse().unwrap(), kept_scope("hypermemetic:OUGHT")).await.unwrap());
    s.set_team_for_tests(ALICE, crate::vocab::TeamName::stored("crew"), false);
    assert_eq!(a.generation(), before);
}

#[tokio::test]
async fn verify_needs_a_principal_and_never_falls_back_to_the_user_id() {
    let a = login(vec![]);
    a.attach(store().await);
    // A user_id the identity layer cannot read as a principal, and no claim.
    let bare = AuthContext::new("user-1".into(), "s".into(), vec![], json!({}));
    assert_eq!(a.verify_now(&bare).unwrap_err(), Refusal::NoPrincipal);
    // A claim it cannot read is no principal either — not the user_id.
    let corrupt = AuthContext::new(ALICE.trim_start_matches("idp:").into(), "s".into(), vec![], json!({ "principal": "idp:not-a-uuid" }));
    assert_eq!(a.verify_now(&corrupt).unwrap_err(), Refusal::NoPrincipal);
    assert!(a.waiting().is_empty(), "no one to wait");
    assert_eq!(a.verify_now(&AuthContext::anonymous()).unwrap_err(), Refusal::SignIn);
    let u = a.verify_now(&ctx(json!({ "name": "alice" }))).unwrap();
    assert_eq!((u.principal().map(|p| p.as_str()), u.name()), (Some(ALICE), Some("alice")));
    // Login off: anyone, everything — no principal at all.
    let s = store().await;
    let anyone = Access::open().verify_now(&AuthContext::anonymous()).unwrap();
    assert!(anyone.principal().is_none());
    assert!(anyone.permit::<Admin>(&t("OUGHT-2"), &s).is_ok());
    assert!(anyone.admin_of(&Scope::Everywhere, &s).is_ok());
}

/// ACCESS-13: the frame's resolver — `verify_now` for the call being
/// dispatched, its refusal typed for the wire (MACRO-14), the gate's reach
/// for the call kept on the user (what a watch follows).
#[tokio::test]
async fn the_frames_resolver_answers_typed_and_keeps_the_calls_reach() {
    use plexus_core::plexus::PlexusError;
    let a = login(vec![]);
    a.attach(store().await);
    match a.verify(&AuthContext::anonymous()).await {
        Err(PlexusError::Unauthenticated(m)) => assert!(m.contains("sign in first"), "{m}"),
        other => panic!("anonymous, login on: {other:?}"),
    }
    let bare = AuthContext::new("user-1".into(), "s".into(), vec![], json!({}));
    assert!(matches!(a.verify(&bare).await, Err(PlexusError::Unauthenticated(_))), "no principal is no sign-in");
    let u = a.verify(&ctx(json!({ "name": "alice" }))).await.expect("alice signs in");
    assert_eq!(u.principal().map(|p| p.as_str()), Some(ALICE));
    assert!(u.context().is_some() && u.gate_reach().is_some(), "the identity and the call's reach, kept");
    // Login off: anyone, whatever identity the transport handed it.
    let anyone = Access::open().verify(&AuthContext::anonymous()).await.expect("login off admits anyone");
    assert!(anyone.principal().is_none() && anyone.gate_reach().is_none());
    // Every other refusal is `forbidden`.
    let e: PlexusError = Refusal::Lacks("no".into()).into();
    assert!(matches!(e, PlexusError::Forbidden { .. }), "{e:?}");
}

#[tokio::test]
async fn admin_of_is_held_on_the_scope_or_one_covering_it() {
    let a = login(vec![]);
    let s = store().await;
    s.grant(SYS, grant(ALICE, "hypermemetic:PLEXUS", Role::Admin)).await.unwrap();
    s.grant(SYS, grant(ALICE, "hypermemetic:OUGHT", Role::Editor)).await.unwrap();
    a.attach(s.clone());
    let u = a.verify_now(&ctx(json!({}))).unwrap();
    let shortcode = |id: &str| Scope::Shortcode { org: org(), id: ShortcodeId::new(id) };
    assert_eq!(u.admin_of(&shortcode("PLEXUS"), &s).unwrap().scope(), &shortcode("PLEXUS"));
    assert!(u.admin_of(&shortcode("IDENTITY"), &s).is_ok(), "hosted under PLEXUS");
    assert!(matches!(u.admin_of(&shortcode("OUGHT"), &s), Err(Refusal::Lacks(_))), "editor, not admin");
    assert!(u.admin_of(&Scope::Org(org()), &s).is_err());
    assert!(u.admin_of(&Scope::Everywhere, &s).is_err());
    // It answers as the gate does.
    for (scope, gate) in [(shortcode("PLEXUS"), true), (shortcode("IDENTITY"), true), (shortcode("OUGHT"), false), (Scope::Everywhere, false)] {
        assert_eq!(u.administers(&scope.text()), gate, "{scope}");
        assert_eq!(u.admin_of(&scope, &s).is_ok(), gate, "{scope}");
    }
    // The operator's admin on `*`, held to a ceiling: no wider than it.
    let op = login(vec![ALICE.into()]);
    op.attach(s.clone());
    let held = op.verify_now(&ctx(json!({ CEILING_KEY: ceiling(&[&["admin@hypermemetic:IDENTITY"]]) }))).unwrap();
    assert!(held.admin_of(&shortcode("IDENTITY"), &s).is_ok());
    assert!(held.admin_of(&shortcode("PLEXUS"), &s).is_err());
    assert!(op.verify_now(&ctx(json!({}))).unwrap().admin_of(&Scope::Everywhere, &s).is_ok());
}

#[tokio::test]
async fn a_ceiling_on_a_ticket_holds_permits_to_its_subtree() {
    let a = login(vec![ALICE.into()]);
    let s = deeper().await;
    a.attach(s.clone());
    let u = a.verify_now(&ctx(json!({ CEILING_KEY: ceiling(&[&["editor@PLEXUS-2"]]) }))).unwrap();
    assert_eq!(permitted(&u, &s), vec!["PLEXUS-2", "PLEXUS-3"]);
    assert!(u.permit::<Admin>(&t("PLEXUS-2"), &s).is_err(), "no more than editor");
    // A ceiling's single grant is no role: it gives no permit.
    let g = a.verify_now(&ctx(json!({ CEILING_KEY: ceiling(&[&["tickets.get@*"]]) }))).unwrap();
    assert!(permitted(&g, &s).is_empty());
}

#[tokio::test]
async fn the_reach_key_moves_with_the_grants_and_is_compared_not_formatted() {
    let a = login(vec![]);
    let s = store().await;
    s.grant(SYS, grant(ALICE, "hypermemetic:OUGHT", Role::Viewer)).await.unwrap();
    a.attach(s.clone());
    let me = ctx(json!({}));
    let list = reach_of(&a, &me, "tickets.list");
    let before = a.verify_call(Some(&me), &list).unwrap().reach_key().unwrap();
    assert_eq!(a.verify_call(Some(&me), &list).unwrap().reach_key().unwrap(), before, "as it was");
    s.grant(SYS, grant(ALICE, "hypermemetic:IDENTITY", Role::Viewer)).await.unwrap();
    assert_ne!(a.verify_call(Some(&me), &list).unwrap().reach_key().unwrap(), before);
    s.grant(SYS, grant(ALICE, "*", Role::Viewer)).await.unwrap();
    assert_eq!(a.verify_call(Some(&me), &list).unwrap().reach_key().unwrap(), None, "everywhere");
}

#[tokio::test]
async fn the_store_hands_out_its_system_once() {
    let s = Store::new();
    assert!(s.system().is_some());
    assert!(s.system().is_none(), "a second taker gets nothing");
    // The access layer takes it at attach, when it is still there.
    let a = login(vec![]);
    let fresh = store().await;
    a.attach(fresh.clone());
    assert!(fresh.system().is_none());
    assert!(a.book().system.get().is_some());
}

/// ACCESS-12: whether a [`Following`]'s wake comes within `ms`.
async fn wakes(f: &mut Following, ms: u64) -> bool {
    tokio::time::timeout(std::time::Duration::from_millis(ms), f.changed()).await.is_ok()
}

#[tokio::test]
async fn a_followed_caller_is_verified_again_only_when_the_grants_move() {
    let a = login(vec![]);
    let s = store().await;
    s.grant(SYS, grant(ALICE, "OUGHT", Role::Viewer)).await.unwrap();
    a.attach(s.clone());
    let me = ctx(json!({}));
    let mut f = Following::open(&a, &s, Some(&me), &reach_of(&a, &me, "tickets.list")).unwrap();
    // Nothing moved: no wake, the same caller.
    assert!(!wakes(&mut f, 50).await);
    assert!(matches!(f.again(), Ok(Followed::Same)));
    // A ticket write moves no grant: still no wake.
    s.amend(SYS.permit("OUGHT-2"), Some("t".into()), None).await.unwrap();
    assert!(!wakes(&mut f, 50).await);
    // A grant: woken, verified again, the reach moved.
    s.grant(SYS, grant(ALICE, "IDENTITY", Role::Viewer)).await.unwrap();
    assert!(wakes(&mut f, 500).await, "a grant wakes the watch");
    assert!(matches!(f.again(), Ok(Followed::Moved { .. })));
    assert_eq!(f.user().reach_filter().unwrap().unwrap().format(), "under in (IDENTITY, OUGHT)");
    // A role's grants edited moves the generation too.
    let before = s.grants_generation();
    s.set_role(SYS, "viewer".into(), vec!["tickets.get".into(), "tickets.list".into()]).await.unwrap();
    assert!(s.grants_generation() > before);
    assert!(wakes(&mut f, 500).await);
    assert!(matches!(f.again(), Ok(Followed::Same)), "the same reach: nothing to recompute");
    // Every grant revoked: the watch ends, typed.
    s.revoke(SYS, ALICE.parse().unwrap(), kept_scope("OUGHT")).await.unwrap();
    s.revoke(SYS, ALICE.parse().unwrap(), kept_scope("IDENTITY")).await.unwrap();
    assert!(wakes(&mut f, 500).await);
    let closed = f.again().unwrap_err();
    assert_eq!(closed.reason, crate::types::WatchEnd::Revoked, "{closed:?}");
    assert!(closed.message.starts_with("your access changed"), "{closed:?}");
}

#[tokio::test]
async fn a_followed_caller_signed_out_or_expired_ends_typed() {
    let live = Arc::new(Live::default());
    live.0.lock().unwrap().insert("k1".into());
    let a = Access::with_login(live.clone(), true, vec![]);
    a.book().seed(declared());
    let s = store().await;
    s.grant(SYS, grant(ALICE, "OUGHT", Role::Viewer)).await.unwrap();
    a.attach(s.clone());
    // Signed out in the table: the next change it hears ends it.
    let me = ctx(json!({ SESSION_KEY: "k1" }));
    let mut f = Following::open(&a, &s, Some(&me), &reach_of(&a, &me, "tickets.list")).unwrap();
    assert!(matches!(f.again(), Ok(Followed::Same)));
    live.0.lock().unwrap().clear();
    assert_eq!(f.again().unwrap_err().reason, crate::types::WatchEnd::SignedOut);
    // Its expiry comes: woken then, and ended.
    let soon = ctx(json!({ "expires_at": now() + 1 }));
    let mut f = Following::open(&a, &s, Some(&soon), &reach_of(&a, &soon, "tickets.list")).unwrap();
    assert!(wakes(&mut f, 3000).await, "the expiry wakes it");
    assert_eq!(f.again().unwrap_err().reason, crate::types::WatchEnd::Expired);
    // Login off: never woken, never refused.
    let open = Access::open();
    let mut f = Following::open(&open, &s, None, &GateReach::everywhere()).unwrap();
    assert!(!wakes(&mut f, 50).await);
    assert!(matches!(f.again(), Ok(Followed::Same)));
}
