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
     mounted yet. -->
<script module>
  export const uses = [];
</script>

<script>
  let { filter = null } = $props();
</script>

<div class="board-layout" data-board-layout data-filter={filter}>
  <div data-slot="top" data-filter={filter} data-here="board"></div>
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
  .board-side { width: 260px; flex: none; display: flex; flex-direction: column; border-right: var(--border) solid var(--line); overflow: auto; background: var(--panel); }
  .nav { flex: 1 0 auto; }
  .board-main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .main { flex: 1; min-height: 0; min-width: 0; display: flex; }
  .main > :global(*) { flex: 1; min-width: 0; display: flex; }
  .board-layout > [data-slot=top] > :global(*), .board-layout > [data-slot=bar] > :global(*) { display: block; }
  @media (max-width: 720px) {
    .board-side { display: none; }
  }
</style>
