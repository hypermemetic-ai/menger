<!-- STUDIO-12: the sheet's page, placing sheet-page (svelte/1: every
     component in every state, both schemes) by its bound mark, the app's top
     in its `top` section and the palette in its `palette` section (app-top's
     jump button and ctrl+k open it) (app-top by ui/slots.json). #/sheet and
     #/sheet/:id: the ticket from the route. The sheet places every component
     but this one, which places it. SV-4: real Svelte (svelte/1),
     page-sheet.rhx's port (ui/retired-rhx/). -->
<script module>
  export const uses = [];
</script>

<script>
  import SheetPage from "plexus-ui:sheet-page";
  let { id = null } = $props();
</script>

<section class="page page-sheet">
  <div data-slot="top" data-here="sheet"></div>
  <SheetPage {id}/>
  <div data-slot="palette"></div>
</section>

<style>
  .page-sheet { min-height: 100vh; min-height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
</style>
