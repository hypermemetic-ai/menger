<!-- EDITOR-9: HTML (and XML, Svelte) painted — a component of its own so it is a module of its
     own, fetched the first time code-card draws a block in it; the parser is
     its Lezer grammar (@lezer/html, which carries its own highlight tags), the painting code-paint's. -->
<script module>
  export const uses = [];
</script>

<script>
  import { parser } from "@lezer/html";
  import Paint from "./code-paint.svelte";

  let { code = "", lang = "html" } = $props();
</script>

<Paint {code} {parser} name={lang}/>
