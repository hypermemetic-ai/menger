# Deploying menger

menger runs in two environments, **test** (https://app-test.menger.sh) and
**prod** (https://app.menger.sh), both on the Linode as containers of one
image, through hypermemetic-infra ("On the Linode", below). This machine
builds the releases and their images and runs the suite; it runs no
environment (DEPLOY-52). Production changes only through this pipeline
(DEPLOY-1): `deploy/pipeline`, its record `deploy/record` and its executor
`deploy/runner.mjs`; the bundle is built, signed and pushed by `../plexus-ui`'s
`plexus-ui` tool (FRAME-8).

What v2 cut from this file is at tag v0.0.1: the deploy page and its → test / → prod buttons, the request webhook, the activation that kept records and requests, the landing's media regenerator and mock, the local smoke's screenshots, and the diagrams service's release and units — read v1 there, never here.

```
a branch's tip → build (no gate) → release → image (GHCR) → remote test → bundle pushed → smoke → live on app-test.menger.sh
                                                                 ↓ red                               ↓
                                                         prod untouched           the suite, on this machine, at the release's commit
                                                                                                     ↓ green: the release may go to prod
                                                                                                     ↓ red: marked; remote prod refuses it
                                                       remote prod (the owner's) → smoke → live on app.menger.sh
```

## The environments

| | test | prod |
|---|---|---|
| where | https://app-test.menger.sh (`wss://app-test.menger.sh/ws`) | https://app.menger.sh (`wss://app.menger.sh/ws`) |
| deployed by | `deploy/pipeline remote test --image …` (infra's `app.yml`) | `deploy/pipeline remote prod` (infra's `promote.yml`: what test runs) |
| recorded as | `test` | `app-prod`, promoting from `test` |
| its data | its own tracker, in the box's volume | the tracker since RUN-13 |
| its UI | UI-15: one component store, `menger-shared`'s `/shared/ui.db`, on the channel `test` — a deploy pushes the bundle here | the same file, on the channel `prod` — `remote prod` promotes test's live version onto it, no second push |
| settings | `deploy/env/app-test.env` | `deploy/env/app-prod.env` |

**What each runs** is what `deploy/pipeline status` says — ask it, never
this file. On 2026-10-29 test ran v2 (`20261029T174244Z-52c9537`, built
with `--branch v2`, its `ui` service live with the bundle that release
pushed) and prod ran main's v1 (`20261029T101350Z-e910781`); prod moves
only when the owner promotes.

**Check work on test, signed in as your key there** — once per machine (a
session is the host's own), then as usual:

```sh
synapse --url wss://app-test.menger.sh/ws _login menger --key builder --as builder
synapse --url wss://app-test.menger.sh/ws --as builder menger tickets health
synapse --url wss://app-test.menger.sh/ws --as builder menger ui versions
```

Test is where people and agents work against a deploy; the suite never
does: it runs on this machine, against the release's own tree, and writes
nothing into app-test's tracker. `deploy/pipeline local` (RUN-25's
`menger-app-local` on 127.0.0.1:4470/4471) is a CI throwaway, not an
environment.

### The local environments, retired (DEPLOY-52)

This machine's own test (:4450–4453) and prod (:4455–4458) were stopped and
their units uninstalled on 2026-10-23. `deploy/pipeline install`, `deploy/pipeline deploy test`,
`deploy/pipeline sync`, `deploy/pipeline promote`, `deploy/pipeline rollback`
and `deploy/pipeline logs` say so and exit 2; so does `deploy/pipeline media`,
retired on v2 (V2-9: landing/media was captured from v1's app and stays as
committed until the landing is redone for v2). **Their data stays where it
was, untouched** — retiring a service is reversible, deleting a tracker is
not:

| what | where |
|---|---|
| prod's database (the move's source, RUN-13) | `~/.local/share/menger/tickets.db` (+ `-wal`, `-shm`) |
| prod's backups, the move's own among them | `~/.local/share/menger/backups/` (`tickets.db.<when>-move-RUN-13`) |
| test's copy and its backups | `~/.local/share/menger-test/` |
| their settings | `~/.config/menger/{prod,test}{,.local,.smoke}.env` |
| the uninstalled unit files | `~/.local/share/menger-deploy/retired-units/DEPLOY-52-2026-10-23/` — moved, not deleted |

## The pipeline

```sh
deploy/pipeline build [REV] [--branch B] [--no-gate] [--fast]
                                                       # a commit → a release (prints its name); --branch B: B's tip,
                                                       # named in the release; --no-gate: the suite comes after;
                                                       # CI-9: --fast, test's quick build (never a prod candidate)
deploy/pipeline image [RELEASE]                        # RUN-10: the release as an image (IMAGE_STORE=ghcr pushes it to GHCR)
deploy/pipeline remote test --image IMG                # app-test.menger.sh through infra, the bundle pushed, then the smoke
deploy/pipeline suite RELEASE                          # DEPLOY-52: the full suite at the release's commit; marks the release
deploy/pipeline perf RELEASE                           # perf, apart from the suite: plexus-bench's counts and bench against their
                                                       # goldens; marks the release's PERF; nothing reads it to decide a deploy
deploy/pipeline suite --status RELEASE                 # suite: passed | failed | running | none
deploy/pipeline remote prod                            # what test runs → app.menger.sh, only once its suite passed
deploy/pipeline smoke test|prod                        # the smoke against app-test / app.menger.sh, on its own
deploy/pipeline local --image IMG                      # RUN-25: the image here as menger-app-local, 127.0.0.1:4470/4471
deploy/pipeline status                                 # what test and prod run, their suites, health, last records; the runner
                                                       # and its branch; the disk; the newest log
deploy/pipeline releases                               # every release: its suite, tested?, where it runs
deploy/pipeline backfill                               # deployments.log → the tracker's records (idempotent)
deploy/pipeline landing [REV] [--check]                # landing/ at REV (default main) → gate → the edge (menger.sh)
deploy/pipeline ui [--watch]                           # UI-17: ui/ of this checkout, committed or not, live on test: no image, no cargo
deploy/pipeline ui promote                             # UI-17: what test has live onto prod, a pointer move (the owner's)
deploy/pipeline remote-secret test|prod                # IDENTITY-45: the WorkOS API key on the box, from a pipe
```

The deploy runner does build, image and remote test by itself for every
merge set In Review, then the suite ("The deploy runner", below); prod is
the owner's.

1. **Build** (`build [REV] [--branch B] [--no-gate]`). `--branch B` names
   the branch the release is of — its `RELEASE` says `branch: B` — and,
   with no REV, is the revision too: B's tip; with neither, `main`'s tip. A commit,
   never a working tree: `../menger-build` (a sibling checkout, so menger's
   relative plexus dependencies resolve; `MENGER_BUILD_CHECKOUT` names
   another, `MENGER_BUILD_TARGET` its own cargo target) is moved to the
   commit and cleaned. A low disk refuses it before anything runs (CI-2:
   `scripts/disk-check.sh` — under 50 GB free or 90% used).
2. **Gate** — only without `--no-gate`: the workspace's tests at that
   commit, as the suite runs them (step 5). Red: no release. The runner builds with `--no-gate`: the suite
   runs after the deploy (step 5).
3. **Release.** A tree with no `frontend/` (v2) has no browser toolchain on
   this machine, and needs none — every step runs here, none in a container
   (FRAME-8):
   - `cargo build --profile perf -p menger` (the workspace's `menger`, read from
     `cargo metadata`) — a release is the perf build (RUN-39: `[profile.perf]`,
     PERF-6's "the one build that perf runs and releases use", so the binary
     test and prod run is the one the suite's bench holds to its goldens);
     `RELEASE` says `profile: perf`. A tree older than PERF-6 defines no perf
     profile and builds the release profile, said `profile: release`. A tree
     with no `ui/src` is refused. **`--fast`** (CI-9; the owner, 2026-11-05:
     "sub 5 minutes into test … ideally sub 30 seconds") builds
     `[profile.fast]` instead — release's optimisation without the perf
     profile's fat LTO and single codegen unit, incremental, in the build
     checkout's warm target: about 15–35 s of cargo where perf takes 5–8
     minutes, 1m31s from a commit to test against 6m23s. `RELEASE` says
     `profile: fast (CI-9: test's quick build — never a prod candidate)`;
     `remote prod` refuses a fast release, and its suite runs no bench (the
     goldens are the perf build's). A prod candidate is the perf build:
     `build` without `--fast`, image, `remote test`, `suite`, then `remote
     prod`. A tree with no `[profile.fast]` builds perf, and says so;
   - `plexus-ui` — the framework's tool, `cargo build --release
     --manifest-path $PUI/Cargo.toml -p plexus-ui-cli`, `$PUI` the
     framework's checkout the tree's `scripts/plexus-ui-src` names (V2-47):
     the tag `plexus-ui-v<version>` of the `plexus-ui-service` version its
     Cargo.lock takes, cloned once into `~/.cache/plexus-ui` and refused when
     no longer the tag's — never `../plexus-ui`'s working tree, and never
     `PLEXUS_UI_SRC`; a tree before V2-47 (no `scripts/plexus-ui-src`) builds
     from `../plexus-ui` as it did (`MENGER_PLEXUS_UI` names a tool built
     already). CI-9: the tool of a clean checkout is kept per framework
     commit, `~/.cache/menger-deploy/plexus-ui-tool/<commit>/plexus-ui`
     (`MENGER_PLEXUS_UI_CACHE`), and built once, not per release;
   - `ui-prebuild` — `plexus-ui build --prebuild --engine dist/engine`: the
     engine (wasm32, wasm-bindgen);
   - `ui-uses` (SURFACE-11) — the bundle built once unstamped
     (`plexus-ui build ui --engine dist/engine --commit menger@<short>`)
     for its manifest: every method its
     components use (`components.*.uses`), read from the tar;
   - `contract` (UI-12, SURFACE-11) — the contract over every service the
     bundle names, as `ui.push` computes it (`contract_named` in
     `../plexus-ui/service/src/push.rs`), never the tickets service's hash
     alone: the `menger` just built is started on two free loopback ports
     in local mode (no login provider: it makes its own admin key in a
     scratch data directory, so the build holds no key material), under a
     scratch `HOME` and `XDG_RUNTIME_DIR`; synapse signs in with that key
     as `menger-local` and asks `ui contract --uses` the bundle's methods
     (`--no-hold`, the list comma-separated) — and the
     binary is stopped and the scratch removed — always, a failed build
     too. A binary that exits, does not answer within
     `MENGER_CONTRACT_WAIT` seconds (60), or a `ui contract` that answers no
     hash, a refusal (`unknown_method`) or `null` (a bundle naming no
     service: the app's) fails the build, named, as does a bundle whose
     first build left no readable `manifest.json`; the log says each hash and the
     services it covers (`contract: H over orgs, tickets`). A bundle naming
     the tickets service alone gets the hash `tickets.schema` answered;
   - `ui-build` — `plexus-ui build ui --engine dist/engine --contract HASH
     --commit menger@<short>`: every component compiled in-process, the
     manifest naming that hash as its `contract`, and the tar,
     `dist/ui.tar` (`../plexus-ui/CONTRACT.md`'s bytes), which must hold
     `index.html`; no socket asked;
   - SV-6: **no studio step.** The studio is plexus-studio's (SV-5): it is
     seeded once from that repository (its own build script, then
     `plexus-ui push --no-live`, the bundle named `studio`) and from then on
     edited in place in the database (`ui.author { bundle: "studio" }`).
     A build makes no `dist/studio.tar`, and a deploy pushes none — not even
     an older release's, whose push would move `bundle:studio` off what was
     authored. The bundle is `svelte/1` alone (SV-4), and the tool refuses
     an rhx source (SV-6; no `--allow-rhx` here);
   - `ui-compiler-wasm` — the framework's `$PUI/scripts/ui-compiler-wasm.sh
     dist/ui-compiler.wasm` (rustup's `wasm32-unknown-unknown`; FRAME-7):
     the compiler module. A release of v2 always carries it: no script, a
     failed one, or no module after it fails the build, named.

   The release is `~/.local/share/menger-deploy/releases/<UTC time>-<sha>/`:

   ```
   bin/menger                  the binary
   bin/run-menger              its launcher (deploy/launch/run-menger)
   bin/plexus-ui               the tool that built the bundle, and pushes it (not in the image)
   dist/ui.tar                 the UI's bundle, pushed after the deploy
   dist/ui-compiler.wasm       the compiler module, --ui-compiler
   RELEASE                     release, commit, branch, subject, built, rustc, profile, steps, gate
   SUITE, TESTED               marks, once its suite ran and once test passed it
   ```

   Read-only; never rebuilt. Prod runs exactly the bytes test ran. The
   newest ten, and those test and prod run (and ran last before), are kept.
   A tree that tracks `frontend/` (main, v1) builds its browser app as it
   did at tag v0.0.1, into `app/` beside `bin/`; the pipeline tells the two
   apart by what the commit tracks, never by a directory left on disk.
4. **Image and deploy to test.** `IMAGE_STORE=ghcr deploy/pipeline image
   <release>` (below), then `deploy/pipeline remote test --image
   ghcr.io/hypermemetic-ai/menger:<release>`: infra's `app.yml` deploys it
   to app-test.menger.sh; once `tickets.health` answers, the bundle is
   pushed ("The bundle's push", below); then the smoke. Passing marks the
   release `TESTED` and records `test succeeded`, which sets Complete on the
   In Review work it carries (DEPLOY-22/28).
5. **The suite** (`suite RELEASE`, DEPLOY-52). The workspace's tests at
   the release's commit — on v2 that is the whole of it (a frontend is one the
   commit tracks: an untracked `frontend/target/` left in the checkout ran the
   root package's tests a second time until CI-9) — under **cargo-nextest**
   (CI-9: `cargo nextest run --workspace --no-fail-fast`, every test of every
   binary at once, then `cargo test --workspace --doc`; `.config/nextest.toml`
   starts the long poles first; `MENGER_GATE_RUNNER=cargo`, or no
   cargo-nextest installed, is libtest's `cargo test --workspace` as before;
   the counts read both), about 4 minutes on 12 cores where cargo test took
   15 — in its own sibling
   checkout `../menger-suite` (`MENGER_SUITE_CHECKOUT`; its own `target/`),
   so a suite under way never holds up the next build; one at a time (a
   lock). It never touches a deployment. The release's `SUITE` says
   `running …`, then `passed …` with the counts or `failed …` with the log
   (a suite killed half-way is `failed`), and `deployments.log` gets `suite
   test <release> passed|failed`. A ticket's gate is a selection (`cargo
   test -p <crate> --test <name>`), which never resolves what only the whole
   workspace reads — an example's `path`, another member's manifest; the
   suite does: `20261029T173657Z-454cd56`'s went red before a test ran on an
   example path V2-9 had removed (V2-12; `tickets-api/tests/leftovers.rs`
   now holds every manifest `path` to a file in the tree). **Perf is apart**
   (the owner, 2026-11-12: "we want a perf but not in the standard tests.
   they should be run separately"): `deploy/pipeline perf RELEASE` runs
   plexus-bench and marks the release's `PERF`; the suite runs none of it, its
   `SUITE` line names no bench, and `remote prod` never reads `PERF`. What
   follows is what `perf` does, where it says the suite read perf.
   **plexus-bench** (PERF-6, PERF.115-29; V2-44 brought both to v2). First its
   `../plexus-bench/scripts/ci-check.sh`: every scenario's instructions and allocations
   against `goldens/ci`, under callgrind in the `plexus-bench-ci` image. They
   read no clock, so a count past its line is not noise, and it marks the
   release `failed … — counts red; log …` and logs `suite test <release>
   failed counts`. A check that could not run is red too: a release passes on
   a count read. Then its `../plexus-bench/scripts/gate.sh` runs every PERF scenario under
   the perf profile and `plexus-bench check` against the goldens (a red on a
   quiet first pass runs again, three times at most, until a quiet read
   answers it; a red taken on a busy machine is said, not failed), with
   `schema.*` measuring the release's own `bin/menger` — the perf build, as
   the goldens were taken from it (RUN-39). A red bench marks the release
   `failed … — bench red; log …` and logs `suite test <release> failed
   bench`, as a red test does; green, the `SUITE` line ends `; bench
   <check's verdict>; counts <check --ci's verdict>`. `MENGER_BENCH_DIR`
   names the checkout (default `../plexus-bench`; `none` skips both and the
   line says `bench not run`), `MENGER_BENCH_TARGET` their cargo target
   (`~/.cache/cargo-target/plexus-bench`).
6. **Promote** (`remote prod`). infra's `promote.yml` ships the image test
   runs — the release `deployments.log` last shows test running `ok` — and
   only when that release's suite `passed`: `failed`, `running` or no result
   is refused before infra is asked, naming why. Then, for a release that
   carries a bundle, **the UI's promote** (UI-15): once `tickets.health`
   answers on app.menger.sh, `ui promote --to prod --from test` as the
   pipeline key on prod's socket — prod's live pointer onto the version
   test has live, in the one store both environments mount, no second push;
   the hash promoted is said (`ok   promoted test's live version to prod …`)
   and ends the log line (`app-prod run <release> ok promoted <hash>`). What
   test has live is what prod takes: the release's bundle, or an author's
   version made on test since (the studio's), said as such. A refused
   promote fails the deploy, named and recorded (`promote failed: …`), and
   moves nothing; prod's image is still the new one (`bin/infra
   rollback.yml` returns it). Then the smoke against app.menger.sh, wanting
   the promoted version live and `/app/` serving the bundle's page; recorded
   as `app-prod` (from `test`). A failed smoke is recorded `failed`;
   `bin/infra rollback.yml -e app_dir=<checkout>` puts prod back on its
   previous image ("On the Linode"). prod takes no push, so
   `deploy/env/app-prod.env` names no `MENGER_UI_SIGNERS`; it names the
   store and its channel (`MENGER_UI_DB_PATH=/shared/ui.db`,
   `MENGER_UI_CHANNEL=prod`), as test's names its (`test`). The pipeline key
   must hold `deployer` on `*` on prod, as it does on test (the operator
   grants it once).

### The bundle's push (V2-7)

The image copies `dist/` to `/opt/menger/dist`, and `run-menger` starts the
`ui` service with `--ui-db-path` (`MENGER_UI_DB_PATH`, else `ui.db` beside
the database, in the data volume — UI-15: on the box, `/shared/ui.db`, the
one store both environments mount; the channel is `MENGER_UI_CHANNEL`,
read by menger itself), one `--ui-signer` per key of
`MENGER_UI_SIGNERS` (`;`-separated 64-hex public keys) and `--ui-compiler`
when the module is there. A service names its signers at start: app-test's
line in `deploy/env/app-test.env` is the pipeline key's public key, written
by hand, never by a test; prod names none (UI-15: it takes no push).

V2-24: the media service keeps its file beside the database in the data
volume by default (`media.db`; `--media-db-path`, `MENGER_MEDIA_DB_PATH`
to name another) — nothing in the env files or `run-menger` for it.
V2-29: the docs service alike — `docs.db` beside the database
(`--docs-db-path`, `MENGER_DOCS_DB_PATH`); a release carries it, so the
first deploy after it opens an empty `docs.db` in the data volume.

After a deploy is healthy — `remote test`, and `local` — the pipeline
pushes the release's bundle: `ui.push { bundle, manifest, signature,
commit }` by `plexus-ui push` (the release's own `bin/plexus-ui`, else
`MENGER_PLEXUS_UI`, else `plexus-ui` on PATH) as the `pipeline` agent — a
`deployer` on test (the role that holds `ui.push` and nothing else), the
admin of `local`'s container — signed in at the socket's upgrade with the
pipeline key. The tool makes the params: the tar in base64, the bundle's own
`manifest.json`, and an Ed25519 signature over the bundle's sha256 with the
pipeline key's seed, read from `MENGER_DEPLOY_KEY_FILE` (`--key-file`, never
the seed in argv). The
service keeps a version once per hash (the same bytes again are `new:
false`) and moves its live pointer in the same write. It answers
`contract_matches` (UI-11): whether the running services are the ones the
bundle names (its hash over the services its `uses` name; SURFACE-11). Only `true` passes (UI-12) — the push line says
`contract_matches: true`; `null` (a bundle saying `unknown`) or `false` (a
bundle built against another service, a hand-built tar pushed to another
release) fails the deploy, named on stderr and in deployments.log, recorded
`failed`.

The smoke then wants that version live in `ui versions` and reads `/app/`
signed in (a session from `auth.login`, its cookie in a 0600 header file)
for the bundle's own `index.html`, stamps filled. A refused push fails the
deploy — recorded `failed`, the reason (`bad_signature`, `no_signer`, …) on
stderr and in deployments.log — and leaves live the version that was live;
the pipeline never calls `ui.rollback`.

**Prod is a promote, not a push** (UI-15). The store is one file for both
environments (infra's `menger-shared` volume, `/shared/ui.db`; `deploy/app.yml`
`shared`), test on the channel `test`, prod on `prod`: a push on test moves
`live:test` alone, and `remote prod` calls `ui.promote { to: prod, from:
test }` as the pipeline key on prod's socket — `deploy/menger-call.mjs`, no
bundle on the wire — which moves `live:prod` onto test's live version in one
write; prod's open pages hear `live` and swap. The smoke on prod then wants
that version live and `/app/` serving the bundle's page. The UI alone goes
back on prod with `ui rollback --to <version> --channel prod` (an admin's,
from either socket), or forward again with another promote.

### The smoke

`smoke test|prod`, and `remote` after infra: from outside, as a person or
an agent meets the deploy, reading only — `http://` redirects to `https://`;
`/` is behind the login page (303 to `/login`) with HSTS
(`max-age=31536000`); `/login` is the sign-in page; RUN-9's
`/.well-known/oauth-protected-resource/ws` names the host's `/ws`; a socket
upgrade from another origin is refused (403); `tickets.health` answers on
`wss://<host>/ws` signed in as the pipeline key (retried for
`MENGER_SMOKE_WAIT`, 60 s, after a restart; no key: the smoke fails); an
anonymous socket reaches nothing. On test, for a v2 release, the bundle's
checks above; for a v1 release, its app's script served. It writes nothing.
`DEPLOY_SMOKE_FAIL=test|prod` fails it on purpose (a drill).

**A deploy keeps every session** (FRAME-21). `remote test` signs in on the
release test runs just before infra deploys (`auth.login {session}` as the
pipeline key, 15 minutes), keeps the token as a cookie in a 0600 header file
(never argv), and after the smoke wants `/login/me` to answer 200 with it,
then signs it out (`POST /logout`). A release that does not descend from
the commit that keeps sessions (`MENGER_SESSIONS_KEPT_SINCE`; by default
FRAME-21's) kept them in memory: the check is skipped, and said. A session
gone after the deploy fails it, recorded `sessions lost`.

### Logs (CI-6)

Every log a run writes is a file of its own, never overwritten:
`~/.local/state/menger-deploy/logs/<UTC yyyymmddThhmmssZ>-<short sha>-<step>.log`
(`MENGER_DEPLOY_LOGS` moves the directory). `build` keeps `gate-workspace`,
`build-release`, `build-ui-prebuild`, `build-ui` and `build-ui-compiler`
apart; `build`, `suite`, `smoke`, `landing`, `image`, `local` and `remote`
also keep everything they said as `<stamp>-<command>[-<arg>].log`. Each
says `log: <path>` as it opens one, and a failure names the file. The gate
runs `cargo test --workspace --no-fail-fast`, so one red shows every
failing test, each printed `FAILED <test>`. A run that fails is marked
(`<stamp>.red` beside its logs). `deploy/pipeline status` prints the newest
log. Retention: the hourly sweep (`scripts/cleanup-gate.sh --sweep`)
removes logs older than 60 days and never a marked run's.

### The landing page (DEPLOY-17)

menger.sh is `landing/` at one commit, pinned in hypermemetic-infra
(`menger_landing_commit` in `inventory/group_vars/all/main.yml`) and read by
the edge role from this checkout with `git show` and `git archive` — never a
working tree. `deploy/pipeline landing [REV]` (default `main`) takes a
commit there in one run:

1. **Export.** `landing/` at the commit, into a scratch directory.
2. **Gate.** `main.js` parses; neither page carries an inline style or
   script (the CSP allows none); every file a page names on its own origin
   is in the commit; `deploy/landing-check.mjs` serves the export and loads
   `/` and `/guide/` in Chromium at 390 and 1440: no console error, page
   error, failed request, answer of 400 or more, horizontal scroll or
   missing title.
3. **Pin and apply.** The pin set to the commit, `bin/infra edge.yml` run
   with this checkout as infra's menger source. The role validates, writes,
   reloads, and on a failed check puts the old files back by itself; then
   the pipeline restores the pin, records `failed` and exits 1.
4. **Verify.** `https://menger.sh/` (`MENGER_LANDING_URL`; `none` skips it)
   must serve the commit's `index.html` byte for byte.
5. **Commit and record.** The pin committed in infra (`menger.sh at <sha> —
   <subject> (deploy/pipeline landing)`); one line in `deployments.log`;
   `started` then `succeeded` or `failed` recorded on the tracker as project
   `menger`, environment `edge`.

`landing --check` runs the gate and infra's dry run (`edge.yml --check
--diff`) and writes nothing. `status` adds an `edge` block: the pin, how
many commits on main touch `landing/` since it, whether the live page
answers, the last record sent. The stills and clips in `landing/media/` are
v1's app and stay as committed (`media` is retired, above). Infra is found at
`MENGER_INFRA_DIR` (default `../hypermemetic-infra`).

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
from a checkout — the image comes before the test, so what test runs and
prod is promoted to is that very image. `deploy/pipeline image [RELEASE]`
stages the release's `bin/menger`, `bin/run-menger`, `dist/` (v2) or `app/`
(v1) and `RELEASE` beside `deploy/image/Dockerfile` and builds it.

```sh
deploy/pipeline image                              # the newest release → menger:<release> in this machine's Docker
deploy/pipeline image 20261029T174244Z-52c9537     # a named one
IMAGE_STORE=ghcr deploy/pipeline image R           # ghcr.io/hypermemetic-ai/menger:<release> and :latest, pushed
```

- **Which release.** The one named, else the newest built. A release whose
  menger predates `--bind` (RUN-8) is refused: bound to loopback inside a
  container, it could not be reached.
- **Where it is kept** is `IMAGE_STORE`: `local` (the default) tags
  `menger:<release>` (`MENGER_IMAGE_LOCAL` names the repository); `ghcr`
  tags `ghcr.io/hypermemetic-ai/menger:<release>` and `:latest`
  (`MENGER_IMAGE_GHCR`) and pushes both, Docker signed in with `gh auth
  token` on stdin (this machine's token has `write:packages`).
- **What it answers**: `image=REF` and `release=NAME` on stdout, and the
  same lines appended to `$HF_OUTPUTS` when it is set (hyperforge's stage
  outputs). Each run is a line in `deployments.log` (`image  local|ghcr
  RELEASE  ok REF`, or `build failed` / `push failed`), not a deploy.

**Inside the image**: `debian:trixie-slim` (glibc 2.41, at least what the
release binary links, DEPLOY-51), user `menger` (uid 10001), `tini` as
PID 1, menger bound to `0.0.0.0` (`MENGER_BIND`), the WebSocket on 8080 and
the shell on 8081 (`MENGER_WS_PORT`, `MENGER_SHELL_PORT`), the database at
`/data/tickets.db` and the `ui` service's at `/data/ui.db` in the `/data`
volume, `PLEXUS_NO_REGISTRY=1`. All of menger's settings are its
environment (`MENGER_*`; `--help-all` names each). Binding `0.0.0.0` adds no
name (ACCESS-25), so every name the container is reached by is an
`--allowed-host` (`MENGER_ALLOWED_HOSTS`) or `MENGER_PUBLIC_URL`'s, and a
bind beyond loopback needs a public URL (ACCESS-30).

## The menger-app pipeline (RUN-25)

The image built, stored and deployed as a pipeline on hyperforge's `ci`,
declared in `.hyperforge/pipelines.toml`. Each stage is a `deploy/pipeline`
subcommand, so the same step runs from a shell without hyperforge:

| stage | trigger | needs | runs | outputs |
| -- | -- | -- | -- | -- |
| `build` | auto | — | `deploy/pipeline image` — the newest release as `menger:<release>` | `image`, `release` |
| `deploy-local` | manual | build | `deploy/pipeline local --image "$IMAGE"` | `url` |
| `deploy-test` | manual | build | `deploy/pipeline remote test --image "$IMAGE"` | `url` |
| `deploy-prod` | manual | deploy-test | `deploy/pipeline remote prod` | `url` |

```sh
synapse -P 44104 lforge-v5 hyperforge ci menger-app start                                 # build → run N
synapse -P 44104 lforge-v5 hyperforge ci menger-app get --run N                           # each stage's state and outputs
synapse -P 44104 lforge-v5 hyperforge ci menger-app trigger --run N --stage deploy-local
```

- **`local --image IMAGE`** (or `$IMAGE`) runs the image on this machine as
  the container `menger-app-local`, its database in the volume
  `menger-app-local-data`, the WebSocket on `127.0.0.1:4470` and the shell
  on `127.0.0.1:4471` (`MENGER_LOCAL_WS_PORT`, `MENGER_LOCAL_SHELL_PORT`;
  4450–4462 are refused). It needs the pipeline's key: the container starts
  with the `key` provider, the pipeline's principal as `MENGER_ADMINS`, its
  public key as `MENGER_UI_SIGNERS`, and
  `MENGER_PUBLIC_URL=http://localhost:<shell port>` (the image binds
  0.0.0.0, which needs a public URL; `localhost` is a LAN name, served with
  a warning). It waits (90 s, `MENGER_LOCAL_WAIT`) for `tickets.health` on
  4470 and the sign-in page on 4471, pushes the release's bundle and runs
  the bundle's smoke against `127.0.0.1:4471` (its cookie
  `menger_session_4471`), then answers `url=http://127.0.0.1:4471/`. Run
  again, it replaces the container and keeps the volume. A container that
  does not answer is left running for `docker logs menger-app-local`. Each
  run is a line in `deployments.log` (`app-local  run  RELEASE  ok IMAGE`),
  never a record.
- **`remote test --image IMAGE`** is `bin/infra app.yml -e app_dir=<the
  main checkout> -e env=test -e sha=<the release>` — the tag infra deploys
  is the release's name on `ghcr.io/hypermemetic-ai/menger`, so a local
  `menger:<release>` is tagged that way first; then the push, the smoke, and
  `TESTED`. **`remote prod`** is `bin/infra promote.yml -e app_dir=…` only.
  Both are driven by `deploy/app.yml` (RUN-11); without it each refuses
  before touching anything.

## On the Linode (RUN-11)

`deploy/app.yml` describes menger to hypermemetic-infra in plexus-starter's
host-layer contract: the image `ghcr.io/hypermemetic-ai/menger:<release>`,
two hosts, the WebSocket (8080) and the shell (8081) inside the container,
the standard routes (`/ws` and any upgrade → the WebSocket, the rest → the
shell), a `/data` volume per environment (each its own tracker), UI-15: one
`shared` volume both environments mount at `/shared` (`menger-shared`, owned
by the image's uid, made by infra on the next deploy or promote and kept by
every one after — the ui store, `/shared/ui.db`, lives there), 1 GB, and
the health checks below. infra makes `/opt/apps/menger-test` (test) and
`/opt/apps/menger` (prod) on the box, the edge's site per host (Caddy:
Let's Encrypt, http → https 308), the A record, and sets
`MENGER_TRUSTED_PROXIES=172.30.0.2/32` (the edge's pinned address) over
`app.env`.

| environment | host | settings (copied to the box as `app.env`) |
|---|---|---|
| test | `app-test.menger.sh` | `deploy/env/app-test.env` — WorkOS staging, `MENGER_UI_SIGNERS` the pipeline key's, `MENGER_UI_DB_PATH=/shared/ui.db`, `MENGER_UI_CHANNEL=test` |
| prod | `app.menger.sh` | `deploy/env/app-prod.env` — WorkOS production; the GitHub button through WorkOS's API with the key on the box; `MENGER_UI_DB_PATH=/shared/ui.db`, `MENGER_UI_CHANNEL=prod`, no signer |

The settings are menger's own (README "Run"): `MENGER_PUBLIC_URL=https://<host>`,
`MENGER_BIND=0.0.0.0`, `MENGER_AUTH_PROVIDERS` = WorkOS (AuthKit) and
`key=wss://<host>/ws` for agents, the provider buttons, and `MENGER_ADMINS` =
the owner's WorkOS principal and the `operator` key. Nothing in either file
is a secret; one goes in a file on the box (below).

prod signs in through WorkOS **production**, whose API key is sealed to the
owner's Ledger (WORKOS-14): what needs it runs under `ledger-secret`, the
owner approving on the device. `deploy/workos-redirect-uris` (RUN-12) keeps
an environment's redirect URIs:

```sh
deploy/workos-redirect-uris list                               # staging
ledger-secret run workos-prod WORKOS_API_KEY -- deploy/workos-redirect-uris ensure \
  https://app.menger.sh/login/callback https://app-test.menger.sh/login/callback
```

A person signed in with no grant waits in `members requests`, where an
admin — the `operator` key — grants them; `deploy/grant-first-admin --url
wss://app.menger.sh/ws --name <email>` does it once, for the first such
WorkOS sign-in.

**Healthy** (infra probes each, through the edge): `http://<host>/` 308 to
`https://<host>/`; `/` 303 to `/login` with
`strict-transport-security: max-age=31536000`; `/login` 200, titled *Sign
in*; an upgrade at `/ws` 101 from the site's origin and 403 from another.

**The rollback of test.** Test is rolled back by deploying the release it
ran before — `status` names it on its `before` line — the same way as any
release; its bundle is pushed again (the same bytes: the version already
kept, the pointer moved back to it), and the smoke runs:

```sh
deploy/pipeline status                                              # test: before <R>
deploy/pipeline remote test --image ghcr.io/hypermemetic-ai/menger:<R>
```

A deploy that fails its smoke is recorded `failed` and leaves the
container infra started; the next green deploy carries its commits. The UI
alone goes back with `ui rollback --to <version>` (an admin's; the
pipeline never does), which moves the pointer — test's, `live:test` — and
nothing else; prod's with `--channel prod` (UI-15).

**The rollback of prod** is infra's: `bin/infra rollback.yml -e
app_dir=<checkout>` puts prod back on its previous image. Its data is a
restore through infra's `seed.yml` — the move's file
(`~/.local/share/menger/backups/tickets.db.<when>-move-RUN-13`), or a fresh
copy taken from the box first:

```sh
B=~/.local/share/menger/backups/tickets.db.<when>-move-RUN-13
../hypermemetic-infra/bin/infra seed.yml -e app_dir=. -e env=prod -e seed=$B --check      # what would be done
../hypermemetic-infra/bin/infra seed.yml -e app_dir=. -e env=prod -e seed=$B \
  -e '{"seed_verify":["/opt/menger/bin/menger","--verify-derived","--db-path","/data/tickets.db"]}'
```

The seed moves the volume's current file aside (never deleted), copies the
file in, checks its sha256 and `--verify-derived`, and starts menger again;
a failed check puts the old file back. A seed is the owner's: it replaces a
live tracker.

### The WorkOS key on the box (IDENTITY-45)

Each environment's menger can hold its WorkOS environment's API key — a
file on the box, never in the repository, `app-*.env`, the image or a log —
so the GitHub button completes through WorkOS's API (README "Login"). `deploy/app.yml` declares the file (`secrets`:
`workos-api-key`, named to menger as `MENGER_WORKOS_API_KEY_FILE`);
`deploy/pipeline remote-secret` puts the value there through infra's
`secret.yml`, on a pipe: written 0400, the container restarted, health
checked, and on a failed check the old value put back. The value comes from
`$WORKOS_API_KEY`, `--from FILE`, or stdin:

```sh
ledger-secret run workos-prod WORKOS_API_KEY -- deploy/pipeline remote-secret prod
deploy/pipeline remote-secret test --from ~/.config/workos/staging.key
deploy/pipeline remote-secret prod --from ~/.config/workos/production.key --check   # infra's dry run
```

Each run is a line in `deployments.log`, never the value. A new image needs
nothing: the file stays on the box.

## Behind https (ACCESS-30)

A deploy reached beyond its machine is **one https origin**: a proxy
terminates TLS for menger's own hostname, sends `/ws` to the WebSocket and
everything else to the shell, and redirects plain http. On the Linode the
edge's Caddy is that proxy; on any other host:

```caddyfile
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

with `MENGER_PUBLIC_URL=https://app.example` and `MENGER_TRUSTED_PROXIES`
the proxy's address, and the provider's redirect URI
`https://app.example/login/callback` registered with it.

**The checklist**:

| check | expect |
|---|---|
| `curl -sI http://app.example/` | `308`/`301` to `https://app.example/` |
| `curl -sI https://app.example/` | `303` to `/login?next=%2F`; `strict-transport-security: max-age=31536000` |
| the page's `content-security-policy` | `connect-src` holds `wss://app.example` |
| sign in in a browser | the session cookie is `__Host-<name>_session; Secure; HttpOnly; SameSite=Lax; Path=/` |
| an upgrade at `/ws` with `Origin: https://evil.example` | `403` |
| `/login/start` for an IdP button | `redirect_uri=https://app.example/login/callback` |
| `curl -s https://app.example/.well-known/oauth-protected-resource/ws` | `200`, `"resource":"https://app.example/ws"` |
| an upgrade at `/ws` with a forged bearer | `401`, `WWW-Authenticate: … resource_metadata=…` |
| `synapse --url wss://app.example/ws --as <agent> menger tickets health` | answered through the proxy |
| the server's startup log | no `plain http on the LAN` warning; no `no --trusted-proxy` warning |

`deploy/systemd/menger@.service` is the hardened unit (ACCESS-27) for a
host that runs menger as a service rather than the image; this machine runs
none.

## Records on the tracker (DEPLOY-12)

Every `remote test` and `remote prod` (and `landing`) records itself on the
tracker through `deploy/record`: `started` before it begins, then
`succeeded` or `failed` — the environment, the release, its revision, the
range of commits `git log previous..revision` (`previous`: the release the
environment last ran with its smoke passed, so a failed deploy's commits
ride with the next green one), and a run id (`source.run`) that is also the
end of its line in `deployments.log`. The tracker is
`MENGER_DEPLOY_TRACKER_URL` (default `wss://app.menger.sh/ws`), reached as
the pipeline's key (`MENGER_DEPLOY_KEY_FILE`, default
`~/.plexus/keys/pipeline.key`, `viewer` on `*` and `deployer` on `MENGER`
there; `deploy/menger-call.mjs` reads the seed from the file, never argv).
Every record goes through the outbox (`~/.local/share/menger-deploy/outbox/`,
oldest first, each send bounded by `MENGER_DEPLOY_RECORD_TIMEOUT`, 10 s);
one the tracker does not take waits there for the next run, one it refuses
moves to `outbox/refused/`, and a record never fails a deploy. A
`succeeded` record to `test`, once kept, sets **Complete** on each In Review
ticket it carried that now runs its current version there (DEPLOY-22/28),
as `MENGER_DEPLOY_COMPLETE_KEY_FILE` (default `~/.plexus/keys/builder.key`,
an editor's; `none`: records only). `status` shows each environment's last
record sent and what waits; `backfill` replays `deployments.log` through the
outbox, and the tracker answers what it kept already `duplicate`. A v2
server keeps no records — the tracker does.

**A tracker with no `deployments.record`** (DEPLOY-62): menger v2 cut the
deployments service, and its gate answers a call to the absent method as it
answers a missing grant (`forbidden`, `needs deployments.record`). So a run
asks once, before its first record, `deploy/record --probe` — the gate's own
`auth.requires` for `deployments.record`, which v2 answers `no method
deployments.record here` (exit 4). Then nothing is sent and nothing queued,
the run says once `the tracker (…, v2) keeps no deployment records; this
deploy is in deployments.log`, and the deploy goes on: `deployments.log`,
`status` and `releases` are the record. What waited in the outbox is set
aside — moved, never deleted — into `outbox/undeliverable/`, whose `README`
gains a line saying when, how many and why; the drain never reads that
directory and `status` counts it. `deploy/pipeline outbox` asks the same and
sets aside what waits; `backfill` sends nothing to such a tracker. A
`deploy/record --send` refused there exits 4 and says so, told apart from a
refused grant (3, `refused`; the pipeline's hint then names the grant the
key lacks) and from no answer (3). A tracker that has the method (v1, or a
later one) is recorded exactly as above.

A record's shape is `deploy/record.schema.json`; `deploy/record --validate`
checks one, and `deploy/record --send --url …` sends it:

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

**From another CI.** `deploy/record` makes the record from what a CI has
(`--from github-actions`, `--from gitlab`, `--from github-event`, `--from
json`, `--from pipeline-log`, or flags), and `deploy/record-action` wraps it
as a GitHub action; `deploy/github-relay.mjs` turns GitHub's
deployment-status events into records. A CI signs in with an agent key
holding `deployer` on its project's shortcode (`node deploy/menger-call.mjs
--new-key`, then an admin's `members grant --role deployer`), and three
files are all a runner needs. Each snippet below runs in `deploy/tests/run`,
its records checked against the schema.

**GitLab CI.**

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

**Buildkite.**

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

**A plain shell script.**

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

## The deploy runner (DEPLOY-37, DEPLOY-48)

`deploy/runner.mjs` (the user unit `menger-deploy-runner`) is this
machine's executor, and **test is automatic** (owner, 2026-10-19: "we
always instantly deploy everything to test. we tolerate rollback"). It
holds a watch on the tracker as the pipeline key (`--watch`: app.menger.sh
cannot reach this machine), and a ticket of the project — MENGER or any
shortcode hosted under it — entering **In Review**, or a commit linked to
one that is In Review, files a test request on the tracker by itself:

- **The branch** is `MENGER_RUNNER_BRANCH` in
  `~/.config/menger/deploy-runner.env` (default `main`; V2-7) — the one
  source: the runner builds that branch's tip (`build <sha> --branch <it>`),
  records each request's branch, and `deploy/pipeline status` prints
  `branch <it>` from the same file. On 2026-10-29 it was `main`; v2's
  releases were built with `--branch v2` by hand.
- **Debounce and coalesce:** filed 60 s (`MENGER_RUNNER_DEBOUNCE_SECS`)
  after the last trigger; one run at a time and at most one pending
  request, which takes the branch's tip when it starts.
- **The plan:** `deploy/pipeline build <tip> --branch <branch> --no-gate` →
  `IMAGE_STORE=ghcr deploy/pipeline image <release>` →
  `deploy/pipeline remote test --image ghcr.io/hypermemetic-ai/menger:<release>`
  (the push and the smoke with it) — live on app-test within minutes — and
  then, apart from the request, `deploy/pipeline suite <release>`, one at a
  time, the newest waiting.
- **Red:** a red build, image or deploy is recorded `failed`; a red suite
  marks the release (test keeps running it; `remote prod` refuses it).
  Either way the failing lines are posted on the triggering ticket on the
  tracker and a gap is filed blocking every triggering ticket (CI-3), as the
  editor key (`MENGER_RUNNER_EDITOR_KEY_FILE`, `~/.plexus/keys/builder.key`;
  `none`: neither). A green suite says so on the ticket.
- **Off:** `MENGER_RUNNER_AUTO_TEST=0` in the env file (or
  `--no-auto-test`), then `systemctl --user restart menger-deploy-runner`.
  `deploy/pipeline status` says `auto-test on|off`, the runner's unit, the
  branch, the request running and pending, the last run and the suite.
- **Tracking:** a ticket's work is its branch's commits; merges are
  `--no-ff`, so "is X on test" is "is X's newest linked commit an ancestor
  of what test runs" (DEPLOY-46). Never squash or rebase on merge.

Each step's output is in
`~/.local/state/menger-deploy-runner/runs/<request>/<step>.log`, the
suite's in `…/runs/<request>/suite.log`. Prod stays the owner's: a request
to `app-prod` runs only when its requester is one of `MENGER_RUNNER_OWNERS`
(never a key or an agent) and the release's suite passed — image, remote
test, remote prod; empty, every one is refused. From a shell the owner runs
`deploy/pipeline remote prod`.

```sh
scripts/deploy-runner/install.sh --start      # the unit and ~/.config/menger/deploy-runner.env
                                              # (from scripts/deploy-runner/deploy-runner.env.example)
systemctl --user status menger-deploy-runner
journalctl --user -u menger-deploy-runner -f
```

## Tests

Each runs on a throwaway root and touches nothing real; none runs under
`cargo test`, so run the one a change touches:

- `deploy/tests/pipeline` — the pipeline itself, `HOME` a scratch directory,
  infra, `synapse`, `docker`, `gh`, `node` and the tracker stand-ins: every
  record, the smoke (each check failing on its own), the push (signed,
  refused, no key), a tracker down, hung, refusing or with no
  `deployments.record` (DEPLOY-62: nothing sent or queued, what waited set
  aside), `remote prod`'s suite
  check and its promote (UI-15: no push to prod, one `ui.promote` as the
  pipeline key, the hash in the log line, a refusal failing it with nothing
  moved), the suite green and red, `build --branch` on a tree with no
  `frontend/`, `local`, `status`, `backfill` and the retired commands.
- `deploy/tests/run` — `deploy/record`: every input form against
  `deploy/tests/fixtures/`, the schema against the examples above, and the
  three snippets as written.
- `node --test deploy/tests/runner.test.mjs` — the runner over
  `deploy/tests/fake-pipeline`: trigger, debounce, coalesce, the branch, the
  off switch, a red run, the suite after a green deploy.

## Where things are

- releases: `~/.local/share/menger-deploy/releases/` (each with its `SUITE` once one ran, `TESTED` once test passed it)
- the build's checkout: `../menger-build`; the suite's: `../menger-suite`
- every build, image, remote run, suite and secret: `~/.local/share/menger-deploy/deployments.log`
- records waiting for the tracker: `~/.local/share/menger-deploy/outbox/`
  (refused: `outbox/refused/`; set aside for a tracker with no
  `deployments.record`, DEPLOY-62: `outbox/undeliverable/` and its `README`);
  the last sent per environment: `…/menger-deploy/sent/`
- logs: `~/.local/state/menger-deploy/logs/`; the runner's `~/.local/state/menger-deploy-runner/runs/`;
  the boxes': `../hypermemetic-infra/bin/infra status.yml`
- the build-output sweep (CI-2): `menger-sweep.timer`, hourly
  (`scripts/sweep/install.sh --start`); what it removed in
  `~/.local/state/menger/cleanup-gate.log`

## Rules

- Production changes only through `remote prod`. A `cargo build` in a
  checkout changes nothing that is serving.
- Work is checked on test (https://app-test.menger.sh) or on a throwaway
  server — never on prod. The suite never runs against a deploy.
- A release that failed test, or whose suite failed, is never promoted; fix
  forward with a new commit.
