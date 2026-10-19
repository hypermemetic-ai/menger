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
import { PLANS, Runner, signed, tail } from "../runner.mjs";

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

test("only test has a plan: build the tip, then deploy test naming the request", () => {
  assert.deepEqual(Object.keys(PLANS), ["test"]);
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
  assert.deepEqual(r.queue[0], { request: 4, project: "MENGER", environment: "test", revision: "", branch: "main" });
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
