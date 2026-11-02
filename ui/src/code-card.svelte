<!-- EDITOR-9: a block of code — every ```<lang> fence, and every <m-code> —
     drawn as one card, the same in the editor (a block the caret is not in)
     and the read-only view: ticket-view places it over the fence it drew. A
     quiet tint, a small caption (the language, and <m-code>'s `path` and
     `why`), a copy button that puts the exact code on the clipboard, and
     horizontal scroll — code never wraps. The code shows plain at once; a
     language this card knows is painted by its own component (code-rust,
     code-javascript, … code-legacy), placed as a bound mark over it, so each
     language is a module of its own, fetched the first time a block in it is
     drawn. A language it does not know, or none, stays plain in the same card.
     `langOf(info)` names the component's language for a fence's info string
     (the editor highlights a fence's source with the same parser). -->
<script module>
  export const uses = [];

  /** A language's names, as fences say them (after @codemirror/language-data's names and aliases). */
  const NAMES = {
    rust: ["rust", "rs"],
    javascript: ["javascript", "js", "mjs", "cjs", "node", "ecmascript"],
    jsx: ["jsx"],
    typescript: ["typescript", "ts", "mts", "cts"],
    tsx: ["tsx"],
    python: ["python", "py", "py3"],
    json: ["json", "jsonc", "json5", "geojson"],
    yaml: ["yaml", "yml"],
    go: ["go", "golang"],
    sql: ["sql", "sqlite", "postgres", "postgresql", "mysql"],
    html: ["html", "htm", "xhtml", "xml", "svg", "svelte", "vue"],
    css: ["css"],
    shell: ["sh", "bash", "shell", "zsh", "console", "shellsession", "fish"],
    toml: ["toml"],
    diff: ["diff", "patch"],
    haskell: ["haskell", "hs"],
    dockerfile: ["dockerfile", "docker"],
    lua: ["lua"],
  };
  const BY_NAME = new Map(Object.entries(NAMES).flatMap(([lang, names]) => names.map((n) => [n, lang])));
  /** The language of a fence's info string (`rust`, `ts title="x"`, `language-py`), or null. */
  export const langOf = (info) => BY_NAME.get(String(info ?? "").trim().split(/\s+/)[0].replace(/^language-/, "").toLowerCase()) ?? null;
  /** Each language's painting component. */
  export const PAINTER = { rust: "rust", javascript: "javascript", jsx: "javascript", typescript: "javascript", tsx: "javascript", python: "python", json: "json", yaml: "yaml", go: "go", sql: "legacy", html: "html", css: "css", shell: "legacy", toml: "legacy", diff: "legacy", haskell: "legacy", dockerfile: "legacy", lua: "legacy" };
</script>

<script>
  import CodeRust from "plexus-ui:code-rust";
  import CodeJavascript from "plexus-ui:code-javascript";
  import CodePython from "plexus-ui:code-python";
  import CodeJson from "plexus-ui:code-json";
  import CodeYaml from "plexus-ui:code-yaml";
  import CodeGo from "plexus-ui:code-go";
  import CodeHtml from "plexus-ui:code-html";
  import CodeCss from "plexus-ui:code-css";
  import CodeLegacy from "plexus-ui:code-legacy";

  let { code = "", info = "", path = "", why = "" } = $props();
  const COMPONENTS = { rust: CodeRust, javascript: CodeJavascript, python: CodePython, json: CodeJson, yaml: CodeYaml, go: CodeGo, html: CodeHtml, css: CodeCss, legacy: CodeLegacy };
  const said = $derived(String(info ?? "").trim().split(/\s+/)[0] || "");
  const lang = $derived(langOf(said));
  const Painter = $derived(lang ? COMPONENTS[PAINTER[lang]] : null);
  let copied = $state("");
  async function copy() {
    try { await navigator.clipboard.writeText(code); copied = "copied"; }
    catch { copied = "could not copy"; }
  }
</script>

<figure class="code-card" data-surface="code-card" data-lang={lang ?? "plain"}>
  <figcaption class="cap">
    <span class="lang" data-field="lang">{said || "text"}</span>
    {#if path}<span class="path" data-field="path">{path}</span>{/if}
    {#if why}<span class="why" data-field="why">{why}</span>{/if}
    <button type="button" class="copy" data-action="copy" aria-label="copy the code" onclick={copy}>{copied || "copy"}</button>
  </figcaption>
  <div class="scroll">
    {#if Painter}<div class="paint"><Painter {code} {lang}/></div>{/if}
    <pre class="plain"><code>{code}</code></pre>
  </div>
</figure>

<style>
  .code-card { margin: 6px 0; padding: 8px 12px 10px; background: var(--panel); border-radius: var(--radius); }
  .cap { display: flex; flex-wrap: wrap; gap: 4px 10px; align-items: baseline; margin: 0 0 4px; font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  .path { color: var(--ink); }
  .copy { margin-left: auto; padding: 0 6px; font: inherit; color: var(--muted); background: none; border: 0; border-radius: var(--radius-sm); cursor: pointer; }
  .copy:hover, .copy:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }
  .copy:focus-visible, .copy:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .scroll { overflow-x: auto; }
  .plain { margin: 0; padding: 0; font-family: var(--font-text); font-size: 12.5px; line-height: 1.55; white-space: pre; color: var(--ink); background: none; border-radius: 0; }
  .plain code { font: inherit; padding: 0; background: none; }
  /* Painted, the plain copy goes. */
  .paint:has(:global([data-state=painted])) + .plain { display: none; }
  .paint:has(:global([data-state=painted])) { display: block; }
  .paint { display: none; }
</style>
