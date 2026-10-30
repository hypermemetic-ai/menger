<!-- FRAME-14: jump-palette in real Svelte (svelte/1), jump-palette.rhx's twin
     — `find` after 80 ms of quiet, the newest query only, its hits first and
     `search`'s best below. FRAME-25: on FRAME-19's bus, as rhx's — nothing
     asked while there is no query (`when`), the typing coalesced by
     `debounce_ms` in the host's registry; no timer of its own.
     SURFACE-8: FRAME-36's keys, the host's alone — `ctrl+k` (and `/` outside
     a field) opens it, and so does `palette.open` on the bus (`palette.close`
     closes it). The open palette's keys — escape, the arrows, enter — are
     bound only while it is open (registered on open, let go on close), so a
     closed palette leaves Enter and the arrows to the page and the browser.
     The arrows move `[data-selected]` over the find hits, then the search
     hits; enter opens the selected (the first when none was moved to).
     A `>` prefix is a command: the commands whose name starts with what
     follows, each with its route; one that takes an argument
     (`>waves to GLAZE-12`) fills its route with it, and without one says
     what it needs. A command asks no ticket. -->
<script module>
  // PERF-15: a row is the four fields drawn, not the whole summary row.
  const FIELDS = ["id", "title", "kind", "status"];
  export const uses = ["tickets.find", "tickets.search"];

  // [name, route (an argument appended), what the argument is].
  const COMMANDS = [
    ["board", "#/", ""], ["ready", "#/ready", ""], ["waves", "#/waves", ""], ["waves under", "#/waves/", "an id"],
    ["waves to", "#/waves/to/", "an id"], ["new", "#/new", ""], ["new under", "#/new/", "a parent"], ["filters", "#/filters", ""],
    ["search", "#/search/", "words"], ["epics", "#/epics", ""], ["me", "#/me", ""], ["releases", "#/releases", ""],
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

  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";

  let open = $state(false);
  let q = $state("");
  let sel = $state(""); // "f:ID", "s:ID", "c:NAME"; "" the first
  let need = $state("");
  const command = $derived(q.startsWith(">"));
  const fq = $derived(command ? "" : q);
  const found = live({ snapshot: "tickets.find", params: () => ({ query: fq, limit: 12, fields: FIELDS }), key: "id", when: () => fq.length > 0, debounce_ms: 80 });
  const searched = live({ snapshot: "tickets.search", params: () => ({ query: fq, limit: 8 }), key: "id", when: () => fq.length > 0, debounce_ms: 80 });
  // Read through the stores, held from the start (as before SURFACE-8): a
  // getter read only while there is a query would let the collection go
  // between queries, and the next key would ask at once, past the debounce.
  const hits = $derived($found.rows);
  const best = $derived($searched.rows);
  const cmds = $derived(command ? commands(q) : []);
  // What the arrows move over, in order; the selected one ("" is the first).
  const order = $derived(command ? cmds.map((c) => "c:" + c.name) : fq ? [...hits.map((f) => "f:" + f.id), ...best.map((h) => "s:" + h.id)] : []);
  const current = $derived(order.includes(sel) ? sel : (order[0] ?? ""));

  function show() {
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
  function step(by) {
    if (!order.length) return;
    const at = Math.max(0, order.indexOf(current));
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
    b.navigate(c.route);
    close();
  }
  function clicked(ev, c) {
    ev.preventDefault();
    run(c);
  }
  function choose() {
    if (command) {
      const c = cmds.find((c) => "c:" + c.name === current);
      if (c) run(c);
      return;
    }
    if (!current) return;
    b.navigate(base + "ticket/" + current.slice(2));
    close();
  }

  b.keys({
    "ctrl+k": { to: show, label: "jump to a ticket" },
    "/": { to: () => { if (!open) show(); }, label: "jump to a ticket" },
  });
  b.on("palette.open", show);
  b.on("palette.close", close);
  // The open palette's keys: bound while it is open, let go when it closes.
  $effect(() => {
    if (!open) return;
    return b.keys({
      "escape": { to: close, in_inputs: true, label: "close the palette" },
      "arrowdown": { to: () => step(1), in_inputs: true, label: "the next hit" },
      "arrowup": { to: () => step(-1), in_inputs: true, label: "the hit before" },
      "enter": { to: choose, in_inputs: true, label: "open the selected" },
    });
  });
  function focus(node) {
    node.focus();
  }
</script>

<div class="jump-palette" data-palette data-surface="jump-palette-sv">
  {#if open}
    <div class="scrim" data-scrim onclick={close} aria-hidden="true"></div>
    <div class="palette-sheet" role="dialog" aria-label="jump to a ticket">
      <input class="palette-input" data-palette-input spellcheck="false" autocomplete="off" use:focus
             placeholder="jump to a ticket — an id or words; > for a command" bind:value={q} oninput={typed}/>
      {#if command}
        <p class="palette-label">commands</p>
        <ul class="palette-hits" data-palette-commands>
          {#each cmds as c (c.name)}
            <li><a class="palette-cmd" data-command={c.name} data-selected={current === "c:" + c.name ? "" : undefined} href={c.ready ? c.route : undefined} onclick={(ev) => clicked(ev, c)}><span class="gt">{">"}</span><span class="cmd"><b>{c.typed}</b>{c.rest}</span><span class="arg">{c.arg}</span><span class="to">{c.shown}</span></a></li>
          {/each}
        </ul>
        {#if cmds.length === 0}<p class="palette-empty" data-empty>No command starts “{q.slice(1).trim()}”.</p>{/if}
        {#if need}<p class="palette-need" data-palette-need role="alert">{need}</p>{/if}
      {:else if q !== ""}
        <ul class="palette-hits" data-palette-hits>
          {#each hits as f (f.id)}
            <li><a class="palette-hit" data-palette-hit={f.id} data-selected={current === "f:" + f.id ? "" : undefined} href={base + "ticket/" + f.id} onclick={close}><span class="row-id">{f.id}</span> <span class="row-title">{f.title}</span></a></li>
          {/each}
        </ul>
        {#if $found.status === "pending"}<div class="palette-loading" data-loading><i></i><i></i></div>{/if}
        {#if $found.status === "error" || $found.status === "refused"}<p class="palette-need" data-refused role="alert">{$found.message}</p>{/if}
        <p class="palette-label">in titles and bodies</p>
        <ul class="palette-hits search" data-search-hits>
          {#each best as h (h.id)}
            <li><a class="palette-hit" data-search-hit={h.id} data-selected={current === "s:" + h.id ? "" : undefined} href={base + "ticket/" + h.id} onclick={close}><span class="row-id">{h.id}</span> <span class="row-title">{h.title}</span></a></li>
          {/each}
        </ul>
        {#if $found.status === "live" && $searched.status === "live" && hits.length === 0 && best.length === 0}<p class="palette-empty" data-empty>Nothing says “{q}”.</p>{/if}
      {/if}
      <div class="palette-foot"><span><kbd>↑</kbd><kbd>↓</kbd> move</span><span><kbd>enter</kbd> {command ? "go" : "open"}</span><span><kbd>esc</kbd> close</span><span><kbd>{">"}</kbd> commands</span></div>
    </div>
  {/if}
</div>

<style>
  .jump-palette .scrim { position: fixed; inset: 0; z-index: 9; background: var(--shadow); }
  .jump-palette .palette-sheet { position: fixed; z-index: 10; top: 12vh; left: 50%; transform: translateX(-50%); display: flex; flex-direction: column; width: min(640px, calc(100vw - 32px)); max-height: 70vh; overflow: hidden; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-xl); box-shadow: 0 10px 30px var(--shadow); }
  .jump-palette .palette-input { padding: 12px 14px; font-size: 14px; color: var(--ink); background: none; border: 0; border-bottom: var(--border) solid var(--line); outline: none; }
  .jump-palette .palette-input:focus-visible { box-shadow: inset 0 -2px 0 var(--focus); }
  .jump-palette .palette-hits { list-style: none; margin: 0; padding: 4px; overflow: auto; }
  .jump-palette .palette-hit, .jump-palette .palette-cmd { cursor: pointer; display: flex; gap: 10px; align-items: center; height: var(--pitch); padding: 0 10px; overflow: hidden; white-space: nowrap; color: var(--ink); text-decoration: none; border-radius: var(--radius); transition: background-color var(--motion-fast) var(--ease); }
  .jump-palette .palette-hit:hover, .jump-palette .palette-cmd:hover { background: var(--hover); }
  .jump-palette .palette-hit[data-selected], .jump-palette .palette-cmd[data-selected] { background: var(--row-sel); box-shadow: inset 3px 0 0 var(--accent); }
  .jump-palette .palette-hit:focus-visible, .jump-palette .palette-cmd:focus-visible { outline: none; background: var(--row-sel); box-shadow: inset 0 0 0 2px var(--focus); }
  .jump-palette .row-id { flex: none; color: var(--accent); font-variant-numeric: tabular-nums; }
  .jump-palette .row-title { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .jump-palette .gt { color: var(--muted); }
  .jump-palette .cmd b { color: var(--accent); font-weight: var(--weight-bold); }
  .jump-palette .arg { color: var(--muted); }
  .jump-palette .to { margin-left: auto; font-size: 11px; color: var(--muted); }
  .jump-palette .palette-label { margin: 6px 14px 2px; font-size: 11px; color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .jump-palette .palette-empty { margin: 6px 14px; color: var(--muted); }
  .jump-palette .palette-need { margin: 4px 14px 8px; font-size: 11px; color: var(--warn); }
  .jump-palette .palette-loading { display: flex; flex-direction: column; gap: 6px; padding: 6px 14px; }
  .jump-palette .palette-loading i { display: block; height: 12px; border-radius: var(--radius-sm); background: var(--skeleton); }
  .jump-palette .palette-foot { display: flex; gap: 14px; padding: 6px 14px; font-size: 11px; color: var(--muted); border-top: var(--border) solid var(--line); }
  .jump-palette kbd { display: inline-block; min-width: 18px; padding: 0 5px; font: 11px/17px var(--font-text); text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
</style>
