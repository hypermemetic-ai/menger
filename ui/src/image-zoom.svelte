<!-- EDITOR-31: an image in a body opens large on a click (owner, 2026-11-15: "I would like to be able to
     click images on the documents and ticket surface (or any surface they render in) and have them zoom in
     as a modal"). One listener for the whole page: placed by app-top's bound mark beside keys-global, so it
     is on every page with the top bar, and several mounted at once (the sheet draws many tops) listen once —
     the first holds the listener, the next takes it when it goes.

     What zooms is `zoomable(img)`: an image drawn in a body — a ticket's page and its blocks
     (`[data-ticket-body]`, `.blk-in`: the editor's drawn blocks are ticket-view's), a margin comment
     (`.md`), the sheet's media tile, any surface that marks itself `data-zoomable` — and not one inside a link or a control (a link card follows its link,
     image-pick's radios pick). A click with ctrl, cmd, shift or alt held is the browser's. A page drawn in
     a sandboxed frame is the frame's own; the listener doesn't reach it.

     The modal is a native `<dialog>` opened with `showModal()`: on the top layer, the focus held inside,
     Escape closing it (bound on the bus with `only` while it is open, so the page's own Escape waits) and the focus going back where it was. The image is drawn at most at its natural size
     and fitted to the window; its alt text is the caption; "open" opens the original in a new tab. A click
     anywhere but the image or the bar closes it. `sheet` draws the modal open, as the sheet shows it, and
     listens to nothing. -->
<script module>
  export const uses = [];

  const BODY = "[data-ticket-body], .blk-in, .md, [data-surface=media-draw], [data-zoomable]";
  const OWN = "a, button, label, [role=radio], [role=button], [role=option], [role=checkbox], [data-no-zoom]";

  /** Whether a click on `img` zooms it: drawn in a body, and not part of a link or a control. */
  export function zoomable(img) {
    if (!img || img.tagName !== "IMG" || !img.getAttribute("src")) return false;
    return !!img.closest(BODY) && !img.closest(OWN);
  }

  // The mounted zooms' openers, in the order they came: the first holds the page's listener.
  const mounted = [];
  let off = null;
  function listen(open) {
    const on = (ev) => {
      if (ev.button !== 0 || ev.defaultPrevented || ev.metaKey || ev.ctrlKey || ev.shiftKey || ev.altKey) return;
      const img = ev.target;
      if (!zoomable(img)) return;
      ev.preventDefault();
      open(img);
    };
    document.addEventListener("click", on);
    return () => document.removeEventListener("click", on);
  }
  function hold(open) {
    mounted.push(open);
    if (mounted.length === 1) off = listen(open);
    return () => {
      const i = mounted.indexOf(open);
      mounted.splice(i, 1);
      if (i !== 0) return;
      off?.();
      off = mounted.length ? listen(mounted[0]) : null;
    };
  }
</script>

<script>
  import { tick } from "svelte";
  import { bus } from "plexus-ui/host";

  let { sheet = null } = $props();
  let dialog = $state(null);
  // The sheet's image: a drawing of a screenshot, made here (the page loads no image from another origin).
  const SAMPLE = { src: "data:image/svg+xml," + encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="640" height="400"><rect width="640" height="400" fill="#8aa"/><rect width="640" height="80" fill="#577"/></svg>'), alt: "a screenshot, opened large" };
  let shown = $state(sheet != null ? SAMPLE : null); // { src, alt }
  const onApp = !/^\/(studio|frame)(\/|$)/.test(location.pathname) && window.parent === window;

  async function open(img) {
    shown = { src: img.currentSrc || img.src, alt: (img.getAttribute("alt") ?? "").trim() };
    await tick();
    if (dialog && !dialog.open) dialog.showModal();
  }
  function close() { dialog?.close(); }
  function closed() { shown = null; }
  // A click on the backdrop (the dialog itself, around its box) or on the frame around the image closes it.
  function clicked(ev) { if (!ev.target.closest?.("img, .iz-bar")) close(); }

  $effect(() => {
    if (sheet != null || !onApp) return;
    return hold(open);
  });
  // Escape is the zoom's while it is open, and only the zoom's: the page's own Escape (a ticket's page goes
  // back to the board) waits until it is closed.
  const b = bus();
  let offKeys = null;
  $effect(() => {
    const isOpen = sheet == null && !!shown;
    if (isOpen && !offKeys) offKeys = b.keys({ escape: { to: close, label: "close the image", in_inputs: true, only: true } });
    if (!isOpen && offKeys) { offKeys(); offKeys = null; }
  });
</script>

{#if sheet != null}
  <div class="image-zoom iz-sheet" data-surface="image-zoom">
    <div class="iz-box" data-open>
      {#if shown}<figure class="iz-fig"><img class="iz-img" src={shown.src} alt={shown.alt} data-field="image"/>{#if shown.alt}<figcaption data-field="caption">{shown.alt}</figcaption>{/if}</figure>{/if}
      <div class="iz-bar"><span class="iz-hint">esc</span><button type="button" class="iz-btn" data-action="close">close</button></div>
    </div>
  </div>
{:else}
  <dialog class="image-zoom" data-surface="image-zoom" data-open={shown ? "" : undefined} bind:this={dialog} onclose={closed} onclick={clicked} aria-label={shown?.alt || "image"}>
    {#if shown}
      <div class="iz-bar">
        <a class="iz-btn" href={shown.src} target="_blank" rel="noopener noreferrer" data-action="open-original">open</a>
        <button type="button" class="iz-btn" data-action="close" onclick={close}>close <kbd>esc</kbd></button>
      </div>
      <figure class="iz-fig">
        <img class="iz-img" src={shown.src} alt={shown.alt} data-field="image"/>
        {#if shown.alt}<figcaption data-field="caption">{shown.alt}</figcaption>{/if}
      </figure>
    {/if}
  </dialog>
{/if}

<style>
  dialog.image-zoom { box-sizing: border-box; width: 100vw; height: 100vh; max-width: 100vw; max-height: 100vh; margin: 0; padding: 0; border: 0; background: transparent; color: var(--ink); overflow: hidden; }
  dialog.image-zoom[open] { display: flex; flex-direction: column; }
  dialog.image-zoom::backdrop { background: var(--bg); opacity: 0.94; }
  .iz-bar { display: flex; justify-content: flex-end; gap: 8px; padding: 10px 14px; }
  .iz-btn { display: inline-flex; gap: 6px; align-items: center; padding: 3px 10px; font-size: 12px; color: var(--ink); text-decoration: none; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: border-color var(--motion-fast) var(--ease); }
  .iz-btn:hover, .iz-btn:global([data-sheet~=hover]) { border-color: var(--muted); }
  .iz-btn:focus-visible, .iz-btn:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .iz-btn kbd { font: 10px/15px var(--font-text); color: var(--muted); }
  .iz-fig { flex: 1; min-height: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; margin: 0; padding: 0 24px 24px; }
  .iz-img { display: block; max-width: 100%; max-height: calc(100vh - 110px); width: auto; height: auto; object-fit: contain; background: var(--card); border-radius: var(--radius); box-shadow: 0 10px 40px var(--shadow); cursor: zoom-out; }
  .iz-fig figcaption { max-width: 70ch; padding: 2px 10px; font-size: 13px; color: var(--ink); text-align: center; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .iz-sheet .iz-box { position: relative; display: flex; flex-direction: column-reverse; padding: 12px; background: var(--bg); }
  .iz-sheet .iz-fig { padding: 0; }
  .iz-sheet .iz-img { max-height: 320px; }
  .iz-hint { align-self: center; font-size: 11px; color: var(--muted); }
  @media (max-width: 640px) {
    .iz-fig { padding: 0 8px 8px; }
  }
</style>
