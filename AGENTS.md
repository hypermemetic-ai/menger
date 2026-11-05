# menger

A plan is a program whose leaves are prose. menger is a Plexus RPC
ticketing service and the seed of a Plexus-native reactive web framework.
This is **v2**: the tickets service, the ui service, the media service, the docs service and the quota service, nothing else.

v1 is tag `v0.0.1`; v2 cut its chats, comments, deployments, themes, notes, assignees and components, the Leptos app (trunk) and the frames' SDK (sdk-codegen), mockups and the diagrams service — read v1 there, never here. Media came back as a service of its own (V2-24, `media/`), and a body's pages as sandboxed frames (EDITOR-14, `ui/src/page-frame.svelte`, real Svelte: `svelte/1`).

README.md is the reference: the services and their methods ("Surface"),
the flags ("Run" → Every flag), the rules each ticket set down. This file is
what an agent working in the repository needs on top of it.

## Commands

```sh
cargo test -p menger-tickets-api --test cut       # a selection: a crate and a filter, never the workspace per ticket
# a development server: its own ports and its own database, never prod's —
# and a login provider: menger has no open mode (ACCESS-24)
PLEXUS_NO_REGISTRY=1 cargo run -- --port 4460 --shell-port 4461 --db-path /tmp/menger-dev.db \
  --ui-db-path /tmp/menger-dev-ui.db --auth-provider key --admin "$(synapse _key show builder)"
synapse -P 4460 _login menger --key builder --as builder   # once per server start
synapse -P 4460 menger tickets health
synapse -P 4460 menger ui versions
scripts/agents-commands.sh 4460   # every synapse command in this file, run against that server
```

**There is no open mode** (ACCESS-24). `menger` with no flags (DIST-14)
runs on loopback as its **local admin**: a key made in its data directory
(`~/.local/share/menger/local.key`; the database `local.db`, never
`tickets.db`) and a one-time sign-in link printed at start (`menger login`
prints another; `--open` opens it). A dev server runs `key` with your key
as `--admin` (above), on its own ports and database. The tests run the same
way: `tests/support` starts every server under `key` with a fixture key as
`--admin`; `tests/first_run.rs` runs the no-flags start on a temp HOME.

Bare `cargo run` takes prod's ports (4455–4456, or the next free ones) and
the default data directory, `~/.local/share/menger` — prod's directory on
this machine; it writes `local.key`, `local.json` and `once/` beside prod's
file. Never run it that way here. Give a dev server its own `--ui-db-path`
too: the default is `ui.db` beside the database, and throwaways in `/tmp`
would share one.

**`scripts/agents-commands.sh PORT`** (V2-8) runs every `synapse` command
in this file — each line of a fenced `sh` block, and each inline
`synapse … menger …` span — against a throwaway, its address rewritten, and
says `ok` or `FAIL` for each, then `N ok, M failed`. `--list` prints them.
A command written here must run as written; one whose precondition a
throwaway cannot meet is prose.

**tether** (`tether/`, TETHER-7) is menger's local side as a Plexus RPC
tool of its own, with substrate imported as a crate and called in memory
(plexus-core's `HubTransport`, LOCAL-6). It is its **own cargo workspace**
— substrate's sqlx and menger's rusqlite link two `libsqlite3-sys` — so
`cargo test` here does not cover it: `cd tether && cargo test`.

Every listener answers only its own names (ACCESS-25): `127.0.0.1:<port>`,
`localhost:<port>`, `--public-url`'s host, and each `--allowed-host`; any
other `Host` is refused 421. Beyond loopback menger is one https origin
(ACCESS-30): `--bind`, `--public-url https://…`, `--trusted-proxy` for the
edge. README "Run".

## What v2 is

**menger is a tenant of the framework** (FRAME-7): the stored-UI framework
is `../plexus-ui` — the ui service, the compiler, the engine, the routes a
tenant mounts, the `plexus-ui` tool and the contract — and menger links it
as any Plexus service may: its own code is `tickets-api/`, `src/` and the
components in `ui/`, nothing of the framework's.

**The services registered** (`src/builder.rs`, one process): `tickets`
(children `commits`, `filters`, `inputs`, `nav`, `revisions`, `subtasks`, `work`),
`members` (child `teams`), `orgs`, `session`, `auth`, `ui`
(child `slots`), `media` (V2-24: `media/`, `menger-media` — images by
sha256 in `media.db`, `--media-db-path`), `docs` (V2-29: `docs/`,
`menger-docs` — documents as Yjs CRDTs in `docs.db`, `--docs-db-path`;
who may is the gate's word on the document's organization, asked through
its seam; the editor writes it at `#/doc/:id`), and `quota` (V2-31: `quota/`, `menger-quota` —
every principal's allowance per source in `quota.db`, `--quota-db-path`;
every storing write reserved through its seam — README "`quota`"). A service reaches another as a
client, through plexus-core's `HubTransport::as_(service, caller)` — the ui
service's seam (`../plexus-ui/service/src/seam.rs`) asks each service a
component's `uses` names (`tickets`) as the server's own caller; the media
service's (`media/src/seam.rs`) asks `tickets.may { id, role }` **as the
caller** before it keeps an image for a ticket, so the tickets service
decides by the caller's reach — through the same route and gate a socket
reaches; neither opens the tickets file. A body shows an image as
`![alt](media:<hash>)`; the page draws it from the shell's `/media/<hash>`.

**The tickets service** is v1's core and nothing else: the cache
(`tickets-api/src/store/`), the cells and the lens over Roaring bitmaps, the
kept tree, the stored derived state (form 17), FTS5 search, questions
(`tickets inputs`) and the queue (V2-20; README "Questions and the queue"), the filter
language. `tickets-api/tests/cut.rs` holds the cut: no module of a service
that went, no flag for one, and README and this file naming none of them.

**The ui service** (UI-3, UI-7, UI-9) is the framework's (FRAME-5):
`plexus-ui-service` in `../plexus-ui/service`, by sibling path, with no
menger type in it — its caller the one plexus-core's gate admitted, its
`ui.watch` held by the gate's grants generation (menger hands the store's
count to the gate, `AuthSetup::with_generation` in `src/builder.rs`), its
routes (`plexus_ui_service::routes::UiRoutes`) mounted on menger's shell
(`src/shell.rs`) with menger's session, stamps, policy and placeholder. It
keeps the UI's versions by content hash in its own file and serves
whichever the live pointer names at `/app/`:

