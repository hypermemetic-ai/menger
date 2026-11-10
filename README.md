# menger v2

A plan is a program whose leaves are prose.

menger is a ticketing service on Plexus RPC: a store of tickets that a
person reads in the browser and an agent reads with `synapse`, both over one
signed-in WebSocket, every change pushed to every open page as it happens.
v2 keeps v1's core — the tickets store, its filter language, its access
rules — and replaces v1's browser app with a **ui service**: pages are
components written in real Svelte (`ui/src/<name>.svelte`, profile
`svelte/1`; `../plexus-ui/CONTRACT.md`), pushed as signed bundles,
versioned by content hash and drawn by a small engine. SV-4: rhx (Rhai in
markup, `rhx/1`) is retired from menger's UI — its sources are kept in
`ui/retired-rhx/`, out of the bundle.

v1 is tag `v0.0.1`; what v2 cut — the chats, comments, deployments, themes, notes, assignees and components, the Leptos app (trunk) with the frames' SDK (sdk-codegen) and mockups, the diagrams service — lives there. Media came back as a service of its own (V2-24, "The services"), and documents have a service of their own (V2-29, `docs`). Every input is held to a per-user allowance by the quota service (V2-31).

## Run

```sh
cargo install menger
menger --open
```

That is the whole first run (DIST-14). With no flags menger keeps its data
in `~/.local/share/menger` (`local.db`; `--data-dir`, `--db-path`), listens
on this machine only, and prints where to open it:

```
menger 0.6.2 — running
  open:  http://127.0.0.1:4456/login?once=…   (one-time; `menger login` for another)
  agents: synapse -P 4455 _login menger --key menger-local --as menger-local
  data:  ~/.local/share/menger/local.db
  stop:  Ctrl-C
```

`--open` opens that link in the browser too. `menger --help` is the short
list; `menger --help-all` is every option, grouped; `menger --version`.

**There is no open mode** (ACCESS-24). With no `--auth-provider` (or
`--identity`) and every listener on loopback, menger signs its one person in
by itself:

- On the first start it makes a **local admin key** in the data directory
  (`local.key`, 0600, synapse's format) and makes its `key:<hex>` the
  server's `--admin`; the `key` provider is on, so synapse signs in with
  that key too.
- Each start prints a **one-time link**, `http://127.0.0.1:<shell
  port>/login?once=<token>`: one use, ten minutes, taken only from
  loopback. `menger login` (against the server running on that data
  directory — its `local.json`) prints a fresh one. The links are files
  under `once/` in the data directory, each named by its token's SHA-256.
- **Agents** (DIST-17): a local start makes `~/.plexus/keys/menger-local.key`
  a symlink to `local.key` — never a copy of the seed — and prints the
  `agents:` line with the WebSocket's real port. `menger login --agent`
  prints it again. An existing file there is left alone (the banner says
  so). A server with a login provider is not in local mode and links
  nothing.
- A port not given that is taken falls back to the next free one, and the
  banner says which; a port given is bound or startup fails.
- The terminal shows the banner, not the log (errors only; `RUST_LOG` says
  otherwise).

Reached beyond this machine (`--bind`, a `--public-url` off loopback) with
no provider, it refuses, naming the flag to add.

A development server from the checkout runs a provider and its own ports
and database — never the defaults on a machine that runs prod:

```sh
PLEXUS_NO_REGISTRY=1 cargo run -- --port 4460 --shell-port 4461 --db-path /tmp/menger-dev.db \
  --auth-provider key --admin "$(synapse _key show builder)"
synapse -P 4460 _login menger --key builder --as builder   # once per server start
synapse -P 4460 menger tickets health
```

The ui service's file is `ui.db` beside the database unless
`--ui-db-path` names another; two dev servers in one directory share it, so
give each its own. A push needs `--ui-signer` (below, "The ui service").

**A restart keeps every session** (FRAME-21). The login's session table —
every browser's sign-in and every agent's `session login` token — is kept
beside the database, named for it: `tickets.db` → `tickets.sessions.jsonl`
(the image's `/data/tickets.sessions.jsonl`, a local start's
`local.sessions.jsonl`), 0600, each session under its id's sha256 and never
the id. So a deploy signs no one out: an open page keeps its cookie, an
agent its token. Beside it, `….sessions.jsonl.lock` is held while the
server runs: a second server on the same database's sessions refuses to
start, naming the file. A sign-in under way at the restart starts again. A
session the sign-in page started (WorkOS) stands across a restart until its
own expiry and sign-out still ends it at WorkOS, but its tokens are not
kept, so a revocation made at WorkOS no longer ends it here (FRAME-22). The
sessions file goes with the database: delete both, or neither.

Login is always required: the shell's pages are behind `/login`, a socket
that proves no one reaches the anonymous view and nothing else ("The AUTH
protocol"). There is no REST bridge (it could not carry an identity;
`--no-rest` / `MENGER_NO_REST` is accepted and changes nothing) and no
`--stdio`. On its first start on a database this build purges the grants
open mode planted ("Login" → The purge).

**Throwaway servers** (ACCESS-52). A script that starts its own menger —
`perf/`, the pages' browser test and probe (`tests/pages_browser.mjs`,
`perf/pages-probe.mjs`, through `scripts/ui-throwaway.mjs`) — runs it
under `key` with a key of its own as `--admin`, made in its scratch
directory (never `~/.plexus`), and signs in with it through
`scripts/throwaway-login.mjs`:

```sh
op=$(node scripts/throwaway-login.mjs key "$work/operator.key")      # key:<hex>
menger --port 4510 --shell-port 4511 --db-path "$work/t.db" \
  --auth-provider key --admin "$op" &
node scripts/throwaway-login.mjs session --url ws://127.0.0.1:4510 \
  --key "$work/operator.key" --out "$work/session" [--agent NAME]  # an mgs_ token, 0600
synapse --token-file "$work/session" -P 4510 menger tickets health
```

**What the shell serves** (`src/shell.rs`). `/app/` answers the entrypoint
of the ui service's live version (`index.html` of its bundle, stamped with
`{{WS_PORT}}`, `{{TITLE}}` and `{{UI_VERSION}}`, the version's hash;
`Cache-Control: no-store`) — a session previewing a version gets that one —
and, with no version live, a placeholder page naming the socket.
`/app/<hash>/<file>` is that version's file, `immutable` for a year. `/` is
the placeholder too (or the entrypoint of an `--app-dist` build). `/studio/` (STUDIO-2) is the framework's studio — the version a push of
plexus-ui's `studio/` built `--name studio` and pushed `--no-live` keeps as
`bundle:studio` (a deploy pushes it after the app's) — to a session the gate
says holds `ui.author` on `*` — admin, an `editor` on `*` (403 otherwise), with `--ui-compiler`'s module at
`/studio/ui-compiler.wasm`, which it compiles a new page in. The
login routes (`/login`, `/login/start`, `/login/callback`, `/logout`,
`/auth/cli`, …) are plexus-login's. Every page goes out under a
Content-Security-Policy (ACCESS-28, `PAGE_CSP`): no inline script but the
page's own blocks, hashed; no script or image from another origin;
connections only to itself, its socket and loopback.

**MCP** (TRANSPORT-15; on v2, V2-43). menger is an MCP server too: `/mcp`
on the app's origin (`http://127.0.0.1:4456/mcp` here,
`https://app.menger.sh/mcp` deployed — the edge needs no route of its own;
`--no-mcp` / `MENGER_NO_MCP` turns it off), served through
plexus-transport-mcp's bridge mounted behind the same login. Its tools are
every method of every service registered — `tickets`, `members`, `orgs`,
`ui`, `media`, `docs`, `quota`, `auth`, `session` — as the caller may see
them. Every `tools/call` goes through the gate as the caller the request's
Bearer names — an agent's session, or a token an `--auth-provider` admits,
exactly what the WebSocket takes — and `tools/list` is that caller's view; a
request with no credential is the anonymous view (the `auth.*` door); a
refused one is `401`. An MCP client that takes a static header signs in
with the session synapse keeps after `_login` (its keychain item:
`backend=<host:port>`, `agent=<name>`, `service=plexus-session`; the JSON's
`token`), which ends with the session (12 h). A key proof will not do: it
opens one upgrade, and MCP carries the header on every request, so its
second use is refused.

With `--mcp-authorization-server URL` (repeatable;
`MENGER_MCP_AUTHORIZATION_SERVERS`, `;`-separated), `/mcp` is an OAuth 2.1
protected resource: a request with no token is answered `401` naming
`/.well-known/oauth-protected-resource/mcp`, so an MCP client signs in at
that server by itself. On app.menger.sh and app-test that server is the
environment's WorkOS AuthKit domain (TRANSPORT-24, `deploy/env/app-*.env`):
the same sign-in as the app's, and a third `--auth-provider`, the generic
`oidc:authkit=<domain>,<public-url>/mcp`, admits the tokens it mints as
`oidc:authkit/<WorkOS user id>` (one principal with the app's cookie is
TRANSPORT-20); `MENGER_LOGIN_PASSWORD=none` keeps the generic provider off
the login page's password form. A person's entry then carries no header —
Claude Code's `/mcp` shows **Authenticate** and the browser does the rest:

```
claude mcp add --transport http menger https://app.menger.sh/mcp
```

`tests/mcp.rs` pins the three callers, v2's services, and the protected
resource's 401.

**Found from the domain** (V2-64; SEP-2127, MCP Server Cards). Wherever
`/mcp` is served, so are three public documents (`src/discovery.rs`) that
let an agent knowing only the host find it: `/.well-known/ai-catalog.json`
(`application/ai-catalog+json`, an AI Catalog of one entry) names
`/mcp/server-card` (`application/mcp-server-card+json`: the card's name —
the host reversed, `sh.menger.app/menger` — version, description and the
`/mcp` remote, never the tools), and `/llms.txt` says the same in prose.
Each carries CORS `*`, an hour's cache and an `ETag` that `If-None-Match`
answers `304`. menger.sh, the landing page, has its own catalog and
llms.txt (`landing/`), naming app.menger.sh's card.

**Hosts** (ACCESS-25). Every listener — the WebSocket and the shell —
answers only its own names, so a page that rebinds its name to 127.0.0.1
(DNS rebinding) gets nothing: any other `Host` is refused `421 Misdirected
Request` before anything routes it. Each port always answers
`127.0.0.1:<port>`, `localhost:<port>` and `--public-url`'s host;
`--allowed-host` adds more (repeatable, `host:port` or `host`;
`MENGER_ALLOWED_HOSTS`, `;`-separated).

```sh
PLEXUS_NO_REGISTRY=1 cargo run -- --port 4460 --shell-port 4461 --auth-provider key --admin "$(synapse _key show builder)" \
  --db-path /tmp/menger-dev.db --allowed-host menger.lan:4461 --allowed-host menger.lan:4460
curl -si -H 'Host: evil.example' http://127.0.0.1:4461/login   # 421
```

**Bind** (RUN-8). Every listener binds loopback unless `--bind ADDR`
(`MENGER_BIND`) names another — `0.0.0.0`, `::`, or one address. Binding
changes where menger listens, not what it answers to: a specific address
adds its own literal to the names (and the shell's origin at it to the
origins that may open the socket); an unspecified one adds nothing, so each
name it is reached by is an `--allowed-host` (and an `--allowed-origin`).
plexus-transport's builder binds the socket (`TransportServerBuilder::bind`,
TRANSPORT-7), with every check it keeps; `src/bind.rs` holds the flag.

**Trusted proxies** (TRANSPORT-7). `--trusted-proxy` (`MENGER_TRUSTED_PROXIES`;
CIDRs, addresses, or `none`) names the edge in front. `X-Forwarded-For` /
`X-Forwarded-Proto` are believed only from a peer in that set — by the login
page's throttles (AUTH-22) and the WebSocket's per-IP and anonymous quotas.
From any other peer a forwarded header is ignored. `tests/wire_hosts.rs`
opens sockets through a fake proxy to show both cases.

**Behind https** (ACCESS-30). Reached beyond this machine, menger is one
https origin: a proxy terminates TLS, sends `/ws` to the WebSocket and the
rest to the shell, and `--public-url https://<host>` is required. Cookies are
then `Secure` (`__Host-` for the session's), every shell answer carries
`Strict-Transport-Security: max-age=31536000`, the provider's redirect URI
is `<public-url>/login/callback`, and RFC 9728's metadata is
`https://<host>/.well-known/oauth-protected-resource/ws` (RUN-9). An
`http://` public URL on the LAN (a private address, `.lan`, `.local`, one
label such as `localhost`) is served with a warning; any other is refused at
startup. The proxy recipe and the checklist are `deploy/README.md`'s.

```sh
PLEXUS_NO_REGISTRY=1 cargo run -- --port 4460 --shell-port 4461 --auth-provider key=wss://menger.example \
  --admin "$(synapse _key show builder)" --db-path /tmp/menger-dev.db --bind 0.0.0.0 --trusted-proxy 172.30.0.2 \
  --allowed-host menger.example --public-url https://menger.example
```

### Every flag

`menger --help-all` groups them and names each one's `MENGER_*` variable,
where it has one (a list in it is `;`-separated).

| group | flags |
|---|---|
| run | `--open`, `-p/--port` (4455), `--shell-port` (4456), `--data-dir`, `--db-path`, `--ui-db-path`, `--ui-signer`, `--ui-compiler`, `--ui-allow-rhx` (SV-6: rhx/1 taken again — without it every `ui.push` and `ui.author` carrying an rhx component is refused `rhx_disabled`, naming it; `MENGER_UI_ALLOW_RHX`), `--ui-author-key`, `--media-db-path` (`media.db` beside the database), `--media-store` (`disk`), `--media-blob-dir`, `--media-max-bytes`, `--quota-db-path` (`quota.db` beside the database), `--quota-plan` (the defaults a new quota file starts from; the built-in `free-1k`), `--ui-channel` (UI-15: the live pointer this environment serves in its ui file; default from `--env`: `test`, `prod`, else `default`), `--bind`, `--app-dist` (other files under `/app/`, from a directory), `--env` (`test` titles the tab `menger.test` and allows the dev sign-in), `--no-shell`, `--no-mcp` (V2-43: no `/mcp`), `--child NAME=stdio:CMD` or `NAME=ws://…` (POLYGLOT-5: an activation in another process mounted under NAME behind the gate; repeatable; `--child-timeout`, `--child-max-calls`, `--child-env`, `--child-key`: plexus-core's `ChildArgs`; one that does not answer stops the start, exit 2; NAME a service of menger's own is refused) |
| sign-in | `--auth-provider`, `--admin`, `--public-url`, `--login-button`, `--login-email`, `--login-password`, `--login-forgot`, `--identity`, `--workos-api-key-file`, `--allowed-origin`, `--frame-ancestor` (an origin that may frame the shell's pages; none by default), `--live-origin` (an origin the shell's pages may frame), `--allowed-host`, `--trusted-proxy`, `--return-origin`, `--session-hours` (12), `--session-idle-minutes` (120), `--mcp-authorization-server` (V2-43: `/mcp` a protected resource naming it) |
| maintenance and testing | `--dev-login`, `--dev-password`, `--dev-idp-ws`, `--dev-user` (all `--env test` only), `--verify-derived`, `--seed-synthetic`, `--seed-value`, `--seed-shape` (`flat`, `nested`), `--seed-into-existing` |
| limits | `--max-body-bytes` (8 MiB), `--max-message-bytes` (32 MiB; V2-54: one socket message — a UI bundle is pushed in one), `--max-calls-per-connection` (256), `--idle-timeout-secs` (900), `--max-connections` (1,024), `--max-connections-per-ip` (256), `--max-connections-per-principal` (4; V2-31: a person's), `--max-connections-per-agent` (64; V2-31: a `key:` principal's), `--max-writes-per-second` (20; V2-31), `--max-anonymous-connections` (128), `--max-anonymous-connections-per-ip` (32) |

`--ui-author-key` names the file of the key the ui service signs an
editor's authored version with (made at first start, 0600; one open to its
group or others is refused, naming its mode); a start naming its public key
as a `--ui-signer` too is refused. On a server whose `editor` role an admin
ever changed with `auth role_set` (app-test included), the stored role wins:
`ui.author` and `ui.routes.set|clear` are not in it until it is set again
with them (`auth roles` says what it holds). The same holds for COMPS-4's
`ui.components` in `viewer`: a stored viewer role lacks it until it is set
again with it.

