# menger

A plan is a program whose leaves are prose. Plexus RPC ticketing backend +
the seed of a Plexus-native reactive web framework. Successor to
`../menger-hs` (0.04, Haskell); its rule carries over: frontend and backend
online from the first commit — every noun visible the day it is written.

## Commands

```sh
cargo test         # unit tests + wire tests (tests/wire.rs speaks the real protocol)
# a development server: its own ports and its own database, never prod's
PLEXUS_NO_REGISTRY=1 cargo run -- --port 4460 --shell-port 4461 --rest-port 4462 --db-path /tmp/menger-dev.db
```

Bare `cargo run` takes prod's ports (4455–4457) **and prod's database**
(the default `--db-path` is `~/.local/share/menger/tickets.db`) — never run
it that way while prod is installed.

## Deploying

menger runs in two environments on this machine, **prod** (4455–4458) and
**test** (4450–4453), both under `systemd --user`, both from immutable
releases — never from a checkout. `deploy/README.md` defines the pipeline
(DEPLOY-1); in short:

```sh
deploy/pipeline build          # main's tip → gate (cargo test --workspace + frontend tests) → release
deploy/pipeline deploy test    # the release → test, on a fresh copy of prod's data; smoke checks
deploy/pipeline sync test      # test's data refreshed from prod, same release (DEPLOY-4)
deploy/pipeline promote        # the release test passed → prod; smoke; automatic rollback on failure
deploy/pipeline status         # what each env runs, whether it is healthy, its last deployment record
deploy/pipeline backfill       # deployments.log → menger's deployments (DEPLOY-12; idempotent)
deploy/pipeline landing        # DEPLOY-17: main's landing/ → gate → menger.sh (infra's pin moved, the
                               # edge applied, the live page verified, recorded as env `edge`)
deploy/pipeline landing --check  # the gate and infra's dry run, nothing written
deploy/pipeline media          # landing/media regenerated from the release prod runs; then commit
```

- **Every deploy test, sync, promote and rollback records itself in menger**
  (DEPLOY-12): `started`, then `succeeded` / `failed` / `rolled_back`, sent
  to the tracker (`MENGER_DEPLOY_TRACKER_PORT`, 4455) through
  `deploy/record`. A record the tracker does not take waits in
  `~/.local/share/menger-deploy/outbox/` for the next run; it never fails a
  deploy. A test of the pipeline runs it on a throwaway root
  (`deploy/tests/pipeline`), never against the real environments.

- **menger.sh changes only through `deploy/pipeline landing`** (DEPLOY-17):
  never by editing infra's pin or running `bin/infra` by hand. It deploys a
  commit on main, so merge first; a change to the app's look is followed by
  `deploy/pipeline media`, its files committed, then `landing`.
- **Production changes only through `promote`**, and only to a release that
  passed the test environment. A `cargo build` or `trunk build` in a checkout
  changes nothing that is serving.
- Merging to main is not deploying: a ticket is `Complete` at the merge,
  and reaches users when its release is promoted — say which release in the
  ticket when it matters.
