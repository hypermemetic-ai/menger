<!-- SURFACE-9: the epics (#/epics), svelte/1 — `epics.list` (the archived
     ones too behind a toggle) kept by `epics.watch`: a change comes as
     `epic_changed { name, what }` and lands on its row as what changed (the
     watch names no card; the list's snapshot is the card). `epics.current`
     with the watch's `moved` after it: the notice an archive moved me, and
     `dismiss`. `switch` is a write, `bus.call`: the top says the epic asked
     at once and the answer puts it right — `current { epic }` confirms,
     `not_found { name }` or a refusal is said beside the action and the top
     goes back. `get` is one card. A switch is said on the bus as
     `epic.switched { epic }` for the app's top. -->
<script module>
  export const uses = ["epics.list", "epics.watch", "epics.current", "epics.switch", "epics.dismiss", "epics.get"];
</script>

<script>
  import { untrack } from "svelte";
  import { bus, live } from "plexus-ui/host";
  // SURFACE-10: the sheet holds the loading state by its mark (data-sheet="loading").
  let { sheet = null } = $props();

  const b = bus();

  // FRAME-37: `bus.call` answers `{ ok, events, message }`; before it, the
  // data events (an array) or a rejection with the refusal's words.
  async function answer(asked) {
    try {
      const a = await asked;
      if (a && typeof a.ok === "boolean") return { ok: a.ok, events: a.events ?? [], message: a.message ?? "" };
      const events = Array.isArray(a) ? a : [];
      const bad = events.find((e) => e?.type === "refused" || e?.type === "error" || e?.type === "not_found");
      return { ok: !bad, events, message: bad ? (bad.message ?? `no epic ${bad.name ?? ""} here`) : "" };
    } catch (e) {
      return { ok: false, events: [], message: e?.message ?? String(e) };
    }
  }

  let archived = $state(false);
  let picked = $state(null);
  // The list: `epics.list` kept by `epics.watch`. An `epic_changed` names an
  // epic and what happened, not its card, so the list is asked again
  // (FRAME-40's refetch rule will say so in the spec) and what changed is
  // kept on its row; a deleted or archived epic stays drawn as what
  // happened to it, with nothing to switch to. Rows are this organization's:
  // the watch covers every one the reader holds.
  const epics = live({ snapshot: "epics.list", params: () => ({ archived }), changes: "epics.watch", key: "name", on: { epic_changed: "upsert:." } });
  const current = live({ snapshot: "epics.current", params: {}, changes: "epics.watch", key: "org", on: { current: "upsert:.", moved: "upsert:." } });
  const one = live({ snapshot: "epics.get", params: () => ({ name: picked }), key: "name", when: () => picked != null, on: { epic: "upsert:epic" } });

  let fresh = $state(null);
  let changed = $state({});
  const known = {};
  // Where I am, and the notice an archive moved me: from the current event's
  // own fields at first, then from each answer and each `moved` of my
  // organization the watch brings.
  let epicNow = $state(null);
  let org = $state(null);
  let notice = $state(null);
  let refused = $state({});

  // The list asked again; the newest asking's answer stands, whichever lands last.
  let asked = 0;
  async function reask() {
    const n = ++asked;
    const a = await answer(b.call("epics.list", { archived }));
    if (a.ok && n === asked) fresh = a.events.filter((e) => e?.type === "epic").map((e) => e.epic);
  }
  // What the watch brought, each row heard once (a row a change left alone is
  // the same object: FRAME-20).
  const heard = new WeakSet();
  $effect(() => {
    const rows = epics.rows;
    untrack(() => {
      // Any change asks the list again — another organization's epic of the
      // same name lands on this one's row in the collection, so the list
      // (this organization's) is the truth; only this organization's are
      // kept as what changed.
      const news = rows.filter((r) => r.what != null && !heard.has(r));
      for (const r of rows) heard.add(r);
      if (!news.length) return;
      const mine = news.filter((r) => org == null || r.org === org);
      if (mine.length) {
        const next = { ...changed };
        for (const r of mine) next[r.name] = r.what;
        changed = next;
      }
      reask();
    });
  });
  $effect(() => {
    const rows = current.rows;
    untrack(() => {
      for (const r of rows) {
        if (heard.has(r)) continue;
        heard.add(r);
        if (r.epic != null) {
          org = r.epic.org;
          if (epicNow == null) epicNow = r.epic.name;
          if (r.moved && notice == null) notice = r.moved;
        } else if (r.moved != null && r.org != null && (org == null || r.org === org)) {
          notice = r.moved;
          epicNow = r.moved.to;
        }
      }
    });
  });
  // Asked once at mount and on each toggle: the list drawn is the one asked
  // for this organization, never a row another organization's change landed on.
  $effect(() => {
    archived;
    untrack(reask);
  });

  const rows = $derived.by(() => {
    const full = fresh ?? epics.rows.filter((r) => r.what == null);
    for (const r of full) known[r.name] = r;
    const out = full.map((r) => ({ ...r, what: changed[r.name] ?? null, gone: false }));
    for (const [name, what] of Object.entries(changed)) {
      if ((what === "deleted" || what === "archived") && !out.some((r) => r.name === name)) out.push({ ...(known[name] ?? { name }), name, what, gone: true });
    }
    return out;
  });
  const cur = $derived(epicNow ?? "");
  const card = $derived(picked != null && $one.status === "live" ? $one.rows.find((r) => r.name === picked) ?? null : null);

  async function switchTo(name) {
    const was = epicNow;
    epicNow = name;
    refused = {};
    b.emit("epic.switched", { epic: name });
    const a = await answer(b.call("epics.switch", { name }));
    const now = a.events.find((e) => e?.type === "current");
    if (a.ok && now) epicNow = now.epic.name;
    else {
      epicNow = was;
      refused = { [name]: a.message || `no epic ${name} here` };
      b.emit("epic.switched", { epic: was });
    }
  }
  async function dismiss() {
    const was = notice;
    notice = null;
    const a = await answer(b.call("epics.dismiss", {}));
    if (!a.ok) { notice = was; refused = { dismiss: a.message }; }
  }
  const when = (t) => (t ? new Date(t * 1000).toLocaleDateString(undefined, { year: "numeric", month: "short", day: "2-digit" }) : "");
</script>

<section class="epics-page" data-surface="epics-page">
  <div class="page-head">
    <h1>epics</h1>
    <span class="sub">you are in <strong data-field="current">{cur}</strong></span>
    {#if $current.status !== "live"}<span class="state" data-loading={$current.status}>{$current.status}</span>{/if}
  </div>
  {#if notice}
    <p class="notice" data-field="moved">
      an archive of <strong>{notice.from}</strong> moved you to <strong>{notice.to}</strong>
      <button class="btn" data-action="epics.dismiss" onclick={dismiss}>dismiss</button>
      {#if refused.dismiss}<span class="refused" data-refused="">refused: {refused.dismiss}</span>{/if}
    </p>
  {/if}
  <div class="tools" data-form="epics.list">
    <label><input type="checkbox" bind:checked={archived}/> the archived ones too</label>
  </div>
  <div class="panel">
    {#each rows as e (e.name)}
      <div class="row" data-id={e.name} data-current={e.name === cur} data-archived={e.archived != null || e.what === "archived"} data-changed={e.what ?? ""}>
        <button class="name" data-action="epics.get" onclick={() => (picked = e.name)}>{e.name}</button>
        {#if !e.gone}
          <span class="meta"><span data-field="tickets">{e.tickets}</span> tickets</span>
          <span class="meta"><span data-field="shortcodes">{e.shortcodes}</span> shortcodes</span>
          <span class="meta"><span data-field="members_on">{e.members_on}</span> on it</span>
          <span class="tags">{#if e.default}<span class="tag" data-field="default">default</span>{/if}{#if e.archived}<span class="tag archived">archived</span>{/if}</span>
        {:else}
          <span class="meta changed" data-field="what">{e.what}</span><span></span><span></span><span class="tags"></span>
        {/if}
        {#if e.name === cur}
          <span class="here">here</span>
        {:else if e.gone || e.archived != null}
          <span class="here gone">{e.what ?? "archived"}</span>
        {:else}
          <button class="btn" data-action="epics.switch" onclick={() => switchTo(e.name)}>switch</button>
        {/if}
        {#if refused[e.name]}<span class="refused" data-refused="">refused: {refused[e.name]}</span>{/if}
      </div>
    {/each}
    {#if sheet === "loading" || $epics.status === "pending"}<div class="row"><span data-loading="">genesis</span></div>{/if}
    {#if $epics.status === "live" && rows.length === 0}<p class="empty" data-empty="">No epics here.</p>{/if}
    {#if $epics.status === "error" || $epics.status === "refused"}<p class="refused pad" data-refused="">refused: {$epics.message}</p>{/if}
  </div>
  {#if picked != null}
    <aside class="panel card" data-field="detail">
      {#if card && card.org != null}
        <h2 data-field="name">{card.name}</h2>
        <dl>
          <dt>organization</dt><dd data-field="org">{card.org}</dd>
          <dt>tickets</dt><dd data-field="tickets">{card.tickets}</dd>
          <dt>shortcodes</dt><dd data-field="shortcodes">{card.shortcodes}</dd>
          <dt>people on it</dt><dd data-field="members_on">{card.members_on}</dd>
          <dt>made</dt><dd data-field="created">{when(card.created_unix)}</dd>
          <dt>first</dt><dd>{card.first ? "yes" : "no"}</dd>
          <dt>default</dt><dd>{card.default ? "yes" : "no"}</dd>
          {#if card.archived}<dt>archived</dt><dd data-field="archived">{when(card.archived.at_unix)} by {card.archived.by}</dd>{/if}
        </dl>
      {:else if card}
        <p class="refused" data-refused="">no epic {card.name} here</p>
      {:else if $one.status === "error" || $one.status === "refused"}
        <p class="refused" data-refused="">refused: {$one.message}</p>
      {:else}
        <p><span data-loading="">{picked}</span></p>
      {/if}
    </aside>
  {/if}
</section>

<style>
  .epics-page { max-width: 1180px; margin: 0 auto; padding: 18px 24px 40px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .page-head { display: flex; align-items: baseline; gap: 10px; margin: 0 0 14px; }
  .page-head h1 { margin: 0; font-family: var(--font-display); font-size: 20px; font-weight: var(--weight-strong); }
  .sub { color: var(--muted); font-size: 12px; }
  .sub strong { color: var(--accent); font-weight: var(--weight-medium); }
  .state { margin-left: auto; font-size: 11px; color: var(--muted); }
  .notice { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin: 0 0 12px; padding: 6px 10px; background: var(--sel); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .tools { margin: 0 0 8px; font-size: 12px; color: var(--muted); }
  .tools input:focus-visible, .tools input:global([data-sheet~=focus]) { outline: 2px solid var(--focus); }
  .panel { overflow: hidden; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .row { display: grid; grid-template-columns: minmax(120px, 1fr) auto auto auto minmax(0, 1fr) auto; gap: 12px; align-items: center; min-height: var(--pitch); padding: 0 10px; border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .row:last-child { border-bottom: 0; }
  .row:hover, .row:global([data-sheet~=hover]) { background: var(--hover); }
  .row[data-current=true] { background: var(--sel); box-shadow: inset 3px 0 0 var(--accent); }
  .row[data-archived=true] .name { color: var(--muted); }
  .row[data-changed=deleted] .name { color: var(--disabled); text-decoration: line-through; }
  .name { justify-self: start; padding: 0; color: var(--accent); background: none; border: 0; cursor: pointer; border-radius: var(--radius-sm); }
  .name:hover, .name:global([data-sheet~=hover]) { text-decoration: underline; }
  .name:focus-visible, .name:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .meta { color: var(--muted); font-size: 12px; font-variant-numeric: tabular-nums; }
  .meta span { color: var(--ink); }
  .changed { font-style: italic; }
  .tags { display: flex; gap: 6px; }
  .tag { padding: 0 7px; font-size: 11px; line-height: 18px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  .tag.archived { color: var(--muted); }
  .here { font-size: 11px; color: var(--accent); }
  .here.gone { color: var(--muted); }
  .btn { padding: 3px 10px; font-size: 12px; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: border-color var(--motion-fast) var(--ease), background-color var(--motion-fast) var(--ease); }
  .btn:hover:not(:disabled), .btn:global([data-sheet~=hover]):not(:disabled) { border-color: var(--muted); background: var(--hover); }
  .btn:focus-visible, .btn:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .btn:disabled, .btn:global([data-sheet~=disabled]) { color: var(--disabled); cursor: default; }
  .refused { grid-column: 1 / -1; margin: 0 0 6px; font-size: 11px; color: var(--warn); }
  .pad { margin: 8px 10px; }
  [data-loading] { color: transparent; background: var(--skeleton); border-radius: var(--radius-sm); }
  .empty { margin: 8px 10px; color: var(--muted); }
  .card { margin-top: 18px; padding: 12px 14px; }
  .card h2 { margin: 0 0 8px; font-size: 15px; font-weight: var(--weight-strong); }
  .card dl { display: grid; grid-template-columns: max-content 1fr; gap: 4px 14px; margin: 0; }
  .card dt { color: var(--muted); }
  .card dd { margin: 0; overflow-wrap: anywhere; }
  @media (prefers-reduced-motion: reduce) { .row, .btn { transition: none; } }
  @media (max-width: 720px) { .row { grid-template-columns: 1fr auto; } .meta, .tags { display: none; } }
</style>
