<!-- SURFACE-7: the order of work as lanes, left to right — `work.waves
     { epics, kinds }` at #/waves/in/:epics (#/waves offers the epic's
     shortcodes, `nav groups`, each a link that adds or takes it),
     `waves_under { id }` at #/waves/:id, `waves_to { id }` at
     #/waves/to/:to; the kinds toggled in the head. Each `placed` is a chip in
     its wave's lane (`[data-wave=N]`; the missing-blocker lane after them, the
     done counted and not drawn); each ticket's `waiting_on` a line from the
     blocker's chip into its own — CSS alone: a rotated hairline whose ends
     come from the lanes' fixed pitch (a lane 226 px wide and 56 px apart, a
     chip 26 px tall and 8 px apart; the sheet holds both). A hovered chip
     lights its lines and the chips they join; a click opens the ticket. A
     route or a kind moved asks again. -->
<script module>
  const KINDS = ["scope", "execution", "build", "gap", "spike"];
  // The pitch the sheet draws at (.lanes, .lane, .lane-head, .chip).
  const PAD_X = 24, PAD_Y = 16, LANE = 226, GAP = 56, HEAD = 34, CHIP = 26, CHIP_GAP = 8;
  const laneOf = (r) => (r.placement?.type === "wave" ? String(r.placement.n) : r.placement?.type === "done" ? null : "missing");
  export const uses = ["tickets.nav.groups", "tickets.work.waves", "tickets.work.waves_under", "tickets.work.waves_to"];
</script>

<script>
  import { live } from "plexus-ui/host";

  let { epics = null, id = null, to = null, sheet = null } = $props();
  let kinds = $state(["build", "gap", "spike"]);
  let hot = $state("");

  const chosen = $derived(epics ? epics.split(",").filter(Boolean) : []);
  const mode = $derived(to ? "to" : id ? "under" : chosen.length ? "in" : "pick");
  const target = $derived(mode === "to" ? to : id);

  const groups = live({ snapshot: "tickets.nav.groups", params: {}, key: "shortcode", on: { group: "upsert:group" },
                        when: () => mode === "in" || mode === "pick" });
  // The rows are the answer's events less `type`: `placed { summary,
  // placement, waiting_on }`, `wave_cycle { ids }`, `not_found { id }`. The
  // spec's key is consulted only by a fold of a changes stream, which these
  // reads have none of; it is `id`, never the summary object, and each placed
  // row is given its `id` (the summary's) below, so a fold added later keys
  // on the ticket.
  const all = live({ snapshot: "tickets.work.waves", params: () => ({ epics: chosen, kinds: [...kinds] }),
                     key: "id", when: () => mode === "in", on: { placed: "upsert:.", wave_cycle: "upsert:.", not_found: "upsert:." } });
  const under = live({ snapshot: "tickets.work.waves_under", params: () => ({ id, kinds: [...kinds] }),
                       key: "id", when: () => mode === "under", on: { placed: "upsert:.", wave_cycle: "upsert:.", not_found: "upsert:." } });
  const upto = live({ snapshot: "tickets.work.waves_to", params: () => ({ id: to, kinds: [...kinds] }),
                      key: "id", when: () => mode === "to", on: { placed: "upsert:.", wave_cycle: "upsert:.", not_found: "upsert:." } });
  const C = $derived(mode === "to" ? upto : mode === "under" ? under : all);
  const status = $derived(mode === "pick" ? "live" : C.status);
  const rows = $derived(mode === "pick" ? [] : C.rows);
  const placed = $derived(rows.filter((r) => r.summary).map((r) => ({ ...r, id: r.summary.id })));
  const missing = $derived(rows.find((r) => !r.summary && !r.ids && r.id != null)?.id ?? null);
  const cycles = $derived(rows.filter((r) => r.ids));

  // The lanes in the answer's order (waves ascending, then the missing), each
  // chip's place in its lane, and the lines between them.
  const G = $derived.by(() => {
    const lanes = [], by = new Map(), pos = new Map();
    let done = 0;
    for (const r of placed) {
      const k = laneOf(r);
      if (k === null) { done++; continue; }
      if (!by.has(k)) { by.set(k, []); lanes.push(k); }
      pos.set(r.summary.id, { l: lanes.indexOf(k), j: by.get(k).length });
      by.get(k).push(r);
    }
    const edges = [];
    for (const r of placed) {
      const p = pos.get(r.summary.id);
      if (!p) continue;
      for (const w of r.waiting_on ?? []) {
        const q = pos.get(w);
        if (!q || q.l >= p.l) continue;
        const x1 = PAD_X + q.l * (LANE + GAP) + LANE, y1 = PAD_Y + HEAD + q.j * (CHIP + CHIP_GAP) + CHIP / 2;
        const x2 = PAD_X + p.l * (LANE + GAP) - 2, y2 = PAD_Y + HEAD + p.j * (CHIP + CHIP_GAP) + CHIP / 2;
        const dx = x2 - x1, dy = y2 - y1;
        edges.push({ from: w, to: r.summary.id, style: `left:${x1}px;top:${y1}px;width:${Math.hypot(dx, dy).toFixed(1)}px;transform:rotate(${Math.atan2(dy, dx).toFixed(4)}rad)` });
      }
    }
    return { lanes, by, edges, done };
  });
  const touches = (r) => hot !== "" && hot !== r.summary.id && ((r.waiting_on ?? []).includes(hot) || (r.summary.unlocks ?? []).includes(hot));

  function toggled(code) {
    const next = chosen.includes(code) ? chosen.filter((c) => c !== code) : [...chosen, code];
    return next.length ? "#/waves/in/" + next.join(",") : "#/waves";
  }
  function kind(k) {
    kinds = kinds.includes(k) ? kinds.filter((x) => x !== k) : [...kinds, k];
  }
