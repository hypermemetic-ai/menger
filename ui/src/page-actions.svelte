<!-- STUDIO-12: a component alone, for its browser cases — a page like every
     route's, placing one svelte/1 component by its bound mark
     (`plexus-ui:ticket-actions`) and nothing else (no top: the app's pages
     carry it). #/actions/:id: ticket-actions. SV-4: real Svelte (svelte/1),
     page-actions.rhx's port (ui/retired-rhx/ keeps that one). -->
<script module>
  export const uses = [];
</script>

<script>
  import TicketActions from "plexus-ui:ticket-actions";
  let { id = null } = $props();
</script>

<section class="page page-actions">
  <TicketActions id={id}/>
</section>

<style>
  .page-actions { color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
</style>
