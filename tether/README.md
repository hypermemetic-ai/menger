# menger tether

The local side of menger (TETHER-1) as a Plexus RPC tool of its own:
`synapse tether …`. substrate is a library inside it — the
`plexus-substrate` crate, its hub built in this process
(`build_plexus_rpc_with`) and called with substrate's own
`ClaudeCodeClient` over plexus-core's in-memory `HubTransport` (LOCAL-6).
No substrate process, no port between them.

Its first activation is `sessions` (TETHER-7): `start`, `stop`, `list`,
`watch` — agent sessions on this machine, each a `claudecode` session in
the substrate inside tether, working a ticket.

## Its own workspace

`tether/` is not a member of menger's workspace and has its own
`Cargo.lock`: menger's `rusqlite 0.37` links `libsqlite3-sys 0.35`,
substrate's `sqlx 0.8` links `libsqlite3-sys 0.30`, and cargo refuses two
`links = "sqlite3"` in one resolve. `menger-tickets-api` with
`default-features = false` (the client half, for the remote — TETHER-2)
resolves beside substrate. It needs the sibling checkouts menger does
(`../../plexus-core`, `../../plexus-substrate`, …); `plexus-login` is named
`../../menger/plexus-login`, the path substrate names, so the graph has one.

```sh
cd tether
cargo test                    # tests/wire.rs (the socket, signed in), tests/engine.rs (substrate in memory)
cargo run -- --port 4486 --auth-provider key --admin "$(synapse _key show builder)" \
  --data-dir /tmp/tether-dev --work-dir ~/dev --claude-path /path/to/claude
```

## Run and reach it

There is no open mode: `--auth-provider key` (agents' keys) at least, and
`--admin` for you. substrate's databases go under `--data-dir`
(`~/.local/share/menger-tether/substrate` by default) — never
`~/.plexus/substrate`, so a substrate beside it keeps its own.

```sh
synapse -P 4486 _login tether --key builder --as builder
synapse -P 4486 tether sessions start --ticket MENGER-12 --prompt "…" --model sonnet
synapse -P 4486 tether sessions list
synapse -P 4486 tether sessions watch --name menger-12-1a2b3c4d     # the tail, then each line, then ended
synapse -P 4486 tether sessions stop --name menger-12-1a2b3c4d      # the turn and its Claude Code end
```

Roles: `viewer` lists and watches; `editor` starts and stops; every
signed-in viewer sees every session on the machine (first slice). Inside,
tether is `apikey:menger-tether` to substrate, `editor` there and nothing
more: claudecode sessions of its own; `bash` and the rest stay admin's.
