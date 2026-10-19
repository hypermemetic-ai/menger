#!/usr/bin/env node
// CHAT-11's acceptance on a throwaway login-on server: an allowed principal
// posts and gets a reply from a spawned agent; a second message resumes the
// same session; a principal not on the list gets nothing; a server restart
// mid-wait, and a bridge restart, lose nothing and answer nothing twice.
// CHAT-14: a direct message and a group, on no ticket, answered too; an
// image passed to the agent as a note. CHAT-18: the owner's watch hears
// `activity started` before the agent's first output, its tool calls as
// lines (a secret masked), `ended` after the answer, posted once; no
// "working…" post where activity is taken. The server must have
// `chats.activity` (CHAT-18): MENGER_BIN names a build that has it until
// prod runs one. CHAT-40: `/help` and `/status` answered by the bridge with
// no agent spawned; `/stop` ends a long turn within 5 s, its session kept
// and resumed; `/clear` then a message starts a new session.
//
//   node scripts/chat-bridge/tests/throwaway.mjs WORKDIR [--real] [--port 6150]
//
// WORKDIR is a scratch directory of the caller's (keys, the database, the
// logs, the bridge's state are left there). The agent is tests/fake-claude.mjs
// unless --real (claude -p, the unit's model). The server is the release prod
// runs, unless MENGER_BIN names one. Node ≥ 22, its standard library only.
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { call, connect, keyProof, seedOf } from "../../../deploy/menger-call.mjs";
import { makeKey } from "../../throwaway-login.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const work = path.resolve(argv.find((a) => !a.startsWith("--") && !/^\d+$/.test(a)) ?? (console.error("usage: throwaway.mjs WORKDIR [--real] [--port N]"), process.exit(1)));
const real = argv.includes("--real");
const port = Number(argv[argv.indexOf("--port") + 1]) || 6150;
const url = `ws://127.0.0.1:${port}`;
const bin = process.env.MENGER_BIN || fs.realpathSync(path.join(os.homedir(), ".local/share/menger-deploy/envs/prod/current/bin/menger"));
fs.mkdirSync(work, { recursive: true });
const waitReply = real ? 600 : 30;

