// FILTER-4: the filter's reads over the WebSocket (`menger.call`, as
// web/index.html speaks it), against a server already running on PORT —
// never the production ports. Medians of RUNS, timed from the send to the
// `page` event (a page) or to `done` (`counts`, `facets`).
//
//   node perf/filter-probe.mjs after PORT RUNS
//     for each of FILTER-3 §6.2's nine filters: `counts { filter }`;
//     `list { filter, summary, limit: 200 }` at the head, after MAP-1, and
//     at `at = held / 2`; `facets { filter }` — and the transport's floor,
//     `list { summary, limit: 200 }`.
//
//   node perf/filter-probe.mjs before PORT RUNS
//     what the commit before answers of the same: the floor, and the nine
//     filters where it has a call that answers one (`list { status }`,
//     `ready_set`) — at the head, after MAP-1 and after the middle match.
const [mode, port, runs] = process.argv.slice(2);
const R = Number(runs);
if (!["after", "before"].includes(mode) || !port || !R) {
  console.error("usage: filter-probe.mjs after|before PORT RUNS");
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

export const NINE = [
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

/// RUNS calls; the median, min and max of `until` - send, and the last answer.
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

const floor = await time("tickets.list", { summary: true, limit: 200 });
console.log(JSON.stringify({ probe: "list {} (the floor)", median_ms: floor.median_ms, min_ms: floor.min_ms, max_ms: floor.max_ms }));

if (mode === "after") {
  for (const filter of NINE) {
    const out = { filter };
    const counts = await time("tickets.nav.counts", { filter }, "done");
    const held = counts.last.events.reduce((a, e) => a + e.held, 0);
    out.matches = held;
    out.counts = counts.median_ms;
    const head = await time("tickets.list", { filter, summary: true, limit: 200 });
    out.head = head.median_ms;
    out.head_rows = head.last.events.length - 1;
    const map = await time("tickets.list", { filter, summary: true, limit: 200, after: "MAP-1" });
    out.after_map1 = map.median_ms;
    const at = Math.floor(held / 2);
    const mid = await time("tickets.list", { filter, summary: true, limit: 200, at });
    out.at_half = mid.median_ms;
    out.at_half_first = mid.last.events.at(-1).first;
    const facets = await time("tickets.nav.facets", { filter }, "done");
    out.facets = facets.median_ms;
    out.max_of_the_five_ms = Math.max(out.counts, out.head, out.after_map1, out.at_half, out.facets);
    console.log(JSON.stringify(out));
  }
} else {
  // What the commit before can answer of the nine: a status, the ready set.
  const equivalents = {
    "status Ready": ["tickets.list", { status: "Ready" }],
    "ready": ["tickets.ready_set", {}],
  };
  for (const filter of NINE) {
    const eq = equivalents[filter];
    if (!eq) {
      console.log(JSON.stringify({ filter, before: "no call answers it (no counts, no position)" }));
      continue;
    }
    const [method, base] = eq;
    const head = await time(method, { ...base, summary: true, limit: 200 });
    const map = await time(method, { ...base, summary: true, limit: 200, after: "MAP-1" });
    // No `at`: the nearest a client could get is a walk; the cost of one
    // page from the middle of the store stands in for it.
    const mid = await time(method, { ...base, summary: true, limit: 200, after: "MAP-80000" });
    console.log(JSON.stringify({ filter, via: `${method} ${JSON.stringify(base)}`, head: head.median_ms, after_map1: map.median_ms,
      after_map80000: mid.median_ms, matches_after_head: head.last.events.at(-1).remaining + head.last.events.length - 1 }));
  }
}
ws.close();
process.exit(0);