</script>

<section class="work-waves" data-surface="work-waves">
  <header class="ww-head" data-mode={mode}>
    <h1>waves</h1>
    <span class="sum" data-field="summary">{mode === "pick" ? "choose shortcodes" : `${placed.length - G.done} tickets in ${G.lanes.length} waves${G.done ? ` · ${G.done} done` : ""}`}</span>
    {#if mode === "in" || mode === "pick"}
      <span class="ctl" data-param="epics"><span>epics</span>
        {#each groups.rows as g (g.shortcode)}
          <a class="tog" data-epic={g.shortcode} aria-pressed={chosen.includes(g.shortcode) ? "true" : "false"} href={toggled(g.shortcode)}>{g.shortcode}</a>
        {/each}
      </span>
    {:else}
      <span class="ctl routes" data-param="route"><span>{mode}</span>
        <a class="tog" data-route="under" aria-pressed={mode === "under" ? "true" : "false"} href={"#/waves/" + target}>under {target}</a>
        <a class="tog" data-route="to" aria-pressed={mode === "to" ? "true" : "false"} href={"#/waves/to/" + target}>to {target}</a>
        <a class="tog" data-route="all" aria-pressed="false" href="#/waves">by shortcode</a>
      </span>
    {/if}
    <span class="ctl" data-param="kinds"><span>kinds</span>
      {#each KINDS as k (k)}
        <button class="tog" data-kind={k} aria-pressed={kinds.includes(k) ? "true" : "false"} onclick={() => kind(k)}>{k}</button>
      {/each}
    </span>
  </header>
  {#if status === "error" || status === "refused"}
    <p class="ww-refused" data-refused role="alert">{C.message ?? "refused"}</p>
  {/if}
  {#each cycles as cy}
    <p class="ww-cycle" data-field="cycle">a cycle: {cy.ids.join(" → ")}</p>
  {/each}
  <div class="ww-scroll">
    {#if mode === "pick"}
      <p class="ww-empty" data-empty>Choose the shortcodes whose order of work to draw.</p>
    {:else if sheet === "loading" || status === "pending"}
      <div class="lanes" data-loading aria-busy="true"><div class="lane skel"><i></i><i></i><i></i></div><div class="lane skel"><i></i><i></i></div></div>
    {:else if status === "live" && G.lanes.length === 0}
      {#if missing != null}
        <p class="ww-empty" data-empty data-not-found={missing}>No ticket {missing} here.</p>
      {:else}
        <p class="ww-empty" data-empty>{mode === "in" ? `Nothing waits in ${chosen.join(", ")} for these kinds.` : mode === "under" ? `Nothing under ${target} waits, for these kinds.` : `Nothing waits before ${target}, for these kinds.`}</p>
      {/if}
    {:else if G.lanes.length}
      <div class="lanes" data-dim={hot ? "1" : "0"} role="presentation" onmouseleave={() => (hot = "")}>
        {#each G.edges as e (e.from + ">" + e.to)}
          <i class="edge" data-from={e.from} data-to={e.to} data-on={hot && (hot === e.from || hot === e.to) ? "1" : "0"} style={e.style} aria-hidden="true"></i>
        {/each}
        {#each G.lanes as k (k)}
          <section class="lane" data-wave={k}>
            <h2 class="lane-head">{#if k === "missing"}missing blocker{:else}wave <b>{k}</b>{/if} · {G.by.get(k).length}</h2>
            <div class="lane-chips">
              {#each G.by.get(k) as r (r.id)}
                <a class="chip" data-id={r.summary.id} data-status={r.summary.status} data-kind={r.summary.kind}
                   data-on={hot === r.summary.id ? "1" : "0"} data-rel={touches(r) ? "1" : "0"}
                   href={"#/ticket/" + r.summary.id} title={`${r.summary.id} ${r.summary.title} · ${r.summary.status}`}
                   onmouseenter={() => (hot = r.summary.id)} onfocus={() => (hot = r.summary.id)} onblur={() => { if (hot === r.summary.id) hot = ""; }}>
                  <i class="kmark" data-kind={r.summary.kind} aria-hidden="true"></i>
                  <span class="chip-code">{r.summary.id}</span>
                  <span class="chip-title" data-field="title">{r.summary.title}</span>
                  <i class="st" data-field="status"></i>
                </a>
              {/each}
            </div>
          </section>
        {/each}
      </div>
    {/if}
  </div>
</section>

<style>
  .work-waves { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; height: 100%; background: var(--bg); color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .ww-head { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 18px; padding: 10px 24px; background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .ww-head h1 { margin: 0; font-family: var(--font-display); font-size: 18px; font-weight: var(--weight-strong); }
  .sum { color: var(--muted); font-size: 12px; }
  .ctl { display: flex; flex-wrap: wrap; gap: 4px; align-items: center; font-size: 11px; color: var(--muted); }
  .ctl > span { margin-right: 2px; }
  .routes { margin-left: auto; }
  .tog { padding: 1px 9px; font-size: 11px; color: var(--muted); text-decoration: none; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .tog:hover, .tog:global([data-sheet~=hover]) { color: var(--ink); border-color: var(--muted); }
  .tog[aria-pressed=true], .tog:global([data-sheet~=checked]) { color: var(--accent); border-color: var(--accent); background: var(--sel); }
  .tog:focus-visible, .tog:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .ww-refused { margin: 0; padding: 6px 24px; font-size: 11px; color: var(--warn); }
  .ww-cycle { margin: 0; padding: 4px 24px; font-size: 11px; color: var(--warn); }
  .ww-scroll { flex: 1; min-height: 0; overflow: auto; position: relative; }
  .ww-empty { margin: 0; padding: 40px 24px; color: var(--muted); }
  .lanes { position: relative; display: flex; gap: 56px; padding: 16px 24px 24px; min-width: max-content; }
  .lane { position: relative; z-index: 1; width: 226px; flex: none; }
  .lane-head { box-sizing: border-box; display: flex; gap: 6px; align-items: baseline; height: 24px; margin: 0 0 10px; padding: 0 2px 6px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; border-bottom: var(--border) solid var(--line); }
  .lane-head b { color: var(--ink); font-weight: var(--weight-bold); letter-spacing: 0; }
  .lane[data-wave=missing] .lane-head { color: var(--warn); }
  .lane-chips { display: flex; flex-direction: column; gap: 8px; }
  .lane.skel i { display: block; height: 26px; margin: 0 0 8px; background: var(--skeleton); border-radius: var(--radius-pill); }
  .chip { box-sizing: border-box; display: flex; gap: 6px; align-items: center; width: 100%; height: 26px; padding: 0 10px 0 8px; font-size: 12px; white-space: nowrap; color: var(--chip-fg); text-decoration: none; background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); cursor: pointer; transition: background-color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease), opacity var(--motion-fast) var(--ease); }
  .chip:hover, .chip:global([data-sheet~=hover]) { background: var(--hover); border-color: var(--line); }
  .chip:focus-visible, .chip:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .chip[data-on="1"] { border-color: var(--accent); background: var(--sel); }
  .chip[data-rel="1"] { border-color: var(--accent); }
  .lanes[data-dim="1"] .chip[data-on="0"][data-rel="0"] { opacity: .45; }
  .chip-code { flex: none; color: var(--accent); }
  .chip-title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; color: var(--ink); }
  .edge { position: absolute; z-index: 0; height: 0; border-top: 1.5px solid var(--tree-line); transform-origin: 0 0; pointer-events: none; transition: border-color var(--motion-fast) var(--ease); }
  .edge::after { content: ""; position: absolute; right: -1px; top: -4.5px; border: 4px solid transparent; border-left: 6px solid var(--tree-line); border-right: 0; }
  .edge[data-on="1"] { z-index: 2; border-top: 2px solid var(--accent); }
  .edge[data-on="1"]::after { border-left-color: var(--accent); }
  .lanes[data-dim="1"] .edge[data-on="0"] { opacity: .35; }
  .kmark { display: inline-block; width: 8px; height: 8px; flex: none; background: var(--kind-build); border-radius: 50%; }
  .kmark[data-kind=scope] { background: var(--kind-scope); border-radius: var(--radius-xs); transform: rotate(45deg) scale(.85); }
  .kmark[data-kind=gap] { background: var(--kind-gap); border-radius: 0; clip-path: polygon(50% 0, 100% 100%, 0 100%); }
  .kmark[data-kind=spike] { background: none; border: 2px solid var(--kind-spike); }
  .kmark[data-kind=doc] { width: 7px; height: 9px; background: none; border: var(--border) solid var(--kind-doc); border-radius: var(--radius-xs); }
  .kmark[data-kind=execution] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  .st { flex: none; width: 6px; height: 6px; border-radius: 50%; background: var(--st-draft-fg); }
  .chip[data-status=Pending] .st { background: var(--st-pending-fg); }
  .chip[data-status=Ready] .st { background: var(--st-ready-fg); }
  .chip[data-status=Coding] .st { background: var(--st-coding-fg); }
  .chip[data-status="In Review"] .st { background: var(--st-in-review-fg); }
  .chip[data-status=Complete] .st { background: var(--st-complete-fg); }
  .chip[data-status="Blocked"] .st { background: var(--st-blocked-fg); }
  .chip[data-status="Superseded"] .st { background: var(--st-superseded-fg); }
  .chip[data-status="Canceled"] .st { background: var(--st-canceled-fg); }
  .chip[data-status="Epic"] .st { background: var(--st-epic-fg); }
  .chip[data-status="Doc"] .st { background: var(--st-doc-fg); }
  @media (prefers-reduced-motion: reduce) {
    .tog, .chip, .edge { transition: none; }
  }
</style>
