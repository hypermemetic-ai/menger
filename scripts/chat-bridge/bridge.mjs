#!/usr/bin/env node
// CHAT-11: the chat bridge — a perpetual listener on menger's chats that
// spawns a Claude Code agent on this machine to answer the owner there.
//
// It holds `chats.watch` open on MENGER_URL, signed in with the builder's
// key (a key proof at the upgrade, deploy/menger-call.mjs's wire), and for
// each message by a principal on the allow list it
//
//   1. says `chats.activity started` at once (CHAT-18: the app shows "on
//      it"), and marks the chat read up to it;
//   2. runs `claude -p --output-format stream-json --verbose` in
//      CHAT_BRIDGE_CWD, resuming the chat's own session (one per chat, kept
//      in $STATE/state.json), with the chat's recent turns and its ticket on
//      the first turn and the new messages after — and, as it streams, says
//      each tool call and its interim text as `chats.activity` lines
//      (live.mjs: throttled ≥ 300 ms, batched, secrets masked, a heartbeat
//      after 15 s of quiet);
//   3. posts the agent's final message (the stream's `result`) with
//      `chats.post` — or, on a failed turn, "couldn't answer: <why>", never
//      silence — then says `activity ended`. Only where the server takes no
//      activity (an older menger) does a turn still running after
//      CHAT_BRIDGE_WORKING_AFTER seconds post "working…" once.
//
// One agent per chat at a time — messages that arrive while it works are
// the next turn's, together; chats run in parallel up to CHAT_BRIDGE_MAX.
//
// CHAT-40: a message that is exactly `/clear`, `/stop`, `/status` or `/help`
// is answered by the bridge itself, at once — no agent turn, and around the
// per-chat queue, so a `/stop` reaches the turn it stops: `/clear` forgets
// the chat's session (the next message starts a new one, with the chat
// after the `/clear` as its context), `/stop` ends the running turn
// (SIGTERM; the session kept; the turn's answer says the owner stopped it),
// `/status` says what runs and the session's age, `/help` lists them. Any
// other `/x` goes to the agent as before.
//
// CHAT-45: which model a turn runs on. Each archetype has a default model,
// kept in one file the bridge and every launcher read (models.mjs:
// CHAT_BRIDGE_ARCHETYPES, else ~/.config/menger/archetypes.env, read on
// every turn and command — an edit needs no restart); the bridge is the
// architect. A chat may change it for itself: `/model` lists the models,
// this chat's marked, `/model sonnet` switches the chat's next turns (kept
// in state.json; the session is resumed on the new model), `/model default`
// goes back to the architect's. `/status` says which model the chat is on.
//
// Nothing is missed or answered twice: the bridge keeps, per chat, the last
// message it has handled, and every time the watch is (re)subscribed — the
// socket dropped, the server restarted, a stream ended silently
// (SYNAPSE-21), the watch lagged — it pages `chats.messages --after` that
// mark in every open chat. An answer is written to the outbox before it is
// posted and the mark moves only after the post, so a restart between the
// two posts it once (a post whose fate is unknown is looked for first).
//
// Who it answers: the principals in CHAT_BRIDGE_ALLOW (one a line, `#`
// comments). Never itself, never an `agent:` principal, and never a `key:`
// principal — unless its line is written `+key:…`, which admits that key
// explicitly (for a test, removed after). The list is read on every
// message: an edit needs no restart.
//
// The agent's permission mode is this process's argument, written in the
// unit (scripts/chat-bridge/menger-chat-bridge.service), never chosen at
// runtime. Its model is the chat's (CHAT-45, above); `--model` is only the
// architect's default where the archetypes file names none:
//
//   node bridge.mjs --permission-mode bypassPermissions [--model claude-opus-5-5]
//
// Environment (the unit reads ~/.config/menger/chat-bridge.env):
//   MENGER_URL                 the hub's socket (ws://127.0.0.1:4455, wss://app.menger.sh/ws)
//   MENGER_KEY_FILE            the key it signs in with (~/.plexus/keys/builder.key; 0600)
//   MENGER_AGENT               the key proof's agent name (builder)
//   CHAT_BRIDGE_ALLOW          the allow list (~/.config/menger/chat-bridge.allow)
//   CHAT_BRIDGE_ARCHETYPES     the archetypes' default models (~/.config/menger/archetypes.env; CHAT-45)
//   CHAT_BRIDGE_STATE          its state dir (~/.local/state/menger/chat-bridge)
//   CHAT_BRIDGE_CWD            where the agent runs (~/dev/controlflow/hypermemetic/menger)
//   CHAT_BRIDGE_MAX            chats answered at once (3)
//   CHAT_BRIDGE_TURN_MINUTES   a turn's limit before it is stopped (0: none, the default; CHAT-36)
//   CHAT_BRIDGE_WORKING_AFTER  seconds before "working…" is posted, only where the
//                              server takes no chats.activity (20; 0 = never)
//   CLAUDE_BIN                 the claude binary (~/.local/bin/claude)
//
// Logs: one line per event to stdout (journald), and one JSON line per turn
// in $STATE/turns.log: the chat, the messages, the session, the duration,
// the exit. Node ≥ 22, its standard library only.
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { connect, keyProof, publicHex, seedOf } from "../../deploy/menger-call.mjs";
import { clip, jsonLines, linesOf, liveLine } from "./live.mjs";
import { MODELS, defaultFile, modelName, readDefaults, resolveModel } from "./models.mjs";

const HOME = os.homedir();
const env = process.env;
const log = (...a) => console.log(new Date().toISOString(), ...a);

