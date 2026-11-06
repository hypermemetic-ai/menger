<!-- COMPS-4: what a ```component definition's block says under its card — this page's save of it
     (saving · saved, where: the live bundle, or the saver's own namespace, and the version · not saved,
     the refusal and its line), else what the store holds for its name (tag-components' table). Mounted
     by ticket-view under each ```component fence's code card, so the editor says it too.
     COMPS-5: v1's custom element (`v1`, plexus-ui's element/1) is said as such.
     COMPS-8: the definition's source is hidden — this line stands in its place, and its `source` opens
     the code card (`card`, ticket-view's) and closes it again. Closed on every load; open stays open
     while the page redraws the body (a save, an outside write), kept here by name.
     Marks: `[data-surface=component-def]` (`data-name`, `data-v1`, `data-state=saving|authored|refused|unsaved|unknown`,
     `data-open`), `[data-field=line]`, `[data-refused]`, `[data-field=source]` (`aria-expanded`). -->
<script module>
  export const uses = [];
  /** The definitions whose source is open on this page, by name. */
  const opened = new Set();
</script>

<script>
  import { onDestroy } from "svelte";
  import { listen, registry, want } from "./tag-components.svelte";
  let { name, source, html = false, v1 = false, card = null } = $props();

  let open = $state(opened.has(name));
  $effect(() => { card?.classList.toggle("def-open", open); });
  const toggle = () => { open = !open; if (open) opened.add(name); else opened.delete(name); };

  let tick = $state(0);
  onDestroy(listen(() => tick++));
  want();

  const said = $derived.by(() => {
    tick;
    const r = registry.results.get(name);
    if (r && r.source === source) {
      if (r.state === "saving") return { state: "saving", text: "saving to plexus-ui…" };
      if (r.state === "authored") return { state: "authored", text: `saved · ${r.own ? "your namespace, drawn framed" : "the live bundle, drawn natively"} · version ${String(r.version).slice(0, 12)}` };
      return { state: "refused", text: `not saved · ${r.reason}${r.line != null ? ` · line ${r.line}` : ""}: ${r.message}` };
    }
    if (!registry.ready) return { state: "unknown", text: "" };
    const row = registry.table.get(name);
    if (row?.from === "bundle") return { state: "authored", text: "in the live bundle · drawn natively" };
    if (row && row.source === source) return { state: "authored", text: `saved · ${row.by ?? row.author}'s, drawn framed · version ${String(row.version).slice(0, 12)}` };
    if (row) return { state: "unsaved", text: `the saved ${name} is ${row.by ?? row.author}'s and differs: saved again when this ticket is` };
    return { state: "unsaved", text: "not saved yet: saved with the ticket" };
  });
</script>

<div class="def" data-surface="component-def" data-name={name} data-state={said.state} data-html={html ? "" : undefined} data-v1={v1 ? "" : undefined} data-open={open ? "" : undefined}>
  <span class="tag">&lt;{name}&gt; · component{v1 ? " · v1's element, framed" : html ? " · html, framed" : ""}</span>
  {#if said.text}<span class="line" data-field="line" data-refused={said.state === "refused" ? "" : undefined}>{said.text}</span>{/if}
  {#if card}<button type="button" class="src" data-field="source" aria-expanded={open} onclick={toggle}>{open ? "hide source" : "source"}</button>{/if}
</div>

<style>
  .def { display: flex; flex-wrap: wrap; gap: 4px 10px; align-items: baseline; margin: 2px 0 6px; font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  .tag { color: var(--ink); }
  .def[data-state=refused] .line { color: var(--warn); }
  .def[data-state=authored] .line { color: var(--live); }
  .src { font: inherit; color: var(--accent); background: none; border: 0; padding: 0; cursor: pointer; }
  .src:hover, .src:global([data-sheet~=hover]) { text-decoration: underline; }
  .src:focus-visible, .src:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
</style>