const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a);
let failures = 0;
const check = (ok, what) => { log(`${ok ? "PASS" : "FAIL"} ${what}`); if (!ok) failures++; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const key = (name) => ({ file: path.join(work, `${name}.key`), principal: makeKey(path.join(work, `${name}.key`)), agent: name });
const operator = key("operator"), builder = key("builder"), owner = key("owner"), stranger = key("stranger");

async function rpc(who, method, params) {
  const seed = seedOf(fs.readFileSync(who.file, "utf8"));
  const ws = await connect(url, { Authorization: `Bearer ${keyProof(seed, url, who.agent)}` });
  const out = [];
  try {
    await call(ws, 1, method, params, (item) => {
      if (item?.type === "data") out.push(item.content);
      else if (item?.type === "error") out.push({ type: "error", ...item });
    });
  } finally { ws.close(); }
  const err = out.find((c) => ["error", "refused", "not_found"].includes(c?.type));
  if (err) throw new Error(`${method} as ${who.agent}: ${JSON.stringify(err)}`);
  return out;
}

let server = null;
async function startServer() {
  const out = fs.openSync(path.join(work, "server.log"), "a");
  server = spawn(bin, ["--port", String(port), "--shell-port", String(port + 1), "--db-path", path.join(work, "menger.db"),
    "--auth-provider", "key", "--admin", operator.principal, "--no-rest"],
    { env: { ...process.env, PLEXUS_NO_REGISTRY: "1" }, stdio: ["ignore", out, out] });
  for (let i = 0; i < 120; i++) {
    try { await rpc(operator, "tickets.health", {}); return; } catch { await sleep(500); }
  }
  throw new Error(`the server on ${url} did not come up (${work}/server.log)`);
}
async function stopServer() {
  const s = server; server = null;
  const gone = new Promise((r) => s.once("exit", r));
  s.kill("SIGTERM"); await gone;
}

let bridge = null;
const bridgeLog = path.join(work, "bridge.log");
const allowFile = path.join(work, "allow");
const stateDir = path.join(work, "state");
const markFile = path.join(work, "fake-claude.marks");

// CHAT-18: the owner's watch, held on its own socket — every activity and
// post with the moment it arrived.
const heard = [];
let watchSock = null;
async function holdOwnerWatch() {
  const seed = seedOf(fs.readFileSync(owner.file, "utf8"));
  watchSock = await connect(url, { Authorization: `Bearer ${keyProof(seed, url, owner.agent)}` });
  call(watchSock, 1, "chats.watch", {}, (item) => {
    if (item?.type === "data" && ["activity", "posted"].includes(item.content?.type)) heard.push({ at: Date.now(), ...item.content });
  }).catch(() => {});
  await sleep(300);
}
function startBridge(extra = {}) {
  const out = fs.openSync(bridgeLog, "a");
  const env = {
    ...process.env, MENGER_URL: url, MENGER_KEY_FILE: builder.file, MENGER_AGENT: "builder",
    CHAT_BRIDGE_ALLOW: allowFile, CHAT_BRIDGE_STATE: stateDir, CHAT_BRIDGE_CWD: work, CHAT_BRIDGE_WORKING_AFTER: "0",
    FAKE_CLAUDE_MARK: markFile,
    ...extra,
    CLAUDE_BIN: real ? (process.env.CLAUDE_BIN || path.join(os.homedir(), ".local/bin/claude")) : path.join(here, "fake-claude.mjs"),
  };
  bridge = spawn(process.execPath, [path.join(here, "..", "bridge.mjs"), "--model", "claude-opus-5-5", "--permission-mode", "bypassPermissions"], { env, stdio: ["ignore", out, out] });
}
async function stopBridge() {
  const b = bridge; bridge = null;
  const gone = new Promise((r) => b.once("exit", r));
  b.kill("SIGTERM"); await gone;
}
const bridgeSays = (re) => re.test(fs.readFileSync(bridgeLog, "utf8"));
async function waitLog(re, secs) { for (let i = 0; i < secs * 2; i++) { if (bridgeSays(re)) return true; await sleep(500); } return false; }

async function messages(chat) {
  return (await rpc(owner, "chats.messages", { chat, limit: 1000 })).filter((i) => i.type === "message").map((i) => i.message);
}
const replies = async (chat) => (await messages(chat)).filter((m) => m.author === builder.principal && m.body !== "_working…_");
async function waitReplies(chat, n, secs) {
  for (let i = 0; i < secs; i++) { const r = await replies(chat); if (r.length >= n) return r; await sleep(1000); }
  return replies(chat);
}
const post = async (who, chat, body) => (await rpc(who, "chats.post", { chat, body })).find((i) => i.type === "posted").message;
const turns = () => { try { return fs.readFileSync(path.join(stateDir, "turns.log"), "utf8").trim().split("\n").filter(Boolean).map((l) => JSON.parse(l)); } catch { return []; } };

try {
  log(`server ${bin} on ${url}; work ${work}; agent ${real ? "claude (real)" : "fake-claude"}`);
  await startServer();
  for (const [p, role] of [[builder.principal, "editor"], [owner.principal, "viewer"], [stranger.principal, "viewer"]])
    await rpc(operator, "members.grant", { principal: p, scope: "*", role });
  await rpc(operator, "tickets.create", { epic: "BRIDGE", title: "A ticket to chat on", kind: "scope" });
  // the owner's line admits a key explicitly (+); the stranger's bare key: line is ignored
  fs.writeFileSync(allowFile, `# test\n+${owner.principal}\n${stranger.principal}\n`);
  const chat = (await rpc(owner, "chats.open", { ticket: "BRIDGE-1", others: [builder.principal, stranger.principal] })).find((i) => i.type === "opened").chat.id;
  log(`chat ${chat} on BRIDGE-1: owner, builder, stranger`);

  startBridge();
  check(await waitLog(/watch held/, 30), "the bridge holds the watch");

  // 1. an allowed principal posts and gets a reply from a spawned agent
  await holdOwnerWatch();
  const m1 = await post(owner, chat, "Reply with one short sentence: what ticket is this chat on?");
  let r = await waitReplies(chat, 1, waitReply);
  check(r.length === 1 && r[0].id > m1.id, `a reply to the owner's first message (${r[0]?.body?.slice(0, 120) ?? "none"})`);

  // 1b. CHAT-18: the live line on the owner's watch
  {
    for (let i = 0; i < 20 && !heard.some((h) => h.type === "activity" && h.activity.state === "ended"); i++) await sleep(250);
    const acts = heard.filter((h) => h.type === "activity" && h.activity.chat === chat);
    const started = acts.find((a) => a.activity.state === "started");
    const firstOut = Number((fs.existsSync(markFile) ? fs.readFileSync(markFile, "utf8") : "").split("\n")[0]) || Infinity;
    check(!!started && started.at - 0 <= firstOut + 50 && started.activity.by === builder.principal, `activity started reached the owner before the agent's first output (${started ? started.at - firstOut : "none"} ms)`);
    const lines = acts.map((a) => a.activity.line).filter(Boolean);
    log(`  lines: ${JSON.stringify(lines)}`);
    if (!real) {
      check(lines.some((l) => l.includes("Bash: List the files")) && lines.some((l) => l.includes("Read README.md")) && lines.some((l) => l.includes("Looking at it now.")), "the tool calls and the interim text arrived as lines");
      check(lines.some((l) => l.includes("MENGER_TOKEN=•••")) && !lines.some((l) => l.includes("supersecret")), "the secret was masked");
    } else check(lines.length > 0, "lines arrived");
    const ended = acts.filter((a) => a.activity.state === "ended");
    const answer = heard.find((h) => h.type === "posted" && h.message.id === r[0]?.id);
    check(ended.length === 1 && !!answer && ended[0].activity.seq > Math.max(...acts.filter((a) => a.activity.state !== "ended").map((a) => a.activity.seq)) && ended[0].at >= answer.at, "activity ended once, after the answer was posted");
    check(heard.filter((h) => h.type === "posted" && h.message.author === builder.principal).length === 1, "the answer was posted exactly once");
    const kept = (await rpc(owner, "chats.activity_log", { chat })).filter((i) => i.type === "activity");
    check(kept.length === acts.length, `activity_log keeps what the watch heard (${kept.length}/${acts.length})`);
  }
  const s1 = turns()[0]?.session;
  check(!!s1, `the turn is logged with its session (${s1})`);

  // 2. a second message resumes the same session
  const m2 = await post(owner, chat, "And in one word: which shortcode is it in?");
  r = await waitReplies(chat, 2, waitReply);
  check(r.length === 2 && r[1].id > m2.id, `a reply to the second message (${r[1]?.body?.slice(0, 120) ?? "none"})`);
  check(turns()[1]?.session === s1, `the second turn resumed session ${s1} (${turns()[1]?.session})`);
  const st = JSON.parse(fs.readFileSync(path.join(stateDir, "state.json"), "utf8"));
  check(st.chats[chat]?.session === s1 && st.chats[chat]?.handled >= m2.id, "state.json maps the chat to its session and its handled mark");

  // 3. a principal not on the list gets nothing (a bare key: line does not admit it)
  const m3 = await post(stranger, chat, "Stranger here: please answer me.");
  await sleep(real ? 20_000 : 6_000);
  r = await replies(chat);
  check(r.length === 2, "the stranger's message gets no reply");
  check(bridgeSays(new RegExp(`message ${m3.id} by ${stranger.principal} — not on the allow list`)), "the bridge logs it as not on the allow list");

  // 4. a server restart mid-wait loses nothing and answers nothing twice
  await stopServer();
  check(await waitLog(/watch lost/, 30), "the bridge sees the watch end when the server stops");
  await sleep(8_000);
  await startServer();
  const m4 = await post(owner, chat, "After the restart: reply with the word 'back'.");
  r = await waitReplies(chat, 3, waitReply);
  check(r.length === 3 && r[2].id > m4.id, `a reply to the message posted as the server came back (${r[2]?.body?.slice(0, 120) ?? "none"})`);
  check(await waitLog(/watch held/, 5), "the bridge re-subscribed");
  const how4 = bridgeSays(new RegExp(`message ${m4.id} by .*\\(catch-up`)) ? "catch-up" : "watch";
  log(`  (message ${m4.id} reached the bridge by ${how4})`);

  // 5. the bridge restarted while a message is posted: caught up by messages
  await stopBridge();
  const m5 = await post(owner, chat, "Posted while the bridge was down: reply with 'caught'.");
  startBridge();
  r = await waitReplies(chat, 4, waitReply);
  check(r.length === 4 && r[3].id > m5.id, `a reply to the message posted while the bridge was down (${r[3]?.body?.slice(0, 120) ?? "none"})`);
  check(bridgeSays(new RegExp(`message ${m5.id} by .*\\(catch-up`)), `message ${m5.id} came by catch-up`);
  check(turns()[3]?.session === s1, "and resumed the same session after the bridge's restart");

  // 6. nothing answered twice
  await sleep(real ? 15_000 : 5_000);
  r = await replies(chat);
  const answered = turns().flatMap((t) => t.messages);
  check(r.length === 4, `four replies in all, none twice (${r.length})`);
  check(JSON.stringify(answered) === JSON.stringify([m1.id, m2.id, m4.id, m5.id]), `the turns answered ${[m1.id, m2.id, m4.id, m5.id]} once each (${answered})`);

  // 7. (the fake agent only) a slow turn: "working…" once, and what arrives meanwhile is one next turn
  if (!real) {
    await stopBridge();
    startBridge({ FAKE_CLAUDE_SLEEP: "4", CHAT_BRIDGE_WORKING_AFTER: "2" });
    await waitLog(/caught up \(start\)/, 10);
    const m6 = await post(owner, chat, "a slow one");
    await sleep(1000);
    const m7 = await post(owner, chat, "meanwhile one");
    const m8 = await post(owner, chat, "meanwhile two");
    r = await waitReplies(chat, 6, 30);
    const t = turns().slice(-2).map((x) => x.messages);
    check(JSON.stringify(t) === JSON.stringify([[m6.id], [m7.id, m8.id]]), `the messages posted during a turn are the next turn, together (${JSON.stringify(t)})`);
    const working = (await messages(chat)).filter((m) => m.author === builder.principal && m.body === "_working…_");
    check(working.length === 0, `no "working…" post: the live line says it (${working.length})`);
    const beats = heard.filter((h) => h.type === "activity" && /^working · \d+s$/.test(h.activity.line ?? ""));
    log(`  (heartbeats heard: ${beats.length})`);
  }

  // 8. CHAT-14: a direct message (no ticket) with an image — answered, the image passed as a note; a group too
  {
    const dm = (await rpc(owner, "chats.open", { others: [builder.principal] })).find((i) => i.type === "opened").chat;
    check(dm.kind === "direct" && dm.ticket === undefined, `a direct message with builder, on no ticket (${dm.kind})`);
    const png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
    const hash = (await rpc(owner, "media.put", { chat: dm.id, mime: "image/png", data: png })).find((i) => i.type === "stored").hash;
    await post(owner, dm.id, `what is this? ![kiln shot](media:${hash})`);
    const dr = await waitReplies(dm.id, 1, waitReply);
    check(dr.length === 1, `a reply in the direct message (${dr[0]?.body?.slice(0, 160) ?? "none"})`);
    if (!real) check(/\[image: kiln shot, 1 KB\]/.test(dr[0]?.body ?? ""), "the image reached the agent as a note, [image: name, size]");
    const g = (await rpc(owner, "chats.open", { others: [builder.principal, stranger.principal], title: "the bridge crew" })).find((i) => i.type === "opened").chat;
    await post(owner, g.id, "a group question");
    const gr = await waitReplies(g.id, 1, waitReply);
    check(g.kind === "group" && gr.length === 1, `a reply in the group (${gr[0]?.body?.slice(0, 120) ?? "none"})`);
  }

  // 9. CHAT-40: the commands, answered by the bridge itself (the fake agent only: it holds a long turn)
  if (!real) {
    const spawned = () => (fs.existsSync(markFile) ? fs.readFileSync(markFile, "utf8").trim().split("\n").filter(Boolean).length : 0);
    const c = (await rpc(owner, "chats.open", { others: [builder.principal, stranger.principal], title: "commands" })).find((i) => i.type === "opened").chat.id;
    const replyTo = async (m, secs = 10) => {
      for (let i = 0; i < secs * 4; i++) { const r = (await replies(c)).find((x) => x.id > m.id); if (r) return r; await sleep(250); }
      return null;
    };
    let n0 = spawned();
    const help = await post(owner, c, "  /help ");
    const hr = await replyTo(help);
    check(!!hr && ["/clear", "/stop", "/status", "/help"].every((x) => hr.body.includes(x)), `/help lists the commands (${hr?.body?.slice(0, 80) ?? "none"})`);
    const st0 = await post(owner, c, "/status");
    const sr0 = await replyTo(st0);
    check(!!sr0 && /Idle/.test(sr0.body) && /No session/.test(sr0.body), `/status, idle: no turn, no session (${sr0?.body?.replace(/\n/g, " ") ?? "none"})`);
    await sleep(1000);
    check(spawned() === n0 && !turns().some((x) => x.chat === c && x.messages), `/help and /status spawned no agent (${spawned() - n0})`);

    // a long turn: /status says it runs; /stop ends it within 5 s
    const slow = await post(owner, c, "a long one [slow]");
    for (let i = 0; i < 40 && spawned() === n0; i++) await sleep(250);
    await sleep(1500); // its first steps
    n0 = spawned();
    const st1 = await post(owner, c, "/status");
    const sr1 = await replyTo(st1, 5);
    check(!!sr1 && /Running\*\* since/.test(sr1.body) && /Last step: /.test(sr1.body) && /Session [0-9a-f]{8}…, begun by this turn/.test(sr1.body), `/status during a turn: running, its last step (${sr1?.body?.replace(/\n/g, " ") ?? "none"})`);
    const stop = await post(owner, c, "/stop");
    const t0 = Date.now();
    let stopped = null;
    while (Date.now() - t0 < 5000 && !stopped) { stopped = (await replies(c)).find((x) => x.id > stop.id && /Stopped by the owner/.test(x.body)); if (!stopped) await sleep(200); }
    const took = Date.now() - t0;
    check(!!stopped, `/stop ended the long turn within 5 s, its answer says the owner stopped it (${took} ms)`);
    check((await replies(c)).some((x) => x.id > stop.id && /^Stopping the turn/.test(x.body)), "/stop answered at once");
    await sleep(500);
    const ts = turns().filter((x) => x.chat === c && x.messages);
    check(ts.length === 1 && ts[0].stopped === true && JSON.stringify(ts[0].messages) === JSON.stringify([slow.id]), `the stopped turn is logged stopped (${JSON.stringify(ts.map((x) => ({ m: x.messages, stopped: x.stopped })))})`);
    const kept = ts[0]?.session;
    check(spawned() === n0, `/status and /stop spawned no agent (${spawned() - n0})`);

    // the next message resumes the kept session
    const after = await post(owner, c, "after the stop");
    const ar = await replyTo(after, 15);
    check(!!ar && kept && ar.body.includes(`resumed ${kept}`), `the message after /stop resumed session ${kept} (${ar?.body?.slice(0, 80) ?? "none"})`);

    // /clear, then a message: a new session
    const clr = await post(owner, c, "/clear");
    const cr = await replyTo(clr);
    check(!!cr && /Session forgotten/.test(cr.body), `/clear forgets the session (${cr?.body ?? "none"})`);
    const fresh = await post(owner, c, "fresh after clear");
    const fr = await replyTo(fresh, 15);
    const newSession = /\(new ([0-9a-f-]{36})\)/.exec(fr?.body ?? "")?.[1];
    check(!!newSession && newSession !== kept, `the message after /clear started a new session (${newSession} ≠ ${kept})`);
    const stc = JSON.parse(fs.readFileSync(path.join(stateDir, "state.json"), "utf8")).chats[c];
    check(stc?.session === newSession && stc?.handled >= fresh.id, "state.json holds the new session and the handled mark");
    const st2 = await post(owner, c, "/status");
    const sr2 = await replyTo(st2);
    check(!!sr2 && /Idle/.test(sr2.body) && new RegExp(`Session ${newSession.slice(0, 8)}…, begun \\d+s ago`).test(sr2.body), `/status: idle, the session's age (${sr2?.body?.replace(/\n/g, " ") ?? "none"})`);
    // a command is answered once: a bridge restart's catch-up answers none again
    const before = (await replies(c)).length;
    await stopBridge(); startBridge();
    await waitLog(/caught up \(start\)/, 15);
    await sleep(2000);
    check((await replies(c)).length === before, `after a restart no command is answered twice (${(await replies(c)).length - before} more)`);
    // an unknown /x is the agent's
    const unk = await post(owner, c, "/frobnicate");
    const ur = await replyTo(unk, 15);
    check(!!ur && /fake reply/.test(ur.body), `an unknown /x goes to the agent (${ur?.body?.slice(0, 60) ?? "none"})`);
  }
} catch (e) {
  log(`FAIL ${e.stack || e.message}`); failures++;
} finally {
  try { watchSock?.close(); } catch {}
  if (bridge) await stopBridge();
  if (server) await stopServer();
}
log(failures ? `${failures} failed` : "all passed");
process.exit(failures ? 1 : 0);
