// FILTER-4, acceptance 7: every page given neither `filter` nor `at`, and
// every `ready_set` and `find` page, byte for byte what the commit before
// answers — two servers on copies of one file, the same calls to each, each
// item compared as the bytes that arrived, the subscription id and the
// envelope's `metadata` (the schema's hash and the time) aside.
//
//   node perf/filter-identical.mjs BEFORE_PORT AFTER_PORT SEED
//
// The calls: `list` pages (summary and whole) after cursors spread over the
// store, with `epic`, `status` and `parent`; `ready_set` pages; `find`
// pages for common, rare and absent queries; and an unpaged `list { epic }`
// of a small shortcode. Prints the number of calls and items compared, and
// every difference; exits 1 on one.
const [before, after, seedArg] = process.argv.slice(2);
if (!before || !after) {
  console.error("usage: filter-identical.mjs BEFORE_PORT AFTER_PORT [SEED]");
  process.exit(2);
}
let seed = BigInt(seedArg ?? 7);
const rand = (n) => {
  seed = (seed + 0x9e3779b97f4a7c15n) & 0xffffffffffffffffn;
  let z = seed;
  z = ((z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n) & 0xffffffffffffffffn;
  z = ((z ^ (z >> 27n)) * 0x94d049bb133111ebn) & 0xffffffffffffffffn;
  return Number((z ^ (z >> 31n)) % BigInt(n));
};
const open = (port) => new Promise((ok, no) => { const ws = new WebSocket(`ws://127.0.0.1:${port}`); ws.onopen = () => ok(ws); ws.onerror = no; });

/// Calls on one socket: each answer's items, as the bytes that arrived with
/// the subscription id taken out.
function caller(ws) {
  let pending = null, id = 0;
  ws.onmessage = (m) => {
    const text = String(m.data);
    const msg = JSON.parse(text);
    const item = msg.params?.result;
    if (!item || !pending) return;
    if (item.type === "done") {
      const p = pending; pending = null; p.resolve(p.items);
    } else {
      // The envelope's `metadata` is the transport's — the schema's hash,
      // which any new method moves, and the time — not the answer.
      pending.items.push(text.replace(/"subscription":("[^"]*"|\d+)/, '"subscription":_').replace(/"metadata":\{"provenance":\[[^\]]*\],"plexus_hash":"[0-9a-f]+","timestamp":\d+\},/, ""));
    }
  };
  return (method, params) => new Promise((resolve) => {
    pending = { resolve, items: [] };
    ws.send(JSON.stringify({ jsonrpc: "2.0", id: ++id, method: "menger.call", params: { method, params } }));
  });
}

const [a, b] = [caller(await open(before)), caller(await open(after))];
const extents = (await a("tickets.extents", {})).map((t) => JSON.parse(t.replace('"subscription":_', '"subscription":0')).params.result.content);
const epics = extents.map((e) => [e.epic, e.last_seq]);
const anyId = () => { const [e, n] = epics[rand(epics.length)]; return `${e}-${1 + rand(n)}`; };
const small = epics.reduce((m, e) => (e[1] < m[1] ? e : m));
const big = epics.reduce((m, e) => (e[1] > m[1] ? e : m));
const statuses = ["Pending", "Ready", "Coding", "In Review", "Blocked", "Complete", "Idea", "Epic", "Superseded", "Doc"];

const calls = [];
for (let i = 0; i < 1200; i++) {
  const after = rand(5) === 0 ? undefined : anyId();
  const limit = [0, 1, 50, 200][rand(4)];
  const p = { summary: true, limit, ...(after ? { after } : {}) };
  switch (rand(4)) {
    case 0: calls.push(["tickets.list", p]); break;
    case 1: calls.push(["tickets.list", { ...p, epic: epics[rand(epics.length)][0] }]); break;
    case 2: calls.push(["tickets.list", { ...p, status: statuses[rand(statuses.length)] }]); break;
    default: calls.push(["tickets.list", { ...p, parent: `${big[0]}-1` }]); break;
  }
}
for (let i = 0; i < 100; i++) calls.push(["tickets.list", { limit: 20, ...(rand(3) ? { after: anyId() } : {}) }]);
for (let i = 0; i < 1100; i++) {
  const after = rand(5) === 0 ? undefined : anyId();
  calls.push(["tickets.ready_set", { summary: true, limit: [0, 1, 50, 200][rand(4)], ...(after ? { after } : {}), ...(rand(3) === 0 ? { epic: epics[rand(epics.length)][0] } : {}) }]);
}
for (let i = 0; i < 50; i++) calls.push(["tickets.ready_set", { limit: 20, ...(rand(2) ? { after: anyId() } : {}) }]);
const queries = ["audit", "map", "zzqx", "1", "SCALE", "the", "x"];
for (let i = 0; i < 1200; i++) {
  calls.push(["tickets.find", { query: queries[rand(queries.length)], limit: [1, 50, 200][rand(3)], ...(rand(3) ? { after: anyId() } : {}) }]);
}
calls.push(["tickets.list", { epic: small[0] }]);
calls.push(["tickets.list", { epic: small[0], summary: true }]);
calls.push(["tickets.ready_set", { epic: small[0] }]);

let items = 0;
const diffs = [];
const started = Date.now();
for (const [method, params] of calls) {
  const [x, y] = [await a(method, params), await b(method, params)];
  items += x.length;
  if (x.length !== y.length || x.some((t, i) => t !== y[i])) {
    const i = x.findIndex((t, k) => t !== y[k]);
    diffs.push({ method, params, before_items: x.length, after_items: y.length, first: i, before: x[i]?.slice(0, 300), after: y[i]?.slice(0, 300) });
  }
}
console.log(JSON.stringify({ calls: calls.length, items, differing_calls: diffs.length, seconds: (Date.now() - started) / 1000 }));
for (const d of diffs.slice(0, 20)) console.log(JSON.stringify(d));
process.exit(diffs.length ? 1 : 0);
