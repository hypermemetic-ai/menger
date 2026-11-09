<!-- V2-31: the admin's table of everyone's usage — #/quota: the app's top in its `top` section, the palette in its `palette` section (ui/slots.json), quota-table between. SV-4: real Svelte (svelte/1), page-quota.rhx's port (ui/retired-rhx/). -->
<script module>
  export const uses = [];
</script>

<script>
  import QuotaTable from "plexus-ui:quota-table";
</script>

<section class="page page-quota">
  <div data-slot="top" data-here="quota"></div>
  <header class="page-title"><h1>usage — everyone</h1></header>
  <QuotaTable/>
  <div data-slot="palette"></div>
</section>

<style>
  .page-quota { min-height: 100vh; min-height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .page-title { max-width: 1280px; margin: 0 auto; padding: 16px 24px 0; }
  .page-title h1 { margin: 0; font-family: var(--font-display); font-size: 20px; font-weight: var(--weight-strong); color: var(--ink); }
</style>
