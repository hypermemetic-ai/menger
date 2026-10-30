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
  import { bus } from "plexus-ui/host";

  let { filter = null } = $props();
  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  const counts = $derived(b.live({ snapshot: "tickets.nav.counts", params: { filter: filter ?? null }, key: "shortcode", on: { count: "upsert:." } }));

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
