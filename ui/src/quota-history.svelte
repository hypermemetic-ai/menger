<!-- V2-70: consumption over time (#/usage, and the whole system's on it for an admin of `*`), svelte/1 —
     `quota.history { principal?, everyone?, days }`: a `point { day, source, used, estimate }` per
     source per UTC day, then `end`. Two charts, never a second axis (the dataviz skill's method):
     what is stored — a line per source (files, tickets, documents) at each day's end — and what a
     day fetched and served, each its own scale. One row above both: the range (30 d · 90 d · 1 y)
     and the table view of the same points. A crosshair finds the day; its tooltip lists every
     series (keyboard: focus a chart, ← →). Colours: --chart-1..3, in a fixed order (theme.toml).
     Marks: `[data-surface=quota-history]` (`data-whose` mine | <principal> | everyone),
     `[data-range=30|90|365]` (`aria-pressed`), `[data-view=chart|table]` (`aria-pressed`), a chart
     `[data-chart=stored|day]` holding a line per series `[data-series=<source>]` (`data-points`,
     `data-last`), its legend `[data-legend=<source>]`, the tooltip `[data-tip]` (`data-day`); the
     table `[data-history-table]`, a row `[data-day=<YYYY-MM-DD>]` and its cells `[data-col=<source>]`;
     `[data-estimate]` when a day is backfilled; a refusal `[data-refused]`. -->
<script module>
  export const uses = ["quota.history"];
</script>

<script>
  import { bus } from "plexus-ui/host";
  let { principal = null, system = false, sheet = null } = $props();
  const b = bus();

  const RANGES = [[30, "30 d"], [90, "90 d"], [365, "1 y"]];
  const CHARTS = [
    { id: "stored", title: "stored", sub: "what is kept, at each day's end", series: [["media.bytes", "files", 1], ["tickets.bytes", "tickets", 2], ["docs.bytes", "documents", 3]] },
    { id: "day", title: "fetched and served", sub: "each UTC day's total", series: [["media.fetch.bytes", "fetched", 1], ["media.egress.bytes", "served", 2]] },
  ];
  /** The table's columns: every chart's series, then the day's fetches (a count, on no chart). */
  const COLUMNS = [...CHARTS.flatMap((c) => c.series.map(([s, n]) => [s, n, "bytes"])), ["media.fetch.calls", "fetches", "count"]];

  let days = $state(90);
  let view = $state("chart");
  let status = $state("pending");
  let refused = $state("");
  let loading = $state(false);
  let points = $state([]);

  async function load(d) {
    if (sheet === "loading") return;
    loading = true;
    const params = { days: d };
    if (principal) params.principal = principal;
    if (system) params.everyone = true;
    const r = await b.call("quota.history", params);
    loading = false;
    if (!r.ok) { status = "refused"; refused = String(r.message ?? "refused"); return; }
    const ev = r.events ?? [];
    const err = ev.find((e) => e.type === "error");
    if (err) { status = "refused"; refused = err.message; return; }
    points = ev.filter((e) => e.type === "point");
    status = "live";
  }
  $effect(() => { load(days); });

  const dayList = $derived([...new Set(points.map((p) => p.day))].sort());
  const at = $derived(new Map(points.map((p) => [`${p.source}|${p.day}`, p])));
  const val = (source, day) => at.get(`${source}|${day}`)?.used ?? 0;
  const estimated = $derived(points.filter((p) => p.estimate).map((p) => p.day).sort());

  /** An amount in words: bytes as 1.9 GB (powers of 1000, as the caps are set), a count as the number. */
  function amount(unit, n) {
    if (unit !== "bytes") return Number(n).toLocaleString("en");
    const units = ["B", "KB", "MB", "GB", "TB"];
    let v = Number(n), u = 0;
    while (v >= 1000 && u < units.length - 1) { v /= 1000; u += 1; }
    return u === 0 ? `${n} B` : `${(Math.round(v * 10) / 10).toString()} ${units[u]}`;
  }
  const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const short = (day) => { const [, m, d] = day.split("-"); return `${MONTHS[Number(m) - 1]} ${Number(d)}`; };
  /** A clean step for `max` over about four ticks: 1, 2 or 5 of a power of ten. */
  function niceStep(max) {
    if (max <= 0) return 1;
    const raw = max / 4, p = 10 ** Math.floor(Math.log10(raw)), f = raw / p;
    return (f <= 1 ? 1 : f <= 2 ? 2 : f <= 5 ? 5 : 10) * p;
  }

  const H = 180, ML = 64, MR = 16, MT = 10, MB = 26;
  let width = $state(640);
  const pw = $derived(Math.max(60, width - ML - MR));
  const x = (i, n) => ML + (n <= 1 ? pw : (i * pw) / (n - 1));

  function geometry(chart) {
    const n = dayList.length;
    const max = Math.max(0, ...chart.series.flatMap(([s]) => dayList.map((d) => val(s, d))));
    const step = niceStep(max);
    const top = Math.max(step, Math.ceil(max / step) * step);
    const y = (v) => MT + H - (v / top) * H;
    const ticks = [];
    for (let t = 0; t <= top + step / 2; t += step) ticks.push(t);
    const lines = chart.series.map(([source, name, slot]) => {
      const vs = dayList.map((d) => val(source, d));
      const path = vs.map((v, i) => `${i ? "L" : "M"}${x(i, n).toFixed(1)},${y(v).toFixed(1)}`).join("");
      return { source, name, slot, vs, path, last: vs.at(-1) ?? 0 };
    });
    const labels = n ? [...new Set([0, Math.floor((n - 1) / 2), n - 1])] : [];
    return { n, y, ticks, lines, labels };
  }
  const geo = $derived(Object.fromEntries(CHARTS.map((c) => [c.id, geometry(c)])));

  let hover = $state(null); // { chart, i }
  function pick(ev, chart) {
    const r = ev.currentTarget.getBoundingClientRect();
    const n = dayList.length;
    if (!n) return;
    const px = ((ev.clientX - r.left) / r.width) * (width) - ML;
    hover = { chart, i: Math.max(0, Math.min(n - 1, Math.round((px / pw) * (n - 1)))) };
  }
  function key(ev, chart) {
    const n = dayList.length;
    if (!n) return;
    const i = hover?.chart === chart ? hover.i : n - 1;
    if (ev.key === "ArrowLeft") { hover = { chart, i: Math.max(0, i - 1) }; ev.preventDefault(); }
    else if (ev.key === "ArrowRight") { hover = { chart, i: Math.min(n - 1, i + 1) }; ev.preventDefault(); }
    else if (ev.key === "Escape") hover = null;
  }
  const whose = $derived(system ? "everyone" : principal ?? "mine");
  const shown = $derived(sheet === "loading" ? "pending" : status);
</script>

<section class="quota-history" class:inset={system} data-surface="quota-history" data-whose={whose}>
  <div class="page-head">
    <h2>{system ? "the whole system over time" : "over time"}</h2>
    <span class="sub">{system ? "everyone's, summed per day" : "what you keep and send, per UTC day"}</span>
    {#if shown !== "live"}<span class="state" data-loading={shown}>{shown}</span>{/if}
  </div>
  <div class="controls" role="toolbar" aria-label="range and view">
    {#each RANGES as [d, label] (d)}
      <button type="button" class="seg" data-range={d} aria-pressed={days === d ? "true" : "false"} onclick={() => (days = d)}>{label}</button>
    {/each}
    <span class="gap"></span>
    <button type="button" class="seg" data-view="chart" aria-pressed={view === "chart" ? "true" : "false"} onclick={() => (view = "chart")}>chart</button>
    <button type="button" class="seg" data-view="table" aria-pressed={view === "table" ? "true" : "false"} onclick={() => (view = "table")}>table</button>
  </div>
  {#if shown === "pending"}
    <div class="panel"><div class="skeleton" data-loading="">media.bytes</div></div>
  {:else if shown === "live" && view === "chart"}
    <div class="charts" class:refetch={loading}>
      {#each CHARTS as chart (chart.id)}
        {@const g = geo[chart.id]}
        <figure class="panel chart" data-chart={chart.id} bind:clientWidth={width}>
          <figcaption>
            <span class="title">{chart.title}</span><span class="sub">{chart.sub}</span>
            <span class="legend">
              {#each g.lines as l (l.source)}
                <span class="key" data-legend={l.source}><i class={`line s${l.slot}`}></i>{l.name}</span>
              {/each}
            </span>
          </figcaption>
          <svg width={width} height={H + MT + MB} role="img" tabindex="0"
               aria-label={`${chart.title}: ${g.lines.map((l) => `${l.name} ${amount("bytes", l.last)}`).join(", ")} on ${dayList.at(-1) ?? "no day"}`}
               onpointermove={(e) => pick(e, chart.id)} onpointerleave={() => (hover = null)}
               onkeydown={(e) => key(e, chart.id)} onfocus={() => (hover = { chart: chart.id, i: Math.max(0, dayList.length - 1) })} onblur={() => (hover = null)}>
            {#each g.ticks as t (t)}
              <line class="grid" x1={ML} x2={ML + pw} y1={g.y(t)} y2={g.y(t)}></line>
              <text class="tick" x={ML - 8} y={g.y(t) + 4} text-anchor="end">{amount("bytes", t)}</text>
            {/each}
            {#each g.labels as i (i)}
              <text class="tick" x={x(i, g.n)} y={MT + H + 18} text-anchor={i === 0 ? "start" : i === g.n - 1 ? "end" : "middle"}>{short(dayList[i])}</text>
            {/each}
            {#each g.lines as l (l.source)}
              <path class={`series s${l.slot}`} d={l.path} data-series={l.source} data-points={l.vs.length} data-last={l.last}></path>
              {#if g.n}<circle class={`end s${l.slot}`} cx={x(g.n - 1, g.n)} cy={g.y(l.last)} r="4"></circle>{/if}
            {/each}
            {#if hover?.chart === chart.id && g.n}
              <line class="cross" x1={x(hover.i, g.n)} x2={x(hover.i, g.n)} y1={MT} y2={MT + H}></line>
              {#each g.lines as l (l.source)}
                <circle class={`end s${l.slot}`} cx={x(hover.i, g.n)} cy={g.y(l.vs[hover.i])} r="4"></circle>
              {/each}
            {/if}
          </svg>
          {#if hover?.chart === chart.id && g.n}
            {@const left = x(hover.i, g.n)}
            <div class="tip" data-tip="" data-day={dayList[hover.i]} style={left > width / 2 ? `right: ${width - left + 10}px` : `left: ${left + 10}px`}>
              <div class="tip-day">{dayList[hover.i]}{#if at.get(`${chart.series[0][0]}|${dayList[hover.i]}`)?.estimate}<span class="est">estimate</span>{/if}</div>
              {#each g.lines as l (l.source)}
                <div class="tip-row"><i class={`line s${l.slot}`}></i><b>{amount("bytes", l.vs[hover.i])}</b><span>{l.name}</span></div>
              {/each}
            </div>
          {/if}
        </figure>
      {/each}
      {#if estimated.length}<p class="note" data-estimate="">Before {estimated.at(-1)} the stored amounts are estimates — laid back from what the stores keep now, along when it was written.</p>{/if}
    </div>
  {:else if shown === "live"}
    <div class="panel table-wrap" class:refetch={loading}>
      <table data-history-table="">
        <thead><tr><th scope="col">day</th>{#each COLUMNS as [s, name] (s)}<th scope="col" title={s}>{name}</th>{/each}</tr></thead>
        <tbody>
          {#each [...dayList].reverse() as d (d)}
            <tr data-day={d}>
              <th scope="row">{d}{#if at.get(`media.bytes|${d}`)?.estimate}<span class="est" data-estimate="">est.</span>{/if}</th>
              {#each COLUMNS as [s, , unit] (s)}<td data-col={s}>{amount(unit, val(s, d))}</td>{/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
  {#if shown === "refused" || shown === "error"}<p class="empty" data-refused="">refused: {refused}</p>{/if}
</section>

<style>
  .quota-history { max-width: 980px; margin: 0 auto; padding: 18px 24px 0; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .quota-history.inset { padding: 18px 0 0; }
  .page-head { display: flex; align-items: baseline; gap: 10px; margin: 0 0 10px; }
  .page-head h2 { margin: 0; font-family: var(--font-display); font-size: 15px; font-weight: var(--weight-strong); }
  .sub { color: var(--muted); font-size: 12px; }
  .state { margin-left: auto; font-size: 11px; color: var(--muted); }
  .controls { display: flex; align-items: center; gap: 4px; margin: 0 0 10px; }
  .gap { flex: 1; }
  .seg { font: inherit; font-size: 12px; color: var(--muted); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); padding: 2px 10px; cursor: pointer; }
  .seg:hover, .seg:global([data-sheet~=hover]) { background: var(--hover); color: var(--ink); }
  .seg:focus-visible, .seg:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .seg[aria-pressed=true] { color: var(--ink); background: var(--sel); border-color: var(--accent); }
  .panel { position: relative; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .charts { display: grid; gap: 12px; transition: opacity var(--motion-fast) var(--ease); }
  .refetch { opacity: 0.5; }
  .chart { margin: 0; padding: 10px 0 4px; overflow: hidden; }
  figcaption { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 10px; padding: 0 12px 4px; }
  .title { font-weight: var(--weight-medium); }
  .legend { display: flex; gap: 12px; margin-left: auto; font-size: 12px; color: var(--ink); }
  .key { display: inline-flex; align-items: center; gap: 6px; }
  i.line { display: inline-block; width: 14px; height: 2px; border-radius: 1px; }
  i.s1 { background: var(--chart-1); }
  i.s2 { background: var(--chart-2); }
  i.s3 { background: var(--chart-3); }
  svg { display: block; }
  svg:focus-visible, svg:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: -2px; }
  .grid { stroke: var(--line); stroke-width: 1; }
  .tick { fill: var(--muted); font-family: var(--font-text); font-size: 11px; font-variant-numeric: tabular-nums; }
  .series { fill: none; stroke-width: 2; stroke-linejoin: round; stroke-linecap: round; }
  path.s1 { stroke: var(--chart-1); }
  path.s2 { stroke: var(--chart-2); }
  path.s3 { stroke: var(--chart-3); }
  circle.end { stroke: var(--panel); stroke-width: 2; }
  circle.s1 { fill: var(--chart-1); }
  circle.s2 { fill: var(--chart-2); }
  circle.s3 { fill: var(--chart-3); }
  .cross { stroke: var(--muted); stroke-width: 1; }
  .tip { position: absolute; top: 40px; pointer-events: none; min-width: 140px; padding: 6px 10px; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); box-shadow: 0 4px 14px var(--shadow); font-size: 12px; }
  .tip-day { color: var(--muted); margin-bottom: 2px; }
  .tip-row { display: grid; grid-template-columns: 14px auto 1fr; align-items: center; gap: 6px; }
  .tip-row b { color: var(--ink); font-weight: var(--weight-strong); font-variant-numeric: tabular-nums; }
  .tip-row span { color: var(--muted); }
  .est { margin-left: 6px; padding: 0 5px; font-size: 10px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  .note { margin: 0; color: var(--muted); font-size: 12px; }
  .table-wrap { max-height: 420px; overflow: auto; }
  table { width: 100%; border-collapse: collapse; font-size: 12px; font-variant-numeric: tabular-nums; }
  th, td { padding: 4px 10px; text-align: right; border-bottom: var(--border) solid var(--line); white-space: nowrap; }
  th:first-child { text-align: left; }
  thead th { position: sticky; top: 0; background: var(--panel); color: var(--muted); font-weight: var(--weight-medium); }
  tbody th { font-weight: normal; color: var(--ink); }
  .skeleton { height: 120px; margin: 10px; color: transparent; background: var(--skeleton); border-radius: var(--radius-sm); }
  .empty { margin: 8px 10px; color: var(--muted); }
  [data-refused] { color: var(--warn); }
  @media (prefers-reduced-motion: reduce) { .charts { transition: none; } }
</style>
