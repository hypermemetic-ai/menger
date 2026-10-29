// PERF-12: one client process of the concurrency ladder (concurrency.mjs
// forks several, so the client side is not what is measured). It holds a
// share of the rung's signed-in WebSockets — each from its own 127.0.0.x
// source address, each as one of several keys' sessions, each holding the
// watches the app holds — times every write the parent announces from send
// to its arrival on each of its sockets, and runs the per-socket read mix.
//
// Driven over IPC by the parent, never by hand:
//   {cmd:"open", url, tokensFile, sockets:[{i, addr, key}], watches, inflight}
//                                      → {type:"opened", ok, failed, errors, open_ms}
//   {cmd:"load", readEverySecs, reads:[{method, params}]}   → (starts the read mix)
//   {cmd:"write", key, t, kind}        (a write the parent sent, at time t)
//   {cmd:"drain", graceMs}             → {type:"loadStats", …} (reads stopped,
//                                        deliveries awaited up to graceMs)
//   {cmd:"close"}                      → {type:"closed", n}
//
// Times are performance.timeOrigin + performance.now(): one wall clock with
// sub-millisecond resolution, the same in every process on the machine.
// Node ≥ 22, its standard library and deploy/menger-call.mjs's socket only.
import fs from "node:fs";
import { connect } from "../deploy/menger-call.mjs";

const clock = () => performance.timeOrigin + performance.now();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** The cap on the per-delivery latencies one worker returns (a stride
 *  sample past it; the per-write maxima are always whole). */
const SAMPLE_CAP = 250_000;

const sockets = [];          // { ws, i, nextId, subs: Map(subId → kind), calls: Map(id → {method, t, kind}), seen: Map(key → count), alive }
let tokens = [];
let readTimer = null;

// A write: its send time per `${key}#${occurrence}`, and the deliveries
// that arrived before the parent's announcement did.
const sent = new Map();      // wkey → { t, kind }
const early = new Map();     // wkey → [receipt times]
const perWrite = new Map();  // wkey → { kind, n, max, t }
let deliveries = [];         // { kind, ms }
const eventsByType = {};
const reads = { sent: 0, ok: 0, errors: 0, ms: [], byMethod: {} };
const errors = {};
let closedEarly = 0;

function count(map, k) { map[k] = (map[k] ?? 0) + 1; }

function deliver(wkey, kind, t) {
  const s = sent.get(wkey);
  if (!s) { (early.get(wkey) ?? early.set(wkey, []).get(wkey)).push(t); return; }
  record(wkey, s, t);
}
function record(wkey, s, t) {
  const ms = t - s.t;
  deliveries.push({ kind: s.kind, ms });
  const w = perWrite.get(wkey) ?? perWrite.set(wkey, { kind: s.kind, n: 0, max: 0, t: s.t }).get(wkey);
  w.n++;
  if (ms > w.max) w.max = ms;
}

/** Which write an event is, if any: `[wkey, kind]`. A status change is
 *  named by its ticket and new status and told apart by how many times this
 *  socket has heard that pair (each watch hears every write once, in
 *  order). */
function writeOf(sock, kind, content) {
  if (kind === "tickets" && content?.type === "status_changed") {
    const key = `status:${content.ticket?.id}>${content.ticket?.status}`;
    const n = (sock.seen.get(key) ?? 0) + 1;
    sock.seen.set(key, n);
    return [`${key}#${n}`, "status"];
  }
  return null;
}

function onMessage(sock, text) {
  const t = clock();
  let msg;
  try { msg = JSON.parse(text); } catch { return; }
  if (msg.id !== undefined && msg.id !== null) {
    const c = sock.calls.get(msg.id);
    if (!c) return;
    if (msg.error) { sock.calls.delete(msg.id); c.reject?.(new Error(msg.error.message ?? "error")); if (c.read) { reads.errors++; count(errors, `read ${c.method}: ${String(msg.error.message).slice(0, 80)}`); } return; }
    c.sub = msg.result;
    sock.subs.set(msg.result, c);
    c.ack?.();
    return;
  }
  const sub = msg.params?.subscription;
  const c = sub !== undefined ? sock.subs.get(sub) : null;
  if (!c) return;
  const item = msg.params.result;
  if (c.read) {
    if (item?.type === "done") {
      sock.subs.delete(sub); sock.calls.delete(c.id);
      const ms = t - c.t;
      reads.ok++; reads.ms.push(ms);
      const m = reads.byMethod[c.method] ??= { n: 0, ms: [] };
      m.n++; m.ms.push(ms);
    } else if (item?.type === "error") {
      sock.subs.delete(sub); sock.calls.delete(c.id);
      reads.errors++; count(errors, `read ${c.method}: ${JSON.stringify(item).slice(0, 80)}`);
    }
    return;
  }
  // A watch.
  if (item?.type === "data") {
    count(eventsByType, `${c.kind}.${item.content?.type ?? "?"}`);
    const w = writeOf(sock, c.kind, item.content);
    if (w) deliver(w[0], w[1], t);
  } else if (item?.type === "error" || item?.type === "done") {
    count(eventsByType, `${c.kind}.${item.type}`);
  }
}

