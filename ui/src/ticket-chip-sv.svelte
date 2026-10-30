<!-- FRAME-25: a ticket named on a page in real Svelte (svelte/1),
     ticket-chip.rhx's twin — its id at once, its title when `get` answers.
     ticket-page-sv imports it (bundled into that module, on its bus); it is a
     component in its own right too. -->
<script module>
  export const uses = ["tickets.get"];
</script>

<script>
  import { live } from "plexus-ui/host";

  let { id, kind } = $props();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  const t = live({ snapshot: "tickets.get", params: () => ({ id }), key: "id" });
  const title = $derived($t.rows[0]?.title ?? "");
</script>

<a class="chip" data-chip={kind} data-chip-id={id} href={base + "ticket/" + id}>
  <span class="chip-kind">{kind}</span>
  <span class="chip-code">{id}</span>
  <span class="chip-title" data-chip-title>{title}</span>
</a>

<style>
  .chip { display: inline-flex; gap: 6px; align-items: center; max-width: 100%; padding: 2px 10px 2px 8px; font-size: 12px; white-space: nowrap; color: var(--chip-fg); text-decoration: none; background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); transition: background-color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .chip:hover, .chip:global([data-sheet~=hover]) { background: var(--chip-hover); border-color: var(--line); }
  .chip:focus-visible, .chip:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .chip-kind { font-size: 10px; color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .chip-code { color: var(--accent); }
  .chip-title { max-width: 28ch; overflow: hidden; text-overflow: ellipsis; color: var(--ink); }
</style>