- Check work on the test environment (http://127.0.0.1:4451/) or a throwaway
  server — never on prod. The tracker itself is prod: `synapse -P 4455` is
  still how tickets are read and written.

## Reaching the backend

Everything menger does is a Plexus RPC method on the `menger` hub
(WebSocket :4455) — there is no side door, and nothing an agent needs is
off the wire. The activation publishes its schema; every client below is
derived from it or from the declaration that produces it. (Mermaid is
checked by a separate service, `diagrams` on :4458 — `diagrams/`,
COMPILE-4, README "Diagrams"; menger runs without it.) Pick by consumer:

- **`synapse`** (`../synapse`; on PATH) — the CLI over any Plexus backend.
  This is how an agent, a worker or a shell script reads and writes
  tickets; prefer it to hand-rolled curl against the REST bridge.
  **Read `../ought/synapse/SKILL.md` before issuing synapse commands** —
  it is the house's invocation contract (where flags go, how to navigate
  to a method, how to discover its parameters from the CLI itself); what
  follows here is only what is particular to menger.

  ```sh
  synapse -P 4455 menger tickets health
  synapse -P 4455 menger tickets list --epic MNG
  synapse -P 4455 menger tickets set_status --id MNG-1 --status Ready
  synapse -P 4455 menger tickets watch          # open-ended stream
  ```

  **Filters** (FILTER-4) are one quoted flag in FILTER-3's language —
  `ready and kind build and in SCALE`, `status in (Pending, Ready)`,
  `category in (active, review)`, `not category static`; answered from the
  cells, never by testing a ticket. `under X` (FILTER-13) is X and its
  descendants for a ticket, and a shortcode, its root and every shortcode
  hosted under it for a shortcode — answered from the kept tree, on every
  filtered read. `text` (the palette's `find`) is refused, as is any chunk
  the language cannot read:

  ```sh
  synapse -P 4455 menger tickets counts --filter 'ready and kind build'    # per shortcode
  synapse -P 4455 menger tickets list --filter 'blocked' --at 1000 --summary --limit 20
  synapse -P 4455 menger tickets facets --filter 'status Ready and kind build'   # menu counts
  synapse -P 4455 menger tickets watch --filter 'category in (active, review)'   # + entered/left
  ```

  The other reads take the same flag (FILTER-15) — `find`, `ready_set`
  and `inputs` answer what they answered, of the tickets the filter
  admits; `member` says which of up to 1,000 ids it admits:

  ```sh
  synapse -P 4455 menger tickets find --query watch --filter 'not category done'
  synapse -P 4455 menger tickets ready_set --filter 'kind build and in SCALE' --summary
  synapse -P 4455 menger tickets inputs --state open --filter 'kind build'
  synapse -P 4455 menger tickets member --ids FILTER-4,FILTER-5 --filter 'status Ready'
  ```

  **History** (MENGER-13): every status and kind change is a transition —
  who, from where, why, caused by what — read with `history --id X`.
  `set_status … Canceled` cancels the ticket's open descendants in the same
  write (and withdraws their open inputs); leaving `Canceled` with
  `--restore-children true` puts back what that cancel canceled. Filters
  read the log: `entered <status> [window] [by who]`, `left …`, `changed
  …`, `by …`; windows `within 12h`, `today`, `yesterday`, `this week`,
  `since/before/between` (UTC unless `--tz Europe/Berlin`):

  ```sh
  synapse -P 4455 menger tickets list --filter 'status "In Review" and entered "In Review" within 12h' --summary true
  synapse -P 4455 menger tickets set_status --id MNG-7 --status Canceled --message "folded into MNG-9"
  ```

  **Search inside tickets** (SEARCH-2) is `search` — titles and bodies,
  every word required, prefixes and typos too, best first; each `hit`
  carries the passage that matched (`find` stays the jump by id or title).
  It takes the same `--filter`, and pages with `--offset` / `--limit`:

  ```sh
  synapse -P 4455 menger tickets search --query 'plcement wave'
  synapse -P 4455 menger tickets search --query 'set_status' --filter 'not category done' --limit 5
  ```

  **The nav under a filter** is `tree` (FILTER-13): a parent's children —
  or a shortcode's top row (its root; a rootless shortcode's tickets) —
  that match or hold a match, each a `node { summary, matched, below }`
  (`below`: the matches strictly under it; `matched: false` a holder), then
  a `page`, paged as `list`. The nav asks it with `--matches-only`
  (FILTER-23): only matches — at a parent, the matches nearest below it (a
  ticket that does not match is no row; the matches under it are answered
  at its nearest matching ancestor, or at the shortcode's top), then the
  hosted shortcodes whose row is drawn there, each its root's node
  (`matched: false`: a heading); at `--epic`, the root if it matches, else
  the shortcode's top. `--keep ID` (FILTER-24, the nav's open ticket)
  answers as if that ticket matched — drawn where a match would be, its
  matches under it, its node still `matched: false`; `list` (with a
  filter) and `counts` take it too, and none of them keeps a ticket
  outside the caller's reach:

  ```sh
  synapse -P 4455 menger tickets tree --epic FILTER --filter 'status Ready'
  synapse -P 4455 menger tickets tree --parent FILTER-9 --filter 'not category done' --limit 50
  synapse -P 4455 menger tickets tree --epic MENGER --filter 'kind build' --matches-only true
  synapse -P 4455 menger tickets tree --parent FILTER-1 --filter 'not category done' --matches-only true --keep FILTER-23
  synapse -P 4455 menger tickets counts --filter 'ready and under FILTER-9'
  ```

  **Comments** (COMMENT-2) are the `comments` activation beside `tickets`:
  a reader's notes on the app and on ticket bodies, kept as records of
  their own — `add { anchor, body, author? }`, `list` (newest first; `ticket`,
  `under`, `route`, `path` prefix, `state` open/resolved, `author`, `text`;
  `after` + `limit`), `resolve`, `reopen`, `watch`. The anchor goes as
  `--anchor.route … --anchor.path …`; `list`'s `path` goes in `-p` (synapse
  rewrites a `--path` flag into a file path). The app writes them in its
  annotation mode (`a`, COMMENT-3) and lists them at `#/comments`; an anchor
  inside a framed body is `iframe[…] >> <selector in the body>`. README
  "Comments" has the rest:

  ```sh
  synapse -P 4455 menger comments list --under FILTER --state open --text title
  ```

  **Notes** (NOTE-2) are the `notes` activation: dated, untitled thoughts
  kept in one place — `add { body }`, `list` (newest first; `state`
  open/upgraded, `author`, `text`; `after` + `limit`), `get { id }`, and
  `upgrade { id, epic, title?, kind?, parent? }`, which makes the note a
  ticket (titled by its first line unless given) and keeps the ticket's id
  on the note, in one write. README "Notes" has the rest:

  ```sh
  synapse -P 4455 menger notes add --body "the palette should remember its last query"
  synapse -P 4455 menger notes upgrade --id 7 --epic MENGER --parent MENGER-2
  ```

  **Deployments** (DEPLOY-11) are the `deployments` activation: every
  deploy any CI records (DEPLOY-10's record, its fields as `record`'s
  params — `deploy/record --send` passes it whole), what each environment
  runs, and where each ticket's work runs. A project is a shortcode
  (`menger` is MENGER); an environment is a project's. A deploy's tickets
  are those whose linked commits are among its `commits` and `revision`
  (a hash prefix is the same commit — menger has no repository) plus the
  ids it names; a rollback takes out what only the undone deploys carried;
  `failed` and `started` run nothing; a deployment never moves a status.
  `deployed <env> [window]` and `not deployed <env>` are filter cells.
  `record` is the `deployer` grant (a CI's key on the project's
  shortcode). README "Deployments" has the rest:

  ```sh
  synapse -P 4455 menger deployments record --project menger --environment prod --state succeeded --revision 235ff62 --commits 824e943,235ff62
  synapse -P 4455 menger deployments environments --project menger
  synapse -P 4455 menger deployments list --environment prod --limit 20
  synapse -P 4455 menger deployments for_ticket --id MENGER-34
  synapse -P 4455 menger deployments running --project menger --environment prod   # the kept cells
  synapse -P 4455 menger tickets list --filter 'category done and not deployed prod' --summary true
  ```

  The app (DEPLOY-13) shows it: a ticket page's Running, a mark per
  environment on board rows and wave cards, `#/deployments` (environments
  as columns, each deploy and what it carried, the Complete work waiting
  for the last one), the filter menu's Deployed and the palette's
  `deployed …` — all live on `deployments.watch`.

  **Themes** (THEME-8) are the `themes` activation: a theme is a name and
  token overrides for light and dark — written as a ticket's ` ```theme
  name [extends=parent] ` block (THEME-3: owned by that ticket as a
  component is, orphaned and adopted the same way, validated on write —
  tokens of the contract `tickets-api/src/tokens.toml` (THEME-2) in `:root`
  / `:root[data-theme="dark"]`, `@font-face`, `@import url("https://…")`,
  `m-*::part(…)`, looks only — a component's skinnable parts are the
  `<part name>`s its `<docs>` declares (THEME-5; a documented part its
  template lacks is refused); a refusal names the block's line), or with
  `define` for admin and tests (the maps go through `-p`);
  the site or a shortcode adopts one with `set` (`--reach frame|ticket|site`,
  `--takeover off`; THEME-4: a hosted shortcode inherits its host's, and a
  reader may turn project themes off in settings — `themes set --scope`
  is the spelling, there is no `tickets set_theme`), a ticket in its frontmatter (`theme:`, `reach:`,
  `takeover:`, `frame-theme:`, `frame-takeover:`); `theme_for` says who
  wears what and from which level; `watch` says `theme_changed` /
  `theme_removed`. PANTRY is the worked example (THEME-6). README "Themes
  and their scopes", "A theme is a ticket", "Parts to skin":

  ```sh
  synapse -P 4455 -p '{"name":"ocean","light":{"--accent":"#00796b"},"dark":{"--accent":"#4db6ac"}}' menger themes define
  synapse -P 4455 menger themes set --scope PANTRY --theme pantry --reach site
  synapse -P 4455 menger themes theme_for --id PANTRY-4
  ```

  Every open page follows a theme change in place (THEME-7): one repaint
  path in the app (`scopes::repaint`), `menger.on('theme')` in a frame for
  a component that draws with colours in JS. A change that touches how
  anything is painted is checked with the sweep test,
  `scripts/theme-sweep.mjs`, on a throwaway server (README "On the fly").

  **Login** (IDENTITY-14) is off unless `--auth-provider` names a
  provider: `idp=…`, `auth0=…`, `oidc:<name>=…` or `nostr=…`. Each
  becomes an adapter that turns its credential into a Plexus identity at
  the WebSocket upgrade, and nothing past the upgrade knows the provider.
  `session.whoami` reports the identity. README "Login" has the rest:

  ```sh
  synapse -P 4460 -t "$TOKEN" menger session whoami
  ```

  **Under login, menger speaks the AUTH protocol** (AUTH-7): every method
  is its own grant, `viewer` / `editor` are explicit grant lists declared
  with `roles(...)` on each activation (`admin` holds all), and plexus-core's
  gate decides every call — a write by its target (`target = "id"`), a read
  wherever its grant is held, answering only from there. An anonymous
  connection is admitted into the anonymous view (`auth.providers`,
  `auth.login`, `auth.signup`, nothing else); a refusal is typed
  (`error_kind: forbidden`, `grant`, `scope`, `roles`, `held_on`). `auth.*`
  (`whoami`, `requires`, `roles`, `role_set`, `grant`, `revoke`, `waiting`)
  sits beside `session.*` and `members.*`, which stay for the app. A new
  method gets a role by being listed in its activation's `roles(...)` —
  unlisted, only admin holds it. README "The AUTH protocol" has the rest.

  **Params are individual flags, not a JSON blob** — treat synapse as a
  normal CLI: `--kebab-case-flag value` after the method path, and
  `<method> --help` prints each parameter's name and type. **A list
  parameter is its flag repeated, or commas**: `--blocked-by MNG-1
  --blocked-by MNG-2` and `--blocked-by MNG-1,MNG-2` both send
  `["MNG-1","MNG-2"]` (commas need synapse ≥ 3.14, SYNAPSE-2; on an
  older binary `a,b` is silently one string). An
  element that itself holds a comma is double-quoted inside the value, and
  the shell must not eat the quotes: `'"a,b","c"'`. A multi-line `--body`
  is an ordinary quoted argument — `--body "$(cat body.md)"` — whatever it
  begins with (frontmatter's `---` included) and whatever it holds: a `$`
  the shell did not expand arrives literally. Both need synapse ≥ 3.14.2
  (SYNAPSE-4, SYNAPSE-6; this machine has it); `--key=value` works too.

  Global options go **before** the backend name — after the path they are
  read as a subcommand ("Method 'create' cannot have subcommands"). `-j`
  prints the raw JSON stream items (for a script that parses the reply),
  `-n` shows the request without sending it, `-s` fetches the schema, `-i`
  emits the IR the code generators consume, and `-p '{…}'` passes the whole
  params object at once — a script's convenience, never a requirement.
  4455 is inside its `_self scan` range.
- **The macro-emitted Rust client** — `TicketsClient<T>`, emitted by
  `#[plexus_macros::activation(client, …)]` from the activation's own
  `impl` in `tickets-api/` (MFE-11); `CommentsClient<T>` the same way
  (COMMENT-2). Compile-time and Rust-only: the
  browser app takes the crate with `default-features = false` and a
  renamed field or parameter fails its build. Nothing is generated or
  checked in.
