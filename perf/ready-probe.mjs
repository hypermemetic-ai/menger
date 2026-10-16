// SCALE-32: the two probes behind its before/after and fan-out sheets, over
// one WebSocket each (`menger.call`, as web/index.html speaks it), against a
// server already running on PORT — never the production ports.
//
//   node perf/ready-probe.mjs pages PORT RUNS CURSOR...   ("-" is the head)
//     the first page of `ready_set { summary, limit: 200 }`, of `list {
//     status: "Ready", summary, limit: 200 }` and, as the transport's own
//     floor, of `list { summary, limit: 200 }`, after each cursor: median,
//     min and max of RUNS, timed from the send to the `page` event.
//
//   node perf/ready-probe.mjs fan-out PORT RUNS           (perf/fan-out.py's store)
//     `set_status` of FAN-2, FAN-3 and FAN-4 (blocking 1, 10^3 and 10^5) to
//     Complete and back to Ready, RUNS crossings each way: the latency (send
//     to `done`), the bytes a `watch` subscriber receives for the write and
//     the bytes of the answer, and the ready set in FAN after it.
const [mode, port, runs, ...cursors] = process.argv.slice(2);
const R = Number(runs);
if (!["pages", "fan-out"].includes(mode) || !port || !R) {
  console.error("usage: ready-probe.mjs pages PORT RUNS CURSOR... | fan-out PORT RUNS");
  process.exit(2);
}
const open = () => new Promise((ok, no) => { const ws = new WebSocket(`ws://127.0.0.1:${port}`); ws.onopen = () => ok(ws); ws.onerror = no; });
const med = (v) => { const s = [...v].sort((a, b) => a - b); return s[Math.floor(s.length / 2)]; };
const ms = (v) => +v.toFixed(3);

/// One logical call at a time on `ws`: its data events, their bytes, when
/// the `page` event and the `done` arrived.
function caller(ws) {
  let pending = null, id = 0;
  ws.onmessage = (m) => {
    const text = String(m.data);
    const item = JSON.parse(text).params?.result;
    if (!item || !pending) return;
    if (item.type === "data") {
      pending.events.push(item.content);
      pending.bytes += text.length;
      if (item.content.type === "page") pending.page = performance.now();
    } else if (item.type === "done") {
      const p = pending; pending = null; p.done = performance.now(); p.resolve(p);
    }
  };
  return (method, params) => new Promise((resolve) => {
    pending = { resolve, events: [], bytes: 0, t0: performance.now() };
    ws.send(JSON.stringify({ jsonrpc: "2.0", id: ++id, method: "menger.call", params: { method, params } }));
  });
}

if (mode === "pages") {
  const ws = await open();
  const call = caller(ws);
  for (const cursor of cursors) {
    for (const [label, method, base] of [
      ["ready_set", "tickets.work.ready", { summary: true, limit: 200 }],
      ["list { status: Ready }", "tickets.list", { status: "Ready", summary: true, limit: 200 }],
      ["list {} (the floor)", "tickets.list", { summary: true, limit: 200 }],
    ]) {
      const params = cursor === "-" ? base : { ...base, after: cursor };
      const ts = [];
      let last;
      for (let i = 0; i < R; i++) {
        last = await call(method, params);
        const err = last.events.find((e) => e.type === "error");
        if (err) throw new Error(`${label} after ${cursor}: ${err.message}`);
        ts.push(last.page - last.t0);
      }
      const page = last.events.at(-1);
      console.log(JSON.stringify({ label, after: cursor, rows: last.events.length - 1, remaining: page.remaining,
        median_ms: ms(med(ts)), min_ms: ms(Math.min(...ts)), max_ms: ms(Math.max(...ts)) }));
    }
  }
  ws.close();
} else {
  const watcher = await open();
  const seen = [];
  watcher.onmessage = (m) => {
    const text = String(m.data);
    const item = JSON.parse(text).params?.result;
    if (item?.type !== "data") return;
    seen.push({ type: item.content.type, bytes: text.length, id: item.content.ticket?.id, status: item.content.ticket?.status });
  };
  watcher.send(JSON.stringify({ jsonrpc: "2.0", id: 1, method: "menger.call", params: { method: "tickets.watch", params: {} } }));
  const call = caller(await open());
  const pause = (t) => new Promise((r) => setTimeout(r, t));
  const until = async (pred, t) => { const end = Date.now() + t; while (!pred() && Date.now() < end) await pause(5); };
  await pause(300);
  for (const [blocker, k] of [["FAN-2", 1], ["FAN-3", 1_000], ["FAN-4", 100_000]]) {
    const out = { k, blocker };
    for (const [way, status] of [["to Complete", "Complete"], ["back to Ready", "Ready"]]) {
      const ts = [], watch = [], answer = [], types = {};
      let ready;
      for (let i = 0; i < R; i++) {
        // Each run is one crossing, measured; the one that sets it up is not.
        if (status === "Ready") await call("tickets.set_status", { id: blocker, status: "Complete" });
        await pause(150);
        const mark = seen.length;
        const p = await call("tickets.set_status", { id: blocker, status });
        if (p.events[0]?.type !== "status_changed") throw new Error(JSON.stringify(p.events[0]).slice(0, 200));
        ts.push(p.done - p.t0);
        answer.push(p.bytes);
        await until(() => seen.slice(mark).some((e) => e.type === "status_changed" && e.id === blocker && e.status === status), 30_000);
        await pause(150);
        const mine = seen.slice(mark);
        watch.push(mine.reduce((a, e) => a + e.bytes, 0));
        for (const e of mine) types[e.type] = (types[e.type] ?? 0) + 1 / R;
        ready = (await call("tickets.work.ready", { epic: "FAN", summary: true, limit: 0 })).events.at(-1).remaining;
        if (status === "Complete") await call("tickets.set_status", { id: blocker, status: "Ready" });
      }
      out[way] = { median_ms: ms(med(ts)), min_ms: ms(Math.min(...ts)), max_ms: ms(Math.max(...ts)),
        watch_bytes: med(watch), answer_bytes: med(answer), watch_events: types, ready_in_FAN: ready };
    }
    console.log(JSON.stringify(out));
  }
  watcher.close();
  process.exit(0);
}
