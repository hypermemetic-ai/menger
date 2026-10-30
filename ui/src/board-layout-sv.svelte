<!-- FRAME-14: board-layout's own part in real Svelte (svelte/1) — the top bar
     and its filter line (filter-line.rhx's). The sections are board-frame's,
     an rhx layout: a svelte/1 module mounts no section. Enter (the field's
     change) moves the route to #/sv/f/<text>; the refused chunk is what
     `nav counts { filter }` says of it, the nav's and the board's own live
     too, so the three share one call. -->
<script module>
  export const uses = ["tickets.nav.counts"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";

  let { filter = null } = $props();
  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  const counts = live({ snapshot: "tickets.nav.counts", params: () => ({ filter: filter ?? null }), key: "shortcode", on: { count: "upsert:." } });

  function apply(ev) {
    const t = ev.currentTarget.value.trim().replaceAll("%", "%25").replaceAll("/", "%2F").replaceAll("#", "%23");
    b.navigate(t === "" ? base : base + "f/" + t);
  }
</script>

<header class="topbar">
  <a class="brand" href={base}>menger</a>
  <div class="filter-line" data-filter-line>
    <input class="filter-input" data-filter-input type="search" spellcheck="false" autocomplete="off"
           placeholder="filter — e.g. status Ready and kind build" aria-label="filter"
           value={filter ?? ""} onchange={apply}/>
    {#if $counts.status === "error" || $counts.status === "refused"}
      <p class="filter-refused" data-filter-refused role="alert">{$counts.message ?? "this filter is not taken"}</p>
    {/if}
  </div>
  <span class="hint" data-palette-hint>press / to jump</span>
</header>

<style>
  .topbar { display: flex; align-items: flex-start; gap: 14px; min-height: 46px; padding: 8px max(14px, env(safe-area-inset-left)); background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .brand { font-family: var(--font-display); font-weight: var(--weight-bold); color: var(--ink); text-decoration: none; padding: 5px 4px 0; border-radius: var(--radius); transition: color var(--motion-fast) var(--ease); }
  .brand:hover { color: var(--accent); }
  .brand:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .hint { margin-left: auto; padding-top: 7px; font-size: 11px; color: var(--muted); white-space: nowrap; }
  @media (max-width: 720px) {
    .topbar { gap: 8px; }
    .hint { display: none; }
  }
  .filter-line { flex: 1; min-width: 0; }
  .filter-input { width: 100%; max-width: 640px; padding: 5px 10px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .filter-input:hover { border-color: var(--muted); }
  .filter-input:focus-visible { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .filter-refused { margin: 4px 0 0; color: var(--warn); font-size: 11px; }
</style>
