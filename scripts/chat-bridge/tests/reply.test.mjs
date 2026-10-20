// CHAT-66: a reply as the agent reads it — `node --test scripts/chat-bridge/tests/reply.test.mjs`.
import assert from "node:assert/strict";
import test from "node:test";
import { fmtMessage, quoteText, withoutToken } from "../reply.mjs";

const at = Date.UTC(2026, 9, 20, 5, 6) / 1000;

test("a reply shows what it answers, without its token", () => {
  const m = { id: 516, author: "workos:owner", name: "Owner", at, body: "[↩](message:512) archive, please", reply_to: 512, quote: { id: 512, author: "key:54a9", name: "builder", snippet: "Expired events: archive or delete?" } };
  assert.equal(fmtMessage(m), `[message 516 · Owner (workos:owner) · 2026-10-20 05:06Z]\n(replying to #512: builder: "Expired events: archive or delete?")\narchive, please`);
});

test("a message that is no reply is as it was", () => {
  const m = { id: 7, author: "key:54a9", at, body: "[↩](message:3) is a link here too" };
  assert.equal(fmtMessage(m), `[message 7 · key:54a9 · 2026-10-20 05:06Z]\n[↩](message:3) is a link here too`, "no reply_to: the body untouched");
});

test("a deleted or unseen original is said so", () => {
  assert.equal(quoteText({ id: 1, author: "a", snippet: "", deleted: true }), "a deleted message");
  assert.equal(quoteText(undefined), "a message you can't see");
  assert.equal(quoteText({ id: 1, author: "key:x", snippet: "" }), "key:x");
  assert.equal(withoutToken("[↩](message:4)\nhi"), "hi");
  assert.equal(withoutToken("hi [↩](message:4)"), "hi [↩](message:4)");
});
