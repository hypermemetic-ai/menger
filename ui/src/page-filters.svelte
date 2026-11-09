<!-- STUDIO-12: the filters page, placing its svelte/1 component by its bound
     mark, the app's top in its `top` section and the palette in its
     `palette` section (app-top's jump button and ctrl+k open it) (app-top by
     ui/slots.json). #/filters and #/filters/:filter: filters-saved, whole
     (the board's nav holds it compact); its own heading is its. SV-4: real
     Svelte (svelte/1), page-filters.rhx's port (ui/retired-rhx/). -->
<script module>
  export const uses = [];
</script>

<script>
  import FiltersSaved from "plexus-ui:filters-saved";
  let { filter = null } = $props();
</script>

<section class="page page-filters">
  <div data-slot="top" data-here="filters"></div>
  <FiltersSaved {filter}/>
  <div data-slot="palette"></div>
</section>

<style>
  .page-filters { min-height: 100vh; min-height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
</style>
