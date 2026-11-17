<!-- LAYERS-20: a real activation inside a menger component — plexus-examples' kiln (Rust, its log in SQLite),
     compiled to wasm and run in a worker of this tab, reached through its generated client as frames.
     menger's server serves this component and takes no other part: `uses` is empty, and nothing here
     calls the bus. A component with no route of its own (owner, 2026-11-16: "we don't want a page, just
     a component"): it is shown where it is placed — a layout's section (`ui slots set --layout page-shell
     --slot main --component kiln-live`, as tests/pages/kiln.mjs does) or another component's bound mark.
     `sheet`: "sample" draws a firing, a refusal and a heat with no kiln started (the sheet's); "loading"
     draws the start.

     The generated part is ui/src/kiln-activation.svelte (scripts/kiln-activation; never edited). It is
     NOT imported as a sibling — a sibling is bundled into its importer and kept as its own module too, so
     the kiln would ship twice (LAYERS-12). This component imports that file's own module by URL, once,
     reads its page side from globalThis[Symbol.for("menger.kiln-activation")], and starts the worker from
     the same URL: `/app/<version>/components/kiln-activation.js`, the version's path the page's import map
     names (a worker has no import map, so its `./svelte-runtime.js` resolves only beside that path). The
     page asks it with a `#page` fragment, which no import-map key matches, so the page and the worker
     fetch the same URL and the browser's cache answers the second.

     LAYERS-30 (owner, 2026-11-16: "give the kiln component a pretty user interface :)"): the kiln drawn,
     its window the colour of the last reading; the firing as an SVG curve over the firing's two hours,
     growing a reading at a time, its phases said under it and the place the page left the stream
     flagged; the cone on a scale of 1 to 14 with the kiln's limit (10) marked; the log as rows; a
     refusal as a notice beside the controls; the frames as a tape, → and ← told apart. Laid out by its
     own width (a container query, two columns from 760 px), on the theme's tokens alone: heat is a
     `color-mix` between them, never a literal. Reduced motion: nothing moves.

     Marks: `[data-surface=kiln-live]` (`data-state`: starting | running | failed; `data-store`: file |
     memory), `[data-field=interface|store|log|readings|frames|heat-left]`, `[data-action=fire|heat]`,
     `input[data-field=cone]`, `[data-refused]` (the code), `[data-firing=<id>]`. -->
<script module>
  export const uses = [];
</script>

<script>
  let { sheet = null } = $props();

  let state = $state("starting");
  let failure = $state("");
  let iface = $state("");
  let store = $state(null);
  let firings = $state([]);
  let cone = $state(6);
  let firing = $state(false);
  let refused = $state(null);
  let readings = $state([]);
  let heating = $state(false);
  let left = $state("");
  let frames = $state([]);
  let fresh = $state(null);
  let whatOpen = $state(false); // narrow: the paragraph about what runs here, two lines until opened
  let kiln = null;
  let seq = 0;

  const READINGS = 12; // heat is left after this many: the stream is cancelled, not run out
  const HOURS = 2; // every call here asks a two-hour firing
  const CONES = Array.from({ length: 14 }, (_, i) => i + 1);
  const LIMIT = 10;
  const text = new TextDecoder();
  const uid = Math.random().toString(36).slice(2, 8);

  /** This bundle's kiln-activation module at its version's path. */
  function activationUrl() {
    let map = {};
    try { map = JSON.parse(document.getElementById("plexus-ui-files")?.textContent || "{}").imports || {}; } catch {}
    const key = Object.keys(map).find((k) => /^\/app\/[^/]+\/components\/kiln-activation\.js$/.test(k));
    return new URL(key ?? "./kiln-activation.js", key ? location.href : import.meta.url).href;
  }

  const frameOf = (line) => ({ n: ++seq, line, out: line.startsWith("→") });
  function tap(from, frame) {
    const line = (from === "caller" ? "→ " : "← ") + text.decode(frame).slice(0, 110);
    frames = [...frames.slice(-15), frameOf(line)];
  }

  async function start() {
    try {
      const url = activationUrl();
      await import(/* the page's side; the fragment keeps the import map from redirecting it */ url + "#page");
      const side = globalThis[Symbol.for("menger.kiln-activation")];
      if (!side) throw new Error(`${url} put no kiln on the page`);
      kiln = await side.startKiln(url, tap);
      iface = kiln.interface;
      store = kiln.store;
      firings = await kiln.client.log();
      state = "running";
    } catch (e) {
      failure = String(e?.message ?? e);
      state = "failed";
    }
  }
  if (sheet === "sample") {
    state = "running";
    iface = "abad598999eaf1bc";
    store = { kept: true };
    firings = [{ id: 1, cone: 6, hours: 2, loadedBy: "you" }];
    refused = { code: "too-hot", payload: { tag: "too-hot", val: 11 } };
    readings = [0, 5, 10].map((minute, i) => ({ minute, celsius: 20 + 180 * i, phase: "climbing" }));
    left = "left after 3 readings (the sheet's own: no kiln runs here)";
    frames = ['→ [3,1,["kiln.heat",{"cone":6,"hours":2}]]', '← [3,{"minute":0,"celsius":20,"phase":"climbing"}]', "→ [3,5,null]"].map(frameOf);
  } else if (sheet == null) start();
  $effect(() => () => kiln?.worker.terminate());

  async function fire() {
    if (!kiln) return;
    refused = null;
    firing = true;
    const before = firings.length;
    try {
      await kiln.client.fire(Number(cone), HOURS, "you");
    } catch (e) {
      refused = e instanceof kiln.PlexusFailure ? { code: e.code, payload: e.payload } : { code: "error", payload: String(e?.message ?? e) };
    }
    firings = await kiln.client.log();
    if (firings.length > before) {
      const id = firings.at(-1)?.id;
      fresh = id;
      setTimeout(() => { if (fresh === id) fresh = null; }, 2000);
    }
    firing = false;
  }

  async function heat() {
    if (!kiln) return;
    heating = true;
    readings = [];
    left = "";
    for await (const r of kiln.client.heat(Number(cone), HOURS)) {
      readings = [...readings, r];
      if (readings.length >= READINGS) break;
    }
    left = `left after ${readings.length} readings: the page sent the kiln a cancel (→ [call,5,null] below); a reading already on its way may still cross after it`;
    heating = false;
  }

  /** Mixes of the theme's tokens, never a literal: the build's colour lint reads every color-mix in a sheet as one, so
   *  the mixes are set here, on the section, and the sheet paints with var(--kl-…) alone. */
  const MIX = [
    "--kl-cold: color-mix(in oklab, var(--ink) 24%, var(--panel))",
    "--kl-ember: color-mix(in oklab, var(--warn) 78%, var(--kl-cold))",
    "--kl-hot: color-mix(in oklab, var(--chart-2) 52%, var(--sev-medium))",
    "--kl-ping: color-mix(in oklab, var(--chart-2) 55%, transparent)",
    "--kl-warn-soft: color-mix(in oklab, var(--warn) 30%, var(--hover))",
    "--kl-warn-mid: color-mix(in oklab, var(--warn) 45%, var(--panel))",
    "--kl-warn-line: color-mix(in oklab, var(--warn) 35%, transparent)",
    "--kl-accent-hover: color-mix(in oklab, var(--accent) 86%, var(--ink))",
    "--kl-in-bg: color-mix(in oklab, var(--chart-3) 6%, transparent)",
  ].join("; ");

  const step = (by) => { cone = Math.min(14, Math.max(1, (Number(cone) || 1) + by)); };

  // ── heat as colour: a ramp between the theme's tokens, mixed by the reading ──
  const RAMP = [[20, "var(--kl-cold)"], [520, "var(--kl-ember)"], [860, "var(--chart-2)"], [1280, "var(--kl-hot)"]];
  function heatColour(c) {
    if (c == null || c <= RAMP[0][0]) return RAMP[0][1];
    for (let i = 1; i < RAMP.length; i++) {
      const [b, to] = RAMP[i], [a, from] = RAMP[i - 1];
      if (c <= b) return `color-mix(in oklab, ${to} ${Math.round(((c - a) / (b - a)) * 100)}%, ${from})`;
    }
    return RAMP.at(-1)[1];
  }
  /** A cone's tint: cone 1 a dull glow, cone 10 the hot end; past the limit, the hot end. */
  const coneColour = (c) => heatColour(560 + ((Math.min(c, LIMIT) - 1) / (LIMIT - 1)) * 720);

  let last = $derived(readings.at(-1) ?? null);
  let glow = $derived(last ? Math.max(0, Math.min(1, (last.celsius - 450) / 830)) : 0);
  let window_ = $derived(heatColour(last?.celsius));
  let past = $derived(Number(cone) > LIMIT);

  // ── the firing, drawn: °C over the firing's minutes ──
  let cw = $state(0);
  let ph = $state(0);
  let CH = $derived(Math.max(232, ph));
  const PR = 12, PT = 30, PB = 44, YMAX = 1300;
  let PL = $derived(cw < 420 ? 40 : 46); // a phone's chart gives its labels less
  const YT = [0, 400, 800, 1200];
  let xmax = $derived(Math.max(HOURS * 60, last?.minute ?? 0));
  let XT = $derived(Array.from({ length: Math.floor(xmax / 30) + 1 }, (_, i) => i * 30));
  let base = $derived(CH - PB);
  const X = (m) => PL + (m / xmax) * Math.max(0, cw - PL - PR);
  const Y = (c) => PT + (1 - Math.min(c, YMAX) / YMAX) * (base - PT);
  let pts = $derived(readings.map((r) => [X(r.minute), Y(r.celsius)]));
  const line = (ps) => (ps.length ? "M" + ps.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join("L") : "");
  let settled = $derived(line(pts.slice(0, -1)));
  let growing = $derived(pts.length > 1 ? line(pts.slice(-2)) : "");
  let area = $derived(pts.length > 1 ? `${line(pts)}L${pts.at(-1)[0].toFixed(1)},${base}L${pts[0][0].toFixed(1)},${base}Z` : "");
  let runs = $derived.by(() => {
    const out = [];
    readings.forEach((r, i) => {
      const from = i ? readings[i - 1].minute : r.minute;
      const run = out.at(-1);
      if (run && run.phase === r.phase) run.to = r.minute;
      else out.push({ phase: r.phase, from, to: r.minute });
    });
    return out;
  });
  let leftAt = $derived(left && last ? X(last.minute) : null);
</script>

<section class="kiln-live" style={MIX} data-surface="kiln-live" data-state={state} data-store={store ? (store.kept ? "file" : "memory") : null}>
  <header class="head">
    <div class="title">
      <h2>A kiln, running in this tab</h2>
      <div class="badges">
        <span class="badge" data-s={state}><i class="dot" class:busy={heating || firing}></i>{state}</span>
        {#if state === "running"}
          <span class="badge mono" title="the kiln's interface hash"><span class="badge-word">interface </span><code data-field="interface">{iface}</code></span>
          <span class="badge" class:memory={!store?.kept}>{store?.kept ? "log kept" : "log in memory"}</span>
        {/if}
      </div>
    </div>
    <p class="what" class:open={whatOpen}>
      <strong>Real:</strong> the kiln is Rust (<code>#[plexus::activation]</code>), compiled to WebAssembly and running in a
      worker of this tab, with its log in SQLite. Every button calls it through its generated client, as frames between
      this page and the worker. <strong>Not here:</strong> menger's server takes no part — it served this component and
      nothing else, and no call on this page reaches it. The log is kept by this browser only.
    </p>
    <button type="button" class="more" aria-expanded={whatOpen} onclick={() => (whatOpen = !whatOpen)}>{whatOpen ? "less" : "what runs here"}</button>
  </header>

  <div class="grid" class:waiting={state !== "running"}>
    <div class="col">
    <div class="card kiln-card">
      <div class="kiln-row">
        <svg class="kiln" viewBox="0 0 120 150" role="img" aria-label={last ? `the kiln at ${Math.round(last.celsius)} degrees` : "the kiln, cold"} style="--heat: {window_}; --glow: {glow}">
          <defs>
            <radialGradient id="kl-win-{uid}">
              <stop offset="0" class="win-core" />
              <stop offset="1" class="win-edge" />
            </radialGradient>
            <filter id="kl-blur-{uid}" x="-1" y="-1" width="3" height="3"><feGaussianBlur stdDeviation="7" /></filter>
          </defs>
          <ellipse class="shadow" cx="60" cy="142" rx="44" ry="4" />
          <g class="wisps" class:on={glow > 0.35}>
            <path d="M52 20 q-4 -6 0 -12 t0 -10" /><path d="M60 18 q4 -6 0 -12 t0 -10" /><path d="M68 20 q-4 -6 0 -12 t0 -10" />
          </g>
          <rect class="vent" x="51" y="20" width="18" height="10" rx="2" />
          <rect class="leg" x="24" y="128" width="8" height="12" rx="1.5" />
          <rect class="leg" x="88" y="128" width="8" height="12" rx="1.5" />
          <rect class="body" x="12" y="28" width="96" height="102" rx="12" />
          <path class="lid" d="M12 46 v-6 a12 12 0 0 1 12 -12 h72 a12 12 0 0 1 12 12 v6 z" />
          <rect class="door" x="22" y="54" width="76" height="64" rx="7" />
          <line class="seam" x1="22" y1="66" x2="98" y2="66" />
          <rect class="hinge" x="18" y="62" width="5" height="10" rx="1.5" /><rect class="hinge" x="18" y="100" width="5" height="10" rx="1.5" />
          <circle class="halo" cx="60" cy="88" r="20" filter="url(#kl-blur-{uid})" />
          <circle class="rim" cx="60" cy="88" r="15" />
          <circle class="win" cx="60" cy="88" r="12" />
          <circle class="win-light" cx="60" cy="88" r="12" fill="url(#kl-win-{uid})" />
          <circle class="led" class:busy={heating || firing} class:off={state !== "running"} cx="90" cy="38" r="2.6" />
        </svg>
        <div class="readout">
          {#if last}
            <span class="temp">{Math.round(last.celsius)}<small>°C</small></span>
            <span class="phase"><i data-phase={last.phase}></i>{last.phase} · {last.minute}′</span>
          {:else}
            <span class="temp cold">—<small>°C</small></span>
            <span class="phase">{firing ? `firing cone ${cone} …` : "cold · nothing read"}</span>
          {/if}
          {#if state === "starting"}<p class="state">starting the kiln …</p>{/if}
        </div>
      </div>

      {#if state === "failed"}
        <p class="state failed" data-refused="start">the kiln did not start: {failure}</p>
      {:else if state === "running"}
        <div class="cone">
          <div class="cone-head">
            <span class="label">cone</span>
            <div class="stepper">
              <button type="button" aria-label="one cone cooler" onclick={() => step(-1)}>−</button>
              <input type="number" min="1" max="14" data-field="cone" bind:value={cone} aria-label="cone, 1 to 14" />
              <button type="button" aria-label="one cone hotter" onclick={() => step(1)}>+</button>
            </div>
          </div>
          <div class="scale" style="--on: {Number(cone)}">
            {#each CONES as c}
              <button type="button" tabindex="-1" class="cell" class:past={c > LIMIT} class:lit={c <= Number(cone)} class:on={c === Number(cone)} class:limit={c === LIMIT} style="--c: {coneColour(c)}; --c-dim: color-mix(in oklab, {coneColour(c)} 22%, var(--hover))" aria-label="cone {c}" onclick={() => (cone = c)}></button>
            {/each}
            {#each CONES as c}<span class="num" class:on={c === Number(cone)} class:past={c > LIMIT}>{c}</span>{/each}
          </div>
          <p class="scale-note" class:past>
            {#if past}cone {cone} is past the limit: fire is allowed, and the kiln will refuse it{:else}the kiln goes to cone 10; above it, it refuses{/if}
          </p>
        </div>

        <div class="actions">
          <button class="primary" data-action="fire" onclick={fire} disabled={firing}>{firing ? "firing …" : "fire"}</button>
          <button class="secondary" data-action="heat" onclick={heat} disabled={heating}>
            {#if heating}reading {readings.length} of {READINGS}{:else}heat<span class="sub">a stream, left after {READINGS}</span>{/if}
          </button>
        </div>

        {#if refused}
          <div class="notice" role="status" data-refused={refused.code}>
            <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M8 1.5 15 14H1z" /><path class="mark" d="M8 6v4M8 11.6v.4" /></svg>
            <p>refused: <code>{refused.code}</code> — {refused.code === "too-hot" ? `the kiln goes to cone 10; cone ${refused.payload?.val} was asked` : JSON.stringify(refused.payload)}</p>
          </div>
        {/if}
      {/if}
    </div>

    <div class="card log-card">
      <h3>the log <span class="count">{firings.length}</span></h3>
      {#if firings.length}
        <ol class="log" data-field="log">
          {#each firings as f (f.id)}<li data-firing={f.id} class:fresh={f.id === fresh}><i class="sw" style="--c: {coneColour(f.cone)}"></i><span class="id">#{f.id}</span><span class="sep">{" · "}</span><span class="cone-n">cone {f.cone}</span><span class="sep">{" · "}</span><span class="hours">{f.hours} h</span><span class="sep">{" · "}</span><span class="by">by {f.loadedBy}</span></li>{/each}
        </ol>
      {:else}
        <div class="empty" data-field="log">
          <svg viewBox="0 0 120 40" aria-hidden="true"><rect class="shelf" x="6" y="30" width="108" height="4" rx="2" /><path class="ghost" d="M22 30 v-12 a6 6 0 0 1 12 0 v12 M52 30 v-18 a8 8 0 0 1 16 0 v18 M86 30 v-10 a5 5 0 0 1 10 0 v10" /></svg>
          <span>{state === "running" ? "nothing fired yet" : "the log, once the kiln is up"}</span>
        </div>
      {/if}
      {#if state === "running"}
        <p class="store" class:memory={!store?.kept} data-field="store">
          {#if store?.kept}
            The log is kept in this browser's file store: reload and it is still here; no other browser sees it.
          {:else}
            This browser gave the kiln no file store ({store?.why}). The log is kept in memory and will not outlive this page.
          {/if}
        </p>
      {/if}
    </div>

    </div>
    <div class="col">
    <figure class="card firing">
      <figcaption>
        <h3>the firing</h3>
        <span class="aside">°C over {HOURS} hours</span>
        {#if heating}<span class="live-count"><i class="dot busy"></i>reading {readings.length} of {READINGS}</span>
        {:else if readings.length}<span class="count">{readings.length}</span>{/if}
      </figcaption>
      <div class="plot" bind:clientWidth={cw} bind:clientHeight={ph}>
        {#if cw > 0}
          <svg width={cw} height={CH} role="img" aria-label={readings.length ? `a firing: ${readings.length} readings to ${last ? Math.round(last.celsius) : 0} degrees` : "no firing yet"}>
            <defs>
              <linearGradient id="kl-ramp-{uid}" gradientUnits="userSpaceOnUse" x1="0" y1={Y(0)} x2="0" y2={Y(1280)}>
                <stop offset="0" class="ramp-0" /><stop offset={520 / 1280} class="ramp-1" /><stop offset={860 / 1280} class="ramp-2" /><stop offset="1" class="ramp-3" />
              </linearGradient>
              <pattern id="kl-hatch-{uid}" width="7" height="7" patternUnits="userSpaceOnUse" patternTransform="rotate(45)"><line class="hatch" x1="0" y1="0" x2="0" y2="7" /></pattern>
            </defs>
            {#each YT as t}
              <line class="grid-line" class:zero={t === 0} x1={PL} x2={cw - PR} y1={Y(t)} y2={Y(t)} />
              <text class="tick" x={PL - 8} y={Y(t) + 4} text-anchor="end">{t}°</text>
            {/each}
            {#each XT as m}
              <text class="tick" x={X(m)} y={base + 22} text-anchor={m === 0 ? "start" : m === xmax ? "end" : "middle"}>{m}′</text>
            {/each}

            {#if !readings.length && !heating}
              <text class="empty-title" x={PL + (cw - PL - PR) / 2} y={PT + (base - PT) / 2 - 4} text-anchor="middle">no firing yet</text>
              <text class="empty-sub" x={PL + (cw - PL - PR) / 2} y={PT + (base - PT) / 2 + 16} text-anchor="middle">heat draws one here, a reading each five minutes</text>
            {/if}

            {#if leftAt != null}
              <rect x={leftAt} y={PT} width={Math.max(0, cw - PR - leftAt)} height={base - PT} fill="url(#kl-hatch-{uid})" class="unread" />
              {#if cw - PR - leftAt > 90}<text class="unread-label" x={(leftAt + cw - PR) / 2} y={PT + (base - PT) / 2 + 4} text-anchor="middle">not read</text>{/if}
            {/if}

            {#if area}<path class="area" d={area} fill="url(#kl-ramp-{uid})" />{/if}
            {#if settled}<path class="curve" d={settled} stroke="url(#kl-ramp-{uid})" />{/if}
            {#key readings.length}
              {#if growing}<path class="curve grow" d={growing} stroke="url(#kl-ramp-{uid})" pathLength="1" />{/if}
            {/key}
            {#each pts.slice(0, -1) as [x, y]}<circle class="pt" cx={x} cy={y} r="2" />{/each}

            {#each runs as r}
              <rect class="band" data-phase={r.phase} x={X(r.from)} y={base + 4} width={Math.max(3, X(r.to) - X(r.from))} height="4" rx="2" />
              <text class="band-label" data-phase={r.phase} x={X(r.from)} y={base + 38}>{r.phase}</text>
            {/each}

            {#if leftAt != null}
              <line class="left-line" x1={leftAt} x2={leftAt} y1={PT - 14} y2={base} />
              <path class="flag" d="M{leftAt},{PT - 22} l11,4 l-11,4 z" />
              <text class="flag-label" x={leftAt > cw - 170 ? leftAt - 6 : leftAt + 15} y={PT - 14} text-anchor={leftAt > cw - 170 ? "end" : "start"}>the page left · {last.minute}′</text>
            {/if}

            {#if last}
              {#key readings.length}
                <circle class="pt last" class:live={heating} cx={pts.at(-1)[0]} cy={pts.at(-1)[1]} r="4.5" style="--heat: {window_}" />
              {/key}
              {#if leftAt == null}
                <text class="last-label" x={pts.at(-1)[0]} y={pts.at(-1)[1] - 12} text-anchor={pts.at(-1)[0] > cw - 50 ? "end" : "middle"}>{Math.round(last.celsius)}°</text>
              {/if}
            {/if}
          </svg>
        {/if}
      </div>
      {#if readings.length || heating}
        {#if left}<p class="left" data-field="heat-left">{left}</p>{/if}
        <details class="readings-list">
          <summary>the readings, as a list</summary>
          <ol class="readings" data-field="readings">
            {#each readings as r (r.minute)}<li>{r.minute}′ <b>{Math.round(r.celsius)}°</b> {r.phase}</li>{/each}
          </ol>
        </details>
      {/if}
    </figure>

    <div class="card tape-card">
      <h3>the frames crossing <span class="dirs"><span class="out">→ to the kiln</span><span class="in">← from it</span><span>the last 16</span></span></h3>
      <div class="tape-wrap">
        <pre class="frames" data-field="frames">{#each frames as f, i (f.n)}{#if i}{"\n"}{/if}<span class="frame" class:out={f.out} class:in={!f.out}><b>{f.line.slice(0, 1)}</b>{f.line.slice(1)}</span>{/each}</pre>
        {#if !frames.length}<p class="tape-empty">every call crosses here as a frame, both ways</p>{/if}
      </div>
    </div>
    </div>
  </div>
</section>

<style>
  .kiln-live {
    /* the component's own properties, each declared here with a default of tokens alone; the script sets the
       mixed values (MIX on the section, the heat and the cones' tints on their elements), which win */
    --kl-cold: var(--skeleton); --kl-ember: var(--warn); --kl-hot: var(--chart-2); --kl-ping: transparent;
    --kl-warn-soft: var(--hover); --kl-warn-mid: var(--panel); --kl-warn-line: var(--warn);
    --kl-accent-hover: var(--accent); --kl-in-bg: transparent;
    --heat: var(--kl-cold); --glow: 0; --c: var(--hover); --c-dim: var(--hover);
    container: kiln-live / inline-size;
    box-sizing: border-box; max-width: 1120px; padding: 20px 0 24px; color: var(--ink); background: var(--bg);
    font-family: var(--font-text); font-size: var(--text-size); line-height: 1.45;
  }
  .kiln-live * { box-sizing: border-box; }
  .kiln-live code { font-family: var(--font-text); }

  /* the head: the title and what is real, said once and quietly */
  .head { margin: 0 24px 16px; }
  .grid { margin: 0 24px; }
  .more { display: none; }
  .title { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 14px; }
  h2 { margin: 0; font-family: var(--font-display); font-size: 17px; font-weight: var(--weight-strong); letter-spacing: -0.01em; }
  .badges { display: flex; flex-wrap: wrap; gap: 6px; }
  .badge { display: inline-flex; align-items: center; gap: 6px; height: 22px; padding: 0 9px; font-size: 11px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); white-space: nowrap; }
  .badge code { color: var(--ink); font-size: 11px; }
  .badge.memory { color: var(--st-blocked-fg); background: var(--st-blocked-bg); border-color: transparent; }
  .dot { display: inline-block; width: 7px; height: 7px; border-radius: 50%; background: var(--muted); }
  .badge[data-s=running] .dot { background: var(--live); }
  .badge[data-s=failed] { color: var(--warn); }
  .badge[data-s=failed] .dot { background: var(--warn); }
  .badge[data-s=starting] .dot { animation: kl-breathe var(--motion-pulse) var(--ease) infinite alternate; }
  .kiln-live .dot.busy { background: var(--chart-2); box-shadow: 0 0 0 0 var(--kl-ping); animation: kl-ping 1s var(--ease) infinite; }
  .what { max-width: 76ch; margin: 8px 0 0; font-family: var(--sans); font-size: 12.5px; line-height: 1.55; color: var(--muted); }
  .what strong { color: var(--ink); font-weight: var(--weight-medium); }
  .what code { font-size: 11.5px; color: var(--ink); }

  /* the parts: stacked, then two columns by the component's own width */
  .grid { display: grid; gap: 14px; grid-template-columns: minmax(0, 1fr); grid-template-areas: "kiln" "firing" "log" "tape"; }
  .col { display: contents; }
  @container kiln-live (min-width: 760px) {
    /* two columns, each its own height: the kiln over its log, the firing over its frames */
    .grid { grid-template-columns: minmax(300px, 360px) minmax(0, 1fr); grid-template-areas: none; align-items: start; }
    .col { display: flex; flex-direction: column; gap: 14px; min-width: 0; }
    .firing .plot { min-height: 300px; }
  }
  .card { min-width: 0; margin: 0; padding: 16px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-xl); }
  .kiln-card { grid-area: kiln; display: flex; flex-direction: column; gap: 16px; }
  .firing { grid-area: firing; display: flex; flex-direction: column; }
  .log-card { grid-area: log; display: flex; flex-direction: column; }
  .log-card .store { margin-top: auto; }
  .log-card .log, .log-card .empty { margin-bottom: 12px; }
  .tape-card { grid-area: tape; }
  .grid.waiting .firing, .grid.waiting .log-card, .grid.waiting .tape-card { opacity: 0.6; }
  h3, figcaption { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 10px; margin: 0 0 10px; font-family: var(--sans); font-size: 12.5px; font-weight: var(--weight-medium); color: var(--ink); }
  h3 { margin: 0 0 10px; }
  figcaption h3 { margin: 0; }
  .aside { color: var(--muted); font-weight: normal; }
  .count { min-width: 20px; padding: 0 6px; font-family: var(--font-text); font-size: 11px; text-align: center; color: var(--chip-fg); background: var(--chip-bg); border-radius: var(--radius-pill); }
  figcaption .count, .live-count { margin-left: auto; }
  .live-count { display: inline-flex; align-items: center; gap: 6px; font-family: var(--font-text); font-size: 11px; font-weight: normal; color: var(--muted); }

  /* the kiln, drawn */
  .kiln-row { display: flex; align-items: center; gap: 18px; }
  .kiln { flex: none; width: 120px; height: auto; overflow: visible; }
  .kiln .shadow { fill: var(--shadow); }
  .kiln .body { fill: var(--card); stroke: var(--tree-line); stroke-width: 1.5; }
  .kiln .lid { fill: var(--hover); stroke: var(--tree-line); stroke-width: 1.5; }
  .kiln .vent, .kiln .leg, .kiln .hinge { fill: var(--hover); stroke: var(--tree-line); stroke-width: 1.2; }
  .kiln .door { fill: var(--bg); stroke: var(--tree-line); stroke-width: 1.2; }
  .kiln .seam { stroke: var(--line); stroke-width: 1; }
  .kiln .rim { fill: var(--card); stroke: var(--tree-line); stroke-width: 2; }
  .kiln .win { fill: var(--heat); stroke: var(--tree-line); stroke-width: 1; transition: fill 0.3s var(--ease); }
  .kiln .win-light { opacity: var(--glow); transition: opacity 0.3s var(--ease); }
  .kiln .win-core { stop-color: var(--kl-hot); stop-opacity: 0.9; }
  .kiln .win-edge { stop-color: var(--heat); stop-opacity: 0; }
  .kiln .halo { fill: var(--heat); opacity: calc(var(--glow) * 0.75); transition: opacity 0.3s var(--ease), fill 0.3s var(--ease); }
  .kiln .led { fill: var(--live); }
  .kiln .led.off { fill: var(--disabled); }
  .kiln .led.busy { fill: var(--chart-2); animation: kl-blink 0.8s steps(2, jump-none) infinite; }
  .kiln .wisps path { fill: none; stroke: var(--muted); stroke-width: 1.4; stroke-linecap: round; opacity: 0; }
  .kiln .wisps.on path { animation: kl-rise 2.4s var(--ease) infinite; }
  .kiln .wisps.on path:nth-child(2) { animation-delay: 0.8s; }
  .kiln .wisps.on path:nth-child(3) { animation-delay: 1.6s; }
  .readout { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .temp { font-family: var(--font-text); font-size: 38px; font-weight: var(--weight-medium); line-height: 1; letter-spacing: -0.03em; font-variant-numeric: tabular-nums; color: var(--ink); }
  .temp small { margin-left: 2px; font-size: 14px; font-weight: normal; letter-spacing: 0; color: var(--muted); }
  .temp.cold { color: var(--disabled); }
  .phase { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--muted); }
  .phase i { width: 8px; height: 8px; border-radius: 2px; }
  .state { margin: 6px 0 0; color: var(--muted); font-family: var(--sans); font-size: 12.5px; }
  .state.failed { margin: 0; color: var(--warn); }

  /* the cone: a stepper and a scale with the limit on it */
  .cone-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 24px; }
  .label { font-family: var(--sans); font-size: 12.5px; font-weight: var(--weight-medium); }
  .stepper { display: inline-flex; align-items: stretch; height: 28px; border: var(--border) solid var(--line); border-radius: var(--radius); background: var(--card); overflow: hidden; }
  .stepper button { width: 28px; padding: 0; font: inherit; font-size: 14px; color: var(--muted); background: none; border: 0; cursor: pointer; }
  .stepper button:hover, .stepper button:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }
  .stepper input { width: 3.2em; padding: 0; font: inherit; font-size: 14px; font-weight: var(--weight-medium); text-align: center; font-variant-numeric: tabular-nums; color: var(--ink); background: none; border: 0; border-left: var(--border) solid var(--line); border-right: var(--border) solid var(--line); -moz-appearance: textfield; appearance: textfield; }
  .stepper input::-webkit-inner-spin-button, .stepper input::-webkit-outer-spin-button { -webkit-appearance: none; margin: 0; }
  .stepper:focus-within { outline: 2px solid var(--focus); outline-offset: 1px; }
  .stepper input:focus-visible { outline: none; }
  .scale { position: relative; display: grid; grid-template-columns: repeat(14, minmax(0, 1fr)); column-gap: 3px; row-gap: 5px; }
  .cell { height: 18px; padding: 0; background: var(--c-dim); border: 0; border-radius: var(--radius-sm); cursor: pointer; transition: background var(--motion-fast) var(--ease), transform var(--motion-fast) var(--ease); }
  .cell.lit { background: var(--c); }
  .cell.past { background: repeating-linear-gradient(135deg, var(--kl-warn-soft) 0 3px, var(--hover) 3px 6px); }
  .cell.past.lit { background: repeating-linear-gradient(135deg, var(--warn) 0 3px, var(--kl-warn-mid) 3px 6px); }
  .cell.on { transform: scaleY(1.45); box-shadow: 0 0 0 2px var(--panel), 0 0 0 3px var(--ink); }
  .cell:hover, .cell:global([data-sheet~=hover]) { filter: brightness(1.08); }
  .cell.limit { position: relative; }
  .cell.limit::after { content: "limit"; position: absolute; top: -17px; right: -4px; transform: translateX(50%); font-family: var(--font-text); font-size: 9.5px; color: var(--warn); }
  .cell.limit::before { content: ""; position: absolute; top: -4px; bottom: -4px; right: -2.5px; width: 2px; border-radius: 1px; background: var(--warn); }
  .num { font-size: 10px; text-align: center; color: var(--muted); font-variant-numeric: tabular-nums; }
  .num.past { color: var(--warn); opacity: 0.75; }
  .num.on { color: var(--ink); font-weight: var(--weight-bold); }
  .scale-note { margin: 8px 0 0; font-family: var(--sans); font-size: 12px; color: var(--muted); }
  .scale-note.past { color: var(--warn); }

  /* the actions: fire the one primary, heat beside it */
  .actions { display: flex; flex-wrap: wrap; gap: 8px; }
  .actions button { height: 34px; padding: 0 16px; font: inherit; border-radius: var(--radius); cursor: pointer; transition: background var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .primary { min-width: 92px; font-weight: var(--weight-medium); color: var(--bg); background: var(--accent); border: var(--border) solid var(--accent); }
  .primary:hover:not(:disabled), .primary:global([data-sheet~=hover]) { background: var(--kl-accent-hover); }
  .secondary { flex: 1; min-width: 0; overflow: hidden; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); text-align: left; white-space: nowrap; text-overflow: ellipsis; }
  .secondary .sub { margin-left: 10px; color: var(--muted); font-family: var(--sans); font-size: 11.5px; }
  .secondary:hover:not(:disabled), .secondary:global([data-sheet~=hover]) { background: var(--hover); }
  .actions button:disabled, .actions button:global([data-sheet~=disabled]) { cursor: default; }
  .primary:disabled { opacity: 0.6; }
  .secondary:disabled { color: var(--muted); }
  .actions button:focus-visible, .actions button:global([data-sheet~=focus]), .cell:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }

  /* a refusal: a notice beside the controls */
  .notice { display: flex; gap: 10px; align-items: flex-start; padding: 10px 12px; font-family: var(--sans); font-size: 12.5px; color: var(--ink); background: var(--st-blocked-bg); border: var(--border) solid var(--kl-warn-line); border-radius: var(--radius-lg); animation: kl-enter 0.18s var(--ease); }
  .notice p { margin: 0; }
  .notice code { padding: 1px 6px; font-size: 11.5px; color: var(--st-blocked-fg); background: var(--panel); border-radius: var(--radius-sm); }
  .notice svg { flex: none; width: 16px; height: 16px; margin-top: 1px; }
  .notice svg path { fill: var(--warn); stroke: var(--warn); stroke-width: 1.2; stroke-linejoin: round; }
  .notice svg path.mark { fill: none; stroke: var(--st-blocked-bg); stroke-width: 1.6; stroke-linecap: round; }

  /* the firing, a curve */
  .plot { position: relative; flex: 1; width: 100%; min-height: 232px; }
  .plot svg { position: absolute; inset: 0; display: block; overflow: visible; }
  .grid-line { stroke: var(--line); stroke-width: 1; stroke-dasharray: 2 4; }
  .grid-line.zero { stroke: var(--tree-line); stroke-dasharray: none; }
  .tick { fill: var(--muted); font-family: var(--font-text); font-size: 10.5px; font-variant-numeric: tabular-nums; }
  .ramp-0 { stop-color: var(--muted); }
  .ramp-1 { stop-color: var(--kl-ember); }
  .ramp-2 { stop-color: var(--chart-2); }
  .ramp-3 { stop-color: var(--kl-hot); }
  .area { opacity: 0.16; }
  .curve { fill: none; stroke-width: 2.5; stroke-linejoin: round; stroke-linecap: round; }
  .curve.grow { stroke-dasharray: 1; animation: kl-draw 0.2s var(--ease) both; }
  .pt { fill: var(--panel); stroke: var(--muted); stroke-width: 1.2; }
  .pt.last { fill: var(--heat); stroke: var(--panel); stroke-width: 2; transform-box: fill-box; transform-origin: center; animation: kl-pop 0.2s var(--ease) both; }
  .pt.last.live { animation: kl-pop 0.2s var(--ease) both, kl-glow 1s var(--ease) 0.2s infinite; }
  .last-label { fill: var(--ink); font-family: var(--font-text); font-size: 11.5px; font-weight: var(--weight-medium); }
  .band[data-phase=climbing], .phase i[data-phase=climbing] { fill: var(--chart-2); background: var(--chart-2); }
  .band[data-phase=holding], .phase i[data-phase=holding] { fill: var(--sev-high); background: var(--sev-high); }
  .band[data-phase=cooling], .phase i[data-phase=cooling] { fill: var(--chart-1); background: var(--chart-1); }
  .band-label { font-family: var(--sans); font-size: 11px; fill: var(--muted); }
  .hatch { stroke: var(--line); stroke-width: 2; }
  .unread { opacity: 0.8; }
  .unread-label { fill: var(--muted); font-family: var(--sans); font-size: 11px; }
  .left-line { stroke: var(--ink); stroke-width: 1; stroke-dasharray: 3 3; }
  .flag { fill: var(--ink); }
  .flag-label { fill: var(--ink); font-family: var(--sans); font-size: 11.5px; font-weight: var(--weight-medium); }
  .empty-title { fill: var(--ink); font-family: var(--sans); font-size: 13px; font-weight: var(--weight-medium); }
  .empty-sub { fill: var(--muted); font-family: var(--sans); font-size: 12px; }
  .left { margin: 6px 0 0; font-family: var(--sans); font-size: 12px; line-height: 1.5; color: var(--muted); }
  .readings-list { margin-top: 8px; font-size: 12px; }
  .readings-list summary { width: max-content; font-family: var(--sans); color: var(--muted); cursor: pointer; }
  .readings-list summary:hover { color: var(--ink); }
  .readings-list summary:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; border-radius: var(--radius-sm); }
  .readings { display: flex; flex-wrap: wrap; gap: 4px 14px; margin: 8px 0 0; padding: 0; list-style: none; color: var(--muted); font-variant-numeric: tabular-nums; }
  .readings b { color: var(--ink); font-weight: var(--weight-medium); }

  /* the log, as rows */
  .log { margin: 0; padding: 0; list-style: none; }
  .log li { display: grid; grid-template-columns: 10px 34px minmax(0, 1fr) auto auto; align-items: center; gap: 10px; height: var(--pitch); padding: 0 8px; margin: 0 -8px; border-radius: var(--radius); font-variant-numeric: tabular-nums; }
  .log li + li { box-shadow: inset 0 var(--border) 0 var(--line); }
  .log li.fresh { animation: kl-flash var(--motion-flash) var(--ease); }
  .log .sw { width: 10px; height: 10px; border-radius: 3px; background: var(--c); }
  .log .sep { display: none; }
  .log .id { color: var(--muted); }
  .log .hours { color: var(--muted); }
  .log .by { color: var(--muted); font-family: var(--sans); }
  .empty { display: flex; flex-direction: column; align-items: center; gap: 4px; padding: 10px 0 6px; font-family: var(--sans); color: var(--muted); }
  .empty svg { width: 120px; height: 40px; }
  .empty .shelf { fill: var(--line); }
  .empty .ghost { fill: none; stroke: var(--tree-line); stroke-width: 1.4; stroke-dasharray: 3 3; }
  .store { margin: 0; padding-top: 10px; border-top: var(--border) solid var(--line); font-family: var(--sans); font-size: 12px; line-height: 1.5; color: var(--muted); }
  .store.memory { color: var(--warn); }

  /* the frames, a tape */
  .dirs { display: inline-flex; flex-wrap: wrap; gap: 4px 12px; margin-left: auto; font-family: var(--font-text); font-size: 11px; font-weight: normal; color: var(--muted); }
  .dirs .out { color: var(--accent); }
  .dirs .in { color: var(--chart-3); }
  .tape-wrap { position: relative; }
  .frames { display: grid; align-content: start; min-height: 88px; max-height: 340px; margin: 0; padding: 6px 0; overflow: auto; font-family: var(--font-text); font-size: 11.5px; line-height: 1.7; white-space: pre; background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .frame { display: block; width: max-content; min-width: 100%; padding: 0 12px 0 10px; border-left: 2px solid transparent; color: var(--ink); animation: kl-enter 0.16s var(--ease); }
  .frame b { font-weight: var(--weight-bold); }
  .frame.out { border-left-color: var(--accent); }
  .frame.out b { color: var(--accent); }
  .frame.in { padding-left: 26px; color: var(--muted); border-left-color: var(--chart-3); background: var(--kl-in-bg); }
  .frame.in b { color: var(--chart-3); }
  .tape-empty { position: absolute; inset: 0; display: grid; place-items: center; margin: 0; font-family: var(--sans); font-size: 12px; color: var(--muted); pointer-events: none; }

  /* a phone: tighter margins, bigger targets, the paragraph folded, the tape wrapped rather than cut */
  @container kiln-live (max-width: 559px) {
    .head { margin: 0 12px 12px; }
    .grid { margin: 0 8px; gap: 10px; }
    .card { padding: 14px; border-radius: var(--radius-lg); }
    h2 { font-size: 16px; }
    .title { gap: 8px; }
    .badge-word { display: none; }
    .what:not(.open) { display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
    .more { display: inline-block; margin-top: 4px; padding: 6px 0; font-family: var(--sans); font-size: 12.5px; color: var(--accent); background: none; border: 0; cursor: pointer; }
    .more:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; border-radius: var(--radius-sm); }
    .kiln-card { gap: 14px; }
    .kiln-row { gap: 14px; }
    .kiln { width: 96px; }
    .temp { font-size: 34px; }
    .cone-head { margin-bottom: 22px; }
    .stepper { height: 38px; }
    .stepper button { width: 40px; font-size: 16px; }
    .stepper input { font-size: 16px; }
    .scale { column-gap: 2px; }
    .cell { height: 28px; }
    .cell.on { transform: scaleY(1.25); }
    .actions button { height: 44px; }
    .primary { flex: 1; }
    .secondary { flex: 1.4; }
    .secondary .sub { display: none; }
    .log li { height: 40px; }
    .dirs { margin-left: 0; width: 100%; }
    .frames { max-height: 300px; font-size: 11px; line-height: 1.6; }
    .frame { width: auto; white-space: pre-wrap; word-break: break-all; padding-right: 8px; }
    .frame.in { padding-left: 20px; }
  }

  @keyframes kl-enter { from { opacity: 0; transform: translateY(-3px); } }
  @keyframes kl-draw { from { stroke-dashoffset: 1; } to { stroke-dashoffset: 0; } }
  @keyframes kl-pop { from { transform: scale(0.3); } }
  @keyframes kl-glow { 50% { stroke-width: 6; stroke: var(--heat); stroke-opacity: 0.45; } }
  @keyframes kl-ping { to { box-shadow: 0 0 0 6px transparent; } }
  @keyframes kl-breathe { to { opacity: 0.3; } }
  @keyframes kl-blink { 50% { opacity: 0.25; } }
  @keyframes kl-rise { 0% { opacity: 0; transform: translateY(6px); } 40% { opacity: 0.7; } 100% { opacity: 0; transform: translateY(-6px); } }
  @keyframes kl-flash { from { background: var(--sel); } to { background: transparent; } }
  @media (prefers-reduced-motion: reduce) {
    .kiln-live *, .kiln-live *::before, .kiln-live *::after { animation: none !important; transition: none !important; }
    .kiln .wisps.on path { opacity: 0.5; }
  }
</style>
