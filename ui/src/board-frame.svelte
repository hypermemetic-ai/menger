<!-- FRAME-14: the frame the board is drawn in. SV-4: a layout in real Svelte
     (svelte/1, board-frame.rhx's port; ui/retired-rhx/ keeps that one): its
     sections are DOM marks, `<div data-slot="…">`, which the host fills
     (CONTRACT.md "A layout in any profile"; ui/slots.json the defaults). It
     places its sections and nothing else (tests/apart.rs holds it to that).
     SURFACE-2: the board is `#/board` and `#/f/:filter` (`#/sv/…` and
     `#/rhx/…` kept as aliases); `top` the app's top (app-top), `bar` the top
     bar and its filter line (board-layout-sv), then the nav column — the
     nav, the facets (nav-facets) and the saved filters (filters-saved,
     compact; SURFACE-6) and the board's keymap (keys-help) — the board with
     the bulk bar under it (bulk-bar, SURFACE-5: it hears `board.selected`),
     and the palette. A section is placed only with its default: one with
     none costs the first row ~0.3 s (SURFACE-2's probe). The route gives the
     filter (`#/board` none, `#/f/:filter` the text); every section gets it
     as `data-filter`. FRAME-50: the facets and the saved filters wait for
     `board.drawn` (`after`): a section cannot assume the board beside it is
     mounted yet. SHELL.176-3: the route with no filter is the Recent view —
     `changed within 7d · order changed`, the board's default (owner,
     2026-11-13: "give me a recent tickets page, or filter on the board. make
     that the default board view") — and every section gets that text, so it
     is shown, changed and cleared as any filter is. The All view,
     `#/f/· order code` (every ticket in the store's order), is the board with
     no filter, as `#/board` was: its sections get none. The top (its search
     carries the board's filter) gets only a filter the route names, so a
     search from the default board is not cut to the last seven days. -->
<script module>
  export const uses = [];
  /** SHELL.176-3: the board's filter when the route names none (filter-chips' RECENT). */
  const RECENT = "changed within 7d · order changed";
</script>

<script>
  let { filter: routed = null } = $props();
  const filter = $derived(routed == null ? RECENT : /^\s*·\s*order code\s*$/i.test(routed) ? null : routed);
</script>

<div class="board-layout" data-board-layout data-filter={filter}>
  <div data-slot="top" data-filter={routed == null ? null : filter} data-here="board"></div>
  <div data-slot="bar" data-filter={filter}></div>
  <div class="board-body">
    <div class="board-side">
      <div class="nav" data-slot="nav" data-filter={filter}></div>
      <div data-slot="facets" data-filter={filter} data-after="board.drawn"></div>
      <div data-slot="saved" data-filter={filter} data-compact="true" data-after="board.drawn"></div>
      <div data-slot="keys"></div>
    </div>
    <div class="board-main">
      <div class="main" data-slot="main" data-filter={filter}></div>
      <div data-slot="bulk"></div>
    </div>
  </div>
  <div data-slot="palette"></div>
</div>

<style>
  .board-layout { display: flex; flex-direction: column; height: 100vh; height: 100dvh; background: var(--bg); color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .board-body { flex: 1; min-height: 0; display: flex; }
  .board-side { width: 320px; flex: none; display: flex; flex-direction: column; border-right: var(--border) solid var(--line); overflow: auto; background: var(--panel); }
  .nav { flex: 1 0 auto; }
  .board-main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .main { flex: 1; min-height: 0; min-width: 0; display: flex; }
  .main > :global(*) { flex: 1; min-width: 0; display: flex; }
  .board-layout > [data-slot=top] > :global(*), .board-layout > [data-slot=bar] > :global(*) { display: block; }
  /* On a phone the board is the tree nav: the side column takes the width,
     its nav alone (no facets, saved filters or keymap), and the rows and the
     bulk bar are not drawn; a shortcode or a child opens its ticket. */
  @media (max-width: 720px) {
    .board-side { flex: 1; width: auto; border-right: 0; }
    .board-side > :not(.nav) { display: none; }
    .board-main { display: none; }
  }
</style>