**Components in tickets** (COMPS-4; plexus-ui's CONTRACT.md "An author's own
namespace"). A body defines a component in a fence, ```` ```component x-name ````
holding rhx, or ```` ```component x-name html ```` holding a custom element's
HTML and script (only ever drawn framed); the name is a tag's (lower case,
with a hyphen). SV-6: an rhx definition's save is refused `rhx_disabled`
(said on its line) unless the server runs `--ui-allow-rhx`; an `html` one,
and v1's, are framed and saved as before. The ticket's save is its definitions' — the editor's amend,
the edit form's body, a doc at a pause (`ui/src/tag-components.svelte`,
real Svelte, svelte/1: `authorChanged`): each definition whose text changed is `ui.author`ed, into
the live bundle for a saver holding `ui.author` on `*`, else into their own
namespace (`own: true`: stored, versioned, signed by the service, never in
the bundle); the answer (saved, or the refusal with its line) is said on the
definition's line. The definition's source is hidden wherever a body is
drawn (COMPS-8): one line stands in its place — `<x-name> · component`, its
save — and its `source` opens the code card and closes it again, closed on
every load (`ui/src/component-def.svelte`, real Svelte, svelte/1); in the editor the block under
the caret is its source, as every block is. Any tag in any body (`<x-name attr="…">`, or an `m-*` the house
draws no card for) is resolved by `ui.components` — the live bundle first,
drawn natively (a bound mark), then the newest authored, drawn in a frame
(`bus.frame`, its calls the viewer's held to its `uses`); its attributes are
its props, its children its `body`, and the table follows `ui.watch`, so a
definition edited redraws every ticket using it. A tag nothing names keeps
its words, "no component named x" on hover. `tests/pages/components.mjs`.

**v1's definitions** (COMPS-5). A fence with no `html` holding v1's custom
element — a `<template>`, or a script reading `this.root`, `this.menger` or
`this.attr` (rhx drops `<template>`; its own `<script>` and `<style>` say
nothing either way) — is saved as plexus-ui's `element/1` and drawn by
plexus-ui in a frame: an open shadow root holding its styles and template,
its script run with the element as `this` (`this.root`, `this.attr(name)`,
`this.menger.on('theme' | 'change')`), its attributes its props, the cleanup
it returns run when it leaves (plexus-ui's CONTRACT.md "An element
component"). v1's words after the name (`component x-a data-field=b`) are
kept as its head. **The backfill** (`src/backfill.rs`): every start, before
anything is served, registers each definition the store holds — every body
(archived ones too), each deleted ticket's body its tombstone keeps, then
v1's own registry left in the file (`components`, orphans too), a name at its
first — that plexus-ui does not resolve, in the namespace `menger:v1`,
written by its ticket, signed by `--ui-author-key`; it leaves the live
bundle's names and anything authored on v2, refuses rhx (compiled by its
ticket's save), keeps again one whose body moved, and logs `backfill: N
registered, M kept again, K unchanged, R refused`. A second start changes
nothing. `tests/backfill.rs`. The theme carries v1's `--text-scale`,
`--mono` and `--err-fg`, which v1's components paint with.

`menger login [--agent]` prints a one-time link (or the agent's `_login`
line) for the server running on `--data-dir`.

### Synthetic seeding (PERF-2)

For measurement, not for real data: `--seed-synthetic N` bulk-seeds N
synthetic tickets before serving — through the store, in a few large
transactions — then serves them like any other. The shape is realistic
(Zipf-sized shortcodes, every kind, a short body, a quarter with acyclic
blockers) and deterministic: the same `--seed-value` (default 1) gives the
same tickets; `--seed-shape nested` hangs them in a tree (FILTER-13). A
database that already holds tickets refuses the seed unless
`--seed-into-existing`. The probes in `perf/` (`perf/README.md`) drive it
at each size.

```sh
cargo run --release -- --db-path /tmp/scratch.db --seed-synthetic 100000 \
    --port 4470 --shell-port 4471 --auth-provider key --admin "$(synapse _key show builder)"
```

### The stored derived state, and checking it (SCALE-28)

What the store derives from the tickets — each shortcode's run of
sequences, the dangling forward references, the roll-ups, each ticket's
children and `child_count`, its count of unsatisfied blockers, the ready
set, each shortcode's status counts, the filter cells and their bitmaps,
the kept tree, the search index — is stored beside them, written by every
write in the same transaction, and read at start instead of rebuilt. The
form is **17** (V2-20: the cells carry `asking` again);
a file in an older form is built again, once, on its first open, and the
log says so.

```sh
menger --verify-derived --db-path /path/to/tickets.db
```

opens the file read-only, rebuilds all of it by a scan, compares, and prints
each difference or `consistent: …`. Exit 0 consistent, 1 a difference (or
nothing stored to compare), 2 unreadable. It changes nothing: run it before
a deploy and after any migration.

## The services

menger is a **tenant** of the stored-UI framework plexus-ui (FRAME-1,
FRAME-7): its own code is the tickets service (`tickets-api/`), the server
that registers it (`src/`) and its UI's components (`ui/`); the ui service,
the compiler, the engine, the routes that serve a bundle and the tool that
builds, signs and pushes one are the framework's, taken
by published version, as `plexus-login` is (V2-47): `plexus-ui-service` and
`plexus-ui-compiler` from crates.io at one version, and the tool, the engine
and the compiler module built from that version's tag, `plexus-ui-v<version>`
— the checkout `scripts/plexus-ui-src` names (cloned once into
`~/.cache/plexus-ui`; `PLEXUS_UI_SRC` names a framework worktree on purpose).
A clone of menger alone builds. `scripts/plexus-ui-release VERSION --ui DIR`
publishes plexus-ui, tags it and moves menger to it in one step (`--help`;
plexus-ui's README "Releasing"). Any other Plexus service ships with the
framework the same way.

A service that would rather not link it runs beside the framework's own
binary instead (FRAME-6): `plexus-ui serve --child NAME=ws://…` keeps its
`ui.db`, embeds the compiler, signs people in and mounts the service as a
child behind its gate, so a page's one socket reaches it
(`../plexus-ui/README.md` "A tenant in ten lines"; its example tenant is
`examples/shop`). menger links the library and builds no binary of that
name (`tests/apart.rs`). The tool assumes no service: menger's calls to it
name `--service menger` (`deploy/pipeline`'s push), and a contract asked of
a service is its `ui.contract` — the hash over every service the
bundle's `uses` name (one service: its schema's own hash). The pipeline
stamps each bundle so (SURFACE-11): it reads the methods from the
manifest of a first, unstamped build, asks the menger it just built `ui
contract --uses` them, and builds again with `--contract` — the app over
`tickets` and every other service its components name, the studio over
`ui` — and its log names the services each contract covers.

One process registers these on the `menger` service (`src/builder.rs`), all
behind plexus-core's gate; menger's own are declared on the frame
(ACCESS-13), `auth` is plexus-login's:

| service | what it is | its code |
|---|---|---|
| `tickets` | the store, its reads, writes and watch; children `commits`, `filters`, `nav`, `revisions`, `subtasks`, `work` | `tickets-api/src/activation/` |
| `members` | grants and teams (child `teams`) | `tickets-api/src/members_activation.rs` |
| `orgs` | organizations | `tickets-api/src/orgs_activation.rs` |
| `session` | who a connection is, agents' sessions | `tickets-api/src/session_activation.rs` |
| `auth` | the AUTH protocol's door and roles | plexus-login, over `src/login_access.rs` |
| `ui` | the UI's versions, live pointer, previews; child `slots` | the framework's: `../plexus-ui/service` (`plexus-ui-service`, FRAME-5) |
| `media` | any file a body shows, by sha256 — its rows in a file of its own (`media.db`), its bytes in a `BlobStore` (the disk); links read into cards; the ticket's (or, V2-29, the doc's) permit asked through the seam | `media/` (`menger-media`, V2-24, EDITOR-10) |
| `docs` | documents — writing, not work: a title and a markdown body kept as a Yjs CRDT, written live by many hands, every change kept, in a file of its own (`docs.db`); who may is the gate's word on the document's organization | `docs/` (`menger-docs`, V2-29) |
| `quota` | every principal's allowance per source, its usage counted, tabulated and enforced — its ledger in a file of its own (`quota.db`); every storing write reserved through the seam before it lands | `quota/` (`menger-quota`, V2-31) |

**Service to service.** The ui service reaches the tickets service as a
client, never through its store: its seam (`../plexus-ui/service/src/seam.rs`)
holds the running process's service and calls each service a component's
`uses` names (`tickets.list` names `tickets`) through plexus-core's
`HubTransport::as_(service, caller)` as the server's own caller — the same
route and gate a socket reaches. A push asks it for those methods and for
the services' schema hashes. The ui service never opens the tickets file.

