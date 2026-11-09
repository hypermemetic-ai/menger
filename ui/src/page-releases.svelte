<!-- STUDIO-12: the releases page, placing its svelte/1 component by its bound mark, the app's top in its `top` section and the palette in its `palette` section (app-top by ui/slots.json). #/releases: ui-versions. SV-4: real Svelte (svelte/1), page-releases.rhx's port (ui/retired-rhx/). -->
<script module>
  export const uses = [];
</script>

<script>
  import UiVersions from "plexus-ui:ui-versions";
</script>

<section class="page page-releases">
  <div data-slot="top" data-here="releases"></div>
  <header class="page-title"><h1>releases</h1></header>
  <UiVersions/>
  <div data-slot="palette"></div>
</section>

<style>
  .page-releases { min-height: 100vh; min-height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .page-title { max-width: 1340px; margin: 0 auto; padding: 16px 24px 0; }
  .page-title h1 { margin: 0; font-family: var(--font-display); font-size: 20px; font-weight: var(--weight-strong); color: var(--ink); }
</style>
