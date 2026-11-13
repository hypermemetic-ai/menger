<!-- The global keys: the one place a page-wide chord is bound (owner,
     2026-11-13: "we need one global keybinding registry with global
     behaviors"; the contract: "a page-wide chord belongs to one component").
     GLOBAL lists every global action with its chords; a press emits the
     action's name on the bus (`note.open`, `palette.open`, `page.save`,
     `page.edit`) and whatever carries the action out hears it there — as it
     hears the top bar's buttons, which emit the same names. No other
     component binds a chord of GLOBAL's, except an editor that takes one over
     while it is on the page (editor-body's ctrl+s and ctrl+k, `only`).
     Placed by app-top's bound mark, so it is on every page with the top bar
     — every layout of slots.json (tests/pages/keys.mjs holds that). Several
     mounted at once (the sheet draws many tops) bind once: the first holds
     the keys, the next takes them when it goes. On the app's own page only:
     drawn in a studio frame it binds nothing. `sheet` draws the list for the
     sheet (#/sheet) and binds nothing.
     The chords are matched by the letter the press types (the engine reads
     `KeyboardEvent.key`), so one with option held never matches on a Mac —
     option+n types no letter there. ctrl+m went: Firefox mutes the tab on it
     and the page never hears it (owner, 2026-11-13, Firefox on macOS: "ctrl m
     mutes … only mute"). The note's chords are ctrl+shift+l and cmd+shift+l,
     and `g n` — plain keys no browser takes, outside a text field only (a
     chord with no ctrl, cmd or option is never bound in inputs: it would eat
     typing); ctrl+alt+n stays for Linux and Windows, alt+meta+n for the day
     the engine matches the physical key. -->
<script module>
  export const uses = [];

  /** Every global action: its name on the bus, the chords that run it, and what the overlay says. */
  export const GLOBAL = [
    { action: "palette.open", chords: ["ctrl+k"], label: "jump to a ticket" },
    { action: "note.open", chords: ["ctrl+shift+l", "shift+meta+l", "g n", "ctrl+alt+n", "alt+meta+n"], label: "write a note" },
    { action: "page.save", chords: ["ctrl+s", "meta+s"], label: "save" },
    { action: "page.edit", chords: ["ctrl+e"], label: "edit this page in the studio (an editor's)" },
  ];

  /** The keys of a chord as the bar shows them: the first chord of `action`. */
  export const capsOf = (action) => (GLOBAL.find((g) => g.action === action)?.chords[0] ?? "").split("+").map((c) => c.replace(/^meta$/, "cmd"));

  // The mounted registries' buses, in the order they came: the first holds the keys.
  const mounted = [];
  let off = null;
  function bind(b) {
    const keys = {};
    for (const g of GLOBAL) for (const chord of g.chords) keys[chord] = { to: () => b.emit(g.action, {}), label: g.label, in_inputs: /(^|\+)(ctrl|meta|alt)\+/.test(chord) };
    return b.keys(keys);
  }
  function hold(b) {
    mounted.push(b);
    if (mounted.length === 1) off = bind(b);
    return () => {
      const i = mounted.indexOf(b);
      mounted.splice(i, 1);
      if (i !== 0) return;
      off?.();
      off = mounted.length ? bind(mounted[0]) : null;
    };
  }
</script>

<script>
  import { bus } from "plexus-ui/host";

  let { sheet = null } = $props();
  const b = bus();
  // The shell serves the app at `/` as well as `/app/` (owner, 2026-11-13: the keys were dead at the bare
  // address), so "the app's own page" is every page but the studio's and a frame's.
  const onApp = !/^\/(studio|frame)(\/|$)/.test(location.pathname) && window.parent === window;
  $effect(() => {
    if (sheet != null || !onApp) return;
    return hold(b);
  });
</script>

{#if sheet != null}
  <ul class="keys-global" data-surface="keys-global">
    {#each GLOBAL as g (g.action)}
      <li class="kg-row" data-action-name={g.action}>
        <span class="kg-chords">{#each g.chords as chord (chord)}<span class="kg-chord" data-chord={chord}>{#each chord.split("+") as cap}<kbd class="kg-cap">{cap}</kbd>{/each}</span>{/each}</span>
        <code class="kg-action">{g.action}</code>
        <span class="kg-label">{g.label}</span>
      </li>
    {/each}
  </ul>
{:else}
  <span class="keys-global" data-surface="keys-global" hidden></span>
{/if}

<style>
  ul.keys-global { list-style: none; margin: 0; padding: 10px 14px; display: grid; gap: 6px; font-size: 12px; color: var(--muted); background: var(--panel); }
  .kg-row { display: flex; gap: 10px; align-items: center; flex-wrap: wrap; }
  .kg-chords { display: inline-flex; gap: 8px; flex-wrap: wrap; }
  .kg-chord { display: inline-flex; gap: 3px; }
  .kg-cap { display: inline-block; min-width: 16px; padding: 0 4px; font: 10px/15px var(--font-text); text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
  .kg-action { color: var(--ink); font-size: 11px; }
</style>
