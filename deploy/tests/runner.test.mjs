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
import { GUARDS, PLANS, Runner, failingLines, ofProject, options, releaseIn, shaOf, signed, stepLog, tail, triggerOf, unowned } from "../runner.mjs";

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

test("DEPLOY-47: an ended step's detail is its command and its last 40 lines, at most 4000 bytes", () => {
  const log = Array.from({ length: 100 }, (_, i) => `line ${i}`).join("\n") + "\n";
  const d = stepLog("deploy/pipeline build abc1234", log);
  assert.equal(d.split("\n")[0], "$ deploy/pipeline build abc1234");
  assert.equal(d.split("\n").length, 41);
  assert.equal(d.split("\n")[1], "line 60");
  assert.ok(d.endsWith("line 99"));
  assert.ok(Buffer.byteLength(stepLog("x", "y".repeat(9000) + "\nend")) <= 4000);
  assert.equal(stepLog("x", "", 40, 4000, 2), "$ x\nexit 2");
  assert.equal(stepLog("x", ""), "$ x");
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
  // DEPLOY-47: a step that succeeded sends its log's tail, so the app can quote it.
  const done = r.calls.find(([m, p]) => m === "deployments.step" && p.name === "app-test" && p.state === "succeeded").at(1).detail;
  assert.match(done, /^\$ deploy\/pipeline remote test --image \S+\nfake pipeline: remote test --image /);
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

test("DEPLOY-42: a runner-carried deploy test names its release in the runner's own record", async () => {
  const r = prodRunner();
  process.env.FAKE_PIPELINE_RELEASE = REL;
  const recs = [];
  const call = r.menger;
  r.menger = async (m, p) => { if (m === "deployments.record") recs.push(p); return call(m, p); };
  r.standing = async () => ({ state: "started" }); // the pipeline's own record never landed
  await r.run(job({ environment: "test", revision: "HEAD", release: "" }));
  delete process.env.FAKE_PIPELINE_RELEASE;
  assert.equal(recs.length, 1);
  assert.equal(recs[0].environment, "test");
  assert.equal(recs[0].release, REL);
  assert.equal(releaseIn("a\nrelease 20261019T213135Z-e6efbff\nb"), "20261019T213135Z-e6efbff");
  assert.equal(releaseIn("nothing"), null);
});

// ── DEPLOY-48: entering In Review deploys test ─────────────────────────────
const T = (id, status, address = `hypermemetic:MENGER/${id.replace(/-\d+$/, "")}/${id}`) => ({ id, status, address, epic: id.replace(/-\d+$/, "") });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

test("the trigger: a ticket of the project entering In Review, or a commit linked while it is there", () => {
  assert.deepEqual(triggerOf({ type: "status_changed", ticket: T("CHAT-58", "In Review"), previous: "Coding" }, "MENGER"), { ticket: "CHAT-58", why: "entered In Review" });
  assert.deepEqual(triggerOf({ type: "commits_linked", ticket: T("CHAT-58", "In Review"), added: ["fe3eeaa"] }, "MENGER"), { ticket: "CHAT-58", why: "1 commit(s) linked in review" });
  assert.equal(triggerOf({ type: "commits_linked", ticket: T("CHAT-58", "Coding"), added: ["fe3eeaa"] }, "MENGER"), null, "a commit while Coding waits");
  assert.equal(triggerOf({ type: "status_changed", ticket: T("CHAT-58", "Coding"), previous: "In Review" }, "MENGER"), null, "back to Coding triggers nothing");
  assert.equal(triggerOf({ type: "status_changed", ticket: T("CHAT-58", "Complete"), previous: "In Review" }, "MENGER"), null);
  assert.equal(triggerOf({ type: "amended", ticket: T("CHAT-58", "In Review") }, "MENGER"), null);
  assert.equal(triggerOf({ type: "status_changed", ticket: T("KILN-4", "In Review", "hypermemetic:PANTRY/KILN-4"), previous: "Coding" }, "MENGER"), null, "another project's");
  assert.ok(ofProject({ address: "hypermemetic:MENGER/RUN/DEPLOY-48" }, "menger"));
  assert.ok(ofProject({ address: "hypermemetic:MENGER-1" }, "MENGER"));
  assert.ok(!ofProject({ address: "acme:MENGERX/A-1" }, "MENGER"));
  assert.ok(ofProject({ epic: "MENGER" }, "MENGER"), "no address: its epic");
});

/** A runner whose menger calls are answered here; `pending`: a request pending on the tracker. */
function autoRunner({ pending = null, extra = {} } = {}) {
  const r = runner({ debounceMs: 40, autoTest: true, chat: 6, ...extra });
  r.editorSeed = crypto.randomBytes(32); // a throwaway editor key: no call leaves the test
  let next = 100;
  r.calls = [];
  r.menger = async (method, params, as) => {
    r.calls.push([method, params, as ?? null]);
    if (method === "deployments.list") return pending ? [{ type: "deployment", deployment: { id: pending, requested: { state: "pending", steps: [] } } }] : [];
    if (method === "deployments.request") return [{ type: "request", deployment: { id: ++next, project: "MENGER", recorded_by: "key:0ca8" }, carried: [] }];
    if (method === "tickets.create") return [{ type: "created", ticket: { id: "CHAT-60" } }];
    if (method === "chats.list") return params.ticket === "CHAT-58" ? [{ type: "chat", chat: { id: 41 } }] : [];
    return [{ type: "ok" }];
  };
  r.requests = () => r.calls.filter(([m]) => m === "deployments.request").map(([, p]) => p);
  return r;
}

test("debounce: a burst of triggers is one request, filed once the burst is quiet, for main's tip", async () => {
  const r = autoRunner();
  r.trigger({ ticket: "CHAT-58", why: "entered In Review" });
  await sleep(15);
  r.trigger({ ticket: "CHAT-59", why: "entered In Review" });
  await sleep(15);
  r.trigger({ ticket: "CHAT-58", why: "1 commit(s) linked in review" });
  assert.equal(r.requests().length, 0, "nothing before the quiet");
  await sleep(90);
  assert.equal(r.requests().length, 1);
  assert.deepEqual({ ...r.requests()[0], note: undefined }, { project: "MENGER", environment: "test", branch: "main", note: undefined });
  assert.match(r.requests()[0].note, /CHAT-58 1 commit\(s\) linked in review; CHAT-59 entered In Review/);
  assert.equal(r.queue.length, 1, "taken at once, not left for the watch");
  assert.deepEqual([r.queue[0].request, r.queue[0].branch, r.queue[0].revision], [101, "main", ""]);
  assert.deepEqual(r.auto.triggers[101], ["CHAT-58", "CHAT-59"]);
  const doc = JSON.parse(fs.readFileSync(path.join(r.o.stateDir, "auto.json"), "utf8"));
  assert.deepEqual([doc.on, doc.pending], [true, { request: 101, tickets: ["CHAT-58", "CHAT-59"] }]);
});

test("coalesce: one pending request at most — triggers fold into the one queued here, or the tracker's", async () => {
  const r = autoRunner();
  r.trigger({ ticket: "CHAT-58", why: "entered In Review" });
  await sleep(70);
  r.trigger({ ticket: "DEPLOY-47", why: "entered In Review" });
  await sleep(70);
  assert.equal(r.requests().length, 1, "folded into request 101, still queued");
  assert.deepEqual(r.auto.triggers[101], ["CHAT-58", "DEPLOY-47"]);
  const t = autoRunner({ pending: 77 });
  t.trigger({ ticket: "CHAT-59", why: "entered In Review" });
  await sleep(70);
  assert.equal(t.requests().length, 0, "a → test pending on the tracker takes it");
  assert.deepEqual(t.auto.triggers[77], ["CHAT-59"]);
  // the request under way is not the pending one: a trigger while it runs files the next
  const u = autoRunner({ pending: 77 });
  u.current = { request: 77, environment: "test" };
  u.trigger({ ticket: "CHAT-59", why: "entered In Review" });
  await sleep(70);
  assert.equal(u.requests().length, 1);
});

test("re-entering review: Coding's commits wait, and go when the ticket is In Review again", async () => {
  const r = autoRunner();
  const feed = (ev) => { const t = triggerOf(ev, "MENGER"); if (t) r.trigger(t); };
  feed({ type: "status_changed", ticket: T("CHAT-58", "Coding"), previous: "In Review" });
  feed({ type: "commits_linked", ticket: T("CHAT-58", "Coding"), added: ["a1b2c3d"] });
  await sleep(70);
  assert.equal(r.requests().length, 0, "nothing while Coding");
  feed({ type: "status_changed", ticket: T("CHAT-58", "In Review"), previous: "Coding" });
  await sleep(70);
  assert.equal(r.requests().length, 1);
});

test("the off switch: MENGER_RUNNER_AUTO_TEST=0 (or --no-auto-test) files nothing", async () => {
  assert.equal(options(["--watch"], { MENGER_RUNNER_AUTO_TEST: "0" }).autoTest, false);
  assert.equal(options(["--watch", "--no-auto-test"], {}).autoTest, false);
  const on = options(["--watch"], {});
  assert.deepEqual([on.autoTest, on.debounceMs, on.chat], [true, 60_000, 6]);
  assert.equal(options(["--watch", "--debounce-secs", "5"], {}).debounceMs, 5000);
  const r = autoRunner({ extra: { autoTest: false } });
  assert.equal(r.trigger({ ticket: "CHAT-58", why: "entered In Review" }), false);
  await sleep(70);
  assert.equal(r.calls.length, 0);
});

test("a red auto-test: the request failed, a gap blocking each triggering ticket, the failing lines in its chat", async () => {
  const r = autoRunner({ extra: { fake: false, pipeline: path.join(import.meta.dirname, "fake-pipeline") } });
  process.env.FAKE_PIPELINE_LOG = path.join(r.o.stateDir, "pipeline.log");
  process.env.FAKE_PIPELINE_FAIL = "build";
  r.auto.triggers[101] = ["CHAT-58", "CHAT-59"];
  r.auto.pending = { request: 101, tickets: ["CHAT-58", "CHAT-59"] };
  r.standing = async () => ({ state: "started" });
  r.queue.push({ request: 101, project: "MENGER", environment: "test", revision: "", branch: "HEAD", release: "", requestedBy: "key:0ca8" });
  delete r.drain; // the real one
  await r.drain();
  process.env.FAKE_PIPELINE_FAIL = "";
  const of = (m) => r.calls.filter(([x]) => x === m);
  assert.deepEqual(of("deployments.record").map(([, p]) => p.state), ["failed"]);
  const [[, gap, as]] = of("tickets.create");
  assert.deepEqual([gap.kind, gap.parent, gap.epic, as], ["gap", "CHAT-58", "CHAT", "editor"]);
  assert.match(gap.title, /^test deploy red: request 101 \(build\)/);
  assert.deepEqual(of("tickets.update").map(([, p]) => [p.id, p.block]), [["CHAT-58", ["CHAT-60"]], ["CHAT-59", ["CHAT-60"]]]);
  const [[, post]] = of("chats.post");
  assert.equal(post.chat, 41, "the triggering ticket's chat");
  assert.match(post.body, /FAILED: the fake's failure/);
  assert.match(post.body, /Filed CHAT-60, blocking CHAT-58, CHAT-59/);
  assert.equal(r.auto.pending, null);
  assert.equal(r.auto.running, null, "ended: running no more");
  assert.deepEqual([r.auto.last.request, r.auto.last.outcome], [101, "failed"]);
});

test("CI-6: the pipeline's own logs are copied into the run's directory and named in the red post", async () => {
  const r = autoRunner({ extra: { fake: false, pipeline: path.join(import.meta.dirname, "fake-pipeline") } });
  const kept = path.join(r.o.stateDir, "20261020T100000Z-abc1234-gate-workspace.log");
  process.env.FAKE_PIPELINE_LOG = path.join(r.o.stateDir, "pipeline.log");
  process.env.FAKE_PIPELINE_FAIL = "build";
  process.env.FAKE_PIPELINE_KEEP_LOG = kept;
  r.auto.triggers[102] = ["CHAT-58"];
  r.auto.pending = { request: 102, tickets: ["CHAT-58"] };
  r.standing = async () => ({ state: "started" });
  r.queue.push({ request: 102, project: "MENGER", environment: "test", revision: "", branch: "HEAD", release: "", requestedBy: "key:0ca8" });
  delete r.drain;
  await r.drain();
  process.env.FAKE_PIPELINE_FAIL = ""; delete process.env.FAKE_PIPELINE_KEEP_LOG;
  const copy = path.join(r.o.stateDir, "runs", "102", "build-20261020T100000Z-abc1234-gate-workspace.log");
  assert.match(fs.readFileSync(copy, "utf8"), /test b::two \.\.\. FAILED/);
  const [[, post]] = r.calls.filter(([x]) => x === "chats.post");
  assert.ok(post.body.includes(`Test output: ${copy} (kept at ${kept})`), post.body);
  assert.match(post.body, /test a::one \.\.\. FAILED/);
});

test("a red run's failing lines: cargo's and node's, else the tail", () => {
  const cargo = "running 3 tests\ntest a ... ok\ntest store::b ... FAILED\n\nfailures:\n\n---- store::b stdout ----\nthread 'store::b' panicked at src/x.rs:4:5:\nleft == right\ntest result: FAILED. 2 passed; 1 failed\n";
  assert.equal(failingLines(cargo), "test store::b ... FAILED\n---- store::b stdout ----\nthread 'store::b' panicked at src/x.rs:4:5:\ntest result: FAILED. 2 passed; 1 failed");
  assert.equal(failingLines("a\nb\nc", 2), "b\nc");
});

test("a red auto-test with no editor key posts and files nothing", async () => {
  const r = autoRunner();
  r.editorSeed = null;
  await r.reportRed({ request: 5, sha: "abc1234", red: { step: "build", out: "x" } }, ["CHAT-58"]);
  assert.deepEqual(r.calls, []);
});
