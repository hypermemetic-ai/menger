#!/usr/bin/env node
// DEPLOY-37: the deploy runner — the executor behind a deploy request. menger
// files a request (`deployments.request`, the deploy page's "→ test") and
// knows nothing of how it is carried out; this runner carries it out on this
// machine and reports each step back against the request:
//
//   deployments.step   --request N --name build|deploy --state started|succeeded|failed|skipped|canceled
//   deployments.record --request N  (deploy/pipeline's own records, MENGER_REQUEST; or the runner's when no
//                                    step recorded: a failed build, a cancel, --fake)
//
// It takes requests two ways, one executor behind both:
//
//   --listen  the request's webhook (DEPLOY-25): POST /, the body checked against
//             X-Menger-Signature (HMAC-SHA256 keyed by the secret in --secret-file),
//             answered 202 and queued. On loopback only. For a tracker that can
//             reach the runner — one on this machine, or an https route to it.
//   --watch   dialling out: `deployments.watch` held as the deploy key, each pending
//             request to a planned environment taken (and those pending at start).
//             For a tracker that cannot reach this machine — app.menger.sh today.
//
// Later hyperforge's deploy.* (HYPERFORGE-10) takes the webhook in its place:
// the webhook's URL moves, and nothing in menger changes.
//
// The plan, per environment (`test` and `app-prod`: a request to another is
// refused 422 — the webhook says why — or, watching, left for its own executor):
//
//   test (DEPLOY-52: test is app-test.menger.sh; the suite runs after the deploy, never before it)
//   build   deploy/pipeline build <sha> --no-gate      the branch's tip (the request's revision, else
//                                                      `git rev-parse <branch>`) → a release, no suite;
//                                                      stopped when the request is canceled
//   image   IMAGE_STORE=ghcr deploy/pipeline image <release>   pushed to GHCR; stopped on a cancel
//   deploy  deploy/pipeline remote test --image ghcr.io/hypermemetic-ai/menger:<release>
//                                                      app-test.menger.sh through infra, then the smoke
//                                                      from outside; its records name the request
//                                                      (MENGER_REQUEST) and set Complete (DEPLOY-22/28)
//   then, apart from the request (it has ended), the suite:
//   suite   deploy/pipeline suite <release>            cargo test --workspace + the frontend's on this
//                                                      machine, at the release's commit, never against the
//                                                      deployment; one at a time, the newest waiting (a
//                                                      release passed over is never promoted). Red: the
//                                                      release is marked `failed` (remote prod refuses it),
//                                                      the failing lines in the triggering ticket's chat and
//                                                      a gap blocking it (CI-3); green: a line in its chat.
//
//   app-prod (DEPLOY-40: #/deploy's → prod — app.menger.sh since RUN-13; the local
//            prod is stopped and `promote` refuses while prod.moved exists)
//   image     IMAGE_STORE=ghcr deploy/pipeline image <release>   the release test passed, pushed
//   app-test  deploy/pipeline remote test --image ghcr.io/hypermemetic-ai/menger:<release>
//   app-prod  deploy/pipeline remote prod                         promote.yml: what app-test runs → app.menger.sh;
//                                                                 its records name the request
//
// A request to app-prod is the owner's (owner, 2026-10-19: "Deployments should be
// triggered by me"): taken only when its requester is one of --owner
// (MENGER_RUNNER_OWNERS, comma-separated principals) and never a key or an agent,
// and only for a release test passed (a `succeeded` record to test naming it) whose
// suite passed (DEPLOY-52: `deploy/pipeline suite --status`).
// Otherwise its first step is failed with the refusal, the rest skipped, and
// nothing runs. A failed step stops the plan: what app-prod runs stays.
//
// One request at a time, in the order taken. Each step's output is kept in
// --state-dir/runs/<request>/<step>.log and served at GET /runs/<request>/<step>.log
// (the step's url); DEPLOY-47: every ended step's detail is its command and its
// last 40 lines (stepLog, at most 4000 bytes), so the app has the log to quote
// when the owner asks the builder about it — the url is this machine's alone.
//
// DEPLOY-48: test is automatic (owner, 2026-10-19: "we always instantly deploy everything to
// test … our system should do this automatically upon something entering review"). The runner
// holds `tickets.watch` and, for a ticket of its project (MENGER and every shortcode hosted under
// it) that enters In Review — or that is In Review when a commit is linked to it — files a test
// request for main's tip itself (`deployments.request --branch main`, the pipeline key's
// deployer grant). A ticket moved back to Coding triggers nothing; its next entry does. Triggers
// are debounced (--debounce-secs, 60) and coalesced: one run at a time and at most one pending
// request — a trigger while one is pending folds into it, and it takes main's tip when it starts.
// A red run of such a request posts the failing lines to the triggering ticket's chat (else
// --chat, 6) and files a gap blocking the triggering tickets (CI-3), as the editor key
// (MENGER_RUNNER_EDITOR_KEY_FILE, default ~/.plexus/keys/builder.key). Off:
// MENGER_RUNNER_AUTO_TEST=0 (or --no-auto-test). What it holds is in --state-dir/auto.json, which
// `deploy/pipeline status` reads. Prod stays the owner's (above).
//
//   node deploy/runner.mjs --listen [--port 4475] --secret-file ~/.config/menger/deploy-runner.secret
//   node deploy/runner.mjs --watch
//   options: --url wss://app.menger.sh/ws (MENGER_URL)   the tracker
//            --project MENGER                            whose requests
//            --repo DIR --pipeline FILE                  the checkout and its deploy/pipeline (default: this one's)
//            --state-dir DIR                             default ~/.local/state/menger-deploy-runner
//            --owner PRINCIPAL                           who may ask for app-prod (repeat; MENGER_RUNNER_OWNERS)
//            --no-auto-test                              DEPLOY-48 off (MENGER_RUNNER_AUTO_TEST=0)
//            --debounce-secs N  --chat N                 DEPLOY-48: the quiet before a run (60); a red run's
//                                                        fallback chat (6; MENGER_RUNNER_DEBOUNCE_SECS, MENGER_RUNNER_CHAT)
//            --fake                                      no pipeline: each step a pause (MENGER_RUNNER_FAKE_FAIL=<step>
//                                                        fails it; MENGER_RUNNER_FAKE_SECS, default 2) — the tests'
// The key is $MENGER_DEPLOY_KEY_FILE (default ~/.plexus/keys/pipeline.key: deployer on MENGER), never argv;
// the secret is a file, never argv or the environment, and never printed.
// Node ≥ 22, its standard library only.
import http from "node:http";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn, execFileSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import { call, connect, deployKeyText, keyProof, seedOf } from "./menger-call.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));

