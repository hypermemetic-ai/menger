<!-- V2-31: everyone's usage and caps (#/quota), an admin's of `*`, svelte/1 —
     `quota.table { sort }`: first the box (`box { used_percent, state }`:
     the data disk against its lines — warned from 70 %, capped writes
     refused from 85 %), then one `usage` per principal, then the `total`.
     A header sorts (stored, the principal, or a source's use, most first);
     a cap is edited in place — a click, a value (2 GB, 300, unlimited, clear),
     enter — through `quota.set { principal, source, cap }`, and the table is
     asked again. Marks: `[data-surface=quota-table]`, `[data-box]`
     (`data-state`), `[data-sort=<key>]` (`aria-sort`), a row `[data-id=<principal>]`
     with `[data-field=stored]` and a cell per source `[data-cell=<source>]`
     holding `[data-field=used]` and `[data-action=quota.set]` (the cap; its
     input `[data-cap-input]`), `[data-total]`, a refusal `[data-refused]`. -->
<script module>
  export const uses = ["quota.table", "quota.set"];
</script>

<script>
  import { bus } from "plexus-ui/host";
  let { sheet = null } = $props();
  const b = bus();

  const COLUMNS = [
    ["media.bytes", "files"], ["docs.bytes", "documents"], ["tickets.bytes", "tickets"],
    ["media.fetch.calls", "fetches / day"], ["media.egress.bytes", "served / day"],
  ];
  let sort = $state("bytes");
  let status = $state("pending");
  let refused = $state("");
  let box = $state(null);
  let rows = $state([]);
  let total = $state(null);
  let editing = $state(null); // { principal, source, text }
  let said = $state("");

  async function load() {
    const r = await b.call("quota.table", { sort });
    if (!r.ok) { status = "refused"; refused = String(r.message ?? "refused"); return; }
    const ev = r.events ?? [];
    const err = ev.find((e) => e.type === "error");
    if (err) { status = "refused"; refused = err.message; return; }
    box = ev.find((e) => e.type === "box") ?? null;
    rows = ev.filter((e) => e.type === "usage");
    total = ev.find((e) => e.type === "total") ?? null;
    status = "live";
  }
  $effect(() => { sort; load(); });

  function amount(unit, n) {
    if (n == null) return "unlimited";
    if (unit !== "bytes") return Number(n).toLocaleString("en");
    const units = ["B", "KB", "MB", "GB", "TB"];
    let v = Number(n), u = 0;
    while (v >= 1000 && u < units.length - 1) { v /= 1000; u += 1; }
    return u === 0 ? `${n} B` : `${(Math.round(v * 10) / 10).toString()} ${units[u]}`;
  }
  const cell = (row, source) => row.sources.find((s) => s.source === source);
  const near = (s) => s.cap != null && s.used + s.held >= s.cap * 0.8;
  const over = (s) => s.cap != null && s.used + s.held >= s.cap;
  const sorted = (key) => (sort === key ? (key === "principal" ? "ascending" : "descending") : "none");
  const short = (p) => (p.length > 26 ? p.slice(0, 14) + "…" + p.slice(-8) : p);

  function edit(row, s) {
    said = "";
    editing = { principal: row.principal, source: s.source, text: s.cap == null ? "unlimited" : amount(s.unit, s.cap) };
  }
  async function save(ev) {
    if (ev.key === "Escape") { editing = null; return; }
    if (ev.key !== "Enter" || !editing) return;
    const { principal, source, text } = editing;
    const r = await b.call("quota.set", { principal, source, cap: text.trim() });
    const ok = r.ok && (r.events ?? []).some((e) => e.type === "allowance");
    if (!ok) { said = String(r.message ?? (r.events ?? []).find((e) => e.message)?.message ?? "refused").replace(/^Invalid params: /, ""); return; }
    editing = null;
    await load();
  }
  const focus = (node) => { node.focus(); node.select(); };
</script>

<section class="quota-table" data-surface="quota-table">
  <div class="page-head">
    <span class="sub">everyone's storage and fetches, against their caps</span>
    {#if status !== "live"}<span class="state" data-loading={status}>{status}</span>{/if}
  </div>
  {#if box}
    <p class="box" data-box="" data-state={box.state}>
      the data disk: <b>{box.used_percent == null ? "unknown" : `${Math.round(box.used_percent)}%`}</b> used
      {#if box.state === "full"} — <strong>full</strong>: writes by anyone capped are refused until there is room (from {box.full_percent}%)
      {:else if box.state === "warn"} — <strong>past {box.warn_percent}%</strong>: capped writes stop at {box.full_percent}%
      {:else} · warned from {box.warn_percent}%, capped writes stop at {box.full_percent}%{/if}
    </p>
  {/if}
  {#if said}<p class="said" data-refused="" role="alert">{said}</p>{/if}
  <div class="panel" role="table">
    <div class="th" role="row">
      <button type="button" data-sort="principal" aria-sort={sorted("principal")} onclick={() => (sort = "principal")}>principal</button>
      <button type="button" data-sort="bytes" aria-sort={sorted("bytes")} onclick={() => (sort = "bytes")}>stored</button>
      {#each COLUMNS as [source, label] (source)}
        <button type="button" data-sort={source} aria-sort={sorted(source)} onclick={() => (sort = source)} title={source}>{label}</button>
      {/each}
    </div>
    {#each rows as row (row.principal)}
      <div class="tr" role="row" data-id={row.principal}>
        <span class="who" title={row.principal}>{short(row.principal)}{#if row.exempt}<span class="chip" data-exempt="">admin</span>{/if}</span>
        <span class="num" data-field="stored">{amount("bytes", row.bytes)}</span>
        {#each COLUMNS as [source] (source)}
          {@const s = cell(row, source)}
          <span class="cell" data-cell={source} data-state={over(s) ? "over" : near(s) ? "near" : "ok"}>
            <span data-field="used">{amount(s.unit, s.used)}</span>
            <span class="of">/</span>
            {#if editing && editing.principal === row.principal && editing.source === source}
              <input data-cap-input="" aria-label={`${source} cap for ${row.principal}`} bind:value={editing.text} onkeydown={save} onblur={() => (editing = null)} use:focus/>
            {:else}
              <button type="button" class="cap" data-action="quota.set" disabled={row.exempt} title={row.exempt ? "an admin of everything is never capped" : `the cap (${s.from}) — click to change`} onclick={() => edit(row, s)}>{amount(s.unit, s.cap)}</button>
            {/if}
          </span>
        {/each}
      </div>
    {/each}
    {#if total}
      <div class="tr total" role="row" data-total="">
        <span class="who">{total.principals} principal{total.principals === 1 ? "" : "s"}</span>
        <span class="num">{amount("bytes", total.bytes)}</span>
        {#each COLUMNS as [source] (source)}
          {@const s = total.sources.find((t) => t.source === source)}
          <span class="cell">{s ? amount(s.unit, s.used) : ""}</span>
        {/each}
      </div>
    {/if}
    {#if sheet === "loading" || status === "pending"}<div class="tr"><span data-loading="">key:…</span></div>{/if}
    {#if status === "live" && rows.length === 0}<p class="empty" data-empty="">No one has stored anything yet.</p>{/if}
  </div>
  {#if status === "refused"}<p class="empty" data-refused="">refused: {refused}</p>{/if}
</section>

<style>
  .quota-table { max-width: 1280px; margin: 0 auto; padding: 18px 24px 40px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .page-head { display: flex; align-items: baseline; gap: 10px; margin: 0 0 10px; }
  .sub { color: var(--muted); font-size: 12px; }
  .state { margin-left: auto; font-size: 11px; color: var(--muted); }
  .box { margin: 0 0 12px; padding: 8px 12px; color: var(--muted); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .box b { color: var(--ink); font-variant-numeric: tabular-nums; }
  .box[data-state=warn] { border-color: var(--warn); }
  .box[data-state=warn] strong, .box[data-state=full] strong { color: var(--warn); }
  .box[data-state=full] { border-color: var(--warn); background: var(--hover); }
  .said { margin: 0 0 10px; color: var(--warn); }
  .panel { overflow-x: auto; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .th, .tr { display: grid; grid-template-columns: minmax(170px, 1.6fr) 90px repeat(5, minmax(120px, 1fr)); gap: 0 12px; align-items: center; padding: 0 10px; min-width: 900px; }
  .th { padding-top: 6px; padding-bottom: 6px; border-bottom: var(--border) solid var(--line); }
  .th button { all: unset; cursor: pointer; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); }
  .th button[aria-sort=descending], .th button[aria-sort=ascending] { color: var(--accent); }
  .th button[aria-sort=descending]::after { content: " ↓"; }
  .th button[aria-sort=ascending]::after { content: " ↑"; }
  .th button:focus-visible, .cap:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; border-radius: var(--radius-sm); }
  .tr { min-height: 36px; padding-top: 4px; padding-bottom: 4px; border-bottom: var(--border) solid var(--line); }
  .tr:last-child { border-bottom: 0; }
  .tr:hover { background: var(--hover); }
  .who { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--font-mono, monospace); font-size: 12px; }
  .chip { margin-left: 6px; padding: 0 6px; font-family: var(--font-text); font-size: 10px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  .num, .cell { font-size: 12px; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .cell[data-state=near] [data-field=used] { color: var(--warn); }
  .cell[data-state=over] [data-field=used] { color: var(--warn); font-weight: var(--weight-strong); }
  .of { margin: 0 3px; color: var(--muted); }
  .cap { all: unset; cursor: pointer; color: var(--accent); border-bottom: 1px dashed var(--line); }
  .cap:disabled { cursor: default; color: var(--muted); border-bottom: 0; }
  .cell input { width: 80px; font: inherit; color: var(--ink); background: var(--bg); border: var(--border) solid var(--accent); border-radius: var(--radius-sm); padding: 1px 4px; }
  .total { font-weight: var(--weight-medium); background: var(--hover); }
  [data-loading] { color: transparent; background: var(--skeleton); border-radius: var(--radius-sm); }
  .empty { margin: 8px 10px; color: var(--muted); }
  [data-refused] { color: var(--warn); }
</style>
