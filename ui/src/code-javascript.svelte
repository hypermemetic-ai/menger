<!-- EDITOR-9: JavaScript and TypeScript (and JSX, TSX) painted — a component of its own so it is a module of its
     own, fetched the first time code-card draws a block in it; the parser is
     its Lezer grammar (@lezer/javascript, its dialects `ts` and `jsx`), the painting code-paint's. -->
<script module>
  export const uses = [];
</script>

<script>
  import { parser as base } from "@lezer/javascript";
  import Paint from "./code-paint.svelte";

  let { code = "", lang = "javascript" } = $props();
  const parser = $derived(base.configure({ dialect: [lang === "typescript" || lang === "tsx" ? "ts" : "", lang === "jsx" || lang === "tsx" ? "jsx" : ""].filter(Boolean).join(" ") }));
</script>

<Paint {code} {parser} name={lang}/>
