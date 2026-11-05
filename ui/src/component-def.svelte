<!-- COMPS-4: what a ```component definition's block says under its card — this page's save of it
     (saving · saved, where: the live bundle, or the saver's own namespace, and the version · not saved,
     the refusal and its line), else what the store holds for its name (tag-components' table). Mounted
     by ticket-view under each ```component fence's code card, so the editor says it too.
     COMPS-5: v1's custom element (`v1`, plexus-ui's element/1) is said as such.
     Marks: `[data-surface=component-def]` (`data-name`, `data-v1`, `data-state=saving|authored|refused|unsaved|unknown`),
     `[data-field=line]`, `[data-refused]`. -->
<script module>
  export const uses = [];
</script>

<script>
  import { onDestroy } from "svelte";
  import { listen, registry, want } from "./tag-components.svelte";
  let { name, source, html = false, v1 = false } = $props();

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

<div class="def" data-surface="component-def" data-name={name} data-state={said.state} data-html={html ? "" : undefined} data-v1={v1 ? "" : undefined}>
  <span class="tag">&lt;{name}&gt;{v1 ? " · v1's element, framed" : html ? " · html, framed" : ""}</span>
  {#if said.text}<span class="line" data-field="line" data-refused={said.state === "refused" ? "" : undefined}>{said.text}</span>{/if}
</div>

<style>
  .def { display: flex; flex-wrap: wrap; gap: 4px 10px; align-items: baseline; margin: 2px 0 6px; font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  .tag { color: var(--ink); }
  .def[data-state=refused] .line { color: var(--warn); }
  .def[data-state=authored] .line { color: var(--live); }
</style>
