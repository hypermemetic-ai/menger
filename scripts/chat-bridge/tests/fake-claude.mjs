#!/usr/bin/env node
// CHAT-11's test double for `claude -p --output-format json`: reads the
// prompt on stdin, answers one JSON result naming the session it resumed
// (or a new one) and the last line of the prompt. FAKE_CLAUDE_SLEEP (s)
// holds it, as a slow turn would.
import crypto from "node:crypto";
import fs from "node:fs";
const argv = process.argv.slice(2);
const r = argv.indexOf("--resume");
const session = r >= 0 ? argv[r + 1] : crypto.randomUUID();
const prompt = fs.readFileSync(0, "utf8");
const last = prompt.trim().split("\n").pop();
setTimeout(() => {
  process.stdout.write(JSON.stringify({ type: "result", subtype: "success", is_error: false, session_id: session, result: `fake reply (${r >= 0 ? "resumed" : "new"} ${session}) to: ${last}` }) + "\n");
}, Number(process.env.FAKE_CLAUDE_SLEEP || 0) * 1000);