The media service (V2-24) reaches it the same way, **as the caller**: a
`media.put` for a ticket asks `tickets.may { id, role: editor }` through
`HubTransport::as_(service, caller)` — the caller's own identity, so the
tickets service decides by its reach, exactly as the write itself would —
and a `media.get` asks `tickets.may { id, role: viewer }` for a ticket the
image was put for. Its seam is `media/src/seam.rs`; it never opens the
tickets file either. The shell's `GET /media/<hash>` is a client of the
media service in turn (`menger_media::fetch`, as the request's session).

**Pages in a body** (EDITOR-14) draw in frames, as v1 drew them: an
`<m-mockup widths title description>…a page…</m-mockup>`, a ```` ```mockup ````
(or ```` ```page ````, or ```` ```html ```` with a page's attributes) fence, a body
that is a whole `<!doctype html>` page, and a body holding raw HTML the
page's sanitizer cannot draw (then the whole body, its markdown around the
HTML) — each in `<iframe sandbox>` with no `allow-same-origin`: an opaque
origin, no cookie, no storage, no socket, no `top`. A `live` fence or
`<m-live route>` frames the test app (`--live-origin`, APP-110). Not a
`srcdoc`, which inherits the app's policy and would refuse the page's
scripts. **The frames are plexus-ui's** (COMPS-2; `../plexus-ui/CONTRACT.md`
"Frames"): the ui routes serve the public **`GET /frame`** (`FRAME_CSP`:
`connect-src 'none'; form-action 'none'; frame-ancestors 'self'; sandbox …`),
which says `plexus:boot` on its channel and is handed the page by
`postMessage`, from its own parent only, and `ui/src/m-mockup.svelte`, `m-live.svelte` and `m-page.svelte` (COMPS-3: real Svelte,
`svelte/1`, components of the tags, drawn by ticket-view's tag table) draw each through the engine's `bus.frame` — the handshake, its height, the theme,
the double buffer. A framed page reads the tracker through the bridge
(EDITOR-15): `plexus.call("tickets.get", { id })` is answered as the reader,
held to `tickets.get`, `tickets.list` and `tickets.search` and to what the
component drawing the ticket holds; anything else, a write above all, is
refused in words. A ```` ```mermaid ```` fence in a raw body is drawn before the
body is handed over, so it draws inside the frame as outside.

The docs service (V2-29) asks two things through its seam (`docs/src/seam.rs`),
never another's file: the gate, whether the caller holds a method's grant
(`docs.save`) on the document's organization (`acme:`) — `DynamicHub::gate()`
asked with the scope as target — and the tickets service, which organizations
the caller is in (`orgs.list`, as the caller). The media service asks it in
turn: a `media.put { doc }` asks `docs.may { id, role: editor }` as the
caller, a `media.get` of a doc's image `docs.may { id, role: viewer }`.

The quota service (V2-31) is reached the other way round: a service that
stores or fetches for a principal asks `quota.reserve { principal, source,
amount }` before it writes, then `commit` (what it kept) or `release`,
through `HubTransport::as_(service, system)` as the server's own caller
(`menger_quota::QuotaSeam`, the `Meter` a service holds — the tickets
service in its `Access`, the media service as its `Allowance`). The quota
service asks back the same way: `members.teams.show` for a team's members,
`members.list` for an organization's, and each store's own count for a
recount (`tickets.revisions.usage`, `media.usage { everyone }`,
`docs.usage`). It never opens another file.

A throwaway's `synapse -P 4460 menger` lists them; `synapse -P 4460 menger
tickets` lists a service's methods and children; `<method> --help` prints
its parameters; `synapse -P 4460 -s menger` is the schema.

## Surface

Every method returns a stream of typed events (`#[serde(tag = "type",
rename_all = "snake_case")]`), ending with `done`; a domain refusal is an
in-band `error` (or `not_found`, `ambiguous`, `refused`), and a value its
type refuses is `-32602 invalid_params` naming the parameter, before the
method runs. Flags below are synapse's spelling (`--kebab-case`); `[--x]` is
optional. The schema's description of each method is the reference for
what it yields.

**A page is one event** (PERF-15). Every paged read — `list`, `find`,
`search`, `nav tree`, `work ready`, `subtasks list` — and the board's whole
listings — `nav groups`, `nav extents`, `nav counts` — answer their rows as
one `rows { items }` event, then their one trailing event (`page`;
`searched` for `search`; none for a listing), then `done`: three frames for a
page of 200, not 202. Each item is the per-row event it stands for,
serialized exactly as that event was (`summary`, `ticket`, `node`, `hit`,
`group`, `extent`, `count`), so a reader expands `items` in place. An empty
page is one `rows` with no items; a refusal is its `error` alone, no `rows`.
A page past **1 MiB** of items (`ROWS_BUDGET`: whole tickets, a body up to
8 MiB) goes on in the next `rows` event — an item over the budget alone in
its own — so no frame nears the transport's 10 MB message cap; the trailing
event follows the last. A `watch` is not a page: each change is still its
own event, and so is each item of `history`, `revisions list`,
`work blockers`, `nav facets` and `nav member`. `--fields` (`list`, `find`,
`work ready`; a list of the summary row's field names, `id,title,kind,status`
the board's four; another name refused by its type) thins each summary row
to those keys and its `id`, and asks for summary rows; without it a row is
whole. synapse `-j` prints the `rows` event as one line (SYNAPSE-36).

### `tickets`

| method | flags | what it does |
|---|---|---|
| `health` | | version, unix time, ticket count |
| `get` | `--id [--frame]` | one ticket by id or path address; `--frame` reads as that ticket's body's frame (`frame_reach`) |
| `list` | `[--epic] [--status] [--parent] [--filter] [--keep] [--after] [--at] [--limit] [--summary] [--fields] [--tz] [--frame]` | tickets in store order, paged: `rows`, then `page` |
| `find` | `--query [--filter] [--after] [--limit] [--fields] [--tz]` | id or title contains the query (the jump) |
| `search` | `--query [--filter] [--offset] [--limit] [--tz] [--frame]` | inside titles and bodies, best first, each hit with its passage |
| `create` | `--epic --title [--kind] [--parent] [--blocked-by] [--severity] [--body] [--org] [--path]` | the next `EPIC-N`; born `Draft` |
| `amend` | `--id [--title] [--body]` | replace a title and/or body, whole |
| `update` | `--id [--title] [--body] [--kind] [--status] [--parent] [--severity] [--block] [--unblock] [--hidden] [--message] [--source]` | any part of a ticket in one write, all or none |
| `update_many` | `[--ids] [--filter] [--max] [--kind] [--status] [--parent] [--severity] [--block] [--unblock] [--label] [--unlabel] [--message] [--source] [--tz]` | `update`'s change to many, all or none |
| `set_status` | `--id --status [--message] [--source] [--restore-children]` | a status; `Canceled` cancels open descendants |
| `set_kind` | `--id --kind [--status] [--message] [--source]` | a kind, and everything kept by kind |
| `set_parent` | `--id --parent` | adopt or move a ticket |
| `label` | `[--id] [--ids] [--add] [--remove]` | labels (`needs-you`, …) |
| `rename` | `--id --to [--message] [--dry-run]` | into a subtask id under its own parent |
| `reshortcode` | `--ids --to [--parent] [--new-scope] [--with-children] [--message] [--dry-run]` | move tickets into another shortcode, renumbered |
| `archive` | `[--ids] [--filter] [--max] [--message] [--dry-run] [--tz]` | out of every view with the subtree |
| `restore` | `[--ids] [--filter] [--max] [--with-children] [--message] [--dry-run] [--tz]` | back, with archived ancestors |
| `delete` | `[--ids] [--filter] [--max] [--children] [--unblock] [--message] [--tz]` | gone for good, a tombstone kept (admin) |
| `undelete` | `--ids` | back within 30 days (admin) |
| `freeze` / `unfreeze` | `--id` | a scope that takes no new children or numbers (admin) |
| `history` | `--id` | each status and kind transition, then the queue's lines, then `history_end` |
| `may` | `--id --role` | whether you hold `viewer`, `editor` or `admin` on one ticket, as a write's permit would say: `may { id, role, held }`, or `not_found` for a ticket you cannot see (V2-24: what another service asks through the seam) |
| `frame_reach` | `--id` | what a frame of the ticket's body may read for you — your reach ∩ who last wrote the body's ∩ its `reach-read:`: `frame_reach { reach: { ticket, author, asked, alone, shortcodes, tickets } }` (COMP-16, "Capabilities, reads, watches and writes") |
| `inputs list` | `[--ticket] [--epic] [--state] [--filter] [--tz] [--limit] [--after]` | a page of a ticket's questions, or of every ticket's |
| `inputs ask` | `--ticket --question` | a question filed as a text input of its body |
| `inputs answer` | `--ticket --input --value [--rationale]` | an answer, checked against what the input declares |
| `inputs clear` · `close` · `reopen` | `--ticket --input` | open it again · freeze it · thaw it |
| `queue` | `--id --urgency --reason [--input] [--blocking] [--source]` | in front of the owner; again raises, never lowers |
| `unqueue` | `--id [--source]` | out of the queue |
| `snooze` | `--id --minutes [--source]` | off the alerting surfaces, still queued |
| `request_changes` | `--id --body [--source]` | handed back: the words on the queue's log, the entry out |
| `queued` | `[--id] [--snoozed] [--limit] [--after]` | a page of the queue, most urgent first, then oldest |
| `await_reply` | `--id [--since] [--input] [--timeout-minutes]` | an agent waits for the first response |
| `watch` | `[--epic] [--filter] [--membership] [--revisions] [--tz]` | every change, open-ended |
| `commits link` | `--commits [--id] [--ids]` | the commits that finished a ticket |
| `commits unlink` | `--id --commits` | take them off |
| `filters save` · `list` · `get` · `rename` · `delete` · `check` | `--name --filter` · — · `--name` · `--name --to` · `--name` · `--filter` | saved filters (FILTER-46) |
| `nav groups` · `extents` | — | one roll-up per shortcode · where each shortcode ends |
| `nav tree` | `--filter [--epic] [--parent] [--matches-only] [--keep] [--after] [--limit] [--tz]` | the nav under a filter |
| `nav counts` | `[--filter] [--keep] [--tz]` | matches per shortcode |
| `nav facets` | `[--base] [--filter] [--tz]` | the menus' counts per field and value |
| `nav member` | `--ids --filter [--tz]` | which of these tickets a filter admits |
| `revisions list` · `get` · `diff` · `restore` | `--id [--before] [--limit]` · `--id --rev` · `--id [--from] [--to] [--text]` · `--id --rev [--fields] [--message] [--source]` | a ticket's revisions (TICKETS-7) |
| `subtasks create` | `--parent --title [--kind] [--body] [--blocked-by]` | the next free `<parent>.N`, hidden from the nav |
| `subtasks list` | `--parent [--after] [--limit]` | a ticket's subtasks |
| `work ready` | `[--epic] [--filter] [--after] [--limit] [--summary] [--fields] [--tz]` | the ready set |
| `work waves` | `--epics [--kinds]` | the order of work, in waves |
| `work waves_under` · `waves_to` | `--id [--kinds]` | under one ticket · up to one ticket |
| `work blockers` | `--id` | a ticket's blockers at their statuses |

### `members`, `orgs`, `session`, `auth`

| method | flags | what it does |
|---|---|---|
| `members mine` | | who you are, your teams, your grants |
| `members list` | `[--scope]` | the grants on the scopes you administer |
| `members grant` | `--scope --role [--principal] [--team] [--everyone]` | a role on a scope |
| `members revoke` | `--scope [--principal] [--team] [--everyone]` | take it away |
| `members requests` | | who signed in holding nothing (admin of `*`) |
| `members pairing` | `--code` | the machines waiting on a pairing code (TETHER-12) |
| `members teams list` · `show` · `create` · `rename` · `delete` · `add` · `remove` | `[--org]` · `--team` · `--name [--org]` · `--team --to` · `--team` · `--team --principal` · `--team --principal` | teams (ACCESS-22) |
| `orgs list` · `create` · `rename` · `delete` | — · `--name [--admin]` · `--name --to` · `--name` | organizations (ORG-4); each person's own, its `owner` (V2-41) |
| `session whoami` · `providers` · `login` · `logout` | — · — · `[--ttl-secs] [--scope]` · — | the connection's identity; an agent's session |
| `auth providers` · `login` · `signup` · `whoami` · `logout` · `requires` | — · `--flow` · `[--via]` · — · — · `--method` | the door (AUTH-7) |
| `auth roles` · `role_set` · `grant` · `revoke` · `waiting` | — · `--role --grants` · `--principal --role --scope` · `--principal --scope [--role]` · — | roles and grants (admin) |

### `ui`

| method | flags | what it does |
|---|---|---|
| `push` | `--bundle --manifest --signature [--commit] [--channel]` | store a bundle by hash, compiled and checked, and make it live on this environment's channel, in one transaction |
| `promote` | `--to [--from]` | UI-15: one channel's live pointer onto the version another has live — a release to prod, no second push |
| `rollback` | `--to [--channel]` | move the live pointer to a kept version |
| `versions` | `[--channel]` | every kept version, newest first, one `live: true`; each says which `channels` name it |
| `preview` | `[--version]` | serve a kept version to the caller's sessions alone; none clears it |
| `watch` | | `live` on every pointer move, `slot_changed` on every section change |
| `slots list` | `[--layout]` | every section's assignment and the default it overrides |
| `slots set` | `--layout --slot --component [--version]` | fill a section |
| `slots clear` | `--layout --slot` | back to the default |

### `media` (V2-24, EDITOR-10)

| method | flags | what it does |
|---|---|---|
| `put` | `--data [--name] [--ticket] [--doc] [--mime]` | keep any file in one message (at most 7 MiB, `data` standard base64) for a ticket you may edit, or (V2-29) a doc you may write (`docs.may`) — or, with neither, as yours: `stored { hash, mime, kind, name, bytes, width, height, duplicate }` |
| `begin` | `--size [--name] [--ticket] [--doc]` · `--upload` | start an upload of any size, or resume one of yours: `upload { upload, size, received, chunk }` |
| `chunk` | `--upload --offset --data` | the next bytes (at most `chunk`, 4 MiB) at `offset` (what the upload has): `chunked { upload, received, size, duplicate }`; a gap `refused { reason: out_of_order }` |
| `finish` | `--upload` | the upload hashed and kept: `stored { … }`, or `refused { reason: incomplete }` |
| `get` | `--hash` | a file's bytes up to 7 MiB: `media { …, data }` (data base64); larger, `refused { reason: too_large }` — read it at `/media/<hash>` |
| `info` | `--hash` | what a file is, without its bytes: `info { hash, mime, kind, name, bytes, width, height }` |
| `usage` | `[--principal] [--everyone true]` | what you have put, each file once: `usage { principal, files, bytes }`; V2-31: an admin of `*` names anyone, or every keeper — V2-69: each file against each principal who put or fetched it, then `unattributed { files, bytes }`, what no one is charged for (what `quota.recount` reads) |
| `unfurl` | `--url` | a public link read into a card: `unfurled { url, kind, title, description, site, image, provider, embed }` |
| `fetch` | `--url [--ticket] [--doc] [--name]` | a remote image or file kept as yours (never hot-linked): `stored { … }` |

EDITOR-10: **any file.** Its type is read from its own magic bytes, never
from `mime` or `name`, and drawn by its `kind`: `image` (PNG, JPEG, GIF,
WebP) inline as `![name](media:<hash>)`; `video` (MP4, QuickTime, WebM) and
`audio` (MP3, Ogg, WAV, M4A, FLAC) as players, `pdf` in the browser's
viewer, any other `file` (`application/octet-stream`, an SVG or an HTML
page among them) as a card with its name, size and a download — each as
`[name](media:<hash>)`. **`GET /media/<hash>`** serves media inline and
anything else as an attachment, always `nosniff`, framed by this origin
alone and (but a PDF) under a sandboxing policy, with one `Range` (`206`)
so a video seeks. A file larger than one message goes up in chunks and
resumes after a dropped connection (`begin --upload <id>` answers what it
has); one untouched for a day is dropped. There is no fixed cap: every
byte is asked of the uploader's allowance first (`media.bytes`; a link read
counts `media.fetch.calls` and `media.fetch.bytes`, a byte served
`media.egress.bytes`) — one call site, `menger_media::Allowance`, answered
by the quota service (V2-31, "`quota`"; `src/quota_allowance.rs`), its
`commit` the amount kept;
`--media-max-bytes` caps one upload when the operator wants a cap.

**Where the bytes live** is one trait, `menger_media::BlobStore` (put; a
resumable upload — begin, append at an offset, seal, commit under its
sha256; a range; stat; delete; orphans; and `presign`). The metadata —
hash, type, name, size, owners, uploads — stays in `media.db`. One
implementation ships: the disk, `<root>/<sha256>` (`--media-store disk`,
the default; `--media-blob-dir` / `MENGER_MEDIA_BLOB_DIR`, by default
`media/blobs` beside `media.db` — another disk or a network mount works
unchanged). A store that signs addresses (an object store) makes
`/media/<hash>` redirect to one after the permit and egress checks; the
disk signs none and the shell streams. `--media-store s3` is refused until
V2-32 builds it.

**Links.** `unfurl` and `fetch` read from the public internet alone: http
and https, no credentials; the host resolved by the server and every
address it gives checked — never loopback, private, link-local (the
metadata address `169.254.169.254` and `fd00:ec2::254` among them), CGNAT,
unspecified, multicast, documentation or reserved space, an IPv4 address
inside an IPv6 one judged as itself — the connection pinned to the
addresses checked, no proxy; redirects followed by hand, at most 3, each
checked again; 5 s and 5 MiB for a page (10 s and 7 MiB for a fetched
file); the content type checked. YouTube, Vimeo, X, GitHub and Figma are
read by their oEmbed, and the first four answer an `embed` — a player
address built by menger on the provider's own host, the only frames the
page's policy allows besides its own. An unfurl is kept a day.

**Whose it is**: an image is answered to a reader the tickets service says
sees a ticket it was put for (`tickets.may { id, role: viewer }` through
the seam, as the reader — ACCESS-10: an image out of reach and none are one
answer); put with no ticket (what the editor pastes before its ticket
exists) it is its putter's, and anyone's holding `media.get` on `*` (an
admin of `*`). `media.get` is a viewer's grant, `media.put` an editor's; a
viewer's put is the gate's `forbidden`, a stranger's put on a ticket
`not_found` — the permit is asked, never read.

Kept in the service's own file, `media.db` (`--media-db-path`,
`MENGER_MEDIA_DB_PATH`; default beside the tickets file, never the tickets
file and never the ui file): `media` (a row per file; an image kept before
EDITOR-10 holds its bytes there, every later file's are in the blob
store), `media_owners` (a row per ticket, or putter, it is for),
`uploads` and `unfurls`, made on first open. Through synapse the data is a file, never `"$(base64 …)"` (Linux caps
an argument at 128 KB):

```sh
base64 -w0 shot.png > shot.b64
synapse -P 4460 menger media put --ticket KILN-4 --name shot.png --data @shot.b64        # → stored { hash, … }
synapse -P 4460 menger tickets amend --id KILN-4 --body @body.md                        # body.md says ![shot](media:<hash>)
synapse -P 4460 -j menger media get --hash <hash> | jq -r 'select(.content.data) | .content.data' | base64 -d > back.png
```

### `docs` (V2-29)

The owner, 2026-11-02: *"I would like us to move the docs onto a new docs
api. same fe though."* — and, on V2-29's `pairing`: *"live co editting. we
should see that they have a connection open on that document too. we should
keep all changes over time too, so we could replay how it got built. with
the ability to squash, permission gated"*. A **document** is an id (8
characters of Crockford's base32 — TICKETS-35's form, `7k3mq2xa`; the app
opens it at `#/doc/<id>`), the organization it is in, a title, and a body:
a **Yjs CRDT** (yrs on the service, yjs in the browser, the same v1 updates)
whose markdown is derived on every change for `get`, `list`, the search,
agents and image references.

