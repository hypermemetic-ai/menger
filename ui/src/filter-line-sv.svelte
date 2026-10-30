<!-- SURFACE-6: the filter line in real Svelte (svelte/1), filter-line.rhx's
     twin, drawn by board-layout-sv's top bar. One field in FILTER-3's
     language, bound to its text: Enter moves the route to `#/sv/f/<text>`
     (`#/f/` off the Svelte board); the refused chunk of the applied filter is
     what `nav counts { filter }` says of it (`[data-filter-refused]`); what
     is typed is checked on each pause (`filters.check`, 200 ms of quiet,
     the host's registry keeping the time; nothing asked before a key) and
     said under the line — `ok · N match`, or the refusal's own words, and
     "checking…" until the answer is the text's own (an answer is the text's
     when the ask the store said it sent was for that text); an open
     `saved "…` completes from the saved filters (`filters.list`, asked only
     then), the field keeping the focus; Escape (FRAME-36's keys, heard in
     the field) clears the line and the route's filter; `filter.focus` on the
     bus (the board's `/`) focuses it. -->
<script module>
  export const uses = ["tickets.nav.counts", "tickets.filters.check", "tickets.filters.list"];
  const OPEN = 'saved "';

  /** The name being typed after the last `saved "` with no closing quote, else null. */
  export function opened(t) {
    const at = t.lastIndexOf(OPEN);
    if (at < 0) return null;
    const rest = t.slice(at + OPEN.length);
    return rest.includes('"') ? null : rest;
  }
  export function completed(t, name) {
    return t.slice(0, t.lastIndexOf(OPEN)) + OPEN + name + '"';
  }
</script>

<script>
  import { untrack } from "svelte";
  import { bus, live } from "plexus-ui/host";

  let { filter = null } = $props();
  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  let q = $state(untrack(() => filter ?? ""));
  let typed = $state(false);
  let field = $state(null);
  // a new route's filter is the line's again
  $effect(() => {
    const f = filter ?? "";
    untrack(() => {
        q = f;
        typed = false;
    });
  });

  const counts = live({ snapshot: "tickets.nav.counts", params: () => ({ filter: filter ?? null }), key: "shortcode", on: { count: "upsert:." } });
  const text = $derived(q.trim());
  const checked = live({ snapshot: "tickets.filters.check", params: () => ({ filter: text }), key: "filter", debounce_ms: 200, when: () => typed && text !== "" });
  // Which text the drawn answer is for: the text when the store said it
  // asked (pending, resyncing), else — a first ask, pending all along — the
  // text when it answered. During the pause nothing is asked, so the last
  // answer stays drawn; it is shown only when it is the text's own.
  let asked = null;
  let settled = $state(null);
  $effect(() => {
    const status = checked.status;
    untrack(() => {
      if (status === "pending" || status === "resyncing") asked = typed && text !== "" ? text : null;
      else if (status === "live" || status === "error" || status === "refused") {
        settled = asked ?? text;
        asked = null;
      }
    });
  });
  const fresh = $derived(typed && text !== "" && settled === text);
  const partial = $derived(opened(q));
  const saved = live({ snapshot: "tickets.filters.list", params: {}, key: "name", when: () => partial !== null });
  const offers = $derived(partial === null ? [] : saved.rows.filter((s) => s.name.toLowerCase().startsWith(partial.toLowerCase())));

  b.keys({ escape: { to: clear, in_inputs: true, label: "clear the filter line" } });
  // SURFACE-2's board-rows-sv says `filter.focus` on `/`: the line takes the focus.
  b.on("filter.focus", () => field?.focus());

  function clear(ev) {
    if (ev?.target?.data?.["filter-input"] === undefined) return;
    q = "";
    typed = false;
    if (filter != null) b.navigate(base);
  }
  function keyed(ev) {
    if (ev.key !== "Enter") return;
    const t = text.replaceAll("%", "%25").replaceAll("/", "%2F").replaceAll("#", "%23");
    b.navigate(t === "" ? base : base + "f/" + t);
  }
  function complete(name) {
    q = completed(q, name);
    typed = true;
    field?.focus();
  }
</script>

<div class="filter-line" data-filter-line data-surface="filter-line">
  <input class="filter-input" data-filter-input type="search" spellcheck="false" autocomplete="off"
         placeholder="filter — e.g. status Ready and kind build" aria-label="filter"
         bind:this={field} bind:value={q} oninput={() => (typed = true)} onkeydown={keyed}/>
  {#if partial !== null && offers.length > 0}
    <ul class="filter-complete" data-complete-list role="listbox" aria-label="saved filters">
      {#each offers as s (s.name)}
        <li><button class="filter-complete-row" type="button" data-complete={s.name} onmousedown={(ev) => ev.preventDefault()} onclick={() => complete(s.name)}><span class="kw">saved</span> "{s.name}"<span class="filter-complete-count">{s.count ?? ""}</span></button></li>
      {/each}
    </ul>
  {/if}
  {#if typed && text !== ""}
    {#if fresh && (checked.status === "error" || checked.status === "refused")}
      <p class="filter-check" data-filter-check="refused" data-refused role="status">{checked.message ?? "this filter is not taken"}</p>
    {:else if fresh && checked.status === "live" && checked.rows.length > 0}
      <p class="filter-check" data-filter-check="ok" role="status"><span class="ok">ok</span> · <span data-field="count">{checked.rows[0].count}</span> match</p>
    {:else}
      <p class="filter-check" data-filter-check="pending" data-loading>checking…</p>
    {/if}
  {/if}
  {#if counts.status === "error" || counts.status === "refused"}
    <p class="filter-refused" data-filter-refused data-refused role="alert">{counts.message ?? "this filter is not taken"}</p>
  {/if}
</div>

<style>
  .filter-line { display: block; flex: 1; min-width: 0; position: relative; }
  .filter-input { width: 100%; max-width: 640px; padding: 5px 10px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .filter-input:hover { border-color: var(--muted); }
  .filter-input:focus-visible { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .filter-refused { margin: 4px 0 0; color: var(--warn); font-size: 11px; }
  .filter-check { margin: 4px 0 0; font-size: 11px; color: var(--muted); }
  .filter-check[data-refused] { color: var(--warn); }
  .filter-check[data-loading] { color: var(--disabled); }
  .filter-check .ok { color: var(--live); font-weight: var(--weight-medium); }
  .filter-check [data-field=count] { color: var(--ink); font-variant-numeric: tabular-nums; }
  .filter-complete { position: absolute; z-index: 5; top: 34px; left: 0; width: min(420px, 100%); max-height: 240px; overflow: auto; list-style: none; margin: 0; padding: 4px; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-lg); box-shadow: 0 6px 18px var(--shadow); }
  .filter-complete-row { display: flex; gap: 6px; align-items: center; width: 100%; height: var(--pitch); padding: 0 8px; color: var(--ink); text-align: left; background: none; border: 0; border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .filter-complete-row:hover { background: var(--hover); }
  .filter-complete-row:focus-visible { outline: none; background: var(--row-sel); box-shadow: inset 0 0 0 2px var(--focus); }
  .filter-complete-row .kw { color: var(--accent); }
  .filter-complete-count { margin-left: auto; padding: 0 7px; font-size: 11px; line-height: 18px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); font-variant-numeric: tabular-nums; }
  .filter-complete-count:empty { display: none; }
  @media (prefers-reduced-motion: reduce) {
    .filter-input, .filter-complete-row { transition: none; }
  }
</style>
