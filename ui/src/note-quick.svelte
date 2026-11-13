<!-- A note, written from anywhere (owner, 2026-11-13: "create a note using
     a keybinding that brings up a little window to enter in my note"). It
     opens on `note.open` on the bus: the global keys emit it (keys-global:
     ctrl+m, and ctrl+alt+n off a Mac) and so does the top bar's `note`
     button. A small window: a title line and the note; cmd/ctrl+enter keeps
     it (`docs.create { title, body }`), escape closes it and keeps nothing.
     With no title the note's first line is its title. Kept, the window
     closes and says so for a moment, with a link to the note at `#/doc/<id>`.
     Placed by app-top's bound mark, so it is on every page with the top bar.
     The service is still `docs` until its rename to notes. SURFACE-10:
     `sheet`, a text, draws it open holding that text for the sheet (#/sheet)
     — nothing focused, its keys not bound, `note.open` not heard. -->
<script module>
  export const uses = ["docs.create"];
  const TITLE_MAX = 200;

  /** The title a note is kept under: the one typed, else its first line. */
  export function titleOf(title, body) {
    const t = (title.trim() || body.split("\n").find((l) => l.trim()) || "").replace(/^#+\s*/, "").trim().replace(/\s+/g, " ");
    return [...t].slice(0, TITLE_MAX).join("");
  }
</script>

<script>
  import { bus } from "plexus-ui/host";

  let { sheet = null } = $props();
  const b = bus();
  let open = $state(sheet != null);
  let title = $state("");
  let body = $state(sheet ?? "");
  let saving = $state(false);
  let refused = $state("");
  let kept = $state(null); // { id, title } for a moment after a keep
  let keptTimer = null;
  let field = $state(null);
  const named = $derived(titleOf(title, body));

  function show() {
    if (open) { field?.focus(); return; }
    open = true;
    refused = "";
    kept = null;
  }
  function close() {
    open = false;
    title = "";
    body = "";
    refused = "";
  }
  async function keep() {
    if (saving) return;
    if (!named) { refused = "a note needs a title or a line of text"; return; }
    saving = true;
    refused = "";
    const r = await b.call("docs.create", { title: named, body });
    saving = false;
    const made = r.events?.find((e) => e.type === "created")?.doc;
    if (!(r.ok && made)) {
      refused = String(r.message ?? r.events?.find((e) => e.message)?.message ?? "refused").replace(/^Invalid params: /, "");
      return;
    }
    close();
    kept = { id: made.id, title: made.title ?? named };
    clearTimeout(keptTimer);
    keptTimer = setTimeout(() => (kept = null), 5_000);
  }

  // The chord is keys-global's: it emits `note.open`, as the bar's button does.
  if (sheet == null) b.on("note.open", show);
  // The open window's keys: bound while it is open, let go when it closes.
  $effect(() => {
    if (!open || sheet != null) return;
    return b.keys({
      "escape": { to: close, in_inputs: true, label: "close the note, keeping nothing" },
      "ctrl+enter": { to: keep, in_inputs: true, label: "keep the note" },
      "meta+enter": { to: keep, in_inputs: true, label: "keep the note" },
    });
  });
  $effect(() => () => clearTimeout(keptTimer));
  function focus(node) {
    if (sheet == null) node.focus();
  }
</script>

<div class="note-quick" data-surface="note-quick" data-sheet-open={sheet != null ? "" : undefined}>
  {#if open}
    {#if sheet == null}<div class="scrim" data-scrim onclick={close} aria-hidden="true"></div>{/if}
    <form class="note-sheet" role="dialog" aria-label="a new note" data-note-open onsubmit={(e) => { e.preventDefault(); keep(); }}>
      <input class="note-title" data-field="note-title" type="text" placeholder="title (or the first line)" aria-label="the note's title"
             spellcheck="false" autocomplete="off" maxlength={TITLE_MAX} bind:value={title} oninput={() => (refused = "")}/>
      <textarea class="note-body" data-field="note-body" rows="8" placeholder="write your note…" aria-label="the note"
                bind:value={body} bind:this={field} oninput={() => (refused = "")} use:focus></textarea>
      {#if refused}<p class="note-refused" data-refused role="alert">{refused}</p>{/if}
      <div class="note-foot">
        <span><kbd>esc</kbd> close</span>
        <button type="submit" class="note-keep" data-action="note.keep" disabled={saving || !named}>{saving ? "keeping…" : "keep"} <kbd>⌘/ctrl</kbd><kbd>enter</kbd></button>
      </div>
    </form>
  {/if}
  {#if kept}
    <p class="note-kept" data-note-kept={kept.id} role="status">note kept: <a href={"#/doc/" + kept.id} onclick={() => (kept = null)}>{kept.title}</a></p>
  {/if}
</div>

<style>
  .note-quick { display: contents; }
  .note-quick .scrim { position: fixed; inset: 0; z-index: 11; background: var(--shadow); }
  .note-quick .note-sheet { position: fixed; z-index: 12; top: 14vh; left: 50%; transform: translateX(-50%); display: flex; flex-direction: column; width: min(520px, calc(100vw - 32px)); white-space: normal; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-xl); box-shadow: 0 10px 30px var(--shadow); overflow: hidden; }
  .note-quick .note-title { padding: 12px 14px 8px; font: inherit; font-family: var(--font-display); font-weight: var(--weight-bold); font-size: 15px; color: var(--ink); background: none; border: 0; outline: none; }
  .note-quick .note-body { padding: 4px 14px 12px; min-height: 140px; max-height: 50vh; resize: vertical; font: inherit; font-size: 14px; line-height: 1.5; color: var(--ink); background: none; border: 0; border-bottom: var(--border) solid var(--line); outline: none; }
  .note-quick[data-sheet-open] .note-sheet { position: static; width: auto; transform: none; }
  .note-quick .note-title:focus-visible, .note-quick .note-body:focus-visible, .note-quick .note-title:global([data-sheet~=focus]), .note-quick .note-body:global([data-sheet~=focus]) { box-shadow: inset 2px 0 0 var(--focus); }
  .note-quick .note-refused { margin: 6px 14px 0; font-size: 12px; color: var(--warn); }
  .note-quick .note-foot { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 8px 14px; font-size: 11px; color: var(--muted); }
  .note-quick .note-keep { display: inline-flex; gap: 6px; align-items: center; padding: 4px 10px; font: inherit; font-size: 12px; color: var(--bg); background: var(--accent); border: 0; border-radius: var(--radius); cursor: pointer; }
  .note-quick .note-keep:disabled, .note-quick .note-keep:global([data-sheet~=disabled]) { opacity: .5; cursor: default; }
  .note-quick .note-keep kbd { color: var(--ink); }
  .note-quick .note-kept { position: fixed; z-index: 12; right: 16px; bottom: 16px; margin: 0; padding: 8px 12px; font-size: 13px; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); box-shadow: 0 4px 14px var(--shadow); }
  .note-quick .note-kept a { color: var(--accent); }
  .note-quick kbd { display: inline-block; min-width: 18px; padding: 0 5px; font: 11px/17px var(--font-text); text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
</style>