| method | flags | what it does |
|---|---|---|
| `create` | `--title [--body] [--org]` | a document in an organization you write in (else the first): `created { doc }` |
| `get` | `--id` | `doc { doc }` — `{ id, org, title, body, bytes, revision, created_at, created_by, at, by }`; `not_found` for one you do not read |
| `list` | `[--query] [--limit] [--after]` | the documents you read, newest made first, without bodies; `query` keeps titles holding every word: `rows { items: [doc] }`, `page { next, remaining }` |
| `update` | `--id --update [--session]` | a Yjs v1 update (base64) of the body — the editor's live typing — merged and kept: `applied { id, revision }` |
| `save` | `--id --body [--base]` | a whole body (an agent's, a script's): diffed from the text it read (`base`, the revision read) and made into the CRDT's own edits, so it **merges** with live typing instead of overwriting it: `saved { id, revision, at, by, stale? }` |
| `state` | `--id [--vector]` | what a copy whose state vector is `vector` lacks: `state { id, update, vector, revision }` |
| `rename` | `--id --title` | `renamed { id, title, revision, at, by }` |
| `history` | `--id [--limit] [--before]` | every change, newest first, kept with who and when (`at` in ms): `change { change: { seq, kind, at, by, principal, bytes, title?, authors? } }` — `create`, `update`, `save`, `rename`, `delete`, `snapshot` |
| `replay` | `--id [--seq] [--at]` | the document as it stood after change `seq` (or at the moment `at`, ms): `doc { doc }`; squashed away: `not_found { revision }` |
| `restore` | `--id --seq` | the body put back as it stood after `seq`, merged as a save: `saved { … }` |
| `squash` | `--id --until` | the changes up to `until` collapsed into one `snapshot`, its `authors` keeping who wrote what (changes, bytes, first, last): `squashed { id, until, removed, at, by }`, said on `watch`. Its own grant, `docs.squash` — admin's by default, grantable to a role by the role table |
| `present` | `--id` | who is on it: `present { present: { session, principal, name, agent, colour, client?, awareness?, at } }` |
| `here` | `--id --session [--client] [--awareness]` | your caret (a Yjs awareness update, base64), said to everyone on it |
| `delete` | `--id` | admin's: `deleted { id, at, by }`; its changes stay in the file |
| `may` | `--id --role` | `may { id, role, held }` — the question the media service asks |
| `usage` | `--everyone true` | admin of `*` (V2-31): one `usage { principal, count, bytes }` per writer — the documents they made and the bytes their changes keep (a squash's snapshot is nobody's); what `quota.recount` reads |
| `ask` | `--id --from --to --text [--to-whom]` | V2-30: a question on characters `from..to` of the text, anchored in the CRDT so it follows the words through every edit: `asked { thread }` — `{ thread, id, anchor { from, to }, from, to, detached, quote, question, to_whom?, by, at, resolved?, replies }` |
| `reply` | `--id --thread --body` | an answer (markdown: links, images): `replied { thread }` |
| `resolve` | `--id --thread [--open]` | the thread folded (or opened again): `resolved { thread }` |
| `threads` | `--id` | `thread { thread }` each, by where its range stands, the detached (text deleted whole) last |
| `waiting` | `[--to]` | the open questions asked of someone (`agent`, or a name) in the documents you read, oldest first — an agent's queue |
| `watch` | `[--id] [--session]` | `watching`, then `created`, `updated { id, seq, update, by, session? }`, `renamed`, `deleted`, `squashed`, and on one document `present` and `left { session, client? }`, and (V2-30) `asked`, `replied`, `resolved` with the thread whole; with `session` the watch is your presence while open, and your own updates are not said back; held to you by the gate as `ui.watch` is (`watch_closed { reason }`) |

**Who may**: a viewer reads, follows and is present (`get`, `list`,
`state`, `history`, `replay`, `present`, `here`, `watch`, `may`), an editor
writes (`create`, `update`, `save`, `rename`, `restore`), an admin deletes
and squashes — each grant held on the document's organization (`acme:`) or
on `*`. The gate admits a call whose grant is held somewhere; the method
asks it again on the organization: a document you do not read is
`not_found`, one you read and may not write `refused { forbidden }`.

**Questions on a range** (V2-30; the owner: *"I want to have the ability to
highlight a line in a doc and ask a question"*): a selection on the surface
and **ask** (mod+enter) keep a question on that range, to anyone or to an
agent; the margin beside the text holds each thread at its range's height
(a list under the text below 1400 px), its answers live, a box to answer
(an image pastes) and *resolve*, which folds it (the margin counts the
resolved). The anchor is two Yjs relative positions in the CRDT — the page
resolves them in its own copy, the service in its — so it follows the text
through every hand's edit; its text deleted whole, the thread is detached
and listed last. A question asked of an agent waits in `waiting` and is said
on every watch of the organization's documents; the agent answers with
`reply`, or edits the document itself. Kept in `docs.db`'s `doc_threads` and
`doc_replies`.

**Presence**: a page's `watch { id, session }` is its presence for as long
as it is open (gone, `left`, when the connection closes); a writer with no
watch — an agent's `save` — is present while it writes and idles out 30 s
after; a held one idles out after 10 minutes of silence.

Kept in the service's own file, `docs.db` (`--docs-db-path`,
`MENGER_DOCS_DB_PATH`; default beside the tickets file, never the tickets,
ui or media file): `docs` (each document now, its derived text, and a
snapshot of its CRDT every 64 changes so a load applies only what came
after) and `doc_log` (every change, with who and when). Every byte written
is reserved against a meter first (`docs/src/quota.rs`, answered by the
quota service, V2-31: `docs.bytes` per change, `docs.count` per document;
over the cap `refused { reason: quota }` with the numbers).

