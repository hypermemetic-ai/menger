<!-- CODE-3: code-ide's tabs — one per open file, in order, the shown one raised. Bundled into code-ide; a
     component in its own right too (the sheet draws it alone). A tab shows on a click, Enter or Space
     (`onshow`); it closes by its × (a button of its own), by a middle click, or by Delete while it has the
     focus (`onclose`) — code-ide shows its neighbour. A tab whose file the tree lacks at this commit is drawn
     muted (`gone`). Marks: `[data-surface=code-ide-tabs]`, `[data-tab=<path>][aria-selected]`,
     `[data-close=<path>]`. -->
<script module>
  export const uses = [];
</script>

<script>
  let { tabs: tabsProp = [], shown = "", gone: goneProp = [], onshow = null, onclose = null } = $props();
  /** A prop as a value: a bound mark hands every prop as a string, an object as its JSON (the sheet's tiles). */
  const valueOf = (v, none) => { if (typeof v !== "string") return v ?? none; try { return JSON.parse(v); } catch { return none; } };
  const tabs = $derived(valueOf(tabsProp, []));
  const goneSet = $derived(new Set(valueOf(goneProp, [])));
  const nameOf = (p) => p.split("/").pop();
  // Two tabs of one name: each says its directory too.
  const twice = $derived.by(() => {
    const seen = new Map();
    for (const t of tabs) seen.set(nameOf(t.path), (seen.get(nameOf(t.path)) ?? 0) + 1);
    return seen;
  });
  function key(ev, path) {
    if (ev.target !== ev.currentTarget) return;
    if (ev.key === "Enter" || ev.key === " ") { ev.preventDefault(); onshow?.(path); }
    else if (ev.key === "Delete") { ev.preventDefault(); onclose?.(path); }
  }
</script>

<div class="cit-tabs" data-surface="code-ide-tabs" role="tablist" aria-label="open files">
  {#each tabs as t (t.path)}
    <div class="tab" class:gone={goneSet.has(t.path)} role="tab" tabindex="0" data-tab={t.path} title={t.path} aria-selected={t.path === shown ? "true" : "false"}
      onclick={() => onshow?.(t.path)} onauxclick={(ev) => { if (ev.button === 1) { ev.preventDefault(); onclose?.(t.path); } }}
      onmousedown={(ev) => { if (ev.button === 1) ev.preventDefault(); }} onkeydown={(ev) => key(ev, t.path)}>
      <span class="nm">{nameOf(t.path)}</span>{#if (twice.get(nameOf(t.path)) ?? 0) > 1}<span class="dir">{t.path.slice(0, Math.max(0, t.path.lastIndexOf("/")))}</span>{/if}
      <button type="button" class="x" data-close={t.path} aria-label={`close ${t.path}`} onclick={(ev) => { ev.stopPropagation(); onclose?.(t.path); }}>×</button>
    </div>
  {/each}
</div>

<style>
  .cit-tabs { display: flex; min-width: 0; overflow-x: auto; background: var(--bg); border-bottom: var(--border) solid var(--line); font-family: var(--font-text); font-size: 12.5px; scrollbar-width: thin; }
  .tab { display: flex; flex: none; gap: 6px; align-items: center; padding: 3px 4px 3px 10px; white-space: nowrap; color: var(--muted); border-right: var(--border) solid var(--line); cursor: pointer; }
  .tab:hover, .tab:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }
  .tab:focus-visible, .tab:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: -2px; }
  .tab[aria-selected=true], .tab:global([data-sheet~=selected]) { color: var(--ink); background: var(--panel); box-shadow: inset 0 2px 0 var(--accent); }
  .tab.gone .nm { text-decoration: line-through; }
  .dir { color: var(--muted); font-size: 11px; }
  .x { padding: 0 4px; font: inherit; line-height: 1.2; color: var(--muted); background: none; border: 0; border-radius: var(--radius-sm); cursor: pointer; }
  .x:hover, .x:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }
  .x:focus-visible, .x:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
</style>