- `ui push` — a bundle (a tar, base64), its manifest and an Ed25519
  signature over the bundle's sha256; compiled in the compiler module
  (when `--ui-compiler` names one), each component's `uses` linted against the service it names (the contract: the hash of the services named), and
  stored and made live in one transaction. `ui rollback --to <hash>`,
  `ui versions`, `ui preview [--version]`, `ui watch`.
- **Sections**: `ui slots list | set | clear` — what component fills each
  slot of each layout, over the live manifest's defaults. **A page composed
  from bundles** (FRAME-15): a bundle pushed with `--no-live` (`ui.push
  { live: false }`) is kept and made live by nothing — a bundle of
  components alone, with no `index.html`, can only be kept that way; a
  section is set to one of its components by hash (`--version`) or by
  the bundle's name (`--follow NAME`, the `name` the tool's `build --name`
  writes in the manifest), and a follower moves with every push carrying
  that name, said as `slot_changed`; a set by version, or a clear, ends
  the following. `../plexus-ui/CONTRACT.md` "A page composed from bundles"
  has the rules.
- **Its flags**: `--ui-db-path` (`MENGER_UI_DB_PATH`; default `ui.db` beside
  the database), `--ui-signer` (`MENGER_UI_SIGNERS`, `;`-separated 64-hex
  public keys; none: every push refused `no_signer`), `--ui-compiler`
  (`MENGER_UI_COMPILER`, `dist/ui-compiler.wasm`; without it a push stores
  the trees as given and says `compiled: false`).
- **Who may**: `versions`, `watch` and `slots list` a viewer's; `push` the
  `deployer`'s (V2-7: the pipeline key); `rollback`, `preview`, `slots set`
  and `slots clear` the admin's (`../plexus-ui/service/src/activation.rs`).

```sh
synapse -P 4460 menger ui slots list --layout board-layout
synapse -P 4460 menger ui slots set --layout board-layout --slot nav --component nav-flat
synapse -P 4460 menger ui slots clear --layout board-layout --slot nav
synapse -P 4460 menger ui preview
synapse -P 4460 menger ui watch          # open-ended
```

**The UI's source** is `ui/` — `index.html` (the page's frame alone),
`theme.toml` (the tokens, FRAME-30: the bundle's `theme.css`), `routes.json`, `slots.json`,
`src/*.rhx` — each component carrying its look in a `<style>` on the tokens — compiled by `../plexus-ui`'s `plexus-ui-compiler`
(UI-5: rhx, profile `rhx/1` → a view tree and Rhai handlers;
native and as a wasm module with no imports) and drawn by its
`plexus-ui-engine` (UI-6, UI-10: signals,
`live()`, the DOM renderer, the Rhai folds, hot-swap on `ui.watch`,
sections, the sanitized `{@html markdown()}`). The contract between them is
`../plexus-ui/CONTRACT.md` (it was ticket **UI-4** on the tracker); README
"The component contract" has the bundle's layout. The two crates and the
tool that builds, signs and pushes a bundle — `plexus-ui` (FRAME-8) — live
in the sibling checkout `../plexus-ui` (FRAME-3); the lines below build
them from there into this checkout's target.

```sh
../plexus-ui/scripts/ui-compiler-wasm.sh dist/ui-compiler.wasm   # (rustup's wasm32-unknown-unknown)
scripts/plexus-ui build ui -o dist/ui.tar            # the bundle a deploy pushes (scripts/plexus-ui --help)
```

A change to `ui/src` is checked in Chromium by `tests/pages_browser.mjs`
(through `scripts/ui-throwaway.mjs`); a change to the engine in
`../plexus-ui`, by `npm test` there (the engine alone on a stub socket);
the compiler's golden pairs by `cargo test -p
plexus-ui-compiler --test golden` in `../plexus-ui`.

## Adding a component

From an idea to a component on app-test, top to bottom. The dialect is
rhx (Rhai in markup, `ui/src/<name>.rhx`, profile `rhx/1`), the
framework's contract's, `../plexus-ui/CONTRACT.md` (it was UI-4): a `<script>` of Rhai — `prop()`,
`state()`, `live()`, and handlers as folds `fn name(state, ev) { …; state }`
— and a template of `{expr}`, `{#if}`, `{#each … as x (key)}`,
`on:event={handler}`, another component as `<name prop={…}/>`, and in a
layout `<slot name="…"/>`. `ui/src/*.rhx` are the worked examples; a
`.svelte` is real Svelte (`svelte/1`, FRAME-13): `ui/src/*-sv.svelte` are the board's real-Svelte twins, the default board at `#/` since SURFACE-2 (the rhx one at `#/rhx/`; FRAME-14, README "The bundle in the repository").

**1. Write it** — `ui/src/ticket-kids.rhx`, a ticket's children, live (a
new child appears, a status moves, a delete goes):

```rhx
<script>
  prop("id");
  state(#{ open_only: false });
  let kids = live(#{ snapshot: "tickets.list", params: #{ parent: id, summary: true, limit: 200 },
                     changes: "tickets.watch", key: "id",
                     on: #{ created: "upsert:ticket", status_changed: "upsert:ticket", deleted: "remove:ids" } });
  fn toggle(state, ev) { state.open_only = !state.open_only; state }
</script>

<section class="ticket-kids">
  <h2>{id}</h2>
  <button on:click={toggle}>open only</button>
  <ul>
    {#each kids as c (c.id)}
      {#if !state.open_only || c.status != "Complete"}<li><a href={"#/ticket/" + c.id}>{c.id + " " + c.title}</a> {c.status}</li>{/if}
    {/each}
  </ul>
</section>
```

`live()` calls the snapshot once and folds the changes stream by `on`; an
`upsert` keeps only rows that match the snapshot's params (`parent` here),
so the list stays its own. Every method a component calls is its `uses`,
and a push checks each against the running tickets service.

**2. Put it on screen.** A route names a component — `ui/routes.json`,
`"#/kids/:id": "ticket-kids"` (a `:name` segment is a prop) — and a layout's
sections name their defaults — `ui/slots.json`, `layout → slot →
component`. A section's assignment changes on the running service without a
push: `ui slots set` (or the right-click menu on a section), which is how two
versions of one component are swapped (`--version` names the other);
`ui slots clear` puts the default back.

**3. Build.** With the framework's tool, `plexus-ui` (`../plexus-ui`'s
`cli/`, FRAME-8; `scripts/plexus-ui` builds it into this checkout's target
and runs it). Once, the compiler module and the engine — what needs cargo;
then the bundle, every time — the components compiled in-process, the
manifest, the tar:

