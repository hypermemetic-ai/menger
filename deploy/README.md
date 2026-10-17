# Deploying menger

menger runs in two environments on this machine, **prod** and **test**, both
under `systemd --user` (lingering is on: they start at boot and restart on
failure). Production changes only through this pipeline (DEPLOY-1).

```
commit on main → build → gate → release → deploy test → smoke → promote → smoke → live
                           ↓ red            (prod data          ↓ red              ↓ red
                       no release            snapshot)     prod untouched     automatic rollback
```

## The environments

| | prod | test |
|---|---|---|
| menger WebSocket / app / REST | 4455 / 4456 / 4457 | 4450 / 4451 / 4452 |
| diagrams | 4458 | 4453 |
| app | http://127.0.0.1:4456/ | http://127.0.0.1:4451/ |
| database | `~/.local/share/menger/tickets.db` | `~/.local/share/menger-test/tickets.db` — prod's, copied at each deploy |
| units | `menger@prod`, `menger-diagrams@prod` | `menger@test`, `menger-diagrams@test` |
| settings | `~/.config/menger/prod.env` + `prod.local.env` | `~/.config/menger/test.env` (`PLEXUS_NO_REGISTRY=1`) + `test.local.env` + `test.smoke.env` |
| login the smoke requires | `MENGER_EXPECT_LOGIN=required` (since ACCESS-47, 2026-10-17) | `MENGER_EXPECT_LOGIN=required` |

## Settings (ACCESS-27)

An environment's settings are three files in `~/.config/menger/`; the names
are stable (ACCESS-24 and ACCESS-7 read them):

| file | holds | written by | mode | read by |
|---|---|---|---|---|
| `<env>.env` | ports, database, registry, `MENGER_EXPECT_LOGIN` — `deploy/env/<env>.env` filled in | **every** `pipeline install` | 0600 | both units; the pipeline |
| `<env>.local.env` | menger's login and secrets: `MENGER_AUTH_PROVIDERS`, `MENGER_REQUIRE_LOGIN`, `MENGER_NO_REST`, `MENGER_ADMINS`, `MENGER_LOGIN_*`, `MENGER_DEV_*`, `MENGER_PUBLIC_URL` … | you, by hand (made once by `install`) | 0600 | `menger@<env>` (after `<env>.env`, so it wins); the smoke reads it, never exports it |
| `<env>.smoke.env` | the smoke account: `MENGER_SMOKE_USER`, `MENGER_SMOKE_PASSWORD`, `MENGER_SMOKE_TOKEN_URL`, `MENGER_SMOKE_AUDIENCE` | you, by hand (made once by `install`) | 0600 | the smoke alone — never in menger's environment |

- **`install` never rewrites the local halves.** Where `<env>.local.env` does
  not exist, it is made from what the old `<env>.env` held beyond its
  template — each setting verbatim, the notes beside it — the
  `MENGER_SMOKE_*` going to `<env>.smoke.env`, and the old file kept as
  `<env>.env.before-local` (0600). It says what moved by name, never a value.
  A second `install` leaves both alone. If `<env>.env` has since gained a
  setting its template lacks, `install` refuses and changes nothing, naming
  it: move it into the local half (ACCESS-4 H3 was an install silently
  dropping test's login).
- **`menger@<env>` requires its local half**: without the file the unit does
  not start — never starts without its login. `menger-diagrams@<env>` reads
  only `<env>.env`.
- **The mode the smoke requires** is `MENGER_EXPECT_LOGIN` (`off`, `on` or
  `required`) in the templated half, so losing the local half cannot also
  lose the expectation. The smoke fails when the settings configure another
  mode, and when the server is in another (`deploy/smoke-mode.mjs`, below).
- **No secret in argv.** The smoke's password reaches curl on stdin, its
  token reaches `deploy/smoke-health.mjs` on stdin (`-`), the browser check
  gets the account through its environment; the smoke fails if
  `<env>.local.env` or `<env>.smoke.env` is readable by anyone but you.

## The pipeline

```sh
deploy/pipeline install              # once, and after a change to deploy/: units + env files, enable both envs
deploy/pipeline build                # main's tip → gate → release (prints its name)
deploy/pipeline deploy test          # the newest release → test, with a fresh copy of prod's data
deploy/pipeline sync test            # test's data refreshed from prod; test keeps its release
deploy/pipeline promote              # the release test runs → prod
deploy/pipeline status               # what each env runs, whether it is up, its health, its last record
deploy/pipeline backfill             # deployments.log → menger's deployments (idempotent)
deploy/pipeline landing              # main's landing/ → gate → the edge (menger.sh); recorded as env `edge`
deploy/pipeline landing --check      # the gate and infra's dry run, nothing written
deploy/pipeline media                # landing/media regenerated from the release prod runs
deploy/pipeline image [RELEASE]      # RUN-10: a TESTED release as an image (menger:<release> here; IMAGE_STORE=ghcr pushes)
deploy/pipeline local --image IMG    # RUN-25: the image here as menger-app-local, 127.0.0.1:4470/4471
deploy/pipeline remote test|prod     # RUN-25: app-test / app.menger.sh through infra — refused until RUN-11
```

1. **Build** (`build [REV]`, default `main`). A commit, never a working
   tree: `../menger-build` — a sibling checkout, so menger's relative plexus
   dependencies resolve — is moved to the commit and cleaned, keeping only
   build outputs.
2. **Gate.** `cargo test --workspace` and the frontend's native tests at that
   commit. Red: no release.
3. **Release.** `cargo build --release` (menger, menger-diagrams), `trunk
   build --release`, `npm ci` for the diagrams Node program, assembled into
   `~/.local/share/menger-deploy/releases/<UTC time>-<sha>/` — `bin/` (the two
   binaries and their launchers), `app/`, `diagrams-node/`, and `RELEASE`
   (commit, subject, build time, toolchain, gate results). Read-only; never
   rebuilt. Prod runs exactly the bytes test ran. The newest ten, and any an
   environment runs or can roll back to, are kept.
4. **Deploy to test** (`deploy test [RELEASE]`). Test stops, its database is
   replaced by a live snapshot of prod's (every data upgrade is rehearsed on
   real data before prod sees it), test switches to the release, restarts and
   must pass the smoke checks. Passing marks the release `TESTED`.
5. **Promote** (`promote [RELEASE]`, default: what test runs). Only a
   `TESTED` release. Prod's database is backed up to
   `~/.local/share/menger/backups/`, prod switches (the release it ran becomes
   `previous`), restarts and must pass the smoke checks. A failure stops prod,
   restores that backup, switches back and restarts — by itself.
**Sync** (`sync test`, DEPLOY-4). Test's data refreshed from prod on demand,
between deploys: test stops, its current database is backed up to test's
`backups/` (`…-before-sync`, the newest five kept), and it becomes a live
snapshot of prod's — the same `.backup` step 4 takes — then test restarts on
the release it already runs and must pass the smoke checks. One way only:
`sync prod` is refused, and a sync never stops or writes prod.

