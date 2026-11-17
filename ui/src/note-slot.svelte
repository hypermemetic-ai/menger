<!-- NOTES-4: a note's slot (svelte/1) — what is built from the note, on its
     right (the owner, 2026-11-15: "every note associated with a plexus-ui slot
     on it's right"), placed by editor-screen beside the note. The component
     that fills it is drawn by a bound mark pinned to its version
     (`data-slot="fill" data-component data-version`, plexus-ui's CONTRACT.md
     "A bound mark"), given the note's id (`data-note` → its prop `note`) and
     the slot's props as JSON text (`data-props` → its prop `props`: a mark's
     props are strings, so the component reads `JSON.parse(props)`); new
     props reach the mounted component in place (`update(props)`), a new fill
     or version is the old one taken out and the new one mounted. Above it,
     the component's name, which fill it is, who built it and when; while an
     agent builds the next one (`notes.building`), its steps — heard, written,
     compiled, drawn — and its line, the component that is there still drawn
     and usable; a failed build's refusal under it. Under it every fill,
     newest first: a click draws that version on this page alone (‹ › step
     through them), nothing kept moves. Live by `notes.slot` and
     `notes.watch` (`slot_changed`, `building_changed`, `props_changed`, each
     carrying the slot whole). -->
<script module>
  export const uses = ["notes.slot", "notes.watch"];
  /** The steps a build shows, in order; `drawn` is the fill. */
  export const STEPS = ["heard", "written", "compiled", "drawn"];
  /** Each step's state for a build: reached, failed (the one after the last reached), or to come. */
  export function stepsOf(b) {
    const last = b?.step ? STEPS.indexOf(b.step) : -1;
    return STEPS.map((name, i) => ({ name, state: i <= last ? "reached" : b?.failed && i === last + 1 ? "failed" : "next" }));
  }
</script>

<script>
  import { bus, live } from "plexus-ui/host";

  let { note } = $props();
  const b = bus();
  const slotOf = live({ snapshot: "notes.slot", params: () => ({ id: note }), changes: "notes.watch", changes_params: () => ({ id: note }), key: "id",
                        on: { slot_changed: "upsert:slot", building_changed: "upsert:slot", props_changed: "upsert:slot" } });
  const slot = $derived(slotOf.rows.find((r) => r.id === note && Array.isArray(r.fills)) ?? null);
  const fills = $derived(slot?.fills ?? []);
  const building = $derived(slot?.building ?? null);
  // An earlier fill picked on this page alone; a new fill takes the page to it.
  let picked = $state(null);
  let newest = $state(null);
  $effect(() => { const n = slot?.fill?.n ?? null; if (n !== newest) { newest = n; picked = null; } });
  const shown = $derived(picked != null ? (fills.find((f) => f.n === picked) ?? slot?.fill ?? null) : (slot?.fill ?? null));
  const props = $derived(JSON.stringify(slot?.props ?? {}));
  const order = $derived(fills.map((f) => f.n).sort((x, y) => x - y));
  const step = (by) => { const at = order.indexOf(shown?.n); const to = order[at + by]; if (to != null) pick(to); };
  const pick = (n) => { picked = n === slot?.fill?.n ? null : n; };
  const hm = (ms) => new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: false });
  const next = $derived((fills[0]?.n ?? 0) + 1);
  let copied = $state(false);
  async function copy() {
    try { await navigator.clipboard.writeText(`note ${note}`); copied = true; } catch { copied = false; }
  }
</script>

