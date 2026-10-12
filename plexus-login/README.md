# plexus-login

Login for a [Plexus RPC](https://github.com/hypermemetic-ai) hub: identity
provider specs, a login page and its routes, server-side sessions, the
WebSocket gate, and the protocol's `auth` backend. It is generic over one
trait, `LoginAccess`, which carries the app's access rules.

It is menger's login, lifted as it is (SCAFFOLD-3) and named for whichever
app takes it (SCAFFOLD-9). menger runs on it, and so can any other hub. A
new app gets login in about a dozen lines with the ready-made rules,
`SimpleAccess`.

```toml
plexus-login = "0.1"
```

## What it does

- **Providers become adapters.** Each `--auth-provider` spec is parsed into
  a `Provider`, and its plexus-auth-core adapter verifies that provider's
  credential at the WebSocket upgrade. A caller ends up with a Plexus
  identity (`idp:<uuid>`, `oidc:<provider>/<sub>`, `nostr:<hex>`,
  `key:<hex>`) whichever way it came in.
- **The login page** (`/login`) offers several ways in: an email field
  handed to the IdP, a button per provider, nostr through a NIP-07
  extension, and a password form. Each way in ends the same way. The
  provider's token is verified in the shell, a session starts, and the
  browser gets an `HttpOnly; SameSite=Lax` session cookie. No script on any
  page can read a credential.
- **Sessions** live in memory, keyed by a hash. They end at their expiry or
  at sign-out, and a signed-out session's open sockets stop answering.
- **The gate** is the upgrade's validator. It tries the session cookie
  first, then a minted `mgs_…` session token, then a provider's own
  credential. A connection with no credential is admitted into the
  **anonymous view**, where only `auth.providers`, `auth.login` and
  `auth.signup` answer. A credential that is presented and refused gets a
  401 with RFC 6750's challenge.
- **The auth backend** answers plexus-core's `auth` activation:
  - `auth.login {browser}`: a loopback sign-in through the page;
  - `{code}`: trades the page's one-time code for a session;
  - `{session}`: mints a session held to a ceiling;
  - `auth.signup`: someone holding nothing joins the waiting list;
  - `auth.logout`.

## Login in a dozen lines: `Wiring` and `SimpleAccess`

```rust
use std::sync::Arc;
use plexus_core::plexus::DynamicHub;
use plexus_login::{Branding, LoginOptions, Wiring, simple::SimpleAccess};
use plexus_transport::TransportServer;

const BRANDING: Branding = Branding { app: "myapp", version: env!("CARGO_PKG_VERSION"), mark: "m" };

let opts = LoginOptions::new(BRANDING, ws_port, shell_port)
    .with_providers(["workos=client_01ABC…"])?;
let opts = LoginOptions { email: Some("email=workos:authkit".parse().map_err(anyhow::Error::msg)?), ..opts };
let wiring = Wiring::new(opts)?;                                        // providers checked, sessions made
let access = SimpleAccess::new(wiring.sessions(), &admins)?;            // in-memory rules, roles global
let wired = wiring.finish(access.clone(), None)?;                       // page, gate, auth backend
let hub = DynamicHub::new("myapp").with_auth(access.auth_setup(wired.backend.clone()));
// … register the app's activations on `hub` …
access.seed_roles(&hub);
let server = wired.apply(TransportServer::builder(Arc::new(hub), into_rpc).with_websocket(ws_port));
let shell = my_pages.merge(wired.router().expect("a provider: a page"));
```

`tests/simple_login.rs` is this, run. A fixture issuer stands in for
plexus-idp, and the test goes through the page's password form, the
session cookie (`myapp_session_<port>`), the gate, `auth.whoami`, the
waiting list, an admin's `auth.grant` and sign-out.
`examples/toy.rs` does the same against a real plexus-idp.

### `Wiring`: two phases

```rust
pub struct LoginOptions {
    pub providers: Vec<Provider>,        // --auth-provider (none: login off)
    pub buttons: Vec<LoginButton>,       // --login-button
    pub email: Option<LoginButton>,      // --login-email
    pub password: Option<String>,        // --login-password (`none`: no form)
    pub forgot: Option<String>,          // --login-forgot
    pub ws_port: u16,
    pub shell_port: u16,
    pub public_url: Option<String>,      // --public-url (default http://127.0.0.1:<shell_port>)
    pub allowed_origins: Vec<String>,    // --allowed-origin
    pub session_ttl_secs: i64,           // default 12 h
    pub require: bool,                   // --require-login
    pub page: bool,                      // a shell to serve the page on (false: --no-shell, stdio)
    pub title: &'static str,
    pub branding: Branding,
    pub names: LoginNames,               // default LoginNames::for_app(branding.app)
}
impl LoginOptions {
    pub fn new(branding: Branding, ws_port: u16, shell_port: u16) -> Self;
    pub fn with_providers<I: IntoIterator<Item = S>, S: AsRef<str>>(self, specs: I) -> anyhow::Result<Self>;
}

impl Wiring {
    pub fn new(opts: LoginOptions) -> anyhow::Result<Wiring>;
    pub fn sessions(&self) -> &Arc<Sessions>;     // what the app's rules hold
    pub fn login_on(&self) -> bool;
    pub fn public_url(&self) -> &str;
    pub fn shell_origins(&self) -> &[String];
    pub fn finish<A: LoginAccess>(self, access: A, dev: Option<DevLogin<A>>) -> anyhow::Result<Wired<A>>;
}

pub struct Wired<A: LoginAccess> {
    pub sessions: Arc<Sessions>,
    pub login: Option<Arc<Login<A>>>,     // the page (login on, page true)
    pub backend: Arc<LoginAuth<A>>,       // DynamicHub::with_auth(AuthSetup::new(backend, …))
    pub gate: Option<Arc<Gate>>,          // the upgrade's validator (login on)
    pub protected: ProtectedResource,     // RFC 9728: the socket and its issuers
    pub origins: Vec<String>,             // the shell's own + --allowed-origin
    pub public_url: String,
    pub providers: Vec<Provider>,
    pub access: A,
}
impl<A: LoginAccess> Wired<A> {
    pub fn apply<H: Activation>(&self, b: TransportServerBuilder<H>) -> TransportServerBuilder<H>;
    pub fn router(&self) -> Option<axum::Router>;   // every /login… route
    pub fn metadata_url(&self) -> Option<String>;   // for the hub's 401s
    pub fn offered(&self) -> impl Iterator<Item = &Provider>;   // all but `key`
    pub fn login_on(&self) -> bool;
}
```

There are two phases because an app's rules usually hold the session table:
they ask it whether a session still stands, and they mint sessions in it. The
page, the gate and the backend all hold the rules. `Wiring::new` checks the
providers and makes the table. A bad spec, two providers with one name, an
issuer that is not `https://` off loopback, or `require` with no provider all
stop startup there. `Wiring::finish` takes the rules and makes everything
else.

`apply` always sets the allowed origins. Only the shell's own pages, plus
any named with `--allowed-origin`, may open the socket from a browser; a
client such as synapse sends no Origin and is not affected. With login on,
`apply` also sets the gate, the anonymous view, `access_token` as the one
credential cookie and the protected resource. With no provider, login is
off: there is no gate and no page, and nothing is refused.

A shell that mounts the routes itself (menger's `shell.rs`) takes
`wired.login`. For `--require-login` redirects it uses `Login::session`
(who a request's cookie is), and for Host checks it uses `Login::hosts`.

## The seam: `LoginAccess`

The login layer knows who someone is. The app decides what they may do,
through one trait:

```rust
#[async_trait]
pub trait LoginAccess: Clone + Send + Sync + 'static {
    type Role: Copy + Eq + Debug + Display + FromStr<Err = String> + Send + Sync + 'static;
    fn scope_of(text: &str) -> Result<String, String>;                      // a grant's scope, as the app keeps it
    async fn dev_grant(&self, principal: &str, scope: &str, role: Self::Role) -> Result<(), String>;
    async fn dev_set_grants(&self, principal: &str, grants: &[(Self::Role, String)]) -> Result<(), String>;
    fn notice(&self, ctx: &AuthContext);                                    // the gate: holding nothing → waiting list
    fn alive(&self, ctx: &AuthContext) -> Result<(), String>;               // not expired, not signed out
    fn mint(&self, caller: Option<&AuthContext>, ttl_secs: Option<u64>, ceiling: Option<Vec<String>>) -> Result<Session, String>;
    fn mint_for(&self, ctx: AuthContext, ttl_secs: u64) -> Result<Session, String>;
    fn ask_access(&self, ctx: &AuthContext) -> Result<Waiting, String>;
    fn waiting(&self) -> Vec<Waiting>;
    fn logout(&self, caller: Option<&AuthContext>) -> Result<(), String>;
    fn session_key(&self) -> Option<&str>;                                  // the table's key, never a copy
}
```

`Session` and `Waiting` are plexus-core's own types. `Role` and `scope_of`
are the dev sign-in's vocabulary: its roster (`name=role@SCOPE`) and its
form.

| implementation | rules |
| --- | --- |
| `simple::SimpleAccess` | ships with the crate. It is in memory, over plexus-core's `MemoryRoles` and `MemoryMemberships`. Roles are global: every grant is on `*`. The roles are `viewer`, `editor` and `admin` (`SimpleRole`). Admins are named at startup, and everyone else who signs in waits for one: `auth.waiting`, then `auth.grant`. A restart forgets every grant except the named admins'. |
| menger's `MengerAccess` | menger's members table, with roles per shortcode, its waiting list, and its sessions. It lives in `src/login_access.rs`, and is a newtype because of the orphan rule. |

## Names: `LoginNames` and `Branding`

Nothing the crate writes into a browser or an identity carries a fixed
name. All of it comes from the app:

| `LoginNames` field | `for_app("x")` | where |
| --- | --- | --- |
| `session_cookie` | `x_session` | the session cookie: `x_session_<port>` over http, `__Host-x_session` over https (IDENTITY-38: two servers on one host keep apart) |
| `login_cookie` | `x_login` | a code-flow sign-in's transaction cookie: `x_login_<port>`, or `__Secure-x_login` |
| `dev_cookie` | `x_dev_login` | the dev sign-in's checkbox (the page's script writes it) |
| `session_key` | `x_session` | the metadata key a session-backed identity carries: the session's hash, which `alive` asks the table about |

`session_key` has one definition: the session table's
(`Sessions::session_key`). Rules that read it ask the table: menger's
tickets-api through `SessionCheck::key`, and the auth backend through
`LoginAccess::session_key`.

`Branding { app, version, mark }` is what the page says of the app: its
title, the version in the side column, the brand mark, the theme key, and
"sign in from *app*'s own page". The auth backend's refusals use the same
name ("this *app* has no login page …").

## Providers

| `--auth-provider` | the credential | principal |
| --- | --- | --- |
| `idp=<issuer>,<audience>[,client=<id>]` | a plexus-idp token | `idp:<uuid>` |
| `auth0=<domain>,<audience>[,<roles-claim>][,client=<id>]` | an Auth0 access token | `oidc:auth0/<sub>` |
| `oidc:<name>=<issuer>,<audience>[,<roles-claim>][,client=<id>]` | any OIDC provider's RS256 token | `oidc:<name>/<sub>` |
| `workos=<client-id>[,<api-base>]` | a WorkOS access token (AuthKit, social, SSO) | `oidc:workos/<user id>` |
| `nostr=<ws-url>[,<ws-url>…]` | a NIP-98 event naming that URL | `nostr:<hex>` |
| `key` or `key=<ws-url>[,…]` | an agent's Ed25519 key proof naming this socket. It is not offered on the page. | `key:<hex>` |

**WorkOS** is the default IdP for a new app. Give it its client id, then an
email field and buttons:

```sh
--auth-provider workos=client_01…
--login-email workos:authkit                 # the address goes to AuthKit as login_hint
--login-button Google=workos:GoogleOAuth     # also AppleOAuth, MicrosoftOAuth, GitHubOAuth, conn_…, org_…
```

An SSO domain goes straight to its company's IdP. Anyone else finishes on
AuthKit's page (password, passkey, or a code by email) with the address
already filled in. The code flow runs with PKCE and no secret, so the app
holds no WorkOS API key.

WorkOS's access token names nobody. The person's name and email come from
the authenticate response's `user`, which must be the token's own subject.
Signing out ends the WorkOS session too. The WorkOS environment must list
`<public-url>/login/callback` as a redirect URI and `<public-url>/login` as
a logout URI.

**Buttons** take the form `--login-button <Label>=<provider>[:<connection>]`.
The provider needs a code-flow client (`client=<id>`; WorkOS's client is its
spec). Auth0 and generic OIDC take a `connection`, for example
`Google=auth0:google-oauth2` or `SSO=auth0:samlp`. A connection naming
`saml`/`sso`, or a button labelled `SSO`, asks for a work email first.

Beside an email field, the **password form** shows only at
`/login?password`. That is automation's way in through plexus-idp's password
grant.

**The dev sign-in** (`DevLogin`, IDENTITY-35) is for test environments only,
and the app decides where it is allowed. It adds a checkbox that sends this
browser's sign-in to plexus-idp. There, each seeded user (`name=role@SCOPE`)
is one click, and any username can be signed in with any role. Accounts are
registered at plexus-idp over its socket as needed, and grants are written
through `LoginAccess::dev_grant` / `dev_set_grants`.

## Routes

| route | |
| --- | --- |
| `GET /login` | the page; `?password` shows the password form beside an email field; `?cli=1&state=…` is a loopback sign-in |
| `POST /login` | the password grant |
| `GET /login/start`, `GET /login/callback` | the authorization code flow with PKCE |
| `POST /login/nostr`, `POST /login/nostr/trace` | NIP-98 sign-in, and its browser-side log |
| `GET /login/me` | who the session is, or 401 |
| `POST /login/dev` | the dev sign-in |
| `POST /logout` | sign-out (and WorkOS's) |
| `GET`/`POST /auth/cli` | the loopback sign-in's hand-back to a client (`auth.login {browser}`) |

A POST from another origin is refused.

## License

MIT
