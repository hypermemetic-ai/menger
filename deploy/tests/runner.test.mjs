// DEPLOY-37: deploy/runner.mjs's own tests — the webhook's signature, what it
// takes and refuses, a log's tail. No network but loopback, no menger: the
// executor's calls to menger are the throwaway check's
// (scripts/deploy-button-check.mjs). node --test deploy/tests/runner.test.mjs
import test from "node:test";
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { GUARDS, PLANS, Runner, shaOf, signed, tail, unowned } from "../runner.mjs";

const SECRET = "runner-test-secret-0123456789";
const sign = (body) => "sha256=" + crypto.createHmac("sha256", SECRET).update(body).digest("hex");

function runner(extra = {}) {
  const stateDir = fs.mkdtempSync(path.join(os.tmpdir(), "menger-runner-test-"));
  const key = path.join(stateDir, "test.key"); // a throwaway key: the tests call no menger
  fs.writeFileSync(key, crypto.randomBytes(32).toString("base64"), { mode: 0o600 });
  process.env.MENGER_DEPLOY_KEY_FILE = key;
  const r = new Runner({ listen: true, watch: false, fake: true, port: 0, url: "ws://127.0.0.1:1", project: "MENGER", repo: process.cwd(), pipeline: "/bin/false", stateDir, agent: "t", ...extra }, () => {});
  r.drain = async () => {}; // queued, never run
  return r;
}

test("a signature is the body's HMAC under the secret, compared in constant time", () => {
  const body = Buffer.from('{"request":1}');
  assert.ok(signed(SECRET, body, sign(body)));
  assert.ok(!signed(SECRET, body, sign(Buffer.from('{"request":2}'))));
  assert.ok(!signed(SECRET, body, undefined));
  assert.ok(!signed("another-secret-0123456789", body, sign(body)));
});

test("a log's tail: its last lines, at most the bytes asked", () => {
  const log = Array.from({ length: 100 }, (_, i) => `line ${i}`).join("\n") + "\n";
  assert.equal(tail(log, 3), "line 97\nline 98\nline 99");
  assert.ok(Buffer.byteLength(tail("x".repeat(9000) + "\nend", 30, 4000)) <= 4000);
});

test("test's plan: build the tip, then deploy test naming the request", () => {
  assert.deepEqual(Object.keys(PLANS), ["test", "app-prod"]);
  assert.deepEqual(PLANS.test.map((s) => [s.name, s.argv({ sha: "abc1234" })]), [["build", ["build", "abc1234"]], ["deploy", ["deploy", "test"]]]);
  assert.ok(PLANS.test[0].interruptible && !PLANS.test[1].interruptible && PLANS.test[1].records);
});

test("what it takes: its project's requests to a planned environment, once each", () => {
  const r = runner();
  assert.match(r.take({ request: 4, project: "OTHER", environment: "test" }), /deploys MENGER, not OTHER/);
  assert.match(r.take({ request: 4, project: "menger", environment: "prod" }), /no plan for prod/);
  assert.equal(r.take({ request: 4, project: "menger", environment: "test", revision: "", branch: "main" }), null);
  assert.equal(r.take({ request: 4, project: "menger", environment: "test" }), null, "a retry is answered, not queued again");
  assert.equal(r.queue.length, 1);
  assert.deepEqual(r.queue[0], { request: 4, project: "MENGER", environment: "test", revision: "", branch: "main", release: "", requestedBy: "" });
  const again = new Runner(r.o, () => {});
  assert.ok(again.taken.has(4), "what was taken is kept across a restart");
});

test("the webhook: 401 unsigned, 422 unplanned, 202 queued; logs served; nothing else", async () => {
  const r = runner();
  const server = r.listen(SECRET);
  await new Promise((ok) => server.once("listening", ok));
  const base = `http://127.0.0.1:${server.address().port}`;
  const post = (body, sig) => fetch(base + "/", { method: "POST", body, headers: sig ? { "x-menger-signature": sig } : {} });
  const good = JSON.stringify({ request: 9, project: "MENGER", environment: "test", revision: "", branch: "main" });
  assert.equal((await post(good)).status, 401);
  assert.equal((await post(good, sign(Buffer.from(good + " ")))).status, 401);
  const prod = JSON.stringify({ request: 10, project: "MENGER", environment: "prod" });
  const no = await post(prod, sign(Buffer.from(prod)));
  assert.equal(no.status, 422);
  assert.match(await no.text(), /no plan for prod/);
  const yes = await post(good, sign(Buffer.from(good)));
  assert.equal(yes.status, 202);
  assert.equal(r.queue[0].request, 9);
  fs.mkdirSync(path.join(r.o.stateDir, "runs/9"), { recursive: true });
  fs.writeFileSync(path.join(r.o.stateDir, "runs/9/build.log"), "built\n");
  assert.equal(await (await fetch(base + "/runs/9/build.log")).text(), "built\n\n");
  assert.equal((await fetch(base + "/runs/9/../../taken.json")).status, 404);
  assert.equal((await fetch(base + "/health")).status, 200);
  server.close();
});

// ── DEPLOY-40: → prod, app-prod's plan ─────────────────────────────────────
const REL = "20261019T192902Z-8cb79df";
const OWNER = "workos:user_01OWNER";

test("app-prod's plan: the image of the release test passed, app-test, then app-prod naming the request", () => {
  const job = { release: REL };
  assert.deepEqual(PLANS["app-prod"].map((s) => [s.name, s.argv(job), s.env ?? null, !!s.records]), [
    ["image", ["image", REL], { IMAGE_STORE: "ghcr" }, false],
    ["app-test", ["remote", "test", "--image", `ghcr.io/hypermemetic-ai/menger:${REL}`], null, false],
    ["app-prod", ["remote", "prod"], null, true],
  ]);
  assert.deepEqual(GUARDS["app-prod"], { owner: true, tested: "test" });
  assert.equal(shaOf(REL), "8cb79df");
});

