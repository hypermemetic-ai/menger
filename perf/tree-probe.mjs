// FILTER-13: the kept tree's reads over the WebSocket (`menger.call`, as
// web/index.html speaks it), against a server already running on PORT —
// never the production ports. Medians of RUNS, timed from the send to the
// `page` event (a page) or to `done` (`counts`).
//
//   node perf/tree-probe.mjs after PORT RUNS [ROOT]
//     the floor, `list { parent: ROOT, summary, limit: 200 }`; for each of
//     FILTER-3 §6.2's nine filters, the first `tree { parent: ROOT, filter,
//     limit: 200 }` page (rows, how many only hold matches) and `tree {
//     epic }` of ROOT's shortcode; then `under`: X is ROOT's child holding
//     the most below it — `counts { under X }`, `list { ready and under X }`,
//     `tree { parent: ROOT, status Ready and under X }` (X's row worked out
//     on its own), `tree { parent: X, status Ready }`, and `counts`/`list`
//     `under` ROOT's shortcode.
//
//   node perf/tree-probe.mjs before PORT RUNS [ROOT]
//     the floor, and what the commit before answers nearest a tree page:
//     `list { parent: ROOT, filter, summary, limit: 200 }` — the children
//     that match, with no holder and no count below.
const [mode, port, runs, rootArg] = process.argv.slice(2);
const R = Number(runs);
const ROOT = rootArg || "MAP-1";
if (!["after", "before"].includes(mode) || !port || !R) {
  console.error("usage: tree-probe.mjs after|before PORT RUNS [ROOT]");
  process.exit(2);
}
const EPIC = ROOT.split("-")[0];
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

const floor = await time("tickets.list", { parent: ROOT, summary: true, limit: 200 });
console.log(JSON.stringify({ probe: `list { parent: ${ROOT}, summary, limit: 200 } (the floor)`, median_ms: floor.median_ms, min_ms: floor.min_ms, max_ms: floor.max_ms }));

if (mode === "after") {
  let worst = 0;
  for (const filter of NINE) {
    const t = await time("tickets.tree", { parent: ROOT, filter, limit: 200 });
    const nodes = t.last.events.filter((e) => e.type === "node");
    const page = t.last.events.at(-1);
    const top = await time("tickets.tree", { epic: EPIC, filter });
    const rootNode = top.last.events.find((e) => e.type === "node");
    worst = Math.max(worst, t.median_ms);
    console.log(JSON.stringify({
      filter, tree_first_page: t.median_ms, min_ms: t.min_ms, max_ms: t.max_ms, rows: nodes.length,
      holding_only: nodes.filter((n) => !n.matched && n.below > 0).length, remaining: page.remaining,
      tree_epic: top.median_ms, root_below: rootNode ? rootNode.below : 0,
    }));
  }
  console.log(JSON.stringify({ probe: "the slowest first tree page of the nine", median_ms: worst }));
  // `under`: ROOT's child with the most below it.
  const all = await call("tickets.tree", { parent: ROOT, filter: "" });
  const x = all.events.filter((e) => e.type === "node").sort((a, b) => b.below - a.below)[0];
  const X = x.summary.id;
  const under = {};
  const c = await time("tickets.counts", { filter: `under ${X}` }, "done");
  under.counts_under_x = c.median_ms;
  under.under_x_held = c.last.events.reduce((a, e) => a + (e.held || 0), 0);
  under.list_ready_under_x = (await time("tickets.list", { filter: `ready and under ${X}`, summary: true, limit: 200 })).median_ms;
  under.tree_root_ready_under_x = (await time("tickets.tree", { parent: ROOT, filter: `status Ready and under ${X}`, limit: 200 })).median_ms;
  under.tree_x_ready = (await time("tickets.tree", { parent: X, filter: "status Ready", limit: 200 })).median_ms;
  const ce = await time("tickets.counts", { filter: `under ${EPIC}` }, "done");
  under.counts_under_epic = ce.median_ms;
  under.list_ready_under_epic = (await time("tickets.list", { filter: `ready and under ${EPIC}`, summary: true, limit: 200 })).median_ms;
  console.log(JSON.stringify({ probe: "under", x: X, x_below: x.below, ...under }));
} else {
  for (const filter of NINE) {
    const t = await time("tickets.list", { parent: ROOT, filter, summary: true, limit: 200 });
    console.log(JSON.stringify({ filter, nearest: `list { parent: ${ROOT}, filter, summary, limit: 200 }`, median_ms: t.median_ms, rows: t.last.events.length - 1 }));
  }
}
ws.close();
process.exit(0);