```sh
../plexus-ui/scripts/ui-compiler-wasm.sh dist/ui-compiler.wasm   # dist/ui-compiler.wasm
scripts/plexus-ui build --prebuild --engine dist/engine           # the engine (wasm32, wasm-bindgen)
scripts/plexus-ui build ui --engine dist/engine -o dist/ui.tar    # the bundle; `--contract HASH` to stamp one
```

A source the compiler refuses names its line (`ui/src/<name>.rhx:LINE:
…`), and nothing is written. `scripts/plexus-ui --help` documents every
command.

**4. See it.** A dev server in local mode — no `--auth-provider`, so it
prints a one-time sign-in link for your browser — with your key as admin
and as the signer (the signer is the public key: your principal without
`key:`):

```sh
PLEXUS_NO_REGISTRY=1 cargo run -- --port 4460 --shell-port 4461 --data-dir /tmp/menger-dev \
  --admin "$(synapse _key show builder)" --ui-signer "$(synapse _key show builder | sed 's/^key://')" \
  --ui-compiler dist/ui-compiler.wasm
scripts/plexus-ui push --url ws://127.0.0.1:4460 --key builder dist/ui.tar
scripts/plexus-ui versions --url ws://127.0.0.1:4460 --key builder   # `rollback HASH`, `preview HASH` the same way
```

`push` signs in as the key and, with no `--signer`, signs the bundle with
it too (Ed25519 over its sha256; `plexus-ui sign KEYFILE dist/ui.tar`
prints that signature alone). It reads the seed from the key's file
(`~/.plexus/keys/<name>.key`, 0600 — a name or a path goes in argv, never
the seed; a key synapse keeps in the keychain has no file: make one with
`synapse _key new NAME --store file`). The push answers `pushed { version,
live: true, compiled: true }`. Open the
banner's link, then `http://127.0.0.1:4461/app/#/kids/KILN-1`; `cargo run --
login --data-dir /tmp/menger-dev` prints another link (a local start also
links `~/.plexus/keys/menger-local.key` to its own key when none is there).
Every open page hot-swaps on the next push. `scripts/ui-throwaway.mjs` and `tests/pages_browser.mjs`
do all of this from code.

**5. Ship.** Commit the source (`ui/src`, `routes.json`, `slots.json`; `dist/`
is never committed), merge to `v2` with `--no-ff`, and deploy to test: the
runner deploys its branch's tip (`MENGER_RUNNER_BRANCH`; while it is `main`,
run v2's by hand — `deploy/pipeline build --branch v2 --no-gate`, then
`image`, then `remote test --image …`). The deploy builds the bundle and
pushes it as the `pipeline` key, the only signer on app-test; every open page
there hot-swaps to it.

## Deploying

menger runs in two environments, both on the Linode as one image through
hypermemetic-infra: **test** is https://app-test.menger.sh and **prod**
https://app.menger.sh (DEPLOY-52). This machine builds the releases and
images and runs the suite; it runs no environment. `deploy/README.md`
defines the pipeline (DEPLOY-1); its "v2: a release with no browser
toolchain" (V2-7) is v2's path:

```sh
deploy/pipeline build --branch v2 --no-gate   # v2's tip → a release (RELEASE says `branch: v2`); no frontend/,
                                              # so the workspace, the plexus-ui tool, the engine, the bundle
                                              # and the compiler module, all on this machine
deploy/pipeline image R                       # RUN-10: the release as an image (IMAGE_STORE=ghcr pushes it)
deploy/pipeline remote test --image ghcr.io/hypermemetic-ai/menger:R
                                              # app-test.menger.sh through infra, the bundle pushed through
                                              # ui.push as the pipeline key, then the smoke
deploy/pipeline suite R                       # the full suite at R's commit, on this machine, then plexus-bench's
                                              # counts and bench (PERF-6, PERF.115-29: ../plexus-bench, every
                                              # scenario against its golden); marks R passed / failed
