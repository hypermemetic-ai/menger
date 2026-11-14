<!-- FRAME-14: jump-palette in real Svelte (svelte/1), jump-palette.rhx's twin
     — `find` after 80 ms of quiet, the newest query only, its hits first and
     `search`'s best below. FRAME-25: on FRAME-19's bus, as rhx's — nothing
     asked while there is no query (`when`), the typing coalesced by
     `debounce_ms` in the host's registry; no timer of its own.
     SURFACE-8: FRAME-36's keys, the host's alone — `ctrl+k` opens it (`/` is
     the board's filter line's, SURFACE-2), and so does `palette.open` on the
     bus (`palette.close` closes it). The open palette's keys — escape, the arrows, enter — are
     bound only while it is open (registered on open, let go on close), so a
     closed palette leaves Enter and the arrows to the page and the browser.
     The arrows move `[data-selected]` over the find hits, then the search
     hits; enter opens the selected (the first when none was moved to).
     A `>` prefix is a command: the commands whose name starts with what
     follows, each with its route; one that takes an argument
     (`>waves to GLAZE-12`) fills its route with it, and without one says
     what it needs. A command asks no ticket.
     SURFACE-10: `sheet`, a query, draws it open on that query for the sheet
     (#/sheet) — its field not focused, its keys not bound.
     ARCHIVE-9 (the owner, 2026-11-13: "we need it on the quick search
     too"): the archive switch, as the search page's (ARCHIVE-8) — on, both
     lists ask under `archived or not archived` (a filter that names the
     archive admits it) and an archived row says so; off, as before. It sits
     beside the field (Tab from the field reaches it), and ctrl+i flips it
     while the palette is open, the caret staying in the field. Off at every
     page load; kept for the page's life (`kept`, the module's), through the
     palette's closes and remounts. `>search words` with it on routes to
     #/search/words/include/archived.
     SHELL.176-5 (the owner, 2026-11-14: "the quick search should show the
     siblings and recents before any typing"): `current`, the ticket open on
     the page (page-ticket's `data-current` on its `palette` section). With
     nothing typed the palette lists two groups: *beside it*, the open
     ticket's siblings under its parent in the parent's order (`list {
     parent }`, subtasks left out), at most nine, a window around the open
     one, which is marked `open` (`[data-current]`), with how many come
     before and after; then *recent*, the tickets opened in this browser
     most recently, newest first, the open one left out, at most eight —
     read from the service by uid (`list { filter: (ticket <uid> or …) and
     (archived or not archived) }`), so each title and status is current and
     one the reader cannot reach is not listed. With no ticket open (or a
     root), recent alone. The arrows and enter move over these rows as over
     hits, the first row that is not the open ticket selected first; typing
     replaces both groups with the hits, and clearing the box brings them
     back. Recent is this browser's record, `menger.recent` in
     localStorage (`{ id, uid, at }`, newest first, at most 20), written
     here when the page hands the palette a ticket. Marks:
     `[data-palette-siblings]` (`[data-palette-sibling=<id>]`),
     `[data-palette-recent]` (`[data-palette-recent-hit=<id>]`),
     `[data-palette-more=before|after]`. -->
<script module>
  // PERF-15: a row is the four fields drawn, not the whole summary row.
  // ARCHIVE-9: and `archived`, the mark (a live row carries none).
  const FIELDS = ["id", "uid", "title", "kind", "status", "archived"];
  /** ARCHIVE-9: the archive's filter — every ticket, archived or not. */
  const EITHER = "archived or not archived";
  /** ARCHIVE-9: the switch, kept while the page stays loaded (off at each load). */
  const kept = { archived: false };
  export const uses = ["tickets.find", "tickets.search", "tickets.get", "tickets.list"];
  /** SHELL.176-5: this browser's record of the tickets opened, newest first. */
  const RECENT = "menger.recent";
  const RECENT_KEPT = 20;
  const RECENT_SHOWN = 8;
  const SIBLINGS_SHOWN = 9;
  /** A subtask (`KILN-2.1`) is left out of the siblings, as the nav leaves it out. */
  const SUBTASK = /-\d+\.\d+$/;
  const readRecent = () => { try { const v = JSON.parse(localStorage.getItem(RECENT) || "[]"); return Array.isArray(v) ? v.filter((r) => r && typeof r.id === "string" && typeof r.uid === "string") : []; } catch { return []; } };
  /** The ticket `t` opened now: first in the record. */
  function remember(t) {
    const next = [{ id: t.id, uid: t.uid }, ...readRecent().filter((r) => r.uid !== t.uid)].slice(0, RECENT_KEPT);
    try { localStorage.setItem(RECENT, JSON.stringify(next)); } catch { /* a private window */ }
  }

  // [name, route (an argument appended), what the argument is].
  const COMMANDS = [
    ["board", "#/", ""], ["ready", "#/ready", ""], ["waves", "#/waves", ""], ["waves under", "#/waves/", "an id"],
    ["waves to", "#/waves/to/", "an id"], ["new", "#/new", ""], ["new under", "#/new/", "a parent"], ["filters", "#/filters", ""],
    ["search", "#/search/", "words"], ["releases", "#/releases", ""],
  ];

  /** The commands `>…` names: each { name, typed, rest, arg, needs, route, shown, ready }. */
  function commands(q) {
    const w = q.slice(1).trim();
    const lw = w.toLowerCase();
    const out = [];
    for (const [name, to, needs] of COMMANDS) {
      const pre = name.startsWith(lw);
      const arg = !pre && needs && lw.startsWith(name + " ") ? w.slice(name.length + 1).trim() : "";
      if (!pre && !arg) continue;
      const ready = !needs || arg !== "";
      // The argument is one segment, encoded whole: an address holds `/`
      // (MENGER/ORG-3), and the router cuts at `?` and `#`; it decodes it.
      const route = to + (arg ? encodeURIComponent(arg) : "");
      out.push({ name, typed: pre ? name.slice(0, lw.length) : name, rest: pre ? name.slice(lw.length) : "", arg, needs, route, shown: ready ? to + arg : `${to}‹${needs}›`, ready });
    }
    return out;
  }
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  import { ticketHref } from "./ticket-links.svelte";

  let { sheet = null, current = null } = $props();
  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";

  let open = $state(sheet != null);
  let q = $state(sheet ?? "");
  let sel = $state(""); // "f:ID", "s:ID", "c:NAME"; "" the first
  let need = $state("");
  const command = $derived(q.startsWith(">"));
  const fq = $derived(command ? "" : q);
  let archived = $state(kept.archived);
  let field = $state(null);
  const asks = () => (archived ? { filter: EITHER } : {});
  const found = live({ snapshot: "tickets.find", params: () => ({ query: fq, limit: 12, fields: FIELDS, ...asks() }), key: "id", when: () => fq.length > 0, debounce_ms: 80 });
  const searched = live({ snapshot: "tickets.search", params: () => ({ query: fq, limit: 8, ...asks() }), key: "id", when: () => fq.length > 0, debounce_ms: 80 });
  // Read through the stores, held from the start (as before SURFACE-8): a
  // getter read only while there is a query would let the collection go
  // between queries, and the next key would ask at once, past the debounce.
  const hits = $derived($found.rows);
  const best = $derived($searched.rows);

  // SHELL.176-5: the open ticket, its siblings and the recent tickets, for the empty box.
  const empty = $derived(q === "");
  const opened = live({ snapshot: "tickets.get", params: () => ({ id: current }), key: "id", when: () => current != null });
  const here = $derived(current == null ? null : (opened.rows.find((r) => r.title != null && r.uid != null) ?? null));
  $effect(() => { if (here) remember(here); });
  const parent = $derived(here?.parent ?? null);
  const kin = live({ snapshot: "tickets.list", params: () => ({ parent, summary: true, limit: 1000, fields: FIELDS }), key: "id",
                     when: () => open && parent != null });
  const sibs = $derived(parent == null ? [] : $kin.rows.filter((r) => !SUBTASK.test(r.id)));
  // A window of at most nine around the open ticket.
  const at = $derived(Math.max(0, sibs.findIndex((r) => r.id === here?.id)));
  const from = $derived(Math.max(0, Math.min(at - Math.floor(SIBLINGS_SHOWN / 2), sibs.length - SIBLINGS_SHOWN)));
  const sibShown = $derived(sibs.slice(from, from + SIBLINGS_SHOWN));
  let recentIds = $state([]);
  const wanted = $derived(recentIds.filter((r) => r.uid !== here?.uid).slice(0, RECENT_SHOWN));
  const back = live({ snapshot: "tickets.list",
                      params: () => ({ filter: `(${wanted.map((r) => "ticket " + r.uid).join(" or ")}) and (archived or not archived)`, summary: true, limit: RECENT_SHOWN, fields: FIELDS }),
                      key: "id", when: () => open && wanted.length > 0 });
  const recent = $derived(wanted.length === 0 ? [] : wanted.map((w) => $back.rows.find((r) => r.uid === w.uid)).filter(Boolean));
  const cmds = $derived(command ? commands(q) : []);
  // What the arrows move over, in order; the selected one ("" is the first).
  const order = $derived(command ? cmds.map((c) => "c:" + c.name) : fq ? [...hits.map((f) => "f:" + f.id), ...best.map((h) => "s:" + h.id)]
                        : [...sibShown.map((r) => "b:" + r.id), ...recent.map((r) => "r:" + r.id)]);
  const chosen = $derived(order.includes(sel) ? sel : (order.find((k) => k !== "b:" + here?.id) ?? order[0] ?? ""));

  function show() {
    recentIds = readRecent();
    open = true;
  }
  function close() {
    open = false;
    q = "";
    sel = "";
    need = "";
  }
  function typed() {
    sel = "";
    need = "";
  }
  /** ARCHIVE-9: the archive switch, flipped (ctrl+i) or set (the box); the caret back in the field. */
  function include(on = !archived) {
    archived = kept.archived = on;
    need = "";
    field?.focus();
  }
  function step(by) {
    if (!order.length) return;
    const at = Math.max(0, order.indexOf(chosen));
    sel = order[Math.min(order.length - 1, Math.max(0, at + by))];
    need = "";
  }
  /** A command: its route, or — still wanting its argument — say so and stay. */
  function run(c) {
    if (!c.ready) {
      sel = "c:" + c.name;
      need = `>${c.name} needs ${c.needs}: type it after the command`;
      return;
    }
    b.navigate(c.name === "search" && archived ? c.route + "/include/archived" : c.route);
    close();
  }
  function clicked(ev, c) {
    ev.preventDefault();
    run(c);
  }
  function choose() {
    if (command) {
      const c = cmds.find((c) => "c:" + c.name === chosen);
      if (c) run(c);
      return;
    }
    if (!chosen) return;
    b.navigate(ticketHref([...hits, ...best, ...sibShown, ...recent].find((r) => r.id === chosen.slice(2)) ?? { id: chosen.slice(2) }, base));
    close();
  }

  // ctrl+k is keys-global's: it emits `palette.open`, as the bar's button does.
  b.on("palette.open", show);
  b.on("palette.close", close);
  // The open palette's keys: bound while it is open, let go when it closes.
  $effect(() => {
    if (!open || sheet != null) return;
    return b.keys({
      "escape": { to: close, in_inputs: true, label: "close the palette" },
      "arrowdown": { to: () => step(1), in_inputs: true, label: "the next hit" },
      "arrowup": { to: () => step(-1), in_inputs: true, label: "the hit before" },
      "enter": { to: choose, in_inputs: true, label: "open the selected" },
      "ctrl+i": { to: () => include(), in_inputs: true, label: "include archived tickets, or not" },
    });
  });
  function focus(node) {
    if (sheet == null) node.focus();
  }
</script>

<div class="jump-palette" data-palette data-surface="jump-palette-sv" data-sheet-open={sheet != null ? "" : undefined}>
  {#if open}
    <div class="scrim" data-scrim onclick={close} aria-hidden="true"></div>
    <div class="palette-sheet" role="dialog" aria-label="jump to a ticket">
      <div class="palette-head">
        <input class="palette-input" data-palette-input spellcheck="false" autocomplete="off" use:focus bind:this={field}
               aria-label="jump to a ticket" placeholder="jump to a ticket — an id or words; > for a command" bind:value={q} oninput={typed}/>
        <label class="palette-arch" data-field="include" title="include archived tickets (ctrl+i)">
          <input type="checkbox" data-action="archived" aria-keyshortcuts="Control+I" checked={archived} onchange={(e) => include(e.currentTarget.checked)}/>
          include archived
        </label>
      </div>
      {#if command}
        <p class="palette-label">commands</p>
        <ul class="palette-hits" data-palette-commands>
          {#each cmds as c (c.name)}
            <li><a class="palette-cmd" data-command={c.name} data-selected={chosen === "c:" + c.name ? "" : undefined} href={c.ready ? c.route : undefined} onclick={(ev) => clicked(ev, c)}><span class="gt">{">"}</span><span class="cmd"><b>{c.typed}</b>{c.rest}</span><span class="arg">{c.arg}</span><span class="to">{c.shown}</span></a></li>
          {/each}
        </ul>
        {#if cmds.length === 0}<p class="palette-empty" data-empty>No command starts “{q.slice(1).trim()}”.</p>{/if}
        {#if need}<p class="palette-need" data-palette-need role="alert">{need}</p>{/if}
      {:else if q !== ""}
        <ul class="palette-hits" data-palette-hits>
          {#each hits as f (f.id)}
            <li><a class="palette-hit" data-palette-hit={f.id} data-selected={chosen === "f:" + f.id ? "" : undefined} data-archived={f.archived ? "" : undefined} href={ticketHref(f, base)} onclick={close}><span class="row-id">{f.id}</span> <span class="row-title">{f.title}</span>{#if f.archived}<span class="arch" data-field="archived">archived</span>{/if}</a></li>
          {/each}
        </ul>
        {#if $found.status === "pending"}<div class="palette-loading" data-loading><i></i><i></i></div>{/if}
        {#if $found.status === "error" || $found.status === "refused"}<p class="palette-need" data-refused role="alert">{$found.message}</p>{/if}
        <p class="palette-label">in titles and bodies</p>
        <ul class="palette-hits search" data-search-hits>
          {#each best as h (h.id)}
            <li><a class="palette-hit" data-search-hit={h.id} data-selected={chosen === "s:" + h.id ? "" : undefined} data-archived={h.archived ? "" : undefined} href={ticketHref(h, base)} onclick={close}><span class="row-id">{h.id}</span> <span class="row-title">{h.title}</span>{#if h.archived}<span class="arch" data-field="archived">archived</span>{/if}</a></li>
          {/each}
        </ul>
        {#if $found.status === "live" && $searched.status === "live" && hits.length === 0 && best.length === 0}<p class="palette-empty" data-empty>Nothing says “{q}”.</p>{/if}
      {:else}
        {#if sibShown.length}
          <p class="palette-label">beside it · under <span class="palette-parent">{parent}</span></p>
          {#if from > 0}<p class="palette-more" data-palette-more="before">{from} before</p>{/if}
          <ul class="palette-hits" data-palette-siblings>
            {#each sibShown as r (r.id)}
              <li><a class="palette-hit" data-palette-sibling={r.id} data-current={r.id === here?.id ? "" : undefined} aria-current={r.id === here?.id ? "page" : undefined} data-selected={chosen === "b:" + r.id ? "" : undefined} data-status={r.status} href={ticketHref(r, base)} onclick={close}><span class="row-id">{r.id}</span> <span class="row-title">{r.title}</span>{#if r.id === here?.id}<span class="here" data-field="open">open</span>{/if}</a></li>
            {/each}
          </ul>
          {#if sibs.length - from - sibShown.length > 0}<p class="palette-more" data-palette-more="after">{sibs.length - from - sibShown.length} more</p>{/if}
        {/if}
        {#if recent.length}
          <p class="palette-label">recent</p>
          <ul class="palette-hits" data-palette-recent>
            {#each recent as r (r.id)}
              <li><a class="palette-hit" data-palette-recent-hit={r.id} data-selected={chosen === "r:" + r.id ? "" : undefined} data-archived={r.archived ? "" : undefined} data-status={r.status} href={ticketHref(r, base)} onclick={close}><span class="row-id">{r.id}</span> <span class="row-title">{r.title}</span>{#if r.archived}<span class="arch" data-field="archived">archived</span>{/if}</a></li>
            {/each}
          </ul>
        {/if}
      {/if}
      <div class="palette-foot"><span><kbd>↑</kbd><kbd>↓</kbd> move</span><span><kbd>enter</kbd> {command ? "go" : "open"}</span><span><kbd>esc</kbd> close</span><span><kbd>{">"}</kbd> commands</span><span class="foot-arch"><kbd>ctrl</kbd><kbd>i</kbd> archived {archived ? "on" : "off"}</span></div>
    </div>
  {/if}
</div>

<style>
  .jump-palette .scrim { position: fixed; inset: 0; z-index: 9; background: var(--shadow); }
  .jump-palette .palette-sheet { position: fixed; z-index: 10; top: 12vh; left: 50%; transform: translateX(-50%); display: flex; flex-direction: column; width: min(640px, calc(100vw - 32px)); max-height: 70vh; overflow: hidden; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-xl); box-shadow: 0 10px 30px var(--shadow); }
  .jump-palette[data-sheet-open] .palette-sheet { top: 8px; left: 8px; right: 8px; width: auto; max-height: calc(100% - 16px); transform: none; }
  .jump-palette .palette-head { display: flex; align-items: center; border-bottom: var(--border) solid var(--line); }
  .jump-palette .palette-input { flex: 1; min-width: 0; padding: 12px 14px; font-size: 14px; color: var(--ink); background: none; border: 0; outline: none; }
  .jump-palette .palette-input:focus-visible, .jump-palette .palette-input:global([data-sheet~=focus]) { box-shadow: inset 0 -2px 0 var(--focus); }
  .jump-palette .palette-hits { list-style: none; margin: 0; padding: 4px; overflow: auto; }
  .jump-palette .palette-hit, .jump-palette .palette-cmd { cursor: pointer; display: flex; gap: 10px; align-items: center; height: var(--pitch); padding: 0 10px; overflow: hidden; white-space: nowrap; color: var(--ink); text-decoration: none; border-radius: var(--radius); transition: background-color var(--motion-fast) var(--ease); }
  .jump-palette .palette-hit:hover, .jump-palette .palette-cmd:hover, .jump-palette .palette-hit:global([data-sheet~=hover]), .jump-palette .palette-cmd:global([data-sheet~=hover]) { background: var(--hover); }
  .jump-palette .palette-hit[data-selected], .jump-palette .palette-cmd[data-selected], .jump-palette .palette-hit:global([data-sheet~=selected]), .jump-palette .palette-cmd:global([data-sheet~=selected]) { background: var(--row-sel); box-shadow: inset 3px 0 0 var(--accent); }
  .jump-palette .palette-hit:focus-visible, .jump-palette .palette-cmd:focus-visible, .jump-palette .palette-hit:global([data-sheet~=focus]), .jump-palette .palette-cmd:global([data-sheet~=focus]) { outline: none; background: var(--row-sel); box-shadow: inset 0 0 0 2px var(--focus); }
  .jump-palette .row-id { flex: none; color: var(--accent); font-variant-numeric: tabular-nums; }
  .jump-palette .row-title { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  /* ARCHIVE-9: the switch beside the field, and the archived mark as the search page draws it (ARCHIVE-8). */
  .jump-palette .palette-arch { flex: none; display: inline-flex; gap: 5px; align-items: center; padding: 0 14px 0 6px; font-size: 12px; color: var(--muted); white-space: nowrap; cursor: pointer; }
  .jump-palette .palette-arch input { margin: 0; accent-color: var(--accent); cursor: pointer; }
  .jump-palette .palette-arch input:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .jump-palette .palette-arch:hover { color: var(--ink); }
  .jump-palette .arch { flex: none; margin-left: auto; padding: 0 6px; font-size: 10px; line-height: 16px; color: var(--muted); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); white-space: nowrap; }
  .jump-palette .palette-hit[data-archived] .row-title { color: var(--muted); }
  .jump-palette .gt { color: var(--muted); }
  .jump-palette .cmd b { color: var(--accent); font-weight: var(--weight-bold); }
  .jump-palette .arg { color: var(--muted); }
  .jump-palette .to { margin-left: auto; font-size: 11px; color: var(--muted); }
  .jump-palette .palette-label { margin: 6px 14px 2px; font-size: 11px; color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .jump-palette .palette-empty { margin: 6px 14px; color: var(--muted); }
  /* SHELL.176-5: the empty box's groups — the parent named, the open sibling marked, how many more. */
  .jump-palette .palette-parent { color: var(--accent); text-transform: none; letter-spacing: 0; font-family: var(--mono); }
  .jump-palette .palette-more { margin: 0 14px; font-size: 11px; color: var(--muted); }
  .jump-palette .palette-hit[data-current] .row-title { font-weight: var(--weight-bold); }
  .jump-palette .here { flex: none; margin-left: auto; padding: 0 6px; font-size: 10px; line-height: 16px; color: var(--accent); border: var(--border) solid var(--accent); border-radius: var(--radius-pill); }
  .jump-palette .palette-need { margin: 4px 14px 8px; font-size: 11px; color: var(--warn); }
  .jump-palette .palette-loading { display: flex; flex-direction: column; gap: 6px; padding: 6px 14px; }
  .jump-palette .palette-loading i { display: block; height: 12px; border-radius: var(--radius-sm); background: var(--skeleton); }
  .jump-palette .palette-foot { display: flex; flex-wrap: wrap; gap: 4px 14px; padding: 6px 14px; font-size: 11px; color: var(--muted); border-top: var(--border) solid var(--line); }
  .jump-palette kbd { display: inline-block; min-width: 18px; padding: 0 5px; font: 11px/17px var(--font-text); text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
</style>
