// CHAT-18: the bridge's live line, held — `node --test scripts/chat-bridge/tests/live.test.mjs`.
import assert from "node:assert/strict";
import test from "node:test";
import { batch, clip, describeTool, jsonLines, linesOf, liveLine, mask, readStream } from "../live.mjs";

test("a secret never reaches a line", () => {
  assert.equal(mask("MENGER_TOKEN=abc123 cargo test"), "MENGER_TOKEN=••• cargo test");
  assert.equal(mask("export API_KEY='s3cr3t value'"), "export API_KEY=•••");
  assert.equal(mask('{"access_token": "abc"}'), '{"access_token": •••}');
  assert.equal(mask("The key: it is the cache"), "The key: it is the cache", "prose stays");
  assert.equal(mask("curl -H 'Authorization: Bearer eyJhbGciOi.x.y'"), "curl -H 'Authorization: Bearer •••");
  assert.equal(mask("session mgs_4f9a-Zq_88"), "session mgs_•••");
  assert.equal(mask("seed 54a9677a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e"), "seed •••");
  assert.equal(mask("data aGVsbG8gd29ybGQgSGVsbG8gV29ybGQ1234567890=="), "data •••");
  assert.equal(mask("ghp_abcdefghijklmnop1234"), "ghp_•••");
  // paths and long identifiers stay
  assert.equal(mask("Read tickets-api/src/store/tests_chats_outside_tickets.rs"), "Read tickets-api/src/store/tests_chats_outside_tickets.rs");
  assert.equal(mask("activity_reaches_participants_alone_and_the_memory_log"), "activity_reaches_participants_alone_and_the_memory_log");
});

test("each tool call is a short line", () => {
  const cwd = "/w/menger";
  assert.equal(describeTool("Bash", { command: "deploy/pipeline build", description: "Build the release" }), "Bash: Build the release");
  assert.equal(describeTool("Bash", { command: "x".repeat(300) }).length, "Bash: ".length + 120);
  assert.equal(describeTool("Read", { file_path: "/w/menger/frontend/src/queue.rs" }, cwd), "Read frontend/src/queue.rs");
  assert.equal(describeTool("Edit", { file_path: "/elsewhere/a.rs" }, cwd), "Edit /elsewhere/a.rs");
  assert.equal(describeTool("Read", { file_path: "/var/w/menger/a.rs" }, ["/w/menger", "/var/w/menger"]), "Read a.rs", "the real path too");
  assert.equal(describeTool("Agent", { description: "Find the watch" }), "Agent: Find the watch");
  assert.equal(describeTool("Grep", { pattern: "chats\\.watch", path: "/w/menger/src" }, cwd), "Grep chats\\.watch in src");
  assert.equal(describeTool("Mystery", { a: "b", n: 2, o: {} }), "Mystery a=b n=2");
});

test("a stream's events become lines, its session and result read", () => {
  const ev = { type: "assistant", message: { content: [{ type: "text", text: "Looking at the queue.\n\nThen the tray." }, { type: "tool_use", name: "Bash", input: { command: "TOKEN=zz ls" } }] } };
  assert.deepEqual(linesOf(ev), ["Looking at the queue. Then the tray.", "Bash: TOKEN=••• ls"]);
  assert.deepEqual(linesOf({ type: "user", message: { content: [{ type: "tool_result" }] } }), []);
  const text = [
    JSON.stringify({ type: "system", subtype: "init", session_id: "s-1" }),
    "not json",
    JSON.stringify({ type: "assistant", session_id: "s-1", message: { content: [] } }),
    JSON.stringify({ type: "result", subtype: "success", is_error: false, session_id: "s-1", result: "done" }),
  ].join("\n");
  const r = readStream(text);
  assert.equal(r.session, "s-1");
  assert.equal(r.result.result, "done");
  assert.equal(r.events, 3);
  const seen = [];
  const s = jsonLines((e) => seen.push(e.n));
  s.push('{"n":1}\n{"n"'); s.push(':2}\n\n{"n":3');
  assert.deepEqual(seen, [1, 2]);
});

test("lines are throttled and batched, a heartbeat fills the quiet", () => {
  let t = 0;
  const timers = [];
  const setTimer = (f, ms) => { const h = { at: t + ms, f }; timers.push(h); return h; };
  const clearTimer = (h) => { const i = timers.indexOf(h); if (i >= 0) timers.splice(i, 1); };
  const advance = (ms) => {
    const end = t + ms;
    for (;;) {
      timers.sort((a, b) => a.at - b.at);
      const h = timers[0];
      if (!h || h.at > end) break;
      timers.shift(); t = h.at; h.f();
    }
    t = end;
  };
  const sent = [];
  const live = liveLine((state, line) => sent.push([t, line]), { now: () => t, setTimer, clearTimer });
  live.line("Read a"); // within 300 ms of the start: waits
  live.line("Read b");
  advance(299);
  assert.equal(sent.length, 0);
  advance(1);
  assert.deepEqual(sent, [[300, "Read a · Read b"]]);
  live.line("Edit c");
  advance(300);
  assert.deepEqual(sent.at(-1), [600, "Edit c"]);
  advance(15_399);
  assert.deepEqual(sent.at(-1), [600, "Edit c"], "quiet under 15 s: no heartbeat");
  advance(1);
  assert.deepEqual(sent.at(-1), [16_000, "working · 16s"]);
  live.line("last");
  assert.equal(live.stop(), 3, "stop flushes");
  assert.equal(sent.at(-1)[1], "last");
  const n = sent.length;
  advance(60_000);
  assert.equal(sent.length, n, "no heartbeat after stop");
  assert.equal(batch(["a"]), "a");
  assert.equal(batch(["x".repeat(200), "y".repeat(200)]), clip(`(+1) ${"y".repeat(200)}`));
});
