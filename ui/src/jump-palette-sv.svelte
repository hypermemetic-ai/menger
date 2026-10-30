<!-- FRAME-14: jump-palette in real Svelte (svelte/1), jump-palette.rhx's twin
     — `/` opens it; `find` after 80 ms of quiet, the newest query only, its
     hits first and `search`'s best below; Enter jumps to the first, Escape
     closes. Nothing is asked until there is a query (FRAME-16's `when:`):
     a module's live spec takes neither `when` nor `debounce_ms`, so the
     pause and the hold are here. -->
<script module>
  export const uses = ["tickets.find", "tickets.search"];
</script>

<script>
  import { bus } from "plexus-ui/host";

  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  const NONE = { subscribe: (fn) => (fn({ rows: [], status: "pending" }), () => {}) };
  const QUIET_MS = 80;

  let open = $state(false);
  let q = $state("");
  let asked = $state("");
  let timer = null;
  const found = $derived(asked ? b.live({ snapshot: "tickets.find", params: { query: asked, limit: 12 }, key: "id" }) : NONE);
  const searched = $derived(asked ? b.live({ snapshot: "tickets.search", params: { query: asked, limit: 8 }, key: "id" }) : NONE);

  function close() {
    clearTimeout(timer);
    open = false;
    q = "";
    asked = "";
  }
  function keyed(ev) {
    const tag = ev.target?.tagName?.toLowerCase?.() ?? "";
    if (ev.key === "/" && !open && tag !== "input" && tag !== "textarea" && tag !== "select") {
      ev.preventDefault();
      open = true;
    } else if (ev.key === "Escape" && open) close();
  }
  function typed(ev) {
    q = ev.currentTarget.value;
    clearTimeout(timer);
    timer = setTimeout(() => (asked = q), QUIET_MS);
  }
  function entered(ev) {
    if (ev.key !== "Enter") return;
    const f = $found.rows;
    if (f.length > 0) {
      b.navigate(base + "ticket/" + f[0].id);
      close();
    }
  }
  function focus(node) {
    node.focus();
  }
  $effect(() => () => clearTimeout(timer));
</script>

<svelte:window onkeydown={keyed}/>

<div class="jump-palette" data-palette>
  {#if open}
    <div class="palette-sheet" role="dialog" aria-label="jump to a ticket">
      <input class="palette-input" data-palette-input spellcheck="false" autocomplete="off" use:focus
             placeholder="jump to a ticket — an id or words" value={q} oninput={typed} onkeydown={entered}/>
      {#if q !== ""}
        <ul class="palette-hits" data-palette-hits>
          {#each $found.rows as f (f.id)}
            <li><a class="palette-hit" data-palette-hit={f.id} href={base + "ticket/" + f.id} onclick={close}><span class="row-id">{f.id}</span> <span class="row-title">{f.title}</span></a></li>
          {/each}
        </ul>
        <p class="palette-label">in titles and bodies</p>
        <ul class="palette-hits search" data-search-hits>
          {#each $searched.rows as h (h.id)}
            <li><a class="palette-hit" data-search-hit={h.id} href={base + "ticket/" + h.id} onclick={close}><span class="row-id">{h.id}</span> <span class="row-title">{h.title}</span></a></li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</div>
