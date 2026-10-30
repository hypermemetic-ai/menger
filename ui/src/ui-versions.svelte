<!-- SURFACE-9: the releases (#/releases), svelte/1 — every kept version of
     the UI (`ui.versions`, newest first) with the live one marked, the
     studio's authored ones said, a bundle kept by name (the studio's) never
     rolled back to. `ui.watch`'s `live { version }` names a hash and no row,
     so the pointer is state of its own, set by every move the watch makes
     (the script says how each is heard), and the list is read again. Beside it the contract the tickets service stamps
     and the one over every service the app reaches (`ui.contract`), the
     routes (`ui.routes.list`, kept by `route_changed`) and the sections
     (`ui.slots.list`). For a reader holding admin: rollback asks first and
     its answer (`live { version }`, or the refusal's words) is said in the
     row; preview's answer is the link to /app/, and a clear. Anyone else
     sees both disabled, with the roles `auth.requires` names for them. -->
<script module>
  export const uses = ["ui.versions", "ui.watch", "ui.contract", "ui.routes.list", "ui.slots.list", "ui.rollback", "ui.preview",
                       "auth.whoami", "auth.requires"];
</script>

<script>
  import { untrack } from "svelte";
  import { bus, live } from "plexus-ui/host";

  const b = bus();
  const REACHED = ["tickets.list", "epics.list", "orgs.list", "members.mine", "session.whoami", "auth.whoami", "ui.versions"];

  // FRAME-37: `bus.call` answers `{ ok, events, message }`; before it, the
  // data events (an array) or a rejection with the refusal's words.
  async function answer(asked) {
    try {
      const a = await asked;
      if (a && typeof a.ok === "boolean") return { ok: a.ok, events: a.events ?? [], message: a.message ?? "" };
      const events = Array.isArray(a) ? a : [];
      const bad = events.find((e) => e?.type === "refused" || e?.type === "error" || e?.type === "not_found");
      return { ok: !bad, events, message: bad ? (bad.message ?? `no ${bad.name ?? "such thing"} here`) : "" };
    } catch (e) {
      return { ok: false, events: [], message: e?.message ?? String(e) };
    }
  }

  const door = live({ snapshot: "auth.whoami", params: {}, key: "principal", on: {} });
  const needs = live({ snapshot: "auth.requires", params: { method: "ui.rollback" }, key: "method", on: {} });
  const stamped = live({ snapshot: "ui.contract", params: { uses: ["tickets.list"] }, key: "contract", on: {} });
  const reached = live({ snapshot: "ui.contract", params: { uses: REACHED }, key: "contract", on: {} });
  const routes = live({ snapshot: "ui.routes.list", params: {}, changes: "ui.watch", key: "pattern", on: { route_changed: "upsert:." } });
  const slots = live({ snapshot: "ui.slots.list", params: {}, key: "slot", on: {} });

  // The live pointer is its own state. `ui.watch`'s `live { version }` names
  // a hash and no row, and a collection hears a row only when it moves, so
  // the moves are heard twice over: a generation — every version, keyed by hash,
  // its `live` folded as `remove:version` — loses the version that went live
  // (a rollback, a push of bytes kept before), and `firsts` — keyed by
  // `version`, `upsert:.` — gains a version going live for the first time (a
  // push). Either sets the pointer and reads the list again: a new generation
  // (another generation's spec, so another collection, asked anew) whose
  // snapshot is the list with its `live` mark — so a move back to a version
  // already moved to is heard as well as the first. FRAME-40's refetch rule
  // will make this one spec.
  let list = $state(null);
  let pointer = $state(null);
  let added = $state(new Set());
  // The generation now: its handle, its number, and the hashes its snapshot held.
  let watcher = $state.raw(null);
  // A generation's name is this instance's own: the host keeps a collection a
  // few seconds past its last reader, and a remount (a swap) must not be
  // handed the old instance's generation with its moves folded out.
  const me = Math.random().toString(36).slice(2, 10);
  function generation(n) {
    const h = b.live({ snapshot: "ui.versions", params: {}, changes: "ui.watch", key: "hash",
                       on: { live: "remove:version", [`generation_${me}_${n}`]: "remove:none" } });
    watcher = { n, h, base: null };
  }
  generation(1);
  // Read in an effect (the face subscribes while it reads, and lets the old
  // generation go when the effect moves on to the next one).
  $effect(() => {
    const w = watcher;
    if (!w) return;
    const status = w.h.status;
    const rows = w.h.rows;
    untrack(() => {
      if (status !== "live") return;
      if (w.base == null) {
        const before = new Set((list ?? []).map((v) => v.hash));
        w.base = rows.map((r) => r.hash);
        added = new Set(list == null ? [] : w.base.filter((x) => !before.has(x)));
        list = rows;
        pointer = rows.find((r) => r.live)?.hash ?? null;
        return;
      }
      const gone = w.base.filter((x) => !rows.some((r) => r.hash === x));
      if (gone.length) moved(gone[gone.length - 1]);
    });
  });
  const firsts = live({ snapshot: "ui.versions", params: {}, changes: "ui.watch", key: "version", on: { live: "upsert:." } });
  // Each version heard going live for the first time (the watch says the
  // pointer as it opens, too); what the collection held at the first read
  // was heard before this instance.
  let firstSeen = null;
  $effect(() => {
    const rows = firsts.rows;
    untrack(() => {
      if (firstSeen == null) { firstSeen = new Set(rows.map((r) => r.version)); return; }
      const fresh = rows.filter((r) => !firstSeen.has(r.version));
      for (const r of fresh) firstSeen.add(r.version);
      if (fresh.length) moved(fresh[fresh.length - 1].version);
    });
  });
  function moved(version) {
    pointer = version;
    generation((watcher?.n ?? 0) + 1);
  }

  const versions = $derived(list ?? []);
  const cur = $derived(pointer ?? "");
  const app = $derived(versions.find((v) => v.hash === cur)?.name ?? null);
  // Who may act: the roles `auth.requires` names for ui.rollback, held anywhere
  // (the service decides where: a refusal is said on the row).
  const admin = $derived(($door.rows[0]?.memberships ?? []).some((m) => ($needs.rows[0]?.roles ?? []).includes(m.role)));
  const roles = $derived(($needs.rows[0]?.roles ?? []).join(", "));
  const short = (h) => (h ?? "").slice(0, 8);
  const when = (t) => (t ? new Date(t * 1000).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }) : "");
  const kept = (v) => v.name != null && v.name !== app;

  let confirm = $state(null);
  let answers = $state({});
  async function rollback(v) {
    const was = cur;
    confirm = null;
    answers = { ...answers, [v.hash]: { asked: true } };
    const a = await answer(b.call("ui.rollback", { to: v.hash }));
    const moved = a.events.find((e) => e?.type === "live");
    answers = { ...answers, [v.hash]: a.ok ? { live: moved?.version ?? v.hash, was } : { refused: a.message } };
    if (a.ok) pointer = moved?.version ?? v.hash;
  }
  async function preview(v) {
    const a = await answer(b.call("ui.preview", { version: v.hash }));
    answers = { ...answers, [v.hash]: a.ok ? { preview: true } : { refused: a.message } };
  }
  async function clearPreview(v) {
    const a = await answer(b.call("ui.preview", {}));
    answers = { ...answers, [v.hash]: a.ok ? {} : { refused: a.message } };
  }
