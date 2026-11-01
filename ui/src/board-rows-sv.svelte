<!-- FRAME-14: board-rows in real Svelte (svelte/1), board-rows.rhx's twin in
     behaviour and marks. Unfiltered: `nav extents` once — each shortcode's
     last sequence and holes — and the row a scroll lands on is an id by
     arithmetic over them; the window is one `list { after, limit: 200,
     summary: true }` page, re-asked only when a scroll leaves it (after 20 ms
     of quiet: FRAME-25, the spec's `debounce_ms` in the host's registry, as
     rhx's; no timer of its own), drawn at its offset. Filtered (the route's `filter`): the count is
     `nav counts { filter }`'s sum and the page is `list { filter, at }`.
     `created` moves the count and puts the row in; `status_changed` patches it
     in place (a keyed each: the same element). FRAME-20: the window iterates
     the handle's getter, `rows.rows`, not the store's `$rows.rows`: a row a
     change leaves alone is the same object, so only the changed row renders.
     SURFACE-2: the board's keys (FRAME-36, `bus.keys`) and its selection —
     `sel` the selected row's id ([data-selected], one, scrolled into view),
     `checked` the multi-selection ([data-checked]; the box beside a row
     checks it by click), both by id, so a window move keeps them. Every
     change says `board.selected { ids }` and `board.focus { id }` on the bus
     (the bulk bar, SURFACE-5, hears the first); `/` says `filter.focus`
     (the filter line's to take); `ctrl+k` is the palette's own. The host draws
     `?`'s overlay from the labels. PERF-22: its first answer drawn, it says
     `board.drawn` once on the bus — sticky (FRAME-53): kept for a nav section
     mounted after it — and keeps `[data-drawn]` (the tests' mark; no component
     looks at it): the nav's facets, saved filters and counts ask then, not
     beside the rows. -->
<script module>
  // PERF-15: a row is the four fields drawn, not the whole summary row.
  const FIELDS = ["id", "title", "kind", "status"];
  export const uses = ["tickets.nav.extents", "tickets.watch", "tickets.nav.counts", "tickets.list"];
</script>

<script>
  import { tick, untrack } from "svelte";
  import { bus, live } from "plexus-ui/host";

  let { filter = null } = $props();
  const b = bus();
  const PITCH = 28;
  const PAGE = 200;
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";

  // The extents first: the board's first call, before the window's list.
  const extents = live({ snapshot: "tickets.nav.extents", params: {}, changes: "tickets.watch", key: "shortcode",
                           on: { extent: "upsert:.", created: "upsert:ticket" } });
  const counts = live({ snapshot: "tickets.nav.counts", params: () => ({ filter: filter ?? null }), key: "shortcode", on: { count: "upsert:." } });

  let first = $state(0);
  let win = $state({ after: null, at: null });
  const rows = live({ snapshot: "tickets.list",
    params: () => ({ summary: true, limit: PAGE, fields: FIELDS, filter: filter ?? null, after: win.after, at: win.at }),
    changes: "tickets.watch", changes_params: () => ({ filter: filter ?? null }), key: "id",
    on: { created: "upsert:ticket", status_changed: "upsert:ticket", amended: "upsert:ticket",
          kind_changed: "upsert:ticket", labels_changed: "upsert:ticket", deleted: "remove:ids" },
    debounce_ms: 20 });
  // PERF-22: the first answer drawn (rows, none, or a refusal), said once.
  let drawn = $state(false);
  $effect(() => {
    if (rows.status === "pending") return;
    untrack(() => {
      if (drawn) return;
      drawn = true;
      b.emit("board.drawn", {}, { sticky: true });
    });
  });
  const held = (e) => (e.last_seq ?? e.seq) - (e.holes ?? []).length;
  const total = $derived(filter == null ? $extents.rows.reduce((a, e) => a + held(e), 0) : $counts.rows.reduce((a, c) => a + c.held, 0));

  // The row `start` (0-based) by arithmetic over the extents: the id it follows.
  function afterOf(start) {
    const ext = [...$extents.rows].sort((a, z) => (a.shortcode < z.shortcode ? -1 : a.shortcode > z.shortcode ? 1 : 0));
    let k = start - 1;
    for (const e of ext) {
      const holes = e.holes ?? [];
      const n = held(e);
      if (k >= n) { k -= n; continue; }
      let seq = 0;
      for (;;) {
        seq += 1;
        if (!holes.includes(seq)) {
          if (k === 0) break;
          k -= 1;
        }
      }
      return e.epic + "-" + seq;
    }
    return win.after;
  }

  // SURFACE-2: the selection and the keys.
  let sel = $state(null);
  let checked = $state([]);
  let anchor = null;
  let scroller;
  const ids = () => rows.rows.map((r) => r.id);
  function focus(id) {
    sel = id;
    b.emit("board.focus", { id });
    if (id == null || !scroller) return;
    tick().then(() => scroller.querySelector(`.board-row[data-id="${CSS.escape(id)}"]`)?.scrollIntoView({ block: "nearest" }));
  }
  const said = () => b.emit("board.selected", { ids: [...checked] });
  function toggle(id) {
    checked = checked.includes(id) ? checked.filter((x) => x !== id) : [...checked, id];
    anchor = id;
    said();
  }
  function down() {
    const all = ids();
    if (!all.length) return;
    const i = sel == null ? -1 : all.indexOf(sel);
    focus(all[Math.min(all.length - 1, i + 1)]);
  }
  function up() {
    const all = ids();
    if (!all.length) return;
    const i = sel == null ? -1 : all.indexOf(sel);
    focus(all[Math.max(0, i - 1)]);
  }
  function open() {
    if (sel != null) b.navigate(base + "ticket/" + sel);
  }
  function check() {
    if (sel != null) toggle(sel);
  }
  function range() {
    if (sel == null) return;
    const all = ids();
    const to = all.indexOf(sel);
    const from = anchor == null || all.indexOf(anchor) < 0 ? to : all.indexOf(anchor);
    const more = all.slice(Math.min(from, to), Math.max(from, to) + 1).filter((x) => !checked.includes(x));
    checked = [...checked, ...more];
    anchor = sel;
    said();
  }
  function clear() {
    checked = [];
    anchor = null;
    focus(null);
    said();
  }
  function clicked(id) {
    toggle(id);
    focus(id);
  }
  b.keys({
    j: { to: down, label: "next row" },
    arrowdown: { to: down, label: "next row" },
    k: { to: up, label: "previous row" },
    arrowup: { to: up, label: "previous row" },
    enter: { to: open, label: "open the selected ticket" },
    x: { to: check, label: "check the row" },
    "shift+x": { to: range, label: "check the range to here" },
    c: { to: () => b.navigate(base + "new"), label: "a new ticket" },
    "/": { to: () => b.emit("filter.focus", {}), label: "the filter line" },
    "g b": { to: () => b.navigate(base), label: "the board" },
    "g r": { to: () => b.navigate("#/ready"), label: "ready" },
    "g q": { to: () => b.navigate("#/queue"), label: "the queue" },
    "g w": { to: () => b.navigate("#/waves"), label: "waves" },
    escape: { to: clear, label: "clear the selection" },
  });

  function scrolled(ev) {
    const top = Math.floor(ev.currentTarget.scrollTop / PITCH);
    const seen = Math.ceil(ev.currentTarget.clientHeight / PITCH) + 1;
    if (top >= first && top + seen <= first + PAGE) return;
    let start = top < first ? top + seen - PAGE : top;
    if (start < 0) start = 0;
    first = start;
    win = filter != null ? { after: null, at: start } : start === 0 ? { after: null, at: null } : { after: afterOf(start), at: null };
  }
</script>

<section class="board" data-board data-surface="board-rows" data-drawn={drawn ? "" : undefined}>
  <header class="board-head">
    <span class="board-count">{total}</span>
    <span class="board-count-label">tickets</span>
    {#if checked.length}<span class="board-checked"><b data-field="checked">{checked.length}</b> checked</span>{/if}
    {#if rows.status !== "live"}<span class="board-status" data-board-status={rows.status}>{rows.status}</span>{/if}
  </header>
  <div class="board-scroll" data-row-height="28" onscroll={scrolled} bind:this={scroller}>
    <div class="board-space" style={"height:" + PITCH * total + "px"}>
      <div class="board-window" style={"transform:translateY(" + PITCH * first + "px)"}>
      {#each rows.rows as r (r.id)}
        <div class="board-line">
        <a class="board-row" data-id={r.id} data-status={r.status} href={base + "ticket/" + r.id}
           data-selected={sel === r.id ? "" : undefined} data-checked={checked.includes(r.id) ? "" : undefined}>
          <i class="kmark" data-kind={r.kind} aria-hidden="true"></i>
          <span class="row-id" data-field="id">{r.id}</span>
          <span class="row-title" data-field="title">{r.title}</span>
          <span class="row-kind" data-field="kind">{r.kind}</span>
          <span class="row-status" data-field="status">{r.status}</span>
        </a>
        <button class="row-check" type="button" data-action="check" data-check={r.id} aria-label={"check " + r.id} onclick={() => clicked(r.id)}></button>
        </div>
      {/each}
      </div>
    </div>
  </div>
</section>

<style>
  .board { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; background: var(--bg); }
  .board-head { display: flex; gap: 6px; align-items: baseline; padding: 7px 14px; color: var(--muted); background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .board-count { color: var(--ink); font-weight: var(--weight-bold); font-variant-numeric: tabular-nums; }
  .board-status { margin-left: auto; font-size: 11px; color: var(--warn); }
  .board-status::before { content: ""; display: inline-block; width: 6px; height: 6px; margin-right: 5px; vertical-align: 1px; border-radius: 50%; background: var(--warn); }
  .board-scroll { flex: 1; min-height: 0; overflow: auto; position: relative; outline: none; }
  .board-scroll:focus-visible, .board-scroll:global([data-sheet~=focus]) { box-shadow: inset 0 0 0 2px var(--focus); }
  .board-space { position: relative; }
  .board-window { position: absolute; left: 0; right: 0; top: 0; will-change: transform; }
  .board-checked { margin-left: 10px; color: var(--accent); }
  .board-checked b { font-weight: var(--weight-bold); }
  .board-line { position: relative; }
  .row-check { position: absolute; z-index: 1; left: 14px; top: 8px; width: 12px; height: 12px; padding: 0; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-xs); cursor: pointer; transition: border-color var(--motion-fast) var(--ease); }
  .row-check:hover, .board-row:hover + .row-check, .row-check:global([data-sheet~=hover]), .board-row:global([data-sheet~=hover]) + .row-check { border-color: var(--muted); }
  .row-check:focus-visible, .row-check:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .board-row[data-checked] + .row-check, .board-row:global([data-sheet~=checked]) + .row-check { background: var(--accent); border-color: var(--accent); }
  .board-row[data-checked] + .row-check::after, .board-row:global([data-sheet~=checked]) + .row-check::after { content: ""; position: absolute; left: 3px; top: 0; width: 3px; height: 7px; border: solid var(--bg); border-width: 0 2px 2px 0; transform: rotate(45deg); }
  .board-row { display: grid; grid-template-columns: 10px 104px minmax(0, 1fr) auto auto; gap: 10px; align-items: center; height: var(--pitch); padding: 0 14px 0 38px; color: var(--ink); text-decoration: none; white-space: nowrap; border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .board-row:hover, .board-row:global([data-sheet~=hover]) { background: var(--hover); }
  .board-row:focus-visible, .board-row:global([data-sheet~=focus]) { outline: none; box-shadow: inset 0 0 0 2px var(--focus); }
  .board-row[data-checked], .board-row:global([data-sheet~=checked]) { background: var(--sel); }
  .board-row[data-selected], .board-row:global([data-sheet~=selected]) { background: var(--row-sel); box-shadow: inset 3px 0 0 var(--accent); }
  .row-id { color: var(--accent); font-variant-numeric: tabular-nums; overflow: hidden; text-overflow: ellipsis; }
  .row-title { overflow: hidden; text-overflow: ellipsis; }
  .row-kind { color: var(--muted); font-size: 11px; }
  .row-status { display: inline-block; padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .board-row[data-status=Draft] .row-status { background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .board-row[data-status=Pending] .row-status { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .board-row[data-status=Ready] .row-status { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .board-row[data-status=Coding] .row-status { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .board-row[data-status="In Review"] .row-status { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .board-row[data-status=Complete] .row-status { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .board-row[data-status=Blocked] .row-status { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .board-row[data-status=Superseded] .row-status { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .board-row[data-status=Canceled] .row-status { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .board-row[data-status=Epic] .row-status { background: var(--st-epic-bg); color: var(--st-epic-fg); }
  .board-row[data-status=Doc] .row-status { background: var(--st-doc-bg); color: var(--st-doc-fg); }
  .kmark { display: inline-block; width: 8px; height: 8px; flex: none; background: var(--kind-build); border-radius: 50%; }
  .kmark[data-kind=scope] { background: var(--kind-scope); border-radius: var(--radius-xs); transform: rotate(45deg) scale(.85); }
  .kmark[data-kind=gap] { background: var(--kind-gap); border-radius: 0; clip-path: polygon(50% 0, 100% 100%, 0 100%); }
  .kmark[data-kind=spike] { background: none; border: 2px solid var(--kind-spike); }
  .kmark[data-kind=execution] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  .kmark[data-kind=doc] { width: 7px; height: 9px; background: none; border: var(--border) solid var(--kind-doc); border-radius: var(--radius-xs); }
  @media (max-width: 720px) {
    .board-row { grid-template-columns: 10px 88px minmax(0, 1fr) auto; }
    .row-kind { display: none; }
  }
  @media (prefers-reduced-motion: reduce) {
    .board-row, .row-check { transition: none; }
  }
</style>
