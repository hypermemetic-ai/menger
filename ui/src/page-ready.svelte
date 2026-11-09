<!-- STUDIO-12: the ready page, placing its svelte/1 component by its bound
     mark, the app's top in its `top` section and the palette in its
     `palette` section (app-top's jump button and ctrl+k open it) (app-top by
     ui/slots.json); a full-height column, so the list scrolls in its own box
     under its bar and the title sits in the panel above it. #/ready and
     #/ready/:filter: work-ready, under the route's filter. SV-4: real Svelte
     (svelte/1), page-ready.rhx's port (ui/retired-rhx/). -->
<script module>
  export const uses = [];
</script>

<script>
  import WorkReady from "plexus-ui:work-ready";
  let { filter = null } = $props();
</script>

<section class="page page-ready">
  <div data-slot="top" data-here="ready"></div>
  <header class="page-title"><h1>ready</h1></header>
  <WorkReady {filter}/>
  <div data-slot="palette"></div>
</section>

<style>
  .page-ready { display: flex; flex-direction: column; height: 100vh; height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .page-ready > [data-slot=top] { flex: none; }
  .page-title { flex: none; padding: 8px 14px 0; background: var(--panel); }
  .page-title h1 { margin: 0; font-family: var(--font-display); font-size: 16px; font-weight: var(--weight-strong); color: var(--ink); }
  .page-ready > :global([data-slot=work-ready]) { flex: 1; min-height: 0; min-width: 0; display: flex; }
  .page-ready > :global([data-slot=work-ready]) > :global(*) { flex: 1; min-width: 0; min-height: 0; display: flex; }
</style>