The tracker's older documents — tickets of kind `doc` — stay tickets
(V2-29's `existing`, queued to the owner; built as *leave*).

```sh
synapse -P 4460 menger docs create --title "The kiln" --body @kiln.md           # → created { doc: { id: "7k3mq2xa", … } }
synapse -P 4460 menger docs save --id 7k3mq2xa --body @kiln.md --base 3          # merged with whoever is typing
synapse -P 4460 menger docs history --id 7k3mq2xa --limit 20
synapse -P 4460 menger docs replay --id 7k3mq2xa --seq 10
```

### `quota` (V2-31)

The owner, 2026-11-02: "we should have per user allowances for just about
every data source input. and be able to tabulate their total storage usage.
use that to measure and prevent them from exceeding their cap."

| method | flags | who | what it does |
|---|---|---|---|
| `usage` | `[--principal]` | anyone signed in (their own); an admin of `*` names anyone | one `usage { principal, exempt, bytes, sources }`: per source its `unit`, `window`, `used`, `held` (reserved, in flight), `cap` (none: unlimited) and `from` (`own`, `team:<name>`, `default`, `exempt`); `bytes` is what they store |
| `table` | `[--org] [--sort bytes\|principal\|<source>]` | admin of `*` | the tabulation: `box { used_percent, warn_percent, full_percent, state }`, one `usage` per principal (with `org`, everyone granted a role in it), sorted, then `total { principals, bytes, sources }` |
| `set` | `--principal \| --team \| --default true`, `--source --cap` | admin of `*` | an allowance: `unlimited`, a number, `2 GB` (powers of 1000; `GiB` of 1024), or `clear`: `allowance { holder, source, cap, cleared }`. A cap below what is used blocks new writes and deletes nothing |
| `recount` | `[--principal]` | admin of `*` | usage rebuilt from the stores, each through its own read: `drift { principal, source, was, counted }` per total that moved, then `recounted { principals, drifted, sources, not_counted }`; the server runs one nightly and logs the drift |
| `reserve` | `--principal --source --amount` | the server's own caller (admin of `*`) | the seam's first step: `reserved { ticket, remaining, expires }`, or `over_allowance { source, used, cap, asked }`, `too_many_at_once { source, open, max }`, `box_full { used_percent, full_percent }` — nothing held |
| `commit` | `--ticket [--amount]` | the server's | the write landed: `amount` (what was kept, at most what was held; none: all) counts — `committed { …, used }` |
| `release` | `--ticket` | the server's | the write did not land: nothing counts |
| `credit` | `--principal --source --amount` | the server's | a deletion gives bytes back: `credited { used }` |

**Sources** — a name, a unit, a window (`quota/src/sources.rs`; an unknown
name is refused, so a new input registers one):

| source | unit | window | counted when |
|---|---|---|---|
| `media.bytes` | bytes | total | a file is put, uploaded or fetched (`media.put`, `media.begin`, `media.fetch`): against each principal who keeps it, once — V2-69: a duplicate of a file someone else kept is charged to you too (your allowance is what you keep, whoever kept it first; the disk keeps it once), the same file again for another ticket is not. Not charged: a link's preview (`media.unfurl`, kept as anyone's — the server's cache of a public page, one per link), and a ticket's file from before uploaders were recorded; `quota.recount` names both in `not_counted` |
| `media.fetch.bytes` | bytes | day | the server reads a file for you (`media.fetch`) |
| `media.fetch.calls` | calls | day | the server reads a link for you (`media.unfurl`, `media.fetch`) |
| `media.egress.bytes` | bytes | day | `/media/<hash>` serves bytes to you |
| `docs.bytes` | bytes | total | **the update log as stored**: each change's bytes (`docs.create`, `update`, `save`, `restore`, `rename`) its writer's; a squash's snapshot shared among its authors by what each wrote, so a squash shrinks it and credits each author back |
| `docs.count` | count | total | a document is made (`docs.create`) |
| `tickets.bytes` | bytes | total | **what the store keeps**, not every write: each body's content the first time it is kept as a revision (a copy of one kept already costs nothing) is its first writer's, and each ticket's current body its last writer's. A write reserves the delta — the new content's bytes, plus the new body less the old when the old was the writer's own (else the old body's writer is credited it back); a write that leaves the body as it was counts nothing; a write that replaces its author's latest revision (V2-37, below) lets go of the body that revision alone held, and its keeper is credited it back. The store keeps a body twice — the ticket's own row and its revision's copy in `revision_bodies` — so a new ticket with a 4,321-byte body counts 8,642 B (8.6 KB) |
| `tickets.count` | count | total | a ticket is created |
| `questions.count` | count | total | a question is asked on a document (`docs.ask`, V2-39): one held for the asker before it is kept, committed after, released when the ask is refused; past the cap `refused { over_allowance }`. Not recounted: no store counts it yet |

**The effective cap** of a principal on a source: their own allowance, else
the largest of their teams' (unlimited the largest), else the default for
the source, else unlimited. Admins of `*` and the operator are **exempt** —
counted, never capped. An agent's key counts as itself: nothing in
`members` says whose a key is, so a key has the default of its own.

**Reserve, then commit or release** (`quota/src/ledger.rs`). Each reserve is
one `IMMEDIATE` transaction that sweeps the expired, then reads what is used
and what is held and writes the hold, so parallel reserves near the cap pass
exactly what fits; a commit moves its hold into usage in one transaction. A
hold not committed within the plan's hold time (an hour) — an abandoned
upload — is swept, by the next reserve and by the service's minute sweep. A
day's counter starts at nothing each UTC day.

**The defaults are data** — a plan, read once into a fresh `quota.db`: the
built-in `quota/plans/free-1k.json` (the owner's answer to `defaults`, "1k
concurrent free users per day on a decent sized computer": media 2 GB,
documents 200 MB, tickets 50 MB, 300 fetches and 200 MB fetched a day, 2 GB
served a day, two uploads at once), or `--quota-plan FILE`. After that the
file's rows are the truth and `quota.set` moves them.

**The box guard.** Writes that store (a total source) by anyone capped are
refused `box_full` once the data disk — the one holding `quota.db` — is past
the plan's full line (85 %); past its warn line (70 %) `quota.table`'s `box`
says `warn` and the log says so hourly.

**Refusals are in band and typed**, with nothing written: the tickets
service answers a write over the cap `over_allowance { source, used, cap,
asked, message }` (or `box_full`); the media service `refused { reason:
over_allowance, max: <what is left>, message }`, the message naming the
numbers ("1.9 GB used of 2 GB, and this asks 300 MB more"); `/media/<hash>`
past the day's egress is a 429.

```sh
synapse -P 4460 menger quota usage
synapse -P 4460 menger quota table --sort bytes
synapse -P 4460 menger quota set --default true --source docs.bytes --cap "200 MB"
synapse -P 4460 menger quota recount
```

The pages: `#/usage` (yours: a bar per source, used against the cap;
`#/usage/:principal` an admin's look at another) and `#/quota` (an admin's
table of everyone — sortable by stored or a source's use, the cap edited in
place).

## Tickets

**Kinds.** `scope | execution | build | gap | spike` (AGS-13, the owner's
closed five: a scope decomposes into children, a build produces capability,
an execution carries out a procedure, a gap is a defect in shipped
behaviour, a spike answers a question) and **`doc`** (SDLC-11), which is
not work: born in the status `Doc`, which only a doc holds, and never moved
(`set_status` on a doc is refused, and `Doc` on any other kind); never
blocked, never a blocker, never ready; a roll-up's `done/total` ignores it
(`docs` counts it apart). The first ticket of a shortcode is a scope or a
doc.

**Statuses.** `Draft`, `Pending`, `Ready`, `Coding`, `In Review`,
`Blocked`, `Complete`, `Epic`, `Superseded`, `Canceled`, `Doc` — canonical
spellings; `set_status` reads `active` as `Coding` and `in review` as `In
Review` through `-p`, and synapse's `--status` flag takes the canonical
ones only (ACCESS-8). A ticket is born `Draft` (TICKETS-13).

**A kind changes only through `set_kind`** (SDLC-13), which moves
everything kept by kind in one write: to a doc the status becomes `Doc`
(refused while it waits on a blocker or one waits on it); from a doc the
status becomes `--status` when given, else `Epic` for a scope and `Pending`
otherwise; a shortcode's root never becomes a kind that roots nothing.

**`update`** (MENGER-30) changes any part of a ticket in one write — each
aspect by its own write's rules (`amend`, `set_kind`, `set_status`,
`set_parent`, `create`'s for blockers), all asked before anything is
written, a refusal naming every reason. `--severity none` clears the
severity; `--block` / `--unblock` add and remove blockers. **`update_many`**
(ACT-2) is the same to many tickets (`--ids`, or a `--filter` at most
`--max`, default 500), all or none; `label` and `commits link` take `--ids`
too.

**`delete`** (ACT-5, admin) is refused while children or a waiting ticket
point at one, unless told what moves (`--children reparent|delete`,
`--unblock true`); a tombstone and the history stay, `get` answers
`deleted`, the body is kept 30 days for `undelete`, and a number is never
reused. Prefer `Canceled` or `Superseded` for work that is not done.

**`reshortcode`** (ACT-6) moves tickets into another shortcode, renumbered
in the order given; the old id is pinned to the new ticket (`get` says
`moved_from`). **`rename`** (TICKETS-26) is the same write into a subtask id,
`<parent>.N`, under its own parent. **Subtasks** (TICKETS-28) are
`<parent>.N` tickets, `hidden` from the nav's views (TICKETS-27), listed by
`subtasks list` and `list --parent`. **`freeze`** (TICKETS-6, admin) stops a
scope taking new children and its shortcode new numbers until `unfreeze`.

**History** (MENGER-13). Every status and kind change is a transition — who,
from where (`source`: `app`, `api`, `pipeline`, `cascade`), why (`message`),
caused by what — read with `history --id X`. `set_status … Canceled`
cancels the ticket's open descendants in the same write; leaving `Canceled`
with `--restore-children true` puts back what that cancel canceled.

**Revisions** (TICKETS-7). Every write that changes a ticket's own fields
keeps a revision in its transaction; `revisions list | get | diff |
restore`; a restore is an `update` and itself a revision. `watch
--revisions true` adds `revised`. A new write path goes through the store's
`upsert` / `update_row` / `set_body` inside a write runner, or it keeps no
revision (`tickets-api/src/store/revisions.rs`). **Coalesced** (V2-37): a
write by the same principal to the same ticket within 2 minutes
(`COALESCE_WINDOW`, 120 s) of its latest revision **replaces** that revision —
the same number, the fields of both, the write's moment and body — so an
editor autosaving every pause keeps one revision per sitting, not a copy
per pause. Another is kept when the writer changes, after 2 minutes, and
whenever either write moves the status, kind or parent or says why (a
`--message`; a restore always does) — a birth, a delete and an undelete
are always their own. A body only the replaced revision
held is let go.

**The archive** (ARCHIVE-2) is a mark apart from status: `archive` takes ids
or a filter and archives each with its whole subtree; `restore` brings a
ticket back with its archived ancestors (`--with-children`: the subtree
archived with it). An archived ticket is in no view unless the filter names
`archived`; `get` still answers it. `--dry-run true` writes nothing.

**Search** (SEARCH-2) is SQLite FTS5 in the same file: every word required,
prefixes and near typos too, best first, each `hit` with its passage; a
`"quoted phrase"` is exact. `find` stays the jump by id or title.
`--verify-derived` holds the index to the tickets.

**The commits that finished a ticket** (SDLC-9): `commits link --id X
--commits 77df9a0` (`repo@hash` for a commit outside menger), compared as
text — the server has no repository.

### Filters

One flag in the filter language (FILTER-3, `tickets-api/src/filter.rs`),
answered from the kept cells, never by testing a ticket:

- **Predicates**: `status <name>`, `category <open|ready|active|review|done|dropped|static>`,
  `kind <name>`, `severity <…>`, `blocked`, `ready`, `has commits`,
  `menger doc`, `label <name>`, `in <SHORTCODE>`, `under <ID or SHORTCODE>`,
  `epic <name>` (`epic any`), `archived`, `frozen`, `hidden`, `ticket <ID>`,
  `saved "Name"` and `text "…"` (on rows; alone, as a top-level `and`
  group).
- **History** (MENGER-13, FILTER-52): `entered <status> [window] [by who]`,
  `left …`, `moved …` (any transition), `changed …` (the last write),
  `created …`, `by …`, `archived …`, `restored …`; a time is a modifier on
  the rule before it (`status Complete after 2026-10-01`, `kind gap within
  7d`). Windows: `within 12h`, `today`, `yesterday`, `this week`, `after`,
  `before`, `between`; UTC unless `--tz Europe/Berlin`.
- **Groups**: `in (A, B)`, `not …`, `(a or (b and c))` to any depth
  (FILTER-36); a display part after `·` (`tree`, `flat`, `columns a, b`,
  `order code`).
- **Saved filters** (FILTER-46): `tickets filters save --name N --filter F`,
  read back as `saved "N"`, expanded by the server.

A word of a service v2 cut — `chatting`, `unread`, `deployed`,
`deploying`, `failed`, `behind`, `ready for`, `requested`, `waiting`,
`assigned`, `unassigned` and the rest of `filter.rs`'s `GONE` — is refused
by name, never read as another cell.

### Questions and the queue (V2-20)

A **question** is an input of a ticket's body — `<m-decision id>` (a choice),
`<m-input id kind="multi">`, `<m-input id kind="text">` — read from the body
on every write and kept beside it, with its whole history. `inputs ask` files
a text one, appended to the body in one write. An input is `open`, `answered`,
`closed` (frozen), `removed` (its body no longer declares it; its history
stays) or `withdrawn` (its ticket is canceled). `asking` is on the cell: a
ticket with an `open` input. Malformed input markup refuses the write, naming
the input.

The **queue** puts a ticket in front of the owner, with an urgency
(`whenever`, `today`, `now`, `blocking`, which names the input it is idle on)
and a reason. One entry a ticket: queueing it again raises the urgency, never
lowers it, adds the inputs, ends a snooze. Closing the last input an entry
names takes it out; so does an archive. `queued [urgency]` is the entries;
`asked` an entry someone else queued that names an open input, a question
waiting on the reader. `await_reply` holds until the first response: an
answer, changes requested, an unqueue by another, the ticket canceled,
archived or deleted.

A body is read as the app renders it, through the app's markdown parser: code,
HTML's `<!-- -->`, raw text and a page drawn whole inside a body hold no input. A reading is bounded: at
most 64 levels of marked nesting, 1,024 inputs a body and 256 options an input,
and a fixed allowance of work, about 0.7 s of it; a body past any of them is
refused in band, naming why, and a large one is read before the write lock.
`ask` declares its input or writes nothing: a body that ends inside a fence, a
`<!--` or raw text is refused.

`queued` and `inputs list` are pages cut from orders the cache keeps, so a
page costs its rows: `rows { items }`, `page { next, remaining }` and, for
`queued`, `queue_end { count, snoozed, now_unix }`. A queue page's `next` is a
place in the order, so a ticket leaving does not lose the walk. The app's
views are `#/queue` and the ticket page's questions.

## Access

### Typed parameters (ACCESS-8)

Every parameter is a type (`tickets-api/src/vocab/`) with one rule — a
ticket is a `TicketRef`, a filter a `FilterText`, a status a `Status`, a
title a `Title` (≤ 1 KiB), a body a `Body` — and every activation is
declared `typed_params` (MACRO-12), so a bare `String` parameter does not
compile (`tickets-api/tests/param_types.rs`). A bad value is `-32602
invalid_params` naming the parameter, before the method runs; what needs the
store (a ticket that is none, a cycle) stays an in-band `error`. A blank
optional value is refused, not none. A key no parameter claims is refused
the same way (MACRO-31): `tickets.list {sumary: true}` is ``Invalid params:
unknown field `sumary`; tickets.list takes: after, at, epic, fields, filter,
frame, keep, limit, parent, status, summary, tz`` — every unknown key named, the
parameters sorted, a child's method by its own namespace (`nav.tree`, not
`tickets.nav.tree`) — and `unknown field` is the words synapse reads to
re-read a stale cached schema and run the call again.

### Capabilities, reads, watches and writes (ACCESS-9..12)

What the gate does not decide, menger proves with a **capability**, a
value only one module can make (`tickets-api/src/access/`):

| type | holding one proves | minted by |
|---|---|---|
| `VerifiedUser` | signed in, the sign-in alive, the principal parsed, the grants resolved into a `Reach` | `Access::verify`, the frame's resolver |
| `Permit<Viewer \| Editor \| Admin>` | a role on the ticket it names, by the kept tree | `VerifiedUser::permit` |
| `AdminOf` | admin on the scope it names | `VerifiedUser::admin_of` |
| `System` | the holder opened the store and acts for no user | the store, once |

None has a public field or constructor (`compile_fail` doctests hold it);
`for_tests` constructors exist under `cfg(test)` and the `test-support`
feature. `Hidden` is the one answer for a ticket that is not there and one
that is not the caller's.

- **Reads** (ACCESS-10, `store/seen.rs`) take their reader — a
  `VerifiedUser` or the `System` — and answer only its reach: a hidden
  ticket is `not_found`, never listed or counted, and a visible row names
  no hidden ticket (`hidden_blocked_by`, `hidden_unlocks`, `parent_hidden`
  count them). `store/tests_reach.rs` holds it.
- **A body's frame reads no wider than whoever wrote it** (COMP-16,
  `store/frames.rs`): a body's script runs with its reader's eyes, so what
  its frame reads is the reader's reach ∩ the reach of the principal who
  last wrote the body (its latest revision that created it or changed its
  body; that principal's grants alone, `Access::grants_reach`) ∩ what the
  body asks — `reach-read:` in its frontmatter, FILTER-3's language, `all`
  as wide as allowed, and without it its own shortcode with those hosted
  under it. A body nobody known wrote (`system`, `anonymous`, a `baseline`),
  or one whose ask the language does not take, reads the ticket alone; an
  author who holds nothing reads nothing. `tickets.frame_reach { id }`
  answers it; `get`, `list` and `search` given `--frame <id>` read through
  it (a `Framed` reader, sealed, only ever narrower than its caller). It is
  read afresh each call, so another author's amend moves it.
  `store/tests_frames.rs` and `tests/wire_frames.rs` hold it. The page's
  frames (`m-mockup`, plexus-ui's `bus.frame`) do not name `frame` yet: the
  engine passes a framed page's params as the page wrote them, so the
  bound waits on the framer being able to fix one.
- **Watches** (ACCESS-12, `access/following.rs`, `store/watching.rs`) hold
  their caller and are verified again when the grants generation moves, a
  sign-out or the expiry comes; a moved reach says `entered` / `left`, a
  refused one ends with `watch_closed { reason }`. Every change is cut by
  `Store::heard`, whose `match` has no `_` arm.
- **Writes** (ACCESS-11, `store/writs.rs`) take the proof for what they
  write — a `Permit` carrying the ticket's id, `AdminOf` / `On<R>` for a
  scope, `System` for what acts for no user — minted in
  `activation/writes.rs` and checked by the store under its write lock.
  Store tests write as `SYS.permit(id)`, `SYS.moving(a, b)`,
  `SYS.updating(id)`.
- **Another service's writes** (V2-24) hold no `Permit`: they ask for one
  over the seam — `tickets.may { id, role }`, as the caller — and the
  tickets service answers from the same `VerifiedUser::permit` a write
  takes, `not_found` for a ticket out of reach. The media service's put is
  the first (`media/src/streams.rs`).
- **Every storing write is held to an allowance** (V2-31): before it lands,
  the writing service reserves its bytes (and count) for the caller's
  principal through the quota service's seam (`Access::meter`, the
  tickets service's `activation/metered.rs`), and commits or releases after.
  Who may is still the permit's; how much is the quota service's.
  `quota.usage` is a signed-in caller's (`SIGNED_IN_GRANTS`), the seam's
  four (`reserve`, `commit`, `release`, `credit`) the server's own — no
  role holds them — and `table`, `set`, `recount` and
  `tickets.revisions.usage` an admin's of `*`.

### The frame (ACCESS-13)

Every activation of menger's is declared `auth = "required", request =
MengerRequest, typed_params, roles(…)`; every method takes `#[from_auth(self.access.verify)]
user: VerifiedUser`, resolved before its body runs and before a parameter is
read; its description and each parameter's are `///` docs; and its body is
at most 15 lines — a permit or a scoped read, then `answered!` or a
`*_stream` beside it (`tickets-api/tests/sizes.rs`). A child (`tickets
commits`, `nav`, `work`, …) is a file beside the activation mounted with
`#[plexus_macros::child]`, on the wire as `tickets.<child>.<method>` and
gated by that whole path (MACRO-20).

### Addresses and namespaces (ORG-3)

A shortcode is unique within its namespace — its organization and the
shortcode it is hosted under — so two shortcodes named THEME may exist under different
hosts. A ticket carries `epic` (the letters), `shortcode` (the store's
identity: the code, or `CODE.N` for a namesake) and `address`
(`hypermemetic:MENGER/ACCESS/ORG-3`). Every `--id`, `--parent`,
`--blocked-by`, `--epic` and `under` takes any short form of the address;
an id older than ORG-3 is pinned. A name that fits two is refused in band
(`ambiguous { id, candidates }`). A code may hold hyphens (ORG-9:
`SYNAPSE-CC-1` is ticket 1 of `SYNAPSE-CC`). A scope roots its own
shortcode (ORG-5): `create --kind scope --epic NEW --parent X`.

#### A ticket's uid (TICKETS-35; on v2, V2-42)

Every ticket has a **uid** — the one name nothing changes: **8 characters
of Crockford's base32, lower case** (`7k3mq2xa`; digits and the letters but
`i`, `l`, `o`, `u`), read case-blind.

- **Minted** at `create` (a subtask, a seed's tickets and a scope
  `reshortcode --new-scope` makes too): random, never another ticket's (live
  or deleted), never a shortcode's code. `created`, and every `ticket` and
  `summary` (a `rows` page's items too), carry it as `uid`; `--fields uid`
  asks for it on a thinned row.
- **Kept** through every move: `reshortcode` and `rename` rewrite the key
  (`id`), never the uid; `set_parent`, an archive and its restore keep it; a
  `delete` keeps it on the tombstone (`tombstones[].uid`) and `undelete`
  gives it back.
- **Names its ticket** wherever a ticket is named — every `--id`, `--ids`,
  `--parent`, `--blocked-by`, `--block`, `--unblock`, `--ticket`, `--keep`
  takes it bare, as the app's route to it (`#/t/7k3mq2xa/…`) or as the whole
  link; what follows the uid is ignored. A deleted ticket's uid answers its
  tombstone. The filter words take one too: `under 7k3mq2xa`, `ticket
  7k3mq2xa`. A name is resolved once, where the call names it, and a write
  holds the key from there (TICKETS-32): `amend --id
  hypermemetic:MENGER/STORE/PERF-19` lands though the bare `PERF-19` fits two.
- **`uid_of`**: a ticket or a summary that names others — its `parent`, each
  of `blocked_by` and `unlocks` — carries each one's uid, keyed as the row
  names it (`{"PERF-1": "m4c8tz1e"}`); omitted when empty.
- **`choices`**: `ambiguous` says, beside `candidates`, one choice per
  candidate — its `uid`, `id`, `address`, `org`, `epic`, `kind`, `status`,
  `title` and `parent { uid, id, address, title }`; a candidate the caller may
  not read carries its address alone.

**Stored.** A column of the tickets and of the tombstones, each under a
unique index — not derived state. A file without it gains it at its first
open; a file that has it — prod's, made by main, where v2 then wrote
tickets without one — keeps every uid it holds, and every ticket and
tombstone without one is given one at the next open, in one transaction,
the count in the log (`TICKETS-35: minted N uid(s) …`). `--verify-derived`
holds them: a ticket or a tombstone without a uid, or two with one, is a
difference.

```sh
synapse -P 4460 menger tickets get --id 7k3mq2xa
```

### Organizations (ORG-4) and epics (ORG-13, ORG-23; out of the surface, V2-25)

Every ticket and shortcode is in one organization (`hypermemetic`).
`orgs create --name N --admin P` (admin on `*`), `rename` (the old name an
alias), `delete` (only empty).

**A person's own organization** (V2-41; owner, 2026-11-04: "users can join
other orgs but they always have their personal org, and they are always
editor inside it"). The write that accepts a person — their first grant, by
name (`members grant`, `auth grant`, `orgs create --admin`), through a team,
or being added to a team that holds one — also makes theirs, in the same
transaction: `u-` and the first 10 hex of the SHA-256 of their principal
(`u-3f9a02c1d4`; longer if taken), their `editor` on it signed
`personal-org`, and `orgs list` naming them its `owner`. A person is an
`idp`, `oidc` or `nostr` principal; an agent's `key` gets none. In it they
stay editor: a grant to them there below editor is refused, a revoke of
their editor refused, of their admin puts editor back; it is renamed like
any, never deleted. So a person once accepted always holds a grant, and never
waits again. It is named, never defaulted to: a `tickets create` of a new
shortcode or a `teams create` with no `--org` goes to the caller's one
organization that is no person's own (`--epic u-3f9a02c1d4:NOTES` or `--org`
files in theirs). Every start gives one to each person who holds a grant (or is an
`--admin`) and has none — the first start of this build is the backfill
(`tickets-api/src/store/personal.rs`). A grant's scope is `*`, `org:`, a shortcode
(`org:SHORTCODE`, with those hosted under it), a ticket alone (`MNG-40`) or
a subtree (`under MNG-40`, ACCESS-21). A reader sees only the organizations
it holds a role in.

