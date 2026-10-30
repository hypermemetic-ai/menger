# menger v2

A plan is a program whose leaves are prose.

menger is a ticketing service on Plexus RPC: a store of tickets that a
person reads in the browser and an agent reads with `synapse`, both over one
signed-in WebSocket, every change pushed to every open page as it happens.
v2 keeps v1's core — the tickets store, its filter language, its access
rules — and replaces v1's browser app with a **ui service**: pages are
components written in rhx (Rhai in markup, `ui/src/<name>.rhx`, profile
`rhx/1`; `../plexus-ui/CONTRACT.md`), pushed as signed bundles,
versioned by content hash and drawn by a small engine.

v1 is tag `v0.0.1`; what v2 cut — the chats, comments, deployments, themes, media, notes, assignees, the owner's queue, inputs and components, the Leptos app (trunk) with the frames' SDK (sdk-codegen) and mockups, the diagrams service — lives there.

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
the placeholder too (or the entrypoint of an `--app-dist` build). The
login routes (`/login`, `/login/start`, `/login/callback`, `/logout`,
`/auth/cli`, …) are plexus-login's. Every page goes out under a
Content-Security-Policy (ACCESS-28, `PAGE_CSP`): no inline script but the
page's own blocks, hashed; no script or image from another origin;
connections only to itself, its socket and loopback.

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
| run | `--open`, `-p/--port` (4455), `--shell-port` (4456), `--data-dir`, `--db-path`, `--ui-db-path`, `--ui-signer`, `--ui-compiler`, `--bind`, `--app-dist` (other files under `/app/`, from a directory), `--env` (`test` titles the tab `menger.test` and allows the dev sign-in), `--no-shell` |
| sign-in | `--auth-provider`, `--admin`, `--public-url`, `--login-button`, `--login-email`, `--login-password`, `--login-forgot`, `--identity`, `--workos-api-key-file`, `--allowed-origin`, `--frame-ancestor` (an origin that may frame the shell's pages; none by default), `--live-origin` (an origin the shell's pages may frame), `--allowed-host`, `--trusted-proxy`, `--return-origin`, `--session-hours` (12), `--session-idle-minutes` (120) |
| maintenance and testing | `--dev-login`, `--dev-password`, `--dev-idp-ws`, `--dev-user` (all `--env test` only), `--verify-derived`, `--seed-synthetic`, `--seed-value`, `--seed-shape` (`flat`, `nested`), `--seed-into-existing` |
| limits | `--max-body-bytes` (8 MiB), `--max-calls-per-connection` (256), `--idle-timeout-secs` (900), `--max-connections` (1,024), `--max-connections-per-ip` (256), `--max-connections-per-principal` (64), `--max-anonymous-connections` (128), `--max-anonymous-connections-per-ip` (32) |

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
form is **16** (V2-4: the cells no longer carry what the cut services kept);
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

menger is a **tenant** of the stored-UI framework `../plexus-ui` (FRAME-1,
FRAME-7): its own code is the tickets service (`tickets-api/`), the server
that registers it (`src/`) and its UI's components (`ui/`); the ui service,
the compiler, the engine, the routes that serve a bundle and the tool that
builds, signs and pushes one are the framework's, taken from the sibling
checkout as `../plexus-shell` and `../plexus-login` are. Any other Plexus
service ships with the framework the same way.

