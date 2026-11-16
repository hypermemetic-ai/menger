<!-- CODE-3: code-ide's side — the finder's box over the tree. Bundled into code-ide; a component in its own
     right too (the sheet draws it alone).

     - **The tree** is `tree.kids` (a directory's path, "" the root → its entries, directories first, each
       `{ path, name, kind, bytes }`), made by code-ide from one `code.tree` answer; a directory's rows are
       made only when it is open (`open`, its paths). Each row is a button: a directory toggles
       (`ontoggle`), a file opens (`onopen`); a file under the pointer is said (`onhover(path)`, then
       `onhover(null)`) so code-ide reads it ahead. A row whose file is shown carries `aria-current`; a file
       with a tab is drawn in ink.
     - **The finder**: what is typed (`query`, `onquery`) shows `matches` (code-ide's, best first) in the
       tree's place; Enter opens the first (`onfind`), Escape leaves it (`onleave`).
     - `crumb` says where it starts, or what the tag named that the tree lacks; `reveal` (a directory's
       path, and a count that moves) scrolls that row into this list's view — this list alone, never the page.
     Marks: `[data-surface=code-ide-tree]`, `input[data-finder]`, `[data-dir=<path>][aria-expanded]`,
     `[data-file=<path>]` (`aria-current` when shown, `data-open` with a tab), `[data-match=<path>]`,
     `[data-field=crumb]`, `[data-empty]`. -->
<script module>
  export const uses = [];
</script>

<script>
  import { tick } from "svelte";

  let {
    tree: treeProp = null, open: openProp = [], shown = "", tabs: tabsProp = [], query = "", matches: matchesProp = [], crumb = "", reveal: revealProp = null, sheet = null,
    ontoggle = null, onopen = null, onhover = null, onquery = null, onfind = null, onleave = null, finder = $bindable(null),
  } = $props();
  /** A prop as a value: a bound mark hands every prop as a string, an object as its JSON (the sheet's tiles). */
  const valueOf = (v, none) => { if (typeof v !== "string") return v ?? none; try { return JSON.parse(v); } catch { return none; } };
  const tree = $derived(valueOf(treeProp, null));
  const matches = $derived(valueOf(matchesProp, []));
  const reveal = $derived(valueOf(revealProp, null));
  const openSet = $derived(new Set(valueOf(openProp, [])));
  const tabSet = $derived(new Set(valueOf(tabsProp, [])));
  let list = $state(null);

  function keyed(ev) {
    if (ev.key === "Enter") { ev.preventDefault(); onfind?.(); }
    else if (ev.key === "Escape") { ev.preventDefault(); onleave?.(); }
    else if (ev.key === "ArrowDown") { const first = list?.querySelector("[data-match]"); if (first) { ev.preventDefault(); first.focus(); } }
  }

  // A directory the tag named, scrolled into this list's view (the page never moves for it).
  $effect(() => {
    const r = reveal;
    if (!r?.dir || !list) return;
    tick().then(() => {
      const row = list?.querySelector(`[data-dir="${CSS.escape(r.dir)}"]`);
      if (row) list.scrollTop = Math.max(0, row.offsetTop - list.offsetTop - 4);
    });
  });
  const pad = (depth) => `padding-left: ${8 + depth * 12}px`;
</script>

{#snippet level(dir, depth)}
  {#each tree?.kids?.[dir] ?? [] as e (e.path)}
    {#if e.kind === "dir" || e.kind === "module"}
      <button type="button" class="row dir" style={pad(depth)} data-dir={e.path} aria-expanded={openSet.has(e.path) ? "true" : "false"} onclick={() => ontoggle?.(e.path)}>
        <span class="tw" aria-hidden="true">{openSet.has(e.path) ? "▾" : "▸"}</span><span class="nm">{e.name}/</span>{#if e.kind === "module"}<span class="note">submodule</span>{/if}
      </button>
      {#if openSet.has(e.path)}{@render level(e.path, depth + 1)}{/if}
    {:else}
      <button type="button" class="row file" class:has-tab={tabSet.has(e.path)} style={pad(depth)} data-file={e.path} data-open={tabSet.has(e.path) ? "" : undefined}
        aria-current={shown === e.path ? "true" : undefined}
        onclick={() => onopen?.(e.path)} onpointerenter={() => onhover?.(e.path)} onpointerleave={() => onhover?.(null)}>
        <span class="tw" aria-hidden="true"></span><span class="nm">{e.name}</span>{#if e.kind === "link"}<span class="note">→ {e.target}</span>{/if}
      </button>
    {/if}
  {/each}
{/snippet}

<div class="cit" data-surface="code-ide-tree" data-state={sheet === "loading" ? "loading" : tree ? "ready" : "pending"}>
  <div class="find">
    <input type="search" data-finder="" placeholder="find a file (Ctrl P)" aria-label="find a file" autocomplete="off" spellcheck="false"
      value={query} bind:this={finder} oninput={(ev) => onquery?.(ev.currentTarget.value)} onkeydown={keyed} disabled={!tree} />
  </div>
  <div class="list" bind:this={list} role="group" aria-label="files">
    {#if query}
      <div class="crumb" data-field="crumb">{matches.length ? "matches in the whole repository · Enter opens the first" : ""}</div>
      {#each matches as p, i (p)}
        <button type="button" class="row file match" class:first={i === 0} data-match={p} data-file={p} aria-current={shown === p ? "true" : undefined}
          onclick={() => onopen?.(p)} onpointerenter={() => onhover?.(p)} onpointerleave={() => onhover?.(null)}
          onkeydown={(ev) => { if (ev.key === "Escape") { ev.preventDefault(); onleave?.(); } }}>
          <span class="nm">{p.split("/").pop()}</span><span class="dirname">{p.includes("/") ? p.slice(0, p.lastIndexOf("/")) : ""}</span>
        </button>
      {:else}
        <div class="empty" data-empty="">no file matches “{query}”</div>
      {/each}
    {:else if sheet === "loading"}
      {#each [0, 1, 2, 3, 4] as i (i)}<div class="row skel" data-loading="" style={pad(i % 2)}>&nbsp;</div>{/each}
    {:else if !tree}
      <div class="empty" data-empty="">no tree to show</div>
    {:else}
      {#if crumb}<div class="crumb" data-field="crumb">{crumb}</div>{/if}
      {@render level("", 0)}
      {#if tree.truncated}<div class="empty">the tree is cut at 20,000 entries</div>{/if}
    {/if}
  </div>
</div>

<style>
  .cit { display: grid; grid-template-rows: auto 1fr; min-height: 0; height: 100%; background: var(--panel); font-family: var(--font-text); font-size: 12.5px; }
  .find { padding: 6px; }
  .find input { box-sizing: border-box; width: 100%; min-width: 0; padding: 1px 6px; font: inherit; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .find input:focus-visible, .find input:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .find input:disabled, .find input:global([data-sheet~=disabled]) { color: var(--disabled); }
  .list { overflow: auto; min-height: 0; padding: 0 0 8px; }
  .crumb { padding: 2px 8px 4px; font-size: 11px; color: var(--muted); overflow-wrap: anywhere; }
  .row { display: flex; gap: 5px; align-items: center; box-sizing: border-box; width: 100%; padding: 1px 8px; font: inherit; line-height: 1.5; text-align: left; white-space: nowrap; color: var(--ink); background: none; border: 0; border-left: 2px solid transparent; border-radius: 0; cursor: pointer; }
  .row:hover, .row:global([data-sheet~=hover]) { background: var(--hover); }
  .row:focus-visible, .row:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: -2px; }
  .row.file { color: var(--muted); }
  .row.file.has-tab { color: var(--ink); }
  .row[aria-current=true] { color: var(--ink); background: var(--row-sel); border-left-color: var(--accent); }
  .tw { flex: none; width: 10px; color: var(--muted); }
  .nm { overflow: hidden; text-overflow: ellipsis; }
  .note, .dirname { color: var(--muted); font-size: 11px; overflow: hidden; text-overflow: ellipsis; }
  .row.match { padding-left: 10px; }
  .match.first { background: var(--sel); }
  .empty { padding: 10px 8px; color: var(--muted); }
  .skel { margin: 3px 8px; width: auto; height: 14px; padding: 0; color: transparent; background: var(--skeleton); border-radius: var(--radius-sm); cursor: default; }
</style>
