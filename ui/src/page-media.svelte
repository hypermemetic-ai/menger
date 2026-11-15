<!-- EDITOR-32: the media page, #/media (owner, 2026-11-15: "a media page that shows all saved media, should be
     able to get to the tickets they are on too"). svelte/1. The app's top in its `top` section and the
     palette in its `palette` section (app-top by ui/slots.json), as every page; between, a grid of every
     file `media.list` answers the reader, newest first:

     - an image as a thumbnail from `/media/<hash>` — never inside a link or a control, so a click on it
       opens it large (EDITOR-31's image-zoom: the grid is `data-zoomable`);
     - any other kind as its card: the kind, its name, its size (media-draw's `size`), a download;
     - under each, where it is used: a ticket by the one link helper (ticket-links' `ticketHref`), a
       document by `#/doc/<id>`.

     Chips filter by kind (`media.list --kind`); a page is 48 files, the next asked with `after` when the
     end of the grid scrolls into view, or on "more".
     Marks: `[data-surface=media-page]`, the chips `[data-kind-chip=all|image|video|audio|pdf|file]`
     (`aria-pressed`), the grid `[data-media-grid][data-zoomable]`, a file `[data-hash][data-kind]` with
     `[data-field=name|size|kind]`, `img[data-thumb]`, `[data-action=download]`, its owners
     `[data-owner=ticket|doc][data-id]`; `[data-action=more]`, `[data-empty]`, `[data-refused]`,
     `[data-loading]`. `sheet` = "loading" draws the skeleton and asks nothing. -->
<script module>
  export const uses = ["media.list"];

  export const KINDS = ["image", "video", "audio", "pdf", "file"];
  export const PAGE = 48;
</script>

<script>
  import { untrack } from "svelte";
  import { bus } from "plexus-ui/host";
  import { size } from "./media-draw.svelte";
  import { ticketHref } from "./ticket-links.svelte";
  let { sheet = null } = $props();

  const b = bus();
  let kind = $state(null);
  let rows = $state([]);
  let next = $state(null);
  let status = $state("pending");
  let message = $state("");
  let asking = 0;

  const src = (hash) => `${typeof location === "undefined" ? "" : location.origin}/media/${hash}`;
  const when = (at) => (at ? new Date(at * 1000).toISOString().slice(0, 10) : "");
  const ext = (r) => (/\.([a-z0-9]{1,6})$/i.exec(r.name ?? "")?.[1] ?? r.kind).toUpperCase();

  /** One page of `media.list`: the first (`more` false, the grid replaced) or the next. */
  async function load(more) {
    const n = ++asking;
    const params = { limit: PAGE, ...(kind ? { kind } : {}), ...(more && next ? { after: next } : {}) };
    status = more ? "more" : "pending";
    let a;
    try {
      a = await b.call("media.list", params);
    } catch (e) {
      a = { ok: false, events: [], message: e?.message ?? String(e) };
    }
    if (n !== asking) return;
    const events = Array.isArray(a) ? a : (a?.events ?? []);
    const bad = events.find((e) => e?.type === "error" || e?.type === "not_found");
    if (a?.ok === false || bad) {
      status = "refused";
      message = a?.message || bad?.message || "the list was refused";
      return;
    }
    const got = events.filter((e) => e?.type === "listed");
    const seen = new Set(more ? rows.map((r) => r.hash) : []);
    rows = more ? [...rows, ...got.filter((r) => !seen.has(r.hash))] : got;
    next = events.find((e) => e?.type === "page")?.next ?? null;
    status = "live";
  }

  $effect(() => {
    const k = kind;
    if (sheet === "loading" || k === undefined) return;
    untrack(() => load(false));
  });

  function pick(k) {
    if (kind === k) return;
    kind = k;
    rows = [];
    next = null;
  }

  // The end of the grid in view: the next page, once.
  let end = $state(null);
  $effect(() => {
    if (!end || typeof IntersectionObserver === "undefined") return;
    const seen = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting) && next && status === "live") load(true);
    }, { rootMargin: "400px" });
    seen.observe(end);
    return () => seen.disconnect();
  });
</script>

