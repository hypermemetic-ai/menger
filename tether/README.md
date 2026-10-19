# menger tether

The local side of menger (TETHER-1) as a Plexus RPC tool of its own:
`synapse tether …`. substrate is a library inside it — the
`plexus-substrate` crate, its hub built in this process
(`build_plexus_rpc_with`) and called with substrate's own
`ClaudeCodeClient` over plexus-core's in-memory `HubTransport` (LOCAL-6).
No substrate process, no port between them.

Its activations: `sessions` (TETHER-7) — agent sessions on this machine,
each a `claudecode` session in the substrate inside tether, working a
ticket; `runs` (TETHER-9) — a brief handed to an archetype, its report
posted to a chat; `listen` (TETHER-9) — what tether hears on the remote
menger and acts on.

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
cargo test                    # tests/wire.rs (the socket, signed in), tests/engine.rs (substrate in memory),
                              # tests/listen.rs (a throwaway menger + a stub Claude; builds menger's binary
                              # unless TETHER_TEST_MENGER names one)
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
synapse -P 4486 tether sessions say --name menger-12-1a2b3c4d --prompt "…"   # the next turn (queued behind a running one)
synapse -P 4486 tether sessions stop --name menger-12-1a2b3c4d      # the turn and its Claude Code end
```

Roles: `viewer` lists and watches; `editor` starts and stops; every
signed-in viewer sees every session on the machine (first slice). Inside,
tether is `apikey:menger-tether` to substrate, `editor` there and nothing
more: claudecode sessions of its own; `bash` and the rest stay admin's.

## The remote: listening (TETHER-9)

With `--remote wss://app.menger.sh/ws` tether signs in to that menger as
`--remote-as` (default `builder`) with the key synapse keeps,
`~/.plexus/keys/<name>.key` (0600; `--remote-key FILE` names another) —
a fresh key proof on each connection, never a token in argv or the
environment. It holds `tickets watch` and `comments watch` there,
reconnecting (1 s → 60 s) and catching up on what it missed, and acts on:

- **an input answered** — by a person (source `app`); through the API too
  with `--accept-keys`. When a session of tether's asked it (an input
  that appeared on its ticket while it ran), that session is resumed with
  the answer. Otherwise tether waits `--grace-secs` (600) for whoever
  asked — the bridge, a hand-launched run — to act and close it, reads it
  again, and only if it is still answered the same starts a session on the
  ticket (the builder archetype's model), told to check first that no one
  is already on it.
- **a change request** (the tray's *Request changes*) — resumes the
  ticket's session, else starts one, as an answer does.
- **a comment** on a ticket a session of tether's holds, or one that is
  queued — resumes or starts as above. Its own comments, and keys'
  without `--accept-keys`, are not heard.

`synapse -P 4486 tether listen status` says the remote, whether it is
connected, where it is (`since`, its comment mark), what it heard and
acted on, what waits out its grace, and how many tickets are queued.
`--no-listen` keeps the remote for posting reports only.

## Runs (TETHER-9)

A run replaces a hand-launched `systemd-run … claude -p`: a brief, an
archetype, a ticket, and a chat its report goes to when the turn ends.

```sh
synapse -P 4486 tether runs start --ticket TETHER-9 --brief @brief.md --archetype builder --chat 6
synapse -P 4486 tether runs list                       # runs only (sessions list has every session)
synapse -P 4486 tether runs watch --name tether-9-…    # as sessions watch
synapse -P 4486 tether runs stop --name tether-9-…
```

Archetypes `architect`, `builder` (Opus) and `analyst` (Sonnet) take their
model from `~/.config/menger/archetypes.env` (`MENGER_MODEL_BUILDER=…`;
`--archetypes FILE`) when it names one; `--model` overrides. Sessions run
Claude Code with `--permission-mode` when given (a wrapper under
`--data-dir`, since substrate passes no mode). The report is the text
after the turn's last tool call, posted as the remote agent.

## Kept across restarts (TETHER-9)

`<data-dir>/state.json` (0600, written whole and renamed) keeps every
session — its claudecode name, ticket, origin, turns, the inputs it asked,
its report — the prompts waiting for its next turn, and the listener's
place on each remote: `since`, the comment mark and what it has handled.
A session running when tether stopped comes back `interrupted` and is
resumed by the next answer to it; what was answered while tether was down
is acted on once, after the restart. Tails are not kept.

## Install as a user service (the owner)

`deploy/tether.service` is the `systemd --user` unit, running the
installed `~/.local/bin/tether` with `~/.config/menger/tether.env`
(`deploy/tether.env.example`: port, admins, remote, key name, work dir,
Claude, permission mode, grace — no secret).

```sh
tether/deploy/install.sh            # build --release, install binary + unit, make tether.env once, daemon-reload
$EDITOR ~/.config/menger/tether.env # check TETHER_ADMINS, the remote, the work dir
tether/deploy/install.sh --start    # enable --now (restart when running)
journalctl --user -u tether -f
synapse -P 4486 _login tether --key builder --as builder   # once
synapse -P 4486 tether listen status
systemctl --user disable --now tether   # stop it for good
```

Run it from main to update. While the chat bridge (`menger-chat-bridge`)
also runs, both hear answers: the grace is what keeps tether off what the
bridge's sessions asked.
