<!-- SURFACE-6: the saved filters (FILTER-46), svelte/1 — `#/filters` (and
     `#/filters/:filter`, the board's filter handed in to be saved), and the
     nav's "saved" group as `<filters-saved filter={filter} compact="true"/>`.
     `filters.list` live on `watch`'s `filter_saved` / `filter_deleted` (a
     rename is both: the new row, then the old name gone). A row picked is
     `filters.get` (its filter as the server keeps it, its count), opened on
     the board as `#/f/saved "Name"`, renamed or deleted; the form saves a
     name over the typed filter (the board's when nothing is typed),
     `filters.check` on each 200 ms pause saying ok and the count or the
     refusal's words, Save held until the check's answer is the text's own
     and ok. Every write is `await bus.call(…)`, its answer read as
     `{ ok, events, message }` (FRAME-37; today's bus answers the events
     alone, an in-band `error` among them a refusal), and a refusal is said
     in its words beside the action that was refused; the save's fields stay
     until it is ok. PERF-22, FRAME-50: given
     `after` (a bus topic: the board's frame passes "board.drawn") it asks once
     it is heard; without it, at once. -->
<script module>
  export const uses = ["tickets.filters.list", "tickets.watch", "tickets.filters.get", "tickets.filters.check",
                       "tickets.filters.save", "tickets.filters.rename", "tickets.filters.delete"];

  /** A write's answer as FRAME-37 says it, from whatever the bus answered. */
  export function answer(got) {
    if (got && !Array.isArray(got) && typeof got === "object" && "ok" in got) return got;
    const events = Array.isArray(got) ? got : [];
    const no = events.find((e) => e?.type === "error" || e?.type === "refused" || e?.type === "not_found");
    return { ok: !no, events, message: no ? (no.message ?? no.reason ?? "refused") : undefined };
  }
  export function route(name) {
    const t = `saved "${name}"`.replaceAll("%", "%25").replaceAll("/", "%2F").replaceAll("#", "%23");
    return "#/f/" + t;
  }
</script>

<script>
  import { tick, untrack } from "svelte";
  import { bus, live } from "plexus-ui/host";

  let { filter = null, compact = null, after = null } = $props();
  const b = bus();

  let sel = $state("");
  let name = $state("");
  let text = $state(untrack(() => filter ?? ""));
  let typed = $state(false);
  let to = $state("");
  let refused = $state("");     // the form's refusal
  let rowRefused = $state("");  // the picked row's
  let busy = $state(false);

  // PERF-22, FRAME-50: `after` names a bus topic to hear before the first
  // call — the board's frame passes "board.drawn", the rows being what the
  // person waits for; none (a page of its own, the sheet's tiles): at once.
  // A section cannot assume its siblings are mounted, so nothing is guessed
  // from the page; a remount after the board drew (a hot-swap) finds its
  // mark (`[data-drawn]`) and asks.
  const wait = untrack(() => after);
  let drawn = $state(!wait);
  if (wait) {
    b.on(wait, () => (drawn = true));
    tick().then(() => {
      if (document.querySelector("[data-board][data-drawn]")) drawn = true;
    });
  }
  const saved = live({ snapshot: "tickets.filters.list", params: {}, changes: "tickets.watch", key: "name",
                       on: { filter_saved: "upsert:filter", filter_deleted: "remove:name" }, when: () => drawn });
  // a pause before the get: a row picked by the save that made it is asked once the save has landed
  const one = live({ snapshot: "tickets.filters.get", params: () => ({ name: sel }), key: "name", debounce_ms: 250, when: () => sel !== "" });
  const trimmed = $derived(text.trim());
  const checked = live({ snapshot: "tickets.filters.check", params: () => ({ filter: trimmed }), key: "filter", debounce_ms: 200, when: () => typed && trimmed !== "" });
  // Which text the drawn answer is for (filter-line-sv's rule): the text when
  // the store said it asked, else — a first ask — the text when it answered.
  let asked = null;
  let settled = $state(null);
  $effect(() => {
    const status = checked.status;
    untrack(() => {
      if (status === "pending" || status === "resyncing") asked = typed && trimmed !== "" ? trimmed : null;
      else if (status === "live" || status === "error" || status === "refused") {
        settled = asked ?? trimmed;
        asked = null;
      }
    });
  });
  const fresh = $derived(!typed || trimmed === "" || settled === trimmed);
  const checkOk = $derived(!typed || (fresh && checked.status === "live" && checked.rows.length > 0));
  const checkRefused = $derived(typed && trimmed !== "" && fresh && (checked.status === "error" || checked.status === "refused"));
  const canSave = $derived(name.trim() !== "" && (typed ? trimmed !== "" : (filter ?? "") !== "") && checkOk && !busy);

  async function save() {
    if (!canSave) return;
    const n = name.trim();
    const f = typed ? trimmed : (filter ?? "").trim();
    busy = true;
    refused = "";
    const a = answer(await b.call("tickets.filters.save", { name: n, filter: f }).catch((e) => [{ type: "error", message: e?.message ?? String(e) }]));
    busy = false;
    if (!a.ok) {
      refused = a.message ?? "the save was refused";
      return;
    }
    sel = n;
    name = "";
    text = filter ?? "";
    typed = false;
  }
  function pick(n) {
    sel = sel === n ? "" : n;
    to = "";
    rowRefused = "";
  }
  async function rename() {
    const from = sel;
    const t = to.trim();
    if (t === "" || t === from) return void (rowRefused = "a new name, please");
    if (saved.rows.some((f) => f.name !== from && f.name.toLowerCase() === t.toLowerCase())) return void (rowRefused = "another saved filter is named " + t);
    rowRefused = "";
    const a = answer(await b.call("tickets.filters.rename", { name: from, to: t }).catch((e) => [{ type: "error", message: e?.message ?? String(e) }]));
    if (!a.ok) return void (rowRefused = a.message ?? "the rename was refused");
    sel = t;
    to = "";
  }
  async function remove() {
    const n = sel;
    const a = answer(await b.call("tickets.filters.delete", { name: n }).catch((e) => [{ type: "error", message: e?.message ?? String(e) }]));
    if (!a.ok) return void (rowRefused = a.message ?? "the delete was refused");
    sel = "";
    rowRefused = "";
  }
  const onEnter = (fn) => (ev) => {
    if (ev.key === "Enter") fn();
  };
</script>

<section class="filters-saved" data-surface="filters-saved" data-compact={compact == null ? "false" : "true"}>
  <header class="fs-head">
    <h2>Saved filters</h2>
    <span class="fs-count" data-field="total">{saved.rows.length}</span>
  </header>
  {#if compact == null}
    <div class="fs-form" data-form="tickets.filters.save">
      <label class="fs-field"><span>name</span>
        <input data-field="name" type="text" spellcheck="false" autocomplete="off" placeholder="Open builds"
               bind:value={name} oninput={() => (refused = "")} onkeydown={onEnter(save)}/></label>
      <label class="fs-field grow"><span>filter</span>
        <input data-field="filter" type="text" spellcheck="false" autocomplete="off" placeholder="kind build and not category done"
               bind:value={text} oninput={() => ((typed = true), (refused = ""))} onkeydown={onEnter(save)}/></label>
      <button class={canSave ? "fs-btn primary" : "fs-btn"} type="button" data-action="tickets.filters.save" disabled={!canSave} aria-disabled={!canSave} onclick={save}>Save</button>
      {#if typed && trimmed !== ""}
        {#if checkRefused}
          <p class="fs-check" data-check="refused" data-refused role="status">{checked.message ?? "this filter is not taken"}</p>
        {:else if fresh && checked.status === "live" && checked.rows.length > 0}
          <p class="fs-check" data-check="ok" role="status"><span class="ok">ok</span> · <span data-field="count">{checked.rows[0].count}</span> match · <code>{checked.rows[0].filter}</code></p>
        {:else}
          <p class="fs-check" data-check="pending" data-loading>checking…</p>
        {/if}
      {/if}
      {#if refused !== ""}<p class="fs-check" data-refused role="alert">{refused}</p>{/if}
    </div>
  {/if}
  {#if saved.status === "pending"}
    <ul class="fs-rows" data-loading aria-busy="true"><li class="fs-skel"></li><li class="fs-skel"></li><li class="fs-skel"></li></ul>
  {:else}
    {#if saved.status === "error" || saved.status === "refused"}
      <p class="fs-empty" data-refused>{saved.message ?? "the saved filters are not yours to read"}</p>
    {/if}
    <ul class="fs-rows">
      {#each saved.rows as f (f.name)}
        {@const on = f.name === sel}
        <li class="fs-row" data-id={f.name} data-selected={on ? "" : undefined}>
          <div class="fs-line">
            {#if compact == null}
              <button class="fs-name" type="button" data-pick={f.name} aria-expanded={on ? "true" : "false"} title={f.filter} onclick={() => pick(f.name)}>{f.name}</button>
            {:else}
              <button class="fs-name" type="button" data-open={f.name} title={f.filter} onclick={() => b.navigate(route(f.name))}>{f.name}</button>
            {/if}
            <span class="fs-n" data-field="count">{f.count ?? ""}</span>
            <button class="fs-btn" type="button" data-action="open" data-open={f.name} onclick={() => b.navigate(route(f.name))}>Open</button>
          </div>
          {#if on}
            <div class="fs-detail" data-field="detail">
              {#each one.rows as o (o.name)}
                <p class="fs-text"><code data-field="filter">{o.filter}</code> <span class="fs-meta">{o.count} tickets</span></p>
              {/each}
              {#if (one.status === "error" || one.status === "refused") && one.rows.length === 0}<p class="fs-check" data-refused>{one.message}</p>{/if}
              {#if compact == null}
                <div class="fs-acts">
                  <input class="fs-to" data-field="to" type="text" spellcheck="false" autocomplete="off" placeholder="a new name" aria-label="a new name"
                         bind:value={to} oninput={() => (rowRefused = "")} onkeydown={onEnter(rename)}/>
                  <button class="fs-btn" type="button" data-action="tickets.filters.rename" onclick={rename}>Rename</button>
                  <button class="fs-btn danger" type="button" data-action="tickets.filters.delete" onclick={remove}>Delete</button>
                </div>
                {#if rowRefused !== ""}<p class="fs-check" data-refused role="alert">{rowRefused}</p>{/if}
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
    {#if saved.status === "live" && saved.rows.length === 0}
      <p class="fs-empty" data-empty>No saved filters yet — type one above and give it a name.</p>
    {/if}
  {/if}
</section>

<style>
  .filters-saved { display: block; max-width: 760px; margin: 0 auto; padding: 16px 20px 32px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .filters-saved[data-compact=true] { max-width: none; margin: 0; padding: 8px 4px; }
  .fs-head { display: flex; gap: 8px; align-items: baseline; margin-bottom: 10px; }
  .fs-head h2 { margin: 0; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .fs-count { padding: 0 7px; font-size: 11px; line-height: 18px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); font-variant-numeric: tabular-nums; }
  .fs-form { display: flex; flex-wrap: wrap; gap: 8px; align-items: flex-end; padding: 10px; margin-bottom: 14px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .fs-field { display: flex; flex-direction: column; gap: 3px; font-size: 11px; color: var(--muted); }
  .fs-field.grow { flex: 1; min-width: 220px; }
  .fs-field input, .fs-to { padding: 5px 8px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .fs-field input:hover, .fs-to:hover, .fs-field input:global([data-sheet~=hover]), .fs-to:global([data-sheet~=hover]) { border-color: var(--muted); }
  .fs-field input:focus-visible, .fs-to:focus-visible, .fs-field input:global([data-sheet~=focus]), .fs-to:global([data-sheet~=focus]) { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .fs-btn { padding: 4px 10px; font-size: 12px; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .fs-btn:hover, .fs-btn:global([data-sheet~=hover]) { background: var(--hover); }
  .fs-btn:focus-visible, .fs-btn:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .fs-btn.primary { color: var(--accent); border-color: var(--accent); background: var(--sel); }
  .fs-btn.danger { color: var(--warn); }
  .fs-btn[disabled] { color: var(--disabled); border-color: var(--line); background: var(--card); cursor: not-allowed; }
  .fs-check { flex-basis: 100%; margin: 2px 0 0; font-size: 11px; color: var(--muted); }
  .fs-check[data-refused] { color: var(--warn); }
  .fs-check[data-loading] { color: var(--disabled); }
  .fs-check .ok { color: var(--live); font-weight: var(--weight-medium); }
  .fs-check code, .fs-text code { font-family: var(--font-text); color: var(--ink); }
  .fs-rows { list-style: none; margin: 0; padding: 0; }
  .fs-row { border-bottom: var(--border) solid var(--line); }
  .fs-row[data-selected], .fs-row:global([data-sheet~=selected]) { background: var(--row-sel); box-shadow: inset 2px 0 0 var(--accent); }
  .fs-line { display: grid; grid-template-columns: minmax(0, 1fr) auto auto; gap: 8px; align-items: center; min-height: var(--pitch); padding: 0 8px; transition: background-color var(--motion-fast) var(--ease); }
  .fs-row:not([data-selected]) .fs-line:hover, .fs-row:not([data-selected]) .fs-line:global([data-sheet~=hover]) { background: var(--hover); }
  .fs-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding: 0; color: var(--ink); text-align: left; font: inherit; font-weight: var(--weight-medium); background: none; border: 0; cursor: pointer; }
  .fs-name:hover, .fs-name:global([data-sheet~=hover]) { color: var(--accent); }
  .fs-name:focus-visible, .fs-name:global([data-sheet~=focus]) { outline: 2px solid var(--focus); border-radius: var(--radius-sm); }
  .fs-n:empty { display: none; }
  .fs-n { padding: 0 7px; font-size: 11px; line-height: 18px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); font-variant-numeric: tabular-nums; }
  .fs-detail { padding: 2px 8px 10px; }
  .fs-text { margin: 0 0 8px; overflow-wrap: anywhere; }
  .fs-meta { color: var(--muted); font-size: 11px; }
  .fs-acts { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .fs-to { flex: 1; min-width: 140px; }
  .fs-empty { margin: 10px 8px; color: var(--muted); }
  .fs-empty[data-refused] { color: var(--warn); }
  .fs-skel { height: 14px; margin: 7px 8px; border-radius: var(--radius-sm); background: var(--skeleton); animation: fs-pulse var(--motion-pulse) var(--ease) infinite alternate; }
  .filters-saved[data-compact=true] .fs-line { grid-template-columns: minmax(0, 1fr) auto; }
  .filters-saved[data-compact=true] [data-action=open] { display: none; }
  @keyframes fs-pulse { from { opacity: .55; } to { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) {
    .fs-btn, .fs-line, .fs-field input, .fs-to { transition: none; }
    .fs-skel { animation: none; }
  }
</style>
