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
deploy/pipeline status               # what each env runs, whether it is up, its health, its last record
deploy/pipeline backfill             # deployments.log → menger's deployments (idempotent)
deploy/pipeline landing              # main's landing/ → gate → the edge (menger.sh); recorded as env `edge`
deploy/pipeline landing --check      # the gate and infra's dry run, nothing written
deploy/pipeline media                # landing/media regenerated from the release prod runs
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
loads the app to `live · WS` with the nav drawn and no page errors; every port
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

## Recording deployments

Every `deploy test`, `sync test`, `promote` and `rollback` tells menger what
it did (DEPLOY-12), in the deployment record below, made and sent by
`deploy/record` to `deployments.record` (DEPLOY-11) — so each ticket shows
where its work runs:

| the run | records (environment, revision) |
|---|---|
| begins | `started` — the release it moves to (`previous`: what ran) |
| smoke passed | `succeeded` (a rollback: `rolled_back` to the release it returned to) |
| smoke failed | `failed` |
| promote's automatic rollback, smoke passed | `failed`, then `rolled_back` to what ran before |
| dies after `started` (a unit will not restart …) | `failed`, on its way out |

- **The commits** are `git log previous..revision` in the build checkout
  (`../menger-build`, else the repository), oldest first, when a run moves
  forward; a rollback carries none. Without them menger works from what it
  has seen.
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
  `MENGER_DEPLOY_TRACKER_TOKEN` (a key holding `deployer` when the tracker
  requires login).
- **`status`** shows each environment's last record sent (`sent/<env>.json`)
  and how many wait in the outbox.
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
`backfill`; `cargo test` runs it.

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