// ── configuration ──────────────────────────────────────────────────────────
function args(argv) {
  const o = { model: null, permissionMode: null, once: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i], next = () => argv[++i] ?? fail(`${a} needs a value`);
    if (a === "--model") o.model = next();
    else if (a === "--permission-mode") o.permissionMode = next();
    else fail(`unknown argument '${a}'`);
  }
  if (!o.permissionMode) fail("usage: bridge.mjs --permission-mode MODE [--model MODEL] (written in the unit)");
  if (o.model && !resolveModel(o.model)) fail(`--model ${o.model}: not a model`);
  return o;
}
function fail(msg) { console.error(`chat-bridge: ${msg}`); process.exit(1); }

const opt = args(process.argv.slice(2));
const cfg = {
  url: env.MENGER_URL || fail("MENGER_URL is not set (~/.config/menger/chat-bridge.env)"),
  keyFile: env.MENGER_KEY_FILE || path.join(HOME, ".plexus/keys/builder.key"),
  agent: env.MENGER_AGENT || "builder",
  allow: env.CHAT_BRIDGE_ALLOW || path.join(HOME, ".config/menger/chat-bridge.allow"),
  archetypes: defaultFile(env),
  state: env.CHAT_BRIDGE_STATE || path.join(HOME, ".local/state/menger/chat-bridge"),
  cwd: env.CHAT_BRIDGE_CWD || path.join(HOME, "dev/controlflow/hypermemetic/menger"),
  max: Math.max(1, Number(env.CHAT_BRIDGE_MAX || 3)),
  turnMinutes: Math.max(0, Number(env.CHAT_BRIDGE_TURN_MINUTES || 0)),
  workingAfter: Number(env.CHAT_BRIDGE_WORKING_AFTER ?? 20),
  claude: env.CLAUDE_BIN || path.join(HOME, ".local/bin/claude"),
};
if (!/^wss?:\/\//.test(cfg.url)) fail(`MENGER_URL ${cfg.url}: the socket is ws:// or wss://`);

function readSeed(file) {
  let st; try { st = fs.statSync(file); } catch { fail(`MENGER_KEY_FILE: no ${file}`); }
  if ((st.mode & 0o077) !== 0) fail(`MENGER_KEY_FILE: ${file} is readable by others; 0600`);
  return seedOf(fs.readFileSync(file, "utf8"));
}
const seed = readSeed(cfg.keyFile);
const SELF = `key:${publicHex(seed)}`;

// ── who it answers ─────────────────────────────────────────────────────────
function allowed(principal) {
  if (!principal || principal === SELF) return false;
  let lines = [];
  try { lines = fs.readFileSync(cfg.allow, "utf8").split("\n"); } catch { return false; }
  for (const raw of lines) {
    const line = raw.replace(/#.*/, "").trim();
    if (!line) continue;
    if (line.startsWith("+")) { if (line.slice(1) === principal) return true; continue; }
    if (line === principal) return !/^(agent|key):/.test(principal);
  }
  return false;
}

// ── state: per chat the last handled message and its session; the outbox ──
fs.mkdirSync(cfg.state, { recursive: true, mode: 0o700 });
const STATE_FILE = path.join(cfg.state, "state.json");
const TURNS_LOG = path.join(cfg.state, "turns.log");
let state = { born: Math.floor(Date.now() / 1000), chats: {}, outbox: [] };
try { state = { ...state, ...JSON.parse(fs.readFileSync(STATE_FILE, "utf8")) }; } catch (e) { if (e.code !== "ENOENT") fail(`${STATE_FILE}: ${e.message}`); }
function save() {
  const tmp = `${STATE_FILE}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(state, null, 1) + "\n", { mode: 0o600 });
  fs.renameSync(tmp, STATE_FILE);
}
for (const c of Object.values(state.chats)) delete c.inflight;
save();
const chatState = (id) => (state.chats[id] ??= { handled: null, session: null, ticket: null });

// ── models (CHAT-45): the architect's default, and each chat's own ─────────
let saidProblems = "";
/** The architect's default: the archetypes file's line, else `--model`, else the built-in. */
function architectDefault() {
  const d = readDefaults(cfg.archetypes);
  const p = d.problems.join("; ");
  if (p !== saidProblems) { saidProblems = p; if (p) log(`archetypes: ${p}`); }
  return d.from.architect === "built-in" && opt.model ? resolveModel(opt.model) : d.models.architect;
}
/** The model a chat's next turn runs on: its own, else the default. */
function chatModel(s) {
  const def = architectDefault();
  return { id: s.model ?? def, set: !!s.model, def };
}

// ── the hub: one socket, many calls on it ──────────────────────────────────
class Hub {
  constructor() { this.next = 1; this.calls = new Map(); this.subs = new Map(); this.early = new Map(); this.dead = null; }
  async open() {
    const aud = cfg.url.replace(/\/+$/, "");
    this.ws = await connect(cfg.url, { Authorization: `Bearer ${keyProof(seed, aud, cfg.agent)}` });
    this.closed = new Promise((resolve) => { this.ws.onclose = (why) => { this.kill(why); resolve(why); }; });
    this.ws.onmessage = (text) => this.onmessage(text);
  }
  kill(why) {
    if (this.dead) return;
    this.dead = why || "closed";
    for (const c of this.calls.values()) c.reject(new Error(`${c.method}: ${this.dead}`));
    this.calls.clear(); this.subs.clear();
    try { this.ws?.close(); } catch {}
  }
  onmessage(text) {
    let msg; try { msg = JSON.parse(text); } catch { return; }
    if (msg.id !== undefined && this.calls.has(msg.id) && !msg.params) {
      const c = this.calls.get(msg.id);
      if (msg.error) { this.calls.delete(msg.id); return c.reject(new Error(`${c.method}: ${msg.error.message ?? JSON.stringify(msg.error)}`)); }
      c.sub = msg.result; this.subs.set(c.sub, msg.id); c.subscribed();
      for (const item of this.early.get(c.sub) ?? []) this.item(msg.id, item);
      this.early.delete(c.sub);
      return;
    }
    const sub = msg.params?.subscription;
    if (sub === undefined) return;
    const id = this.subs.get(sub);
    if (id === undefined) { const q = this.early.get(sub) ?? []; q.push(msg.params.result); this.early.set(sub, q); return; }
    this.item(id, msg.params.result);
  }
  item(id, item) {
    const c = this.calls.get(id);
    if (!c) return;
    if (item?.type === "done") { this.calls.delete(id); this.subs.delete(c.sub); return c.resolve(); }
    if (item?.type === "error") { this.calls.delete(id); this.subs.delete(c.sub); return c.reject(Object.assign(new Error(`${c.method}: ${item.message ?? JSON.stringify(item)}`), { refusal: item })); }
    if (item?.type === "data") c.onItem(item.content);
  }
  /** A call: `subscribed` once the server took it, `done` when it ends. */
  stream(method, params, onItem) {
    if (this.dead) { const e = Promise.reject(new Error(`${method}: ${this.dead}`)); e.catch(() => {}); return { subscribed: e, done: e }; }
    const id = this.next++;
    let subscribed;
    const sp = new Promise((r) => { subscribed = r; });
    const done = new Promise((resolve, reject) => { this.calls.set(id, { method, onItem, resolve, reject, subscribed }); });
    done.catch(() => {});
    this.ws.send(JSON.stringify({ jsonrpc: "2.0", id, method: "menger.call", params: { method, params } }));
    const subscribedOrFailed = Promise.race([sp, done]);
    subscribedOrFailed.catch(() => {});
    return { subscribed: subscribedOrFailed, done };
  }
  /** A unary call's data items; an in-band `error` event is thrown. */
  async call(method, params, timeoutMs = 60_000) {
    const out = [];
    const { done } = this.stream(method, params, (c) => out.push(c));
    let t;
    const timer = new Promise((_, rej) => { t = setTimeout(() => rej(new Error(`${method}: no answer within ${timeoutMs / 1000}s`)), timeoutMs); });
    try { await Promise.race([done, timer]); } finally { clearTimeout(t); }
    const err = out.find((c) => c?.type === "error" || c?.type === "refused" || c?.type === "not_found");
    if (err) throw new Error(`${method}: ${err.message ?? JSON.stringify(err)}`);
    return out;
  }
}

let hub = null;           // the live connection, or null between connections
let live = null;          // resolves when a connection is up
let setLive = null;
const resetLive = () => { live = new Promise((r) => { setLive = r; }); };
resetLive();

/** A call on whichever connection is live, waiting for one; a read retried. */
async function rpc(method, params, { retry = true } = {}) {
  for (let attempt = 0; ; attempt++) {
    const h = hub ?? await live;
    try { return await h.call(method, params); }
    catch (e) {
      if (!retry || !h.dead || attempt > 50) throw e;
      log(`${method}: ${e.message}; again on the next connection`);
      if (hub === h) await sleep(1000);
    }
  }
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── chats: what is known of each, and the messages to answer ───────────────
const chats = new Map();      // id → Chat (from list / watch)
const pending = new Map();    // id → Message[] waiting for the next turn
const busy = new Set();       // chats with a turn running
const turning = new Map();    // id → the running turn: { started, lastLine, child, stopped } (CHAT-40)
const answering = new Set();  // "chat:message" commands being answered now
let running = 0;
const queue = [];             // chats waiting for a slot

function remember(chat) { if (chat?.id !== undefined) { chats.set(chat.id, chat); const s = chatState(chat.id); const t = chat.ticket ?? null; if (s.ticket !== t) { s.ticket = t; save(); } } }

/** The mark a chat is caught up from: the last handled, else (a chat the
 *  bridge has never handled) the builder's read mark, else nothing. */
function markOf(chat) {
  const s = chatState(chat.id);
  if (s.handled !== null) return s.handled;
  return chat.mark ?? 0;
}

function onMessage(m, source) {
  const s = chatState(m.chat);
  const pend = pending.get(m.chat) ?? [];
  if (s.handled !== null && m.id <= s.handled) return;
  if (pend.some((p) => p.id === m.id) || s.inflight?.includes(m.id)) return;
  if (s.commands?.includes(m.id) || answering.has(`${m.chat}:${m.id}`)) return;
  if (state.outbox.some((o) => o.chat === m.chat && o.upto >= m.id)) return;
  if (s.handled === null && m.at < state.born) return; // older than the bridge: not its to answer
  if (!allowed(m.author)) {
    if (m.author !== SELF) log(`chat ${m.chat}: message ${m.id} by ${m.author} — not on the allow list, ignored`);
    // nothing waiting before it: the mark moves past it
    if (!pend.length && !busy.has(m.chat)) { s.handled = Math.max(s.handled ?? 0, m.id); save(); }
    return;
  }
  const cmd = commandOf(m.body);
  if (cmd) {
    log(`chat ${m.chat}: message ${m.id} by ${m.author} (${source}) — /${cmd}, answered by the bridge`);
    answering.add(`${m.chat}:${m.id}`);
    command(cmd, m).catch((e) => log(`chat ${m.chat}: /${cmd}: ${e.stack || e.message}`)).finally(() => answering.delete(`${m.chat}:${m.id}`));
    return;
  }
  log(`chat ${m.chat}: message ${m.id} by ${m.author} (${source})`);
  pend.push(m); pend.sort((a, b) => a.id - b.id); pending.set(m.chat, pend);
  schedule(m.chat);
}

function schedule(chat) {
  if (busy.has(chat) || queue.includes(chat) || !(pending.get(chat)?.length)) return;
  queue.push(chat); pump();
}
function pump() {
  while (running < cfg.max && queue.length) {
    const chat = queue.shift();
    if (busy.has(chat)) continue;
    busy.add(chat); running++;
    turn(chat).catch((e) => log(`chat ${chat}: turn failed: ${e.stack || e.message}`)).finally(() => {
      busy.delete(chat); running--;
      schedule(chat); pump();
    });
  }
}

/** Every open chat, paged; each remembered. */
async function listChats() {
  const out = [];
  let after;
  for (;;) {
    const items = await rpc("chats.list", { state: "open", limit: 500, ...(after ? { after } : {}) });
    for (const i of items) if (i.type === "chat") { out.push(i.chat); remember(i.chat); }
    const page = items.find((i) => i.type === "page");
    if (!page?.next) return out;
    after = page.next;
  }
}

/** The messages of `chat` after `after`, oldest first, paged. */
async function messagesAfter(chat, after) {
  const out = [];
  for (;;) {
    const items = await rpc("chats.messages", { chat, limit: 1000, ...(after ? { after } : {}) });
    const ms = items.filter((i) => i.type === "message").map((i) => i.message);
    out.push(...ms);
    const page = items.find((i) => i.type === "page");
    if (!page?.next || !ms.length) return out;
    after = page.next;
  }
}

/** Catch up: every open chat's messages after its mark. */
async function catchUp(why) {
  const list = await listChats();
  let n = 0;
  for (const chat of list) {
    const mark = markOf(chat);
    if ((chat.last_message ?? 0) <= mark) continue;
    for (const m of await messagesAfter(chat.id, mark)) { onMessage(m, `catch-up, ${why}`); n++; }
  }
  log(`caught up (${why}): ${list.length} open chats, ${n} messages after their marks`);
}

// ── commands: answered by the bridge, no agent turn (CHAT-40) ─────────────
const COMMANDS = {
  clear: "forget this chat's agent session: the next message starts a new one (the chat itself stays)",
  stop: "end this chat's running turn; its session is kept",
  status: "what is running for this chat: the turn, since when, its last step; the session's age",
  model: "this chat's model: `/model` lists them, `/model sonnet` switches the next turns, `/model default` goes back",
  help: "this list",
};
const TAKES_ARG = new Set(["model"]);
/** The command a body is, when the whole of it (trimmed) is one; else null.
 *  Only `/model` takes a word after it (CHAT-45); `/clear now` is the agent's. */
function commandOf(body) {
  const m = /^\/([a-z]+)(?:[ \t]+(\S.*))?$/i.exec(String(body ?? "").trim());
  const c = m?.[1].toLowerCase();
  if (!c || !Object.hasOwn(COMMANDS, c)) return null;
  return m[2] === undefined || TAKES_ARG.has(c) ? c : null;
}
/** The word after a command, trimmed; "" when none. */
const argOf = (body) => (/^\/[a-z]+[ \t]+(\S.*)$/i.exec(String(body ?? "").trim())?.[1] ?? "").trim();
const ago = (ms) => {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 90) return `${s}s`;
  if (s < 5400) return `${Math.round(s / 60)} min`;
  if (s < 172800) return `${(s / 3600).toFixed(1)} h`;
  return `${Math.round(s / 86400)} days`;
};
const hhmm = (ms) => `${new Date(ms).toISOString().slice(11, 16)}Z`;

/** What a command says (and does) in a chat. */
function commandAnswer(cmd, chatId, arg = "") {
  const s = chatState(chatId);
  const t = turning.get(chatId);
  const waiting = pending.get(chatId)?.length ?? 0;
  switch (cmd) {
    case "help":
      return ["**Commands** — answered by the bridge at once, no agent turn:", "",
        ...Object.entries(COMMANDS).map(([c, d]) => `- \`/${c}\` — ${d}`), "",
        "Anything else, `/x` included, goes to the agent."].join("\n");
    case "clear": {
      const had = s.session;
      s.session = null; s.sessionAt = null; s.cleared = (s.cleared ?? 0) + 1;
      return had
        ? `Session forgotten (${had.slice(0, 8)}…). The next message starts a new one${t ? "; the turn running now finishes in the old one" : ""}.`
        : `No session to forget — the next message starts a new one${t ? "; the turn running now finishes as it began" : ""}.`;
    }
    case "stop": {
      if (!t) return `Nothing is running in this chat${waiting ? ` (${waiting} message${waiting > 1 ? "s" : ""} waiting for a turn)` : ""}.`;
      if (t.stopped) return "Already stopping the running turn.";
      t.stopped = true;
      if (t.child) {
        try { t.child.kill("SIGTERM"); } catch {}
        const child = t.child;
        setTimeout(() => { if (child.exitCode === null && child.signalCode === null) try { child.kill("SIGKILL"); } catch {} }, 5000).unref();
      }
      return `Stopping the turn that began ${hhmm(t.started)} (${ago(Date.now() - t.started)} ago). The session is kept: the next message resumes it.`;
    }
    case "model": return modelAnswer(s, t, arg);
    case "status": {
      const lines = [];
      if (t) {
        lines.push(`**Running** since ${hhmm(t.started)} (${ago(Date.now() - t.started)})${t.stopped ? ", stopping" : ""}.`);
        lines.push(t.lastLine ? `Last step: ${clip(t.lastLine, 200)}` : "No step yet.");
      } else lines.push("**Idle** — no turn running.");
      if (waiting) lines.push(`${waiting} message${waiting > 1 ? "s" : ""} waiting for the next turn.`);
      lines.push(s.session
        ? `Session ${s.session.slice(0, 8)}…, ${s.sessionAt ? `begun ${ago(Date.now() - s.sessionAt)} ago` : "its age unknown (begun before CHAT-40)"}.`
        : t?.session && (s.cleared ?? 0) === t.cleared ? `Session ${t.session.slice(0, 8)}…, begun by this turn.`
        : "No session: the next message starts a new one.");
      const md = chatModel(s);
      lines.push(`Model: ${modelName(md.id)} (\`${md.id}\`), ${md.set ? `set in this chat — the architect's default is ${modelName(md.def)}` : "the architect's default"}${t?.model && t.model !== md.id ? `; the running turn is on ${modelName(t.model)}` : ""}.`);
      return lines.join("\n");
    }
  }
}

