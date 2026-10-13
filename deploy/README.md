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
| settings | `~/.config/menger/prod.env` | `~/.config/menger/test.env` (`PLEXUS_NO_REGISTRY=1`) |

## The pipeline

```sh
deploy/pipeline install              # once, and after a change to deploy/: units + env files, enable both envs
deploy/pipeline build                # main's tip → gate → release (prints its name)
deploy/pipeline deploy test          # the newest release → test, with a fresh copy of prod's data
deploy/pipeline sync test            # test's data refreshed from prod; test keeps its release
deploy/pipeline promote              # the release test runs → prod
deploy/pipeline status               # what each env runs, whether it is up, its health
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

**Smoke checks** (`smoke ENV`): both units active; `tickets.health` answers on
the env's port; `--verify-derived` is consistent; the app is served stamped
with the env's WebSocket port; `diagrams` parses a diagram; a headless browser
loads the app to `live · WS` with the nav drawn and no page errors. On test,
behind the login with the dev sign-in, `deploy/probe-auth.mjs` then plays the
login's known attacks against the live server (IDENTITY-38): foreign and
anonymous sockets, framing, reads and writes across shortcodes, a blocker in a
shortcode the caller cannot see, a watch after its grant is revoked, sign-out —
any that works fails the smoke. It writes probe tickets and a grant to test's
database, which the next `deploy test` or `sync test` replaces. Run it alone:
`node deploy/probe-auth.mjs 4450 4451`.

**Drills.** `DEPLOY_SMOKE_FAIL=test deploy/pipeline promote --to test` fails
the first smoke on purpose, so the automatic rollback can be watched on test.

## The deployment record

What a deployment did, in one JSON shape any CI can produce (DEPLOY-10) —
the schema is `deploy/record.schema.json`, and menger's `deployments.record`
(DEPLOY-11) takes it as its parameters. A deployment never changes a ticket's
status; its tickets are the ones its commits are linked to (`link_commits`),
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
  `CI_JOB_STATUS` (`after_script`). The project defaults to the repository's
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
  record too (same revision before and after).
- **`--commits-from-git`** with no `previous` leaves `commits` out and says
  so: a first deployment does not send the whole history.

Tests: `deploy/tests/run` (fixtures in `deploy/tests/fixtures/`: a GitHub
Actions environment, a `deployment_status` payload, a GitLab environment, a
pipeline log, any JSON); `cargo test` runs it, so the gate does.

## Where things are

- releases: `~/.local/share/menger-deploy/releases/`
- what each env runs: `~/.local/share/menger-deploy/envs/{prod,test}/{current,previous}`
- every build, deploy, promote and rollback: `~/.local/share/menger-deploy/deployments.log`
- logs: `deploy/pipeline logs prod` (`journalctl --user -u menger@prod -u menger-diagrams@prod`)
- prod backups: `~/.local/share/menger/backups/`

## Rules

- Production changes only through `promote`. Nothing runs production from a
  development checkout; `cargo build` or `trunk build` in `menger/` changes
  nothing that is serving.
- Work is checked on the test environment or on a throwaway server — never on
  prod.
- A release that failed test is never promoted; fix forward with a new commit.
