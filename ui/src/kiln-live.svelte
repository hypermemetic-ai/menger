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
  let kiln = null;

  const READINGS = 12; // heat is left after this many: the stream is cancelled, not run out
  const text = new TextDecoder();

  /** This bundle's kiln-activation module at its version's path. */
  function activationUrl() {
    let map = {};
    try { map = JSON.parse(document.getElementById("plexus-ui-files")?.textContent || "{}").imports || {}; } catch {}
    const key = Object.keys(map).find((k) => /^\/app\/[^/]+\/components\/kiln-activation\.js$/.test(k));
    return new URL(key ?? "./kiln-activation.js", key ? location.href : import.meta.url).href;
  }

  function tap(from, frame) {
    const line = (from === "caller" ? "→ " : "← ") + text.decode(frame).slice(0, 110);
    frames = [...frames.slice(-15), line];
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
    frames = ['→ [3,1,["kiln.heat",{"cone":6,"hours":2}]]', '← [3,{"minute":0,"celsius":20,"phase":"climbing"}]', "→ [3,5,null]"];
  } else if (sheet == null) start();
  $effect(() => () => kiln?.worker.terminate());

  async function fire() {
    if (!kiln) return;
    refused = null;
    firing = true;
    try {
      await kiln.client.fire(Number(cone), 2, "you");
    } catch (e) {
      refused = e instanceof kiln.PlexusFailure ? { code: e.code, payload: e.payload } : { code: "error", payload: String(e?.message ?? e) };
    }
    firings = await kiln.client.log();
    firing = false;
  }

  async function heat() {
    if (!kiln) return;
    heating = true;
    readings = [];
    left = "";
    for await (const r of kiln.client.heat(Number(cone), 2)) {
      readings = [...readings, r];
      if (readings.length >= READINGS) break;
    }
    left = `left after ${readings.length} readings: the page sent the kiln a cancel (→ [call,5,null] below); a reading already on its way may still cross after it`;
    heating = false;
  }
</script>

<section class="kiln-live" data-surface="kiln-live" data-state={state} data-store={store ? (store.kept ? "file" : "memory") : null}>
  <h2>A kiln, running in this tab</h2>
  <p class="what">
    <strong>Real:</strong> the kiln is Rust (<code>#[plexus::activation]</code>), compiled to WebAssembly and running in a
    worker of this tab, with its log in SQLite. Every button calls it through its generated client, as frames between
    this page and the worker. <strong>Not here:</strong> menger's server takes no part — it served this component and
    nothing else, and no call on this page reaches it. The log is kept by this browser only.
  </p>

  {#if state === "starting"}
    <p class="state">starting the kiln …</p>
  {:else if state === "failed"}
    <p class="state failed" data-refused="start">the kiln did not start: {failure}</p>
  {:else}
    <p class="state">running · interface <code data-field="interface">{iface}</code></p>
    <p class="store" class:memory={!store?.kept} data-field="store">
      {#if store?.kept}
        The log is kept in this browser's file store: reload and it is still here; no other browser sees it.
      {:else}
        This browser gave the kiln no file store ({store?.why}). The log is kept in memory and will not outlive this page.
      {/if}
    </p>

    <div class="controls">
      <label>cone <input type="number" min="1" max="14" data-field="cone" bind:value={cone} /></label>
      <button data-action="fire" onclick={fire} disabled={firing}>{firing ? "firing …" : "fire"}</button>
      <button data-action="heat" onclick={heat} disabled={heating}>heat — a stream, left after {READINGS}</button>
      <span class="hint">the kiln goes to cone 10; above it, it refuses</span>
    </div>

    {#if refused}
      <p class="refused" data-refused={refused.code}>
        refused: <code>{refused.code}</code> — {refused.code === "too-hot" ? `the kiln goes to cone 10; cone ${refused.payload?.val} was asked` : JSON.stringify(refused.payload)}
      </p>
    {/if}

    <h3>the log <span class="count">{firings.length}</span></h3>
    {#if firings.length}
      <ul class="log" data-field="log">
        {#each firings as f (f.id)}<li data-firing={f.id}>#{f.id} · cone {f.cone} · {f.hours} h · by {f.loadedBy}</li>{/each}
      </ul>
    {:else}
      <p class="empty" data-field="log">nothing fired yet</p>
    {/if}

    {#if readings.length || heating}
      <h3>heat <span class="count">{readings.length}</span></h3>
      <ol class="readings" data-field="readings">
        {#each readings as r (r.minute)}<li>{r.minute}′ <b>{Math.round(r.celsius)}°</b> {r.phase}</li>{/each}
      </ol>
      {#if left}<p class="left" data-field="heat-left">{left}</p>{/if}
    {/if}

    <h3>the frames crossing <span class="hint">→ to the kiln · ← from it; the last 16</span></h3>
    <pre class="frames" data-field="frames">{frames.join("\n")}</pre>
  {/if}
</section>

<style>
  .kiln-live { padding: 16px 20px; max-width: 880px; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .kiln-live h2 { font-family: var(--font-display); font-weight: var(--weight-strong); margin: 0 0 8px; }
  .kiln-live h3 { font-weight: var(--weight-medium); margin: 16px 0 6px; }
  .kiln-live .what { font-family: var(--sans); color: var(--ink); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius); padding: 10px 12px; line-height: 1.5; }
  .kiln-live .state, .kiln-live .hint, .kiln-live .count, .kiln-live .empty, .kiln-live .left { color: var(--muted); }
  .kiln-live .state.failed { color: var(--warn); }
  .kiln-live .store { color: var(--live); }
  .kiln-live .store.memory { color: var(--warn); }
  .kiln-live .controls { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; }
  .kiln-live input { width: 4em; font: inherit; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); padding: 3px 6px; }
  .kiln-live button { font: inherit; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); padding: 3px 10px; cursor: pointer; }
  .kiln-live button:hover:not(:disabled), .kiln-live button:global([data-sheet~=hover]) { background: var(--hover); }
  .kiln-live button:disabled, .kiln-live button:global([data-sheet~=disabled]) { color: var(--disabled); cursor: default; }
  .kiln-live button:focus-visible, .kiln-live input:focus-visible, .kiln-live button:global([data-sheet~=focus]), .kiln-live input:global([data-sheet~=focus]) { outline: 2px solid var(--focus); }
  .kiln-live .refused { color: var(--warn); border-left: 3px solid var(--warn); padding-left: 8px; }
  .kiln-live .log, .kiln-live .readings { margin: 0; padding-left: 20px; }
  .kiln-live .readings { display: flex; flex-wrap: wrap; gap: 4px 14px; list-style: none; padding: 0; }
  .kiln-live .frames { font-family: var(--mono); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius); padding: 8px; overflow: auto; white-space: pre; min-height: 3em; }
</style>