/** CHAT-45: `/model` — list; `/model NAME` — switch this chat; `/model default` — back. */
function modelAnswer(s, t, arg) {
  const word = arg.trim().toLowerCase();
  const running = t ? ` The turn running now finishes on ${modelName(t.model ?? chatModel(s).id)}.` : "";
  if (word === "default") {
    const was = s.model;
    delete s.model;
    const def = architectDefault();
    return was
      ? `Back on the architect's default, ${modelName(def)} (\`${def}\`), from the next turn; the session carries over.${running}`
      : `This chat is already on the architect's default, ${modelName(def)}.`;
  }
  if (word) {
    const id = resolveModel(word);
    if (!id) return `No model “${clip(arg, 60)}”. ${choices(s)}`;
    const def = architectDefault();
    if (id === def) { delete s.model; return `This chat runs on ${modelName(id)} from the next turn — the architect's default; the session carries over.${running}`; }
    if (s.model === id) return `This chat is already on ${modelName(id)}.`;
    s.model = id;
    return `This chat runs on ${modelName(id)} (\`${id}\`) from the next turn; the session carries over. The architect's default is ${modelName(def)} — \`/model default\` goes back.${running}`;
  }
  const md = chatModel(s);
  return [`**Model** for this chat: ${modelName(md.id)} (\`${md.id}\`), ${md.set ? "set here" : "the architect's default"}.`, "",
    ...MODELS.map((m) => `- \`${m.alias}\` — ${m.name}${m.id === md.def ? " · the architect's default" : ""}${m.id === md.id ? " · **this chat**" : ""}`),
    ...(MODELS.some((m) => m.id === md.id) ? [] : [`- \`${md.id}\` · **this chat**`]), "",
    "`/model <name>` switches this chat's next turns (the session carries over); `/model default` goes back."].join("\n");
}
const choices = (s) => `Choices: ${MODELS.map((m) => `\`${m.alias}\``).join(", ")}, or \`default\` — \`/model\` lists them.`;

