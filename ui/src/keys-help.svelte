<!-- SURFACE-2: the board's keymap at the nav's foot — board-frame's `keys`
     section: the chords board-rows-sv declares (FRAME-36), each with what it
     does; `?` opens the host's overlay of every mounted component's keys,
     which this complements. It hears the rows' selection on the bus
     (`board.selected`, `board.focus`) to say what `enter` and `x` act on now,
     and carries it as `data-selected-ids` / `data-focus`. -->
<script module>
  export const uses = [];
</script>

<script>
  import { bus } from "plexus-ui/host";

  const b = bus();
  let ids = $state([]);
  let focus = $state(null);
  b.on("board.selected", (d) => { ids = d?.ids ?? []; });
  b.on("board.focus", (d) => { focus = d?.id ?? null; });
  const KEYS = [
    ["j k", ["j", "k"], "move"],
    ["enter", ["enter"], "open"],
    ["x", ["x"], "check"],
    ["shift+x", ["shift", "x"], "range"],
    ["c", ["c"], "new"],
    ["/", ["/"], "filter"],
    ["g b", ["g", "b"], "board"],
    ["g r", ["g", "r"], "ready"],
    ["g w", ["g", "w"], "waves"],
    ["ctrl+k", ["ctrl", "k"], "jump"],
    ["escape", ["esc"], "clear"],
    ["?", ["?"], "every key"],
  ];
</script>

<section class="keys-help" data-surface="keys-help" aria-label="keys" data-selected-ids={ids.join(",")} data-focus={focus ?? ""}>
  <h2 class="keys-head">keys{#if ids.length}<span class="keys-now" data-field="checked"> · {ids.length} checked</span>{/if}</h2>
  <ul class="keys-list">
    {#each KEYS as [chord, caps, what] (chord)}
      <li class="keys-row" data-chord={chord}>{#each caps as cap}<kbd class="keys-cap">{cap}</kbd>{/each}<span class="keys-what">{chord === "enter" && focus ? "open " + focus : what}</span></li>
    {/each}
  </ul>
</section>

<style>
  .keys-help { padding: 10px 14px 12px; border-top: var(--border) solid var(--line); background: var(--panel); }
  .keys-head { margin: 0 0 6px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .keys-now { color: var(--accent); text-transform: none; letter-spacing: 0; }
  .keys-list { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: 1fr 1fr; gap: 4px 10px; font-size: 11px; color: var(--muted); }
  .keys-row { display: flex; gap: 3px; align-items: center; min-width: 0; white-space: nowrap; }
  .keys-cap { display: inline-block; min-width: 16px; padding: 0 4px; font: 10px/15px var(--font-text); text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
  .keys-what { margin-left: 3px; overflow: hidden; text-overflow: ellipsis; }
</style>