A service that would rather not link it runs beside the framework's own
binary instead (FRAME-6): `plexus-ui serve --child NAME=ws://…` keeps its
`ui.db`, embeds the compiler, signs people in and mounts the service as a
child behind its gate, so a page's one socket reaches it
(`../plexus-ui/README.md` "A tenant in ten lines"; its example tenant is
`examples/shop`). menger links the library and builds no binary of that
name (`tests/apart.rs`). The tool assumes no service: menger's calls to it
name `--service menger` (`deploy/pipeline`'s push), and a contract asked of
a service is its `ui.contract` — menger's, one service named, is
`tickets.schema`'s hash, as the pipeline stamps it.

One process registers these on the `menger` service (`src/builder.rs`), all
behind plexus-core's gate; menger's own are declared on the frame
(ACCESS-13), `auth` is plexus-login's:

| service | what it is | its code |
|---|---|---|
| `tickets` | the store, its reads, writes and watch; children `commits`, `filters`, `nav`, `revisions`, `subtasks`, `work` | `tickets-api/src/activation/` |
| `members` | grants and teams (child `teams`) | `tickets-api/src/members_activation.rs` |
| `orgs` | organizations | `tickets-api/src/orgs_activation.rs` |
| `epics` | an organization's levels of shortcodes | `tickets-api/src/epics_activation.rs` |
| `session` | who a connection is, agents' sessions | `tickets-api/src/session_activation.rs` |
| `auth` | the AUTH protocol's door and roles | plexus-login, over `src/login_access.rs` |
| `ui` | the UI's versions, live pointer, previews; child `slots` | the framework's: `../plexus-ui/service` (`plexus-ui-service`, FRAME-5) |

**Service to service.** The ui service reaches the tickets service as a
client, never through its store: its seam (`../plexus-ui/service/src/seam.rs`)
holds the running process's service and calls each service a component's
`uses` names (`tickets.list` names `tickets`) through plexus-core's
`HubTransport::as_(service, caller)` as the server's own caller — the same
route and gate a socket reaches. A push asks it for those methods and for
the services' schema hashes. The ui service never opens the tickets file.

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

### `tickets`

| method | flags | what it does |
|---|---|---|
| `health` | | version, unix time, ticket count |
| `get` | `--id` | one ticket by id or path address |
| `list` | `[--epic] [--status] [--parent] [--filter] [--keep] [--after] [--at] [--limit] [--summary] [--tz]` | tickets in store order, paged; a `page` event last |
| `find` | `--query [--filter] [--after] [--limit] [--tz]` | id or title contains the query (the jump) |
| `search` | `--query [--filter] [--offset] [--limit] [--tz]` | inside titles and bodies, best first, each hit with its passage |
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
| `history` | `--id` | each status and kind transition, then `history_end` |
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
| `work ready` | `[--epic] [--filter] [--after] [--limit] [--summary] [--tz]` | the ready set |
| `work waves` | `--epics [--kinds]` | the order of work, in waves |
| `work waves_under` · `waves_to` | `--id [--kinds]` | under one ticket · up to one ticket |
| `work blockers` | `--id` | a ticket's blockers at their statuses |

### `members`, `orgs`, `epics`, `session`, `auth`

| method | flags | what it does |
|---|---|---|
| `members mine` | | who you are, your teams, your grants |
| `members list` | `[--scope]` | the grants on the scopes you administer |
| `members grant` | `--scope --role [--principal] [--team] [--everyone]` | a role on a scope |
| `members revoke` | `--scope [--principal] [--team] [--everyone]` | take it away |
| `members requests` | | who signed in holding nothing (admin of `*`) |
| `members pairing` | `--code` | the machines waiting on a pairing code (TETHER-12) |
| `members teams list` · `show` · `create` · `rename` · `delete` · `add` · `remove` | `[--org]` · `--team` · `--name [--org]` · `--team --to` · `--team` · `--team --principal` · `--team --principal` | teams (ACCESS-22) |
| `orgs list` · `create` · `rename` · `delete` | — · `--name [--admin]` · `--name --to` · `--name` | organizations (ORG-4) |
| `epics list` · `get` · `current` · `switch` · `dismiss` · `watch` | `[--archived] [--org]` · `--name [--org]` · `[--org]` · `--name [--org]` · `[--org]` · — | an organization's epics, and yours |
| `epics create` · `rename` · `delete` · `set_default` · `archive` · `restore` | `--name [--org]` · `--name --to [--org]` · `--name [--org]` · `--name [--org]` · `--name [--dry-run] [--org]` · `--name [--org]` | an admin's |
| `session whoami` · `providers` · `login` · `logout` | — · — · `[--ttl-secs] [--scope]` · — | the connection's identity; an agent's session |
| `auth providers` · `login` · `signup` · `whoami` · `logout` · `requires` | — · `--flow` · `[--via]` · — · — · `--method` | the door (AUTH-7) |
| `auth roles` · `role_set` · `grant` · `revoke` · `waiting` | — · `--role --grants` · `--principal --role --scope` · `--principal --scope [--role]` · — | roles and grants (admin) |

### `ui`

| method | flags | what it does |
|---|---|---|
| `push` | `--bundle --manifest --signature [--commit]` | store a bundle by hash, compiled and checked, and make it live, in one transaction |
| `rollback` | `--to` | move the live pointer to a kept version |
| `versions` | | every kept version, newest first, one `live: true` |
| `preview` | `[--version]` | serve a kept version to the caller's sessions alone; none clears it |
| `watch` | | `live` on every pointer move, `slot_changed` on every section change |
| `slots list` | `[--layout]` | every section's assignment and the default it overrides |
| `slots set` | `--layout --slot --component [--version]` | fill a section |
| `slots clear` | `--layout --slot` | back to the default |

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
revision (`tickets-api/src/store/revisions.rs`).

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
`assigned`, `unassigned`, `asked`, `asking` and the rest of `filter.rs`'s
`GONE` — is refused by name, never read as another cell.

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
unknown field `sumary`; tickets.list takes: after, at, epic, filter, keep,
limit, parent, status, summary, tz`` — every unknown key named, the
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

### Organizations (ORG-4) and epics (ORG-13, ORG-23)

Every ticket and shortcode is in one organization (`hypermemetic`).
`orgs create --name N --admin P` (admin on `*`), `rename` (the old name an
alias), `delete` (only empty). A grant's scope is `*`, `org:`, a shortcode
(`org:SHORTCODE`, with those hosted under it), a ticket alone (`MNG-40`) or
a subtree (`under MNG-40`, ACCESS-21). A reader sees only the organizations
it holds a role in.

An **epic** is a level of an organization that holds shortcodes:
organization → epic → shortcode → ticket. Each organization starts with
`genesis`; a principal reads in its own epic (its `switch`, else the
organization's default). `epics archive` moves everyone on it to the next
newer live epic in one write (`--dry-run true` first); an archived epic's
tickets are in no view unless a filter names it or `archived`. `tickets
reshortcode --to org:epic:CODE` moves tickets between epics.

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
| `viewer` | read under the scope: `tickets`' reads and its children's, `orgs.list`, `epics`' own settings, `ui.versions`, `ui.watch`, `ui.slots.list` |
| `editor` | also create and change tickets: `tickets`' writes, `commits`, saved filters, `revisions restore`, `subtasks create` |
| `admin` | every grant: `tickets.delete`, `undelete`, `freeze`, `unfreeze`, `members.*`, `orgs.create`, `auth.role_set`, `auth.grant`, `ui.rollback`, `ui.preview`, `ui.slots.set`, `ui.slots.clear`, … |
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
| WebSocket connections: in all / per IP / per principal / anonymous / anonymous per IP | the `--max-connections*` flags (1,024 / 256 / 64 / 128 / 32) | the upgrade refused 429 |
| calls open on one connection (a `watch` for its life) | `--max-calls-per-connection` (256) | `error_kind: too_many_calls` |
| a connection with no call open | `--idle-timeout-secs` (900) | closed |
| a browser's sign-in unused | `--session-idle-minutes` (120) | the session ends |
| sign-in flows and password guesses per client and account | AUTH-22's throttles | 429 |
| the waiting list | `WAITING_MAX` 1,000 (`tickets-api/src/access/mod.rs`) | the oldest forgotten |
| a title / a filter | `TITLE_MAX` 1 KiB / `FILTER_MAX` 4 KiB (`tickets-api/src/limits.rs`) | `invalid_params` |
| a body | `--max-body-bytes` (8 MiB) | `body_too_large` |
| `tickets.list`'s page | 1,000 unless `--limit`, at most 10,000 | page with `page.next` |
| a ui bundle | `BUNDLE_MAX` 16 MiB (`../plexus-ui/service/src/vocab.rs`) | `invalid_params` |

`perf/flood.mjs` floods a throwaway with upgrades and sign-ins and checks
its memory settles.

## The ui service (UI-3, UI-7, UI-9)

A pushed UI is a row by content hash in the ui service's own file, and
`/app/` serves whichever row the live pointer names: **the pointer is the
deploy**.

**The service is the framework's** (FRAME-5): `plexus-ui-service` in
`../plexus-ui/service`, by sibling path, with no menger type in it. menger
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
  `ui_pointer`, `ui_preview`, `ui_slots`.
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
  services named — one service, its schema's own hash (menger's bundles name
  `tickets` alone: `tickets.schema`'s, which `plexus-ui build
  --contract-from URL` stamps, or `--contract HASH` as given — UI-12: the
  pipeline's build reads it from the binary it built and passes it so;
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
- **`rollback --to <hash>`** moves the pointer to a kept version;
  **`preview`** serves one to the caller's own sessions; **`versions`**
  lists them; **`watch`** says `live` on each move and `slot_changed` on each
  section change, held to its caller by the gate: `watch_closed { reason }`
  (`signed_out`, `expired`, `revoked`, `refused`) when it no longer stands.
- **Who may**: viewers and editors `versions`, `watch`, `slots list`; a
  `deployer` `push` (V2-7: a deploy pushes as the pipeline key); `rollback`,
  `preview`, `slots set`, `slots clear` are the admin's.
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
manifest.json              abi, contract, commit, routes, slots, components (profile rhx/1, hashes, uses)
components/<name>.json     each component's view tree
components/<name>.rhai     its handlers
src/<name>.rhx             the sources (rhx), kept
```

UI-4 pins the source dialect, rhx (a `<script>` of Rhai and a template;
a `.svelte` is real Svelte, `svelte/1`, FRAME-13), the tree's schema, the
`engine/1` host interface, sections, and the sanitizer's allow-list.

### The compiler (UI-5)

The compiler is `../plexus-ui`'s `compiler/` (`plexus-ui-compiler`, FRAME-3:
the framework's own repository, a sibling checkout), a pure function: a component's
source → its view tree (`components/<name>.json`, keys sorted) and its
handlers (`<name>.rhai`), folds `(state, event) → state`; the same source
gives the same bytes natively and as a wasm module that imports nothing
(`../plexus-ui/scripts/ui-compiler-wasm.sh dist/ui-compiler.wasm`, the
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

### The bundle in the repository

`ui/` is the UI's source: `index.html`, `routes.json` (`#/` and `#/f/:filter`
→ `board-layout`, `#/ticket/:id` → `ticket-page`), `slots.json` (the
layouts' default sections), `src/*.rhx` (`board-layout`, `board-rows`,
`filter-line`, `jump-palette`, `nav-tree`, `nav-flat`, `nav-children`,
`ticket-page`, `ticket-chip`, `page-shell`, `hello-list`).

FRAME-14: the board is in two profiles. Beside each of the five board
components above (`board-layout`, `board-rows`, `nav-tree`, `ticket-page`,
`jump-palette`) is its real Svelte twin, `src/<name>-sv.svelte` (`svelte/1`:
compiled at build in Node, `--node-modules ../plexus-ui/node_modules`), with
the same calls and the same DOM marks. The real Svelte board is at `#/sv/`,
`#/sv/f/:filter` and `#/sv/ticket/:id`; it is drawn in `board-frame.rhx`, an
rhx layout that only places four sections (a section is the host's, and only
an rhx template marks one), whose defaults are the svelte/1 ones — `top` holds
`board-layout-sv`, the top bar and its filter line. The rhx components stay
the defaults of `board-layout`'s sections; a right-click on one puts the
real Svelte twin there (`ui slots set`), and `ui slots clear` takes it back. A
real Svelte twin carries what a module cannot ask of the host today: the
palette's 80 ms pause and its hold until a query (rhx's `debounce_ms` and
`when:`), the window's 20 ms, the nav's and the page's children inside them
(no `.svelte` imports another), and the body's markdown and the engine's
allow-list (`markdown()` is not on the bus).
`../plexus-ui`'s `plexus-ui` tool (FRAME-8; `scripts/plexus-ui` builds it
from the sibling into this checkout's target and runs it) builds it into
`dist/ui.tar`: `build --prebuild` the engine (cargo: `wasm32-unknown-unknown`
and wasm-bindgen), then `build` the components compiled in-process, the
manifest and the tar — the same inputs, the same bytes, no cargo needed.
The bundle is `../plexus-ui/CONTRACT.md`'s. A deploy pushes it
(`deploy/README.md`, "v2: a release with no browser toolchain").

```sh
../plexus-ui/scripts/ui-compiler-wasm.sh dist/ui-compiler.wasm  # dist/ui-compiler.wasm
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

**plexus-shell** (SCAFFOLD-5; `../plexus-shell`, a repository of its own
since FRAME-4, taken by sibling path) is the serving half: `AppSource`, the
hardened router, the page policy and its stamping, and `LocalServer` for
any Plexus service. Its `toy/` is its worked example.

## Layout

```
src/                 the binary: main.rs (flags), builder.rs (the services registered),
                     shell.rs, bind.rs, calls.rs, local.rs, first_run.rs, login_access.rs
tickets-api/         menger-tickets-api: the tickets service — vocabulary, store, filter
                     language, activations (activation/), access (access/)
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
v2 keeps its store and replaces the app with the ui service.

## License

MIT
