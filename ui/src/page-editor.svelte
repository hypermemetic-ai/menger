<!-- EDITOR-4: the first screen — a page like every route's (STUDIO-12),
     placing editor-screen by its bound mark: `#/` for the ticket last opened
     (or a new one), `#/edit/:id` for a given ticket; the app's top in its
     `top` section and the palette in its `palette` section (app-top and
     jump-palette-sv by ui/slots.json). A full-height column: the two halves
     scroll in their own boxes under the bar (EDITOR-8: one surface and the
     documents beside it). The board is at `#/board`. V2-29: `#/doc/:doc`
     opens a document of the docs service on the same surface. V2-42:
     `#/t/:uid/:tail/edit` — a ticket by its uid; editor-screen resolves it.
     SV-4: real Svelte (svelte/1), page-editor.rhx's port (ui/retired-rhx/). -->
<script module>
  export const uses = [];
</script>

<script>
  import EditorScreen from "plexus-ui:editor-screen";
  let { id = null, doc = null, uid = null } = $props();
</script>

<section class="page page-editor">
  <div data-slot="top" data-here="editor"></div>
  <EditorScreen {id} {doc} {uid}/>
  <div data-slot="palette"></div>
</section>

<style>
  .page-editor { display: flex; flex-direction: column; height: 100vh; height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .page-editor > [data-slot=top] { flex: none; }
  .page-editor > :global([data-slot=editor-screen]) { flex: 1; min-height: 0; min-width: 0; display: flex; }
  .page-editor > :global([data-slot=editor-screen]) > :global(*) { flex: 1; min-width: 0; min-height: 0; display: flex; flex-direction: column; }
</style>
