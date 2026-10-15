// FILTER-15: the reads that take the lens, over the WebSocket (`menger.call`,
// as web/index.html speaks it), against a server already running on PORT —
// never the production ports. Medians of RUNS, timed from the send to the
// `page` event (`find`, `ready_set`) or to `done` (`member`).
//
//   node perf/reads-probe.mjs PORT RUNS
//
// - `find { query }` for a common, a rare and an absent query — the
//   unfiltered baseline, whose `remaining` tests every id and title in the
//   store — and `find { query, filter }` for each of FILTER-3 §6.2's nine
//   filters, with the ratio filtered / unfiltered;
// - `member { ids, filter }` of 1,000 random ids for each of the nine;
// - `ready_set { filter, summary, limit: 200 }` at the head and after MAP-1.
const [port, runs] = process.argv.slice(2);
const R = Number(runs);
if (!port || !R) {
  console.error("usage: reads-probe.mjs PORT RUNS");
  process.exit(2);
}
const open = () => new Promise((ok, no) => { const ws = new WebSocket(`ws://127.0.0.1:${port}`); ws.onopen = () => ok(ws); ws.onerror = no; });
const med = (v) => { const s = [...v].sort((a, b) => a - b); return s[Math.floor(s.length / 2)]; };
const ms = (v) => +v.toFixed(3);

function caller(ws) {
  let pending = null, id = 0;
  ws.onmessage = (m) => {
    const item = JSON.parse(String(m.data)).params?.result;
    if (!item || !pending) return;
    if (item.type === "data") {
      pending.events.push(item.content);
      if (item.content.type === "page") pending.page = performance.now();
    } else if (item.type === "done") {
      const p = pending; pending = null; p.done = performance.now(); p.resolve(p);
    }
  };
  return (method, params) => new Promise((resolve) => {
    pending = { resolve, events: [], t0: performance.now() };
    ws.send(JSON.stringify({ jsonrpc: "2.0", id: ++id, method: "menger.call", params: { method, params } }));
  });
}

const NINE = [
  "status Ready",
  "status in (Pending, Ready) and kind build",
  "ready",
  "blocked and severity critical",
  "kind scope and status Idea and severity critical",
  "category in (done, dropped)",
  "(status Blocked or blocked) and not kind doc",
  "not kind doc",
  "category in (active, review)",
];

const ws = await open();
const call = caller(ws);

async function time(method, params, until = "page") {
  const ts = [];
  let last;
  for (let i = 0; i < R; i++) {
    last = await call(method, params);
    const err = last.events.find((e) => e.type === "error");
    if (err) throw new Error(`${method} ${JSON.stringify(params)}: ${err.message}`);
    ts.push((until === "page" ? last.page : last.done) - last.t0);
  }
  return { median_ms: ms(med(ts)), min_ms: ms(Math.min(...ts)), max_ms: ms(Math.max(...ts)), last };
}

// The store's shape, for ids.
let seed = 0x15n;
const rand = (n) => {
  seed = (seed + 0x9e3779b97f4a7c15n) & 0xffffffffffffffffn;
  let z = seed;
  z = ((z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n) & 0xffffffffffffffffn;
  z = ((z ^ (z >> 27n)) * 0x94d049bb133111ebn) & 0xffffffffffffffffn;
  return Number((z ^ (z >> 31n)) % BigInt(n));
};
const extents = (await call("tickets.nav.extents", {})).events.map((e) => [e.epic, e.last_seq]);
const total = extents.reduce((a, [, n]) => a + n, 0);
const anyId = () => {
  // Uniform over tickets, not shortcodes.
  let k = rand(total);
  for (const [e, n] of extents) {
    if (k < n) return `${e}-${k + 1}`;
    k -= n;
  }
};
const ids = Array.from({ length: 1000 }, anyId);
console.log(JSON.stringify({ tickets: total, shortcodes: extents.length }));

const QUERIES = ["audit", "zzqx", "the"];
const base = {};
for (const query of QUERIES) {
  const t = await time("tickets.find", { query });
  const page = t.last.events.at(-1);
  base[query] = t.median_ms;
  console.log(JSON.stringify({ probe: "find", query, filter: null, median_ms: t.median_ms, min_ms: t.min_ms, max_ms: t.max_ms,
    matches: page.remaining + t.last.events.length - 1 }));
}
for (const filter of NINE) {
  const counts = (await call("tickets.nav.counts", { filter })).events.reduce((a, e) => a + e.held, 0);
  const out = { filter, admits: counts, admits_pct: +(100 * counts / total).toFixed(2) };
  for (const query of QUERIES) {
    const t = await time("tickets.find", { query, filter });
    const page = t.last.events.at(-1);
    out[`find ${query}`] = t.median_ms;
    out[`find ${query} matches`] = page.remaining + t.last.events.length - 1;
    out[`find ${query} / unfiltered`] = +(t.median_ms / base[query]).toFixed(4);
  }
  const m = await time("tickets.nav.member", { ids, filter }, "done");
  out.member_1000 = m.median_ms;
  out.member_1000_max = m.max_ms;
  out.member_matched = m.last.events.filter((e) => e.type === "member" && e.matched).length;
  const head = await time("tickets.ready_set", { filter, summary: true, limit: 200 });
  out.ready_head = head.median_ms;
  const map = await time("tickets.ready_set", { filter, summary: true, limit: 200, after: "MAP-1" });
  out.ready_after_map1 = map.median_ms;
  console.log(JSON.stringify(out));
}
const empty = await time("tickets.nav.member", { ids, filter: "" }, "done");
console.log(JSON.stringify({ probe: "member 1000, empty filter", median_ms: empty.median_ms, max_ms: empty.max_ms }));
ws.close();
process.exit(0);