/** A command: done, answered as the builder, its message marked handled. */
async function command(cmd, m) {
  const s = chatState(m.chat);
  const body = commandAnswer(cmd, m.chat, argOf(m.body));
  if (cmd === "clear") s.clearedAt = m.id;
  save();
  await rpc("chats.read", { chat: m.chat, upto: m.id }).catch((e) => log(`chat ${m.chat}: read: ${e.message}`));
  await rpc("chats.post", { chat: m.chat, body }, { retry: false });
  // nothing waiting before it: the mark moves past it; else it is kept apart
  if (!(pending.get(m.chat)?.length) && !busy.has(m.chat)) s.handled = Math.max(s.handled ?? 0, m.id);
  else (s.commands ??= []).push(m.id);
  if (s.commands) { s.commands = s.commands.filter((id) => id > (s.handled ?? 0)); if (!s.commands.length) delete s.commands; }
  save();
  fs.appendFileSync(TURNS_LOG, JSON.stringify({ at: new Date().toISOString(), chat: m.chat, command: cmd, message: m.id, ...(cmd === "model" ? { model: chatModel(s).id } : {}) }) + "\n");
  log(`chat ${m.chat}: /${cmd} answered`);
}

// ── a turn ─────────────────────────────────────────────────────────────────
async function turn(chatId) {
  const batch = pending.get(chatId) ?? [];
  pending.delete(chatId);
  if (!batch.length) return;
  const s = chatState(chatId);
  s.inflight = batch.map((m) => m.id);
  const upto = batch[batch.length - 1].id;
  const started = Date.now();
  const cleared = s.cleared ?? 0;
  const model = chatModel(s).id;
  const t = { started, lastLine: null, child: null, stopped: false, session: null, cleared, model };
  turning.set(chatId, t);
  try { await turnBody(chatId, batch, s, t, cleared, upto, started); } finally { turning.delete(chatId); }
}