/** Where `remote test` takes a release's image from (deploy/pipeline's IMAGE_GHCR). */
export const IMAGE_GHCR = "ghcr.io/hypermemetic-ai/menger";

/** The plan for an environment: its steps, in order. `records`: the step's
 * own records name the request (MENGER_REQUEST) — only a step that records
 * to the request's environment may. `env`: the step's environment. */
export const PLANS = {
  test: [
    { name: "build", argv: (job) => ["build", job.sha, "--no-gate"], interruptible: true },
    { name: "image", argv: (job) => ["image", job.release], env: { IMAGE_STORE: "ghcr" }, interruptible: true, release: true },
    { name: "deploy", argv: (job) => ["remote", "test", "--image", `${IMAGE_GHCR}:${job.release}`], records: true, release: true },
  ],
  "app-prod": [
    { name: "image", argv: (job) => ["image", job.release], env: { IMAGE_STORE: "ghcr" }, interruptible: true },
    { name: "app-test", argv: (job) => ["remote", "test", "--image", `${IMAGE_GHCR}:${job.release}`] },
    { name: "app-prod", argv: () => ["remote", "prod"], records: true },
  ],
};

/** DEPLOY-40: what a request to an environment needs beyond its plan. DEPLOY-52: `suite`,
 * the release's suite passed. */
export const GUARDS = {
  "app-prod": { owner: true, tested: "test", suite: true },
};

/** DEPLOY-52: the suite after a deploy to test — not a step of the request, which has ended. */
export const SUITE = { name: "suite", argv: (s) => ["suite", s.release] };

/** DEPLOY-52: what `deploy/pipeline suite --status` says of a release: passed, failed, running, none. */
export const suiteIn = (text) => /^suite: (passed|failed|running|none)\b/m.exec(String(text || ""))?.[1] ?? null;

/** Whether a principal is a key or an agent — never the owner, whatever the config says. */
export const keyish = (p) => /^(key|agent):/.test(String(p || ""));

/** DEPLOY-40: why `job` may not run as its guard says (its requester, the owners), or null. */
export function unowned(job, owners) {
  const g = GUARDS[job.environment];
  if (!g?.owner) return null;
  const by = String(job.requestedBy || "");
  if (!by) return `refused: a request to ${job.environment} names its requester, and this one does not`;
  if (keyish(by)) return `refused: ${by} is a key — a deploy to ${job.environment} is the owner's to ask for (owner, 2026-10-19), never an agent's`;
  if (!owners.length) return `refused: this runner has no owner (MENGER_RUNNER_OWNERS) — a deploy to ${job.environment} is the owner's to ask for`;
  if (!owners.includes(by)) return `refused: ${by} is not the owner — a deploy to ${job.environment} is the owner's to ask for (MENGER_RUNNER_OWNERS)`;
  if (!job.release) return `refused: a request to ${job.environment} names the release to promote (the one ${g.tested} passed)`;
  return null;
}

/** DEPLOY-42: the release a step's output names last (`release <id>` from build, `test runs <id>` from deploy), or null. */
export const releaseIn = (text) => [...String(text || "").matchAll(/\b\d{8}T\d{6}Z-[0-9a-f]{7,40}\b/g)].at(-1)?.[0] ?? null;

/** A release's commit: the hash it ends in. */
export const shaOf = (release) => /-([0-9a-f]{7,40})$/.exec(String(release || ""))?.[1] ?? null;

/** DEPLOY-48: whether a ticket is `project`'s — its own shortcode or one hosted under it (the
 * address's first segment: `hypermemetic:MENGER/RUN/DEPLOY-48` is MENGER's). */
export function ofProject(ticket, project) {
  const want = String(project || "").toUpperCase();
  const addr = String(ticket?.address || "");
  const first = (addr.includes(":") ? addr.slice(addr.indexOf(":") + 1) : addr).split("/")[0];
  const code = first.replace(/-\d+$/, "").replace(/\.\d+$/, "").toUpperCase();
  return code ? code === want : String(ticket?.epic || "").toUpperCase() === want;
}

/** DEPLOY-48: a `tickets.watch` event that should deploy test — a ticket of `project` entering
 * In Review, or a commit linked to one that is In Review — as `{ ticket, why }`; else null.
 * Leaving In Review (back to Coding) triggers nothing: its re-entry does. */
export function triggerOf(ev, project) {
  const t = ev?.ticket;
  if (!t || t.status !== "In Review" || !ofProject(t, project)) return null;
  if (ev.type === "status_changed" && ev.previous !== "In Review") return { ticket: t.id, why: "entered In Review" };
  if (ev.type === "commits_linked" && (ev.added || []).length) return { ticket: t.id, why: `${ev.added.length} commit(s) linked in review` };
  return null;
}

