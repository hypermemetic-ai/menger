<!-- V2-68: the whole system's usage, on #/usage for an admin of `*`, svelte/1 —
     `quota.table` once, sorted by bytes stored: the box (`box { used_percent,
     warn_percent, full_percent, state }`, the data disk against its lines),
     the `total { principals, bytes, sources }` — what everyone stores and
     how many, and each source's total used with its unit and window — and
     the first five `usage` rows, the principals storing the most, with a link
     to #/quota for everyone. page-usage places it only for an admin of `*`
     (quota.table's roles held on `*`), so a viewer's page never asks it.
     Marks: `[data-surface=quota-total]`, `[data-box]` (`data-state`),
     `[data-total]` with `[data-field=bytes]` and `[data-field=principals]`,
     a row per source `[data-source=<source>]` (`data-window`) holding
     `[data-field=used]`, the top rows `[data-top=<principal>]` holding
     `[data-field=stored]`, the link `[data-everyone]`, a refusal `[data-refused]`. -->
<script module>
  export const uses = ["quota.table"];
</script>

<script>
  import { bus } from "plexus-ui/host";
  let { sheet = null } = $props();
  const b = bus();

  const TOP = 5;
  let status = $state("pending");
  let refused = $state("");
  let box = $state(null);
  let top = $state([]);
  let total = $state(null);

  async function load() {
    const r = await b.call("quota.table", { sort: "bytes" });
    if (!r.ok) { status = "refused"; refused = String(r.message ?? "refused"); return; }
    const ev = r.events ?? [];
    const err = ev.find((e) => e.type === "error");
    if (err) { status = "refused"; refused = err.message; return; }
    box = ev.find((e) => e.type === "box") ?? null;
    top = ev.filter((e) => e.type === "usage").slice(0, TOP);
    total = ev.find((e) => e.type === "total") ?? null;
    status = "live";
  }
  load();

  const NAMES = {
    "media.bytes": "files kept", "media.fetch.bytes": "fetched today", "media.fetch.calls": "fetches today",
    "media.egress.bytes": "served today", "docs.bytes": "documents", "docs.count": "documents made",
    "tickets.bytes": "tickets", "tickets.count": "tickets made", "questions.count": "questions asked",
  };
  /** An amount in words: bytes as 1.9 GB (powers of 1000, as the caps are set), counts as the number. */
  function amount(unit, n) {
    if (n == null) return "unlimited";
    if (unit !== "bytes") return Number(n).toLocaleString("en");
    const units = ["B", "KB", "MB", "GB", "TB"];
    let v = Number(n), u = 0;
    while (v >= 1000 && u < units.length - 1) { v /= 1000; u += 1; }
    return u === 0 ? `${n} B` : `${(Math.round(v * 10) / 10).toString()} ${units[u]}`;
  }
  const short = (p) => (p.length > 26 ? p.slice(0, 14) + "…" + p.slice(-8) : p);
  const unitOf = (s) => (s.unit === "bytes" ? "bytes" : "a count");
  const windowOf = (s) => (s.window === "day" ? "today (UTC)" : "kept");
</script>

<section class="quota-total" data-surface="quota-total">
  <div class="page-head">
    <h2>whole system</h2>
    <span class="sub">everyone's, as an admin of everything sees it</span>
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
  {#if total}
    <p class="stored" data-total="">
      Everyone stores <b data-field="bytes">{amount("bytes", total.bytes)}</b>
      across <b data-field="principals">{total.principals}</b> principal{total.principals === 1 ? "" : "s"}
    </p>
    <div class="panel">
      {#each total.sources as s (s.source)}
        <div class="tr" data-source={s.source} data-window={s.window}>
          <span class="name">{NAMES[s.source] ?? s.source}<code>{s.source}</code></span>
          <span class="nums"><b data-field="used">{amount(s.unit, s.used)}</b>{#if s.held > 0}<span class="held">+{amount(s.unit, s.held)} in flight</span>{/if}</span>
          <span class="unit">{unitOf(s)} · {windowOf(s)}</span>
        </div>
      {/each}
    </div>
    <div class="top-head">
      <h3>storing the most</h3>
      <a href="#/quota" data-everyone="">everyone →</a>
    </div>
    <div class="panel">
      {#each top as row (row.principal)}
        <div class="tr top" data-top={row.principal}>
          <a class="who" href={`#/usage/${encodeURIComponent(row.principal)}`} title={row.principal}>{short(row.principal)}{#if row.exempt}<span class="chip">admin</span>{/if}</a>
          <span class="nums" data-field="stored">{amount("bytes", row.bytes)}</span>
        </div>
      {/each}
      {#if top.length === 0}<p class="empty" data-empty="">No one has stored anything yet.</p>{/if}
    </div>
  {:else if sheet === "loading" || status === "pending"}
    <div class="panel"><div class="tr"><span data-loading="">media.bytes</span></div></div>
  {/if}
  {#if status === "refused"}<p class="empty" data-refused="">refused: {refused}</p>{/if}
</section>

<style>
  .quota-total { max-width: 980px; margin: 0 auto; padding: 18px 24px 0; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .page-head { display: flex; align-items: baseline; gap: 10px; margin: 0 0 10px; }
  .page-head h2 { margin: 0; font-family: var(--font-display); font-size: 15px; font-weight: var(--weight-strong); }
  .sub { color: var(--muted); font-size: 12px; }
  .state { margin-left: auto; font-size: 11px; color: var(--muted); }
  .box { margin: 0 0 12px; padding: 8px 12px; color: var(--muted); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .box b { color: var(--ink); font-variant-numeric: tabular-nums; }
  .box[data-state=warn] { border-color: var(--warn); }
  .box[data-state=warn] strong, .box[data-state=full] strong { color: var(--warn); }
  .box[data-state=full] { border-color: var(--warn); background: var(--hover); }
  .stored { margin: 0 0 10px; color: var(--muted); }
  .stored b { color: var(--ink); font-variant-numeric: tabular-nums; }
  .panel { overflow: hidden; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .tr { display: grid; grid-template-columns: minmax(150px, 1.4fr) minmax(120px, 1fr) minmax(120px, 1fr); gap: 0 14px; align-items: center; min-height: 36px; padding: 4px 12px; border-bottom: var(--border) solid var(--line); }
  .tr.top { grid-template-columns: 1fr auto; }
  .tr:last-child { border-bottom: 0; }
  .name { display: flex; flex-direction: column; font-weight: var(--weight-medium); }
  .name code { font-size: 11px; font-weight: normal; color: var(--muted); font-family: var(--font-text); }
  .nums { color: var(--muted); font-size: 12px; font-variant-numeric: tabular-nums; }
  .nums b { color: var(--ink); font-weight: var(--weight-medium); }
  .held { margin-left: 0.3em; color: var(--accent); }
  .unit { color: var(--muted); font-size: 11px; text-align: right; }
  .top-head { display: flex; align-items: baseline; gap: 10px; margin: 16px 0 8px; }
  .top-head h3 { margin: 0; font-size: 13px; font-weight: var(--weight-medium); }
  .top-head a { margin-left: auto; font-size: 12px; color: var(--accent); text-decoration: none; }
  .top-head a:hover, .top-head a:focus-visible, .top-head a:global([data-sheet~=hover]), .top-head a:global([data-sheet~=focus]) { text-decoration: underline; }
  .who { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; color: var(--ink); text-decoration: none; }
  .who:hover, .who:focus-visible, .who:global([data-sheet~=hover]), .who:global([data-sheet~=focus]) { color: var(--accent); }
  .chip { margin-left: 6px; padding: 0 6px; font-size: 10px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  [data-loading] { color: transparent; background: var(--skeleton); border-radius: var(--radius-sm); }
  .empty { margin: 8px 10px; color: var(--muted); }
  [data-refused] { color: var(--warn); }
  @media (max-width: 720px) { .tr { grid-template-columns: 1fr auto; } .unit { display: none; } }
</style>