deploy/pipeline remote prod                   # what test runs → app.menger.sh, only once its suite passed; the owner's
deploy/pipeline local --image menger:R        # RUN-25: the image on 127.0.0.1:4470/4471, a CI throwaway
deploy/pipeline status                        # what test and prod run, their suites, the runner and its branch
deploy/pipeline releases                      # every release: its suite, tested?, where it runs
```

- **The release names its branch** (`build --branch B`: B's tip), and is
  **the perf build** (RUN-39: `[profile.perf]`, the one build perf runs and
  releases use; `RELEASE` says `profile: perf`). A tree
  with no `frontend/` builds `cargo build --profile perf -p menger`, the
  `plexus-ui` tool from `../plexus-ui` (FRAME-8; the release carries it as
  `bin/plexus-ui`), the engine (`plexus-ui build --prebuild`), the bundle
  (`plexus-ui build --contract HASH` → `dist/ui.tar`) and
  `dist/ui-compiler.wasm`, all on this machine — no container. The image runs
  with `--ui-db-path`, a `--ui-signer` per key of `MENGER_UI_SIGNERS` and
  `--ui-compiler`.
- **A deploy pushes the bundle**: after a healthy `remote test` (and
  `local`), `plexus-ui push` as the `pipeline` key (a `deployer`; the
  release's own `bin/plexus-ui`), signed by that key; the smoke wants that
  version live. A refused push
  fails the deploy and leaves the version that was live.
- **Every remote test and remote prod records itself on the tracker**
  (DEPLOY-12, DEPLOY-28) as the `pipeline` key, through `deploy/record`; a
  record the tracker does not take waits in
  `~/.local/share/menger-deploy/outbox/` and never fails a deploy.
- **Production changes only through `remote prod`**, and only to a release
  that passed test and whose suite passed. A `cargo build` in a checkout
  changes nothing that is serving.
- **Complete means deployed to the lowest environment** (owner,
  2026-10-17; DEPLOY-22): the merge links its commit and leaves the ticket
  In Review; the deploy to test that carries it sets Complete. Work in a
  repository with no environment (`repo@hash`) is Complete at its merge.
- **Test is automatic; prod is the owner's** (DEPLOY-48). The deploy
  runner (`deploy/runner.mjs`) watches the tracker: a MENGER ticket entering
  In Review, or a commit linked to one there, files a test request for the
  tip of **the runner's branch** — `MENGER_RUNNER_BRANCH` in
  `~/.config/menger/deploy-runner.env` (default `main`; V2-7) — debounced
  60 s, one run at a time. The run is build → image → app-test.menger.sh →
  the suite; a red one is posted on the tracker with a gap blocking the
  triggering tickets (CI-3). So a builder merges `--no-ff`, links, sets In
  Review, and runs no deploy; never squash or rebase on merge.
  `MENGER_RUNNER_AUTO_TEST=0` turns it off.
- 2026-10-29: test runs v2, release `20261029T114133Z-06aeff5`; prod runs
  v1 and is the tracker.

## Reaching the service

Everything menger does is a Plexus RPC method on the `menger` service —
there is no side door. **Read `../menger-skills/reaching/SKILL.md` before
issuing synapse commands**: it is the house's invocation contract (global
options before the service name, params as flags, a list as its flag
repeated or comma-separated, a body as `--body @file.md`).

**The tracker is prod** (RUN-13): https://app.menger.sh, reached as
`synapse --url wss://app.menger.sh/ws --as builder menger tickets health`
after one `_login` there. The tracker runs v1 until v2 is promoted, so
it answers more than this file names; menger-skills `reaching` and `asking`
are its surface. Check work on test as your key there, or on a throwaway —
never on prod:

```sh
synapse --url wss://app.menger.sh/ws _login menger --key builder --as builder
synapse --url wss://app.menger.sh/ws --as builder menger tickets get --id V2-8
synapse --url wss://app-test.menger.sh/ws _login menger --key builder --as builder
synapse --url wss://app-test.menger.sh/ws --as builder menger ui versions
```

This machine's agents are Ed25519 keys kept by synapse
(`~/.plexus/keys/<name>.key`, 0600):

| key | principal | holds |
|---|---|---|
| `builder` | `key:54a9677a…3b0d28` | `editor` on `*` — the machine's default agent |
| `pipeline` | `key:0ca8b6d1…227f60` | `viewer` on `*` + `deployer` — the pipeline's records, and on test `ui.push` |
| `operator` | `key:ba720a3d…3974b8e` | `--admin` — grants and break-glass, by hand only |

**Exit 77 means sign in**, not a dead server: `_login` once per machine
(and per server start for a throwaway); synapse re-mints the session from
the key after a restart or expiry. Run `_login` for another key with `--as`
and then `_login` builder again last (SYNAPSE-16). Never put a token or a
seed in argv or the environment.

### Reading and writing tickets

```sh
synapse -P 4460 menger tickets health
synapse -P 4460 menger tickets get --id MENGER/ORG-3
synapse -P 4460 menger tickets list --epic MNG --summary true
synapse -P 4460 menger tickets list --filter 'blocked' --at 1000 --summary true --limit 20
synapse -P 4460 menger tickets list --epic MENGER --summary true --limit 1000 --after MENGER-1000
synapse -P 4460 menger tickets list --summary true --limit 200 --fields id,title,kind,status
synapse -P 4460 menger tickets find --query watch --filter 'not category done'
synapse -P 4460 menger tickets search --query 'plcement wave'
synapse -P 4460 menger tickets search --query 'set_status' --filter 'not category done' --limit 5 --offset 0
synapse -P 4460 menger tickets history --id MNG-7
synapse -P 4460 menger tickets watch          # open-ended
synapse -P 4460 menger tickets watch --filter 'category in (active, review)' --membership true
```

A body is a file (`--body @body.md`, its contents exactly), never
`"$(cat …)"`:

```sh
synapse -P 4460 menger tickets create --epic KILN --kind scope --title "the kiln" --body @body.md
synapse -P 4460 menger tickets create --epic KILN --title "a firing log" --kind build --parent KILN-1 --blocked-by KILN-5 --severity High
synapse -P 4460 menger tickets amend --id KILN-3 --body @body.md
synapse -P 4460 menger tickets set_status --id MNG-1 --status Ready
synapse -P 4460 menger tickets set_status --id MNG-7 --status Canceled --message "folded into MNG-9"
synapse -P 4460 menger tickets set_status --id MNG-7 --status Pending --restore-children true
synapse -P 4460 menger tickets set_kind --id IDENTITY-1 --kind scope
synapse -P 4460 menger tickets set_kind --id MNG-40 --kind gap --status Ready
synapse -P 4460 menger tickets set_parent --id KILN-9 --parent KILN-2
synapse -P 4460 menger tickets label --id MENGER-40 --add needs-you
synapse -P 4460 menger tickets commits link --id MNG-10 --commits 77df9a0 --commits volt@1a2b3c4
synapse -P 4460 menger tickets commits unlink --id MNG-10 --commits 77df9a0
```

**`update`** (MENGER-30) changes any part of a ticket in one write — all of
it or none, each aspect by its own write's rules, a refusal naming every
reason; it answers `updated { ticket, changed }`. It is how a blocker is
added after filing, or a severity changed (`none` clears it).
**`update_many`** (ACT-2) is the same to many — `--ids`, or `--filter` (at
most `--max`, default 500) — plus `--label` / `--unlabel`:

