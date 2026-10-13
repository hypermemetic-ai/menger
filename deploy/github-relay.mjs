#!/usr/bin/env node
// DEPLOY-14: the zero-change option — GitHub's own `deployment_status`
// webhook, relayed to menger. A job that names an `environment:` already
// makes GitHub create a deployment and post its statuses; point a webhook
// (Settings → Webhooks, content type application/json, the "Deployment
// statuses" event, a secret) at this relay and every such deployment is
// recorded, with no change to any workflow. Each delivery is checked
// against the secret (X-Hub-Signature-256), written to a temporary file and
// handed to
//
//   deploy/record --from github-event FILE --previous-from-menger [--commits-from-git --repo R] --send --url $MENGER_URL
//
// so the record is DEPLOY-10's, and the deploy key signs it in as anywhere.
// Node ≥ 18, nothing to install.
//
//   GITHUB_WEBHOOK_SECRET=…  MENGER_URL=wss://menger.example.com  MENGER_DEPLOY_KEY=…  \
//     node deploy/github-relay.mjs [--port 8787] [--project PANTRY] [--repo /srv/clones/pantry]
//
// --project: the shortcode when the repository's name is not one (default:
// the payload's own project, else the repository's name). --repo: a clone
// to list commits from (fetched before each delivery); without one the
// record carries the revision and previous only, and menger works from the
// commits it has seen. Answers 202 once recorded, 204 for an event that is
// no deployment (a ping, an `inactive` status), 401 for a bad signature,
// 502 when menger refused it.
import http from "node:http";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFile } from "node:child_process";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const opt = (name, def) => { const i = args.indexOf(name); return i >= 0 ? args[i + 1] : def; };
const port = Number(opt("--port", process.env.PORT || 8787));
const project = opt("--project", process.env.MENGER_PROJECT || "");
const repo = opt("--repo", process.env.MENGER_REPO || "");
const secret = process.env.GITHUB_WEBHOOK_SECRET;
const url = process.env.MENGER_URL;
if (!secret || !url) { console.error("github-relay: set GITHUB_WEBHOOK_SECRET and MENGER_URL (and MENGER_DEPLOY_KEY under login)"); process.exit(1); }

const signed = (body, sig) => {
  const want = "sha256=" + crypto.createHmac("sha256", secret).update(body).digest("hex");
  return typeof sig === "string" && sig.length === want.length && crypto.timingSafeEqual(Buffer.from(sig), Buffer.from(want));
};
const run = (cmd, argv) => new Promise((resolve) =>
  execFile(cmd, argv, { env: process.env, maxBuffer: 1 << 20 }, (err, stdout, stderr) =>
    resolve({ code: err ? (err.code ?? 1) : 0, stdout, stderr })));

http.createServer((req, res) => {
  const chunks = [];
  req.on("data", (c) => chunks.push(c));
  req.on("end", async () => {
    const body = Buffer.concat(chunks);
    const answer = (code, text) => { res.writeHead(code, { "content-type": "text/plain" }); res.end(text + "\n"); };
    if (req.method !== "POST") return answer(405, "POST a GitHub webhook delivery");
    if (!signed(body, req.headers["x-hub-signature-256"])) return answer(401, "bad signature");
    const event = req.headers["x-github-event"];
    if (event !== "deployment_status") return answer(204, `${event}: not a deployment status`);
    let payload; try { payload = JSON.parse(body); } catch { return answer(400, "not JSON"); }
    if (payload.deployment_status?.state === "inactive") return answer(204, "inactive: a later deployment replaced it");
    const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), "menger-relay-")), "event.json");
    fs.writeFileSync(file, body);
    try {
      const argv = ["--from", "github-event", file, "--previous-from-menger", "--send", "--url", url, "--agent", "github-relay"];
      if (project) argv.push("--project", project);
      if (repo) {
        await run("git", ["-C", repo, "fetch", "--quiet", "--no-tags"]);
        argv.push("--commits-from-git", "--repo", repo);
      }
      const r = await run(path.join(HERE, "record"), argv);
      process.stderr.write(r.stderr);
      if (r.code !== 0) return answer(502, `menger refused it (exit ${r.code}): ${(r.stdout + r.stderr).trim().slice(0, 500)}`);
      answer(202, r.stdout.trim());
    } finally {
      fs.rmSync(path.dirname(file), { recursive: true, force: true });
    }
  });
}).listen(port, () => console.error(`github-relay: listening on :${port}, recording into ${url}`));
