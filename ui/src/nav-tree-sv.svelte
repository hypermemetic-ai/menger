<!-- FRAME-14: nav-tree in real Svelte (svelte/1). Tree is v1's tree
     (AGS-12, AGS-18, AGS-20, AGS-23, AGS-25, AGS-29; v1's frontend/src/nav.rs
     at v0.0.1): `nav groups` — a shortcode a row, drawn as its root ticket's
     (nav-group-sv) with its roll-up live on `group_changed` — and any node
     opens, at any depth, to one page of its children (nav-children-sv); a
     sub-shortcode nests under its host ticket, and is at the top only when
     its host's shortcode is not in the reader's view. Rows (nav-row-sv) carry
     the kind's glyph, the id and title, a count and the status's tag, on
     v1's connectors. What is open persists in localStorage (AGS-12: a new
     shortcode opens closed). The tree is a WAI-ARIA tree (AGS-18): Up/Down
     (k/j) move, Right/Left (l/h) open a node or step between it and its
     children, Home/End jump, Enter opens the ticket. Flat is one page of
     `list { filter, summary, limit: 200 }` in store order (nav-flat-sv).
     Under a filter each shortcode says how many match. FRAME-25: the
     siblings are imported from ui/src (FRAME-19), bundled into this module
     and on its bus. -->
<script module>
  export const uses = ["tickets.nav.groups", "tickets.watch", "tickets.nav.counts"];
  const OPEN = "menger.nav.open";
</script>

<script>
  import { setContext } from "svelte";
  import { live } from "plexus-ui/host";
  import NavFlat from "./nav-flat-sv.svelte";
  import NavChildren from "./nav-children-sv.svelte";
  import NavGroup from "./nav-group-sv.svelte";

  let { filter = null } = $props();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  // EDITOR-4: the board itself is #/board (#/ is the editor); under #/sv/ the alias stays.
  const board = base === "#/" ? "#/board" : base;

  const groups = live({ snapshot: "tickets.nav.groups", params: {}, changes: "tickets.watch", key: "shortcode", on: { group_changed: "upsert:group" } });
  const counts = live({ snapshot: "tickets.nav.counts", params: () => ({ filter: filter ?? null }), key: "shortcode", on: { count: "upsert:." } });
  let flat = $state(false);

  const readOpen = () => { try { const v = JSON.parse(localStorage.getItem(OPEN) || "[]"); return Array.isArray(v) ? v.filter((x) => typeof x === "string") : []; } catch { return []; } };
  let open = $state(readOpen());
  const keep = () => { try { localStorage.setItem(OPEN, JSON.stringify(open)); } catch { /* a private window */ } };

  const drawn = (g) => g.total > 0 || g.docs > 0;
  const codeOf = (id) => String(id ?? "").replace(/^.*\//, "").replace(/-\d+(\.\d+)*$/, "");
  const shown = $derived(groups.rows.filter(drawn));
  const codes = $derived(new Set(shown.map((g) => g.epic)));
  // AGS-20: a sub-shortcode nests under its host; at the top when its host's shortcode is out of view.
  const top = $derived(shown.filter((g) => g.host == null || !codes.has(codeOf(g.host))));

  setContext("menger.nav", {
    base, board,
    isOpen: (k) => open.includes(k),
    toggle: (k) => { open = open.includes(k) ? open.filter((x) => x !== k) : [...open, k]; keep(); },
    hosted: (id) => shown.filter((g) => g.host === id && codes.has(codeOf(g.host))),
    held: (sc) => (filter == null ? null : $counts.rows.filter((c) => c.shortcode === sc).reduce((a, c) => a + c.held, 0)),
  });

  // AGS-18: the tree's keys, over the rows that are mounted (a closed node mounts nothing).
  let tree;
  const rows = () => [...tree.querySelectorAll("[data-nav-focus]")];
  const itemOf = (el) => el?.closest("[role=treeitem]");
  const focusAt = (list, i) => list[Math.max(0, Math.min(list.length - 1, i))]?.focus();
  function keys(e) {
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const list = rows();
    const i = list.indexOf(document.activeElement);
    if (i < 0) return;
    const item = itemOf(list[i]);
    const expanded = item?.getAttribute("aria-expanded");
    const toggle = () => item.querySelector("[data-nav-toggle]")?.click();
    switch (e.key) {
      case "ArrowDown": case "j": focusAt(list, i + 1); break;
      case "ArrowUp": case "k": focusAt(list, i - 1); break;
      case "Home": focusAt(list, 0); break;
      case "End": focusAt(list, list.length - 1); break;
      case "ArrowRight": case "l":
        if (expanded === "false") toggle(); else if (expanded === "true") focusAt(list, i + 1);
        break;
      case "ArrowLeft": case "h":
        if (expanded === "true") toggle();
        else itemOf(item?.parentElement)?.querySelector("[data-nav-focus]")?.focus();
        break;
      default: return;
    }
    e.preventDefault();
    e.stopPropagation();
  }
</script>

<nav class="nav-tree" data-nav aria-label="shortcodes">
  <div class="nav-modes" role="group" aria-label="view">
    <button class="nav-mode" data-nav-mode="tree" aria-pressed={flat ? "false" : "true"} onclick={() => (flat = false)}>Tree</button>
    <button class="nav-mode" data-nav-mode="flat" aria-pressed={flat ? "true" : "false"} onclick={() => (flat = true)}>Flat</button>
  </div>
  {#if flat}
    <NavFlat {filter}/>
  {:else}
    <ul class="nav-groups" role="tree" aria-label="tickets" bind:this={tree} onkeydown={keys}>
      {#each top as g (g.shortcode)}
        <NavGroup {g}/>
      {/each}
    </ul>
  {/if}
</nav>

<style>
  .nav-tree { container: nav-tree / inline-size; display: flex; flex-direction: column; min-height: 100%; padding: 8px 0; background: var(--panel); font-size: var(--text-size); }
  .nav-modes { display: flex; gap: 4px; padding: 0 10px 8px; }
  .nav-mode { padding: 1px 9px; font-size: 11px; color: var(--muted); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .nav-mode:hover, .nav-mode:global([data-sheet~=hover]) { color: var(--ink); }
  .nav-mode:focus-visible, .nav-mode:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .nav-mode[aria-pressed=true], .nav-mode:global([data-sheet~=checked]) { color: var(--accent); border-color: var(--accent); background: var(--sel); }
  .nav-groups { list-style: none; margin: 0; padding: 0 6px 0 18px; }
  @media (max-width: 720px) {
    .nav-tree { --pitch: 40px; }
    .nav-mode { padding: 4px 14px; font-size: 13px; }
  }
  @media (prefers-reduced-motion: reduce) {
    .nav-mode { transition: none; }
  }
</style>
