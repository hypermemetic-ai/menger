<!-- FRAME-14: jump-palette in real Svelte (svelte/1), jump-palette.rhx's twin
     — `/` opens it; `find` after 80 ms of quiet, the newest query only, its
     hits first and `search`'s best below; Enter jumps to the first, Escape
     closes. FRAME-25: on FRAME-19's bus, as rhx's — nothing asked while
     there is no query (`when`), the typing coalesced by `debounce_ms` in the
     host's registry; no timer of its own. -->
<script module>
  // PERF-15: a row is the four fields drawn, not the whole summary row.
  const FIELDS = ["id", "title", "kind", "status"];
  export const uses = ["tickets.find", "tickets.search"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";

  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";

  let open = $state(false);
  let q = $state("");
  const found = live({ snapshot: "tickets.find", params: () => ({ query: q, limit: 12, fields: FIELDS }), key: "id", when: () => q.length > 0, debounce_ms: 80 });
  const searched = live({ snapshot: "tickets.search", params: () => ({ query: q, limit: 8 }), key: "id", when: () => q.length > 0, debounce_ms: 80 });
  const hits = $derived($found.rows);
  const best = $derived($searched.rows);

  function close() {
    open = false;
    q = "";
  }
  function keyed(ev) {
    const tag = ev.target?.tagName?.toLowerCase?.() ?? "";
    if (ev.key === "/" && !open && tag !== "input" && tag !== "textarea" && tag !== "select") {
      ev.preventDefault();
      open = true;
    } else if (ev.key === "Escape" && open) close();
  }
  function entered(ev) {
    if (ev.key !== "Enter" || hits.length === 0) return;
    b.navigate(base + "ticket/" + hits[0].id);
    close();
  }
  function focus(node) {
    node.focus();
  }
</script>

<svelte:window onkeydown={keyed}/>

<div class="jump-palette" data-palette>
  {#if open}
    <div class="palette-sheet" role="dialog" aria-label="jump to a ticket">
      <input class="palette-input" data-palette-input spellcheck="false" autocomplete="off" use:focus
             placeholder="jump to a ticket — an id or words" value={q} oninput={(ev) => (q = ev.currentTarget.value)} onkeydown={entered}/>
      {#if q !== ""}
        <ul class="palette-hits" data-palette-hits>
          {#each hits as f (f.id)}
            <li><a class="palette-hit" data-palette-hit={f.id} href={base + "ticket/" + f.id} onclick={close}><span class="row-id">{f.id}</span> <span class="row-title">{f.title}</span></a></li>
          {/each}
        </ul>
        <p class="palette-label">in titles and bodies</p>
        <ul class="palette-hits search" data-search-hits>
          {#each best as h (h.id)}
            <li><a class="palette-hit" data-search-hit={h.id} href={base + "ticket/" + h.id} onclick={close}><span class="row-id">{h.id}</span> <span class="row-title">{h.title}</span></a></li>
          {/each}
        </ul>
      {/if}
    </div>
  {/if}
</div>
