# menger 0.05

A plan is a program whose leaves are prose.

0.05 moves menger onto Plexus RPC and Rust. The 0.04 rule carries over:
**frontend and backend are online from the first commit** — every noun added
is visible the day it is written. What's new is *how* the frontend stays
current: 0.04 polled `/rev`; 0.05 subscribes. The same typed event streams
that answer RPC calls push every mutation to the browser as it happens. That
subscription loop is the seed of a Plexus-native reactive web framework
(see `plans/MNG/`), with tickets as its first application.

## Run

```sh
cargo run                     # WebSocket :4455, web shell http://127.0.0.1:4456/
cargo run -- --stdio          # MCP-compatible stdio mode
cargo test
```

Open the shell in two browser windows; create a ticket or flip a status in
one and watch the other update. No polling — the server pushes.

## Surface

menger is DB-first: the database is the system of record, and the service
is CRUD over tickets plus a live subscription. One activation so far,
`tickets` (statuses and types use the house ticketing vocabulary):

| method | yields |
|---|---|
| `tickets.health` | `health { version, unix_now, ticket_count }` |
| `tickets.create { epic, title, ticket_type?, severity?, blocked_by?, body? }` | `created { ticket }` — id is the next `EPIC-N`, status starts `Pending` |
| `tickets.get { id }` | `ticket` or `not_found` |
| `tickets.list { epic?, status? }` | one `ticket` per match, ordered (epic, seq) |
| `tickets.set_status { id, status }` | `status_changed { ticket, previous }` |
| `tickets.watch { epic? }` | open-ended stream of `created` / `status_changed` |

Via synapse (once on PATH — 4455 is inside the `_self scan` range):

```sh
synapse -P 4455 menger tickets health
synapse -P 4455 menger tickets create --epic MNG --title "..."
synapse -P 4455 menger tickets watch
```

Raw wire: JSON-RPC 2.0 over WebSocket. Every logical call is a
`menger.call` subscription with `{method: "tickets.watch", params: {...}}`;
events arrive as `result` notifications carrying `PlexusStreamItem`s.
`web/index.html` is a complete, dependency-free client.

## Layout

```
src/
  main.rs              CLI args, transports, spawns the shell
  builder.rs           build_rpc() → Arc<DynamicHub> ("menger")
  store.rs             domain types + in-memory store + change broadcast
  shell.rs             serves web/index.html (axum, embedded at compile time)
  activations/
    tickets/           types.rs (events) + activation.rs (#[plexus_macros::activation])
web/index.html         the day-one reactive shell
plans/MNG/             the epic: persistence, plans/ interop, DAG queries, framework
```

## Status

The store is an in-memory placeholder — a restart forgets everything.
The database (MNG-2), ready-set DAG queries (MNG-4), the schema-derived
shell (MNG-5), and the framework spike (MNG-6) are ticketed in
`plans/MNG/` (Pending, awaiting promotion).

## History

`menger-hs` (0.04, Haskell) is the ancestor; its `web/` window manager —
tabs, tear-out, desks, fuzzy find — is the eventual consumer of the
framework this repo grows.

## License

MIT
