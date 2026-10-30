<!-- FRAME-14: board-layout's own part in real Svelte (svelte/1) — the top bar
     and its filter line, filter-line-sv (SURFACE-6: filter-line.rhx's twin,
     a sibling component of its own). The sections are board-frame's, an rhx
     layout: a svelte/1 module mounts no section. -->
<script module>
  export const uses = [];
</script>

<script>
  import FilterLine from "./filter-line-sv.svelte";

  let { filter = null } = $props();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
</script>

<header class="topbar">
  <a class="brand" href={base}>menger</a>
  <FilterLine {filter}/>
  <span class="hint" data-palette-hint>press / to jump</span>
</header>

<style>
  .topbar { display: flex; align-items: flex-start; gap: 14px; min-height: 46px; padding: 8px max(14px, env(safe-area-inset-left)); background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .brand { font-family: var(--font-display); font-weight: var(--weight-bold); color: var(--ink); text-decoration: none; padding: 5px 4px 0; border-radius: var(--radius); transition: color var(--motion-fast) var(--ease); }
  .brand:hover { color: var(--accent); }
  .brand:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .hint { margin-left: auto; padding-top: 7px; font-size: 11px; color: var(--muted); white-space: nowrap; }
  @media (max-width: 720px) {
    .topbar { gap: 8px; }
    .hint { display: none; }
  }
</style>
