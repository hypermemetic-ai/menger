<!-- LAYERS-20: the kiln at an address of its own, #/kiln — the ticket asks for it there, since it belongs
     to no ticket's body or page section yet. svelte/1. The page only places kiln-live by its bound mark;
     `sheet` is handed on (the sheet draws it with no kiln started). Marks: `[data-surface=page-kiln]`. -->
<script module>
  export const uses = [];
</script>

<script>
  import KilnLive from "plexus-ui:kiln-live";
  let { sheet = null } = $props();
</script>

<section class="page page-kiln" data-surface="page-kiln">
  <KilnLive {sheet}/>
</section>

<style>
  .page-kiln { min-height: 100vh; min-height: 100dvh; background: var(--bg); }
</style>
