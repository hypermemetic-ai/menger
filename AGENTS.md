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
deploy/pipeline build          # master's tip → gate (cargo test --workspace + frontend tests) → release
deploy/pipeline deploy test    # the release → test, on a fresh copy of prod's data; smoke checks
deploy/pipeline promote        # the release test passed → prod; smoke; automatic rollback on failure
deploy/pipeline status         # what each env runs and whether it is healthy
```

- **Production changes only through `promote`**, and only to a release that
  passed the test environment. A `cargo build` or `trunk build` in a checkout
  changes nothing that is serving.
- Merging to master is not deploying: a ticket is `Complete` at the merge,
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
  `impl` in `tickets-api/` (MFE-11). Compile-time and Rust-only: the
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
  `~/.local/bin` (2026-10-02); neither has been run against menger.
- **A new language target** is a new `hub-codegen` generator over the same
  IR — never a hand-written client that mirrors the wire.
- **REST + SSE** (:4457) — for curl and for scripts that cannot run
  synapse. Unary responses are capped at 10,000 items (TRANSPORT-2).

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
  `extents` and one board window — no walk). The REST bridge and the
  `/rest` proxy stay for curl, scripts and non-Rust clients. `frontend/src`
  must contain no `/rest/`, no hand-parsed SSE, and no `serde_json::Value`
  on a wire path. Both halves are on the plexus 0.6 line (MFE-10), and the
  client the app uses is emitted from the server's own activation (MFE-11).
- Every RPC method returns `impl Stream<Item = TicketsEvent> + Send + 'static`;
  event enums use `#[serde(tag = "type", rename_all = "snake_case")]` with an
  in-band `Error { message }` variant.
- menger is DB-first: the database is the system of record — no plans/
  directory integration, no file-based ticket store. The status
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
- Architecture (owner-pinned): a service-wide in-memory cache
  (`src/store.rs`) is the ONLY interface the application has to tickets.
  Reads hit the cache, never storage; reads are async-safe (no lock
  across an await, no I/O on the read path); writes go through the cache
  write-through to the database. Don't hand any other module a database
  handle.
- A ticket's outputs are tickets (owner, 2026-09-06): a report, spike
  result, checkpoint, or evaluation memo is filed as a CHILD ticket of
  the ticket that generated it (`parent:` in frontmatter; typed once
  AGS-19 lands), not as a file under `plans/*/artifacts/`. Files there
  are history and attachments (raw data, screenshots) only.
- Planning: `plans/MNG/` per the ticketing skill. Tickets are `Pending`
  until the human promotes them to `Ready`; implementation does not begin
  on `Pending` tickets.
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
