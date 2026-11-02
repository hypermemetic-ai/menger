<!-- EDITOR-9: a mermaid diagram, drawn by Pele (obsidianmd/pele on npm, MIT;
     COMPILE-9's `draw`, answered `pele`). Its own component so it is its own
     module in the bundle: ticket-view places it as a bound mark over each
     ```mermaid fence it draws, and the host fetches this module the first time
     one is on screen — a ticket with no diagram never loads Pele. Pele's
     `mount` draws the SVG into its element (it escapes the diagram's text,
     filters links and styles and limits sizes, its "Security"), reads the
     fonts from CSS and fits the width; one SVG serves light and dark, its
     colours the theme's tokens on Pele's CSS variables. A diagram Pele
     refuses says so, its words and line, labelled; ticket-view keeps the
     fence's source under the mark, shown whenever no diagram is drawn. -->
<script module>
  export const uses = [];
</script>

<script>
  import { mount as draw } from "pele";

  let { source = "" } = $props();
  let host = $state(null);
  let said = $state(null); // { message, line } when Pele refused it
  let drawn = $state(false);

  $effect(() => {
    if (!host) return;
    const text = String(source ?? "");
    said = null;
    drawn = false;
    let m;
    try {
      m = draw(host, text, {
        zoom: false,
        onRender: () => { drawn = true; said = null; },
        onError: (e) => { drawn = false; said = { message: String(e?.message ?? e), line: e?.line || null }; },
      });
      if (m.error) said = { message: String(m.error?.message ?? m.error), line: m.error?.line || null };
      else if (m.result) drawn = true;
    } catch (e) {
      said = { message: String(e?.message ?? e), line: e?.line || null };
    }
    return () => { try { m?.destroy(); } catch { /* gone with its element */ } };
  });
</script>

<figure class="mermaid-diagram" data-surface="mermaid-diagram" data-state={said ? "refused" : drawn ? "drawn" : "drawing"}>
  <div class="svg" bind:this={host} hidden={!!said} aria-label="a diagram"></div>
  {#if said}<figcaption class="no" data-refused>mermaid{said.line ? ` · line ${said.line}` : ""} · Pele refused it: {said.message}</figcaption>{/if}
</figure>

<style>
  .mermaid-diagram { margin: 0; color: var(--ink); }
  .svg {
    --pele-bg: transparent;
    --pele-fg: var(--ink);
    --pele-muted: var(--muted);
    --pele-line: var(--muted);
    --pele-surface: var(--panel);
    --pele-surface-alt: var(--hover);
    --pele-border: var(--line);
    --pele-accent: var(--accent);
    --pele-font: var(--sans);
    --pele-radius: var(--radius);
    --pele-series-1: var(--accent);
    --pele-series-2: var(--kind-scope);
    --pele-series-3: var(--kind-execution);
    --pele-series-4: var(--kind-spike);
    --pele-series-5: var(--kind-gap);
    --pele-series-6: var(--live);
    --pele-series-7: var(--warn);
    --pele-series-8: var(--kind-doc);
    overflow-x: auto;
    max-width: 100%;
  }
  .svg :global(svg) { display: block; max-width: 100%; height: auto; margin: 0 auto; }
  .no { font-family: var(--font-text); font-size: 11px; color: var(--warn); }
</style>