6. **Rollback** (`rollback ENV [--restore BACKUP]`). Back to `previous`. A
   data upgrade can be one-way — an older build cannot always read what a
   newer one wrote (SDLC-11's docs did that) — so a rollback across one needs
   `--restore` with the backup taken before the promote.

**Smoke checks** (`smoke ENV`): both units active; the local half is
there and 0600 (the smoke account's file too); the login mode — the settings
configure `MENGER_EXPECT_LOGIN`'s, and the server is in it, as
`deploy/smoke-mode.mjs` reads it from outside (`off`: an anonymous `_info`
is the whole hub; `on`: it is AUTH-7's anonymous view; `required`: that, and
the app behind its login page), a mismatch failing the smoke; then the
checks for that mode (behind the login: the smoke account signs in, and an
anonymous socket reaches nothing); `tickets.health` answers on
the env's port; `--verify-derived` is consistent; the app is served stamped
with the env's WebSocket port; `diagrams` parses a diagram; a headless browser
loads the app to `live · WS` with the nav drawn and no page errors, its page
under its Content-Security-Policy and nothing refused by one (ACCESS-28); every port
(WebSocket, shell, REST unless `MENGER_NO_REST`, diagrams) refuses a request
naming `Host: evil.example` with 421 (ACCESS-25; skipped for a release whose
menger predates `--allowed-host`, so a rollback to one still passes). On test,
behind the login with the dev sign-in, `deploy/probe-auth.mjs` then plays the
login's known attacks against the live server (IDENTITY-38): foreign and
anonymous sockets, framing, reads and writes across shortcodes, a blocker in a
shortcode the caller cannot see, a watch after its grant is revoked, sign-out —
any that works fails the smoke. It writes probe tickets and a grant to test's
database, which the next `deploy test` or `sync test` replaces. Run it alone:
`node deploy/probe-auth.mjs 4450 4451`.

**Drills.** `DEPLOY_SMOKE_FAIL=test deploy/pipeline promote --to test` fails
the first smoke on purpose, so the automatic rollback can be watched on test.

### The landing page (DEPLOY-17)

menger.sh is `landing/` at one commit, pinned in hypermemetic-infra
(`menger_landing_commit` in `inventory/group_vars/all/main.yml`) and read by
the edge role from this checkout with `git show` and `git archive` — never a
working tree (SCAFFOLD-19). `deploy/pipeline landing [REV]` (default `main`)
takes a commit there in one run:

1. **Export.** `landing/` at the commit, into a scratch directory.
2. **Gate.** `main.js` parses; neither page carries an inline style or
   script (the CSP allows none); every file a page names on its own origin —
   stills, clips, fonts, the grain — is in the commit; `deploy/landing-check.mjs`
   serves the export and loads `/` and `/guide/` in Chromium at 390 and 1440:
   no console error, page error, failed request, answer of 400 or more,
   horizontal scroll or missing title.
3. **Pin and apply.** The pin set to the commit, `bin/infra edge.yml` run with
   this checkout as infra's menger source. The role validates, writes, reloads,
   and on a failed check puts the old files back by itself; then the pipeline
   restores the pin, records `failed` and exits 1.
4. **Verify.** `https://menger.sh/` (`MENGER_LANDING_URL`; `none` skips it)
   must serve the commit's `index.html` byte for byte.
5. **Commit and record.** The pin committed in infra (`menger.sh at <sha> —
   <subject> (deploy/pipeline landing)`); one line in `deployments.log`
   (`edge deploy landing-<when>-<sha>`); `started` then `succeeded` or
   `failed` sent to menger as project `menger`, environment `edge`, the
   previous pin as `previous`, the commits between from `git log`. The app
   shows it at `#/deployments` like any other environment.

`landing --check` runs the gate and infra's dry run (`edge.yml --check
--diff`, the commit as an extra var) and writes nothing. `status` adds an
`edge` block: the pin, how many commits on main touch `landing/` since it,
whether the live page answers, the last record sent.

**Media.** The stills and clips are recorded from the real app, so they go
stale when the app's look changes. `deploy/pipeline media [RELEASE]` runs
`scripts/landing-media.sh` on a release's own binary and app (default: what
prod runs — the pictures show exactly what prod serves) and lists the changed
files under `landing/media/`; commit them, merge, then `landing`. Infra is
found at `MENGER_INFRA_DIR` (default `../hypermemetic-infra`).

**The mock** (DESIGN-58). Before a change to the page goes live, the page is
shown on the tracker as one document — DESIGN-49, `view: mockup` at 390 and
1440 — made by `scripts/landing-mock.py [landing] OUT.html` (styles, fonts,
media and script inlined) and put in the doc's body through
`deploy/menger-call.mjs` (its usage says how). The whole way a change to
menger.sh is made, shown, asked and shipped is the doc under DESIGN-49.

**Link previews** (DESIGN-52, DESIGN-53). A link to menger.sh unfurls from
the tags in each page's `<head>`: `og:*` (title, description, url, site
name, image with its type, size and alt) and `twitter:card
summary_large_image` with `twitter:title`, `twitter:description` and
`twitter:image` — X reads its own tags first. Every image URL is absolute
(`https://menger.sh/media/og.jpg`). The image is the page's foot under the
promise, 1200×630 JPEG (keep it under 1 MB; X takes 300×157 to 5 MB, no
SVG, no animation), made from the page itself:

```sh
PLAYWRIGHT_FROM=perf node scripts/landing-og.mjs   # → landing/media/og.jpg; commit it, then landing
```

Rerun it when the foot, the promise or the look changes. After `landing`,
check what a crawler gets, from here and from outside:

```sh
for ua in Twitterbot/1.0 facebookexternalhit/1.1 Slackbot-LinkExpanding Discordbot/2.0 LinkedInBot/1.0; do
  curl -s -A "$ua" https://menger.sh/ | grep -c 'og:image\|twitter:image'          # the tags
  curl -s -A "$ua" -o /dev/null -w '%{http_code} %{content_type}\n' https://menger.sh/media/og.jpg
done
curl -s 'https://api.microlink.io/?url=https%3A%2F%2Fmenger.sh%2F' | jq .data   # a fetch from another cloud
echo | openssl s_client -connect menger.sh:443 -servername menger.sh -showcerts 2>/dev/null | grep -E '^ *[0-9] s:|^ *i:'
```

The chain must be RSA to ISRG Root X1 (`EE ← YR1|YR2 ← Root YR ← ISRG Root
X1`): `landing/menger.sh.caddy` asks for `key_type rsa2048`. Left to its
default, Caddy takes ECDSA, whose chain (`YE1 ← Root YE ← X2 ← X1`) X's
crawler did not follow — every other fetcher showed the card, X showed none.
Caddy applies `key_type` only when it next issues and keeps serving the
certificate it holds, so the type is also declared in hypermemetic-infra
(`edge_cert_keys`, DESIGN-54): a `landing` run whose name answers with
another type moves the stored certificate aside (kept under
`/data/edge-kept/`), recreates the edge, and checks the new one — `landing
--check` says `Would re-key: menger.sh (ec → rsa)`.

A preview is cached by the app that drew it, per URL: X about a week, Slack,
Discord and LinkedIn hours to days. To see a change at once, share a URL it
has not seen (`https://menger.sh/?v=3`); X has no validator any more, and its
composer often draws no card until the post is published. The edge logs
every request to menger.sh (Caddy `log`, the edge container's output), so
whether a crawler came, and what it was answered, can be read there.

## The image (RUN-10)

menger as a container image, made from a release test passed — the very
bytes `deploy test` ran, never a build from a checkout. `deploy/pipeline
image [RELEASE]` stages the release's `bin/menger`, `bin/run-menger` (its
own launcher), `app/` and `RELEASE` beside `deploy/image/Dockerfile` and
builds it; RUN-25's `build` stage is this command.

```sh
deploy/pipeline image                      # the release test last passed → menger:<release> in this machine's Docker
deploy/pipeline image 20261017T054109Z-9501b9f   # a named one (it must be TESTED)
IMAGE_STORE=ghcr deploy/pipeline image     # ghcr.io/hypermemetic-ai/menger:<release> and :latest, pushed
```

- **Which release.** Without one named, the release `deployments.log` last
  shows test passing (a `deploy` or `sync` of test that ended `ok`) whose
  directory is still here and marked `TESTED`; with no such line, the newest
  `TESTED` release. A named release that has not passed test is refused, and
  so is one whose menger predates `--bind` (RUN-8): bound to loopback inside
  a container, it could not be reached.
- **Where it is kept** is `IMAGE_STORE`: `local` (the default) tags
  `menger:<release>` (`MENGER_IMAGE_LOCAL` names the repository); `ghcr`
  tags `ghcr.io/hypermemetic-ai/menger:<release>` and `:latest`
  (`MENGER_IMAGE_GHCR`) and pushes both, Docker signed in with `gh auth
  token` on stdin (this machine's token has `write:packages`; without `gh`,
  Docker's own stored login). A new GHCR package is private: make it public
  once in GitHub's UI, or give the box a `read:packages` token
  (SCAFFOLD-19's rule).
- **What it answers**: `image=REF` and `release=NAME` on stdout, and the
  same lines appended to `$HF_OUTPUTS` when it is set (hyperforge's stage
  outputs). Each run is a line in `deployments.log` (`image  local|ghcr
  RELEASE  ok REF`, or `build failed` / `push failed`) — not a deployment:
  `backfill` and `deploy/record` pass it by.

**Inside the image**: `debian:bookworm-slim`, user `menger` (uid 10001),
`tini` as PID 1 (so `docker stop` is a SIGTERM menger heeds), menger bound
to `0.0.0.0` (`MENGER_BIND`), the WebSocket on 8080 and the shell on 8081
(`MENGER_WS_PORT`, `MENGER_SHELL_PORT`), the REST bridge off
(`MENGER_NO_REST=1`: it carries no identity), the database at
`/data/tickets.db` in the `/data` volume, `PLEXUS_NO_REGISTRY=1`. All of
menger's settings are its environment (`MENGER_*`, `--help` names each).
Binding `0.0.0.0` adds no name (ACCESS-25), so every name the container is
reached by is an `--allowed-host` (`MENGER_ALLOWED_HOSTS`) or
`MENGER_PUBLIC_URL`'s; the page dials the WebSocket on the shell's host at
`MENGER_WS_PORT`, so publish each port on the same number inside and out
until RUN-9 puts the socket behind the shell's origin:

```sh
docker volume create menger-scratch
docker run -d --name menger-try -v menger-scratch:/data \
  -e MENGER_WS_PORT=4475 -e MENGER_SHELL_PORT=4476 \
  -p 127.0.0.1:4475:4475 -p 127.0.0.1:4476:4476 menger:<release>
synapse -P 4475 menger tickets health           # Host 127.0.0.1:4475 — its own name
curl -s http://127.0.0.1:4476/ | grep menger-ws-port
docker rm -f menger-try && docker volume rm menger-scratch
```

**Diagrams are not in it** (decided in RUN-10). `menger-diagrams` and its
Node program are a checker for agents and the smoke: menger never calls it,
and it runs mermaid in a Node `vm`, which is no sandbox, with jsdom (~300 MB
resident). A box serving people does not need it; if one ever does, it is a
second image from the same release (`bin/menger-diagrams`,
`bin/run-diagrams`, `app/vendor/mermaid-*.min.js`, `diagrams-node/`, on a
Node base), never a second process in this one.

## The menger-app pipeline (RUN-25)

The image built, stored and deployed as a pipeline on hyperforge's `ci`
(HYPERFORGE-22; hyperforge's README "Pipelines (`ci`)"), declared in
`.hyperforge/pipelines.toml`. Each stage is a `deploy/pipeline` subcommand,
so the same step runs from a shell without hyperforge, and each deploy is
its own trigger, independent of the others:

| stage | trigger | needs | runs | outputs |
| -- | -- | -- | -- | -- |
| `build` | auto | — | `deploy/pipeline image` — the newest TESTED release as `menger:<release>` (`IMAGE_STORE`, the pipeline's `env`) | `image`, `release` |
| `deploy-local` | manual | build | `deploy/pipeline local --image "$IMAGE"` | `url` |
| `deploy-test` | manual | build | `deploy/pipeline remote test --image "$IMAGE"` | `url` |
| `deploy-prod` | manual | deploy-test | `deploy/pipeline remote prod` | `url` |

```sh
synapse -P 44104 lforge-v5 hyperforge ci add --path ~/dev/controlflow/hypermemetic/menger
synapse -P 44104 lforge-v5 hyperforge ci menger-app start                                 # build → run N
synapse -P 44104 lforge-v5 hyperforge ci menger-app get --run N                           # each stage's state and outputs
synapse -P 44104 lforge-v5 hyperforge ci menger-app trigger --run N --stage deploy-local
synapse -P 44104 lforge-v5 hyperforge ci menger-app log --run N --stage deploy-local
synapse -P 44104 lforge-v5 hyperforge ci menger-app trigger --run N --stage deploy-test   # once RUN-11
synapse -P 44104 lforge-v5 hyperforge ci menger-app trigger --run N --stage deploy-prod   # once RUN-11 / RUN-13
```

- **`local --image IMAGE`** (or `$IMAGE`) runs the image on this machine as
  the container `menger-app-local`, its database in the volume
  `menger-app-local-data`, the WebSocket on `127.0.0.1:4470` and the shell
  on `127.0.0.1:4471` (`MENGER_LOCAL_WS_PORT`, `MENGER_LOCAL_SHELL_PORT`;
  4450–4462 are refused — prod's, test's and a dev server's). Each port is
  the same number inside the container and out (`MENGER_WS_PORT` /
  `MENGER_SHELL_PORT` set in it), because until RUN-9 the page dials the
  WebSocket on the port it was stamped with. Login is off, as the image's
  defaults have it; nothing is reachable beyond loopback. It waits (90 s,
  `MENGER_LOCAL_WAIT`) for `tickets.health` over synapse on 4470 and the
  page on 4471 stamped with 4470, then answers `url=http://127.0.0.1:4471/`
  (and appends it to `$HF_OUTPUTS`). Run again, it removes the old
  container and starts the new one on the same volume, so the local app's
  tickets survive a new image. A container that does not answer is left
  running for `docker logs menger-app-local`, and the stage fails. No
  bind mount is used (SELinux is enforcing here; a volume needs no label).
  Each run is a line in `deployments.log` (`app-local  run  RELEASE  ok
  IMAGE`), not a deployment record.
- **`remote test --image IMAGE`** deploys to `app-test.menger.sh` through
  hypermemetic-infra's Plexus app role: `bin/infra app.yml -e
  app_dir=<the main checkout> -e env=test -e sha=<the release's commit>`.
  **`remote prod`** is `bin/infra promote.yml -e app_dir=…` only: prod takes
  the image test runs and passed, by infra's rule. Both are driven by
  menger's own `deploy/app.yml` (SCAFFOLD-12's contract), which RUN-11
  writes; **until it exists, each refuses before touching anything**:
  `app-test.menger.sh is not declared yet (RUN-11): menger has no
  deploy/app.yml for hypermemetic-infra to deploy — nothing was changed`.
  RUN-11 owns the rest of the contract (the image infra pulls is named by
  commit, `menger:<release>` here) and may change these two lines.
- **The stages' environment.** A stage inherits hyperforge's (its unit's
  `PATH` holds `~/.local/bin`, where `synapse` and `node` are, and
  `/usr/bin`, where `docker` is); each `run` line also puts
  `$HOME/.local/bin` and the system's bin first itself, since a pipeline
  `env` value is taken literally, never expanded. `IMAGE_STORE` is the
  pipeline's `env`: `ghcr` there makes `build` push and the deploys take the
  registry's ref — a change to the file, not to hyperforge.
- None of this touches the 4450–4458 environments: `deploy test` and
  `promote` are unchanged, and `build` reads the releases they made.

## Hardening (ACCESS-27)

Both units run with the least the OS gives a service, `systemd --user` as on
this machine (user namespaces do the mounts; no root):

| | `menger@` | `menger-diagrams@` — it runs mermaid in a Node `vm`, which is no sandbox |
|---|---|---|
| file system | `ProtectSystem=strict`, `ProtectHome=read-only`; writes only `ReadWritePaths=` the database's directory (drop-in `menger@<env>.service.d/50-data.conf`, written by `install`) | `ProtectHome=tmpfs`: no home but the releases and Node, read-only (`BindReadOnlyPaths=`, drop-in `…/50-paths.conf`); writes nowhere |
| privileges | `NoNewPrivileges`, empty `CapabilityBoundingSet`, `RestrictSUIDSGID`, `LockPersonality`, `MemoryDenyWriteExecute`, `RestrictNamespaces`, `RestrictRealtime` | the same, but no `MemoryDenyWriteExecute` (V8's JIT needs it) |
| kernel and devices | `PrivateTmp`, `PrivateDevices`, `PrivateIPC`, `ProtectKernel{Tunables,Modules,Logs}`, `ProtectControlGroups`, `ProtectClock`, `ProtectHostname`, `ProtectProc=invisible`, `ProcSubset=pid`, `KeyringMode=private` | the same |
| system calls | `SystemCallFilter=@system-service ~@privileged ~@resources`, `SystemCallErrorNumber=EPERM`, `SystemCallArchitectures=native` | the same |
| network | `RestrictAddressFamilies=AF_INET AF_INET6 AF_UNIX` (it listens, and fetches login providers' keys over HTTPS) | the same, and `IPAddressDeny=any` + `IPAddressAllow=localhost` — enforced only by the system manager (below); `--user` logs "configures an IP firewall, but not running as root" and goes on |
| memory | `MemoryHigh=1536M`, `MemoryMax=2G`, `TasksMax=512` — prod peaks near 600 MB | `MemoryHigh=512M`, `MemoryMax=768M`, `TasksMax=64` — it peaks near 200 MB |
| files it creates | `UMask=0077` | `UMask=0077` |

Node is named by its real path in `MENGER_DIAGRAMS_NODE` (install resolves
the symlinks), because the diagrams unit binds only that installation: after
moving Node, run `install` again.

`systemd-analyze security` (exposure, 10 = unsafe):

| unit | before | after |
|---|---|---|
| `menger@test` | 9.4 UNSAFE (`--user`, the installed unit, live); 9.2 (`--offline=true`, the old unit file) | 1.3 OK (`--offline=true`, `deploy/systemd/menger@.service`) |
| `menger-diagrams@test` | 9.4 UNSAFE (live) | 1.1 OK (`--offline=true`) |

```sh
systemd-analyze security --user menger@test.service                     # live, read-only
cp deploy/systemd/menger@.service /tmp/u/menger@test.service && cd /tmp/u && \
  systemd-analyze security --offline=true --user menger@test.service    # a unit file, before it is installed
```

The smoke's headless browser, `--verify-derived` and the probe run outside
the units (the pipeline runs them), so none of this constrains them.

### A public host: a dedicated system user

This machine stays on `systemd --user`. On a host that serves the internet,
run menger as its own system user, so a compromise of it holds nothing of
any person's: no home, no keys, no other service's files. The pipeline here
drives `--user` units; on such a host the units below take their place (the
pipeline's `install` does not write them).

```sh
sudo useradd --system --user-group --home-dir /var/lib/menger --shell /usr/sbin/nologin menger
sudo install -d -o root -g menger -m 0750 /etc/menger                  # settings: root writes, menger reads
sudo install -o root -g menger -m 0640 prod.env prod.local.env /etc/menger/
sudo install -d -o root -g root -m 0755 /opt/menger/releases /opt/menger/envs/prod   # releases: read-only to menger
```

`/etc/systemd/system/menger@.service` — the unit above, with:

```ini
[Service]
User=menger
Group=menger
EnvironmentFile=/etc/menger/%i.env
EnvironmentFile=/etc/menger/%i.local.env
ExecStart=/opt/menger/envs/%i/current/bin/run-menger
# its database: /var/lib/menger/%i/tickets.db (MENGER_DB_PATH in %i.env),
# made and owned by systemd — no ReadWritePaths drop-in
StateDirectory=menger/%i
StateDirectoryMode=0700
ProtectHome=yes
PrivateUsers=yes
# the system manager enforces these (the user manager cannot)
IPAddressDeny=any
IPAddressAllow=localhost
# + everything under "ACCESS-27" in deploy/systemd/menger@.service
```

and `menger-diagrams@.service` the same way (`User=menger`, no
`StateDirectory`, `ProtectHome=yes` — Node from `/usr/bin/node` or `/opt`,
no bind needed). REST and diagrams stay on loopback; the public origin is a
TLS proxy in front of the shell and the WebSocket (ACCESS-30).

### Behind https (ACCESS-30)

A deploy reached beyond this machine is **one https origin**: a proxy
terminates TLS for menger's own hostname, sends `/ws` to the WebSocket and
everything else to the shell, and redirects plain http. menger itself
speaks plain http on loopback (or the address `--bind` names) behind it.
README "Behind https" has what menger does with the URL; this is the proxy.

```caddyfile
# Caddy: a certificate for app.example (Let's Encrypt on a public name,
# `tls internal` for a local CA), http → https by itself on :80.
app.example {
	@ws path /ws
	handle @ws {
		reverse_proxy 127.0.0.1:4455    # MENGER_WS_PORT
	}
	handle {
		reverse_proxy 127.0.0.1:4456    # MENGER_SHELL_PORT
	}
}
```

Caddy passes the `Host` through and sets `X-Forwarded-For` /
`X-Forwarded-Proto`; WebSockets need nothing more. In the environment's
`<env>.local.env`:

```sh
MENGER_PUBLIC_URL=https://app.example        # required beyond loopback with login on
MENGER_TRUSTED_PROXIES=127.0.0.1             # the proxy's address (the edge's, on a box)
# MENGER_ALLOWED_HOSTS / MENGER_ALLOWED_ORIGINS: nothing more — the public
# URL's host and origin are already answered and admitted.
```

and the provider's redirect URI, `https://app.example/login/callback`
(the startup log says it: `Sign-in: returns to …`), registered with the
provider (WorkOS: Redirects). Agents dial `wss://app.example/ws`; a `key`
provider accepts proofs naming it (and `wss://app.example`).

**The checklist** (ACCESS-32 runs it on test; ACCESS-30's acceptance ran
it on a throwaway name behind Caddy with its internal CA):

| check | expect |
|---|---|
| `curl -sI http://app.example/` | `308`/`301` to `https://app.example/` |
| `curl -sI https://app.example/` | `303` to `/login?next=%2F`; `strict-transport-security: max-age=31536000` |
| the page's `content-security-policy` | `connect-src` holds `wss://app.example` |
| sign in in a browser | the session cookie is `__Host-<name>_session; Secure; HttpOnly; SameSite=Lax; Path=/` |
| the app after sign-in | `live · WS`, its socket `wss://app.example/ws` |
| a write (create a ticket) | answered over that socket |
| an upgrade at `/ws` with `Origin: https://evil.example` | `403` |
| `/login/start` for an IdP button | `redirect_uri=https://app.example/login/callback` |
| `curl -s https://app.example/.well-known/oauth-protected-resource/ws` (RUN-9) | `200`, `"resource":"https://app.example/ws"` — the shell answers it, so the proxy needs no route of its own |
| an upgrade at `/ws` with a forged bearer | `401`, `WWW-Authenticate: … resource_metadata="https://app.example/.well-known/oauth-protected-resource/ws"` |
| `synapse --url wss://app.example/ws -t <key proof> menger tickets health` | answered through the proxy |
| the server's startup log | no `plain http on the LAN` warning; no `no --trusted-proxy` warning |

A public URL that is `http://` beyond loopback is refused at startup; one
on the LAN (a private address, `.local`, …) is served with a warning until
it has TLS.

## Recording deployments

Every `deploy test`, `sync test`, `promote` and `rollback` tells menger what
it did (DEPLOY-12), in the deployment record below, made and sent by
`deploy/record` to `deployments.record` (DEPLOY-11) — so each ticket shows
where its work runs:

| the run | records (environment, revision) |
|---|---|
| begins | `started` — the release it moves to (`previous`: the release the environment last ran with its smoke passed) |
| smoke passed | `succeeded` (a rollback: `rolled_back` to the release it returned to) |
| smoke failed | `failed` |
| promote's automatic rollback, smoke passed | `failed`, then `rolled_back` to what ran before |
| dies after `started` (a unit will not restart …) | `failed`, on its way out |

- **The commits** are `git log previous..revision` in the build checkout
  (`../menger-build`, else the repository), oldest first, when a run moves
  forward; a rollback carries none. Without them menger works from what it
  has seen.
- **The range starts at the last run that passed** (DEPLOY-23): `previous`
  of a deploy test, sync or promote is the release `deployments.log` last
  shows the environment running with its smoke passed — a run that ended
  `ok`, or the release a promote's automatic rollback returned to — never a
  failed run, one that died, or a rollback whose smoke failed. So a deploy
  that fails its smoke does not lose its commits: the next one that passes
  (a sync on the same release too) carries them. An environment with no run
  in the log yet starts at what it runs. `backfill` reads the log the same
  way; a rollback's `previous` stays the release it took out.
- **One run, one id.** Each run's records carry `source.run` =
  `promote@<when it began>#<pid>`, and so does the end of its line in
  `deployments.log`. `backfill` makes from that line the record the run sent
  (same environment, state, revision and run), which menger already keeps.
- **Never in the way.** Every record goes through the outbox
  (`~/.local/share/menger-deploy/outbox/`, `MENGER_DEPLOY_OUTBOX`), oldest
  first, each send bounded (`MENGER_DEPLOY_RECORD_TIMEOUT`, 10 s). One menger
  keeps (or already kept) leaves; at the first that gets no answer the rest
  wait for the next run, which sends them before its own. One menger refuses
  moves to `outbox/refused/` and is said, never retried. A record never
  fails a run.
- **Where.** `MENGER_DEPLOY_TRACKER_PORT` (4455, prod's tracker — test's
  deploys are recorded there too), `MENGER_DEPLOY_TRACKER_HOST`,
  `MENGER_DEPLOY_TRACKER_TOKEN` (a token, through synapse — argv; prefer the key).
- **Signed in as the pipeline's key** (ACCESS-47; prod requires login).
  Where `MENGER_DEPLOY_KEY_FILE` (default `~/.plexus/keys/pipeline.key`,
  `synapse _key new pipeline --store file`; `none`: no key) is a file, every
  record goes over the socket — `deploy/record --url
  ws://127.0.0.1:$MENGER_DEPLOY_TRACKER_PORT --agent pipeline`, whose
  `deploy/menger-call.mjs` reads the seed from the file (0600, or it is
  refused) and signs a key proof; the seed is in no environment and no argv.
  The key holds `viewer` on `*` and `deployer` on `MENGER` on prod (the
  operator granted them at the switch; test's database is prod's copy, so
  test has them from its next `deploy test` / `sync test`). A gate's refusal
  (the key not granted, or not admitted) is the tracker's setup, not the
  record's: the record waits in the outbox, said, and is never moved to
  `refused/`. No key file: synapse, anonymous, as before.
- **`status`** shows each environment's last record sent (`sent/<env>.json`)
  and how many wait in the outbox, and reads `tickets.health` signed in as
  the pipeline's key (ACCESS-47) — `health    no answer on :4450 (forbidden)`
  names a gate's refusal (the key not granted there yet).
- **`backfill`** replays `deployments.log` (`deploy/record --from
  pipeline-log`, each with its commits from git) through the outbox.
  Idempotent: run it again and menger answers each record `duplicate`.

```sh
deploy/pipeline backfill        # backfill: 88 records … → record: 88 sent, 0 already kept, 0 refused, 0 waiting
deploy/pipeline backfill        # record: 0 sent, 88 already kept, 0 refused, 0 waiting
synapse -P 4455 menger deployments for_ticket --id MENGER-34
```

Tests: `deploy/tests/pipeline` runs the pipeline itself on a throwaway root —
`HOME` a scratch directory, `systemctl`, `synapse` and the rest stand-ins —
through every row above, a tracker down, hung and refusing, `status` and
`backfill`; and (ACCESS-27) `install`'s split of an old test file (login
kept, nothing printed, a second install a no-op, a hand-added setting
refused) and the smoke's login mode (a mismatch either way fails, the local
half lost fails, no secret in any argv); `cargo test` runs it.

## The deployment record

What a deployment did, in one JSON shape any CI can produce (DEPLOY-10) —
the schema is `deploy/record.schema.json`, and menger's `deployments.record`
(DEPLOY-11) takes it as its parameters. A deployment never changes a ticket's
status; its tickets are the ones its commits are linked to (`commits link`),
plus any it names; an environment is a project's — menger's `prod` and
PANTRY's `prod` are two (DEPLOY-9's decisions).

| field | | meaning |
|---|---|---|
| `project` | required | whose environment: a shortcode, or a project name mapped to one |
| `environment` | required | its name: `test`, `staging`, `prod`, `pr-412` … |
| `state` | required | `started` · `succeeded` · `failed` · `rolled_back` |
| `revision` | required | the commit now running (`repo@sha` outside the project's repository); for `failed`, the one tried; for `rolled_back`, the one returned to |
| `previous` | | the commit that ran before, when known (for `rolled_back`, the one taken out) |
| `commits` | | every commit in `previous..revision`, oldest first, when the CI can list them; omitted, menger works from what it has seen |
| `tickets` | | ticket ids the CI names itself, added to what the commits give |
| `release` | | the artifact's name |
| `at` | | when, UTC (`2026-10-12T22:04:19Z`); `deploy/record` fills in now |
| `source` | | `{ system, run, url }`: who sent it and where to read the run |
| `from` | | DEPLOY-21: the environment this one promotes from (`--promotes-from`) |
| `request` | | DEPLOY-25: the request this deployment answers — the webhook payload's `request` (`--request N`, or `MENGER_REQUEST` in a CI job) |

A hash is 7 to 64 lowercase hex digits; a field the record does not know is
refused, and an absent field is simply left out (`null` means the same).

```json
{
  "project": "menger",
  "environment": "prod",
  "state": "succeeded",
  "revision": "235ff62",
  "previous": "29a50be",
  "commits": ["824e943", "235ff62"],
  "release": "20261012T220402Z-235ff62",
  "at": "2026-10-12T22:04:19Z",
  "source": { "system": "menger-pipeline", "run": "promote", "url": null }
}
```

```json
{
  "project": "PANTRY",
  "environment": "staging",
  "state": "rolled_back",
  "revision": "pantry-web@29a50be",
  "previous": "pantry-web@4f1c2a9",
  "tickets": ["PANTRY-12"],
  "source": { "system": "github-actions", "run": "11223344", "url": "https://github.com/acme/pantry-web/actions/runs/11223344" }
}
```

**`deploy/record`** makes the record from what a CI already has, checks it
against the schema (exit 2, every reason named) and prints it — or, with
`--send`, sends it: `synapse -P PORT -p RECORD menger deployments record`.
It needs bash, jq and (for `--commits-from-git`) git, nothing else.

```sh
deploy/record --project menger --env prod --state succeeded --revision 235ff62 --previous 29a50be
deploy/record --from github-actions            # GITHUB_SHA, GITHUB_RUN_ID, GITHUB_REPOSITORY; MENGER_ENVIRONMENT
deploy/record --from gitlab                    # CI_COMMIT_SHA, CI_ENVIRONMENT_NAME, CI_JOB_ID/URL, CI_JOB_STATUS
deploy/record --from github-event event.json   # a deployment_status (or deployment) payload
deploy/record --from json --map 'environment=.env.name' < ci-output.json   # any JSON
deploy/record --from pipeline-log ~/.local/share/menger-deploy/deployments.log   # one record per deploy line
deploy/record … --commits-from-git [--repo DIR]   # commits = git log previous..revision
deploy/record … --send --port 4455 [--token JWT] [--dry-run]
deploy/record … --send --url wss://menger.example.com [--header 'K: V']   # no synapse: the WebSocket (a deploy key
                                               # in MENGER_DEPLOY_KEY signs in), or http(s):// the REST bridge
deploy/record … --previous-from-menger          # previous = what menger says the environment runs now
deploy/record --validate < record.json
```

- **Flags win** over whatever a form or a map gave: `--project --env --state
  --revision --previous --commits --tickets --release --at --source-system
  --source-run --source-url` (lists repeated or comma-separated).
- **`--map FIELD=JQ`** (repeatable; `source.url` and the like for the nested
  ones) sets a field from the input — the JSON for `json` and `github-event`,
  the process environment for `github-actions` and `gitlab`, the parsed line
  (`at env action release result`) for `pipeline-log`. `--from json` keeps a
  field already named as the record names it and drops the rest.
- **The CI forms** also read `MENGER_PROJECT`, `MENGER_ENVIRONMENT`,
  `MENGER_STATE`, `MENGER_PREVIOUS`, `MENGER_RELEASE` and `MENGER_TICKETS`,
  which a job sets in its `env:` — GitHub Actions has no variable for the
  job's `environment:`, so a workflow names it (`MENGER_ENVIRONMENT: prod` or
  `--env prod`). With no state given, a GitHub Actions step says `succeeded`
  (it runs after the deploy step succeeded); GitLab's comes from
  `CI_JOB_STATUS` (`after_script`); GitHub's `job.status` words are read too (`success` →
  succeeded, `failure`/`cancelled` → failed). The project defaults to the repository's
  name.
- **`github-event`**: `success` → succeeded, `failure`/`error` → failed,
  `pending`/`queued`/`in_progress` (or a `deployment` event) → started;
  `inactive` is refused — a later deployment replaced it, nothing was
  deployed. The deployment's own `payload` may carry `previous`, `commits`,
  `tickets`, `release` and `project`.
- **`pipeline-log`** reads `deployments.log` in order and follows what each
  environment runs, so each record's `previous` is what that environment ran
  before it. Builds are no deployment. `ok` is `succeeded` (`rolled_back` for
  `rollback`); `smoke failed; rolled back to R` is `rolled_back` to R, taking
  the promoted release out; every other failure is `failed`. `sync` is a
  record too (same revision before and after). Each line's `source.run` is
  the run id its sixth column holds (DEPLOY-12), else `action@at` — one line,
  one deployment.
- **`--commits-from-git`** with no `previous` leaves `commits` out and says
  so: a first deployment does not send the whole history.

Tests: `deploy/tests/run` (fixtures in `deploy/tests/fixtures/`: a GitHub
Actions environment, a `deployment_status` payload, a GitLab environment, a
pipeline log, any JSON); `cargo test` runs it, so the gate does.
The same fixtures are menger's own input: the store test
`deploy_10s_records_are_taken_whole` (`tickets-api/src/store/tests_deployments.rs`)
records each one and checks what every environment runs and where each
ticket's work runs. So a change to a fixture runs `cargo test --workspace`,
not only `deploy/tests/run` and `deploy/tests/pipeline` (DEPLOY-24: DEPLOY-23's
two new pipeline lines merged with that test red).

## Deployments from any CI (DEPLOY-14)

A CI records each deployment where it happens: a GitHub Actions workflow
with one step, any other CI with one `deploy/record` command. Each speaks
straight to menger — no synapse on the runner.

**GitHub Actions.** `deploy/record-action` is a composite action around
`deploy/record --from github-actions --previous-from-menger
--commits-from-git --send --url …`. Run it before the deploy as `started`,
and after it, `if: always()`, with the job's status:

```yaml
jobs:
  deploy:
    runs-on: ubuntu-latest
    environment: prod
    steps:
      - uses: actions/checkout@v4
      - uses: hypermemetic-ai/menger/deploy/record-action@v1
        with: { menger-url: "${{ vars.MENGER_URL }}", key: "${{ secrets.MENGER_DEPLOY_KEY }}", project: PANTRY, environment: prod, state: started }
      - run: ./deploy.sh prod
      - uses: hypermemetic-ai/menger/deploy/record-action@v1
        if: always()
        with:
          menger-url: ${{ vars.MENGER_URL }}
          key: ${{ secrets.MENGER_DEPLOY_KEY }}
          project: PANTRY
          environment: prod                     # the job's environment: GitHub has no context for it
          state: ${{ job.status }}              # success → succeeded; failure, cancelled → failed
```

- `GITHUB_SHA` is the revision, `GITHUB_RUN_ID` the run (a re-run step is
  the same deployment, kept once), the run's page the source.
- **Previous and commits.** The previous revision is what menger says the
  environment runs now (`deployments.environments`; the `started` record
  changes nothing there), and the commits are `git log previous..revision`
  from the checkout. The default `actions/checkout` is shallow
  (`fetch-depth: 1`): the action fetches the history itself, or check out
  with `fetch-depth: 0`. A first deployment sends no commits; `previous:`
  overrides the question.
- Other inputs: `release`, `tickets`, `agent` (default `github-actions`),
  `headers`, `path`, `commits-from-git: false`. Outputs: `deployment` (its
  id) and `carried` (the tickets); the step summary says both.
- The runner needs bash, git, jq and node ≥ 18, which every GitHub-hosted
  runner has.

**The deploy key.** A CI signs in with an agent key (IDENTITY-40/42) that
holds the `deployer` role on the project's shortcode: the reads, and
`deployments.record` — nothing an editor may do, and no other project. No
person's session is involved, and no token expires.

```sh
synapse _key new pantry-deploy       # prints key:<hex>; the seed is in ~/.plexus/keys/pantry-deploy.key
node deploy/menger-call.mjs --new-key   # or without synapse: the seed (line 1), then key:<hex>
synapse -P 4455 -t "$ADMIN" menger members grant --principal key:<hex> --scope PANTRY --role deployer
gh secret set MENGER_DEPLOY_KEY < ~/.plexus/keys/pantry-deploy.key
gh variable set MENGER_URL --body wss://menger.example.com
```

menger must admit key proofs: `--auth-provider key` (in
`MENGER_AUTH_PROVIDERS`). On each call `deploy/menger-call.mjs` signs a
fresh proof naming the URL it dials, for the agent `--agent`; the
connection is `key:<hex>`, and each deployment is `recorded_by` it. To stop
a CI, revoke its key: `members revoke --principal key:<hex> --scope PANTRY`.
A deploy key is also `auth.grant`'s `deployer` (AUTH-7). A file holding a
deployer grant opens only in a build from DEPLOY-14 on (an older build does
not know the role): revoke it before going back past it.

**Reaching menger.** `--url` (the action's `menger-url`) picks the way:

| `--url` | how | login |
|---|---|---|
| `ws://host:4455`, `wss://…` | the WebSocket, through `deploy/menger-call.mjs` (Node's standard library only) | any; a deploy key signs in |
| `http://host:4457`, `https://…` | the REST bridge, through curl: `POST /rest/deployments/record` | none — REST carries no identity, so `deploy/record` refuses a key there |
| none | synapse: `--send --port 4455 [--host H] [--token T]` | any |

The WebSocket is the one way with a key: the proof rides the upgrade as
`Authorization: Bearer plexus-key.…`, which the REST bridge never reads, and
a menger with `--require-login` has no REST bridge at all. REST is for a
menger without login on a private network.

**Behind a tunnel** (Cloudflare Tunnel, Tailscale Funnel, ngrok, `ssh -R`):
point the tunnel's hostname at menger's WebSocket port and name it to menger
as a socket a proof may name — the proof's audience is the URL the CI
dials, and menger accepts only its own:

```sh
menger --auth-provider key=wss://menger.example.com …   # or --public-url https://menger.example.com (its ws form)
```

A tunnel that gates requests itself takes its headers on every call —
`headers:` in the action, `--header 'Name: value'` (repeatable) for
`deploy/record`, `MENGER_HEADERS` (one a line) for `menger-call.mjs`:
`CF-Access-Client-Id` and `CF-Access-Client-Secret` for a Cloudflare Access
service token. A proof is good for a minute and once; the connection opened
with it, for five.

**Getting `deploy/record`** on a runner that has no menger checkout — three
files, beside each other:

```sh
mkdir -p .menger && for f in record record.schema.json menger-call.mjs; do curl -fsSL "https://raw.githubusercontent.com/hypermemetic-ai/menger/v1/deploy/$f" -o ".menger/$f"; done && chmod +x .menger/record
```

Each snippet below is run by `deploy/tests/run`, its records checked
against the schema (`<!-- ci-snippet … -->` marks them).

**GitLab CI.** `--from gitlab` reads `CI_COMMIT_SHA`, `CI_ENVIRONMENT_NAME`,
`CI_JOB_ID`/`URL` and `CI_JOB_STATUS` — `running` in `before_script` is
`started`, `success` or `failed` in `after_script` the outcome. The key is a
masked CI/CD variable, `MENGER_DEPLOY_KEY`; `MENGER_URL` beside it.

<!-- ci-snippet gitlab -->
```yaml
deploy:
  stage: deploy
  image: node:22                         # bash, git, curl, node; jq below
  environment: production
  variables:
    GIT_DEPTH: 0                         # the whole history: previous..revision
    MENGER_PROJECT: PANTRY
  before_script:
    - apt-get update -qq && apt-get install -y -qq jq
    - mkdir -p .menger && for f in record record.schema.json menger-call.mjs; do curl -fsSL "https://raw.githubusercontent.com/hypermemetic-ai/menger/v1/deploy/$f" -o ".menger/$f"; done && chmod +x .menger/record
    - .menger/record --from gitlab --previous-from-menger --commits-from-git --send --url "$MENGER_URL"
  script:
    - ./deploy.sh production
  after_script:
    - .menger/record --from gitlab --previous-from-menger --commits-from-git --send --url "$MENGER_URL"
```

**Buildkite.** No form of its own: flags from Buildkite's variables.

<!-- ci-snippet buildkite -->
```yaml
steps:
  - label: ":rocket: prod"
    command: |
      mkdir -p .menger && for f in record record.schema.json menger-call.mjs; do curl -fsSL "https://raw.githubusercontent.com/hypermemetic-ai/menger/v1/deploy/$f" -o ".menger/$f"; done && chmod +x .menger/record
      .menger/record --project PANTRY --env prod --state started --revision "$BUILDKITE_COMMIT" \
        --source-system buildkite --source-run "$BUILDKITE_BUILD_ID" --source-url "$BUILDKITE_BUILD_URL" \
        --previous-from-menger --commits-from-git --send --url "$MENGER_URL"
      if ./deploy.sh prod; then state=succeeded; else state=failed; fi
      .menger/record --project PANTRY --env prod --state "$state" --revision "$BUILDKITE_COMMIT" \
        --source-system buildkite --source-run "$BUILDKITE_BUILD_ID" --source-url "$BUILDKITE_BUILD_URL" \
        --previous-from-menger --commits-from-git --send --url "$MENGER_URL"
      test "$state" = succeeded
```

**A plain shell script.** Any deploy that is a script records itself:

<!-- ci-snippet shell -->
```sh
#!/usr/bin/env bash
# release ENV — deploys this checkout's HEAD to ENV and records it in menger
set -uo pipefail
env=$1 rev=$(git rev-parse HEAD) run="$(hostname)-$(date +%s)"
record() { .menger/record --project PANTRY --env "$env" --state "$1" --revision "$rev" \
  --source-system shell --source-run "$run" --previous-from-menger --commits-from-git --send --url "$MENGER_URL"; }
record started
if ./deploy.sh "$env"; then record succeeded; else record failed; exit 1; fi
```

**GitHub's `deployment_status` webhook: no change at all.** A job that
names an `environment:` already makes GitHub create a deployment and post
its statuses. `deploy/github-relay.mjs` (Node, nothing to install) receives
the repository's webhook — content type `application/json`, the
*Deployment statuses* event, a secret — checks the signature, and hands each
status to `deploy/record --from github-event --previous-from-menger --send`:

```sh
GITHUB_WEBHOOK_SECRET=… MENGER_URL=wss://menger.example.com MENGER_DEPLOY_KEY=… \
  node deploy/github-relay.mjs --port 8787 --project PANTRY --repo /srv/clones/pantry
```

`--project` when the repository's name is not the shortcode; `--repo` a
clone it fetches and lists the commits from (without one, the record
carries the revision and previous only). It answers 202 once recorded, 204
for what is no deployment (a ping, `inactive`), 401 for a bad signature and
502 when menger refused it — GitHub's *Recent Deliveries* shows which, and
redelivers. The relay must be reachable by GitHub; the action is the way
when only the runner can reach menger.

**Talking to menger from a script.** `deploy/menger-call.mjs` calls any
method over the WebSocket — one call per params line on stdin, or
`--params`; each data item a JSON line; exit 3 on a refusal:

```sh
MENGER_DEPLOY_KEY=… node deploy/menger-call.mjs --url wss://menger.example.com deployments.for_ticket --params '{"id":"PANTRY-12"}'
node deploy/menger-call.mjs --principal      # key:<hex> of $MENGER_DEPLOY_KEY: what to grant
```

## A deploy request's webhook (DEPLOY-25)

A deploy can be queued from menger (`deployments request`, or *Queue a
deploy* on `#/deployments`). menger keeps the request and POSTs it to the
environment's webhook, which an admin sets:

```sh
cat > webhook.json <<'JSON'   # 0600; the secret never goes in argv
{"project": "menger", "environment": "prod", "url": "https://ci.example.com/hooks/menger", "secret": "…16 to 256 bytes…"}
JSON
synapse -P 4455 -p @webhook.json menger deployments webhook set
synapse -P 4455 menger deployments webhook show --project menger --environment prod   # url, secret_set — never the secret
```

The URL is https, or http to this machine (`127.0.0.1`, `localhost`,
`[::1]`) for a CI that runs here. Each delivery is:

```http
POST /hooks/menger HTTP/1.1
Content-Type: application/json
X-Menger-Event: deploy.requested
X-Menger-Delivery: 3f6c0e9a-…            the same on every attempt of one request
X-Menger-Attempt: 1
X-Menger-Signature: sha256=5bdcc146…     lower-case hex HMAC-SHA256 of the body, keyed by the secret

{"request":41,"project":"MENGER","environment":"prod","revision":"235ff62","release":"20261012T220402Z-235ff62",
 "from":"test","requested_by":"key:4b…","note":"the week's work","at":"2026-10-17T07:00:00Z"}
```

A 2xx answers it. Anything else, or no answer within 10 s, is tried again
after 1 s, 5 s, 30 s, 2 min and 10 min; after the sixth attempt the delivery
is `failed`. Redirects are not followed. The request shows each attempt.

**Verifying a delivery.** Compute HMAC-SHA256 of the raw body bytes — as
received, before parsing — keyed by the secret, and compare its hex with
the header after `sha256=` in constant time. Refuse anything else. Then
treat `X-Menger-Delivery` as the idempotency key (a retry repeats it):

```js
// Node: verify, then answer before the deploy runs (menger waits 10 s at most).
import { createHmac, timingSafeEqual } from "node:crypto";
function verified(rawBody, header, secret) {
  const want = "sha256=" + createHmac("sha256", secret).update(rawBody).digest("hex");
  return typeof header === "string" && header.length === want.length && timingSafeEqual(Buffer.from(header), Buffer.from(want));
}
```

```sh
# a shell receiver, the body in body.json: the signature it should carry
printf 'sha256=%s\n' "$(openssl dgst -sha256 -hmac "$SECRET" -hex < body.json | sed 's/^.*= //')"
```

**Closing the loop.** The CI records what it did as any deploy, naming the
request, so menger shows its outcome: `deploy/record --state started
--request 41 … --send`, then `succeeded` or `failed` the same way (or
`MENGER_REQUEST=41` in the job's environment). menger's own pipeline is not
a receiver yet; this machine's `deploy/pipeline` would be one added later.
A request can be canceled (`deployments cancel_request --id 41`) until a
record names it; its delivery then stops.

## Where things are

- releases: `~/.local/share/menger-deploy/releases/`
- what each env runs: `~/.local/share/menger-deploy/envs/{prod,test}/{current,previous}`
- every build, deploy, promote and rollback: `~/.local/share/menger-deploy/deployments.log`
- deployment records waiting for menger: `~/.local/share/menger-deploy/outbox/`
  (refused: `outbox/refused/`); the last sent per env: `…/menger-deploy/sent/`
- logs: `deploy/pipeline logs prod` (`journalctl --user -u menger@prod -u menger-diagrams@prod`)
- prod backups: `~/.local/share/menger/backups/`

## Rules

- Production changes only through `promote`. Nothing runs production from a
  development checkout; `cargo build` or `trunk build` in `menger/` changes
  nothing that is serving.
- Work is checked on the test environment or on a throwaway server — never on
  prod.
- A release that failed test is never promoted; fix forward with a new commit.
