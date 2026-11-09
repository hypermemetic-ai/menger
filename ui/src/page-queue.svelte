<!-- V2-20: the queue's page, placing its svelte/1 component by its bound
     mark, the app's top in its `top` section and the palette in its
     `palette` section (app-top by ui/slots.json); a full-height column, so
     the list scrolls in its own box under its head. #/queue: queue-page.
     SV-4: real Svelte (svelte/1), page-queue.rhx's port (ui/retired-rhx/). -->
<script module>
  export const uses = [];
</script>

<script>
  import QueuePage from "plexus-ui:queue-page";
</script>

<section class="page page-queue">
  <div data-slot="top" data-here="queue"></div>
  <QueuePage/>
  <div data-slot="palette"></div>
</section>

<style>
  .page-queue { display: flex; flex-direction: column; height: 100vh; height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .page-queue > [data-slot=top] { flex: none; }
  .page-queue > :global([data-slot=queue-page]) { flex: 1; min-height: 0; min-width: 0; display: flex; }
  .page-queue > :global([data-slot=queue-page]) > :global(*) { flex: 1; min-width: 0; min-height: 0; display: flex; }
</style>