- **`synapse-cc`** (`../synapse-cc`) — the compiler collection for every
  other language: schema → IR → `hub-codegen` → an installed, compiled,
  smoke-tested client. TypeScript and Rust targets today (`python` parses,
  no generator yet). `synapse-cc build typescript <backend> -o DIR`, or a
  `synapse.config.json` and plain `synapse-cc build`; `synapse-cc watch`
  rebuilds on schema changes. A generated client is a snapshot of the
  schema, so it is regenerated whenever the activation's surface changes —
  a new method, parameter or event. Run it **in the `plexus-codegen`
  container** (`../dockerfiles/plexus-codegen`, STARTER-2) rather than
  from a host install; `../plexus-starter`'s `scripts/codegen.sh` is the
  worked example of wiring that into `npm run build`, `cargo xtask` and
  CI. `synapse-cc` 0.3.4 and `hub-codegen` 0.4.1 are also installed in
  `~/.local/bin` (2026-10-02). menger's own use (COMP-10):
  `scripts/sdk-codegen.sh` generates the frames' typed SDK
  (`menger.tickets.*`) into `frontend/sdk/generated/` — rerun it whenever
  the activation's surface changes; `tickets-api/tests/sdk_manifest.rs`
  fails until it is.
- **A new language target** is a new `hub-codegen` generator over the same
  IR — never a hand-written client that mirrors the wire.