<section class="note-slot" data-field="slot" data-note={note} data-state={building?.failed ? "failed" : building ? "building" : shown ? "filled" : slotOf.status === "pending" ? "pending" : "empty"} aria-label="the note's slot">
  <header class="head">
    <b class="label">slot</b>
    {#if shown}
      <span class="name" data-field="component">{shown.component}</span>
      <span class="meta" data-field="which">v{shown.n} of {fills.length} · built {hm(shown.at)} by {shown.by}</span>
      {#if picked != null}<span class="pill" data-field="mine">this page only</span>{/if}
    {:else if slotOf.status !== "pending"}
      <span class="meta">empty</span>
    {/if}
    {#if building}
      <span class="pill" data-field="building" data-failed={building.failed ? "" : undefined}>{building.failed ? `v${next} did not compile` : `building v${next}`}</span>
    {/if}
    {#if fills.length > 1}
      <span class="nav">
        <button type="button" class="btn" data-action="earlier" aria-label="the fill before" disabled={order.indexOf(shown?.n) <= 0} onclick={() => step(-1)}>‹</button>
        <button type="button" class="btn" data-action="later" aria-label="the fill after" disabled={order.indexOf(shown?.n) >= order.length - 1} onclick={() => step(1)}>›</button>
      </span>
    {/if}
  </header>

  {#if building}
    <ol class="steps" data-field="steps" aria-label="the build">
      {#each stepsOf(building) as s (s.name)}
        <li data-step={s.name} data-state={s.state}><i aria-hidden="true"></i>{s.state === "failed" ? (s.name === "compiled" ? "did not compile" : `not ${s.name}`) : s.name}</li>
      {/each}
      {#if building.say && !building.failed}<li class="say" data-field="say">{building.say}</li>{/if}
    </ol>
  {/if}

  {#if shown}
    <div class="card" data-field="component-card">
      <div class="fill" data-slot="fill" data-component={shown.component} data-version={shown.version} data-note={note} data-props={props}></div>
      {#if building?.failed}
        <div class="refusal" data-field="refusal" role="alert">
          <p class="why">{building.say ?? "the build failed"}</p>
          <p class="stays">v{shown.n} stays in the slot. {building.by} has the error.</p>
        </div>
      {/if}
    </div>
  {:else if slotOf.status !== "pending"}
    <div class="empty" data-empty>
      {#if building?.failed}<p class="refusal-line" data-field="refusal" role="alert">{building.say ?? "the build failed"}</p>{/if}
      <p class="lead">Nothing is built here yet.</p>
      <p class="muted">An agent that listens to this note builds a component into this slot, and builds it again as the note changes.</p>
      <p class="chips"><span class="chip" data-field="note-id">note {note}</span><button type="button" class="chip" data-action="copy" onclick={copy}>{copied ? "copied" : "copy for an agent"}</button></p>
    </div>
  {/if}

  {#if fills.length || building}
    <div class="list" data-field="fills">
      <p class="list-head"><b>built here</b> {shown?.component ?? building?.component} · {fills.length} {fills.length === 1 ? "version" : "versions"}{fills.length > 1 ? " · pick one to draw it" : ""}</p>
      {#if building}
        <div class="row building" data-row="building" data-failed={building.failed ? "" : undefined}>
          <span class="t">{hm(building.started)}</span><span class="v">v{next}</span><span class="s">{building.failed ? "did not compile" : (building.say ?? building.component)}</span><span class="by">{building.failed ? "failed" : `${building.step ?? "starting"}…`}</span>
        </div>
      {/if}
      {#each fills as f (f.n)}
        <button type="button" class="row" data-fill={f.n} aria-pressed={f.n === shown?.n} data-current={f.n === shown?.n ? "" : undefined} onclick={() => pick(f.n)}>
          <span class="t">{hm(f.at)}</span><span class="v">v{f.n}</span><span class="s">{f.say ?? f.component}</span><span class="by">{f.by}</span>
        </button>
      {/each}
    </div>
  {/if}
</section>

<style>
  .note-slot { display: flex; flex-direction: column; gap: 10px; min-height: 100%; box-sizing: border-box; padding: 16px 20px 24px; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .head { display: flex; flex-wrap: wrap; gap: 8px; align-items: baseline; font-family: var(--mono); font-size: 12px; color: var(--muted); }
  .label { font-family: var(--font-text); font-weight: var(--weight-strong); color: var(--ink); }
  .name { color: var(--ink); }
  .pill { padding: 0 6px; font-size: 11px; line-height: 17px; color: var(--ink); border: var(--border) dashed var(--line); border-radius: var(--radius-sm); }
  .pill[data-failed] { color: var(--err-fg); border-style: solid; border-color: var(--err-fg); }
  .nav { display: inline-flex; gap: 6px; margin-left: auto; }
  .btn { min-width: 26px; padding: 1px 8px; font: inherit; color: var(--ink); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; }
  .btn:hover, .btn:global([data-sheet~=hover]) { background: var(--hover); }
  .btn:focus-visible, .btn:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .btn:disabled, .btn:global([data-sheet~=disabled]) { color: var(--disabled); cursor: not-allowed; }
  .steps { display: flex; flex-wrap: wrap; gap: 6px 14px; align-items: center; margin: 0; padding: 0; list-style: none; font-family: var(--mono); font-size: 11px; color: var(--muted); }
  .steps li { display: inline-flex; gap: 6px; align-items: center; }
  .steps i { display: inline-block; width: 7px; height: 7px; border: var(--border) solid var(--muted); border-radius: 50%; }
  .steps li[data-state=reached] { color: var(--ink); }
  .steps li[data-state=reached] i { background: var(--ink); border-color: var(--ink); }
  .steps li[data-state=failed] { color: var(--err-fg); }
  .steps li[data-state=failed] i { background: var(--err-fg); border-color: var(--err-fg); }
  .steps .say { color: var(--ink); font-family: var(--font-text); font-size: 12px; }
  .card { display: flex; flex-direction: column; min-height: 320px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); overflow: hidden; }
  .fill { flex: 1; min-height: 320px; min-width: 0; display: flex; }
  .fill > :global(*) { flex: 1; min-width: 0; }
  .refusal { padding: 10px 12px; font-family: var(--mono); font-size: 11px; border-top: var(--border) solid var(--err-fg); }
  .why, .refusal-line { margin: 0 0 4px; color: var(--err-fg); white-space: pre-wrap; overflow-wrap: anywhere; font-family: var(--mono); font-size: 11px; }
  .stays { margin: 0; color: var(--muted); }
  .empty { display: flex; flex: 1; flex-direction: column; align-items: center; justify-content: center; min-height: 320px; padding: 24px; text-align: center; background: var(--panel); border: var(--border) dashed var(--line); border-radius: var(--radius-lg); }
  .lead { margin: 0 0 6px; font-size: 15px; }
  .muted { max-width: 34ch; margin: 0 0 14px; font-family: var(--mono); font-size: 12px; color: var(--muted); }
  .chips { display: flex; flex-wrap: wrap; gap: 8px; justify-content: center; margin: 0; }
  .chip { padding: 2px 9px; font-family: var(--mono); font-size: 12px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); }
  button.chip { cursor: pointer; }
  button.chip:hover, button.chip:global([data-sheet~=hover]) { background: var(--hover); }
  button.chip:focus-visible, button.chip:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .list { display: flex; flex-direction: column; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); overflow: hidden; }
  .list-head { margin: 0; padding: 8px 12px; font-family: var(--mono); font-size: 11px; color: var(--muted); border-bottom: var(--border) solid var(--line); }
  .list-head b { font-family: var(--font-text); color: var(--ink); }
  .row { display: grid; grid-template-columns: 48px 32px minmax(0, 1fr) auto; gap: 12px; align-items: baseline; width: 100%; padding: 6px 12px; font: inherit; font-size: 13px; text-align: left; color: var(--ink); background: none; border: 0; border-top: var(--border) solid var(--line); cursor: pointer; }
  .row:first-of-type, .list-head + .row { border-top: 0; }
  .row:hover, .row:global([data-sheet~=hover]) { background: var(--hover); }
  .row:focus-visible, .row:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: -2px; }
  .row[data-current], .row:global([data-sheet~=selected]) { background: var(--row-sel); }
  .row.building { cursor: default; }
  .row.building:hover { background: none; }
  .t, .v, .by { font-family: var(--mono); font-size: 12px; color: var(--muted); }
  .v { color: var(--ink); }
  .s { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row[data-failed] .v, .row[data-failed] .by { color: var(--err-fg); }
  @media (max-width: 759px) {
    .note-slot { padding: 12px; }
    .nav { margin-left: 0; }
    .row { grid-template-columns: 44px 26px minmax(0, 1fr) auto; gap: 8px; }
  }
</style>