async function turnBody(chatId, batch, s, t, cleared, upto, started) {
  let chat = chats.get(chatId);
  if (!chat) { await listChats().catch(() => {}); chat = chats.get(chatId); }
  // CHAT-14: a direct message or a group has no ticket.
  const ticket = chat ? (chat.ticket ?? null) : (s.ticket ?? null);

  // CHAT-18: "on it", before anything else.
  const activityOk = await activity(chatId, "started");
  await rpc("chats.read", { chat: chatId, upto }).catch((e) => log(`chat ${chatId}: read: ${e.message}`));

  let working = null;
  if (!activityOk && cfg.workingAfter > 0) working = setTimeout(() => {
    rpc("chats.post", { chat: chatId, body: "_working…_" }, { retry: false }).catch(() => {});
  }, cfg.workingAfter * 1000);
  const live = activityOk ? liveLine((st, line) => activity(chatId, st, line)) : null;

  let answer, exit = 0, session = s.session, why = null;
  try {
    const r = await runAgent(chatId, chat, ticket, batch, s.session, (line) => { t.lastLine = line; live?.line(line); }, t, t.model);
    session = r.session ?? session;
    exit = r.exit;
    if (r.ok) answer = r.text;
    else why = r.why;
  } catch (e) { exit = -1; why = e.message; }
  clearTimeout(working);
  const steps = live?.stop() ?? 0;
  if (!answer && t.stopped) answer = "_Stopped by the owner (`/stop`)._ The session is kept: the next message resumes it.";
  if (!answer) answer = `couldn't answer: ${String(why || "the agent gave no final message").slice(0, 500)}`;
  // a /clear during the turn wins: the next message starts fresh
  if (session && session !== s.session && (s.cleared ?? 0) === cleared) { s.session = session; s.sessionAt = Date.now(); }
  state.outbox.push({ chat: chatId, upto, body: answer.slice(0, 60_000), at: Math.floor(Date.now() / 1000) });
  delete s.inflight;
  save();
  await flushOutbox().finally(() => (activityOk ? activity(chatId, "ended") : null));
  const line = { at: new Date().toISOString(), chat: chatId, ticket, messages: batch.map((m) => m.id), session, model: t.model, duration_ms: Date.now() - started, exit, ok: !why, steps, ...(t.stopped ? { stopped: true } : {}) };
  fs.appendFileSync(TURNS_LOG, JSON.stringify(line) + "\n");
  log(`chat ${chatId}: answered ${batch.map((m) => m.id).join(",")} in ${Math.round(line.duration_ms / 1000)}s (session ${session ?? "-"}, ${t.model}, exit ${exit}${why ? `, ${why}` : ""})`);
}

