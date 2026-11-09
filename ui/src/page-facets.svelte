<!-- STUDIO-12: a component alone, for its browser cases — a page like every
     route's, placing one svelte/1 component by its bound mark
     (`plexus-ui:nav-facets`) and nothing else (no top: the app's pages
     carry it). #/facets and #/facets/:filter: nav-facets. SV-4: real Svelte (svelte/1),
     page-facets.rhx's port (ui/retired-rhx/ keeps that one). -->
<script module>
  export const uses = [];
</script>

<script>
  import NavFacets from "plexus-ui:nav-facets";
  let { filter = null } = $props();
</script>

<section class="page page-facets">
  <NavFacets filter={filter}/>
</section>

<style>
  .page-facets { color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
</style>
