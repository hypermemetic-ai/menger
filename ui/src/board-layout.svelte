<script>
  // UI-8: the board's layout — the filter line in its own top bar, and three
  // sections, each filled by an assignment (UI-9; ui/slots.json the
  // defaults): the nav, the board, the jump palette. The route gives the
  // filter (`#/` none, `#/f/:filter` the text); the nav and the board get it.
  prop("filter");
</script>

<div class="board-layout" data-board-layout data-filter={filter}>
  <header class="topbar">
    <a class="brand" href="#/">menger</a>
    <filter-line filter={filter}/>
    <span class="hint" data-palette-hint>press / to jump</span>
  </header>
  <div class="board-body">
    <slot name="nav" filter={filter}/>
    <slot name="main" filter={filter}/>
  </div>
  <slot name="palette"/>
</div>
