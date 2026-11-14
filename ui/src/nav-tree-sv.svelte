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
     and on its bus.

     SHELL.176-4 (the owner, 2026-11-14: "I want the nav to always show the
     ticket i'm open on"): `current`, the ticket open beside the nav (the
     ticket page's `data-current` on its section). The nav walks its
     ancestors (`get`, parent by parent), opens each one's node — a ticket
     that roots a shortcode is that shortcode's row — and the shortcode it is
     in, adding to what is open and taking nothing away; a node on the way
     pages until the child it leads to is drawn (nav-children-sv,
     `want`). The open ticket's row is marked (`[data-nav-current]`,
     `aria-current="page"`) in Tree and in Flat, and scrolled into the nav's
     column once per ticket and mode, a third of the way down — never when
     the person has wheeled, touched, clicked or pressed a key in the nav
     since, never when it is in view already. When the nav has drawn nothing
     new for 1.5 s and the ticket's row is not among its rows, it is pinned
     above them (`[data-nav-pinned]`), marked, saying why
     (`[data-nav-pinned-why]`): archived, outside the filter (Flat, asked by
     `nav member`), past the first 200 (Flat's one page), or not in the
     tree. -->
<script module>
  export const uses = ["tickets.nav.groups", "tickets.watch", "tickets.nav.counts", "tickets.get", "tickets.nav.member"];
  const OPEN = "menger.nav.open";
  /** SHELL.176-4: how long the nav must draw nothing new before a missing row is pinned. */
  const SETTLE_MS = 1500;
  /** The nearest ancestor that scrolls (`full`: and is taller than its box now). */
  function scroller(el, full = true) {
    for (let p = el.parentElement; p; p = p.parentElement) {
      const o = getComputedStyle(p).overflowY;
      if ((o === "auto" || o === "scroll") && (!full || p.scrollHeight > p.clientHeight)) return p;
    }
    return null;
  }
  const MOVES = ["wheel", "touchstart", "pointerdown", "keydown"];
</script>

<script>
  import { setContext, untrack } from "svelte";
  import { bus, live } from "plexus-ui/host";
  import { ticketHref } from "./ticket-links.svelte";
  import Row from "./nav-row-sv.svelte";
  import NavFlat from "./nav-flat-sv.svelte";
  import NavChildren from "./nav-children-sv.svelte";
  import NavGroup from "./nav-group-sv.svelte";

  let { filter = null, current = null } = $props();
  const b = bus();
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

  // SHELL.176-4: the open ticket, its ancestors (up[0] the ticket, then its parent, …) and the way to it.
  let up = $state([]);
  let wants = $state({});
  let pin = $state(null);
  const here = $derived(up[0]?.id ?? current);
  const groupsIn = $derived(groups.status === "live" || groups.rows.length > 0);

  setContext("menger.nav", {
    base, board,
    current: () => here,
    want: (node) => (node == null ? null : wants[node] ?? null),
    isOpen: (k) => open.includes(k),
    toggle: (k) => { open = open.includes(k) ? open.filter((x) => x !== k) : [...open, k]; keep(); },
    hosted: (id) => shown.filter((g) => g.host === id && codes.has(codeOf(g.host))),
    held: (sc) => (filter == null ? null : $counts.rows.filter((c) => c.shortcode === sc).reduce((a, c) => a + c.held, 0)),
  });

  // SHELL.176-4: the person's own moves in the nav; the nav never scrolls for a ticket after one.
  let since = Date.now();
  let touched = 0;
  let last = null;
  const moved = () => { touched = Date.now(); };
  let root;
  let body;
  // The person's moves are heard on the column that scrolls the nav (the page's, around this section), so a
  // wheel over its empty foot counts too; on the nav itself when nothing around it scrolls.
  $effect(() => {
    const at = scroller(root, false) ?? root;
    for (const m of MOVES) at.addEventListener(m, moved, { capture: true, passive: true });
    return () => { for (const m of MOVES) at.removeEventListener(m, moved, { capture: true }); };
  });

  $effect(() => {
    const id = current;
    if (!id || !groupsIn) { up = []; wants = {}; pin = null; return; }
    let stop = false;
    // The ticket's moment is when it changed, not when the shortcodes came: a wheel while they load counts.
    if (id !== last) { last = id; since = Date.now(); }
    pin = null;
    up = [];
    (async () => {
      const chain = [];
      for (let at = id; at && chain.length < 32 && !stop;) {
        const r = await b.call("tickets.get", { id: at });
        const t = r.ok ? r.events.find((e) => e?.type === "ticket")?.ticket : null;
        if (!t) break;
        chain.push(t);
        at = t.parent;
      }
      if (stop || !chain.length) return;
      const gs = untrack(() => groups.rows);
      const keys = [];
      const way = {};
      for (let i = 1; i < chain.length; i++) {
        const g = gs.find((g) => g.root === chain[i].id);
        keys.push(g ? g.shortcode : chain[i].id);
        way[chain[i].id] = chain[i - 1].id;
      }
      // A shortcode with no root lists its tickets by epic: open it, and look there for the top.
      const high = chain.at(-1);
      const own = gs.find((g) => g.root === high.id) ? null : gs.find((g) => g.shortcode === high.shortcode || (g.root == null && g.epic === high.epic));
      if (own) { keys.push(own.shortcode); way[own.epic] = high.id; }
      up = chain;
      wants = way;
      const add = keys.filter((k) => !untrack(() => open).includes(k));
      if (add.length) { open = [...untrack(() => open), ...add]; keep(); }
    })();
    return () => { stop = true; };
  });

  // Watch the nav's rows for the open ticket's: scroll to it once, or pin it once the nav has settled without it.
  let scrolled = false;
  $effect(() => {
    const id = here;
    const mode = flat;
    if (!id || !body || up.length === 0) return;
    scrolled = false;
    let quiet = null;
    let gone = false;
    const look = () => {
      if (gone) return;
      const el = body.querySelector(`[data-nav-current="${CSS.escape(id)}"]`);
      clearTimeout(quiet);
      if (el) {
        if (untrack(() => pin)) pin = null;
        if (!scrolled) { scrolled = true; if (touched < since) bring(el); }
        return;
      }
      quiet = setTimeout(() => { if (!gone) why(id, mode).then((w) => { if (!gone && !body.querySelector(`[data-nav-current="${CSS.escape(id)}"]`)) pin = { t: up[0], why: w }; }); }, SETTLE_MS);
    };
    const obs = new MutationObserver(look);
    obs.observe(body, { childList: true, subtree: true, attributes: true, attributeFilter: ["data-nav-current"] });
    untrack(look);
    return () => { gone = true; obs.disconnect(); clearTimeout(quiet); };
  });

  /** The row in view in the nav's column, a third of the way down; nothing when it is in view already. */
  function bring(el) {
    const sc = scroller(el);
    if (!sc) return;
    const r = el.getBoundingClientRect();
    const box = sc.getBoundingClientRect();
    if (r.top >= box.top && r.bottom <= box.bottom) return;
    sc.scrollTop += r.top - box.top - box.height / 3;
  }

  /** Why the open ticket is not among the rows. */
  async function why(id, mode) {
    const t = up[0];
    if (t?.archived != null) return "archived";
    if (!mode) return t?.hidden ? "hidden" : "not in the tree";
    if (filter == null) return "past the first 200";
    const r = await b.call("tickets.nav.member", { ids: [id], filter });
    const m = r.ok ? r.events.find((e) => e?.type === "member") : null;
    return m && m.matched === false ? "outside the filter" : "past the first 200";
  }

  function mode(to) {
    flat = to;
    since = Date.now();
  }

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

<nav class="nav-tree" data-nav aria-label="shortcodes" data-nav-here={here ?? undefined} bind:this={root}>
  <div class="nav-modes" role="group" aria-label="view">
    <button class="nav-mode" data-nav-mode="tree" aria-pressed={flat ? "false" : "true"} onclick={() => mode(false)}>Tree</button>
    <button class="nav-mode" data-nav-mode="flat" aria-pressed={flat ? "true" : "false"} onclick={() => mode(true)}>Flat</button>
  </div>
  {#if pin}
    <div class="nav-pin" data-nav-pinned={pin.t.id}>
      <Row id={pin.t.id} href={ticketHref(pin.t, base)} title={pin.t.title} kind={pin.t.kind} status={pin.t.status} frozen={pin.t.frozen} current={true}/>
      <span class="nav-pin-why" data-nav-pinned-why>{pin.why}</span>
    </div>
  {/if}
  <div class="nav-body" bind:this={body}>
  {#if flat}
    <NavFlat {filter} current={here}/>
  {:else}
    <ul class="nav-groups" role="tree" aria-label="tickets" bind:this={tree} onkeydown={keys}>
      {#each top as g (g.shortcode)}
        <NavGroup {g}/>
      {/each}
    </ul>
  {/if}
  </div>
</nav>

<style>
  .nav-tree { container: nav-tree / inline-size; display: flex; flex-direction: column; min-height: 100%; padding: 8px 0; background: var(--panel); font-size: var(--text-size); }
  .nav-modes { display: flex; gap: 4px; padding: 0 10px 8px; }
  .nav-mode { padding: 1px 9px; font-size: 11px; color: var(--muted); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .nav-mode:hover, .nav-mode:global([data-sheet~=hover]) { color: var(--ink); }
  .nav-mode:focus-visible, .nav-mode:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .nav-mode[aria-pressed=true], .nav-mode:global([data-sheet~=checked]) { color: var(--accent); border-color: var(--accent); background: var(--sel); }
  .nav-groups { list-style: none; margin: 0; padding: 0 6px 0 18px; }
  .nav-pin { margin: 0 6px 8px 10px; padding-bottom: 6px; border-bottom: var(--border) dashed var(--line); }
  .nav-pin-why { display: block; padding: 0 8px 0 40px; font-size: 11px; color: var(--warn); }
  @media (max-width: 720px) {
    .nav-tree { --pitch: 40px; }
    .nav-mode { padding: 4px 14px; font-size: 13px; }
  }
  @media (prefers-reduced-motion: reduce) {
    .nav-mode { transition: none; }
  }
</style>
