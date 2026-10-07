# Deploying menger

menger runs in two environments on this machine, **prod** and **test**, both
under `systemd --user` (lingering is on: they start at boot and restart on
failure). Production changes only through this pipeline (DEPLOY-1).

```
commit on master → build → gate → release → deploy test → smoke → promote → smoke → live
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
deploy/pipeline build                # master's tip → gate → release (prints its name)
deploy/pipeline deploy test          # the newest release → test, with a fresh copy of prod's data
deploy/pipeline sync test            # test's data refreshed from prod; test keeps its release
deploy/pipeline promote              # the release test runs → prod
deploy/pipeline status               # what each env runs, whether it is up, its health
```

1. **Build** (`build [REV]`, default `master`). A commit, never a working
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
loads the app to `live · WS` with the nav drawn and no page errors.

**Drills.** `DEPLOY_SMOKE_FAIL=test deploy/pipeline promote --to test` fails
the first smoke on purpose, so the automatic rollback can be watched on test.

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