- **REST + SSE** (:4457) — for curl and for scripts that cannot run
  synapse. Unary responses are capped at 10,000 items (TRANSPORT-2).

## Inputs — asking the owner inside a ticket (INPUT)

A ticket can ask the owner something and record the answer: a **decision**
(pick one option), a **multi** (tick any) or a **text** reply. The reader
answers in the ticket's own frame; every answer is stored with its history
and is queryable. (INPUT-1; the house components are ART-9's.)

**Declare an input** in a ticket body — an `id`, unique in that ticket, is
what makes it an input (a decision without one is display only; a duplicate
id refuses the write):

```html
<m-decision data-field="decision" id="landing">
<m-question data-field="question">Expired events: archive or delete?</m-question>
<m-option data-field="option" name="Archive" value="archive" recommended cost="a second store" benefit="holds honoured"></m-option>
<m-option data-field="option" name="Delete" value="delete" cost="holds broken" benefit="nothing to run"></m-option>
<m-recommendation data-field="recommendation">Archive: holds are real.</m-recommendation>
</m-decision>

<m-input id="checks" kind="multi"><m-question data-field="question">Which checks ran?</m-question>
<m-choice value="unit">Unit tests</m-choice><m-choice value="wire">The wire test</m-choice></m-input>

<m-input id="notes" kind="text"><m-question data-field="question">Anything else before this ships?</m-question></m-input>
```

An option's value is its `value`, else its `name`.

**The owner answers; an agent doesn't** (ought: an agent never answers its
own decision). A reader's click records the answer with source `app`; a
reader may change or clear it until the input is closed.