test("who may ask for app-prod: an owner the config names, never a key or an agent", () => {
  const job = (requestedBy, release = REL) => ({ environment: "app-prod", requestedBy, release });
  assert.equal(unowned(job(OWNER), [OWNER]), null);
  assert.match(unowned(job("key:54a9677a"), [OWNER, "key:54a9677a"]), /is a key — .* never an agent's/);
  assert.match(unowned(job("agent:builder"), ["agent:builder"]), /is a key/);
  assert.match(unowned(job("workos:user_someone"), [OWNER]), /is not the owner/);
  assert.match(unowned(job(OWNER), []), /no owner \(MENGER_RUNNER_OWNERS\)/);
  assert.match(unowned(job(""), [OWNER]), /names its requester/);
  assert.match(unowned(job(OWNER, ""), [OWNER]), /names the release/);
  assert.equal(unowned({ environment: "test", requestedBy: "key:x" }, []), null, "test is anyone's who may ask");
});

/** A runner over deploy/tests/fake-pipeline, its menger calls answered here: test passed `passed`. */
function prodRunner({ passed = [REL], fail = "" } = {}) {
  const r = runner({ fake: false, pipeline: path.join(import.meta.dirname, "fake-pipeline"), owners: [OWNER] });
  const log = path.join(r.o.stateDir, "pipeline.log");
  process.env.FAKE_PIPELINE_LOG = log;
  process.env.FAKE_PIPELINE_FAIL = fail;
  r.calls = [];
  r.menger = async (method, params) => {
    r.calls.push([method, params]);
    if (method === "deployments.list" && params.state === "succeeded") return passed.map((release) => ({ type: "deployment", deployment: { release } }));
    if (method === "deployments.list") return [];
    if (method === "deployments.step") return [{ type: "request", deployment: { requested: { state: "started", steps: [] } } }];
    return [{ type: "deployment", deployment: {} }];
  };
  r.ran = () => (fs.existsSync(log) ? fs.readFileSync(log, "utf8").trim().split("\n") : []);
  r.steps = () => r.calls.filter(([m]) => m === "deployments.step").map(([, p]) => `${p.name}:${p.state}`);
  r.records = () => r.calls.filter(([m]) => m === "deployments.record").map(([, p]) => `${p.environment}:${p.state}`);
  return r;
}
const job = (over = {}) => ({ request: 21, project: "MENGER", environment: "app-prod", revision: "8cb79df", branch: "main", release: REL, requestedBy: OWNER, ...over });

test("the owner's request runs the three steps in order; only app-prod's records name the request", async () => {
  const r = prodRunner();
  await r.run(job());
  assert.deepEqual(r.ran(), [`ghcr||image ${REL}`, `||remote test --image ghcr.io/hypermemetic-ai/menger:${REL}`, "|21|remote prod"]);
  assert.deepEqual(r.steps(), ["image:started", "image:succeeded", "app-test:started", "app-test:succeeded", "app-prod:started", "app-prod:succeeded"]);
  assert.match(r.calls.find(([m, p]) => m === "deployments.step" && p.name === "image").at(1).detail, /^IMAGE_STORE=ghcr deploy\/pipeline image /);
});

test("a failing step stops the plan: the rest skipped, app-prod never runs, the request failed", async () => {
  const r = prodRunner({ fail: "remote test" });
  await r.run(job());
  assert.deepEqual(r.ran().map((l) => l.split("|")[2].split(" ").slice(0, 2).join(" ")), ["image " + REL, "remote test"]);
  assert.deepEqual(r.steps(), ["image:started", "image:succeeded", "app-test:started", "app-test:failed", "app-prod:skipped"]);
  assert.match(r.calls.find(([m, p]) => m === "deployments.step" && p.state === "failed").at(1).detail, /remote test .*FAILED: the fake's failure/);
  assert.deepEqual(r.records(), ["app-prod:failed"]);
});

test("an agent's request is refused in its first step, and nothing runs", async () => {
  const r = prodRunner();
  await r.run(job({ requestedBy: "key:54a9677a3b0d28" }));
  assert.deepEqual(r.ran(), []);
  assert.deepEqual(r.steps(), ["image:failed", "app-test:skipped", "app-prod:skipped"]);
  assert.match(r.calls.find(([m]) => m === "deployments.step").at(1).detail, /^refused: key:54a9677a3b0d28 is a key/);
  assert.deepEqual(r.records(), ["app-prod:failed"]);
});

test("a release test never passed is refused, and nothing runs", async () => {
  const r = prodRunner({ passed: ["20261018T000000Z-1234567"] });
  await r.run(job());
  assert.deepEqual(r.ran(), []);
  assert.match(r.calls.find(([m]) => m === "deployments.step").at(1).detail, new RegExp(`^refused: ${REL} never passed test`));
  assert.ok(r.calls.some(([m, p]) => m === "deployments.list" && p.environment === "test" && p.state === "succeeded"));
});

test("a watched request to app-prod carries its release and its requester", () => {
  const r = runner({ owners: [OWNER] });
  r.consider({ id: 30, project: "MENGER", environment: "app-prod", revision: "8cb79df", release: REL, recorded_by: OWNER, requested: { state: "pending", steps: [] } });
  assert.deepEqual(r.queue[0], { request: 30, project: "MENGER", environment: "app-prod", revision: "8cb79df", branch: "main", release: REL, requestedBy: OWNER });
});
