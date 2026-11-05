//! V2-4, acceptance 6: the seam. Every other service reads tickets through
//! the tickets service — in the same process, through plexus-core's
//! in-memory transport (`HubTransport::as_`, LOCAL-6) and the client the
//! `tickets` activation emits (`TicketsClient`), as the caller it serves.
//! So a second activation, registered beside `tickets` in a service built as
//! `main` builds it (the gate, the roles, the book over the store), reads a
//! ticket its caller may read, and is answered `not_found` for one it may
//! not: the tickets service decides, by the caller's reach, whoever asks.
//!
//! The caller is a key holding `viewer` on one shortcode (`SEEN`), and
//! nothing on another (`ELSE`). Its own throwaway database.

use async_stream::stream;
use futures::Stream;
use menger::auth_backend::MengerAuth;
use menger_tickets_api::access::Access;
use menger_tickets_api::members::{Grant, Role};
use menger_tickets_api::store::{NewTicket, Store};
use menger_tickets_api::vocab::{GrantedBy, Grantee, TicketRef};
use menger_tickets_api::{Kind, TicketsActivation, TicketsClient, TicketsEvent};
use plexus_client::ClientError;
use plexus_core::plexus::{AuthContext, AuthSetup, DynamicHub, HubTransport};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock, Weak};

/// The reader: a key that holds `viewer` on `SEEN` alone.
const READER: [u8; 32] = [0x5e; 32];

fn principal_of(seed: &[u8; 32]) -> String {
    use ed25519_dalek::SigningKey;
    plexus_auth_core::adapters::key_principal(SigningKey::from_bytes(seed).verifying_key().as_bytes()).to_string()
}

/// What the probe answers: what `tickets.get` said to it, and as whom it
/// asked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProbeEvent {
    /// `tickets.get` answered the ticket.
    Ticket { id: String, asked_as: Option<String> },
    /// `tickets.get` answered `not_found`.
    NotFound { id: String, asked_as: Option<String> },
    /// Anything else, as it came.
    Error { message: String },
}

impl plexus_client::FromClientError for ProbeEvent {
    fn from_client_error(error: ClientError) -> Self {
        ProbeEvent::Error { message: error.to_string() }
    }
}

/// A second service, in miniature: one read that asks `tickets` for a
/// ticket through the service it is registered in, as its own caller.
#[derive(Clone)]
pub struct ProbeActivation {
    service: Arc<OnceLock<Weak<DynamicHub>>>,
}

#[plexus_macros::activation(version = "1.0.0", description = "V2-4: a second activation that reads through the seam", roles(viewer = ["read"]))]
impl ProbeActivation {
    /// Read one ticket through the seam, as the caller.
    #[plexus_macros::method]
    pub async fn read(&self, auth: Option<&AuthContext>, id: String) -> impl Stream<Item = ProbeEvent> + Send + 'static {
        let service = self.service.get().and_then(Weak::upgrade);
        let caller = auth.cloned();
        stream! {
            let (Some(service), Some(caller)) = (service, caller) else {
                yield ProbeEvent::Error { message: "no service, or no caller".into() };
                return;
            };
            let asked_as = caller.principal().map(|p| p.to_string());
            let tickets = TicketsClient::new(HubTransport::as_(service, caller));
            let r#ref: TicketRef = match id.parse() {
                Ok(r) => r,
                Err(e) => {
                    yield ProbeEvent::Error { message: format!("{e:?}") };
                    return;
                }
            };
            match tickets.get_all(r#ref).await {
                Ok(events) => match events.as_slice() {
                    [TicketsEvent::Ticket { ticket }] => yield ProbeEvent::Ticket { id: ticket.id.to_string(), asked_as },
                    [TicketsEvent::NotFound { .. }] => yield ProbeEvent::NotFound { id, asked_as },
                    other => yield ProbeEvent::Error { message: format!("{other:?}") },
                },
                Err(e) => yield ProbeEvent::Error { message: e.to_string() },
            }
        }
    }
}