**Read what's waiting, act, then close:**

```sh
synapse -P 4455 menger tickets inputs --state open          # everything waiting, across tickets
synapse -P 4455 menger tickets inputs --ticket RETURNS-13   # one ticket's inputs, answers and history
synapse -P 4455 menger tickets close_input --ticket RETURNS-13 --input landing   # after acting on it
synapse -P 4455 menger tickets reopen_input --ticket RETURNS-13 --input landing  # if it must be asked again
```

Act on an answer, then `close_input` it: closing freezes it (the reader can
no longer change or clear it) and the component shows DECIDED. States are
`open` · `answered` · `closed` · `removed` (gone from the body, history
kept). An answer can be cleared by the reader while open — read the state
before acting. `watch` carries `input_changed` (the whole input) for anything
that follows live; the app's `#/inputs` page lists them all.

**Answering through synapse** (the owner from a shell; source `api`):

```sh
synapse -P 4455 menger tickets answer --ticket T --input landing --value.type choice --value.option archive --rationale "why"
synapse -P 4455 menger tickets answer --ticket T --input notes --value.type text --value.text "the reply"
synapse -P 4455 -p '{"ticket":"T","input":"checks","value":{"type":"multi","options":["unit","wire"]}}' menger tickets answer
synapse -P 4455 menger tickets clear_answer --ticket T --input landing
```

