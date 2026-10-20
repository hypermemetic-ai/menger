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
//   test
//   build   deploy/pipeline build <sha>   the branch's tip (the request's revision, else
//                                         `git rev-parse <branch>`) → the full suite → a release;
//                                         stopped when the request is canceled
//   deploy  deploy/pipeline deploy test   the release → test, smoke checks; its records name the
//                                         request (MENGER_REQUEST); runs to its end once begun
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
// and only for a release test passed (a `succeeded` record to test naming it).
// Otherwise its first step is failed with the refusal, the rest skipped, and
// nothing runs. A failed step stops the plan: what app-prod runs stays.
//
// One request at a time, in the order taken. Each step's output is kept in
// --state-dir/runs/<request>/<step>.log and served at GET /runs/<request>/<step>.log
// (the step's url); DEPLOY-47: every ended step's detail is its command and its
// last 40 lines (stepLog, at most 4000 bytes), so the app has the log to quote
// when the owner asks the builder about it — the url is this machine's alone.
//
//   node deploy/runner.mjs --listen [--port 4475] --secret-file ~/.config/menger/deploy-runner.secret
//   node deploy/runner.mjs --watch
//   options: --url wss://app.menger.sh/ws (MENGER_URL)   the tracker
//            --project MENGER                            whose requests
//            --repo DIR --pipeline FILE                  the checkout and its deploy/pipeline (default: this one's)
//            --state-dir DIR                             default ~/.local/state/menger-deploy-runner
//            --owner PRINCIPAL                           who may ask for app-prod (repeat; MENGER_RUNNER_OWNERS)
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
    { name: "build", argv: (job) => ["build", job.sha], interruptible: true },
    { name: "deploy", argv: () => ["deploy", "test"], records: true },
  ],
  "app-prod": [
    { name: "image", argv: (job) => ["image", job.release], env: { IMAGE_STORE: "ghcr" }, interruptible: true },
    { name: "app-test", argv: (job) => ["remote", "test", "--image", `${IMAGE_GHCR}:${job.release}`] },
    { name: "app-prod", argv: () => ["remote", "prod"], records: true },
  ],
};

/** DEPLOY-40: what a request to an environment needs beyond its plan. */
export const GUARDS = {
  "app-prod": { owner: true, tested: "test" },
};

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

function options(argv, env = process.env) {
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
  }

  headers() {
    return this.seed ? { Authorization: `Bearer ${keyProof(this.seed, this.o.url.replace(/\/+$/, ""), this.o.agent)}` } : {};
  }

  /** One menger call on a socket of its own: its data items. */
  async menger(method, params) {
    const ws = await connect(this.o.url, this.headers());
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
    try { while (this.queue.length) await this.run(this.queue.shift()); } finally { this.busy = false; }
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
    return passed ? null : `refused: ${job.release} never passed ${g.tested} — promote only a release ${g.tested} passed`;
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
      const cmd = `${Object.entries(s.env || {}).map(([k, v]) => `${k}=${v} `).join("")}deploy/pipeline ${s.argv(job).join(" ")}`;
      const begun = await this.step(job, s.name, "started", { detail: cmd, url: this.urlOf(job, s) });
      if (begun.refused || begun.request?.cancel_asked_at) {
        outcome = "canceled";
        if (!begun.refused) await this.step(job, s.name, "canceled");
        break;
      }
      const res = await this.exec(job, s, path.join(dir, `${s.name}.log`));
      if (s.records) recorded = true;
      // The runner's own closing record must name the release too, or it overwrites the pipeline's with none.
      if (!GUARDS[job.environment]) job.release ||= releaseIn(res.out) || "";
      if (res.canceled) { outcome = "canceled"; await this.step(job, s.name, "canceled", { detail: stepLog(cmd, res.out) }); break; }
      if (res.code !== 0) { outcome = "failed"; await this.step(job, s.name, "failed", { detail: stepLog(cmd, res.out, 40, 4000, res.code), url: this.urlOf(job, s) }); break; }
      await this.step(job, s.name, "succeeded", { detail: stepLog(cmd, res.out), url: this.urlOf(job, s) });
    }
    for (const s of plan.slice(i + 1)) await this.step(job, s.name, outcome === "canceled" ? "canceled" : "skipped");
    this.log(`request ${job.request}: ${outcome}`);
    // The pipeline's own records name the request once its deploy began; else the runner says how it ended
    // (and when a recording step ended before it recorded: the request still open).
    const now = await this.standing(job);
    if (now?.state === "canceled") return; // canceled outright: nothing ran to record
    if (recorded && !this.o.fake && !["pending", "started"].includes(now?.state)) return;
    await this.record(job, outcome === "succeeded" ? "succeeded" : "failed");
  }

  urlOf(job, s) {
    return this.o.listen ? `http://127.0.0.1:${this.o.port}/runs/${job.request}/${s.name}.log` : undefined;
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
        const m = /^\/runs\/(\d+)\/([a-z0-9._-]+)\.log$/.exec(req.url || "");
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
}