An **epic** is a level of an organization that holds shortcodes:
organization → epic → shortcode → ticket. Each organization starts with
`genesis`, whose addresses name no epic; another epic's name it
(`hypermemetic:fresh:MENGER-1`), and `create --epic org:epic:CODE` and
`tickets reshortcode --to org:epic:CODE` still address one a file holds.
**V2-25** (owner, 2026-11-01: "remove even more … epics"): the `epics`
activation went — no method lists, makes, switches or archives an epic — and
nothing narrows a read to one: the board, `list`, `nav groups` and
`extents` and a `watch` are the organization's whole set, whatever a file
keeps of ORG-13's choices and ORG-23's marks; a filter naming `epic` reads
that one, `epic any` every live one. The store keeps the level
(`tickets-api/src/store/eras.rs`, `era_archive.rs`) and the header's
switcher, `#/epics` and the `me` page went with it (`session` stays: it is
how agents sign in).

## Login (IDENTITY-14)

The login is a crate and repository of its own, [`plexus-login`](../plexus-login/README.md)
(SCAFFOLD-3, SCAFFOLD-9; beside menger by sibling path, FRAME-4): the provider specs, the login page and its
routes, the sessions, the gate at the upgrade and the `auth` backend,
generic over `LoginAccess`, which menger implements over its members and
sessions (`src/login_access.rs`). Each `--auth-provider` becomes an adapter
at the WebSocket upgrade that turns its credential into a Plexus identity;
past the upgrade there is only the connection's identity.

| `--auth-provider` | the credential | principal |
|---|---|---|
| `idp=<issuer>,<audience>` | a plexus-idp token | `idp:<uuid>` |
| `auth0=<domain>,<audience>[,<roles-claim>]` | an Auth0 access token | `oidc:auth0/<sub>` |
| `oidc:<name>=<issuer>,<audience>[,<roles-claim>]` | any OIDC provider's RS256 token | `oidc:<name>/<sub>` |
| `workos=<client-id>` | a WorkOS access token (IDENTITY-30) | `oidc:workos/<user id>` |
| `nostr=<ws-url>[,…]` | a NIP-98 event | `nostr:<hex>` |
| `key` or `key=<ws-url>[,…]` | an agent's Ed25519 key proof (below) | `key:<hex>` |

No credential is the **anonymous view** — `auth.providers`, `auth.login`,
`auth.signup` and `session.providers`, nothing else; a credential presented
and refused is a 401 at the upgrade. `--identity` puts menger's own sign-in
page over an identity backend (AUTH-22: `workos:…`; `plexus-idp:…` and
`memory` with `--env test` only). `tests/login.rs` runs each provider over
the real wire.

**The purge** (ACCESS-24). The first start of this build on a database
removes each grant planted while menger could run open (a stored row signed
`--admin`), unless an `--admin` of that start accounts for it — one
transaction, its mark in the `open_purge` table, each removal logged
(`tickets-api/src/store/open_purge.rs`).

### Agents (IDENTITY-40, IDENTITY-42)

An agent logs in with an Ed25519 key it holds (synapse's
`~/.plexus/keys/<name>.key`). The upgrade's bearer is a key proof naming this
socket (`ws://127.0.0.1:<port>`, `localhost`, the ws form of `--public-url`,
or a URL given as `key=<ws-url>`), accepted within a minute, once.
`session.login { ttl_secs?, scope? }` then mints an `mgs_` session (12 h by
default, at most 24 h) that opens later connections as the same principal;
`scope` is a ceiling, `role@SCOPE` each (`editor@MENGER-12`). synapse does
all of it: `synapse -P 4460 _login menger --key builder --as builder`, then
every call. A key is granted like anyone (`members grant --principal
key:<hex>`); `tests/agent_login.rs` runs it over the socket.

### Members and roles (IDENTITY-21)

A grant gives a principal (or a team, or everyone signed in) a role on a
scope; every read answers under the caller's filter **and** its scopes.

| role | may |
|---|---|
| `viewer` | read under the scope: `tickets`' reads and its children's (`tickets.may` among them), `orgs.list`, `ui.versions`, `ui.watch`, `ui.slots.list`, `ui.history`, `media.get` |
| `editor` | also create and change tickets: `tickets`' writes, `commits`, saved filters, `revisions restore`, `subtasks create`, `media.put` |
| `admin` | every grant: `tickets.delete`, `undelete`, `freeze`, `unfreeze`, `members.*`, `orgs.create`, `auth.role_set`, `auth.grant`, `ui.rollback`, `ui.preview`, `ui.slots.set`, `ui.slots.clear`, and the undo of a push or a rollback (FRAME-52), … |
| `deployer` | `ui.push` alone — a CI's key (V2-7: the pipeline key) |
| `member` | `auth.whoami`, `auth.logout`, `auth.requires` |

Any signed-in caller holds `session.*` and `members.mine` beside its
roles. `synapse -P 4460 menger auth roles` lists
each role's grants as the running server holds them; `auth role_set`
(admin on `*`) changes one.

### The AUTH protocol (AUTH-7)

**Every method is its own grant** (`tickets.set_status`), a **role is an
explicit list of grants** declared with `roles(…)` on each activation, and
plexus-core's **gate** decides every call before it runs — the grant, held
through a role on the scope the call's **target** lives in, within the
session's ceiling. Default deny. A write's target is the ticket it acts on;
a read has none and answers only from where its grant is held. A caller's
`_info` and every `schema` list only what it holds, each with `grant`,
`target` and `held_on`. A refusal is typed: `{type: error, error_kind:
forbidden, grant, scope, roles, held_on, message: "needs tickets.amend on
OUGHT"}`. `tests/auth_protocol.rs` runs it over the built binary.

### Limits and caps (ACCESS-29)