A `multi` answer goes through `-p` for now — synapse sends a list inside a
nested parameter as one string (SYNAPSE-9).

**A question for the owner is filed, never asked only in chat** (owner,
2026-10-12, MENGER-12). Put it on its ticket as an input — or, when it is
not a choice or a reply, label the ticket `needs-you` with the reason in its
body — and only then mention it in chat, with the ticket id. Home's "Needs
you" panel (filter `asking or label needs-you`) is where the owner looks;
a question that lives only in a chat transcript is never seen. Whoever acts
on a `needs-you` takes the label off:

```sh
synapse -P 4455 menger tickets label --id MENGER-40 --add needs-you      # the reason is in the body
synapse -P 4455 menger tickets list --filter 'asking or label needs-you' --summary
synapse -P 4455 menger tickets label --id MENGER-40 --remove needs-you   # once acted on
```

## Conventions

- Plexus crates come from the **0.6 line by sibling path** (MFE-10):
  `../plexus-core` 0.6.0, `../plexus-macros` 0.6.0, `../plexus-transport`
  0.5.0, all on `main`, referencing each other the same way so the graph
  resolves one of each. The line is unpublished and untagged — MACRO-8 is
  that release — so a clone of menger alone does not build; the sibling
  checkouts must be beside it. Same macro API as before
  (`#[plexus_macros::activation]` + `#[plexus_macros::method]`,
  `crate_path = "plexus_core"`).
- The `tickets` activation lives ONCE, in `tickets-api/`
  (`menger-tickets-api`, MFE-11): the vocabulary (`Ticket`, `Status`,
  `Kind`, `Severity`, `TicketSummary`, `TicketsEvent`), the store, and
  `activation.rs`'s `#[activation(client, server_cfg = "feature =
  \"server\"")]`. The server takes the crate with `server` (default);
  the browser app takes it with `default-features = false` and gets the
  emitted `TicketsClient<T>` over wasm-safe crates. `menger::store`
  re-exports the store, so `crate::store::Status` still resolves. A
  renamed field OR parameter is a wasm compile error — there is no second
  declaration. Don't re-declare a wire type or a method signature
  anywhere else.
- The browser app's data plane is the Plexus WebSocket, not the REST
  bridge (MFE-9): one `WsTransport` for the app's lifetime, the typed
  client from `tickets-api`'s `client` feature, `watch` opened before
  anything else the session asks (SCALE-26: `health`, `groups`,
  `extents` and one board window — no walk). The REST bridge (its own
  port) stays for curl, scripts and non-Rust clients; the shell has no
  `/rest` proxy (ACCESS-33) and serves only menger's pages and the app's
  build, read into a table — never a request path on disk. `frontend/src`
  must contain no `/rest/`, no hand-parsed SSE, and no `serde_json::Value`
  on a wire path. Both halves are on the plexus 0.6 line (MFE-10), and the
  client the app uses is emitted from the server's own activation (MFE-11).
- Every RPC method returns `impl Stream<Item = TicketsEvent> + Send + 'static`;
  event enums use `#[serde(tag = "type", rename_all = "snake_case")]` with an
  in-band `Error { message }` variant.
- menger is DB-first: the database is the system of record — no plans/
  directory integration, no file-based ticket store. The repository holds
  the source and nothing of the store (owner, DEPLOY-5): no tickets, no
  ticket artifacts, no snapshot of the live tracker as a fixture — test
  data is synthetic, and `/plans/` is ignored. The status
  vocabulary matches `skills/skills/ticketing/SKILL.md` (`Pending`, …);
  the kind vocabulary is the owner's closed five (AGS-13): `scope`,
  `execution`, `build`, `gap`, `spike` — and a sixth, `doc` (owner,
  2026-10-02, SDLC-11), which is not work. Docs "are documents and are
  thus connected to a different set of sdlc movements. namely, none":
  a doc simply exists in its state, so it has no lifecycle — it is born
  in the status `Doc`, which only a doc holds, and nothing moves it
  (`set_status` on a doc is refused, and `Doc` on any other kind); it is
  never blocked, never a blocker, never ready, and a roll-up's
  `done/total` ignores it (`docs` counts it apart). File findings,
  manifests and references as docs — a child of the ticket that
  produced them, or under a doc collection's root (a doc may root a
  shortcode, as a scope may) — not as work that will never close.
  Don't invent a second vocabulary for either.