```sh
synapse -P 4460 menger tickets update --id THEME-4 --block THEME-7
synapse -P 4460 menger tickets update --id MENGER-30 --unblock MENGER-23 --block MENGER-29
synapse -P 4460 menger tickets update --id MENGER-40 --kind gap --status Ready --severity High --hidden false
synapse -P 4460 menger tickets update_many --ids THEME-9,THEME-10 --status Ready --message "promoted"
synapse -P 4460 menger tickets update_many --filter 'label stale and not category done' --status Canceled
synapse -P 4460 menger tickets label --ids A-1,A-2 --add needs-you
```

**`delete`** (ACT-5, admin) removes tickets for good — refused while
children or a waiting ticket point at one unless told what moves
(`--children reparent|delete`, `--unblock true`); a tombstone stays, the
body is kept 30 days for `undelete`, a number is never reused. Prefer
`Canceled` or `Superseded` for work that is not done. **`reshortcode`**
(ACT-6) moves tickets into another shortcode, renumbered, the old id pinned;
**`rename`** (TICKETS-26) moves one into a subtask id under its own parent.
**Subtasks** (TICKETS-28) are `<parent>.N`, hidden from the nav's views.
**`freeze`** / **`unfreeze`** (TICKETS-6, admin) stop and restart a scope's
growth:

```sh
synapse -P 4460 menger tickets delete --ids MENGER-40 --message "filed twice"
synapse -P 4460 menger tickets delete --ids THEME-12 --children reparent --unblock true
synapse -P 4460 menger tickets undelete --ids MENGER-40
synapse -P 4460 menger tickets reshortcode --ids THEME-9,THEME-10 --to LOOK
synapse -P 4460 menger tickets reshortcode --ids MENGER-2 --to APP --with-children true --dry-run true
synapse -P 4460 menger tickets reshortcode --ids KILN-4 --to KILN --new-scope "glazes" --parent KILN-1 --message "split out"
synapse -P 4460 menger tickets rename --id KILN-5 --to KILN-2.1 --dry-run true
synapse -P 4460 menger tickets subtasks create --parent KILN-2 --title "load the shelves"
synapse -P 4460 menger tickets subtasks list --parent KILN-2
synapse -P 4460 menger tickets freeze --id KILN-1
synapse -P 4460 menger tickets unfreeze --id KILN-1
```

**The archive** (ARCHIVE-2) is a mark apart from status — a ticket with its
whole subtree out of every view unless a filter names `archived`; `restore`
brings it back with its archived ancestors:

```sh
synapse -P 4460 menger tickets archive --ids KILN-1 --dry-run true --message "first season fired"
synapse -P 4460 menger tickets archive --filter 'category done and under KILN' --max 500
synapse -P 4460 menger tickets restore --ids KILN-4 --with-children true
```

**Revisions** (TICKETS-7) — every write of a ticket's own fields keeps one;
a restore is an `update`:

```sh
synapse -P 4460 menger tickets revisions list --id DEPLOY-9 --limit 20
synapse -P 4460 menger tickets revisions get --id DEPLOY-9 --rev 3
synapse -P 4460 menger tickets revisions diff --id DEPLOY-9 --from 3 --to 5 --text true
synapse -P 4460 menger tickets revisions restore --id DEPLOY-9 --rev 3 --fields body --message "why"
```

### Questions and the queue

A question is an input of a ticket's body; `ask` appends a text one. An
agent queues the ticket, then waits (`await_reply`, open-ended) for the
first response. `queued` and `inputs list` are pages (`--limit`, `--after`
the `next` a page answered):

```sh
synapse -P 4460 menger tickets inputs list --state open --limit 50
synapse -P 4460 menger tickets inputs ask --ticket KILN-2 --question "Hold at cone 6 or 5?"
synapse -P 4460 menger tickets inputs answer --ticket KILN-2 --input ask-1 --value.type text --value.text "cone 6"
synapse -P 4460 menger tickets inputs close --ticket KILN-2 --input ask-1
synapse -P 4460 menger tickets inputs reopen --ticket KILN-2 --input ask-1
synapse -P 4460 menger tickets inputs clear --ticket KILN-2 --input ask-1
synapse -P 4460 menger tickets queue --id KILN-2 --urgency blocking --reason "idle until it is decided" --input ask-1
synapse -P 4460 menger tickets queued --limit 50
synapse -P 4460 menger tickets snooze --id KILN-2 --minutes 60
synapse -P 4460 menger tickets request_changes --id KILN-2 --body @body.md
synapse -P 4460 menger tickets unqueue --id KILN-2
synapse -P 4460 menger tickets await_reply --id KILN-2 --input ask-1 --timeout-minutes 30    # open-ended
synapse -P 4460 menger tickets list --filter 'queued now or asking or asked' --summary true
```

### Filters, the nav and the order of work

A filter is one quoted flag in FILTER-3's language, answered from the
cells: `ready and kind build and in SCALE`, `status in (Pending, Ready)`,
`not category static`, `under X` (X and its descendants, or a shortcode
and those hosted under it), groups nested to any depth, `saved "Name"`.
History reads the log (MENGER-13): `entered <status> [window] [by who]`,
`moved …`, `changed …`, `created …`, `archived …`; a time modifies the rule
before it. README "Filters" has the words, and the words of the services
v2 cut, which are refused by name:

