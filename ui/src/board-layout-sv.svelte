<!-- FRAME-14: board-layout's own part in real Svelte (svelte/1) — the top bar
     and its filter line, filter-line-sv (SURFACE-6: filter-line.rhx's twin,
     a sibling component of its own; the app's name and link are app-top's,
     SURFACE-2). The sections are board-frame's (SV-4: a svelte/1 layout,
     its sections DOM marks the host fills; board-layout.rhx is retired to
     ui/retired-rhx/). -->
<script module>
  export const uses = [];
</script>

<script>
  import FilterLine from "./filter-line-sv.svelte";

  let { filter = null } = $props();
</script>

<header class="topbar">
  <FilterLine {filter}/>
  <span class="hint" data-palette-hint>press / to jump</span>
</header>

<style>
  .topbar { display: flex; align-items: flex-start; gap: 14px; min-height: 46px; padding: 8px max(14px, env(safe-area-inset-left)); background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .hint { margin-left: auto; padding-top: 7px; font-size: 11px; color: var(--muted); white-space: nowrap; }
  @media (max-width: 720px) {
    .topbar { gap: 8px; }
    .hint { display: none; }
  }
</style>