/** CHAT-18: say `state` (and `line`) in `chat`'s live line — never retried
 *  (it is the present), never failing a turn; whether the server took it. */
async function activity(chat, state, line) {
  try {
    await rpc("chats.activity", { chat, state, ...(line ? { line } : {}) }, { retry: false });
    return true;
  } catch (e) {
    log(`chat ${chat}: activity ${state}: ${e.message}`);
    return false;
  }
}

/** Post what the outbox holds, oldest first; each answer once. */
let flushing = null;
async function flushOutbox() {
  if (flushing) return flushing;
  flushing = (async () => {
    while (state.outbox.length) {
      const o = state.outbox[0];
      try {
        if (o.unsure) {
          // a post whose answer was lost: is it there already?
          const mine = (await messagesAfter(o.chat, o.upto)).find((m) => m.author === SELF && m.body === o.body);
          if (!mine) await rpc("chats.post", { chat: o.chat, body: o.body }, { retry: false });
        } else {
          o.unsure = true; save();
          await rpc("chats.post", { chat: o.chat, body: o.body }, { retry: false });
        }
      } catch (e) {
        if (/not_found|refused/i.test(e.message) && hub && !hub.dead) {
          log(`chat ${o.chat}: the answer cannot be posted (${e.message}); dropped`);
        } else { log(`chat ${o.chat}: post: ${e.message}; again when connected`); await sleep(2000); await (hub && !hub.dead ? null : live); continue; }
      }
      const s = chatState(o.chat);
      s.handled = Math.max(s.handled ?? 0, o.upto);
      state.outbox.shift(); save();
    }
  })().finally(() => { flushing = null; });
  return flushing;
}

