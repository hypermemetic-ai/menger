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
// The plan, per environment (only `test` today: a request to another is refused
// 422 — the webhook says why — or, watching, left for its own executor):
//
//   build   deploy/pipeline build <sha>   the branch's tip (the request's revision, else
//                                         `git rev-parse <branch>`) → the full suite → a release;
//                                         stopped when the request is canceled
//   deploy  deploy/pipeline deploy test   the release → test, smoke checks; its records name the
//                                         request (MENGER_REQUEST); runs to its end once begun
//
// One request at a time, in the order taken. Each step's output is kept in
// --state-dir/runs/<request>/<step>.log and served at GET /runs/<request>/<step>.log
// (the step's url); a failed step's last lines are its detail.
//
//   node deploy/runner.mjs --listen [--port 4475] --secret-file ~/.config/menger/deploy-runner.secret
//   node deploy/runner.mjs --watch
//   options: --url wss://app.menger.sh/ws (MENGER_URL)   the tracker
//            --project MENGER                            whose requests
//            --repo DIR --pipeline FILE                  the checkout and its deploy/pipeline (default: this one's)
//            --state-dir DIR                             default ~/.local/state/menger-deploy-runner
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

/** The plan for an environment: its steps, in order. */
export const PLANS = {
  test: [
    { name: "build", argv: (job) => ["build", job.sha], interruptible: true },
    { name: "deploy", argv: () => ["deploy", "test"], records: true },
  ],
};

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
    const rec = { project: job.project, environment: job.environment, state, revision: job.sha, request: job.request,
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
    this.queue.push({ request: id, project: this.o.project, environment: env, revision: p.revision || "", branch: p.branch || "main" });
    this.log(`request ${id}: taken (${env}, ${p.revision || `the tip of ${p.branch || "main"}`})`);
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

  async run(job) {
    const plan = PLANS[job.environment];
    const dir = path.join(this.o.stateDir, "runs", String(job.request));
    fs.mkdirSync(dir, { recursive: true });
    job.sha = this.resolve(job);
    let outcome = "succeeded", recorded = false, i = 0;
    if (!job.sha) {
      await this.step(job, plan[0].name, "failed", { detail: `no commit '${job.revision || job.branch}' in ${this.o.repo}` });
      for (const s of plan.slice(1)) await this.step(job, s.name, "skipped");
      job.sha = "0000000";
      return this.record(job, "failed");
    }
    for (; i < plan.length; i++) {
      const s = plan[i];
      const begun = await this.step(job, s.name, "started", { detail: `deploy/pipeline ${s.argv(job).join(" ")}`, url: this.urlOf(job, s) });
      if (begun.refused || begun.request?.cancel_asked_at) {
        outcome = "canceled";
        if (!begun.refused) await this.step(job, s.name, "canceled");
        break;
      }
      const res = await this.exec(job, s, path.join(dir, `${s.name}.log`));
      if (s.records) recorded = true;
      if (res.canceled) { outcome = "canceled"; await this.step(job, s.name, "canceled", { detail: tail(res.out) || undefined }); break; }
      if (res.code !== 0) { outcome = "failed"; await this.step(job, s.name, "failed", { detail: tail(res.out) || `exit ${res.code}`, url: this.urlOf(job, s) }); break; }
      await this.step(job, s.name, "succeeded", { url: this.urlOf(job, s) });
    }
    for (const s of plan.slice(i + 1)) await this.step(job, s.name, outcome === "canceled" ? "canceled" : "skipped");
    this.log(`request ${job.request}: ${outcome}`);
    // The pipeline's own records name the request once its deploy began; else the runner says how it ended.
    if (recorded && !this.o.fake) return;
    const now = await this.standing(job);
    if (now?.state === "canceled") return; // canceled outright: nothing ran to record
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
      const env = { ...process.env, MENGER_DEPLOY_TRACKER_URL: this.o.url };
      if (s.records) env.MENGER_REQUEST = String(job.request);
      const fakeSecs = Number(process.env.MENGER_RUNNER_FAKE_SECS || 2);
      const fail = process.env.MENGER_RUNNER_FAKE_FAIL === s.name;
      const child = this.o.fake
        ? spawn(process.execPath, ["-e", `console.log("fake ${s.name}: deploy/pipeline ${s.argv(job).join(" ")}"); setTimeout(() => { console.log(${JSON.stringify(fail ? `${s.name} FAILED: the fake's failure` : `${s.name} ok`)}); process.exit(${fail ? 1 : 0}); }, ${fakeSecs * 1000});`], { env })
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
    this.take({ request: d.id, project: d.project, environment: d.environment, revision: d.revision, branch: q.branch });
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
