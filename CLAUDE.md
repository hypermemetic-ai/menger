# menger

A plan is a program whose leaves are prose. Plexus RPC ticketing backend +
the seed of a Plexus-native reactive web framework. Successor to
`../menger-hs` (0.04, Haskell); its rule carries over: frontend and backend
online from the first commit — every noun visible the day it is written.

## Commands

```sh
cargo run          # WS :4455, shell http://127.0.0.1:4456/
cargo test         # unit tests + wire tests (tests/wire.rs speaks the real protocol)
```

## Conventions

- Plexus crates come from crates.io (`plexus-core`/`plexus-macros` 0.5,
  `plexus-transport` 0.2) — same pins as `../hypermemetic-whoami` and
  `../hypermemetic-runs`, which are the reference apps for the macro API
  (`#[plexus_macros::activation]` + `#[plexus_macros::method]`,
  `crate_path = "plexus_core"`).
- The ticket vocabulary lives ONCE, in `tickets-api/`
  (`menger-tickets-api`): `Ticket`, `Status`, `Kind`, `Severity`,
  `TicketSummary`, `TicketsEvent`. `src/store.rs` and the tickets
  activation re-export them, so `crate::store::Status` still resolves;
  the browser app depends on the same crate, which is what makes a
  renamed server field a wasm compile error rather than a blank cell
  (MFE-9). Don't re-declare a wire type in `frontend/src`.
- The browser app's data plane is the Plexus WebSocket, not the REST
  bridge (MFE-9): one `WsTransport` for the app's lifetime, the typed
  client from `tickets-api`'s `client` feature, `watch` opened before
  MNG-9's `list` page walk. The REST bridge and the `/rest` proxy stay
  for curl, scripts and non-Rust clients. `frontend/src` must contain no
  `/rest/`, no hand-parsed SSE, and no `serde_json::Value` on a wire path.
  The `client` feature path-depends on the **plexus 0.6** worktrees
  (`../plexus-macros-macro-6`), which the server is deliberately NOT on —
  MFE-10 is the bump, MFE-11 collapses the two activation declarations
  into one.
- Every RPC method returns `impl Stream<Item = TicketsEvent> + Send + 'static`;
  event enums use `#[serde(tag = "type", rename_all = "snake_case")]` with an
  in-band `Error { message }` variant.
- menger is DB-first: the database is the system of record — no plans/
  directory integration, no file-based ticket store. The status
  vocabulary matches `skills/skills/ticketing/SKILL.md` (`Pending`, …);
  the kind vocabulary is the owner's closed five (AGS-13): `scope`,
  `execution`, `build`, `gap`, `spike`. Don't invent a second vocabulary
  for either.
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
  double-claimed) and record the branch on the ticket; flip to `in
  review` in the commit that finishes the work — gate green, committed,
  change linked (the commit message ends `— in review`); the **merge**
  (the pipeline) lands it to `done`, so the merge hash is the completion
  hash. Each flip is the ticket's frontmatter in the worktree plus
  `tickets.set_status` on the running server, so the board shows where
  the work is. Until SDLC-7 lands the two worker states in the server's
  `Status` enum, write them in frontmatter only — `set_status` rejects
  them today — and the merge still sets `Complete` (menger's current
  name for `done`; SDLC-2 reconciles the vocabulary with ought's).
- `web/index.html` is dependency-free and embedded via `include_str!` —
  no build step, keep it that way (MNG-5 hardens this into a contract).
- Wire protocol notes (envelope, `menger.call` subscription) are in
  README.md — `tests/wire.rs` is the executable reference.
- Delegation (owner, 2026-09-06): implementation subagents run on Opus 5
  (`model: opus`); worktree isolation; one commit per ticket.
