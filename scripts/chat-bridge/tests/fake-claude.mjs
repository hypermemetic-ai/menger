#!/usr/bin/env node
// CHAT-11's test double for `claude -p` — CHAT-18: `--output-format
// stream-json --verbose`, as the bridge runs it. Reads the prompt on stdin,
// streams the events a turn makes (init, interim text, tool calls — one of
// them carrying a secret the bridge must mask — a tool result), then one
// `result` naming the session it resumed (or a new one) and the last line of
// the prompt. FAKE_CLAUDE_STEP (ms, 300) spaces the events; FAKE_CLAUDE_SLEEP
// (s) holds the result, as a slow turn would; FAKE_CLAUDE_MARK (a file)
// gets one line per run, the unix ms of its first output.
import crypto from "node:crypto";
import fs from "node:fs";
const argv = process.argv.slice(2);
if (!argv.includes("stream-json") || !argv.includes("--verbose")) {
  process.stderr.write("fake-claude: run as --output-format stream-json --verbose\n");
  process.exit(2);
}
const r = argv.indexOf("--resume");
const session = r >= 0 ? argv[r + 1] : crypto.randomUUID();
const prompt = fs.readFileSync(0, "utf8");
const last = prompt.trim().split("\n").pop();
const step = Number(process.env.FAKE_CLAUDE_STEP ?? 300);
const sleep = (ms) => new Promise((res) => setTimeout(res, ms));
const say = (ev) => process.stdout.write(JSON.stringify({ ...ev, session_id: session }) + "\n");
const tool = (name, input) => say({ type: "assistant", message: { content: [{ type: "tool_use", id: crypto.randomUUID(), name, input }] } });

if (process.env.FAKE_CLAUDE_MARK) fs.appendFileSync(process.env.FAKE_CLAUDE_MARK, `${Date.now()}\n`);
say({ type: "system", subtype: "init", cwd: process.cwd(), tools: ["Bash", "Read"] });
await sleep(step);
say({ type: "assistant", message: { content: [{ type: "text", text: "Looking at it now." }] } });
await sleep(step);
tool("Bash", { command: "ls -la", description: "List the files" });
say({ type: "user", message: { content: [{ type: "tool_result", content: "a\nb" }] } });
await sleep(step);
tool("Read", { file_path: `${process.cwd()}/README.md` });
await sleep(step);
tool("Bash", { command: "MENGER_TOKEN=supersecret-value cargo test" });
await sleep(Number(process.env.FAKE_CLAUDE_SLEEP || 0) * 1000);
say({ type: "result", subtype: "success", is_error: false, result: `fake reply (${r >= 0 ? "resumed" : "new"} ${session}) to: ${last}` });
