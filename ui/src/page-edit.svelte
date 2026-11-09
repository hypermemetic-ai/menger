<!-- STUDIO-12: a component alone, for its browser cases — a page like every
     route's, placing one svelte/1 component by its bound mark
     (`plexus-ui:ticket-edit`) and nothing else (no top: the app's pages
     carry it). #/form/:id: ticket-edit. SV-4: real Svelte (svelte/1),
     page-edit.rhx's port (ui/retired-rhx/ keeps that one). -->
<script module>
  export const uses = [];
</script>

<script>
  import TicketEdit from "plexus-ui:ticket-edit";
  let { id = null } = $props();
</script>

<section class="page page-edit">
  <TicketEdit id={id}/>
</section>

<style>
  .page-edit { color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
</style>