function callOn(sock, method, params, extra) {
  const id = sock.nextId++;
  const c = { id, method, t: clock(), ...extra };
  sock.calls.set(id, c);
  sock.ws.send(JSON.stringify({ jsonrpc: "2.0", id, method: "menger.call", params: { method, params } }));
  return c;
}

async function openOne(url, spec, watches) {
  const t0 = clock();
  const ws = await connect(url, { Authorization: `Bearer ${tokens[spec.key]}` }, { localAddress: spec.addr });
  const sock = { ws, i: spec.i, nextId: 1, subs: new Map(), calls: new Map(), seen: new Map(), alive: true };
  ws.onmessage = (text) => onMessage(sock, text);
  ws.onclose = (why) => { if (sock.alive) { sock.alive = false; closedEarly++; count(errors, `closed: ${String(why).slice(0, 80)}`); } };
  // The watches, as the app opens them; open once every one is acknowledged.
  await Promise.all(watches.map((w) => new Promise((ack, reject) => {
    const [kind] = w.method.split(".");
    callOn(sock, w.method, w.params, { kind, ack, reject });
  })));
  sockets.push(sock);
  return clock() - t0;
}

async function open(m) {
  tokens = fs.readFileSync(m.tokensFile, "utf8").split("\n").filter(Boolean);
  let ok = 0, failed = 0, next = 0;
  const openMs = [];
  const one = async () => {
    while (next < m.sockets.length) {
      const spec = m.sockets[next++];
      try {
        openMs.push(await Promise.race([openOne(m.url, spec, m.watches), sleep(m.timeoutMs).then(() => { throw new Error("timed out"); })]));
        ok++;
      } catch (e) {
        failed++;
        count(errors, `open: ${String(e.message).replace(/key:[0-9a-f]+/g, "key:…").slice(0, 120)}`);
      }
    }
  };
  await Promise.all(Array.from({ length: Math.min(m.inflight, m.sockets.length) }, one));
  process.send({ type: "opened", ok, failed, errors, open_ms: openMs });
}

function load(m) {
  // Each socket reads every readEverySecs, from a random phase, cycling
  // through the mix.
  const period = m.readEverySecs * 1000;
  if (!(period > 0) || !m.reads.length) return;
  const start = clock();
  const due = sockets.map(() => start + Math.random() * period);
  const which = sockets.map((_, k) => k % m.reads.length);
  const tick = () => {
    const now = clock();
    for (let k = 0; k < sockets.length; k++) {
      const s = sockets[k];
      if (!s.alive || due[k] > now) continue;
      due[k] += period;
      const r = m.reads[which[k]++ % m.reads.length];
      reads.sent++;
      try { callOn(s, r.method, r.params, { read: true }); } catch { reads.errors++; }
    }
  };
  readTimer = setInterval(tick, 20);
}

function onWrite(m) {
  const wkey = `${m.key}#${m.occurrence}`;
  const s = { t: m.t, kind: m.kind };
  sent.set(wkey, s);
  const e = early.get(wkey);
  if (e) { early.delete(wkey); for (const t of e) record(wkey, s, t); }
}

async function drain(m) {
  clearInterval(readTimer);
  readTimer = null;
  const live = sockets.filter((s) => s.alive).length;
  const expect = (kind) => live * (m.watchKinds.includes(kind) ? 1 : 0);
  const deadline = clock() + m.graceMs;
  const done = () => [...sent.entries()].every(([k, s]) => (perWrite.get(k)?.n ?? 0) >= expect("tickets"))
    && [...sockets].every((s) => [...s.calls.values()].every((c) => !c.read));
  while (clock() < deadline && !done()) await sleep(50);
  let sample = deliveries;
  if (sample.length > SAMPLE_CAP) {
    const stride = sample.length / SAMPLE_CAP;
    sample = Array.from({ length: SAMPLE_CAP }, (_, k) => sample[Math.floor(k * stride)]);
  }
  const inFlightReads = sockets.reduce((a, s) => a + [...s.calls.values()].filter((c) => c.read).length, 0);
  process.send({
    type: "loadStats",
    live,
    deliveries_total: deliveries.length,
    deliveries: sample,
    sampled: sample.length < deliveries.length,
    per_write: Object.fromEntries([...sent.keys()].map((k) => [k, perWrite.get(k) ?? { kind: sent.get(k).kind, n: 0, max: null }])),
    early_unmatched: [...early.values()].reduce((a, v) => a + v.length, 0),
    reads: { sent: reads.sent, ok: reads.ok, errors: reads.errors, in_flight: inFlightReads, ms: reads.ms, by_method: reads.byMethod },
    events_by_type: eventsByType,
    errors,
    closed_early: closedEarly,
  });
}

async function close() {
  const n = sockets.length;
  for (const s of sockets) { s.alive = false; try { s.ws.close(); } catch { /* gone */ } }
  await sleep(200);
  for (const s of sockets) { try { s.ws.sock.destroy(); } catch { /* gone */ } }
  sockets.length = 0;
  process.send({ type: "closed", n });
}

process.on("message", (m) => {
  const run = { open, load, write: onWrite, drain, close }[m.cmd];
  if (!run) return;
  Promise.resolve(run(m)).catch((e) => process.send({ type: "error", message: e.message }));
});
process.on("disconnect", () => process.exit(0));