```sh
synapse -P 4460 menger tickets list --filter 'status "In Review" within 12h' --summary true
synapse -P 4460 menger tickets list --filter 'kind gap within 7d or created after 2026-10-01' --summary true
synapse -P 4460 menger tickets list --filter 'entered Complete this week by owner' --tz Europe/Berlin --summary true
synapse -P 4460 menger tickets list --filter 'archived within 7d' --summary true
synapse -P 4460 menger tickets list --filter 'saved "Needs you" or label needs-you' --summary true
synapse -P 4460 menger tickets filters save --name "Open builds" --filter 'kind build and not category done'
synapse -P 4460 menger tickets filters list
synapse -P 4460 menger tickets filters get --name "Open builds"
synapse -P 4460 menger tickets filters check --filter 'saved "Open builds" and ready'
synapse -P 4460 menger tickets filters rename --name "Open builds" --to "Builds"
synapse -P 4460 menger tickets filters delete --name "Builds"
synapse -P 4460 menger tickets nav groups
synapse -P 4460 menger tickets nav extents
synapse -P 4460 menger tickets nav counts --filter 'ready and kind build'
synapse -P 4460 menger tickets nav facets --filter 'status Ready and kind build' --base 'not category done'
synapse -P 4460 menger tickets nav member --ids FILTER-4,FILTER-5 --filter 'status Ready'
synapse -P 4460 menger tickets nav tree --epic FILTER --filter 'status Ready'
synapse -P 4460 menger tickets nav tree --parent FILTER-1 --filter 'not category done' --matches-only true --keep FILTER-23 --limit 50
synapse -P 4460 menger tickets work ready --filter 'kind build and in SCALE' --summary true
synapse -P 4460 menger tickets work waves --epics MENGER,TICKETS --kinds build,gap
synapse -P 4460 menger tickets work waves_under --id MENGER-1
synapse -P 4460 menger tickets work waves_to --id MENGER-30
synapse -P 4460 menger tickets work blockers --id MENGER-30
```

### Addresses, organizations, members

A shortcode is unique within its namespace (ORG-3); every `--id`,
`--parent`, `--blocked-by`, `--epic` and `under` takes any short form of a
ticket's address (`MENGER/ORG-3`, `ACCESS/ORG-3`, the bare id); a name that
fits two is refused in band, naming each candidate. **Every ticket has a
uid** (TICKETS-35, on v2 V2-42: 8 characters of Crockford's base32,
`7k3mq2xa`, kept through every move): each of those takes it too, bare or in
the UI's link, `#/t/<uid>/<ID>-<slug>` — which the UI makes from one helper,
`ui/src/ticket-links.svelte`, real Svelte (svelte/1) (README "A ticket's uid", "A ticket's link"). A scope roots its own
shortcode (ORG-5). Every ticket is in one organization (ORG-4) and one epic
(ORG-13; V2-25: the `epics` methods went — a read is the organization's
whole set, and an epic is named only in an address). A grant's scope is `*`, `org:`, a shortcode, a ticket
alone or `under` a ticket (ACCESS-21); a team holds grants for its members
(ACCESS-22); `--everyone true` grants every signed-in principal
(ACCESS-57). README "Addresses", "Organizations", "Members and roles":

```sh
synapse -P 4460 menger tickets create --epic PLEXUS/THEME --title "a build in PLEXUS's THEME"
synapse -P 4460 menger orgs list
synapse -P 4460 menger orgs create --name acme --admin key:54a9677a62e4289ee5a2d0a4751a6f188abafd8b1f2b2d4019ec0768eb3b0d28
synapse -P 4460 menger orgs rename --name acme --to acme-co
synapse -P 4460 menger orgs delete --name acme-co
synapse -P 4460 menger tickets reshortcode --ids MENGER-40 --to hypermemetic:fresh:MENGER --dry-run true
synapse -P 4460 menger members mine
synapse -P 4460 menger members list
synapse -P 4460 menger members grant --principal key:54a9677a62e4289ee5a2d0a4751a6f188abafd8b1f2b2d4019ec0768eb3b0d28 --scope 'under MENGER-5' --role viewer
synapse -P 4460 menger members revoke --principal key:54a9677a62e4289ee5a2d0a4751a6f188abafd8b1f2b2d4019ec0768eb3b0d28 --scope 'under MENGER-5'
synapse -P 4460 menger members grant --everyone true --scope DOCS --role viewer
synapse -P 4460 menger members revoke --everyone true --scope DOCS
synapse -P 4460 menger members requests
synapse -P 4460 menger members pairing --code ABCD-EFGH
synapse -P 4460 menger members teams create --name release-crew
synapse -P 4460 menger members teams list
synapse -P 4460 menger members teams add --team release-crew --principal key:54a9677a62e4289ee5a2d0a4751a6f188abafd8b1f2b2d4019ec0768eb3b0d28
synapse -P 4460 menger members grant --team release-crew --scope 'under MENGER-5' --role editor
synapse -P 4460 menger members teams show --team release-crew
synapse -P 4460 menger members teams remove --team release-crew --principal key:54a9677a62e4289ee5a2d0a4751a6f188abafd8b1f2b2d4019ec0768eb3b0d28
synapse -P 4460 menger members teams rename --team release-crew --to ship-crew
synapse -P 4460 menger members teams delete --team ship-crew
```

### Login and the AUTH protocol

Login is always on (ACCESS-24): `--auth-provider` names at least one
provider — `key` (agents), `idp=…`, `auth0=…`, `oidc:<name>=…`, `workos=…`
or `nostr=…` — else menger runs as its local admin on loopback, or refuses
beyond it. Sessions are kept beside the database
(`<database>.sessions.jsonl`, FRAME-21), so a restart or a deploy signs no
browser and no agent out. Each becomes an adapter at the WebSocket upgrade; nothing past
the upgrade knows the provider. An anonymous connection reaches `auth`'s
door and nothing else. **Every method is its own grant** (AUTH-7), a role is
a list of grants declared with `roles(...)` on each activation (`admin`
holds all), and plexus-core's gate decides every call — a write by its
target, a read wherever its grant is held, answering only from there. A
refusal is typed (`error_kind: forbidden`, `grant`, `scope`, `roles`,
`held_on`). A new method gets a role by being listed in its activation's
`roles(...)`; unlisted, only admin holds it. An agent's session may be
minted with a ceiling (`role@SCOPE`). `auth role_set --role R --grants G,…`
(admin on `*`) changes a role; `auth grant` / `auth revoke` take
`--principal --role --scope`; `auth login --flow` and `auth signup [--via]`
are the door's; `session login [--ttl-secs] [--scope]` mints an agent's
session:

