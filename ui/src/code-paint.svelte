<!-- EDITOR-9: code painted — a sibling each language's component (code-rust,
     code-python, …) inlines, given the language CodeMirror parses it with. The
     parse runs in idle time, a slice at a time (`startParse` / `advance`), so a
     long block never holds a key; until it is done the code shows plain. The
     spans carry @lezer/highlight's classes (`tok-keyword`, `tok-string`, …),
     painted with the theme's code tokens. Given `parser` (a Lezer grammar's, or
     a stream language's `language` whose parser it uses). Its painting is also
     kept on `globalThis.mengerCodeLanguages` (by its name, `{ paint(text) }` →
     `[{ from, to, cls }]`) and said as the window event `menger:code-language`,
     so the editor paints a fence's source the same way once a block in it has
     been drawn — the parse runs here, in this module's own copy of Lezer (a tree
     from one bundle's copy carries props another's highlighter cannot read). -->
<script module>
  export const uses = [];
</script>

<script>
  import { classHighlighter, highlightTree } from "@lezer/highlight";

  let { code = "", parser = null, language = null, name = "" } = $props();
  let runs = $state(null); // [{ text, cls }] once painted

  /** The highlighted ranges of `text` by `grammar`, at once (the editor's, for a fence's source). */
  const rangesOf = (grammar, text) => {
    const out = [];
    highlightTree(grammar.parse(text), classHighlighter, (from, to, cls) => out.push({ from, to, cls }));
    return out;
  };

  $effect(() => {
    const text = String(code ?? ""), grammar = parser ?? language?.parser ?? null;
    runs = null;
    if (!grammar) return;
    try {
      const kept = (globalThis.mengerCodeLanguages ??= new Map());
      if (!kept.has(name)) { kept.set(name, { paint: (t) => rangesOf(grammar, t) }); dispatchEvent(new CustomEvent("menger:code-language", { detail: name })); }
    } catch { /* no window */ }
    let cancelled = false, handle = null;
    const idle = globalThis.requestIdleCallback ? (f) => globalThis.requestIdleCallback(f, { timeout: 500 }) : (f) => setTimeout(() => f({ timeRemaining: () => 8 }), 1);
    const stop = (h) => (globalThis.cancelIdleCallback ? globalThis.cancelIdleCallback(h) : clearTimeout(h));
    const parse = grammar.startParse(text);
    const step = (deadline) => {
      handle = null;
      let tree = null;
      while (!cancelled && !(tree = parse.advance())) if (deadline.timeRemaining() < 1) { handle = idle(step); return; }
      if (cancelled || !tree) return;
      const out = [];
      let at = 0;
      highlightTree(tree, classHighlighter, (from, to, cls) => {
        if (from > at) out.push({ text: text.slice(at, from), cls: "" });
        out.push({ text: text.slice(from, to), cls });
        at = to;
      });
      if (at < text.length) out.push({ text: text.slice(at), cls: "" });
      runs = out;
    };
    handle = idle(step);
    return () => { cancelled = true; if (handle != null) stop(handle); };
  });
</script>

<pre class="painted" data-surface="code-paint" data-state={runs ? "painted" : "painting"} data-lang={name || undefined}><code>{#if runs}{#each runs as r, i (i)}{#if r.cls}<span class={r.cls}>{r.text}</span>{:else}{r.text}{/if}{/each}{:else}{code}{/if}</code></pre>

<style>
  .painted { margin: 0; padding: 0; overflow: visible; font-family: var(--font-text); font-size: 12.5px; line-height: 1.55; white-space: pre; color: var(--ink); background: none; }
  .painted code { font: inherit; padding: 0; background: none; }
  .painted :global(.tok-keyword), .painted :global(.tok-atom), .painted :global(.tok-bool) { color: var(--code-keyword); }
  .painted :global(.tok-string), .painted :global(.tok-string2), .painted :global(.tok-literal), .painted :global(.tok-url) { color: var(--code-string); }
  .painted :global(.tok-number) { color: var(--code-number); }
  .painted :global(.tok-comment) { color: var(--code-comment); font-style: italic; }
  .painted :global(.tok-typeName), .painted :global(.tok-className), .painted :global(.tok-namespace) { color: var(--code-type); }
  .painted :global(.tok-definition), .painted :global(.tok-labelName) { color: var(--code-def); }
  .painted :global(.tok-propertyName) { color: var(--code-property); }
  .painted :global(.tok-meta), .painted :global(.tok-macroName) { color: var(--code-meta); }
  .painted :global(.tok-operator), .painted :global(.tok-punctuation) { color: var(--muted); }
  .painted :global(.tok-inserted) { color: var(--diff-add-fg); }
  .painted :global(.tok-deleted) { color: var(--diff-del-fg); }
  .painted :global(.tok-invalid) { color: var(--warn); }
  .painted :global(.tok-heading), .painted :global(.tok-strong) { font-weight: var(--weight-strong); }
  .painted :global(.tok-emphasis) { font-style: italic; }
</style>
