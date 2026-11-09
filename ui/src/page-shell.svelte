<!-- UI-8: the first layout — a header and one section, `main` (hello-list by
     ui/slots.json), at `#/hello`. SV-4: real Svelte (svelte/1),
     page-shell.rhx's port (ui/retired-rhx/): the section a DOM mark the host
     fills. -->
<script module>
  export const uses = [];
</script>

<header class="shell"><span>menger</span></header>
<div data-slot="main"></div>

<style>
  .shell { padding: 8px 14px; background: var(--panel); border-bottom: var(--border) solid var(--line); font-weight: var(--weight-bold); }
</style>