<section class="page page-media">
  <div data-slot="top" data-here="media"></div>
  <header class="page-title">
    <h1>media</h1>
    <span class="sub">every file kept for a ticket or a document you can read</span>
  </header>
  <div class="media-page" data-surface="media-page" data-status={sheet === "loading" ? "pending" : status}>
    <div class="chips" role="group" aria-label="kind">
      {#each [null, ...KINDS] as k (k ?? "all")}
        <button type="button" class="chip" data-kind-chip={k ?? "all"} aria-pressed={kind === k ? "true" : "false"} onclick={() => pick(k)}>{k ?? "all"}</button>
      {/each}
    </div>
    {#if sheet === "loading" || (status === "pending" && rows.length === 0)}
      <div class="grid">
        {#each [0, 1, 2, 3] as i (i)}<div class="tile" data-loading=""><div class="face"></div><div class="cap">&nbsp;</div></div>{/each}
      </div>
    {:else if status === "refused"}
      <p class="empty" data-refused="">refused: {message}</p>
    {:else if rows.length === 0}
      <p class="empty" data-empty="">{kind ? `no ${kind} kept for anything you can read` : "no file kept for anything you can read"}</p>
    {:else}
      <div class="grid" data-media-grid="" data-zoomable="">
        {#each rows as r (r.hash)}
          <figure class="tile" data-hash={r.hash} data-kind={r.kind}>
            {#if r.kind === "image"}
              <div class="face"><img data-thumb="" src={src(r.hash)} alt={r.name ?? "image"} loading="lazy" width={r.width || undefined} height={r.height || undefined} /></div>
            {:else}
              <div class="face card"><span class="ext" aria-hidden="true">{ext(r)}</span><span class="kind" data-field="kind">{r.kind}</span></div>
            {/if}
            <figcaption class="cap">
              <span class="line">
                <span class="name" data-field="name" title={r.name ?? r.hash}>{r.name ?? r.hash.slice(0, 12)}</span>
                <span class="size" data-field="size">{size(r.bytes)}</span>
                <a class="dl" data-action="download" href={src(r.hash)} download={r.name ?? r.hash}>download</a>
              </span>
              <span class="meta"><time datetime={when(r.at)}>{when(r.at)}</time>{#if r.width && r.height} · {r.width}×{r.height}{/if}</span>
              <span class="owners">
                {#each r.tickets as id (id)}<a class="owner" data-owner="ticket" data-id={id} href={ticketHref({ id })}>{id}</a>{/each}
                {#each r.docs as id (id)}<a class="owner doc" data-owner="doc" data-id={id} href={`#/doc/${id}`}>doc {id}</a>{/each}
              </span>
            </figcaption>
          </figure>
        {/each}
      </div>
      <div class="end" bind:this={end}>
        {#if next}<button type="button" class="more" data-action="more" disabled={status === "more"} onclick={() => load(true)}>{status === "more" ? "loading…" : "more"}</button>{/if}
      </div>
    {/if}
  </div>
  <div data-slot="palette"></div>
</section>

<style>
  .page-media { padding-bottom: 40px; min-height: 100vh; min-height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .page-title { display: flex; align-items: baseline; gap: 12px; max-width: 1340px; margin: 0 auto; padding: 16px 24px 0; }
  .page-title h1 { margin: 0; font-family: var(--font-display); font-size: 20px; font-weight: var(--weight-strong); color: var(--ink); }
  .sub { color: var(--muted); font-size: 12px; }
  .media-page { max-width: 1340px; margin: 0 auto; padding: 14px 24px 0; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; margin: 0 0 14px; }
  .chip { padding: 2px 12px; font: inherit; font-size: 12px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); cursor: pointer; }
  .chip:hover, .chip:global([data-sheet~=hover]) { background: var(--chip-hover); }
  .chip:focus-visible, .chip:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .chip[aria-pressed=true], .chip:global([data-sheet~=checked]) { color: var(--accent); border-color: var(--accent); background: var(--sel); }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: 14px; }
  .tile { display: flex; flex-direction: column; min-width: 0; margin: 0; overflow: hidden; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .face { display: flex; align-items: center; justify-content: center; aspect-ratio: 4 / 3; overflow: hidden; background: var(--hover); }
  .face img { display: block; width: 100%; height: 100%; object-fit: contain; cursor: zoom-in; }
  .card { flex-direction: column; gap: 4px; }
  .ext { font-family: var(--font-display); font-size: 22px; font-weight: var(--weight-strong); color: var(--muted); }
  .kind { font-size: 11px; color: var(--muted); }
  .cap { display: flex; flex-direction: column; gap: 4px; padding: 8px 10px 10px; font-size: 12px; }
  .line { display: flex; align-items: baseline; gap: 8px; min-width: 0; }
  .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: var(--weight-medium); }
  .size, .meta { color: var(--muted); font-variant-numeric: tabular-nums; }
  .meta { font-size: 11px; }
  .dl { color: var(--accent); text-decoration: none; }
  .dl:hover { text-decoration: underline; }
  .owners { display: flex; flex-wrap: wrap; gap: 4px; }
  .owner { padding: 0 8px; font-size: 11px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); text-decoration: none; }
  .owner:hover { color: var(--accent); background: var(--chip-hover); }
  .owner:focus-visible, .dl:focus-visible, .more:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .end { display: flex; justify-content: center; min-height: 1px; padding: 18px 0 0; }
  .more { padding: 4px 18px; font: inherit; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-pill); cursor: pointer; }
  .more:hover { background: var(--hover); }
  .more:disabled, .more:global([data-sheet~=disabled]) { color: var(--disabled); cursor: default; }
  [data-loading] .face, [data-loading] .cap { color: transparent; background: var(--skeleton); }
  .empty { margin: 8px 0; color: var(--muted); }
  [data-refused] { color: var(--warn); }
  @media (max-width: 720px) { .media-page, .page-title { padding-left: 12px; padding-right: 12px; } .grid { grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 10px; } }
</style>