```sh
synapse -P 4460 menger session whoami
synapse -P 4460 menger session providers
synapse -P 4460 menger auth whoami
synapse -P 4460 menger auth providers
synapse -P 4460 menger auth roles
synapse -P 4460 menger auth requires --method tickets.set_status
synapse -P 4460 menger auth waiting
```

**Limits** (ACCESS-29): connections in all, per IP, per principal and
anonymous (429 past them), calls open on one connection (`error_kind:
too_many_calls`), an idle connection, an unused browser sign-in, titles
(1 KiB), filters (4 KiB), bodies (8 MiB), `list` pages (1,000 unless
`--limit`, at most 10,000 — page with `page.next`). README "Limits and
caps".

**Every parameter is a type** (ACCESS-8, `tickets-api/src/vocab/`): a value
its type refuses is `-32602 invalid_params` naming the parameter, before the
method runs, and so is a key no parameter claims — ``unknown field
`sumary`; tickets.list takes: …`` (MACRO-31; `unknown field` is what
synapse heals a stale schema cache on), so a typo in `-p` JSON is refused,
never read as the default; domain refusals stay in band. An enum flag
takes its canonical spelling only (`--status Coding`, `--kind` lower case). A new parameter gets
a type in the vocabulary — every activation is `typed_params` and
`tickets-api/tests/param_types.rs` holds the flag.

## Asking the owner

A question for the owner is filed on its ticket, never asked only in
conversation (owner, 2026-10-12, MENGER-12): label the ticket `needs-you`
with the reason in its body, and mention the ticket id. Its saved filter,
`Needs you`, is `queued or asking or asked or label needs-you`; whoever acts
on it takes the label off:

```sh
synapse -P 4460 menger tickets label --id MENGER-40 --add needs-you
synapse -P 4460 menger tickets list --filter 'label needs-you' --summary true
synapse -P 4460 menger tickets label --id MENGER-40 --remove needs-you
```

The tracker (prod, v1) has more ways to ask, and to ask urgently;
menger-skills `asking` says when and how. An agent never answers its own
question.

## Conventions

