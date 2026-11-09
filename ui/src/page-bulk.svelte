<!-- STUDIO-12: a component alone, for its browser cases — a page like every
     route's, placing one svelte/1 component by its bound mark
     (`plexus-ui:bulk-bar`) and nothing else (no top: the app's pages
     carry it). #/bulk and #/bulk/:ids (the selection from the route): bulk-bar. SV-4: real Svelte (svelte/1),
     page-bulk.rhx's port (ui/retired-rhx/ keeps that one). -->
<script module>
  export const uses = [];
</script>

<script>
  import BulkBar from "plexus-ui:bulk-bar";
  let { ids = null } = $props();
</script>

<section class="page page-bulk">
  <BulkBar ids={ids}/>
</section>

<style>
  .page-bulk { color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
</style>