/** CHAT-14: a chat's images, for the agent, as notes — `[image: name, size]` (the bytes are not passed yet). */
const sizes = new Map();
async function imageNotes(body) {
  const found = [...body.matchAll(/!\[([^\]\n]*)\]\(media:([0-9a-f]{64})\)/g)];
  for (const [, , hash] of found) {
    if (sizes.has(hash)) continue;
    const got = (await rpc("media.get", { hash }).catch(() => [])).find((i) => i.type === "media");
    sizes.set(hash, got ? got.bytes : null);
  }
  return body.replace(/!\[([^\]\n]*)\]\(media:([0-9a-f]{64})\)/g, (_, alt, hash) => {
    const n = sizes.get(hash);
    const size = n == null ? "not readable" : n >= 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`;
    return `[image: ${alt || "image"}, ${size}]`;
  });
}
async function withNotes(ms) {
  return Promise.all(ms.map(async (m) => (/\(media:[0-9a-f]{64}\)/.test(m.body) ? { ...m, body: await imageNotes(m.body) } : m)));
}

/** Where a chat is, in words: on a ticket, or (CHAT-14) a direct message or a group. */
const where = (chat, ticket) => (ticket ? `on ticket ${ticket}` : chat?.kind === "group" ? "a group chat outside tickets" : "a direct message outside tickets");

const fmt = (m) => `[message ${m.id} · ${m.name ? `${m.name} (${m.author})` : m.author} · ${new Date(m.at * 1000).toISOString().slice(0, 16).replace("T", " ")}Z]\n${m.body}`;

function systemPrompt(chatId, ticket, title, chat) {
  return [
    `You are the owner's builder on this machine, answering the owner in menger chat ${chatId}, ${where(chat, ticket)}${title ? ` (${title})` : ""}.`,
    `The chat bridge (menger's scripts/chat-bridge, CHAT-11) runs you. Your final message is posted to that chat as the builder, verbatim, so do not post it yourself with chats.post.`,
    ``,
    `Keep chat answers short: a few sentences of markdown, the answer first. The owner is reading on a phone.`,
    `Name tickets in the notation (CHAT-57; menger-skills chatting): bold, the id alone — **KILN-7**, a run **KILN-2..5**, a set **KILN-11/12**, an ask **KILN-7** · inputid. The chat draws each as a live chip and a card under your message; bold only the tickets the message is about, and leave a passing mention plain.`,
    `A message may carry runtime components (CHAT-21, CHAT-59; menger-skills chatting): m-stat, m-dial, m-ring, m-cases, m-decision, a project's own (\`tickets components list\`) — markup outside code draws in the message's own frame, and its components read live as each reader (in a ticket's chat, that ticket's shortcode; in a direct message or a group, the reader's own reach). Put it in your answer as markup, not in a code fence; a separate message goes as \`chats post --chat ${chatId} --body @file\`, never "$(cat …)".`,
    ``,
    `House rules — follow them as any builder here does:`,
    `- ~/AGENTS.md and ~/dev/controlflow/hypermemetic/menger/AGENTS.md are the rules (read them before work that touches the tracker or a repository); ~/dev/controlflow/hypermemetic/menger-skills is the method (ticketing, building, asking, reaching).`,
    `- Never hand-write \`rm -r\` / \`rm -rf\`: scripts/cleanup-gate.sh removes what a gate left.`,
    `- Production changes only through menger's deploy/pipeline; never run a server on prod's ports or database.`,
    `- A question for the owner is filed as an input on its ticket (or the needs-you label with the reason), then mentioned here with the ticket id — never only in chat.`,
    `- Never print a secret.`,
    `- For real work — code, a build, a change — file or claim a ticket and work in its own worktree as the building skill says; say in your answer what you started and where to follow it. A turn ends when you answer, so finish or summarise where it stands.`,
    `- A ticket you file for this chat is filed with \`--chat ${chatId}\` (\`tickets create … --chat ${chatId}\`), and one you start from it is linked (\`chats link --chat ${chatId} --tickets ID\`), so it shows on the chat's ticket strip (CHAT-55).`,
    ``,
    `The tracker is ${cfg.url}. synapse reaches it as ${/^ws:\/\/127\.0\.0\.1:(\d+)/.test(cfg.url) ? `\`synapse -P ${cfg.url.match(/:(\d+)/)[1]} menger …\`` : `\`synapse --url ${cfg.url} menger …\``} (the builder's key is signed in). The chat's messages: \`chats messages --chat ${chatId} --newest true\`.`,
  ].join("\n");
}

async function runAgent(chatId, chat, ticket, batch, session, onLine, t = {}, model = architectDefault()) {
  let title = null;
  let prompt;
  const fresh = await withNotes(batch);
  if (session) {
    prompt = `New in chat ${chatId}:\n\n${fresh.map(fmt).join("\n\n")}`;
  } else {
    if (ticket) {
      try {
        const t = (await rpc("tickets.get", { id: ticket })).find((i) => i.type === "ticket")?.ticket;
        title = t?.title ?? null;
      } catch {}
    }
    const recent = await withNotes((await rpc("chats.messages", { chat: chatId, newest: true, limit: 30 }).catch(() => []))
      .filter((i) => i.type === "message").map((i) => i.message)
      // CHAT-40: after a /clear, the chat before it is not the new session's; nor is any command
      .filter((m) => m.id < batch[0].id && m.id > (chatState(chatId).clearedAt ?? 0) && !(m.author !== SELF && commandOf(m.body))));
    prompt = [
      `Chat ${chatId}, ${where(chat, ticket)}${title ? ` — ${title}` : ""}${chat?.title ? `, titled "${chat.title}"` : ""}. Participants: ${(chat?.participants ?? []).join(", ") || "?"}; you are ${SELF}.`,
      recent.length ? `Earlier in the chat:\n\n${recent.map(fmt).join("\n\n")}` : `Nothing earlier in the chat.`,
      `New — answer this:\n\n${fresh.map(fmt).join("\n\n")}`,
    ].join("\n\n---\n\n");
  }
  let r = await claude(prompt, session, systemPrompt(chatId, ticket, title, chat), onLine, t, model);
  if (!r.ok && !t.stopped && session && /No conversation found|session/i.test(r.why ?? "")) {
    log(`chat ${chatId}: session ${session} could not be resumed (${r.why}); a new one`);
    r = await claude(prompt, null, systemPrompt(chatId, ticket, title, chat), onLine, t, model);
  }
  return r;
}

/** The agent's directory by its name and its real path: a tool names either. */
const CWDS = (() => { try { return [cfg.cwd, fs.realpathSync(cfg.cwd)]; } catch { return [cfg.cwd]; } })();

/** One `claude -p` run, streamed (CHAT-18): each event's lines to `onLine`;
 *  the answer is the stream's `result` event, its session the one any event
 *  names (as `--output-format json`'s result named it before). */
