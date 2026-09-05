# menger 0.05

A plan is a program whose leaves are prose.

0.05 moves menger onto Plexus RPC and Rust. The 0.04 rule carries over:
**frontend and backend are online from the first commit** — every noun added
is visible the day it is written. What's new is *how* the frontend stays
current: 0.04 polled `/rev`; 0.05 subscribes. The same typed event streams
that answer RPC calls push every mutation to the browser as it happens —
over WebSocket for the vanilla page, over REST + SSE for the Leptos app.

## Run

```sh
cargo run                     # WebSocket :4455, web shell http://127.0.0.1:4456/,
                              # REST :4457, Leptos app http://127.0.0.1:4456/app/
cargo run -- --stdio          # MCP-compatible stdio mode
cargo test
```

Open the shell in two browser windows; create a ticket or flip a status in
one and watch the other update. No polling — the server pushes.

## Frontend (the Leptos app)

The MFE epic's browser client lives in `frontend/` — a **separate cargo
unit** (Leptos 0.8, CSR only, built with trunk). The server never needs
it to compile: `cargo build` / `cargo test` work with no wasm toolchain
installed, and `http://127.0.0.1:4456/app/` answers a plain "frontend
not built" page (HTTP 200 with build instructions) until you build it.

One-time toolchain (steps that worked on this machine):

```sh
rustup target add wasm32-unknown-unknown
# trunk, prebuilt via cargo-binstall — preferred here because
# `cargo install trunk --locked` fails to compile on GCC 16
# (libdeflate-sys 1.23 uses the removed `evex512` target attribute):
curl -L --proto '=https' --tlsv1.2 -sSf \
  https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
cargo binstall -y trunk
```

Build and run:

```sh
cd frontend && trunk build --release   # production build -> frontend/dist/
cargo run                              # serves it at http://127.0.0.1:4456/app/
```

Dev loop: keep `cargo run` up and run `cd frontend && trunk watch` — it
rebuilds `dist/` on every source change; reload the browser to pick it
up (the server reads `dist/` from disk per request, no restart needed).
`--app-dist DIR` / `MENGER_APP_DIST` point the server at a `dist`
elsewhere.

Data plane (MFE-1 pins: REST for calls, SSE for `watch`; the view layer
owns no routing):

- The REST port (:4457) emits **no CORS headers**, so the app never
  calls it cross-origin. Instead the shell server hosts a same-origin
  reverse proxy: `/rest/{*}` on :4456 forwards verbatim to :4457. The
  REST surface stays the contract; the proxy adds no data path — its one
  liberty is prepending an ignorable SSE comment line (`:`) to
  `text/event-stream` responses, because Firefox does not resolve a
  `fetch()` until the first body bytes arrive and `watch` sends no
  preamble of its own.
- `tickets.watch` streams SSE **over POST**, which browser `EventSource`
  (GET-only) cannot consume. The app uses the MFE-2 pinned fallback:
  `fetch` + ReadableStream, parsing `event:`/`data:` frames by hand
  (`frontend/src/net.rs`), with a ~1 s reconnect-and-resync loop — a
  server restart returns the page to live on its own.

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
  store.rs             the service-wide cache (SQLite system of record behind it)
  shell.rs             / (embedded vanilla page), /app/ (Leptos dist from
                       disk), /rest/* (same-origin proxy to the REST port)
  activations/
    tickets/           types.rs (events) + activation.rs (#[plexus_macros::activation])
web/index.html         the day-one reactive shell
frontend/              the Leptos CSR app (own cargo unit; trunk build → dist/)
plans/MNG/             the backend epic; plans/MFE/ the frontend epic
```

## Status

Phase one of the backend epic is complete — see
`plans/MNG/artifacts/CHECKPOINT.md` for the verified state map. Tickets
live in SQLite behind the service-wide cache (MNG-2); the DAG answers
ready-set/blockers queries with cycle rejection at create (MNG-4); REST +
SSE serve on :4457 via plexus-transport 0.2.3 (MNG-8 — consumed as a
git-tag patch until it's published to crates.io). The Leptos app at
`/app/` is the live board (MFE-3): every ticket as a row with a per-row
status control, pushed over SSE — read-and-status only, no creation
form by design (tickets are created by agents/CLI/API). Parked as Idea:
MNG-5/6 (schema-derivation and framework theses). Next: the default
page swap and the ready-set view (`plans/MFE/`), and the HelixDB
graph-store epic (`plans/HLX/`, spike-gated).

## History

`menger-hs` (0.04, Haskell) is the ancestor; its `web/` window manager —
tabs, tear-out, desks, fuzzy find — is the eventual consumer of the
framework this repo grows.

## License

MIT
