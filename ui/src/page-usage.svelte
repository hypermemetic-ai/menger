<!-- V2-31: a person's usage page — #/usage (yours), #/usage/:principal (an admin's look at another): the app's top in its `top` section, the palette in its `palette` section (ui/slots.json), quota-usage between. SV-4: real Svelte (svelte/1), page-usage.rhx's port (ui/retired-rhx/). -->
<script module>
  export const uses = [];
</script>

<script>
  import QuotaUsage from "plexus-ui:quota-usage";
  let { principal = null } = $props();
</script>

<section class="page page-usage">
  <div data-slot="top" data-here="usage"></div>
  <header class="page-title"><h1>usage</h1></header>
  <QuotaUsage {principal}/>
  <div data-slot="palette"></div>
</section>

<style>
  .page-usage { min-height: 100vh; min-height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .page-title { max-width: 980px; margin: 0 auto; padding: 16px 24px 0; }
  .page-title h1 { margin: 0; font-family: var(--font-display); font-size: 20px; font-weight: var(--weight-strong); color: var(--ink); }
</style>
