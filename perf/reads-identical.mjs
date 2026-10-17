// FILTER-15's must-not-change: `find`, `ready_set` and `inputs` without a
// filter — and with one that narrows nothing (empty, a display part alone),
// which is no filter — byte for byte what the commit before answers without
// one. Two servers on copies of one file, the same calls to each, each item
// compared as the bytes that arrived, the subscription id and the envelope's
// `metadata` (the schema's hash and the time) aside.
//
//   node perf/reads-identical.mjs BEFORE_PORT AFTER_PORT SEED
//
// Prints the number of calls and items compared, and every difference;
// exits 1 on one.
// ACCESS-52: menger has no open mode (ACCESS-24), so every socket here
// signs in: it carries `Authorization: Bearer <token>` from the file
// $MENGER_SESSION_FILES names, BEFORE's then AFTER's, comma-separated (a
// session belongs to the server that minted it; one $MENGER_SESSION_FILE
// serves both when they share it) — an mgs_ token each, for a throwaway
// server started `--auth-provider key --admin <its key>`:
//   node scripts/throwaway-login.mjs session --url ws://127.0.0.1:PORT --key KEY --out FILE
//   MENGER_SESSION_FILES=BEFORE_FILE,AFTER_FILE node perf/<probe>.mjs BEFORE_PORT AFTER_PORT …
import { bearer } from "../scripts/throwaway-login.mjs";
const [before, after, seedArg] = process.argv.slice(2);
if (!before || !after) {
  console.error("usage: reads-identical.mjs BEFORE_PORT AFTER_PORT [SEED]");
  process.exit(2);
}
let seed = BigInt(seedArg ?? 15);
const rand = (n) => {
  seed = (seed + 0x9e3779b97f4a7c15n) & 0xffffffffffffffffn;
  let z = seed;
  z = ((z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n) & 0xffffffffffffffffn;
  z = ((z ^ (z >> 27n)) * 0x94d049bb133111ebn) & 0xffffffffffffffffn;
  return Number((z ^ (z >> 31n)) % BigInt(n));
};
const sessions = (process.env.MENGER_SESSION_FILES || "").split(",").filter(Boolean);
const sessionFor = (port) => sessions[port === before ? 0 : 1] ?? process.env.MENGER_SESSION_FILE;
const open = (port) => new Promise((ok, no) => { const ws = new WebSocket(`ws://127.0.0.1:${port}`, { headers: bearer(sessionFor(port)) }); ws.onopen = () => ok(ws); ws.onerror = no; });

function caller(ws) {
  let pending = null, id = 0;
  ws.onmessage = (m) => {
    const text = String(m.data);
    const item = JSON.parse(text).params?.result;
    if (!item || !pending) return;
    if (item.type === "done") {
      const p = pending; pending = null; p.resolve(p.items);
    } else {
      pending.items.push(text.replace(/"subscription":("[^"]*"|\d+)/, '"subscription":_').replace(/"metadata":\{"provenance":\[[^\]]*\],"plexus_hash":"[0-9a-f]+","timestamp":\d+\},/, ""));
    }
  };
  return (method, params) => new Promise((resolve) => {
    pending = { resolve, items: [] };
    ws.send(JSON.stringify({ jsonrpc: "2.0", id: ++id, method: "menger.call", params: { method, params } }));
  });
}

const [a, b] = [caller(await open(before)), caller(await open(after))];
const content = (t) => JSON.parse(t.replace('"subscription":_', '"subscription":0')).params.result.content;
const epics = (await a("tickets.nav.extents", {})).map(content).map((e) => [e.epic, e.last_seq]);
const anyId = () => { const [e, n] = epics[rand(epics.length)]; return `${e}-${1 + rand(n)}`; };
const inputs = (await a("tickets.inputs.list", {})).map(content).map((e) => e.input);
const nothing = () => [{}, { filter: "" }, { filter: "· flat" }][rand(3)];

const calls = [];
const queries = ["audit", "map", "zzqx", "1", "SCALE", "the", "x", "", "watch", "FILTER"];
for (let i = 0; i < 600; i++) {
  const p = { query: queries[rand(queries.length)], limit: [0, 1, 50, 200][rand(4)], ...(rand(3) ? { after: anyId() } : {}) };
  calls.push([["tickets.find", p], ["tickets.find", { ...p, ...nothing() }]]);
}
for (let i = 0; i < 500; i++) {
  const p = { summary: rand(4) > 0, limit: [0, 1, 50, 200][rand(4)], ...(rand(5) ? { after: anyId() } : {}), ...(rand(3) === 0 ? { epic: epics[rand(epics.length)][0] } : {}) };
  calls.push([["tickets.work.ready", p], ["tickets.work.ready", { ...p, ...nothing() }]]);
}
const states = [undefined, "open", "answered", "closed", "removed"];
for (let i = 0; i < 200; i++) {
  const p = {
    ...(rand(3) === 0 && inputs.length ? { ticket: inputs[rand(inputs.length)].ticket } : {}),
    ...(rand(3) === 0 ? { epic: epics[rand(epics.length)][0] } : {}),
    ...(states[rand(states.length)] ? { state: states[rand(states.length)] } : {}),
  };
  calls.push([["tickets.inputs.list", p], ["tickets.inputs.list", { ...p, ...nothing() }]]);
}
calls.push([["tickets.inputs.list", { ticket: "NOPE-1" }], ["tickets.inputs.list", { ticket: "NOPE-1", filter: "" }]]);
calls.push([["tickets.find", { query: "x", after: "NOPE-1" }], ["tickets.find", { query: "x", after: "NOPE-1", filter: "" }]]);

let items = 0;
const diffs = [];
const started = Date.now();
for (const [[m1, p1], [m2, p2]] of calls) {
  // The commit before, without a filter; this one without, then with one
  // that narrows nothing.
  const x = await a(m1, p1);
  for (const [method, params] of [[m1, p1], [m2, p2]]) {
    const y = await b(method, params);
    items += x.length;
    if (x.length !== y.length || x.some((t, i) => t !== y[i])) {
      const i = x.findIndex((t, k) => t !== y[k]);
      diffs.push({ method, params, before_items: x.length, after_items: y.length, first: i, before: x[i]?.slice(0, 300), after: y[i]?.slice(0, 300) });
    }
  }
}
console.log(JSON.stringify({ calls: calls.length * 2, items, inputs_held: inputs.length, differing_calls: diffs.length, seconds: (Date.now() - started) / 1000 }));
for (const d of diffs.slice(0, 20)) console.log(JSON.stringify(d));
process.exit(diffs.length ? 1 : 0);
