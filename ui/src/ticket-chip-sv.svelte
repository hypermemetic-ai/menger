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
