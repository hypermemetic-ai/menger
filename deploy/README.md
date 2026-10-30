# Deploying menger

menger runs in two environments, **test** (https://app-test.menger.sh) and
**prod** (https://app.menger.sh), both on the Linode as containers of one
image, through hypermemetic-infra ("On the Linode", below). This machine
builds the releases and their images, runs the suite, and runs no
environment (DEPLOY-52, owner 2026-10-22/23: *"we need to work directly
against the test deployment and there should be no tests running on that
deployment"*, *"we run the tests after the deployment to test"*, *"we need to
remove the non remote environments"*). Production changes only through this
pipeline (DEPLOY-1).

```
commit on main → build (no gate) → release → image (GHCR) → remote test → smoke → live on app-test.menger.sh
                                                                  ↓ red                 ↓
                                                          prod untouched     the suite, on this machine, at the release's commit
                                                                                        ↓ green: the release may go to prod
                                                                                        ↓ red: marked; remote prod refuses it
                                                         remote prod (the owner's) → smoke → live on app.menger.sh
```

## The environments

| | test | prod |
|---|---|---|
| where | https://app-test.menger.sh (`wss://app-test.menger.sh/ws`) | https://app.menger.sh (`wss://app.menger.sh/ws`) |
| deployed by | `deploy/pipeline remote test --image …` (infra's `app.yml`) | `deploy/pipeline remote prod` (infra's `promote.yml`: what test runs) |
| recorded as | `test` (DEPLOY-52; it was `app-test`) | `app-prod`, promoting from `test` |
| its data | its own tracker, in the box's volume | the tracker since RUN-13 |
| settings | `deploy/env/app-test.env` | `deploy/env/app-prod.env` |

**Check work on test, signed in as your key there** — once per machine (a
session is the host's own), then as usual:

```sh
synapse --url wss://app-test.menger.sh/ws _login menger --key builder --as builder
synapse --url wss://app-test.menger.sh/ws --as builder menger tickets health
scripts/shot.mjs '#/board' --env test --width 390     # the builder's key by default
```

Test is where people and agents work against a deploy; the suite never
does: it runs on this machine, against the release's own tree, and writes
nothing into app-test's tracker. `deploy/pipeline local` (RUN-25's
`menger-app-local` on 127.0.0.1:4470/4471) stays — a CI throwaway for
hyperforge's `deploy-local` stage, not an environment.

## The local environments, retired (DEPLOY-52)

This machine's test (`menger@test`, `menger-diagrams@test`, :4450–4453) and
prod (`menger@prod`, `menger-diagrams@prod`, :4455–4458, stopped since
RUN-13) were stopped, disabled and their units uninstalled on 2026-10-23,
with the LAN proxies in front of prod (`menger-lan@4455/4456`, RUN-19).
`deploy/pipeline install`, `deploy test`, `sync`, `promote`, `rollback` and
`logs` say so and exit 2.

**Their data stays where it was, untouched** — retiring a service is
reversible, deleting a tracker is not:

| what | where |
|---|---|
| prod's database (the move's source, RUN-13) | `~/.local/share/menger/tickets.db` (+ `-wal`, `-shm`) |
| prod's backups, the move's own among them | `~/.local/share/menger/backups/` (`tickets.db.<when>-move-RUN-13`) |
| test's copy and its backups | `~/.local/share/menger-test/` |
| their settings | `~/.config/menger/{prod,test}{,.local,.smoke}.env` |
| what each ran | `~/.local/share/menger-deploy/envs/{prod,test}/` |
| the releases | `~/.local/share/menger-deploy/releases/` |
| the move's mark | `~/.local/share/menger-deploy/prod.moved` |
| the uninstalled unit files (and their drop-ins, the LAN proxies') | `~/.local/share/menger-deploy/retired-units/DEPLOY-52-2026-10-23/` — moved out of `~/.config/systemd/user`, not deleted |

The unit files, the settings templates and the local smoke's scripts stay
in the repository (`deploy/systemd/`, `deploy/env/{prod,test}.env`,
`deploy/smoke-*.mjs`, `deploy/probe-auth.mjs`; "Hardening" below) — the way
back, should one ever be wanted, is `git show <before DEPLOY-52>:deploy/pipeline`
and its `install`. Nothing on this machine reads them now.

## The pipeline

```sh
deploy/pipeline build [REV] [--no-gate]   # a commit → a release (prints its name); --no-gate: the suite comes after
deploy/pipeline image [RELEASE]           # RUN-10: the release as an image (IMAGE_STORE=ghcr pushes it to GHCR)
deploy/pipeline remote test --image IMG   # app-test.menger.sh through infra, then the smoke; recorded as test
deploy/pipeline suite RELEASE             # DEPLOY-52: the full suite at the release's commit, plexus-bench beside it (PERF-6); marks the release
deploy/pipeline suite --status RELEASE    # suite: passed | failed | running | none
deploy/pipeline remote prod               # what test runs → app.menger.sh, only once its suite passed
deploy/pipeline smoke test|prod           # the smoke against app-test / app.menger.sh, on its own
deploy/pipeline status                    # what test and prod run, their suites, health, last records; the runner; the disk
deploy/pipeline releases                  # every release: its suite, tested?, where it runs
deploy/pipeline backfill                  # deployments.log → menger's deployments (idempotent)
deploy/pipeline landing                   # main's landing/ → gate → the edge (menger.sh); recorded as env `edge`
deploy/pipeline landing --check           # the gate and infra's dry run, nothing written
deploy/pipeline media                     # landing/media regenerated from the release prod runs
deploy/pipeline local --image IMG         # RUN-25: the image here as menger-app-local, 127.0.0.1:4470/4471 (a CI throwaway)
deploy/pipeline remote-secret test|prod   # IDENTITY-45: the WorkOS API key on the box, from a pipe ("The WorkOS key on the box")
```

The deploy runner does the first four by itself for every merge set In
Review ("Automatic test", below); prod is the owner's.

1. **Build** (`build [REV] [--no-gate]`, default `main`). A commit, never a
   working tree: `../menger-build` — a sibling checkout, so menger's relative
   plexus dependencies resolve — is moved to the commit and cleaned, keeping
   only build outputs. A low disk refuses it before anything runs (CI-2:
   `scripts/disk-check.sh` — under 50 GB free or 90% used, `DISK_MIN_FREE_GB`
   / `DISK_MAX_USED_PCT`); `status` says the disk on its last line.
2. **Gate** — only without `--no-gate`: `cargo test --workspace` and the
   frontend's native tests at that commit. Red: no release. The runner builds
   with `--no-gate` (DEPLOY-52): the suite runs after the deploy (step 5).
3. **Release.** `cargo build --release` (menger, menger-diagrams), `trunk
   build --release`, `npm ci` for the diagrams Node program, assembled into
   `~/.local/share/menger-deploy/releases/<UTC time>-<sha>/` — `bin/` (the two
   binaries and their launchers), `app/`, `diagrams-node/`, and `RELEASE`
   (commit, subject, build time, toolchain, `gate:` — the counts, or `none`
   when the suite comes after). Read-only; never rebuilt. Prod runs exactly
   the bytes test ran. The newest ten, and those test and prod run (and ran
   last before), are kept.
4. **Image and deploy to test.** `IMAGE_STORE=ghcr deploy/pipeline image
   <release>` (RUN-10, below), then `deploy/pipeline remote test --image
   ghcr.io/hypermemetic-ai/menger:<release>`: infra's `app.yml` deploys it to
   app-test.menger.sh, then the smoke (below). Passing marks the release
   `TESTED` and records `test succeeded` — which sets Complete on the In
   Review work it carries (DEPLOY-22/28).
5. **The suite** (`suite RELEASE`, DEPLOY-52). `cargo test --workspace` and
   the frontend's tests at the release's commit, in its own sibling checkout
   `../menger-suite` (`MENGER_SUITE_CHECKOUT`; its own `target/`), so a suite
   under way never holds up the next build; one suite at a time (a lock). It
   never touches a deployment: nothing writes test data into app-test's
   tracker. The release's `SUITE` file says `running …`, then `passed …` with
   the counts or `failed …` with the red log (a suite killed half-way is
   `failed`), and `deployments.log` gets `suite test <release> passed|failed`
   (no deployment). Each gate log is a file of its own (CI-6), as the build's.
   A release built with the gate (every one before DEPLOY-52) counts as
   passed: its gate was the suite. **Beside it, plexus-bench** (PERF-6): its
   `scripts/gate.sh` runs every PERF scenario under the perf profile and
   `plexus-bench check` against the goldens (a red scenario is run once more
   before it counts; a red taken on a busy machine is said, not failed), with
   `schema.*` measuring the release's own `bin/menger`. A red bench marks the
   release `failed … — bench red; log …` and logs `suite test <release>
   failed bench`, as a red test does; green, the `SUITE` line ends `; bench
   <check's verdict>`. `MENGER_BENCH_DIR` names the checkout (default
   `../plexus-bench`; `none` skips it and the line says `bench not run`),
   `MENGER_BENCH_TARGET` its cargo target (`~/.cache/cargo-target/plexus-bench`).
6. **Promote** (`remote prod`). infra's `promote.yml` ships the image test
   runs — the release `deployments.log` last shows test running `ok` — and
   only when that release's suite `passed`: `failed`, `running` or no result
   is refused before infra is asked, naming why. Then the smoke against
   app.menger.sh; recorded as `app-prod` (from `test`). A failed smoke is
   recorded `failed`; `bin/infra rollback.yml -e app_dir=<checkout>` puts prod
   back on its previous image ("On the Linode").

**Smoke checks** (`smoke test|prod`; `remote` runs them after infra): from
outside, as a person or an agent meets the deployment, reading only — `http://`
redirects to `https://`; `/` is behind the login page (303 to `/login`) with
HSTS (`max-age=31536000`); `/login` is the sign-in page; RUN-9's
`/.well-known/oauth-protected-resource/ws` names the host's `/ws`; a socket
upgrade from another origin is refused (403); `tickets.health` answers on
`wss://<host>/ws` signed in as the pipeline key (retried for
`MENGER_SMOKE_WAIT`, 60 s, after a restart; no key: the smoke fails); an
anonymous socket reaches nothing (`unauthenticated`); and, when the release is
here, the host serves that release's own app (its `menger-app-*.js`). It
writes nothing — no probe tickets, no grants (the local smoke's
`probe-auth.mjs` wrote to test's tracker; DEPLOY-52 keeps that off app-test).
`DEPLOY_SMOKE_FAIL=test|prod` fails it on purpose (a drill).

### Logs (CI-6)

Every log a run writes is a file of its own, never overwritten:
`~/.local/state/menger-deploy/logs/<UTC yyyymmddThhmmssZ>-<short sha>-<step>.log`
(`MENGER_DEPLOY_LOGS` moves the directory). `build` keeps `gate-workspace`,
`gate-frontend`, `build-release` and `build-trunk` apart (`suite` keeps its own
two gate logs the same way); `build`, `suite`, `smoke`, `landing`, `media`,
`image`, `local` and `remote` also keep everything they said as
`<stamp>-<command>[-<arg>].log`. Each
says `log: <path>` as it opens one, and a `die` names the file. The gate runs
`cargo test --workspace --no-fail-fast`, so one red shows every failing test,
and a red gate prints `FAILED <test>` for each. A run that fails is marked
(`<stamp>.red` beside its logs). The deploy runner copies the logs a step named
into `runs/<request>/<step>-<name>.log` (served at `GET /runs/<request>/…`) and its
red post names them. `deploy/pipeline status` prints the newest log. Retention:
the hourly sweep (`scripts/cleanup-gate.sh --sweep`) removes logs older than 60
days (`--log-days`) and never a marked run's.

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

menger as a container image, made from a release's own bytes, never a build
from a checkout — the image comes before the test (DEPLOY-52), so what test
runs and prod is promoted to is that very image. `deploy/pipeline
image [RELEASE]` stages the release's `bin/menger`, `bin/run-menger` (its
own launcher), `app/` and `RELEASE` beside `deploy/image/Dockerfile` and
builds it; RUN-25's `build` stage is this command.

```sh
deploy/pipeline image                      # the newest release → menger:<release> in this machine's Docker
deploy/pipeline image 20261017T054109Z-9501b9f   # a named one
IMAGE_STORE=ghcr deploy/pipeline image R   # ghcr.io/hypermemetic-ai/menger:<release> and :latest, pushed
```

- **Which release.** The one named, else the newest built (DEPLOY-52: any
  release may be imaged; what guards prod is `remote prod`'s suite check). A
  release whose menger predates `--bind` (RUN-8) is refused: bound to
  loopback inside a container, it could not be reached.
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

**Inside the image**: `debian:trixie-slim` (glibc 2.41 — at least the glibc the release binary links, built on this machine: since DIST-14 it needs 2.39, DEPLOY-51), user `menger` (uid 10001),
`tini` as PID 1 (so `docker stop` is a SIGTERM menger heeds), menger bound
to `0.0.0.0` (`MENGER_BIND`), the WebSocket on 8080 and the shell on 8081
(`MENGER_WS_PORT`, `MENGER_SHELL_PORT`; TRANSPORT-15: `/mcp` is on the
shell's origin, so the edge's default route carries it), the REST bridge off
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
- **`remote test --image IMAGE`** deploys to `app-test.menger.sh` (test,
  DEPLOY-52) through
  hypermemetic-infra's Plexus app role: `bin/infra app.yml -e
  app_dir=<the main checkout> -e env=test -e sha=<the release>` — the tag
  infra deploys is the release's name on `ghcr.io/hypermemetic-ai/menger`
  (deploy/app.yml's image), so a local `menger:<release>` is tagged that
  way first for infra to ship; then the smoke, and passing marks the
  release `TESTED`. **`remote prod`** is `bin/infra promote.yml -e app_dir=…`
  only: prod takes the image test runs and passed, by infra's rule — and only
  once that release's suite passed (DEPLOY-52). Both are driven by menger's own `deploy/app.yml` (RUN-11, below);
  without it each refuses before touching anything.
- **The stages' environment.** A stage inherits hyperforge's (its unit's
  `PATH` holds `~/.local/bin`, where `synapse` and `node` are, and
  `/usr/bin`, where `docker` is); each `run` line also puts
  `$HOME/.local/bin` and the system's bin first itself, since a pipeline
  `env` value is taken literally, never expanded. `IMAGE_STORE` is the
  pipeline's `env`: `ghcr` there makes `build` push and the deploys take the
  registry's ref — a change to the file, not to hyperforge.
- `deploy-local` is a CI throwaway on this machine, not an environment
  (DEPLOY-52 keeps it); hyperforge's `build` stage images the newest release.

## On the Linode (RUN-11)

`deploy/app.yml` describes menger to hypermemetic-infra in plexus-starter's
host-layer contract (SCAFFOLD-12/-19): the image
`ghcr.io/hypermemetic-ai/menger:<release>` (RUN-10, `IMAGE_STORE=ghcr
deploy/pipeline image [RELEASE]`), two hosts, the WebSocket (8080) and the
shell (8081) inside the container, the standard routes (`/ws` and any
upgrade → the WebSocket, the rest → the shell, so RUN-9's metadata needs no
route), a `/data` volume per environment (each its own tracker), 1 GB,
and the health checks below. infra makes `/opt/apps/menger-test` (test)
and `/opt/apps/menger` (prod) on the box, the edge's site per host (Caddy:
Let's Encrypt over HTTP-01, http → https 308), the A record (DNS-only →
45.56.124.48), and sets `MENGER_TRUSTED_PROXIES=172.30.0.2/32` (the edge's
pinned address, SCAFFOLD-22) over `app.env`.

| environment | host | settings (copied to the box as `app.env`) | state |
|---|---|---|---|
| test | `app-test.menger.sh` | `deploy/env/app-test.env` | live since 2026-10-17, its own tracker; since DEPLOY-52 (2026-10-23) **the** test environment — every merge set In Review lands here, recorded as `test` |
| prod | `app.menger.sh` | `deploy/env/app-prod.env` | live since 2026-10-17, prod's data moved there (RUN-13); WorkOS production, the GitHub button through WorkOS's API with the key on the box (IDENTITY-45) |

The settings are menger's own (README "Bind", "Trusted proxies", "Behind
https", Login): `MENGER_PUBLIC_URL=https://<host>`, `MENGER_BIND=0.0.0.0`,
`MENGER_AUTH_PROVIDERS` = WorkOS (AuthKit — no secret on the box; test uses
the staging client, as this machine's retired prod and test did) and
`key=wss://<host>/ws` for agents, `MENGER_LOGIN_EMAIL=workos:authkit`, the
four provider buttons, `MENGER_REQUIRE_LOGIN`, `MENGER_NO_REST`, and
`MENGER_ADMINS` = the owner's WorkOS principal and the `operator` key —
nothing else is granted. Nothing in either file is a secret; one would go in
the box's own `secret.env`, which infra never reads or writes.

prod signs in through WorkOS **production**, whose API key is sealed to the
owner's Ledger (WORKOS-14): what needs it runs under `ledger-secret`, the
owner approving on the device. `deploy/workos-redirect-uris` (RUN-12) keeps
an environment's redirect URIs and names a user's principal — the staging
key by default:

```sh
deploy/workos-redirect-uris list                               # staging
ledger-secret run workos-prod WORKOS_API_KEY -- deploy/workos-redirect-uris ensure \
  https://app.menger.sh/login/callback https://app-test.menger.sh/login/callback
ledger-secret run workos-prod WORKOS_API_KEY -- deploy/workos-redirect-uris user <owner email>
```

**The move (RUN-13, 2026-10-17).** Prod's data runs on app.menger.sh:
local prod stopped (`systemctl --user stop menger@prod`), a `sqlite3
.backup` into `~/.local/share/menger/backups/tickets.db.<when>-move-RUN-13`,
seeded by infra (`bin/infra seed.yml -e app_dir=. -e env=prod -e
seed=<backup> -e '{"seed_verify":["/opt/menger/bin/menger","--verify-derived","--db-path","/data/tickets.db"]}'`
— the volume's old file moved aside, never deleted), then `deploy/pipeline
remote prod`. `~/.local/share/menger-deploy/prod.moved` marks it.

**The rollback (since DEPLOY-52, 2026-10-23).** This machine's prod is
retired — its unit uninstalled, nothing to start — so going back is no
longer a local start: it is **a restore of that file onto the Linode**,
through the same seed. The file is where the move left it, untouched:
`~/.local/share/menger/tickets.db` (the database as it stood when prod
stopped) and its `.backup`, `~/.local/share/menger/backups/tickets.db.<when>-move-RUN-13`.

```sh
B=~/.local/share/menger/backups/tickets.db.<when>-move-RUN-13      # or a newer copy taken from the box
../hypermemetic-infra/bin/infra seed.yml -e app_dir=. -e env=prod -e seed=$B --check      # what would be done
../hypermemetic-infra/bin/infra seed.yml -e app_dir=. -e env=prod -e seed=$B \
  -e '{"seed_verify":["/opt/menger/bin/menger","--verify-derived","--db-path","/data/tickets.db"]}'
../hypermemetic-infra/bin/infra promote.yml -e app_dir=.          # health, as after any seed
```

The seed moves the volume's current file aside (`tickets.db.before-seed-<when>`,
never deleted), copies the file in as the container's user, checks its
sha256 and `--verify-derived`, and starts menger again; a failed check puts
the old file back. What was written on app.menger.sh since the move lives
only in that moved-aside file: a restore of the RUN-13 file loses it, so take
a fresh copy from the box first when it matters. The same restore onto test
is `-e env=test`. A seed is the owner's: it replaces a live tracker.

### The WorkOS key on the box (IDENTITY-45)

WorkOS answers a direct GitHub sign-in with `email_verification_required`
for every user whose address it has not verified (email verification is
always on), and the grant that completes that step needs the client secret.
So each environment's menger can hold its WorkOS environment's API key — a
file on the box, never in the repository, `app-*.env`, the image or a log —
and then the GitHub button completes through WorkOS's API on menger's own
page (README "The direct buttons with the key"). Without the file the
button is the PKCE path, as before.

`deploy/app.yml` declares the file (`secrets`: `workos-api-key`, read by
the image's uid 10001, named to menger as `MENGER_WORKOS_API_KEY_FILE`).
hypermemetic-infra keeps `/opt/apps/<project>/secrets/` (0500) on the box,
mounts it read-only at `/run/secrets`, and sets the variable only for a file
the box holds. `deploy/pipeline remote-secret` puts the value there through
infra's `secret.yml`, on a pipe: written 0400, the container recreated (or
restarted), health checked, and on a failed check the old value (or none)
put back by itself. The value comes from `$WORKOS_API_KEY`, `--from FILE`, or
stdin, and is checked to be an `sk_…` key before anything is sent:

```sh
# app.menger.sh: WorkOS production's key, sealed to the owner's Ledger — one approval on the device
ledger-secret run workos-prod WORKOS_API_KEY -- deploy/pipeline remote-secret prod
# or from a key file the owner placed (0600)
deploy/pipeline remote-secret prod --from ~/.config/workos/production.key
# app-test.menger.sh: WorkOS staging's key
deploy/pipeline remote-secret test --from ~/.config/workos/staging.key
# a dry run: infra's secret.yml --check --diff, nothing written
deploy/pipeline remote-secret prod --from ~/.config/workos/production.key --check
```

Each run is a line in `deployments.log` (`app-prod secret workos-api-key
ok`) and in the box's own log, never the value. Rotating is the same
command with the new key. A new image needs nothing: the file stays on the
box, and every `remote test` / `remote prod` mounts it again.

menger has no admin-by-email form: `MENGER_ADMINS` names principals
(`oidc:workos/user_…`). A person signed in with no grant waits in `members
requests` (with the name the provider gave), where an admin — the
`operator` key — grants them. `deploy/grant-first-admin --url
wss://app.menger.sh/ws --name <email> --name <github name>` does it once,
for the first such WorkOS sign-in (exactly one match, or nothing), and
records it on RUN-11.

```sh
IMAGE_STORE=ghcr deploy/pipeline image 20261017T081530Z-5773250   # a TESTED release → GHCR
../hypermemetic-infra/bin/infra app.yml -e app_dir=. -e env=test -e sha=20261017T081530Z-5773250 --check --diff
../hypermemetic-infra/bin/infra app.yml -e app_dir=. -e env=test -e sha=20261017T081530Z-5773250
synapse --url wss://app-test.menger.sh/ws _login menger --key operator --as operator   # its session is the host's own
synapse --url wss://app-test.menger.sh/ws menger tickets health
```

**Healthy** (infra probes each, through the edge, retrying while the
certificate is issued): `http://<host>/` 308 to `https://<host>/`; `/` 303
to `/login` and `strict-transport-security: max-age=31536000`; `/login` 200,
titled *Sign in*, with HSTS; an upgrade at `/ws` 101 from the site's origin
and 403 from another. The login page has its own policy (`connect-src
'self'`), so the `wss://` origin in `connect-src` is the app's page, checked
after a sign-in.

## Hardening (ACCESS-27)

> DEPLOY-52: this machine's units are retired (uninstalled 2026-10-23); the
> files stay in `deploy/systemd/` as the reference below, for a host that
> runs menger as a service rather than the image.

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

Every `remote test` and `remote prod` (and, before DEPLOY-52, every local
`deploy test`, `sync test`, `promote` and `rollback`) tells menger what it did
(DEPLOY-12), in the deployment record below, made and sent by
`deploy/record` to `deployments.record` (DEPLOY-11) — so each ticket shows
where its work runs:

| the run | records (environment, revision) |
|---|---|
| begins | `started` — the release it moves to (`previous`: the release the environment last ran with its smoke passed) |
| infra deployed it, smoke passed | `succeeded` |
| infra failed, or the smoke failed | `failed` |
| dies after `started` (its log unwritable …) | `failed`, on its way out |

- **The commits** are `git log previous..revision` in the build checkout
  (`../menger-build`, else the repository), oldest first, when a run moves
  forward; a rollback carries none. Without them menger works from what it
  has seen.
- **The range starts at the last run that passed** (DEPLOY-23): `previous`
  of a remote test or remote prod (and of the retired deploy test, sync and
  promote) is the release `deployments.log` last
  shows the environment running with its smoke passed — a run that ended
  `ok`, or the release a promote's automatic rollback returned to — never a
  failed run, one that died, or a rollback whose smoke failed. So a deploy
  that fails its smoke does not lose its commits: the next one that passes
  carries them. An environment with no run in the log yet has none. The
  first `remote test` after DEPLOY-52 starts at the retired local test's last
  good release: `test`'s range runs on through the switch. `backfill` reads the log the same
  way; a rollback's `previous` stays the release it took out.
- **One run, one id.** Each run's records carry `source.run` =
  `remote-test@<when it began>#<pid>`, and so does the end of its line in
  `deployments.log`. `backfill` makes from that line the record the run sent
  (same environment, state, revision and run), which menger already keeps.
- **Never in the way.** Every record goes through the outbox
  (`~/.local/share/menger-deploy/outbox/`, `MENGER_DEPLOY_OUTBOX`), oldest
  first, each send bounded (`MENGER_DEPLOY_RECORD_TIMEOUT`, 10 s). One menger
  keeps (or already kept) leaves; at the first that gets no answer the rest
  wait for the next run, which sends them before its own. One menger refuses
  moves to `outbox/refused/` and is said, never retried. A record never
  fails a run.
- **Where** (DEPLOY-28). `MENGER_DEPLOY_TRACKER_URL`, default
  `wss://app.menger.sh/ws` — the tracker since RUN-13 — always signed in as
  the pipeline's key (below; with no key file the records wait, said).
  `MENGER_DEPLOY_TRACKER_URL=local` is this machine's tracker, as before
  RUN-13: `MENGER_DEPLOY_TRACKER_PORT` (4455), `MENGER_DEPLOY_TRACKER_HOST`,
  `MENGER_DEPLOY_TRACKER_TOKEN` (a token, through synapse — argv; prefer the key).
- **Complete on test** (DEPLOY-28, DEPLOY-22's rule: Complete means running
  on the lowest environment — app-test.menger.sh since DEPLOY-52). A
  `succeeded` record to `test` — a `remote test` whose smoke passed — once
  menger keeps it (or kept it
  already, so a drained outbox completes too), sets **Complete** on each
  ticket it `carried` that is In Review and runs its current version there:
  `deploy/record --complete` asks `tickets.nav.member` with `status "In
  Review" and deployed menger/test and not behind menger/test`, then one
  `tickets.update_many --status Complete`, its message `deploy menger/test
  <release> carries its work (DEPLOY-22)`. The pipeline's key is viewer +
  deployer, so that write signs in with `MENGER_DEPLOY_COMPLETE_KEY_FILE`
  (default `~/.plexus/keys/builder.key`, an editor's; its agent the file's
  name; `none`: records only). A refused completion is said, and the record
  stands; nothing else (prod, a failure, a rollback) moves a status.
- **app-test and app.menger.sh** (DEPLOY-28, DEPLOY-52): `remote test`
  records `test` (it recorded `app-test` until DEPLOY-52, when app-test became
  test) and `remote prod` records `app-prod` (from `test`; the record's `from`
  moved app-prod's source from `app-test`), each `started` then `succeeded` / `failed`,
  so a ticket's Running and `#/deployments` show where app.menger.sh runs
  it. `remote prod` ships the image test runs: its release is the one
  `deployments.log` last shows test running `ok`, and its log line names it.
- **Signed in as the pipeline's key** (ACCESS-47; prod requires login).
  Where `MENGER_DEPLOY_KEY_FILE` (default `~/.plexus/keys/pipeline.key`,
  `synapse _key new pipeline --store file`; `none`: no key) is a file, every
  record goes over the socket — `deploy/record --url
  $MENGER_DEPLOY_TRACKER_URL --agent pipeline` (`local`: `ws://127.0.0.1:$MENGER_DEPLOY_TRACKER_PORT`), whose
  `deploy/menger-call.mjs` reads the seed from the file (0600, or it is
  refused) and signs a key proof; the seed is in no environment and no argv.
  The key holds `viewer` on `*` and `deployer` on `MENGER` on app.menger.sh
  (the operator granted them at the switch), and reads `tickets.health` on
  app-test — the smoke's. A gate's refusal
  (the key not granted, or not admitted) is the tracker's setup, not the
  record's: the record waits in the outbox, said, and is never moved to
  `refused/`. No key file: synapse, anonymous, as before.
- **`status`** shows each environment's last record sent (`sent/<env>.json`)
  and how many wait in the outbox, and reads `tickets.health` signed in as
  the pipeline's key (ACCESS-47) on `wss://app-test.menger.sh/ws` and
  `wss://app.menger.sh/ws` — `health    no answer from wss://… (forbidden)`
  names a gate's refusal (the key not granted there yet).
- **`backfill`** replays `deployments.log` (`deploy/record --from
  pipeline-log`, each with its commits from git) through the outbox — the
  remote `run` lines too (DEPLOY-52; never `app-local`'s, an image's or a
  suite's).
  Idempotent: run it again and menger answers each record `duplicate`.

```sh
deploy/pipeline backfill        # backfill: 88 records … → record: 88 sent, 0 already kept, 0 refused, 0 waiting
deploy/pipeline backfill        # record: 0 sent, 88 already kept, 0 refused, 0 waiting
synapse -P 4455 menger deployments for_ticket --id MENGER-34
```

Tests: `deploy/tests/pipeline` runs the pipeline itself on a throwaway root —
`HOME` a scratch directory, hypermemetic-infra, the deployments as curl and
`deploy/menger-call.mjs` meet them, `synapse`, `docker` and `gh` stand-ins —
through every row above, the smoke (each check failing on its own), a tracker
down, hung, refusing and refusing the key, `remote prod`'s suite check, the
suite green and red in its own checkout, plexus-bench beside it (a stand-in
`scripts/gate.sh`: green, red, `MENGER_BENCH_DIR=none`), `build --no-gate`, `status`,
`backfill` and the retired commands; `cargo test` runs it.

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
a menger with `--require-login` has no REST bridge at all — and from
ACCESS-24 on, no menger has one (there is no open mode).

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
`MENGER_REQUEST=41` in the job's environment). menger's own receiver is
the deploy runner (below, DEPLOY-37). A pending request can be canceled
(`deployments cancel_request --id 41`); its delivery then stops. One an
executor has under way is asked to stop instead (`cancel_asked_at`).

**Steps** (DEPLOY-37). An executor reports each step against the request,
by a name it chooses, and menger draws them live (`#/deploy`'s → test):

```sh
synapse -P 4455 menger deployments step --request 41 --name build --state started --url https://ci.example.com/runs/9
synapse -P 4455 menger deployments step --request 41 --name build --state failed --detail @last-lines.txt
```

`--state` is `started`, `succeeded`, `failed`, `skipped` or `canceled`; a
step is kept by its name (a second report moves it), at most 32 a request,
its `--detail` cut to its last 4 KiB. The first step makes a pending
request `started` (and stops its webhook); a record naming the request
still says how it ended. Each answer is the request whole: when its
`cancel_asked_at` is set, the owner asked it to stop — stop, report the
step `canceled`, and record `failed`. A request may name a **branch**
instead of a revision (`deployments request --branch main`): the
executor resolves the tip, and its records carry the real revision.
`step` is the `deployer` grant, as `record` is.

## The deploy runner (DEPLOY-37)
**DEPLOY-42:** the runner's own closing record (when the pipeline's never landed) names the release too — read from the build/deploy output (`release <id>`, `test runs <id>`) — so `deployments environments` shows test's release after a runner-carried deploy, as one from a shell does.


`deploy/runner.mjs` is menger's own executor: it takes a request to `test`
(and to `app-prod`, below) and runs test's plan (DEPLOY-52), each a step
reported as the `pipeline` key (`MENGER_DEPLOY_KEY_FILE`):

1. `build` — `deploy/pipeline build <the branch's tip> --no-gate` (stopped on a cancel);
2. `image` — `IMAGE_STORE=ghcr deploy/pipeline image <the release it built>` (stopped on a cancel);
3. `deploy` — `deploy/pipeline remote test --image ghcr.io/hypermemetic-ai/menger:<release>`:
   app-test.menger.sh, then the smoke; its records name the request
   (`MENGER_REQUEST`), so it sets its tickets Complete as any deploy to test
   does, and it runs to its end once begun.

A build that fails or is canceled, the runner records `failed` itself. Then,
**apart from the request** (it ended with the deploy), **the suite**:
`deploy/pipeline suite <release>` on this machine, one at a time, the newest
release waiting (one passed over before it began never runs: `remote prod`
refuses it, and the newer one carries its commits). Red: the release is
marked `failed`, and the failing lines go to the triggering ticket's chat
with a gap blocking it, as a red run's do (below); green: one line in that
ticket's own chat. One request at a time; each step's output is in
`~/.local/state/menger-deploy-runner/runs/<request>/<step>.log`, the suite's
in `…/runs/<request>/suite.log`.

It takes requests two ways:

- `--watch` dials out: it holds `deployments.watch` on the tracker and
  takes each pending request to `test` (and those pending when it starts).
  **This is the mode for app.menger.sh**, which cannot reach this
  machine's loopback. No webhook is needed.
- `--listen` is the webhook's receiver on `127.0.0.1:4475`: the signature
  checked, `202` and queued (`401` unsigned, `422` a request it has no plan
  for), `GET /runs/<request>/<step>.log` each step's log (the step's url).
  For a tracker that can reach it — one on this machine, or an https route.

**Turning it on** (the owner, once; nothing is installed until then):

```sh
scripts/deploy-runner/install.sh --start     # the unit, ~/.config/menger/deploy-runner.env (--watch on
                                             # app.menger.sh), a random 0600 webhook secret
systemctl --user status menger-deploy-runner
journalctl --user -u menger-deploy-runner -f
```

With `--listen` (`MENGER_RUNNER_MODE=--listen` in the env file), set the
environment's webhook from the secret file — never typed, never printed:

```sh
node -e 'const fs=require("fs"),h=require("os").homedir();fs.writeFileSync("webhook.json",JSON.stringify({project:"MENGER",environment:"test",url:"http://127.0.0.1:4475/",secret:fs.readFileSync(h+"/.config/menger/deploy-runner.secret","utf8").trim()}),{mode:0o600})'
synapse -P 4455 -p @webhook.json menger deployments webhook set   # admin
```

Then **→ test** on `#/deploy` (admins of every project) files `request
--environment test --branch main` and draws the runner's steps as they come.
The runner's tests: `node --test deploy/tests/runner.test.mjs` (under
`cargo test`, `tests/deploy_record.rs`); the button against a throwaway
with the runner `--fake`: `scripts/deploy-button-check.mjs`.

### Automatic test (DEPLOY-48)

Owner, 2026-10-19: "we always instantly deploy everything to test. we
tolerate rollback. our system should do this automatically upon something
entering review." The runner also holds `tickets.watch` (as the pipeline
key, `viewer` on `*`) and files the test request itself
(`deployments.request --environment test --branch main`, its `deployer`
grant) — **for test only**; prod stays the owner's (below).

- **Trigger:** a ticket of the project — MENGER or any shortcode hosted
  under it, by its address — entering **In Review**, or a commit linked
  (`tickets commits link`) to one that is In Review. Leaving review triggers
  nothing; a ticket's commits while Coding go when it re-enters.
- **Debounce:** the request is filed 60 s (`MENGER_RUNNER_DEBOUNCE_SECS`)
  after the last trigger; each trigger restarts the wait.
- **Coalesce:** one run at a time and at most one pending request. When a
  test request is already pending — queued here or on the tracker (a
  `→ test`) — the triggers fold into it; it resolves main's tip when it
  starts. The request under way is never folded into.
- **Plan:** test's, as → test runs it (DEPLOY-52: build without the gate →
  image → app-test.menger.sh and its smoke), so a merge set In Review is live
  on app-test within minutes, the suite in nobody's way; the deploy sets
  Complete (DEPLOY-22/28) as any does, and the suite's result arrives after
  it — on the release (`SUITE`, `deploy/pipeline releases`), in the ticket's
  chat, and in `deploy/pipeline status`.
- **Red:** a red build, image or deploy is recorded `failed`; a red suite
  marks the release (the deploy stands: test runs it). Either way the failing
  lines (cargo's `FAILED` / `panicked`, else the tail) are posted to the open
  chat of the first triggering ticket, else `MENGER_RUNNER_CHAT` (6), and a
  gap is filed under it blocking every triggering ticket (`suite red:
  <release> …` for a suite) — both as the editor key
  (`MENGER_RUNNER_EDITOR_KEY_FILE`, `~/.plexus/keys/builder.key`; `none`:
  neither).
- **Off:** `MENGER_RUNNER_AUTO_TEST=0` in `~/.config/menger/deploy-runner.env`
  (or `--no-auto-test`), then `systemctl --user restart
  menger-deploy-runner`. `deploy/pipeline status` says `auto-test on|off`
  and, from `~/.local/state/menger-deploy-runner/auto.json`, the request
  running, the one pending (and for which tickets), what waits out the
  debounce, the last run, and the suite running, waiting and last.
- **Tracking:** a ticket's work is its branch's commits; merges are
  `--no-ff`, so each stays in main with its hash and "is X on test" is "is
  X's newest linked commit an ancestor of what test runs" (DEPLOY-46).
  Never squash or rebase on merge.

Its tests are in `deploy/tests/runner.test.mjs` (trigger, debounce,
coalesce, re-entering review, the off switch, a red run; DEPLOY-52: the plan
with no gate, a build naming no release, the suite after a green deploy —
green and red — one suite at a time) and
`deploy/tests/pipeline` (the status lines).

**→ prod** (DEPLOY-40) promotes the release test passed to the page's
destination — `app-prod` (app.menger.sh) since RUN-13; this machine's prod is
retired (DEPLOY-52). It is shown to an
admin or a holder of `deployer` on the project, labelled `→ prod ·
<release>` (what test runs), and off with its reason when test has nothing
newer than the destination, test's last deploy did not succeed, or a
request to it is open. Pressing it opens a confirm sheet — the release, its
commit, and the tickets `ready for` app-test and app-prod, grouped by area —
and Promote files `request --environment app-prod --release <release>`.
The runner's `app-prod` plan, each a step:

1. `image` — `IMAGE_STORE=ghcr deploy/pipeline image <release>` (stopped on a cancel);
2. `app-test` — `deploy/pipeline remote test --image ghcr.io/hypermemetic-ai/menger:<release>`;
3. `app-prod` — `deploy/pipeline remote prod` (infra's promote.yml: what app-test runs);
   its records name the request (`MENGER_REQUEST`), so the request ends with the real deploy.

A failed step stops the plan, the rest skipped: app-prod keeps what it ran.
**A request to app-prod is the owner's** (owner, 2026-10-19: "Deployments
should be triggered by me"): the runner takes it only when its requester is
one of `MENGER_RUNNER_OWNERS` (comma-separated principals, in
`~/.config/menger/deploy-runner.env`; `--owner` on the command line) and
never a `key:` or `agent:` principal, whatever that says, only for a
release with a `succeeded` record to test, and only when its suite passed
(DEPLOY-52: `deploy/pipeline suite --status`; red, running or none is
refused, and `remote prod` refuses it again). Otherwise its first step fails
with the refusal (`refused: key:… is a key — …`) and nothing runs. With
`MENGER_RUNNER_OWNERS` empty every app-prod request is refused — **turning
it on** is that line set to the owner's principal (the WorkOS one in
`MENGER_ADMINS`), then `systemctl --user restart menger-deploy-runner`.
With `--listen`, app-prod's webhook is set as test's above, with
`environment: "app-prod"`; with `--watch` nothing more is needed.

Checked on a throwaway, never the real environments:
`scripts/prod-button-check.sh` starts menger with its own database, a
loopback OIDC issuer for a synthetic owner (`scripts/fixture-idp.mjs` — the
runner refuses keys, so the owner must be a person) and the runner
`--listen --fake --owner idp:…`, and runs `scripts/prod-button-check.mjs`
at 390 and 1440, light and dark: an agent's request refused, a failing step
stopping the plan, the three steps drawn live to the end, the button off
with its reason. The plan itself, over `deploy/tests/fake-pipeline`, is in
`deploy/tests/runner.test.mjs`.

**Later: hyperforge** (HYPERFORGE-10). The runner is one executor behind a
seam menger keeps generic — a request (revision or branch), its signed
webhook, `deployments.step` and `record --request`, and `cancel_asked_at`.
hyperforge's `deploy.*` takes the same payload at its own URL; moving to it
is `webhook set` with that URL and stopping the runner. Nothing in menger
changes.

## Where things are

- releases: `~/.local/share/menger-deploy/releases/` (each with its `SUITE` once one ran, `TESTED` once test passed it)
- the suite's checkout: `../menger-suite` (DEPLOY-52); the build's: `../menger-build`
- every build, image, remote run, suite and secret: `~/.local/share/menger-deploy/deployments.log`
- deployment records waiting for menger: `~/.local/share/menger-deploy/outbox/`
  (refused: `outbox/refused/`); the last sent per env: `…/menger-deploy/sent/`
- logs: `~/.local/state/menger-deploy/logs/`; the runner's `~/.local/state/menger-deploy-runner/runs/`;
  the boxes': `../hypermemetic-infra/bin/infra status.yml`
- the retired local environments' data, untouched: "The local environments, retired" above
- the build-output sweep (CI-2): `menger-sweep.timer`, hourly, from
  `~/.local/share/menger-sweep` (`scripts/sweep/install.sh --start`); what it
  removed in `~/.local/state/menger/cleanup-gate.log`, the run in
  `journalctl --user -u menger-sweep`

## Rules

- Production changes only through `remote prod`. Nothing runs production from a
  development checkout; `cargo build` or `trunk build` in `menger/` changes
  nothing that is serving.
- Work is checked on test (https://app-test.menger.sh) or on a throwaway
  server — never on prod. The suite never runs against a deployment.
- A release that failed test, or whose suite failed, is never promoted; fix
  forward with a new commit.