function claude(prompt, session, sys, onLine = () => {}, t = {}, model = architectDefault()) {
  return new Promise((resolve) => {
    const argv = ["-p", "--output-format", "stream-json", "--verbose", "--model", model, "--permission-mode", opt.permissionMode, "--append-system-prompt", sys];
    if (session) argv.push("--resume", session);
    if (t.stopped) return resolve({ ok: false, exit: -1, why: "stopped by the owner (/stop)" });
    const child = spawn(cfg.claude, argv, { cwd: cfg.cwd, env: { ...env, MENGER_URL: cfg.url }, stdio: ["pipe", "pipe", "pipe"] });
    t.child = child;
    let err = "", tail = "", sessionId = null, result = null;
    const lines = jsonLines((ev) => {
      if (ev?.session_id) sessionId = t.session = ev.session_id;
      if (ev?.type === "result") result = ev;
      for (const l of linesOf(ev, CWDS)) onLine(l);
    });
    child.stdout.on("data", (d) => { const t = String(d); lines.push(t); tail = (tail + t).slice(-2000); });
    child.stderr.on("data", (d) => { err += d; if (err.length > 20_000) err = err.slice(-10_000); });
    // CHAT-36: no limit unless one is set — a long build is not a hung turn.
    const timer = cfg.turnMinutes > 0 ? setTimeout(() => { child.kill("SIGTERM"); }, cfg.turnMinutes * 60_000) : null;
    child.on("error", (e) => { clearTimeout(timer); resolve({ ok: false, exit: -1, why: `claude did not start: ${e.message}` }); });
    child.on("close", (code, signal) => {
      clearTimeout(timer);
      lines.push("\n");
      t.child = null;
      const j = result;
      if (t.stopped) return resolve({ ok: false, exit: code ?? -1, session: sessionId, why: "stopped by the owner (/stop)" });
      if (signal) return resolve({ ok: false, exit: -1, session: sessionId, why: `the agent was stopped (${signal}${signal === "SIGTERM" && cfg.turnMinutes > 0 ? `, past ${cfg.turnMinutes} min?` : ""})` });
      if (!j) return resolve({ ok: false, exit: code, session: sessionId, why: `claude exited ${code}: ${(err || tail).trim().split("\n").pop()?.slice(0, 300) || "no output"}` });
      if (j.is_error || code !== 0) return resolve({ ok: false, exit: code, session: sessionId, why: `${j.subtype ?? "error"}: ${String(j.result ?? err).slice(0, 300)}` });
      const text = String(j.result ?? "").trim();
      resolve(text ? { ok: true, exit: code, session: sessionId, text } : { ok: false, exit: code, session: sessionId, why: "the agent gave no final message" });
    });
    child.stdin.on("error", () => {}); // a child stopped before it read the prompt
    child.stdin.end(prompt);
  });
}

// ── the watch, held forever ────────────────────────────────────────────────
let connections = 0;
async function holdWatch() {
  let backoff = 1000;
  for (;;) {
    const h = new Hub();
    const began = Date.now();
    let beat = null;
    try {
      await h.open();
      const who = (await h.call("auth.whoami", {})).find((i) => i.type === "whoami");
      log(`connected to ${cfg.url} as ${who?.principal ?? "?"} (${who?.agent ?? cfg.agent})`);
      const watch = h.stream("chats.watch", {}, (c) => {
        if (c?.type === "posted") { remember(chats.get(c.message.chat)); onMessage(c.message, "watch"); }
        else if (c?.type === "opened" || c?.type === "added" || c?.type === "renamed") remember(c.chat);
        else if (c?.type === "lagged") { log(`watch lagged (${c.missed} missed); catching up`); catchUp("lagged").catch((e) => log(`catch-up: ${e.message}`)); }
        else if (c?.type === "watch_closed") { log(`watch closed by menger: ${c.reason ?? ""} ${c.message ?? ""}`); h.kill("watch_closed"); }
        else if (c?.type === "left") { log(`chat ${c.chat}: left`); chats.delete(c.chat); }
      });
      await watch.subscribed;
      if (h.dead) throw new Error(h.dead);
      hub = h; setLive(h);
      log(`watch held on ${cfg.url}`);
      await flushOutbox();
      await catchUp(connections++ === 0 ? "start" : "re-subscribed");
      // a heartbeat: a socket that answers nothing for 45 s is dead
      beat = setInterval(() => {
        h.call("chats.list", { limit: 1 }, 45_000).catch((e) => { log(`heartbeat: ${e.message}; reconnecting`); h.kill("heartbeat failed"); });
      }, 60_000);
      const why = await Promise.race([watch.done.then(() => "the watch ended", (e) => e.message), h.closed]);
      log(`watch lost: ${why}`);
    } catch (e) {
      log(`cannot hold the watch on ${cfg.url}: ${e.message}`);
    } finally {
      clearInterval(beat);
      h.kill("reconnecting");
      if (hub === h) { hub = null; resetLive(); }
    }
    if (Date.now() - began > 60_000) backoff = 1000;
    log(`re-subscribing in ${backoff / 1000}s`);
    await sleep(backoff);
    backoff = Math.min(backoff * 2, 30_000);
  }
}

process.on("unhandledRejection", (e) => log(`unhandled: ${e?.stack || e}`));
process.on("SIGTERM", () => { log("stopping"); process.exit(0); });
process.on("SIGINT", () => { log("stopping"); process.exit(0); });
log(`chat bridge: ${cfg.url} as ${SELF} (${cfg.agent}); allow list ${cfg.allow}; state ${cfg.state}; agent ${cfg.claude} --permission-mode ${opt.permissionMode} in ${cfg.cwd}, the architect on ${architectDefault()} (${cfg.archetypes}${opt.model ? `; --model ${opt.model} where it names none` : ""}); ${cfg.max} at once`);
holdWatch();
