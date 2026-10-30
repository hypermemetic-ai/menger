<!-- SURFACE-5: the bulk bar (svelte/1) — under the board's rows (SURFACE-2
     places it), and at `#/bulk/:ids` with a selection of its own. It hears
     the rows' selection on the bus (`board.selected { ids }`), or takes a
     filter typed instead, its count `nav.counts { filter }`'s sum (Dry run
     waits for that answer). An action is `update_many` (`--status`,
     `--label`, `--unlabel`, `--kind`, `--severity`) or `archive`, each at
     most `--max`, asked in two steps: the dry run first — the selection's
     count (`update_many` has none: a filter's is asked of `nav.counts` at
     that moment, and past `max` it is refused before anything is sent),
     `archive --dry-run true`'s own answer (the subtree with it) — then the
     write, its answer (`changed`, the tickets as it left them, at most 24
     drawn and the rest counted) or its refusal said. Any change of what is
     asked lets both steps go. -->
<script module>
  export const uses = ["tickets.nav.counts", "tickets.update_many", "tickets.archive"];
  const SHOWN = 24;
  const VALUES = {
    status: ["Ready", "Draft", "Pending", "Coding", "In Review", "Complete", "Superseded", "Canceled"],
    kind: ["build", "gap", "spike", "execution", "scope", "doc"],
    severity: ["High", "Critical", "Medium", "Low", "none"],
  };

  /** A call's answer as FRAME-37 says it, `{ ok, events, message }` — also
   *  from a host before it (the data events, or a rejection), and from none;
   *  a page's `rows` event read as the items it carries. */
  async function answer(call) {
    try {
      const r = await call;
      if (r === undefined || r === null) return { ok: false, events: [], message: "the call answered nothing" };
      const raw = Array.isArray(r) ? r : (r.events ?? []);
      const events = raw.flatMap((e) => (e.type === "rows" ? e.items ?? [] : [e]));
      const error = events.find((e) => e.type === "error");
      const ok = (Array.isArray(r) ? true : r.ok !== false) && !error;
      return { ok, events, message: error?.message ?? (Array.isArray(r) ? null : r.message ?? null) };
    } catch (e) {
      return { ok: false, events: [], message: e?.message ?? String(e) };
    }
  }
</script>

<script>
  import { bus, live } from "plexus-ui/host";

  let { ids = null } = $props();
  const b = bus();

  let sel = $state([]);
  let filter = $state("");
  let action = $state("status");
  let value = $state("Ready");
  let max = $state(500);
  let dry = $state(null);
  let sent = $state(null);

  const counts = live({ snapshot: "tickets.nav.counts", params: () => ({ filter: filter.trim() }), key: "shortcode",
                        on: { count: "upsert:." }, when: () => filter.trim() !== "", debounce_ms: 150 });
  const routeIds = $derived(ids ? String(ids).split(",").filter(Boolean) : []);
  const selIds = $derived(sel.length ? sel : routeIds);
  const byFilter = $derived(filter.trim() !== "");
  const counted = $derived(counts.rows.reduce((a, c) => a + c.held, 0));
  const canDry = $derived(byFilter ? counts.status === "live" : selIds.length > 0);

  function reset() {
    dry = null;
    sent = null;
  }
  b.on("board.selected", (data) => {
    sel = Array.isArray(data?.ids) ? data.ids : [];
    filter = "";
    reset();
  });
  function setAction(ev) {
    action = ev.currentTarget.value;
    value = VALUES[action]?.[0] ?? "";
    reset();
  }
  function setMax(ev) {
    const n = parseInt(ev.currentTarget.value, 10);
    if (n > 0) max = n;
    reset();
  }
  /** What is asked: the ids, or the filter at most `max`. */
  const target = () => (byFilter ? { filter: filter.trim(), max } : { ids: [...selIds], max });

  async function dryRun() {
    if (!canDry) return;
    const asked = { ...target(), action, value };
    if ((action === "label" || action === "unlabel") && !value.trim()) return;
    sent = null;
    dry = { ...asked, pending: true };
    if (action === "archive") {
      const r = await answer(b.call("tickets.archive", { ...target(), dry_run: true }));
      const a = r.events.find((e) => e.type === "archived");
      dry = { ...asked, pending: false, n: a?.ids.length ?? 0, live: a?.live.length ?? 0, refused: r.ok && a ? null : r.message ?? "refused" };
      return;
    }
    let n = selIds.length;
    if (byFilter) {
      const r = await answer(b.call("tickets.nav.counts", { filter: filter.trim() }));
      if (!r.ok) {
        dry = { ...asked, pending: false, n: 0, refused: r.message };
        return;
      }
      n = r.events.filter((e) => e.type === "count").reduce((a, c) => a + c.held, 0);
    }
    dry = { ...asked, pending: false, n, refused: n > max ? `${n} tickets is more than max ${max}: nothing will be sent` : null };
  }

  async function apply() {
    if (!dry || dry.pending || dry.refused) return;
    const { action: a, value: v } = dry;
    const what = dry.filter != null ? { filter: dry.filter, max: dry.max } : { ids: dry.ids, max: dry.max };
    sent = { archive: a === "archive", pending: true };
    if (a === "archive") {
      const r = await answer(b.call("tickets.archive", { ...what, dry_run: false }));
      const got = r.events.find((e) => e.type === "archived");
      sent = { archive: true, pending: false, answer: got ?? null, refused: r.ok && got ? null : r.message ?? "refused" };
      return;
    }
    const params = { ...what, source: "app" };
    if (a === "status") params.status = v;
    if (a === "kind") params.kind = v;
    if (a === "severity") params.severity = v;
    if (a === "label") params.label = [v.trim()];
    if (a === "unlabel") params.unlabel = [v.trim()];
    const r = await answer(b.call("tickets.update_many", params));
    const got = r.events.find((e) => e.type === "updated_many");
    sent = { archive: false, pending: false, answer: got ?? null, refused: r.ok && got ? null : r.message ?? "refused" };
  }