| what | limit (default) | past it |
|---|---|---|
| WebSocket connections: in all / per IP / anonymous / anonymous per IP | the `--max-connections*` flags (1,024 / 256 / 128 / 32) | the upgrade refused 429 |
| connections per principal (V2-31): a person / an agent's key (`key:`) / an admin of `*` | `--max-connections-per-principal` (4) / `--max-connections-per-agent` (64) / none — menger's call layer splits them at a connection's first call (the transport's own per-principal quota is one number for everyone, so main hands it none) | `error_kind: too_many_connections` with `max`, then the connection closes |
| calls open on one connection (a `watch` for its life) | `--max-calls-per-connection` (256) | `error_kind: too_many_calls` |
| a connection with no call open | `--idle-timeout-secs` (900) | closed |
| one message on a socket (V2-54; a UI bundle is pushed in one, base64) | `--max-message-bytes` (32 MiB; plexus-transport's own default is 10 MiB) | not taken: no answer, the connection closed |
| a browser's sign-in unused | `--session-idle-minutes` (120) | the session ends |
| sign-in flows and password guesses per client and account | AUTH-22's throttles | 429 |
| the waiting list | `WAITING_MAX` 1,000 (`tickets-api/src/access/mod.rs`) | the oldest forgotten |
| a title / a filter | `TITLE_MAX` 1 KiB / `FILTER_MAX` 4 KiB (`tickets-api/src/limits.rs`) | `invalid_params` |
| a body | `--max-body-bytes` (8 MiB) | `body_too_large` |
| writes per principal (an admin of `*` has no bucket; V2-31) | `--max-writes-per-second` (20 sustained, twice that at once) | `error_kind: too_many_writes`, `retry_after_ms` |
| what a principal stores and fetches, per source (V2-31) | the quota service's allowances ("`quota`") | `over_allowance`, `box_full`, `too_many_at_once` |
| `tickets.list`'s page | 1,000 unless `--limit`, at most 10,000 | page with `page.next` |
| a ui bundle | `BUNDLE_MAX` 16 MiB (`../plexus-ui/service/src/vocab.rs`) | `invalid_params` |

`perf/flood.mjs` floods a throwaway with upgrades and sign-ins and checks
its memory settles.

## The ui service (UI-3, UI-7, UI-9)

A pushed UI is a row by content hash in the ui service's own file, and
`/app/` serves whichever row the live pointer names: **the pointer is the
deploy**.

**The service is the framework's** (FRAME-5): `plexus-ui-service` in
plexus-ui's `service/`, by published version (V2-47), with no menger type in it. menger
registers its activation beside `tickets` (`src/builder.rs`), attaches its
seam to the running service as the server's own caller, and mounts its
routes (`/`, `/app/`, `/app/<hash>/…`: `plexus_ui_service::routes::UiRoutes`)
on menger's shell with the login's session, the stamps, the page policy and
menger's placeholder (`src/shell.rs`). Its caller is the one plexus-core's
gate admitted, and `ui.watch` follows the gate's grants generation — menger
hands its store's grants count to the gate (`AuthSetup::with_generation`),
so a `members.*` grant or revoke reaches an open watch. Its own tests run
against a tenant that is not menger (`../plexus-ui/service/tests`); menger's
`tests/ui_serve.rs`, `wire_ui*.rs` hold menger's wiring of it.

- **Its file** is `ui.db` (`--ui-db-path`, `MENGER_UI_DB_PATH`; default
  beside the tickets file, never the same file): `ui_versions`,
  `ui_pointer`, `ui_preview`, `ui_slots`, `ui_routes`, and (FRAME-52)
  `ui_labels`, `ui_tags`, `ui_moves` — made on a file of any form before.
- **One store for test and prod** (UI-15; owner, 2026-11-01: "ideally we
  would iterate on the same database and components would be promoted to
  prod"). The file is opened in WAL with a busy timeout, so two mengers on
  one host share it — on the box, both containers mount infra's
  `menger-shared` volume and name `MENGER_UI_DB_PATH=/shared/ui.db` — each
  on its **channel** (`--ui-channel`, `MENGER_UI_CHANNEL`; the default from
  `--env`: `test`, `prod`, else `default`): the live pointer it serves and
  moves is `live:<channel>` (`live` for `default`, so a dev server and every
  test serve the row they always did). A push on test moves `live:test` and
  leaves prod's page as it was; **`ui promote --to prod --from test`** (the
  deployer's, as `push` is: the pipeline key, on prod's socket, so prod's
  open pages hear `live` and swap) moves `live:prod` onto the version test
  has live in one write, kept on prod's history as `how: promote` — a
  release to prod is a pointer move, no second push. A version is one row
  whatever channels name it (`versions` rows carry `channels`; labels and
  tags are shared, and so are the sections' and routes' rows today); each
  channel's `history`, `undo` and `redo` are its own. Every pointer method
  (`push`, `author`, `rollback`, `undo`, `redo`, `history`, `versions`)
  takes `--channel`, this environment's when none; a write naming another
  channel is the admin's (`ui.rollback` on `*`), refused `forbidden` to
  anyone else. An author on test (the studio) is in the one file, promoted
  with the rest; `versions` says `authored` by this environment's own author
  key alone.
- **`push`** takes the bundle (a tar, base64, at most 16 MiB), its manifest
  and a signature: Ed25519 over the bundle's sha256 by a key named with
  `--ui-signer` (`MENGER_UI_SIGNERS`, `;`-separated 64-hex public keys; none:
  every push is refused `no_signer`). Then, before the store's one
  transaction: every `src/<name>.rhx` is compiled in the compiler module
  (when `--ui-compiler` names one) and compared with the tree the bundle
  claims (filled in when absent);
  every component's `uses` is looked up in the service it names through the
  seam (an unmounted service or a missing method: `unknown_method`, naming
  the `service`); the manifest's `contract` is compared with the hash of the
  services named — one service, its schema's own hash, several a hash over
  each (a bundle naming `tickets` alone: `tickets.schema`'s; one naming
  `orgs` or `ui` beside it, another), which `plexus-ui build
  --contract-from URL` stamps, or `--contract HASH` as given — UI-12,
  SURFACE-11: the pipeline's build asks the binary it built `ui contract
  --uses` the methods the bundle's manifest names and passes the answer so;
  `unknown`, the build's without either flag and never accepted as
  `--contract`'s value, answers `contract_matches: null`, as a manifest
  naming no service does; UI-11). The row and the live pointer move together. The same
  bytes pushed again are the same version (`new: false`). Refusals are in
  band, nothing written: `no_signer`, `bad_signature`, `bad_bundle`,
  `bad_manifest`, and the compile and lint refusals naming the component and
  line. It answers `pushed { version, uploaded, live, new, contract_matches,
  compiled }`.
- **The compiler module** is `--ui-compiler` (`MENGER_UI_COMPILER`,
  `dist/ui-compiler.wasm`), run in wasmtime with nothing linked, under fuel
  and a memory limit; a module that imports anything is refused at start.
  Without it a push stores the trees as given and says `compiled: false`.
- **Routes as data** (STUDIO-3): `ui routes list | set | clear` — a hash
  pattern (`#/two`, `#/ticket/:id`) → a component of the live version, over
  the manifest's `routes` as `ui slots` is over its `slots`; said on `watch`
  as `route_changed`, stamped into the page as `<meta name="plexus-routes">`
  and merged by the engine's router.
- **`author`** (STUDIO-3): an editor's rhx sources (`--sources
  '[{"name":…,"source":…}]'`) compiled here, added to the live bundle, signed
  by the ui service's author key — `--ui-author-key` (`MENGER_UI_AUTHOR_KEY`,
  a seed file made at first start, 0600; the image's
  `/data/ui-author.key`; none: `no_signer`) — and pushed through the push's
  own steps; it answers `authored { version, base, components, live, new,
  contract_matches, signer, signature }`. An author may place only what it
  may call: a component whose `uses` names a method the caller does not hold
  on `*` is refused `forbidden_use` (a page runs its calls as whoever opens
  it); `author` and `routes set|clear` are refused `forbidden` to a caller
  held only on a scope; `stale_base` when the live version moved while it
  was built; `live: false` moves no pointer; at most 16 sources a call. rhx only: a `svelte/1` component comes through the tool's build (Node), which the service does not have.
- **Named and tagged versions** (FRAME-52): every version flag — `rollback
  --to`, `preview --version`, `slots set --version`, `tag --version` —
  takes a hash, 6 or more of its first hex digits, or a label. **`tag
  --version V [--label L] [--tags a,b] [--untag a]`** names a version (a
  label is one per version and unique: given to another, it moves; never hex
  digits alone) and tags it; `versions` rows carry `label` and `tags`,
  `versions --tag demo` filters, `versions --live true` answers the live
  row alone; `tagged` is said on `watch`.
- **Undo is a method** (FRAME-52): **`history [--limit N]`** is the
  pointer's moves, newest first, kept in `ui.db` — `move { seq, version,
  label, how, by, at, from }`, `how` push, author, rollback, undo or redo;
  **`undo`** moves the pointer back to where the last move found it,
  **`redo`** forward again (a new move ends the redo), each answering `live
  { version, label, undone }`; `--seq` names the move the caller saw marked,
  refused `not_the_top` once it is not (two undos at once undo one move).
  `history` always lists the marked rows, with `may` for whether the caller
  may reverse them; the store keeps the last 1000 moves. `tag --label ""`
  clears a label, and a moved label is said (`tagged`) for the version it
  left too. An editor undoes an `author`, anyone's;
  undoing a `push` or a `rollback` is a rollback, the admin's. The studio's
  Undo and Redo and the releases page call these, so an undo survives a
  reload and an agent's reaches every page.
- **`rollback --to <version>`** moves the pointer to a kept version;
  **`preview`** serves one to the caller's own sessions; **`versions`**
  lists them; **`watch`** says `live` on each move, `slot_changed` on each
  section change and `route_changed` on each route's, held to its caller by the gate: `watch_closed { reason }`
  (`signed_out`, `expired`, `revoked`, `refused`) when it no longer stands.
- **Who may**: viewers and editors `versions`, `watch`, `slots list`,
  `routes list`, `here`, `follow`, `history`; editors `routes set`, `routes clear`, `author`,
  `show`, `sessions`, `tag`, `undo`, `redo` (an `author`'s; a push's or a
  rollback's undo is the admin's); a
  `deployer` `push` (V2-7: a deploy pushes as the pipeline key) and
  `promote` (UI-15: a release to prod); `rollback`,
  `preview`, `slots set`, `slots clear` are the admin's. Every write
  (`push`, `promote`, `author`, `rollback`, `preview`, `slots set|clear`, `routes
  set|clear`, `show`, `tag`, `undo`, `redo`) and `sessions` want their grant held on `*`: the UI
  has no scope, and a grant held on a scope alone is refused `forbidden`
  naming the scope (FRAME-43, FRAME-48).
- **An agent drives the studio** (FRAME-48): a studio page opens its
  `watch` with `--session` (its own id) and `--follows` (agents by name or
  key); **`show --to ROUTE [--say NOTE] [--point NAME] [--only WHO]`** moves
  every session that follows the caller there — a studio route
  (`#/c/ticket-chip?id=KILN-4`, `#/page/…`, `#/gallery?q=chip`, `#/pages`)
  or an app route (`/app/#/ticket/KILN-4`) — with a note bar and the
  pointed thing haloed; `shown { sessions, seq }`, or `nobody_follows` (and
  a person who does not follow is knocked once — who, by name and key,
  nothing of what — and the show waits an hour for them to follow). An
  admin's show reaches every session. A name proves nothing (any key may
  sign in as `builder`): the studio shows the key behind it and follows a
  knock by its key. **`here`** is a session's report of its route;
  **`follow --session ID [--follows …]`** sets whom it follows at once;
  **`sessions`** lists who follows the caller and where each is (`../plexus-ui/CONTRACT.md`, "An agent drives
  the studio").
- **Signing and pushing** a bundle: `plexus-ui sign KEYFILE TAR` prints the
  signature; `plexus-ui push --url ws://… --key NAME TAR` signs in as the key
  and pushes it (FRAME-8: `../plexus-ui`'s tool, `scripts/plexus-ui` here;
  the pipeline pushes with the release's own `bin/plexus-ui`). The service's
  own `ui-sign <seed-file> <bundle>` (a binary of `plexus-ui-service`,
  FRAME-5) prints the same signature.

### Sections (UI-9)

A layout names **slots**; a slot draws as a `div` carrying `data-layout` and
`data-slot`, filled by its assignment — `ui slots set --layout L --slot S
--component C [--version H]` (a component of a stored version, or the live
one) — else by the default the live manifest's `slots` names. `slots clear`
returns it to the default; `slots list` shows each assignment beside the
default it overrides. Every change says `slot_changed` on every `ui.watch`,
and the engine remounts that section alone.

### The component contract (UI-4)

The contract is `../plexus-ui/CONTRACT.md` (FRAME-8: kept with the code; it
was ticket **UI-4** on the tracker, which points at it) — read it there;
this is its layout. A bundle is a tar:

```
index.html                 the entrypoint (the shell stamps it)
engine.js  engine.wasm     the engine, abi engine/1
manifest.json              abi, contract, commit, routes, slots, components (profile svelte/1, hashes, uses)
components/svelte-runtime.js  FRAME-26: the bundle's one svelte/1 runtime
components/<name>.js       each component's module (real Svelte, compiled at build)
components/<name>.css      FRAME-30: its sheet, its <style> scoped to its element
theme.css                  FRAME-30: the tokens, ui/theme.toml compiled
src/<name>.svelte          the sources (real Svelte, svelte/1), kept
```

UI-4 pins the source dialects — rhx (a `<script>` of Rhai and a template;
SV-4: retired from menger's UI, `ui/retired-rhx/`) and real Svelte
(`svelte/1`, FRAME-13), menger's — the tree's schema, the `engine/1` host
interface, sections, and the sanitizer's allow-list.

### The compiler (UI-5)

The compiler is plexus-ui's `compiler/` (`plexus-ui-compiler`, FRAME-3:
the framework's own repository, by published version), a pure function: a component's
source → its view tree (`components/<name>.json`, keys sorted) and its
handlers (`<name>.rhai`), folds `(state, event) → state`; the same source
gives the same bytes natively and as a wasm module that imports nothing
(`"$(scripts/plexus-ui-src)"/scripts/ui-compiler-wasm.sh dist/ui-compiler.wasm`, the
framework's script, run from here with the module written into this
checkout's `dist/` and cargo building into its target; the build fails if
it imports anything). A refusal names the line. The compiler's golden pairs are
`../plexus-ui`'s (`compiler/tests/golden/`); a bundle's build compiles every
component in-process with it (`plexus-ui build`, FRAME-8) and has no
fallback: a source it refuses fails the build.

### The engine (UI-6, UI-10)

The engine is `../plexus-ui`'s `engine/` (`plexus-ui-engine`), Rust built to
wasm by the framework's tool (`plexus-ui build --prebuild --engine DIR`),
and implements `engine/1`:
it owns the socket, the cache, the signals and the registry; a component
owns only its tree and its folds.

- **Signals** — bindings re-run only when a signal they read changes.
- **`live()`** — a collection `{ rows, status }` folded from a snapshot and
  a changes stream, its params reactive, `debounce_ms`, folds `upsert:<field>`
  / `remove:<field>`; shared by method and params, kept 5 s past its last
  reader.
- **Folds** — a component's handlers in Rhai; only `call` and `navigate`
  registered, `call` held to the component's `uses`.
- **Hot-swap** on `ui.watch`: an instance whose tree or handlers changed is
  remounted in place, its state kept when its shape is unchanged.
- **Sections** — slots and their assignments, and a section menu on right
  click.
- **`{@html markdown()}`** (UI-10) — through an allow-list sanitizer into a
  tree the renderer draws.
- **The router** — `manifest.routes`: a hash route to a component, its
  `:name` segments the props.

The bundle's `index.html` imports `/app/<version>/engine.js` and boots it
with the stamped version.

### A ticket's link (TICKETS-36; on v2, V2-42)

Every link the UI makes to a ticket is by its uid, from one helper,
`ui/src/ticket-links.svelte` (`ticketHref`, `editHref`; a svelte/1
component imports what it needs): `#/t/<uid>/<ID>-<slug>` — the id as a row
shows it (`PERF.115-19` is `PERF-19`), then the title's words in lower-case
ASCII joined by `-`, cut at a word to 48 characters — and the editor's
`#/t/<uid>/<ID>-<slug>/edit`. Only the uid is read: `page-t` opens the
ticket by it whatever its id is now (a moved ticket's old link lands; a uid
naming nothing says so), and `page-editor` resolves it the same way. A row
the UI holds no uid for is linked by its name, `#/ticket/<ID>`
(`#/edit/<ID>`), which still resolves, as do the old links in bodies and
bookmarks; once the page has the ticket, the address bar is put in the uid
form in place (`history.replaceState`: no history entry, no route change),
and a stale tail is put right the same way. `tests/links_check.mjs` refuses
a ticket's route spelled anywhere else in `ui/src` (SV-4: every component
there is svelte/1, so every link comes from the helper).
`tests/pages/links.mjs` is the page group.

**V2-50 (the rest of main's TICKETS-36):**

| form | what it is |
| -- | -- |
| `#/ticket/<name>` | any name `get` takes. One ticket: its page, the address put in the uid form. Several: the **collision page** (`ticket-collision`, in page-ticket's place). None: "No ticket". A deleted one: its tombstone, as before. A name of two to four segments (`#/ticket/CLA/CLB/CLX-2`, a full address) is `page-named` (a route's `:name` is one segment), which resolves it the same way and replaces the address with the uid route in place. |
| `#/t/<uid>/…` | a segment that is no uid (8 of Crockford's base32) is not found, never read as a name. The original of two namesakes is keyed by a name that fits both (`CLX-2`), so `page-t-key` draws its page by its full address. |
| `<origin>/t/<uid>/<ID>-<slug>` | the link to share: **Copy link**, in the page's head and in its actions (`permalink` in the helper). The shell answers it `302` to `/app/#/t/<uid>/…`. |
| `<origin>/<name>` | a ticket's name at the root (`/PERF-19`, `/PLEXUS/PERF-19`, `/hypermemetic:PLEXUS/PERF-19`): `302` to `/app/#/ticket/<name>`. Both redirects are behind the sign-in as `/app/` is (signed out: `/login?next=…`); GET and HEAD only; every other path is `404` as before (`src/shell.rs` `ticket_redirect`; `tests/ui_serve.rs`). |

**The collision page**: the name, *names two tickets*, why (the shortcodes
that share its letters), then one row per `choices` entry in the store's
order — its address (the hosts muted, its id in the accent), kind, status,
title, its parent (id and title, a link by its uid), where it lives (its
epic; its organization too when the choices span several), its link by uid
and *copy link*. A row opens its ticket at its own `#/t/…`; one out of the
reader's reach is its address alone and no link. ↓ ↑ (j k) choose, ⏎ opens,
`c` copies the chosen link, Esc is the page's (the board).
`tests/pages/collision.mjs` holds main's `scripts/collision-check.sh` checks
on two shortcodes named CLX.

### The bundle in the repository

`ui/` is the UI's source: `index.html` (FRAME-30: the page's frame alone,
`theme.css` loaded first), `theme.toml` (the tokens, light and dark —
`[colour.paper]` `"--bg" = [light, dark, "what it paints"]`, …; every
component's `<style>` paints with them, never a literal), `routes.json` (`#/board` and `#/f/:filter`
→ `board-frame`, `#/` and `#/edit/:id` → `page-editor`, `#/ticket/:id` → `page-ticket`; V2-42: `#/t/:uid` and
`#/t/:uid/:tail` → `page-t`, `#/t/:uid/:tail/edit` → `page-editor` — "A ticket's link" below), `slots.json` (the
layouts' default sections), `src/*.svelte` (every component, real Svelte,
`svelte/1`). **SV-4 (owner, 2026-11-09: "don't remove, just port and stop
using completely"): rhx is retired.** menger's 34 rhx components moved to
`ui/retired-rhx/` — kept in the repository, never built — each ported:
the thin pages (`page-*`), `board-frame`, `page-shell` and `hello-list` to
`src/<name>.svelte` (real Svelte, svelte/1) under the same name (so routes, sections and the
studio's rows keep naming them), and the board's rhx half (`board-layout`,
`board-rows`, `filter-line`, `jump-palette`, `nav-tree`, `nav-flat`,
`nav-children`, `ticket-chip`) to the `-sv` twins already there, the rhx
ticket page (`ticket-page`) to `page-ticket`. `routes.json` and
`slots.json` name only svelte/1 components.

SURFACE-10: the tokens are whole — APP-114's sheet (the designer's), every
builder's addition folded in, each token once: the paper (`--bg`, `--panel`,
`--card`, `--hover`, `--sel`, `--row-sel`, `--skeleton`, `--shadow`), the ink
(`--ink`, `--muted`, `--disabled`), the lines, `--accent` and the `--focus`
ring, the signals (`--warn`, `--live`), the chip, a `--st-<status>-bg|fg`
pair for every status (Draft, Pending, Ready, Coding, In Review, Complete,
Blocked, Superseded, Canceled, Epic, Doc), a `--kind-<kind>` mark for every
kind and a `--sev-<severity>` for every severity, the diff pair
(`--diff-add-*`, `--diff-del-*`), the type, the space, the shapes and the
motion. A colour is `#rrggbb` or `#rrggbbaa`. `scripts/plexus-ui build`
passes `--strict-colours`: a literal colour in a component's `<style>` (a
`#hex`, `rgb(…)`, `hsl(…)`, a named colour) refuses the build, naming its
file and line. **The sheet** at `#/sheet` (`page-sheet` placing `sheet-page`, `#/sheet/:id` for a
given ticket, else the first `tickets.list --limit 1` answers) places every
component of `ui/src` by its bound mark, in every state it draws, under
`data-plexus-theme="light"` and `"dark"` side by side; a state is held
without the pointer by its alias (APP-114) — a component's `:hover`,
`:focus-visible`, `:disabled`, selected and checked rules each also match
`[data-sheet~=hover|focus|disabled|selected|checked]`, and a component with a
skeleton draws it on its `sheet="loading"` prop. `tests/apart.rs` holds every
component to the sheet, every `var(--…)` to a token, every state to its
alias; `tests/pages/sheet.mjs` draws it and writes `sheet-light.png` and
`sheet-dark.png`.

FRAME-14: the board was in two profiles; SV-4 keeps the real Svelte one.
Each board component is `src/<name>-sv.svelte` (`board-layout-sv`,
`board-rows-sv`, `nav-tree-sv`, `jump-palette-sv`; `svelte/1`: compiled at
build in Node, `--node-modules ../plexus-ui/node_modules`). SURFACE-2 (owner, 2026-10-30: "I want
svelte components"): the real Svelte board is the default, at `#/board`
(EDITOR-4, EDITOR-8: `#/` and `#/edit/:id` are the editor, `page-editor` placing
`editor-screen`: one surface — `editor-body`, the ticket drawn by
`ticket-view` and edited where it is drawn, a block away from the caret drawn
as `#/ticket/:id` draws it, the caret's block its markdown — and the documents
beside it, with `+ new`),
`#/f/:filter` and `#/ticket/:id` (`#/sv/…` its alias, and SV-4: `#/rhx/`,
`#/rhx/f/:filter` and `#/rhx/ticket/:id` too, where the rhx board stood).
The board is drawn in `board-frame`, a svelte/1 layout that only places its
sections — DOM marks, `<div data-slot="nav" data-filter={filter}>`, which
the host fills (`../plexus-ui/CONTRACT.md` "A layout in any profile") — whose
defaults are — `top` the app's top (`app-top`: the live
count, search, the links, the palette's button), `bar`
`board-layout-sv` (the top bar and its filter line), `nav`, `keys` the
board's keymap (`keys-help`), `main`, `bulk` the bulk bar (`bulk-bar`,
hearing the rows' `board.selected`) and `palette`. The board's keys are `board-rows-sv`'s (FRAME-36's
`bus.keys`: `j`/`k`, `enter`, `x`, `shift+x`, `c`, `/` the filter line,
`g b`, `g r`, `g w`, `escape`; `ctrl+k` the palette's own; `?` the host's
overlay), its selection
`[data-selected]` and `[data-checked]`, said on the bus as `board.selected
{ ids }` and `board.focus { id }`. A right-click on a section puts another
component there (`ui slots set`), and `ui slots clear` takes it back.
FRAME-25: the board rides FRAME-19's bus — the
palette's `live` held by `when` and its typing coalesced by `debounce_ms: 80`,
the window's by `debounce_ms: 20`, in the host's registry (no timer of
menger's own); the body through `markdown` from `plexus-ui/host`, the engine's
own sanitized render (no allow-list of menger's); and the nav's Flat, an open
shortcode's children and a page's chips as svelte/1 siblings
(`nav-flat-sv`, `nav-children-sv`, `ticket-chip-sv`), each imported by its
parent and bundled into that module, and each a component in its own right
with no route or slot of its own; `tests/apart.rs` holds it to that.

STUDIO-12: **a page is a component of components.** Every route but
the board's draws a page, `src/page-<name>.svelte` (SV-4: svelte/1, the
rhx pages' ports) — a `<section class="page page-<name>">` that places the
feature's components by their bound marks (`import SearchPage from
"plexus-ui:search-page"`, then `<SearchPage {q} {filter}/>`: a mark the
host fills, its props from the route) and carries the app's top as its
`top` section (`<div data-slot="top" data-here="search">`, `app-top` by
`slots.json`) and the palette as its `palette` section (`jump-palette-sv`:
the top's jump button and `ctrl+k` open it), so the studio's "edit this page"
reaches every page while the components stay the repository's: `page-ticket` (`#/ticket/:id`; the head, body, edit,
relations, actions, commits, history and revisions — the ticket page's
skeleton, "no such ticket", the deleted notice and its keys `e`, `escape`,
`g b` are the page's, `escape` and `g b` to `#/` under either prefix, and
its key strip), `page-new`, `page-search`, `page-ready` and `page-waves`
(full-height columns: the list scrolls in its own box under its bar), `page-filters`,
`page-orgs`, `page-releases`, `page-sheet` (V2-25: `page-epics` and `page-me` went); `page-bulk`, `page-blockers`,
`page-edit`, `page-actions`, `page-facets` and `page-counts` draw one
component alone for its browser cases; `page-thread` (COMPS-15, `#/thread/:id`) draws
`tweet-thread` alone — a ticket's body as a thread laid out like a post
composer, MENGER-52's form (the text before the first `## Post N` kept as
it is, each `## Post N` a post, a `[name](media:<hash>)` line alone its
video, any other heading and what follows kept as it is), each post's count
against 280, saved to the body by `tickets.amend` when asked; as a tag,
`<tweet-thread id="X"></tweet-thread>`, it draws natively in any body
(`tests/pages/thread.mjs`). A placed component keeps its
behaviour and its look and loses the page's chrome to the page — the board
link and the jump button (app-top's), the page's title, the palette's mark;
`tests/apart.rs` holds it to that.
plexus-ui's `plexus-ui` tool (FRAME-8; `scripts/plexus-ui` builds it
from the checkout `scripts/plexus-ui-src` names into this checkout's target and runs it) builds it into
`dist/ui.tar`: `build --prebuild` the engine (cargo: `wasm32-unknown-unknown`
and wasm-bindgen), then `build` the components compiled in-process, the
manifest and the tar — the same inputs, the same bytes, no cargo needed.
The bundle is `../plexus-ui/CONTRACT.md`'s. A deploy pushes it
(`deploy/README.md`, "v2: a release with no browser toolchain").

```sh
"$(scripts/plexus-ui-src)"/scripts/ui-compiler-wasm.sh dist/ui-compiler.wasm  # dist/ui-compiler.wasm
scripts/plexus-ui build --prebuild --engine dist/engine          # the engine, once
scripts/plexus-ui build ui --engine dist/engine -o dist/ui.tar   # dist/ui.tar
scripts/plexus-ui sign operator.seed dist/ui.tar                 # the signature, base64
scripts/plexus-ui push --url ws://127.0.0.1:4460 --key operator dist/ui.tar
```

`tests/wire_ui.rs`, `tests/wire_ui_compile.rs`, `tests/wire_ui_slots.rs`,
`tests/ui_serve.rs` and `tests/seam.rs` hold the service over the socket;
`tests/pages_browser.mjs` the pages in Chromium. The engine's own browser
test is the framework's (`../plexus-ui`, `npm test`).

## As a library (DIST-3)

`menger::local` starts the server from code — the store, every service,
the WebSocket and the shell on loopback, no registry announcement — behind
the gate with one owner (`LocalConfig::owner`), whose sessions
`owner_token(ttl_secs)` mints — kept, as the binary keeps them, beside the
database (`<database>.sessions.jsonl`, FRAME-21), so a token outlives a
restart on the same database:

```rust
use menger::local::{LocalConfig, LocalServer, TAURI_ORIGINS};

let mut cfg = LocalConfig::new(data_dir.join("tickets.db")); // ports 0: free ones
cfg.extra_origins = TAURI_ORIGINS.iter().map(|o| o.to_string()).collect();
let server = LocalServer::start(cfg).await?;
let token = server.owner_token(12 * 3600)?;
server.shutdown().await;
```

`tests/local.rs` is the executable reference.

**plexus-webapp** (SCAFFOLD-5; a repository of its own since FRAME-4,
published on crates.io) is the serving half: `AppSource`, the
hardened router, the page policy and its stamping, and `LocalServer` for
any Plexus service. Its `toy/` is its worked example.

## Layout

```
src/                 the binary: main.rs (flags), builder.rs (the services registered),
                     shell.rs, bind.rs, calls.rs, local.rs, first_run.rs, login_access.rs
tickets-api/         menger-tickets-api: the tickets service — vocabulary, store, filter
                     language, activations (activation/), access (access/)
media/               menger-media: the media service — images by sha256 in media.db, the
                     ticket's permit asked of tickets through the seam (V2-24)
quota/               menger-quota: the quota service — allowances per principal and source
                     in quota.db, every storing write reserved through the seam (V2-31)
ui/                  the UI's source bundle (compiled and drawn by ../plexus-ui's
                     plexus-ui-compiler and plexus-ui-engine, FRAME-3; stored and
                     served by its plexus-ui-service, FRAME-5)
tether/              menger's local side, its own workspace (TETHER-7)
tests/               wire tests over the real socket, browser tests
deploy/              the pipeline, the image, the runner (deploy/README.md)
perf/                the probes (perf/README.md)
scripts/             builds, cleanup-gate, disk-check, sweep, throwaways
```

## History

`menger-hs` (0.04, Haskell) is the ancestor. v1 (0.05–0.6, tag `v0.0.1`
above) moved menger onto Plexus RPC and Rust with a browser app of its own;
v2 keeps its store and replaces the app with the ui service. V2-24
(2026-11-02) brought media back as a service of its own — v1's `media.put`
and `media.get`, by sha256 in `media.db`, the ticket's permit asked of the
tickets service through the seam (`tickets.may`), `/media/<hash>` on the
shell — the first service beside `tickets` to write something of a
ticket's. V2-29 (2026-11-02) gave documents a service of their own —
`docs`, in `docs.db`, the same editor surface writing to it (`#/doc/:id`;
`+ new` makes a doc, not a ticket) — and, on the owner's word, made a
document a Yjs CRDT written live by many hands, who is on it shown, every
change kept to replay and squash. V2-30 (2026-11-02) let a selection in a
doc be asked about: a thread anchored in the CRDT, in the margin, answered
live by a person or an agent. V2-25
(2026-11-01): epics leave v2's surface — the `epics` activation, the header's
switcher and `#/epics`, and the `me` page — and the board is the
organization's whole set; `session` stays as the agents' door. V2-31
(2026-11-02): the quota service — per-user allowances on every data input,
usage counted per source in `quota.db`, tabulated for each person
(`#/usage`) and for admins (`#/quota`), and every storing write reserved
against the cap through the seam and refused over it; the owner's free
tier (`free-1k`) as the built-in plan, the box guard, 20 writes a second and
4 connections per principal.

## License

MIT