- A kind changes only through **`tickets.set_kind`** (SDLC-13; owner,
  2026-10-08: "identity is a scope") — never by editing a row — and
  everything kept by kind moves in that one write (roll-ups, status
  counts, the ready set, placements, the filter cells, the kept tree;
  `--verify-derived` checks it). Its rules: to a `doc`, the status becomes
  `Doc`, and a ticket that waits on a blocker or that a ticket waits on is
  refused (both named); from a doc, the status becomes `--status` when
  given (never `Doc`), else `Epic` for a scope and `Pending` otherwise;
  between two kinds of work the status stays unless given. A shortcode's
  root never becomes a kind that roots nothing (doc ↔ scope it may), a
  path holder stays a doc, and a lower-seq ticket made a scope or a doc
  becomes its shortcode's root. A kind and status it already has is a
  no-op:

  ```sh
  synapse -P 4455 menger tickets set_kind --id IDENTITY-1 --kind scope
  synapse -P 4455 menger tickets set_kind --id MNG-40 --kind gap --status Ready
  ```
- **`tickets.update`** (MENGER-30) changes any part of a ticket in one
  write — `--title`, `--body`, `--kind`, `--status`, `--parent`,
  `--severity` (`none` clears it), `--block` / `--unblock` (repeated or
  comma-separated) — all of it or none, each by its own write's rules
  (`amend`, `set_kind`, `set_status`, `set_parent`, `create`'s for
  blockers), a refusal naming every reason. It answers `updated { ticket,
  changed }`; `watch` carries each aspect's own event. The way to add a
  blocker after filing, or to change a severity:

  ```sh
  synapse -P 4455 menger tickets update --id THEME-4 --block THEME-7
  synapse -P 4455 menger tickets update --id MENGER-30 --unblock MENGER-23 --block MENGER-29
  synapse -P 4455 menger tickets update --id MENGER-40 --kind gap --status Ready --severity High
  ```

  **`tickets.update_many`** (ACT-2) is the same update to many tickets in
  one write, all or none — `--ids`, or `--filter` (at most `--max`, default
  500); `update`'s fields but title and body, plus `--label` / `--unlabel`;
  a refusal names each refused id. `label` and `link_commits` take `--ids`
  too:

  ```sh
  synapse -P 4455 menger tickets update_many --ids THEME-9,THEME-10 --status Ready --message "promoted"
  synapse -P 4455 menger tickets update_many --filter 'label stale and not category done' --status Canceled
  synapse -P 4455 menger tickets label --ids A-1,A-2 --add needs-you
  ```

  **`tickets.delete`** (ACT-5, admin only) removes tickets for good —
  `--ids` or `--filter`, all or none — refused while something points at
  one (children, a ticket waiting on it, a shortcode it roots, a component
  or theme it defines, a live input) unless told what moves: `--children
  reparent|delete`, `--unblock true`. A tombstone and the history stay;
  `get` answers `deleted`; the body is kept 30 days for `undelete --ids`;
  the number is never reused. Prefer `Canceled` / `Superseded` for work
  that is not done — delete is for a mistake, a duplicate, a test row:

  ```sh
  synapse -P 4455 menger tickets delete --ids MENGER-40 --message "filed twice"
  synapse -P 4455 menger tickets delete --ids THEME-12 --children reparent
  synapse -P 4455 menger tickets undelete --ids MENGER-40
  ```
- Architecture (owner-pinned): a service-wide in-memory cache
  (`src/store.rs`) is the ONLY interface the application has to tickets.
  Reads hit the cache, never storage; reads are async-safe (no lock
  across an await, no I/O on the read path); writes go through the cache
  write-through to the database. Don't hand any other module a database
  handle.