</script>

<section class="ui-versions" data-surface="ui-versions">
  <div class="page-head">
    <h1>releases</h1>
    <span class="sub"><span data-field="count">{versions.length}</span> versions of the app</span>
    <span class="head-live">live <b class="hash" data-field="live">{cur ? short(cur) : "none"}</b></span>
    {#if list == null}<span class="state" data-loading="pending">pending</span>{/if}
  </div>
  {#if !admin && $door.status === "live"}
    <p class="note" data-field="needs">rollback and preview are held by {roles || "admin"}; you read</p>
  {/if}
  <div class="rv">
    <div class="panel">
      <div class="th"><span>version</span><span>bundle</span><span>from</span><span>when</span><span></span><span></span></div>
      {#each versions as v (v.hash)}
        {@const ans = answers[v.hash] ?? {}}
        <div class="tr" class:flash={added.has(v.hash)} data-id={v.hash} data-live={v.hash === cur} data-matches={$stamped.rows.some((c) => c.contract === v.contract)}>
          <span class="hash" data-field="version" title={v.hash}>{short(v.hash)}</span>
          <span data-field="name">{v.name ?? "the app"}{#if kept(v)}<span class="pill" data-field="kept" title="a bundle kept by name: never the app">kept</span>{/if}</span>
          <span class="who">{#if v.authored}<span class="pill" data-field="authored">authored</span> by <b>{v.pushed_by}</b> in the studio{:else}pushed by <b>{v.pushed_by}</b>{/if}</span>
          <span class="muted" data-field="pushed" title={v.commit ?? ""}>{when(v.pushed_at)}{v.commit ? ` · ${v.commit}` : ""}</span>
          <span>{#if v.hash === cur}<span class="pill live">live</span>{/if}</span>
          <span class="acts">
            <button class="btn" data-action="ui.preview" disabled={!admin || v.hash === cur} onclick={() => preview(v)}>preview</button>
            <button class="btn" data-action="ui.rollback" disabled={!admin || v.hash === cur || kept(v)}
                    title={v.hash === cur ? "already live" : !admin ? "the admin's" : kept(v) ? "a bundle kept by name is never the app" : "make it live"}
                    onclick={() => { confirm = v.hash; answers = { ...answers, [v.hash]: {} }; }}>rollback</button>
          </span>
          {#if confirm === v.hash}
            <div class="ans"><span class="q">make <b class="hash">{short(v.hash)}</b> live? every open page swaps to it.</span>
              <button class="btn primary" data-action="ui.rollback" data-confirm="" onclick={() => rollback(v)}>roll back</button>
              <button class="btn" data-cancel="" onclick={() => (confirm = null)}>keep {short(cur)}</button></div>
          {/if}
          {#if ans.asked}<div class="ans" data-field="answer"><span class="muted">rollback to {short(v.hash)} asked…</span></div>{/if}
          {#if ans.live}<div class="ans" data-field="answer"><span class="landed" data-answer="rolled_back">live {short(ans.was)} → {short(ans.live)} · open pages heard <code>live</code></span></div>{/if}
          {#if ans.preview}
            <div class="ans" data-field="preview"><span class="pv" data-answer="preview">preview of {short(v.hash)}: <a href="/app/" target="_blank" rel="noopener" data-preview="">/app/</a> <span class="muted">· for your sessions</span></span>
              <button class="btn" data-action="ui.preview" data-clear="" onclick={() => clearPreview(v)}>clear</button></div>
          {/if}
          {#if ans.refused}<div class="ans"><p data-refused="">refused: {ans.refused}</p></div>{/if}
        </div>
      {/each}
      {#if list == null}<div class="tr"><span data-loading="">00000000</span></div>{/if}
      {#if list != null && versions.length === 0}<p class="empty" data-empty="">No version is kept: nothing was pushed.</p>{/if}
    </div>
    <div class="side">
      <section class="panel" data-method="ui.contract">
        <h2>contract <span class="m hash" data-field="reached">{$reached.rows[0]?.contract ?? ""}</span></h2>
        <div class="st"><span>tickets</span><span class="hash" data-field="contract">{#if $stamped.status === "pending"}<span data-loading="">0000000000000000</span>{:else}{$stamped.rows[0]?.contract ?? ""}{/if}</span><span class="src">what a deploy stamps</span></div>
        <div class="st"><span>every service</span><span class="hash">{#if $reached.status === "pending"}<span data-loading="">0000000000000000</span>{:else}{$reached.rows[0]?.contract ?? ""}{/if}</span><span class="src">the services the app names</span></div>
        {#if $stamped.status === "error" || $stamped.status === "refused"}<p class="pad" data-refused="">refused: {$stamped.message}</p>{/if}
      </section>
      <section class="panel" data-method="ui.routes.list" data-field="routes">
        <h2>routes <span class="m">{routes.rows.length}</span></h2>
        {#each routes.rows as r (r.pattern)}
          <div class="st" data-id={r.pattern}><span data-field="pattern">{r.pattern}</span><span data-field="component">{r.component ?? "(the manifest's)"}</span><span class="src" data-src="set">{r.default == null ? "set" : `over ${r.default}`}</span></div>
        {/each}
        {#if $routes.status === "live" && routes.rows.length === 0}<p class="empty" data-empty="">No route set over the manifest's.</p>{/if}
      </section>
      <section class="panel" data-method="ui.slots.list" data-field="slots">
        <h2>sections <span class="m">{slots.rows.length}</span></h2>
        {#each slots.rows as s (s.layout + "/" + s.slot)}
          <div class="st four" data-id={s.layout + "/" + s.slot}><span>{s.layout}</span><span class="muted" data-field="slot">{s.slot}</span><span data-field="component">{s.component}</span><span class="src" data-src="set">{s.follow == null ? (s.default == null ? "set" : `over ${s.default}`) : `follows ${s.follow}`}</span></div>
        {/each}
        {#if $slots.status === "live" && slots.rows.length === 0}<p class="empty" data-empty="">Every section holds its default.</p>{/if}
      </section>
    </div>
  </div>
</section>

<style>
  .ui-versions { max-width: 1340px; margin: 0 auto; padding: 18px 24px 40px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .page-head { display: flex; align-items: baseline; gap: 10px; margin: 0 0 14px; }
  .page-head h1 { margin: 0; font-family: var(--font-display); font-size: 20px; font-weight: var(--weight-strong); }
  .sub { color: var(--muted); font-size: 12px; }
  .head-live { display: inline-flex; gap: 6px; align-items: center; font-size: 12px; color: var(--muted); }
  .head-live::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: var(--live); }
  .state { margin-left: auto; font-size: 11px; color: var(--muted); }
  .note { margin: 0 0 12px; font-size: 12px; color: var(--muted); }
  .rv { display: grid; grid-template-columns: minmax(0, 1fr) 400px; gap: 20px; align-items: start; }
  .panel { overflow: hidden; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .side .panel { margin-bottom: 14px; }
  .th, .tr { display: grid; grid-template-columns: 96px minmax(80px, .8fr) minmax(0, 1.6fr) minmax(0, 1fr) 52px 168px; gap: 0 10px; align-items: center; padding: 0 10px; }
  .th { padding-top: 6px; padding-bottom: 6px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); border-bottom: var(--border) solid var(--line); }
  .tr { min-height: 34px; border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .tr:last-child { border-bottom: 0; }
  .tr:hover { background: var(--hover); }
  .tr[data-live=true] { background: var(--sel); box-shadow: inset 3px 0 0 var(--accent); }
  .tr > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hash { color: var(--accent); font-variant-numeric: tabular-nums; }
  .who { color: var(--muted); }
  .who b { color: var(--ink); font-weight: var(--weight-medium); }
  .muted { color: var(--muted); }
  .acts { display: flex; gap: 4px; justify-content: flex-end; }
  .pill { display: inline-block; margin-left: 4px; padding: 0 7px; font-size: 11px; line-height: 18px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  .who .pill { margin: 0 2px 0 0; }
  .pill.live { margin: 0; color: var(--live); border-color: var(--live); background: transparent; }
  .btn { padding: 3px 10px; font-size: 12px; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: border-color var(--motion-fast) var(--ease), background-color var(--motion-fast) var(--ease); }
  .btn:hover:not(:disabled) { border-color: var(--muted); background: var(--hover); }
  .btn:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .btn:disabled { color: var(--disabled); cursor: default; }
  .btn.primary { color: var(--bg); background: var(--accent); border-color: var(--accent); }
  .btn.primary:hover:not(:disabled) { background: var(--accent); opacity: .9; }
  .ans { grid-column: 1 / -1; display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin: 0 -10px; padding: 6px 10px 10px; background: var(--panel); }
  .q { color: var(--ink); }
  .landed { display: inline-flex; gap: 6px; align-items: center; font-size: 11px; color: var(--live); }
  .landed::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: var(--live); }
  .pv a { color: var(--accent); text-decoration: underline; }
  [data-refused] { margin: 4px 0 0; font-size: 11px; color: var(--warn); }
  [data-loading] { color: transparent; background: var(--skeleton); border-radius: var(--radius-sm); }
  .pad { margin: 8px 10px; }
  .empty { margin: 8px 10px; color: var(--muted); }
  .side h2 { display: flex; gap: 8px; align-items: baseline; margin: 0; padding: 8px 10px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; border-bottom: var(--border) solid var(--line); }
  .side h2 .m { margin-left: auto; text-transform: none; letter-spacing: 0; }
  .st { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) auto; gap: 8px; align-items: center; min-height: 26px; padding: 0 8px; font-size: 12px; border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .st.four { grid-template-columns: minmax(0, 1fr) 60px minmax(0, 1fr) auto; }
  .st:last-child { border-bottom: 0; }
  .st:hover { background: var(--hover); }
  .st > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .src { font-size: 11px; color: var(--muted); }
  .src[data-src=set] { color: var(--accent); }
  @keyframes flash { from { background: var(--sel); } to { background: transparent; } }
  .flash { animation: flash var(--motion-flash) var(--ease); }
  @media (prefers-reduced-motion: reduce) { .tr, .st, .btn { transition: none; } .flash { animation: none; } }
  @media (max-width: 960px) { .rv { grid-template-columns: 1fr; } }
  @media (max-width: 720px) { .th { display: none; } .tr { grid-template-columns: 1fr auto; } .tr > span:nth-child(2), .tr > span:nth-child(3), .tr > span:nth-child(4) { display: none; } }
</style>