/** DEPLOY-48: what a red step's log says failed — cargo's and node's failing lines — or its tail. */
export function failingLines(out, max = 25) {
  const lines = String(out || "").split("\n");
  const hits = [...new Set(lines)].filter((l) => /^test .* \.\.\. FAILED|^---- .* ----$|panicked at|^error(\[|:)|^not ok |^# fail [1-9]|test result: FAILED|^FAILED|✗|refused/.test(l.trim()));
  return (hits.length ? hits.slice(0, max) : lines.filter(Boolean).slice(-max)).join("\n").slice(0, 3000);
}

/** Whether `sig` is the body's signature under `secret` (timing-safe). */
export function signed(secret, body, sig) {
  const want = "sha256=" + crypto.createHmac("sha256", secret).update(body).digest("hex");
  return typeof sig === "string" && sig.length === want.length && crypto.timingSafeEqual(Buffer.from(sig), Buffer.from(want));
}

/** A log's last lines, at most `max` bytes. */
export function tail(text, lines = 30, max = 4000) {
  let t = text.trimEnd().split("\n").slice(-lines).join("\n");
  while (Buffer.byteLength(t) > max) t = t.slice(t.indexOf("\n") + 1 || 1);
  return t;
}

/** DEPLOY-47: an ended step's detail — `$ <command>`, then its log's last
 * `lines` (`exit N` when it printed nothing), the whole at most `max` bytes. */
export function stepLog(command, out, lines = 40, max = 4000, code = 0) {
  const head = `$ ${command}`.slice(0, 500);
  const body = tail(out || "", lines, Math.max(0, max - Buffer.byteLength(head) - 1)) || (code ? `exit ${code}` : "");
  return body ? `${head}\n${body}` : head;
}

export function options(argv, env = process.env) {
  const o = {
    listen: false, watch: false, fake: false,
    port: Number(env.MENGER_RUNNER_PORT || 4475),
    url: env.MENGER_URL || "wss://app.menger.sh/ws",
    project: (env.MENGER_RUNNER_PROJECT || "MENGER").toUpperCase(),
    secretFile: env.MENGER_RUNNER_SECRET_FILE || "",
    repo: env.MENGER_RUNNER_REPO || path.resolve(HERE, ".."),
    pipeline: env.MENGER_RUNNER_PIPELINE || "",
    stateDir: env.MENGER_RUNNER_STATE_DIR || path.join(os.homedir(), ".local/state/menger-deploy-runner"),
    agent: env.MENGER_AGENT || "deploy-runner",
    owners: String(env.MENGER_RUNNER_OWNERS || "").split(",").map((s) => s.trim()).filter(Boolean),
    autoTest: env.MENGER_RUNNER_AUTO_TEST !== "0",
    debounceMs: Number(env.MENGER_RUNNER_DEBOUNCE_SECS || 60) * 1000,
    chat: Number(env.MENGER_RUNNER_CHAT || 6),
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i], next = () => { const v = argv[++i]; if (v === undefined) throw new Error(`${a} needs a value`); return v; };
    if (a === "--listen") o.listen = true;
    else if (a === "--watch") o.watch = true;
    else if (a === "--fake") o.fake = true;
    else if (a === "--port") o.port = Number(next());
    else if (a === "--url") o.url = next();
    else if (a === "--project") o.project = next().toUpperCase();
    else if (a === "--secret-file") o.secretFile = next();
    else if (a === "--repo") o.repo = next();
    else if (a === "--pipeline") o.pipeline = next();
    else if (a === "--state-dir") o.stateDir = next();
    else if (a === "--agent") o.agent = next();
    else if (a === "--owner") o.owners.push(next());
    else if (a === "--no-auto-test") o.autoTest = false;
    else if (a === "--debounce-secs") o.debounceMs = Number(next()) * 1000;
    else if (a === "--chat") o.chat = Number(next());
    else throw new Error(`unknown argument '${a}'`);
  }
  if (!o.listen && !o.watch) throw new Error("--listen (the webhook), --watch (dialling out), or both");
  if (!/^wss?:\/\//.test(o.url)) throw new Error(`--url ${o.url}: ws:// or wss://`);
  o.pipeline ||= path.join(o.repo, "deploy/pipeline");
  return o;
}

/** The secret, from its file (0600, 16–256 bytes) — never said back. */
function secretOf(file) {
  if (!file) throw new Error("--listen needs --secret-file (the webhook's secret, a file)");
  const st = fs.statSync(file);
  if ((st.mode & 0o077) !== 0) throw new Error(`${file} is readable by others (0600)`);
  const s = fs.readFileSync(file, "utf8").trim();
  if (s.length < 16 || s.length > 256) throw new Error(`${file}: a webhook secret is 16 to 256 bytes`);
  return s;
}

export class Runner {
  constructor(o, log = (l) => process.stdout.write(`${new Date().toISOString()} ${l}\n`)) {
    this.o = o; this.log = log; this.queue = []; this.busy = false; this.taken = new Set();
    fs.mkdirSync(path.join(o.stateDir, "runs"), { recursive: true });
    this.takenFile = path.join(o.stateDir, "taken.json");
    try { for (const id of JSON.parse(fs.readFileSync(this.takenFile, "utf8"))) this.taken.add(id); } catch {}
    const text = deployKeyText({ ...process.env, MENGER_DEPLOY_KEY_FILE: process.env.MENGER_DEPLOY_KEY_FILE || path.join(os.homedir(), ".plexus/keys/pipeline.key") });
    this.seed = text ? seedOf(text) : null;
    // DEPLOY-48: the auto-test state — tickets waiting out the debounce, and each request's triggering tickets.
    this.auto = { waiting: new Map(), timer: null, triggers: {}, pending: null, last: null };
    // DEPLOY-52: the suites — one running, the newest waiting
    this.suites = { busy: false, running: null, pending: null, last: null };
    this.autoFile = path.join(o.stateDir, "auto.json");
    try { const a = JSON.parse(fs.readFileSync(this.autoFile, "utf8")); this.auto.triggers = a.triggers || {}; this.suites.last = a.suite?.last || null; } catch {}
  }

  headers(seed = this.seed, agent = this.o.agent) {
    return seed ? { Authorization: `Bearer ${keyProof(seed, this.o.url.replace(/\/+$/, ""), agent)}` } : {};
  }

  /** DEPLOY-48: the editor's key (a red run's chat post and gap) — the builder's unless named; null: none. */
  editor() {
    if (this.editorSeed !== undefined) return this.editorSeed;
    const f = process.env.MENGER_RUNNER_EDITOR_KEY_FILE || path.join(os.homedir(), ".plexus/keys/builder.key");
    try { this.editorSeed = f === "none" ? null : seedOf(deployKeyText({ MENGER_DEPLOY_KEY_FILE: f })); } catch { this.editorSeed = null; }
    return this.editorSeed;
  }

  /** One menger call on a socket of its own: its data items. `as`: the editor's key. */
  async menger(method, params, as = null) {
    const ws = await connect(this.o.url, as === "editor" ? this.headers(this.editor(), process.env.MENGER_RUNNER_EDITOR_AGENT || "builder") : this.headers());
    const items = [];
    try { await call(ws, 1, method, params, (it) => items.push(it)); } finally { ws.close(); }
    const refused = items.find((it) => it?.type === "error");
    if (refused) throw new Error(refused.message || JSON.stringify(refused));
    return items.filter((it) => it?.type === "data").map((it) => it.content);
  }

  /** A step reported; the request as it now stands (or the refusal's words). */
  async step(job, name, state, extra = {}) {
    try {
      const [ev] = await this.menger("deployments.step", { request: job.request, name, state, ...extra });
      if (ev?.type === "error") return { refused: ev.message };
      return { request: ev?.deployment?.requested };
    } catch (e) {
      this.log(`request ${job.request}: step ${name} ${state} not taken — ${e.message}`);
      return { lost: true };
    }
  }

  async record(job, state) {
    const rec = { project: job.project, environment: job.environment, state, revision: job.sha, request: job.request, ...(job.release ? { release: job.release } : {}),
      source: { system: "menger-deploy-runner", run: `request-${job.request}-${state}` } };
    try {
      const [ev] = await this.menger("deployments.record", rec);
      if (ev?.type === "error") this.log(`request ${job.request}: record ${state} refused — ${ev.message}`);
    } catch (e) { this.log(`request ${job.request}: record ${state} not taken — ${e.message}`); }
  }

  /** The request as menger has it now — to see a cancel while a step runs. */
  async standing(job) {
    try {
      const rows = await this.menger("deployments.list", { project: job.project, environment: job.environment, state: "requested", limit: 100 });
      return rows.find((r) => r.deployment?.id === job.request)?.deployment?.requested ?? null;
    } catch { return null; }
  }

  /** A payload (the webhook's, or a watched request's) → queued; or why not. */
  take(p) {
    const env = String(p.environment || "").toLowerCase();
    if (String(p.project || "").toUpperCase() !== this.o.project) return `this runner deploys ${this.o.project}, not ${p.project}`;
    if (!PLANS[env]) return `this runner has no plan for ${env} (only ${Object.keys(PLANS).join(", ")})`;
    const id = Number(p.request);
    if (!Number.isInteger(id) || id <= 0) return "no request id";
    if (this.taken.has(id)) return null; // a retry of a delivery already taken
    this.taken.add(id);
    fs.writeFileSync(this.takenFile, JSON.stringify([...this.taken].slice(-500)));
    this.queue.push({ request: id, project: this.o.project, environment: env, revision: p.revision || "", branch: p.branch || "main", release: p.release || "", requestedBy: p.requested_by || "" });
    this.log(`request ${id}: taken (${env}, ${p.release || p.revision || `the tip of ${p.branch || "main"}`})`);
    queueMicrotask(() => this.drain());
    return null;
  }

  async drain() {
    if (this.busy) return;
    this.busy = true;
    try {
      while (this.queue.length) {
        const job = this.current = this.queue.shift();
        if (job.environment === "test") this.autoStarted(job);
        try { await this.run(job); } finally { this.current = null; }
        if (job.environment === "test") await this.autoEnded(job, job.outcome || "failed");
        if (job.environment === "test" && job.outcome === "succeeded" && job.release) this.queueSuite(job);
      }
    } finally { this.busy = false; }
  }

  resolve(job) {
    const ref = job.revision || job.branch;
    try { return execFileSync("git", ["-C", this.o.repo, "rev-parse", "--verify", `${ref}^{commit}`], { encoding: "utf8" }).trim(); }
    catch { return null; }
  }

  /** DEPLOY-40: why the request may not run — its requester, or a release its guard's environment never passed. */
  async refusal(job) {
    const g = GUARDS[job.environment];
    if (!g) return null;
    const why = unowned(job, this.o.owners || []);
    if (why || !g.tested) return why;
    let rows;
    try { rows = await this.menger("deployments.list", { project: job.project, environment: g.tested, state: "succeeded", limit: 1000 }); }
    catch (e) { return `refused: could not ask ${this.o.url} what ${g.tested} passed — ${e.message}`; }
    const passed = rows.some((r) => r?.deployment?.release === job.release);
    if (!passed) return `refused: ${job.release} never passed ${g.tested} — promote only a release ${g.tested} passed`;
    if (!g.suite || this.o.fake) return null;
    let said = "";
    try { said = execFileSync(this.o.pipeline, ["suite", "--status", job.release], { cwd: this.o.repo, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }); }
    catch (e) { said = `${e.stdout || ""}${e.stderr || ""}`; }
    const st = suiteIn(said);
    if (st === "passed") return null;
    if (st === "failed") return `refused: ${job.release}'s suite FAILED — a release whose suite is red never reaches prod (DEPLOY-52)`;
    if (st === "running") return `refused: ${job.release}'s suite is still running — promote once it has passed (DEPLOY-52)`;
    return `refused: ${job.release} has no suite result on this machine (deploy/pipeline suite ${job.release}) — promote once it has passed (DEPLOY-52)`;
  }

  async run(job) {
    const plan = PLANS[job.environment];
    const dir = path.join(this.o.stateDir, "runs", String(job.request));
    fs.mkdirSync(dir, { recursive: true });
    job.sha = GUARDS[job.environment] ? job.revision || shaOf(job.release) : this.resolve(job);
    let outcome = "succeeded", recorded = false, i = 0;
    const refused = await this.refusal(job);
    if (refused) {
      this.log(`request ${job.request}: ${refused}`);
      await this.step(job, plan[0].name, "failed", { detail: refused });
      for (const s of plan.slice(1)) await this.step(job, s.name, "skipped");
      job.sha ||= "0000000";
      return this.record(job, "failed");
    }
    if (!job.sha) {
      await this.step(job, plan[0].name, "failed", { detail: `no commit '${job.revision || job.branch}' in ${this.o.repo}` });
      for (const s of plan.slice(1)) await this.step(job, s.name, "skipped");
      job.sha = "0000000";
      return this.record(job, "failed");
    }
    for (; i < plan.length; i++) {
      const s = plan[i];
      if (s.release && !job.release) {
        // DEPLOY-52: the image and the deploy take the release the build named
        outcome = "failed"; job.red = { step: s.name, out: "the build named no release" };
        await this.step(job, s.name, "failed", { detail: `${s.name}: the build named no release (deploy/pipeline build prints \`release <id>\`)` });
        break;
      }
      const cmd = `${Object.entries(s.env || {}).map(([k, v]) => `${k}=${v} `).join("")}deploy/pipeline ${s.argv(job).join(" ")}`;
      const begun = await this.step(job, s.name, "started", { detail: cmd, url: this.urlOf(job, s) });
      if (begun.refused || begun.request?.cancel_asked_at) {
        outcome = "canceled";
        if (!begun.refused) await this.step(job, s.name, "canceled");
        break;
      }
      const res = await this.exec(job, s, path.join(dir, `${s.name}.log`));
      this.keepLogs(job, s, dir, res);
      if (s.records) recorded = true;
      // The runner's own closing record must name the release too, or it overwrites the pipeline's with none.
      if (!GUARDS[job.environment]) job.release ||= releaseIn(res.out) || "";
      if (res.canceled) { outcome = "canceled"; await this.step(job, s.name, "canceled", { detail: stepLog(cmd, res.out) }); break; }
      if (res.code !== 0) { outcome = "failed"; job.red = { step: s.name, out: res.out, logs: res.logs }; await this.step(job, s.name, "failed", { detail: stepLog(cmd, res.out, 40, 4000, res.code), url: this.urlOf(job, s) }); break; }
      await this.step(job, s.name, "succeeded", { detail: stepLog(cmd, res.out), url: this.urlOf(job, s) });
    }
    for (const s of plan.slice(i + 1)) await this.step(job, s.name, outcome === "canceled" ? "canceled" : "skipped");
    this.log(`request ${job.request}: ${outcome}`);
    job.outcome = outcome;
    // The pipeline's own records name the request once its deploy began; else the runner says how it ended
    // (and when a recording step ended before it recorded: the request still open).
    const now = await this.standing(job);
    if (now?.state === "canceled") return; // canceled outright: nothing ran to record
    if (recorded && !this.o.fake && !["pending", "started"].includes(now?.state)) return;
    await this.record(job, outcome === "succeeded" ? "succeeded" : "failed");
  }

  // ── DEPLOY-48: test is automatic ─────────────────────────────────────────

  /** What auto-test holds, for `deploy/pipeline status`. */
  saveAuto() {
    const triggers = Object.fromEntries(Object.entries(this.auto.triggers).slice(-50));
    this.auto.triggers = triggers;
    const sv = ({ request, release, tickets }) => ({ request, release, tickets });
    const suite = { running: this.suites.running && sv(this.suites.running), pending: this.suites.pending && sv(this.suites.pending), last: this.suites.last };
    const doc = { on: !!this.o.autoTest, waiting: [...this.auto.waiting.keys()], due_at: this.auto.due ?? null,
      running: this.auto.running ?? null, pending: this.auto.pending, last: this.auto.last, suite, triggers, at: new Date().toISOString() };
    try { fs.writeFileSync(this.autoFile, JSON.stringify(doc, null, 1) + "\n"); } catch (e) { this.log(`auto-test: ${this.autoFile}: ${e.message}`); }
  }

  /** A trigger: the ticket waits out the debounce with the others; each one restarts it. */
  trigger({ ticket, why }) {
    if (!this.o.autoTest) return false;
    this.auto.waiting.set(ticket, why);
    clearTimeout(this.auto.timer);
    this.auto.due = Date.now() + this.o.debounceMs;
    this.auto.timer = setTimeout(() => this.autoFire(), this.o.debounceMs);
    this.auto.timer.unref?.();
    this.log(`auto-test: ${ticket} ${why}; test in ${Math.round(this.o.debounceMs / 1000)} s unless more come`);
    this.saveAuto();
    return true;
  }

  /** The pending test request, if any — queued here, else pending on the tracker (a → test's). */
  async pendingTest() {
    const here = this.queue.find((j) => j.environment === "test");
    if (here) return here.request;
    const rows = await this.menger("deployments.list", { project: this.o.project, environment: "test", state: "requested", limit: 100 });
    return rows.map((r) => r?.deployment).find((d) => d?.requested?.state === "pending" && !(d.requested.steps || []).length && d.id !== this.current?.request)?.id ?? null;
  }

  /** The debounce ran out: fold the waiting tickets into the pending request, or file one for main's tip. */
  async autoFire() {
    this.auto.timer = null; this.auto.due = null;
    const tickets = [...this.auto.waiting.keys()];
    if (!tickets.length || !this.o.autoTest) return;
    const why = [...this.auto.waiting].map(([t, w]) => `${t} ${w}`).join("; ");
    this.auto.waiting.clear();
    const attach = (id) => { this.auto.triggers[id] = [...new Set([...(this.auto.triggers[id] || []), ...tickets])]; };
    try {
      const pending = await this.pendingTest();
      if (pending) {
        attach(pending);
        this.auto.pending = { request: pending, tickets: this.auto.triggers[pending] };
        this.log(`auto-test: ${tickets.join(", ")} folded into pending request ${pending} (main's tip when it starts)`);
        return this.saveAuto();
      }
      const [ev] = await this.menger("deployments.request", { project: this.o.project, environment: "test", branch: "main", note: `auto-test (DEPLOY-48): ${why}`.slice(0, 2000) });
      const d = ev?.deployment;
      if (!d?.id) throw new Error(ev?.message || "no request in the answer");
      attach(d.id);
      this.auto.pending = { request: d.id, tickets: this.auto.triggers[d.id] };
      this.log(`auto-test: request ${d.id} filed for main's tip — ${why}`);
      this.saveAuto();
      this.take({ request: d.id, project: d.project || this.o.project, environment: "test", branch: "main", requested_by: d.recorded_by });
    } catch (e) {
      // not filed: the tickets wait again, and the next quiet tries again
      this.log(`auto-test: request not filed — ${e.message}; again in ${Math.round(this.o.debounceMs / 1000)} s`);
      for (const t of tickets) if (!this.auto.waiting.has(t)) this.auto.waiting.set(t, "waiting to be filed");
      this.auto.due = Date.now() + this.o.debounceMs;
      this.auto.timer = setTimeout(() => this.autoFire(), this.o.debounceMs);
      this.auto.timer.unref?.();
      this.saveAuto();
    }
  }

  /** A test run began: the pending request is now the running one, and the next trigger files another. */
  autoStarted(job) {
    if (!this.auto.triggers[job.request]) return;
    this.auto.running = { request: job.request, tickets: this.auto.triggers[job.request] };
    if (this.auto.pending?.request === job.request) this.auto.pending = null;
    this.saveAuto();
  }

  /** A test run ended: no longer pending; a red one carrying triggers is said where its tickets are. */
  async autoEnded(job, outcome) {
    if (this.auto.pending?.request === job.request) this.auto.pending = null;
    if (this.auto.running?.request === job.request) this.auto.running = null;
    const tickets = this.auto.triggers[job.request];
    if (!tickets) return;
    this.auto.last = { request: job.request, outcome, tickets, at: new Date().toISOString() };
    this.saveAuto();
    if (outcome === "failed" && job.red) await this.reportRed(job, tickets);
  }

  /** CI-3: a red run is the batch's — a gap blocking each triggering ticket, and the failing lines in its chat. */
  async reportRed(job, tickets) {
    if (!this.editor()) return this.log(`auto-test: request ${job.request} red — no editor key (MENGER_RUNNER_EDITOR_KEY_FILE), nothing posted or filed`);
    const lines = failingLines(job.red.out, 80);
    const fence = "```";
    const suite = job.red.step === "suite";
    const head = suite
      ? `The suite of ${job.release} (main at ${String(job.sha || "").slice(0, 7)}, deployed to test by request ${job.request}) went red. The release is marked \`suite failed\`: \`remote prod\` refuses it (DEPLOY-52).`
      : `Auto-test request ${job.request} (main at ${String(job.sha || "").slice(0, 7)}) went red in \`${job.red.step}\`.`;
    const body = `${head}\n\n${fence}text\n${lines}\n${fence}\n\n${tickets.length ? `Triggered by ${tickets.join(", ")}. ` : ""}The red is the batch's (CI-3): find the commit, fix or revert; the next ticket entering In Review deploys test again. Log: ${this.o.stateDir}/runs/${job.request}/${job.red.step}.log${(job.red.logs || []).map((l) => `\nTest output: ${this.o.stateDir}/runs/${job.request}/${l.name} (kept at ${l.from})`).join("")}`;
    let gap = null;
    if (tickets.length) try {
      const [ev] = await this.menger("tickets.create", { epic: tickets[0].replace(/-\d+$/, ""), parent: tickets[0], kind: "gap",
        title: suite ? `suite red: ${job.release} (request ${job.request}) — main at ${String(job.sha || "").slice(0, 7)}`
          : `test deploy red: request ${job.request} (${job.red.step}) — main at ${String(job.sha || "").slice(0, 7)}`, body }, "editor");
      if (ev?.type === "error") throw new Error(ev.message);
      gap = ev?.ticket?.id || null;
      if (gap) for (const t of tickets) {
        try {
          const [u] = await this.menger("tickets.update", { id: t, block: [gap], message: `auto-test request ${job.request} went red` }, "editor");
          if (u?.type === "error") throw new Error(u.message);
        }
        catch (e) { this.log(`auto-test: ${gap} could not block ${t} — ${e.message}`); }
      }
    } catch (e) { this.log(`auto-test: no gap filed for request ${job.request} — ${e.message}`); }
    let chat = tickets.length ? await this.chatOf(tickets[0]) : null;
    chat ||= this.o.chat;
    try {
      const [p] = await this.menger("chats.post", { chat, body: `${body}${gap ? `\n\nFiled ${gap}, blocking ${tickets.join(", ")}.` : ""}` }, "editor");
      if (p?.type === "error") throw new Error(p.message);
    }
    catch (e) { this.log(`auto-test: could not post to chat ${chat} — ${e.message}`); }
    this.log(`auto-test: request ${job.request} red — ${gap ? `${gap} filed` : "no gap"}, chat ${chat}`);
    return { gap, chat };
  }

  /** The open chat of `ticket`, or null. */
  async chatOf(ticket) {
    try { return (await this.menger("chats.list", { ticket, state: "open", limit: 1 }, "editor")).find((c) => c?.type === "chat")?.chat?.id ?? null; } catch { return null; }
  }

  // ── DEPLOY-52: the suite runs after the deploy to test ──────────────────

  /** A deploy to test succeeded: its release's suite waits its turn — the newest only. */
  queueSuite(job) {
    const s = { request: job.request, release: job.release, sha: job.sha, tickets: this.auto.triggers[job.request] || [] };
    const was = this.suites.pending;
    if (was) this.log(`suite: ${was.release} passed over for ${s.release} before it began — its suite never runs (remote prod refuses it)`);
    this.suites.pending = s;
    this.log(`suite: ${s.release} waits${this.suites.running ? ` for ${this.suites.running.release}'s` : ""} (request ${s.request})`);
    this.saveAuto();
    queueMicrotask(() => this.drainSuites());
    return s;
  }

  async drainSuites() {
    if (this.suites.busy) return;
    this.suites.busy = true;
    try {
      while (this.suites.pending) {
        const s = this.suites.running = this.suites.pending;
        this.suites.pending = null;
        this.saveAuto();
        try { await this.runSuite(s); } finally { this.suites.running = null; this.saveAuto(); }
      }
    } finally { this.suites.busy = false; }
  }

  /** One suite: `deploy/pipeline suite <release>`, its log beside the request's; red is reported as a red run is. */
  async runSuite(s) {
    const dir = path.join(this.o.stateDir, "runs", String(s.request));
    fs.mkdirSync(dir, { recursive: true });
    this.log(`suite: ${s.release} — running (request ${s.request})`);
    const res = await this.exec({ request: s.request }, { ...SUITE, argv: () => SUITE.argv(s) }, path.join(dir, "suite.log"));
    this.keepLogs(s, SUITE, dir, res);
    const outcome = res.code === 0 ? "passed" : "failed";
    this.suites.last = { request: s.request, release: s.release, outcome, tickets: s.tickets, at: new Date().toISOString() };
    this.log(`suite: ${s.release} — ${outcome}`);
    this.saveAuto();
    if (outcome === "failed") {
      await this.reportRed({ request: s.request, sha: s.sha, release: s.release, red: { step: "suite", out: res.out, logs: res.logs } }, s.tickets);
    } else if (s.tickets.length && this.editor()) {
      const chat = await this.chatOf(s.tickets[0]);
      if (chat) {
        try { await this.menger("chats.post", { chat, body: `The suite of ${s.release} (deployed to test by request ${s.request}) passed: it may go to prod (DEPLOY-52).` }, "editor"); }
        catch (e) { this.log(`suite: could not post to chat ${chat} — ${e.message}`); }
      }
    }
    return outcome;
  }

  /** --watch's or --listen's companion: `tickets.watch`, each trigger debounced; again after a dropped socket. */
  async watchTickets() {
    this.saveAuto();
    for (;;) {
      try {
        const ws = await connect(this.o.url, this.headers());
        this.log(`auto-test: watching ${this.o.url} for ${this.o.project}'s tickets entering In Review`);
        await call(ws, 2, "tickets.watch", {}, (it) => {
          const t = it?.type === "data" ? triggerOf(it.content, this.o.project) : null;
          if (t) this.trigger(t);
        }).catch((e) => this.log(`auto-test: watch: ${e.message}`));
        ws.close();
      } catch (e) { this.log(`auto-test: watch: ${e.message}`); }
      await new Promise((r) => setTimeout(r, 5000));
    }
  }

  urlOf(job, s) {
    return this.o.listen ? `http://127.0.0.1:${this.o.port}/runs/${job.request}/${s.name}.log` : undefined;
  }

  /** CI-6: the logs the pipeline said it kept (`log: <path>`) are copied beside the step's, so GET /runs/<request>/ serves the real test output. */
  keepLogs(job, s, dir, res) {
    res.logs = [];
    for (const m of res.out.matchAll(/^(?:pipeline: .* — |log: )(\/\S+\.log)$/gm)) {
      const from = m[1];
      if (res.logs.some((l) => l.from === from)) continue;
      const name = `${s.name}-${path.basename(from)}`.replace(/[^A-Za-z0-9._-]/g, "_");
      try { fs.copyFileSync(from, path.join(dir, name)); res.logs.push({ from, name }); } catch { /* not readable here: the path is still said */ }
    }
  }

  /** One step's command, its output kept; stopped (when it may be) on a cancel. */
  exec(job, s, logFile) {
    return new Promise((resolve) => {
      const out = fs.createWriteStream(logFile);
      let text = "", canceled = false, timer = null;
      const keep = (b) => { out.write(b); text = (text + b).slice(-64 * 1024); };
      const env = { ...process.env, ...(s.env || {}), MENGER_DEPLOY_TRACKER_URL: this.o.url };
      if (s.records) env.MENGER_REQUEST = String(job.request);
      const fakeSecs = Number(process.env.MENGER_RUNNER_FAKE_SECS || 2);
      const fail = process.env.MENGER_RUNNER_FAKE_FAIL === s.name;
      const child = this.o.fake
        ? spawn(process.execPath, ["-e", `console.log(${JSON.stringify(`fake ${s.name}: ${Object.entries(s.env || {}).map(([k, v]) => `${k}=${v} `).join("")}deploy/pipeline ${s.argv(job).join(" ")}`)}); setTimeout(() => { console.log(${JSON.stringify(fail ? `${s.name} FAILED: the fake's failure` : `${s.name} ok`)}); process.exit(${fail ? 1 : 0}); }, ${fakeSecs * 1000});`], { env })
        : spawn(this.o.pipeline, s.argv(job), { cwd: this.o.repo, env });
      child.stdout.on("data", keep); child.stderr.on("data", keep);
      if (s.interruptible) {
        timer = setInterval(async () => {
          const now = await this.standing(job);
          if (now && (now.state === "canceled" || now.cancel_asked_at)) { canceled = true; child.kill("SIGTERM"); setTimeout(() => child.kill("SIGKILL"), 10_000).unref(); }
        }, Number(process.env.MENGER_RUNNER_POLL_SECS || 5) * 1000);
      }
      child.on("close", (code) => { clearInterval(timer); out.end(); resolve({ code: code ?? 1, out: text, canceled }); });
      child.on("error", (e) => { clearInterval(timer); keep(`runner: ${e.message}\n`); out.end(); resolve({ code: 127, out: text, canceled }); });
    });
  }

  /** --listen: the webhook, and each run's logs. */
  listen(secret) {
    const server = http.createServer((req, res) => {
      const answer = (code, text) => { res.writeHead(code, { "content-type": "text/plain; charset=utf-8" }); res.end(text + "\n"); };
      if (req.method === "GET") {
        if (req.url === "/health") return answer(200, "ok");
        const m = /^\/runs\/(\d+)\/([A-Za-z0-9._-]+)\.log$/.exec(req.url || "");
        if (!m) return answer(404, "not here");
        const f = path.join(this.o.stateDir, "runs", m[1], `${m[2]}.log`);
        return fs.existsSync(f) ? answer(200, fs.readFileSync(f, "utf8")) : answer(404, "no such log");
      }
      if (req.method !== "POST") return answer(405, "POST a deploy request's webhook");
      const chunks = []; let size = 0;
      req.on("data", (c) => { size += c.length; if (size > 64 * 1024) req.destroy(); else chunks.push(c); });
      req.on("end", () => {
        const body = Buffer.concat(chunks);
        if (!signed(secret, body, req.headers["x-menger-signature"])) return answer(401, "bad signature");
        let p; try { p = JSON.parse(body.toString("utf8")); } catch { return answer(400, "not JSON"); }
        const why = this.take(p);
        return why ? answer(422, why) : answer(202, `queued request ${p.request}`);
      });
    });
    server.listen(this.o.port, "127.0.0.1", () => this.log(`listening on 127.0.0.1:${this.o.port} for ${this.o.project}'s requests (${Object.keys(PLANS).join(", ")})`));
    return server;
  }

  /** --watch: the requests, dialling out; again after a dropped socket. */
  async watch() {
    for (;;) {
      try {
        const pending = await this.menger("deployments.list", { project: this.o.project, state: "requested", limit: 100 });
        for (const r of pending.reverse()) this.consider(r.deployment);
        const ws = await connect(this.o.url, this.headers());
        this.log(`watching ${this.o.url} for ${this.o.project}'s requests (${Object.keys(PLANS).join(", ")})`);
        await call(ws, 1, "deployments.watch", {}, (it) => {
          if (it?.type === "data" && it.content?.type === "request") this.consider(it.content.deployment);
        }).catch((e) => this.log(`watch: ${e.message}`));
        ws.close();
      } catch (e) { this.log(`watch: ${e.message}`); }
      await new Promise((r) => setTimeout(r, 5000));
    }
  }

  /** A watched request: taken when it is pending, to a planned environment, and no one has stepped it. */
  consider(d) {
    const q = d?.requested;
    if (!q || q.state !== "pending" || (q.steps || []).length || !PLANS[d.environment] || this.taken.has(d.id)) return;
    this.take({ request: d.id, project: d.project, environment: d.environment, revision: d.revision, branch: q.branch, release: d.release, requested_by: d.recorded_by });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(fs.realpathSync(process.argv[1])).href) {
  let o, secret = null;
  try { o = options(process.argv.slice(2)); if (o.listen) secret = secretOf(o.secretFile); }
  catch (e) { process.stderr.write(`deploy-runner: ${e.message}\n`); process.exit(1); }
  const r = new Runner(o);
  if (o.listen) r.listen(secret);
  if (o.watch) r.watch();
  if (o.autoTest) r.watchTickets(); else { r.saveAuto(); r.log("auto-test: off (MENGER_RUNNER_AUTO_TEST=0)"); }
}
