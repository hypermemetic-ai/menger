<!-- STUDIO-12: the new page, placing its svelte/1 component by its bound mark, the app's top in its `top` section and the palette in its `palette` section (app-top's jump button and ctrl+k open it) (app-top by ui/slots.json). #/new and #/new/:parent: ticket-create, the parent from the route. SV-4: real Svelte (svelte/1), page-new.rhx's port (ui/retired-rhx/). -->
<script module>
  export const uses = [];
</script>

<script>
  import TicketCreate from "plexus-ui:ticket-create";
  let { parent = null } = $props();
</script>

<section class="page page-new">
  <div data-slot="top" data-here="new"></div>
  <header class="page-title"><h1>New ticket</h1></header>
  <TicketCreate {parent}/>
  <div data-slot="palette"></div>
</section>

<style>
  .page-new { min-height: 100vh; min-height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .page-title { max-width: 760px; margin: 0 auto; padding: 16px 20px 0; }
  .page-title h1 { margin: 0; font-family: var(--font-display); font-size: 22px; font-weight: var(--weight-strong); color: var(--ink); }
</style>
