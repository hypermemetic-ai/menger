<!-- CODE-6: code-ide's follow — the toggle and the name of the agent followed, and the session they open. Bundled
     into code-ide (its bus is code-ide's); a component in its own right too (the sheet draws it alone, asking
     nothing). The choice is the reader's, this browser's (localStorage), off until ticked, shared by every
     code-ide on the page and kept for the next (`FOLLOW`). On, it opens the ui service's watch as the page's one
     session (`ui.watch { session, follows }`, `SESSION`: `s-` and 12 hex, made once a page): every code-ide on
     the page asks the same spec, which is one collection, one stream. Its shows (`shown`, folded by `seq`) are
     handed to `onheard`, each as often as the collection moves (code-ide applies a `seq` once); `onopen` is
     called when the watch opens or opens again (code-ide says `ui.here`). Off, the watch is let go and nothing
     is asked; turned off or given another name, `ui.follow` says so at once, before the watch is let go.
     Marks: `[data-field=follow]` (`.on`), `input[data-action=follow]`, `input[data-field=follow-who]`. -->
<script module>
  export const uses = ["ui.versions", "ui.watch", "ui.follow"];

  /** Whom a session may follow: an agent's name or a principal, as the ui service's `AgentName` takes it. */
  export const agentOk = (s) => /^[A-Za-z0-9._:@-]{1,256}$/.test(s);
  const KEY = "menger.code-ide.follow";
  function kept() {
    try {
      const v = JSON.parse(localStorage.getItem(KEY) || "null");
      return { on: v?.on === true, who: typeof v?.who === "string" && agentOk(v.who) ? v.who : "builder" };
    } catch { return { on: false, who: "builder" }; }
  }
  /** The reader's choice, shared by every code-ide on the page. */
  export const FOLLOW = $state(kept());
  /** The page's one session. */
  export const SESSION = `s-${Array.from(crypto.getRandomValues(new Uint8Array(6)), (x) => x.toString(16).padStart(2, "0")).join("")}`;
</script>

<script>
  import { untrack } from "svelte";
  import { bus } from "plexus-ui/host";

  let { sheet = null, disabled = false, onheard = null, onopen = null, onchange = null } = $props();
  const b = bus();
  const asks = sheet == null && !disabled;
  // The snapshot is the live version alone — a row with no `seq`, kept by nobody: the collection holds the shows.
  const watch = asks ? b.live({
    snapshot: "ui.versions", params: { live: true }, changes: "ui.watch", key: "seq", on: { shown: "upsert:." },
    changes_params: () => ({ session: SESSION, follows: [FOLLOW.who] }),
    when: () => FOLLOW.on,
  }) : null;
  $effect(() => {
    if (!watch) return;
    const rows = watch.rows;
    untrack(() => { for (const row of rows ?? []) onheard?.(row); });
  });
  $effect(() => {
    if (!watch) return;
    const s = watch.status, on = FOLLOW.on;
    untrack(() => { if (on && s) onopen?.(s); });
  });

  let whoText = $state(FOLLOW.who);
  let box = $state(null);
  // Another code-ide on the page changed it: the box says so (not while it is being written in).
  $effect(() => { const w = FOLLOW.who; untrack(() => { if (document.activeElement !== box) whoText = w; }); });

  function set(on, who = FOLLOW.who) {
    const was = FOLLOW.on;
    if (asks && ((was && !on) || (on && was && who !== FOLLOW.who))) b.call("ui.follow", { session: SESSION, follows: on ? [who] : [] });
    FOLLOW.who = who;
    FOLLOW.on = on;
    try { localStorage.setItem(KEY, JSON.stringify({ on, who })); } catch { /* a private window */ }
    onchange?.(on, who);
  }
  function key(ev) {
    if (ev.key === "Enter") { ev.preventDefault(); ev.currentTarget.blur(); }
    else if (ev.key === "Escape") { ev.preventDefault(); whoText = FOLLOW.who; ev.currentTarget.blur(); }
  }
  function done() {
    const w = whoText.trim() || "builder";
    if (w === FOLLOW.who) { whoText = w; return; }
    if (!agentOk(w)) { whoText = FOLLOW.who; onchange?.(FOLLOW.on, FOLLOW.who, "an agent is the name it signs in as (builder) or its principal (key:…)"); return; }
    whoText = w;
    set(FOLLOW.on, w);
  }
</script>

<span class="follow" class:on={FOLLOW.on} data-field="follow" title="an agent you follow opens files here: what it shows (ui.show) opens in this view">
  <label><input type="checkbox" data-action="follow" checked={FOLLOW.on} disabled={!asks} onchange={(ev) => set(ev.currentTarget.checked)} /> follow</label>
  <input class="who" type="text" data-field="follow-who" aria-label="the agent to follow: its name or principal" size="9" spellcheck="false" autocomplete="off"
    bind:this={box} bind:value={whoText} disabled={!asks} onkeydown={key} onblur={done} />
</span>

<style>
  .follow { display: inline-flex; gap: 5px; align-items: center; white-space: nowrap; color: var(--muted); font-family: var(--font-text); }
  .follow label { display: inline-flex; gap: 4px; align-items: center; cursor: pointer; }
  .follow.on { color: var(--live); }
  .follow input[type=checkbox] { margin: 0; accent-color: var(--accent); }
  .who { field-sizing: content; min-width: 8ch; max-width: 24ch; padding: 0 5px; font: inherit; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .who:disabled, .who:global([data-sheet~=disabled]) { color: var(--muted); }
  .follow input:focus-visible, .follow input:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
</style>
