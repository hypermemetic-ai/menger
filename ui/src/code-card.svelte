<!-- EDITOR-9: a block of code — every ```<lang> fence, and every <m-code> —
     drawn as one card, the same in the editor (a block the caret is not in)
     and the read-only view: ticket-view places it over the fence it drew. A
     quiet tint, a small caption (the language, and <m-code>'s `path` and
     `why`), a copy button that puts the exact code on the clipboard, and
     horizontal scroll. EDITOR-12: wrap is an option on each block (the owner:
     "ideally code wrap is an option on code") — a *wrap* toggle beside copy,
     wrapped by default on a phone (under 641 px, so the page never scrolls
     sideways) and not on a desktop; wrapped, each line hangs its rows past its
     own leading whitespace; unwrapped, only the code's box scrolls. The choice
     is this viewer's, kept in this browser (`menger.code.wrap`, by the code's
     hash), never in the markdown. The tinted box holds code only — the
     language and the controls on its top edge; <m-code>'s `path` and `why`
     are notes under it, small and muted (the owner: "any notes belong below
     the code snippet box. not inside it"). Copy is the exact code. The code shows plain at once; a
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
  /** EDITOR-12: `code` as its lines, each with its leading whitespace's width in columns (a tab 4, at most 8),
   *  the hanging indent a wrapped line keeps on a phone. */
  export const linesOf = (code) => String(code ?? "").split("\n").map((text) => {
    let ind = 0;
    for (const ch of text) { if (ch === " ") ind++; else if (ch === "\t") ind += 4; else break; }
    return { text, ind: Math.min(ind, 8) };
  });
  /** EDITOR-12: each viewer's wrap choices, by the code's hash (FNV-1a), the newest 200; storage may be absent. */
  export const WRAP = "menger.code.wrap";
  export const codeKey = (code) => { let h = 0x811c9dc5; for (let i = 0; i < code.length; i++) { h ^= code.charCodeAt(i); h = Math.imul(h, 0x01000193); } return (h >>> 0).toString(36) + ":" + code.length; };
  const readWraps = () => { try { const v = JSON.parse(localStorage.getItem(WRAP) || "{}"); return v && typeof v === "object" ? v : {}; } catch { return {}; } };
  function keepWrap(key, on) {
    try {
      const all = readWraps();
      delete all[key];
      all[key] = on;
      const keys = Object.keys(all);
      for (const k of keys.slice(0, Math.max(0, keys.length - 200))) delete all[k];
      localStorage.setItem(WRAP, JSON.stringify(all));
    } catch { /* a private window: the choice lasts the page */ }
  }
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
  const lineCount = $derived(String(code ?? "").split("\n").length);
  // EDITOR-12: wrapped or not — this viewer's choice for this code, else wrapped on a phone alone.
  const phoneQuery = typeof matchMedia === "function" ? matchMedia("(max-width: 640px)") : null;
  let phone = $state(phoneQuery?.matches ?? false);
  $effect(() => {
    if (!phoneQuery) return;
    const on = () => (phone = phoneQuery.matches);
    phoneQuery.addEventListener("change", on);
    return () => phoneQuery.removeEventListener("change", on);
  });
  const key = $derived(codeKey(String(code ?? "")));
  let chosen = $state(null); // { key, on } once this viewer toggles, or as kept
  const kept = $derived(chosen?.key === key ? chosen.on : readWraps()[key]);
  const wrap = $derived(typeof kept === "boolean" ? kept : phone);
  function toggleWrap() { const on = !wrap; chosen = { key, on }; keepWrap(key, on); }
  let copied = $state("");
  async function copy() {
    try { await navigator.clipboard.writeText(code); copied = "copied"; }
    catch { copied = "could not copy"; }
  }
</script>

<figure class="code-card" data-surface="code-card" data-lang={lang ?? "plain"} data-wrap={wrap ? "on" : "off"}>
  <div class="box">
    <div class="edge">
      <span class="lang" data-field="lang">{said || "text"}</span>
      <button type="button" class="ctl" data-action="wrap" aria-pressed={wrap} title={wrap ? "show the lines as they are, scrolling" : "wrap the long lines"} onclick={toggleWrap}>wrap</button>
      <button type="button" class="ctl" data-action="copy" aria-label="copy the code" onclick={copy}>{copied || "copy"}</button>
    </div>
    <div class="scroll" data-field="code">
      {#if Painter}<div class="paint"><Painter {code} {lang}/></div>{/if}
      <pre class="plain"><code>{#each linesOf(code) as l, i (i)}<span class="ln" style={`--ind: ${l.ind}`}>{l.text}{i < lineCount - 1 ? "\n" : ""}</span>{/each}</code></pre>
    </div>
  </div>
  {#if path || why}
    <figcaption class="notes" data-field="notes">
      {#if path}<span class="path" data-field="path">{path}</span>{/if}
      {#if why}<span class="why" data-field="why">{why}</span>{/if}
    </figcaption>
  {/if}
</figure>

<style>
  .code-card { margin: 6px 0; min-width: 0; max-width: 100%; }
  .box { padding: 6px 12px 10px; background: var(--panel); border-radius: var(--radius); }
  .edge { display: flex; gap: 4px 6px; align-items: baseline; margin: 0 0 4px; font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  .lang { margin-right: auto; overflow-wrap: anywhere; }
  .ctl { padding: 0 6px; font: inherit; color: var(--muted); background: none; border: 0; border-radius: var(--radius-sm); cursor: pointer; }
  .ctl:hover, .ctl:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }
  .ctl:focus-visible, .ctl:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .ctl[aria-pressed=true], .ctl:global([data-sheet~=checked]) { color: var(--accent); }
  /* Notes under the box, never in it: small, muted, in the flow. */
  .notes { display: flex; flex-wrap: wrap; gap: 2px 10px; margin: 4px 0 0; padding: 0 2px; font-family: var(--font-text); font-size: 11px; line-height: 1.5; color: var(--muted); overflow-wrap: anywhere; }
  .path { color: var(--ink); }
  .scroll { overflow-x: auto; }
  .plain { margin: 0; padding: 0; font-family: var(--font-text); font-size: 12.5px; line-height: 1.55; white-space: pre; color: var(--ink); background: none; border-radius: 0; }
  .plain code { font: inherit; padding: 0; background: none; }
  /* Painted, the plain copy goes. */
  .paint:has(:global([data-state=painted])) + .plain { display: none; }
  .paint:has(:global([data-state=painted])) { display: block; }
  .paint { display: none; }
  /* EDITOR-12: wrapped, a line is a block whose rows hang past its own indent; painted or plain alike. */
  .code-card[data-wrap=on] .scroll { overflow-x: visible; }
  .code-card[data-wrap=on] .plain, .code-card[data-wrap=on] .paint :global(.painted) { white-space: pre-wrap; overflow-wrap: anywhere; tab-size: 4; }
  .code-card[data-wrap=on] .plain .ln, .code-card[data-wrap=on] .paint :global(.painted .ln) { display: block; padding-left: calc((var(--ind, 0) + 2) * 1ch); text-indent: calc((var(--ind, 0) + 2) * -1ch); }
</style>
