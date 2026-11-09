<!-- STUDIO-12: the waves page, placing its svelte/1 component by its bound
     mark, the app's top in its `top` section and the palette in its
     `palette` section (app-top's jump button and ctrl+k open it) (app-top by
     ui/slots.json); a full-height column, so the list scrolls in its own box
     under its bar and the title sits in the panel above it. #/waves,
     #/waves/in/:epics, #/waves/:id and #/waves/to/:to: work-waves, the mode
     from the route. SV-4: real Svelte (svelte/1), page-waves.rhx's port
     (ui/retired-rhx/). -->
<script module>
  export const uses = [];
</script>

<script>
  import WorkWaves from "plexus-ui:work-waves";
  let { epics = null, id = null, to = null } = $props();
</script>

<section class="page page-waves">
  <div data-slot="top" data-here="waves"></div>
  <header class="page-title"><h1>waves</h1></header>
  <WorkWaves {epics} {id} {to}/>
  <div data-slot="palette"></div>
</section>

<style>
  .page-waves { display: flex; flex-direction: column; height: 100vh; height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .page-waves > [data-slot=top] { flex: none; }
  .page-title { flex: none; padding: 10px 24px 0; background: var(--panel); }
  .page-title h1 { margin: 0; font-family: var(--font-display); font-size: 18px; font-weight: var(--weight-strong); color: var(--ink); }
  .page-waves > :global([data-slot=work-waves]) { flex: 1; min-height: 0; min-width: 0; display: flex; }
  .page-waves > :global([data-slot=work-waves]) > :global(*) { flex: 1; min-width: 0; min-height: 0; display: flex; }
</style>