/// The service as `main` builds it — the store opened, the access layer
/// attached, the gate and `auth` over the book, the declared roles seeded —
/// with the probe registered beside `tickets`. `SEEN-1`, `SEEN-2`,
/// `ELSE-1` and `ELSE-2` exist; [`READER`] holds `viewer` on `SEEN`.
async fn service(dir: &std::path::Path) -> Arc<DynamicHub> {
    let login = Login::new();
    let access = login.access.clone();
    let store = Store::open(&dir.join("tickets.db")).expect("open");
    let system = Arc::new(store.system().expect("the store's System"));
    for (epic, title, kind, parent) in [
        ("SEEN", "seen", Kind::Scope, None),
        ("SEEN", "a seen build", Kind::Build, Some("SEEN-1")),
        ("ELSE", "elsewhere", Kind::Scope, None),
        ("ELSE", "a build elsewhere", Kind::Build, Some("ELSE-1")),
    ] {
        let new = NewTicket { path: None, epic: epic.into(), title: title.into(), kind, severity: None, blocked_by: Vec::new(), parent: parent.map(Into::into), body: String::new() };
        store.create(&*system, new).await.expect("create");
    }
    let scope = store.scope_named("SEEN").expect("SEEN is a scope");
    let grant = Grant { principal: Grantee::try_from(principal_of(&READER).as_str()).unwrap(), scope, role: Role::Viewer, granted_by: GrantedBy::Operator, at: 1 };
    store.grant(&*system, grant).await.expect("grant");

    let store = Arc::new(store);
    access.attach_with(Arc::clone(&store), system);
    let probe = ProbeActivation { service: Arc::new(OnceLock::new()) };
    let backend = Arc::new(MengerAuth::new(access.clone(), Vec::new(), None, menger::login_access::BRANDING.app));
    let book = access.book();
    let service = DynamicHub::new("menger")
        .register(TicketsActivation::with_access(Arc::clone(&store), access.clone()))
        .register(probe.clone())
        .with_auth(AuthSetup::new(backend, book.clone(), book.clone(), book.clone()))
        .with_signed_in_grants(menger::SIGNED_IN_GRANTS);
    book.seed(service.declared_roles());
    let service = Arc::new(service);
    probe.service.set(Arc::downgrade(&service)).expect("set once");
    service
}

/// The login a test service runs ([`support::Login`]'s, kept here so this file
/// needs nothing of the socket).
struct Login {
    access: Access,
}

impl Login {
    fn new() -> Self {
        let sessions = Arc::new(menger::sessions::Sessions::new(3600, &menger::login_names()));
        let table = menger::session_table(&sessions);
        Self { access: Access::with_login(table.clone(), Vec::new()).minting(table) }
    }
}

/// [`READER`], as the transport carries a caller.
fn reader() -> AuthContext {
    AuthContext::for_principal(&principal_of(&READER).parse().unwrap(), "seam", vec![], serde_json::json!({ "name": "reader" }))
}

/// The probe's answer to `read { id }`, called as [`READER`].
async fn probe(service: &Arc<DynamicHub>, id: &str) -> Vec<ProbeEvent> {
    ProbeClient::new(HubTransport::as_(Arc::clone(service), reader())).read_all(id.to_string()).await.expect("the probe answers")
}

#[tokio::test]
async fn a_second_activation_reads_through_the_seam_as_its_caller() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let service = service(dir.path()).await;
    let me = Some(principal_of(&READER));
    assert_eq!(probe(&service, "SEEN-2").await, [ProbeEvent::Ticket { id: "SEEN-2".into(), asked_as: me.clone() }]);
    assert_eq!(probe(&service, "SEEN-1").await, [ProbeEvent::Ticket { id: "SEEN-1".into(), asked_as: me }]);
}

#[tokio::test]
async fn and_is_refused_outside_its_reach() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let service = service(dir.path()).await;
    let me = Some(principal_of(&READER));
    assert_eq!(probe(&service, "ELSE-2").await, [ProbeEvent::NotFound { id: "ELSE-2".into(), asked_as: me.clone() }]);
    // The same answer as a ticket that does not exist: not found ≡ not yours.
    assert_eq!(probe(&service, "ELSE-9").await, [ProbeEvent::NotFound { id: "ELSE-9".into(), asked_as: me }]);
}