- Plexus crates come from the **0.7 line by published version**
  (PLEXUS-16): `plexus-core`, `plexus-macros`, `plexus-client`,
  `plexus-transport`, `plexus-auth-core`, `plexus-webapp`. What is
  unpublished stays by sibling path: `../plexus-login` and `../plexus-ui`
  (FRAME-1's rule, `tests/apart.rs`); a clone of menger alone does not build
  until those are decided. tether takes plexus-substrate and plexus-login
  from their public repositories, each at a revision (PLEXUS-13, V2-44), the
  plexus-login being the one substrate names. Every activation is `impl FooActivation` (MACRO-13,
  MACRO-21): the stem names the namespace, the trait and the client.
- **Workspace members** (`Cargo.toml`): `.`, `tickets-api`, `media`, `quota`;
  `tether/` is its own workspace. Three repositories
  hold what menger lifted out: `plexus-webapp` (FRAME-4; published),
  `../plexus-login` (FRAME-4; by path) and `../plexus-ui`,
  of which menger takes `plexus-ui-service` (FRAME-5: a dependency, its
  routes mounted on the shell) and `plexus-ui-compiler` (FRAME-3: a
  dev-dependency) by path. The engine, `plexus-ui-engine`, is in no graph of
  menger's: the framework's `plexus-ui` tool builds it (FRAME-8), and
  `scripts/plexus-ui` and `deploy/pipeline` build the tool from there.
- The `tickets` activation lives ONCE, in `tickets-api/`
  (`menger-tickets-api`, MFE-11): the vocabulary, the store, and
  `activation/mod.rs`'s `#[activation]` on `impl TicketsActivation` (its
  server half behind the crate's default `server` feature). The macro emits
  `TicketsClient<T>` from it. Don't re-declare a wire type or a method
  signature anywhere else.
- **Every page goes out under a Content-Security-Policy** (ACCESS-28,
  `src/shell.rs`'s `PAGE_CSP`): no inline script but the page's own blocks,
  hashed; nothing from another origin; connections only to itself, its
  socket and loopback. Something a page needs from elsewhere is a change to
  that policy, made deliberately. A page in a body (a mockup, a raw-HTML
  body, EDITOR-14) runs in a sandboxed frame of the shell's `/frame`, under
  `FRAME_CSP` alone — never a `srcdoc`, which would inherit `PAGE_CSP`.
- Every RPC method returns `impl Stream<Item = …Event> + Send + 'static`;
  event enums use `#[serde(tag = "type", rename_all = "snake_case")]` with an
  in-band `Error { message }` variant.
- menger is DB-first: the database is the system of record. The repository
  holds the source and nothing of the store (owner, DEPLOY-5): no tickets,
  no snapshot of the live tracker as a fixture — test data is synthetic.
  The kinds are the owner's closed five (AGS-13) — `scope`, `execution`,
  `build`, `gap`, `spike` — and `doc` (SDLC-11), which is not work: born in
  `Doc`, never moved, never blocked or a blocker. File findings and
  references as docs, children of the ticket that produced them.
- A kind changes only through **`tickets.set_kind`** (SDLC-13), which moves
  everything kept by kind in one write; `--verify-derived` checks it.
- Architecture (owner-pinned): a service-wide in-memory cache
  (`tickets-api/src/store/`, re-exported as `menger::store`) is the ONLY
  interface the application has to tickets. Reads hit the cache, never
  storage, and are async-safe (no lock across an await, no I/O on the read
  path); writes go through the cache to the database. Don't hand any other
  module a database handle.
- **An activation keeps only its methods** (owner, 2026-10-12, MENGER-29).
  A method in an activation's `impl` is its attribute, the auth check and a
  match on what the store answers; its logic lives in the store (a write in
  a module of its own under `tickets-api/src/store/`, its tests in
  `store/tests_<topic>.rs`) or beside the activation
  (`activation/events.rs`, `activation/sight.rs`).
  `tickets-api/tests/sizes.rs` holds every activation file, and
  `store/mod.rs`, to a line budget; raise one only as a deliberate edit.
- **The frame** (ACCESS-13; README "The frame"): every activation is
  declared `auth = "required", request = MengerRequest, typed_params,
  roles(…)`; every method takes `#[from_auth(self.access.verify)] user:
  VerifiedUser` (never `auth: Option<&AuthContext>`, never `ctx.user_id`),
  its description and each parameter's are `///` docs, and its body is at
  most 15 lines — a permit or a scoped read, then `answered!` or a
  `*_stream` beside it. A call with no identity is the anonymous one
  (`menger::calls::caller`), which the gate holds to the anonymous view.
- **Capabilities** (ACCESS-9..12; README "Capabilities, reads, watches and
  writes"): `Access::verify` → `VerifiedUser`, then `permit` / `admin_of`;
  never build one by hand. Every public store read takes its reader and
  answers only its reach; every write takes the proof for what it writes,
  minted in `activation/writes.rs`; every watch holds its caller. Store tests
  read as `SYS` or `tests_reach::viewer_of` and write as `SYS.permit(id)`.
- A ticket's outputs are tickets (owner, 2026-09-06): a report, a spike's
  result or a finding is filed as a CHILD ticket of the ticket that made it,
  not as a file in the repository.
- Planning is in the tracker, per menger-skills (`../menger-skills`:
  `ticketing`, `planning`, `asking`, `building`, `reaching`; `ought` is
  retired). Tickets are born `Draft` (TICKETS-13) and stay `Draft` or
  `Pending` until the owner promotes them to `Ready`; implementation does
  not begin on a `Draft` or `Pending` ticket. The lifecycle is `Draft →
  Pending → Ready → Coding → In Review → Complete` (+ `Superseded` /
  `Canceled`): set **`Coding`** the moment work starts and record the branch
  on the ticket; set **`In Review`** in the step that finishes it — gate
  green, committed, **the commit linked** (SDLC-9: `tickets commits link
  --id X --commits <hash>`, `repo@hash` outside menger); the deploy to test
  sets `Complete`.
- **A scope roots its own shortcode** (ORG-5): a new area under a ticket is
  `create --kind scope --epic NEW --parent X`. **Three roots** (owner,
  2026-10-08): MENGER, PLEXUS and OUGHT; a new shortcode never adds a root.
  menger's areas are hosted under MENGER-1 (APP, STORE, TICKETS, RUN, and V2
  with UI under it); work on the platform goes under PLEXUS-1.
- Wire protocol notes are in README "Surface" — `tests/wire.rs` is the
  executable reference, `tests/v2_surface.rs` holds what v2 serves.
- Delegation (owner, 2026-09-06): implementation subagents run on Opus
  (`model: opus`); worktree isolation; one commit per ticket. A worktree
  builds into a target of its own (`CARGO_TARGET_DIR=~/.cache/cargo-target/<TICKET>`)
  and is removed in the same pass as its merge (MENGER-41). **An agent
  cleans up with `scripts/cleanup-gate.sh`, never a hand-written `rm -r`**
  (RUN-21): `scripts/cleanup-gate.sh NAME` says what the gate NAME left —
  its worktree and branch, its cargo build-dirs, `~/.cache/cargo-target/NAME`,
  `/tmp/menger-NAME.db*`, its session scratch — and `--yes` removes it and
  logs it; `--path P` takes one path by its shape, `--list` surveys every
  leftover and removes nothing. It refuses `/`, `$HOME`, a main checkout, a
  shared target, `~/.local/share/menger`, a dirty worktree and anything in
  use — a refusal is the answer, not a cue for `rm`. Its tests:
  `scripts/tests/cleanup-gate`. **Never `git stash` in a worktree**: a
  stash is the repository's, shared by every worktree, and another agent's
  `stash pop` takes yours (PERF-22, 2026-10-30); a work in progress is a
  commit on your branch.
- **Tests per ticket are a selection; the suite runs per batch** (CI-3,
  owner 2026-10-18). A ticket's gate is the tests it adds and the nearest
  ones it touches (`cargo test -p <crate> <filter>`, the one wire test, the
  one browser test; and `cargo test --workspace --doc` when a public signature or a crate's doc
  example moves — a lone `-p <crate> --doc` is a false red, the examples resolve only under the
  workspace's feature unification; PERF-20) — never `cargo test --workspace` per ticket. Merges
  gather on the branch, and the full suite runs once against the batch,
  right after its deploy to test (`deploy/pipeline suite`); a red one keeps
  the release from prod. A red there is the batch's: find the commit, file a
  gap that blocks its ticket, fix or revert, build again.
- **Builds clean up after themselves** (CI-2): the `systemd --user` timer
  `menger-sweep.timer` (hourly; `scripts/sweep/install.sh --start`) runs
  `cleanup-gate.sh --sweep --yes`, then `scripts/disk-check.sh --alert`.
  It also takes the throwaway's `ui8-*` work directories (RUN-34) in a
  scratch root — `~/.cache/menger-perf-scratch`, or one `MENGER_SCRATCH_DIRS`
  names in `~/.config/menger/sweep.env` — once idle an hour, at the next
  hourly sweep (below `/tmp/claude-UID/` the caller removes them by `--path`).
  `deploy/pipeline build` refuses up front under 50 GB free or at 90% used.
  Before a long gate of your own, `scripts/disk-check.sh` (exit 3: low).

## Naming: a Plexus backend is a *service*, never a *hub*

**Owner, 2026-10-29: "we don't call them hubs anymore. we call them services."** Every
name says service: `DynamicService`, `PlexusService`, `ServiceTransport`,
`service-codegen`, "the Rust service", "service to service", `--service`.
Any remnant of *hub* found anywhere — an identifier, a flag, a crate or
binary name, a doc, a skill, a ticket body, a commit message — is renamed to
*service* the moment it is seen (the sweep is tracked under PLEXUS-1; a
builder renames what is inside its own scope and never writes a new *hub*).
The only *hub* that stays is a foreign name we do not own (GitHub, a
third-party crate's API).