</script>

<div class="bulk" data-surface="bulk-bar">
  <div class="sel">
    {#if byFilter}
      <span class="n" data-bulk-count>{counted}</span><span class="l">match the filter</span>
      {#if counts.status === "pending" || counts.status === "resyncing"}<span class="skeleton" data-loading aria-hidden="true"></span>{/if}
    {:else}
      <span class="n" data-bulk-count>{selIds.length}</span><span class="l">selected</span>
    {/if}
    <input class="filter" data-field="filter" type="search" spellcheck="false" autocomplete="off" aria-label="or a filter"
           placeholder="or a filter — status Draft and in KILN" bind:value={() => filter, (v) => { filter = v; reset(); }}/>
    {#if counts.status === "error" || counts.status === "refused"}<span class="refused" data-refused data-refused-of="filter" role="alert">{counts.message}</span>{/if}
  </div>
  <div class="act">
    <select data-field="action" aria-label="action" value={action} onchange={setAction}>
      <option value="status">set status</option>
      <option value="label">label</option>
      <option value="unlabel">unlabel</option>
      <option value="kind">set kind</option>
      <option value="severity">set severity</option>
      <option value="archive">archive</option>
    </select>
    {#if VALUES[action]}
      <select data-field="value" aria-label={action} value={value} onchange={(ev) => { value = ev.currentTarget.value; reset(); }}>
        {#each VALUES[action] as v (v)}<option value={v}>{v}</option>{/each}
      </select>
    {:else if action !== "archive"}
      <input data-field="value" type="text" spellcheck="false" autocomplete="off" aria-label="label" placeholder="a label"
             bind:value={() => value, (v) => { value = v; reset(); }}/>
    {/if}
    <label class="max">max <input data-field="max" type="number" min="1" value={max} oninput={setMax} onchange={setMax}/></label>
    <button type="button" class="b" data-action="dry-run" disabled={!canDry} title={canDry ? "count what it would take" : "select rows, or type a filter and wait for its count"} onclick={dryRun}>Dry run</button>
  </div>
  {#if dry}
    <div class="dry" data-dry-run>
      {#if dry.pending}
        <span class="skeleton" data-loading aria-hidden="true"></span>
      {:else if dry.refused}
        <span class="refused" data-refused role="alert">{dry.refused}</span>
      {:else if dry.action === "archive"}
        <span data-dry-count={dry.n}>archive would take <b>{dry.n}</b> tickets with their subtrees</span>
        {#if dry.live > 0}<span class="warn" data-dry-live={dry.live}>{dry.live} not done</span>{/if}
        <button type="button" class="b go" data-action="tickets.archive" disabled={sent?.pending} onclick={apply}>Archive {dry.n}</button>
      {:else}
        <span data-dry-count={dry.n}>{dry.action} {dry.value ? "→ " + dry.value : ""} on <b>{dry.n}</b> tickets</span>
        <button type="button" class="b go" data-action="tickets.update_many" disabled={sent?.pending} onclick={apply}>Apply to {dry.n}</button>
      {/if}
      <button type="button" class="b quiet" data-action="cancel" onclick={reset}>Cancel</button>
    </div>
  {/if}
  {#if sent}
    <div class="answer" data-bulk-answer>
      {#if sent.pending}
        <span class="skeleton" data-loading aria-hidden="true"></span>
      {:else if sent.refused}
        <span class="refused" data-refused role="alert">{sent.refused}</span>
      {:else if sent.archive}
        <span class="landed" data-changed={sent.answer.ids.length}>archived {sent.answer.ids.length}</span>
        <ul class="rows">
          {#each sent.answer.ids.slice(0, SHOWN) as id (id)}<li><a class="row" data-id={id} href={"#/ticket/" + id}><span class="row-id">{id}</span></a></li>{/each}
          {#if sent.answer.ids.length > SHOWN}<li class="more" data-more={sent.answer.ids.length - SHOWN}>+{sent.answer.ids.length - SHOWN}</li>{/if}
        </ul>
      {:else}
        <span class="landed" data-changed={sent.answer.ids.length}>changed {sent.answer.ids.length} — {sent.answer.changed.length ? sent.answer.changed.join(", ") : "nothing moved"}</span>
        {#if sent.answer.unchanged.length}<span class="l" data-unchanged={sent.answer.unchanged.length}>{sent.answer.unchanged.length} already so</span>{/if}
        <ul class="rows">
          {#each sent.answer.tickets.filter((t) => sent.answer.ids.includes(t.id)).slice(0, SHOWN) as t (t.id)}
            <li><a class="row" data-id={t.id} data-status={t.status} href={"#/ticket/" + t.id}><span class="row-id">{t.id}</span> <span class="row-status">{t.status}</span></a></li>
          {/each}
          {#if sent.answer.ids.length > SHOWN}<li class="more" data-more={sent.answer.ids.length - SHOWN}>+{sent.answer.ids.length - SHOWN}</li>{/if}
        </ul>
      {/if}
    </div>
  {/if}
</div>

<style>
  .bulk { display: flex; flex-direction: column; gap: 6px; padding: 8px 14px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); background: var(--panel); border-top: var(--border) solid var(--line); }
  .sel, .act, .dry, .answer { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .n { font-weight: var(--weight-bold); font-variant-numeric: tabular-nums; }
  .l { color: var(--muted); }
  .filter { flex: 1; min-width: 180px; max-width: 420px; }
  input, select { padding: 3px 8px; font: inherit; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  input:hover, select:hover { border-color: var(--muted); }
  input:focus-visible, select:focus-visible { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .max { display: inline-flex; gap: 4px; align-items: center; color: var(--muted); }
  .max input { width: 72px; }
  .b { padding: 3px 12px; font: inherit; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .b:hover { background: var(--hover); }
  .b:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .b:disabled { color: var(--disabled); cursor: default; background: var(--card); }
  .b.go { color: var(--accent); background: var(--sel); border-color: var(--accent); }
  .b.go:hover { background: var(--row-sel); }
  .b.quiet { color: var(--muted); }
  .warn, .refused { color: var(--warn); }
  .refused { font-size: 12px; }
  .landed { color: var(--live); }
  .row { display: inline-flex; gap: 6px; align-items: center; padding: 1px 8px; font-size: 12px; color: var(--chip-fg); text-decoration: none; background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  .row:hover { background: var(--hover); }
  .row:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .row-id { color: var(--accent); }
  .row-status { font-size: 11px; color: var(--muted); }
  .more { align-self: center; color: var(--muted); font-size: 12px; }
  .rows { display: flex; flex-wrap: wrap; gap: 6px; list-style: none; margin: 0; padding: 0; }
  .skeleton { display: inline-block; width: 80px; height: 12px; background: var(--skeleton); border-radius: var(--radius-sm); animation: breathe var(--motion-pulse) var(--ease) infinite alternate; }
  @keyframes breathe { from { opacity: 1; } to { opacity: .45; } }
  @media (prefers-reduced-motion: reduce) {
    input, select, .b { transition: none; }
    .skeleton { animation: none; }
  }
</style>
