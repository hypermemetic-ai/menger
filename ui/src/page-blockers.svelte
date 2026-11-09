<!-- STUDIO-12: a component alone, for its browser cases — a page like every
     route's, placing one svelte/1 component by its bound mark
     (`plexus-ui:work-blockers`) and nothing else (no top: the app's pages
     carry it). #/blockers/:id: work-blockers. SV-4: real Svelte (svelte/1),
     page-blockers.rhx's port (ui/retired-rhx/ keeps that one). -->
<script module>
  export const uses = [];
</script>

<script>
  import WorkBlockers from "plexus-ui:work-blockers";
  let { id = null } = $props();
</script>

<section class="page page-blockers">
  <WorkBlockers id={id}/>
</section>

<style>
  .page-blockers { color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
</style>
