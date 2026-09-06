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
- Workers move their ticket across the SDLC (owner, 2026-09-06). A
  ticket worker flips `Ready → Coding` when it starts, `Coding → In
  Review` in the commit that finishes the work (the commit message ends
  `— In Review`, not `— Complete`), and the **merge** flips `In Review →
  Complete`, so the merge hash is the completion hash. Each flip is the
  ticket's frontmatter in the worktree plus `tickets.set_status` on the
  running server, so the board shows where the work is. Until SDLC-7
  lands the two states in the server's `Status` enum, write them in
  frontmatter only — `set_status` rejects them today — and the merge
  still sets `Complete`.
- `web/index.html` is dependency-free and embedded via `include_str!` —
  no build step, keep it that way (MNG-5 hardens this into a contract).
- Wire protocol notes (envelope, `menger.call` subscription) are in
  README.md — `tests/wire.rs` is the executable reference.
- Delegation (owner, 2026-09-06): implementation subagents run on Opus 5
  (`model: opus`); worktree isolation; one commit per ticket.
