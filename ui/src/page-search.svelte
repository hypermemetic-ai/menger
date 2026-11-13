<!-- STUDIO-12: the search page, placing search-page (svelte/1: the query's
     two lists, find and search, and its pager) by its bound mark, the app's
     top in its `top` section and the palette in its `palette` section
     (app-top and jump-palette-sv by ui/slots.json), so ctrl+k jumps from
     here too. #/search, #/search/:q and #/search/:q/f/:filter: the query and
     the filter from the route. SV-4: real Svelte (svelte/1), page-search.rhx's
     port (ui/retired-rhx/). ARCHIVE-8: #/search/:q/include/archived (and
     …/include/archived/f/:filter) — `include`, the archive switch. -->
<script module>
  export const uses = [];
</script>

<script>
  import SearchPage from "plexus-ui:search-page";
  let { q = null, filter = null, include = null } = $props();
</script>

<section class="page page-search">
  <div data-slot="top" data-here="search"></div>
  <SearchPage {q} {filter} {include}/>
  <div data-slot="palette"></div>
</section>

<style>
  .page-search { min-height: 100vh; min-height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
</style>
