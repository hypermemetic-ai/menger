# plexus-login

Login for a [Plexus RPC](https://github.com/hypermemetic-ai) hub: our own
sign-in page over a swappable identity backend (WorkOS, plexus-idp, …),
identity provider specs, a login page and its routes, server-side sessions, the
WebSocket gate, and the protocol's `auth` backend. It is generic over one
trait, `LoginAccess`, which carries the app's access rules.

It is menger's login, lifted as it is (SCAFFOLD-3) and named for whichever
app takes it (SCAFFOLD-9). menger runs on it, and so can any other hub. A
new app gets login in about a dozen lines with the ready-made rules,
`SimpleAccess`.

```toml
plexus-login = "0.2"
```

## What it does

- **Providers become adapters.** Each `--auth-provider` spec is parsed into
  a `Provider`, and its plexus-auth-core adapter verifies that provider's
  credential at the WebSocket upgrade. A caller ends up with a Plexus
  identity (`idp:<uuid>`, `oidc:<provider>/<sub>`, `nostr:<hex>`,
  `key:<hex>`) whichever way it came in.
- **Our own sign-in page** (AUTH-22) over an **identity backend** chosen by
  configuration — WorkOS, plexus-idp, or in-process memory — offering
  exactly what the backend can do: email codes, passwords, social and SSO
  round trips, TOTP. See the next section.
- **The login page** (`/login`) also offers the older ways in: an email
  field handed to the IdP, a button per provider, nostr through a NIP-07
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

## Our own sign-in page: identity backends (AUTH-22)

The page people sign in on is ours, whoever stands behind it. Who signs
them in is an **identity backend**, one trait the page, its routes, its
throttles, its CSRF and return rules and the session code depend on, and
nothing else of any provider:

```rust
#[async_trait]
pub trait IdentityBackend: Send + Sync + 'static {
    fn name(&self) -> &str;                                   // "workos", "plexus-idp", "memory"
    fn capabilities(&self) -> Capabilities;                   // what the page offers
    fn gate_provider(&self) -> Option<Provider>;              // its tokens at the socket (an OIDC adapter)
    async fn send_email_code(&self, email, req) -> IdResult<()>;
    async fn verify_email_code(&self, email, code, req) -> IdResult<SignedIn>;
    async fn password(&self, username, password, req) -> IdResult<SignedIn>;
    async fn sso_for_domain(&self, domain) -> IdResult<Option<Federated>>;
    fn federated_url(&self, target, redirect_uri, state, pkce_challenge, login_hint) -> IdResult<String>;
    async fn complete_federated(&self, code, verifier, redirect_uri, req) -> IdResult<SignedIn>;
    async fn continue_with_code(&self, ticket, code, req) -> IdResult<SignedIn>;   // MFA, email verification, extra checks
    async fn choose(&self, ticket, choice, req) -> IdResult<SignedIn>;             // an organization
    async fn totp_enroll / totp_confirm / totp_factors / totp_remove / totp_discard(…);
    async fn refresh(&self, tokens, req) -> IdResult<SignedIn>;
    async fn sign_out(&self, tokens) -> IdResult<()>;
}
```

Every type crossing it is neutral (`src/identity.rs`): `Capabilities`,
`Social` (`google`, `github`, `apple`, `microsoft`), `Federated` (a social
provider, an SSO organization or connection), `SignedIn` (the verified
identity, the user, the backend's tokens), an opaque `Ticket` for a step the
backend must remember between two page requests (kept server side, never
read by the page), and typed refusals:

| `IdentityError` | the page |
| --- | --- |
| `InvalidCode` | "That code didn't work" — the same for an address with no account; counts against the throttles |
| `RateLimited` | "Too many attempts" |
| `MfaRequired { ticket }` | asks for a TOTP code |
| `MfaEnrollmentRequired { ticket, enrolment }` | shows the new factor's key once, then asks for its code |
| `EmailUnverified { ticket }` / `ExtraCheck { ticket }` | asks for the emailed code |
| `Choose { ticket, choices }` | a button per organization |
| `FederatedRequired { target }` | sends the browser there |
| `MethodsRequired { methods }` | names the ways in the organization allows |
| `Unsupported` | "That way of signing in isn't offered here" |
| `Misconfigured` / `Unavailable` | "Sign-in is unavailable right now"; the operator's log says why |

**The capability model.** The page draws only what `capabilities()` says:
an email-code backend gets the email field, a password backend the
password form, a button per social provider it lists, the SSO lookup when
`sso_by_domain`, the TOTP steps and `/login/account` when `totp`, the
refresher when `refresh`. A route a backend cannot serve answers as if it
were not there.

| backend (`--identity`) | email code | password | social | SSO by domain | TOTP | refresh / revoke |
| --- | --- | --- | --- | --- | --- | --- |
| `workos:<client_…>[,social=google+github+apple+microsoft][,sso=off][,base=<url>][,key=@<file>]` | Magic Auth | — | the listed providers, straight to the provider (`provider=GoogleOAuth`…), no AuthKit screen | a **verified** domain with an **active** SAML/OIDC connection | enrol, challenge, remove; required-MFA enrolment at sign-in | yes / `sessions/revoke` |
| `plexus-idp:<issuer>,<audience>` | — (it sends no email) | the password grant | — | — | — | yes / nothing to revoke |
| `memory[:outbox=<file>][,mfa=required]` | to an outbox (and `Memory::last_code`) | — | simulated, approved at once | — | real RFC 6238 | — |

`memory` and `plexus-idp` are for tests and local development and are refused unless the app allows them (`LoginOptions::allow_development_backends`; menger: `--env test`, AUTH-24 #8): `memory` signs anyone in.
WorkOS's API key never rides the spec (a flag shows in `ps`): it comes from
`<APP>_WORKOS_API_KEY` or the file `key=@<path>` names, and a spec holding
`key=sk_…` is refused without echoing it.

**Hot-swappable.** The backend is configuration, never code: `--identity`
(menger's `MENGER_IDENTITY`) is parsed by `backends::IdentitySpec` and built
at startup; switching needs no rebuild. A running page can also be moved
onto another backend with `Wired::swap_identity(backend)`
(`SignInPage::swap`): sign-ins midway start again (their tickets were the old
backend's), sessions already made stand and keep refreshing and signing out
through the backend that made them (each session's tokens remember it), and
an unconfirmed factor is still deleted where it was enrolled. The gate's
providers are fixed at startup, so a new backend's tokens as a *bearer* are
admitted after a restart; its browser sessions work at once. menger exposes
no swap without a restart today.

**Principals across a swap.** A principal is the backend's
(`oidc:workos/<user>`, `idp:<uuid>`, `oidc:memory/<subject>`), so a person
signing in through a new backend is a new principal: memberships and roles
granted to the old one do not carry over, and an admin re-grants them (the
session carries the person's email as metadata, which `auth.waiting` shows).
Each sign-in also records the backend and its subject in the session
(`identity_backend`, `subject`) — the link a stable Plexus principal
(AUTH-23's `plexus:<id>`) will key on; no link table is kept yet.

### What the page guarantees, whatever the backend

| # | safety rule | how | tested by (`tests/signin.rs`) |
| --- | --- | --- | --- |
| 1 | server side only | forms post to the shell; the backend's secrets stay in its adapter (`ApiKey`'s `Debug` prints no key) | `every_workos_step_…` (the secret rides only the server's call) |
| 2 | nothing secret in a log, error, URL or redirect; one answer for any address | codes, tokens, tickets and TOTP keys are `Secret`s whose `Debug` prints nothing; logs name kinds, never values, and a backend's errors by their code (AUTH-24 #10); a code is sent *after* the page answers, so its timing says nothing; a wrong code and an unknown address read the same; the SSO redirect carries the organization, not the address (AUTH-24 #9) | `no_code_token_address_or_key_is_ever_logged`, `sending_a_code_answers_the_same_for_any_address`, `an_sso_domain_goes_to_its_idp_…` |
| 3 | throttles per address, per user and per network, with backoff; the IP from `X-Forwarded-For` only through trusted proxies | `throttle::Throttles`. **A guess is reserved (counted) before it is made**; only a definite answer settles it — right refunds, wrong stays, and only *no verdict* (the backend down or rate-limiting us) gives it back; a 401/403 is not a refund (AUTH-24 #1, AUTH-25 N8). **An emailed code is guessed only in the flow that asked for it**, at most 5 times; then that flow needs a new code (N4) — so a stranger can only guess codes they caused to be sent. Sends per (address, neighbourhood) 3/10 min and 10/day; from networks that never signed the address in, 3/day in all (N5) — the owner's networks keep their own allowance. **No limit refuses everyone** (N1–N3): the per-client *cost* limits (sends, SSO lookups, flows started) are kept at three network levels (IPv6 /64, /48, /32; IPv4 /32, /24, /16), each with its own allowance, so a provider's thousand /64s share one budget and unrelated networks are untouched. *Guesses* (codes, passwords, TOTP) are limited at the client's own network only, never a wider level (AUTH-26 R1): a wide guess tier would refuse a whole provider the right code, and guesses are already bounded per code and by the tiered sends. SSO lookups are limited apart and come **before** any send limit. Tables drop entries past their own window and, when full, evict unlocked entries of the most-represented neighbourhood first, **newest first** (AUTH-26 R3: a flood is the newest, so an owner's remembered network is the last to go); a new key is always admitted. Off loopback (or a shell listening off loopback with no public URL) the page refuses to start until `--trusted-proxy` names the edge or says `none`; a forwarded header from an untrusted peer is logged under `none` too (N9) | `review.rs`, `throttle::tests`, `codes_sent_and_tried_…`, `wiring::tests` |
| 4 | CSRF, cookies, CSP, `no-store` | every form — the social buttons too — carries a token: an HMAC of the flow id and the time, valid 15 minutes, and a page's flow id starts one flow at most — a double submit lands on the step the first reached (the code entry, once a code went); a page left open past its token's life is drawn again with a fresh token and "That page expired" (200), never a 403 (AUTH-26 R6); the flow cookie is `HttpOnly; SameSite=Lax`, `__Host-` over https; another site's `Origin` is refused; flows are never refused: past the table's size the most-represented neighbourhood's **newest** flows make room, so a sign-in under way and a visitor from an unrelated network are untouched, and no per-client cap evicts a NAT neighbour's flow (N2, N6); every page `no-store`, unframeable, `default-src 'none'`, its hashes taken from the template, a page holding any other block refused; filled in one pass | `one_page_s_token_starts_one_flow`, `a_flood_cannot_evict_…`, `a_thousand_slash_64_flow_flood_…`, `a_nat_neighbour_s_flow_survives_…`, `page::tests` |
| 5 | federated round trips | `state` and the PKCE verifier live in the flow this browser's cookie names, taken once; the redirect URI is exactly `<public-url>/login/callback`; where to land is checked by `ReturnTo` when the flow starts and ends — a path on this origin, or an origin on the list (`--return-origin`) | `a_social_button_goes_out_and_back_bound_to_this_browser`, `return_to_stays_on_this_origin_or_a_listed_one` |
| 6 | tokens | the backend verifies its own (WorkOS's against its JWKS with the gate's own OIDC adapter); they stay in `app_session::TokenStore` beside the session, refreshed before expiry; a refused refresh ends the session, and so does one that cannot be made once the token has expired; sign-out ends the app's session **first**, then revokes at the backend in its own task with retries — the refresher, too, hands revokes to their own tasks (N7); a sign-in finishes with the backend that started it | `sign_out_ends_the_session_before_the_backend_is_told`, `a_refresh_that_cannot_be_made_after_expiry_…`, `a_swap_midway_finishes_…` |
| 7 | MFA | enrolment from `/login/account` needs a sign-in in the last 15 minutes; the key is shown once and the factor deleted unless confirmed within 10 minutes (or on cancel); removal needs a current code from that factor; the sign-in challenge comes whenever the backend asks | `totp_is_enrolled_challenged_at_sign_in_and_removed`, `an_unconfirmed_factor_is_deleted`, `enrolling_needs_a_recent_sign_in`, `a_backend_that_requires_mfa_enrols_at_sign_in` |

Revealing that a **domain** has SSO is the trade-off every SSO sign-in makes
(the email form redirects for it, and shows the code page otherwise);
revealing whether an **address** has an account is not, and the page never
does.

### What a stranger can guess

A stranger can only guess a code they caused to be sent, five times per
code. From networks that never signed an address in, an address gets 3
codes a day in all, so a stranger gets at most **15 guesses a day, ≈450 a
month, per address**. Against a six-digit code that is ≤ 450 × 10⁻⁶ ≈
**0.045 % a month**, if a new code supersedes the last (WorkOS's behaviour
here is unverified). If every unexpired code stayed valid, a guess could hit
any code sent in the last ten minutes: at most the stranger's 3 plus the
owner's own, so ≤ 0.3 % a month in that unlikely worst case. A stranger
inside the owner's own neighbourhood (/24, /48) shares its allowance (10
codes a day). The cost to the owner: when strangers have used up the day's
3, the owner can't get a code **from a network they have never signed in
from** until the next day; from a network they have, they always can, and
social or SSO sign-in is never affected.

### On our page, and at the provider

| on our page | at the provider |
| --- | --- |
| the email field, the code, the password form, the TOTP code, the organization picker, the account page | Google's, GitHub's, Apple's or Microsoft's own consent screen; a company's IdP for SSO |

WorkOS's own screens never appear. A Radar SMS check (WorkOS preview) is
refused on the page; a passkey is not offered (WorkOS's API has no passkey
step outside AuthKit).

### Reusing the page elsewhere

`signin::SignInPage` knows no app and no session table: what happens once
someone is signed in, and how a page is drawn, is the `Continuation` its
owner passes. plexus-login's per-app login passes
`app_session::AppContinuation` (the app's session, the backend's tokens). A
central sign-in service (AUTH-23: a hosted plexus-idp signing people in
through WorkOS behind our page, then issuing per-app tokens) would pass one
that mints its own token and sends the browser back to the client app — an
origin on its `ReturnTo` list — draw each client's branding in `draw`, and
mount `SignInPage::standalone_router()`, which carries its own `GET /login`
and `GET /login/callback`.

| route | |
| --- | --- |
| `POST /login/email` | the email: an SSO domain → its IdP; else a code, the same page for any address |
| `POST /login/resend`, `POST /login/code` | another code; the code |
| `POST /login/password` | a username and password |
| `POST /login/continue` | a code a step waits for (TOTP, email verification, an extra check, a factor enrolled at sign-in) |
| `POST /login/choose` | an organization |
| `POST /login/social` (`provider=google`, the page's CSRF token) | out to the provider; back at `GET /login/callback` |
| `GET /login/account`; `POST /login/account/totp`, `…/confirm`, `…/cancel`, `…/remove` | two-step verification |

## Login in a dozen lines: `Wiring` and `SimpleAccess`

```rust
use std::sync::Arc;
use plexus_core::plexus::DynamicHub;
use plexus_login::{Branding, LoginOptions, Wiring, simple::SimpleAccess};
use plexus_transport::TransportServer;

const BRANDING: Branding = Branding { app: "myapp", version: env!("CARGO_PKG_VERSION"), mark: "m", motto: "" };

// Our own page over WorkOS (key from MYAPP_WORKOS_API_KEY); `memory` for a
// first run without any account.
let identity = plexus_login::backends::from_spec("workos:client_01ABC…,social=google+github+apple", BRANDING.app)?;
let opts = LoginOptions { identity: Some(identity), ..LoginOptions::new(BRANDING, ws_port, shell_port) };
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
    pub identity: Option<Arc<dyn IdentityBackend>>, // --identity (AUTH-22): our own page over it
    pub trusted_proxies: TrustedProxies, // --trusted-proxy CIDRs
    pub return_origins: Vec<String>,     // --return-origin: more origins a sign-in may return to
    pub limits: Limits,                  // the page's throttles
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
| `flow_cookie` | `x_flow` | AUTH-22: our own page's flow cookie, `x_flow_<port>` or `__Host-x_flow` (the CSRF token's key, a sign-in's steps) |

`session_key` has one definition: the session table's
(`Sessions::session_key`). Rules that read it ask the table: menger's
tickets-api through `SessionCheck::key`, and the auth backend through
`LoginAccess::session_key`.

`Branding { app, version, mark, motto }` is what the page says of the app: its
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

**WorkOS behind AuthKit** (the hosted flow; `--identity workos:…` is our
own page instead, above) stays available. Give it its client id, then an
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
| `POST /login/email`, `/login/code`, `/login/password`, `/login/continue`, `/login/choose`, `/login/social`, `GET`/`POST /login/account…` | AUTH-22: our own page's steps, with an identity backend (above) |

A POST from another origin is refused.

## Serving the page

**Behind a proxy (AUTH-24 #2).** A sign-in page whose public URL is not
loopback refuses to start until `trusted_proxies` says what is in front of
it: the edge's address or network (`--trusted-proxy 172.18.0.2/32`), or
`none` when people connect directly. Without it, every visitor behind an
edge is one client: one bucket for every throttle, and the edge's address
sent to the backend's risk checks for everyone. Trust as little as you can:
the edge's own container address beats its whole Docker network, since any
other container on that network could otherwise forge `X-Forwarded-For`.
An `X-Forwarded-For` from a peer that is not trusted is logged once, as an
error.

Serve the router with the peer's address, so the throttles see who asks:
`axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>())`
(plexus-shell's `serve_on` does). Without it every unknown client shares one
bucket. Behind an edge, name it with `trusted_proxies`.

## 0.2.0

AUTH-22: `identity` (the `IdentityBackend` trait), `backends` (WorkOS,
plexus-idp, memory; `--identity` specs), `signin` (our own page), `app_session`
(the backend's tokens beside the session), `throttle`, `client_ip`,
`crypto`, `page` (the chrome, the CSP). AUTH-24's review fixed before
release: reserved (atomic) throttles, per-(address, client) limits, /64
buckets, the trusted-proxy requirement, the flow table, CSRF'd social
buttons, sign-out order, template-hashed CSP, development backends gated.
Breaking: `Branding` gains `motto`;
`LoginNames` gains `flow_cookie`; `LoginOptions` gains `identity`,
`trusted_proxies`, `return_origins`, `limits` (and is no longer `Debug`-derived:
its `Debug` names the backend, not its contents); `LoginConfig` likewise;
`web_login::callback` and `logout` take the peer and the form; every page goes
out with a strict CSP.

## License

MIT
