<!-- COMPS-3: `<m-live route widths>` and ```live (APP-110) — the test app
     at that route, drawn where the body has the tag: an ordinary svelte/1
     component, beside m-mockup (ticket-view's `TAGS` table names both). The
     app comes from the origin the shell stamps (`--live-origin`, `<meta
     name="menger-live-origin">`); none configured, `env=` naming another, or
     the origin this app's own, says so in words. A width the view names is
     drawn at that width and scaled to the column, with a switch when it names
     several; the label above the view, never inside it.
     Not `bus.frame`: the frame API draws a page's HTML or a stored component
     in plexus-ui's opaque, connectionless `/frame`, and this is the test
     app itself, at its own origin, with its own session and socket — so it is
     an `<iframe src>` on that origin (`allow-same-origin` there is the test
     app's own, never this page's), which the shell's `frame-src` admits
     (`--live-origin`) and test's `frame-ancestors` lets frame it. -->
<script module>
  import { pageSpec, widthsOf, startHeight, MAX } from "./m-mockup.svelte";

  export const uses = [];

  /** The live origin this server's bodies may frame (APP-110), stamped by the shell; "" when none. */
  const liveOrigin = () => (typeof document === "object" && document.querySelector('meta[name="menger-live-origin"]')?.content?.trim()) || "";

  /** What `<m-live>` / ```live `spec` draws: `{ src }`, or `{ refused }` in words. */
  export function liveView(spec) {
    const env = (spec.attrs.env || "test").toLowerCase();
    if (env !== "test") return { refused: `A live view draws the test app only; env=${env} is refused.` };
    const origin = liveOrigin();
    if (!origin) return { refused: "No live origin is configured on this server (--live-origin), so the test app is not drawn here." };
    if (typeof location === "object" && origin === location.origin) return { refused: "The live origin is this app's own, which is never framed with its scripts." };
    const route = spec.attrs.route || "#/";
    return { src: `${origin}/${route.replace(/^\//, "")}` };
  }
</script>

<script>
  /** `source`: the piece's markdown — `<m-live …></m-live>` or a ```live fence. */
  let { source = "" } = $props();
  const spec = $derived.by(() => { const s = pageSpec(source); return s?.kind === "live" ? s : null; });
  const sizes = $derived(widthsOf(spec?.attrs ?? {}));
  let pick = $state(0);
  const size = $derived(sizes[Math.min(pick, Math.max(0, sizes.length - 1))] ?? null);
  const label = $derived(spec?.attrs?.label || spec?.attrs?.title || "");
  const about = $derived(spec?.attrs?.description || "");
  let col = $state(0);
  const W = $derived(size ? size.w : Math.max(1, col));
  const scale = $derived(size && col > 0 ? Math.min(1, col / size.w) : 1);
  const H = $derived(Math.max(40, Math.min(MAX, size?.h ?? (Number(spec?.attrs?.height) > 0 ? Math.round(Number(spec.attrs.height)) : startHeight(W, !!size)))));
  const live = $derived(spec ? liveView(spec) : null);
</script>

{#if spec}
<figure class="pf" data-surface="page-frame" data-kind="live" data-state={live?.src ? "drawn" : "note"} data-width={size?.w ?? undefined}>
  {#if label || about || sizes.length > 1}
    <figcaption class="pf-head">
      {#if label}<span class="pf-label" data-field="label">{label}</span>{/if}
      {#if sizes.length > 1}
        <span class="pf-widths" role="group" aria-label="width">
          {#each sizes as s, i (i)}<button type="button" data-action="width" data-width={s.w} aria-pressed={i === pick} onclick={() => (pick = i)}>{s.w}{s.h ? `×${s.h}` : ""}</button>{/each}
        </span>
      {/if}
      {#if about}<span class="pf-about" data-field="description">{about}</span>{/if}
    </figcaption>
  {/if}
  {#if live?.refused}
    <p class="pf-note" data-field="note">{live.refused} <code>{spec.attrs.route || "#/"}</code></p>
  {:else}
    <div class="pf-stage" bind:clientWidth={col}>
      <div class="pf-box" style:width={size ? `${Math.round(W * scale)}px` : "100%"} style:height={`${Math.round(H * scale)}px`}>
        <iframe src={live.src} title={label || "the test app"} loading="lazy" sandbox="allow-scripts allow-same-origin allow-forms allow-popups" referrerpolicy="no-referrer"
                style:width={size ? `${W}px` : "100%"} style:height={`${H}px`} style:transform={scale < 1 ? `scale(${scale})` : null}></iframe>
      </div>
    </div>
  {/if}
</figure>
{/if}

<style>
  .pf { display: block; margin: 8px 0; min-width: 0; max-width: 100%; }
  .pf-head { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 10px; margin: 0 0 6px; font-family: var(--font-text); font-size: 12px; color: var(--muted); }
  .pf-label { color: var(--ink); font-weight: var(--weight-strong); overflow-wrap: anywhere; }
  .pf-about { flex-basis: 100%; overflow-wrap: anywhere; }
  .pf-widths { display: inline-flex; gap: 4px; }
  .pf-widths button { padding: 0 7px; font: inherit; font-size: 11px; line-height: 18px; color: var(--muted); background: none; border: var(--border) solid var(--line); border-radius: var(--radius-md); cursor: pointer; }
  .pf-widths button[aria-pressed=true], .pf-widths button:global([data-sheet~=checked]) { color: var(--ink); border-color: var(--muted); background: var(--panel); }
  .pf-stage { width: 100%; min-width: 0; }
  .pf-box { position: relative; max-width: 100%; overflow: hidden; border: var(--border) solid var(--line); border-radius: var(--radius); background: var(--panel); }
  .pf-box iframe { display: block; max-width: none !important; border: 0; transform-origin: 0 0; }
  .pf-note { margin: 0; font-family: var(--font-text); font-size: 12px; color: var(--muted); }
</style>