- **An activation keeps only its methods** (owner, 2026-10-12, MENGER-29:
  "make sure that the codebase activations aren't getting too huge"). A
  method in an activation's `impl` is its attribute, the auth check and a
  match on what the store answers; its logic lives in the store (a write
  of any size in a module of its own under `tickets-api/src/store/`, its
  tests in `store/tests_<topic>.rs`) or in a module beside the activation
  (`tickets-api/src/activation/events.rs` — answers and changes as wire
  events; `activation/sight.rs` — the caller's reach, clock and act;
  `activation/tests.rs`). `tickets-api/tests/sizes.rs` holds every file
  with an `#[plexus_macros::activation]`, and `store/mod.rs`, to a line
  budget: over it, the test says what to move out and where. Raise a
  budget only as a deliberate edit, never to make room for logic.
- A ticket's outputs are tickets (owner, 2026-09-06): a report, spike
  result, checkpoint, or evaluation memo is filed as a CHILD ticket of
  the ticket that generated it (`parent:` in frontmatter; typed once
  AGS-19 lands), not as a file in the repository.
- Planning: in the tracker (`synapse -P 4455 menger tickets …`) per the
  ticketing skill. Tickets are `Pending`
  until the human promotes them to `Ready`; implementation does not begin
  on `Pending` tickets.
- **Three roots** (owner, 2026-10-08: "we need fewer root scopes"): MENGER
  (menger itself), PLEXUS (the platform) and OUGHT (the methodology). A new
  shortcode never adds a root: create its root ticket with `--parent` set to
  where it belongs. menger's areas are MENGER-2 *the app*, MENGER-3 *the
  store and the server*, MENGER-4 *tickets*, MENGER-5 *running it*; work on
  the platform goes under PLEXUS-1. To move a shortcode, `set_parent` its
  root; a hosted root can't return to the top level.
- **Ask through the tracker** (owner, 2026-10-12, MENGER-12): a question
  for the owner is an input on its ticket (or the `needs-you` label with
  the reason in the body), and is mentioned in chat with the ticket id —
  never asked only in chat. `asking` and `label <name>` are filter cells
  (FILTER-3's language); Home's "Needs you" panel shows `asking or label
  needs-you`.
- Ticketing follows **ought** (`../ought`, the house methodology — its
  `ticketing/`, `building/`, `methodology/` skills). Its lifecycle is
  `Pending → Ready → active → in review → done` (+ `archived`, always
  with a pointer). Workers move their ticket across it (owner,
  2026-09-06): mark `active` the moment work starts (so no leaf is
  double-claimed) and record the branch on the ticket (`tickets.amend`
  rewrites its body — MNG-16); flip to `in review` in the commit that
  finishes the work — gate green, committed,
  change linked (the commit message ends `— in review`); the **merge**
  (the pipeline) lands it to `done`, so the merge hash is the completion
  hash. Each flip is `tickets.set_status` on the running server (and the
  ticket's frontmatter, where the worktree carries one), so the board
  shows where the work is. The server holds both worker states (SDLC-7)
  under menger's names: set **`Coding`** when work starts and **`In
  Review`** in the commit that finishes it. `set_status` also takes
  ought's words — `active` is `Coding`, `in review` is `In Review` — and
  stores the canonical spelling. The merge still sets `Complete`
  (menger's current name for `done`; SDLC-2 reconciles the vocabulary
  with ought's). **Link the commit** (SDLC-9): in the step that flips the
  ticket to `In Review`, the worker links its commit(s) with
  `tickets.link_commits` (`synapse -P 4455 menger tickets link_commits
  --id MNG-10 --commits 77df9a0`, the flag repeated per commit;
  `repo@hash` for a commit outside menger); the merge links the merge
  commit too if it differs, when it sets `Complete`.
- `web/index.html` is dependency-free and embedded via `include_str!` —
  no build step, keep it that way (MNG-5 hardens this into a contract).
  Since MFE-4 it is served at `/vanilla`; `/` is the Leptos app, and
  falls back to this page when `frontend/dist` holds no build.
- Wire protocol notes (envelope, `menger.call` subscription) are in
  README.md — `tests/wire.rs` is the executable reference.
- Delegation (owner, 2026-09-06): implementation subagents run on Opus 5
  (`model: opus`); worktree isolation; one commit per ticket.
