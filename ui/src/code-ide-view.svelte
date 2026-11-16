<!-- CODE-3: code-ide's lines — one CodeMirror EditorView, read-only, whose state is swapped per tab (each
     tab keeps its own state and its scroll), so a tab shown again is drawn from what this view holds and
     asks nothing. Bundled into code-ide; a component in its own right too (the sheet draws it alone).

     - `text` is drawn the moment it is given: a file of 20,000 lines is a CodeMirror document, whose
       view draws only the lines on screen (and a margin), so the page holds a few hundred line elements.
     - It is painted by its language after: the language from the path as code-card's `langOf` names it
       (`langOfPath`), its painter (code-rust, code-javascript, … code-legacy) placed hidden by its bound
       mark, so each grammar is a module of its own, fetched the first time a file in it is shown; the
       painter keeps its painting on `globalThis.mengerCodeLanguages` (code-paint), whose ranges this
       view lays over the lines on screen. A long file is painted a prefix at a time, in idle time,
       as far as it is read. A language with no painter stays plain.
     - A line number marks its line on a click, a range on a shift-click (`onmark([a, b])`); the marked
       lines are drawn on `--code-mark`, and a tab shown first with marks opens on them (`jump`, a mark
       and a count that moves, scrolls to one asked for by number). Every scroll is this view's
       scroller alone (never CodeMirror's scrollIntoView, which would move the page around a body).
     Marks: `[data-surface=code-ide-view]` (`data-key`, `data-lang`, `data-painted`), CodeMirror's own
     `.cm-line` and `.cm-gutterElement`, `.cm-marked` on a marked line. The view is the element's
     `codeView` (tests read the document through it). -->
<script module>
  import { langOf } from "./code-card.svelte";
  export const uses = [];

  /** The language of a file by its path, as code-card's `langOf` names it (`src/a.rs` → `rust`); null when none. */
  export function langOfPath(path) {
    const base = String(path ?? "").split("/").pop().toLowerCase();
    if (base === "dockerfile" || base.startsWith("dockerfile.")) return "dockerfile";
    const dot = base.lastIndexOf(".");
    return dot > 0 ? langOf(base.slice(dot + 1)) : null;
  }
</script>

<script>
  import { onDestroy, onMount, untrack } from "svelte";
  import { EditorState, RangeSet, RangeSetBuilder, StateEffect, StateField } from "@codemirror/state";
  import { Decoration, EditorView, GutterMarker, ViewPlugin, gutterLineClass, lineNumbers } from "@codemirror/view";
  import { PAINTER } from "./code-card.svelte";
  import CodeRust from "plexus-ui:code-rust";
  import CodeJavascript from "plexus-ui:code-javascript";
  import CodePython from "plexus-ui:code-python";
  import CodeJson from "plexus-ui:code-json";
  import CodeYaml from "plexus-ui:code-yaml";
  import CodeGo from "plexus-ui:code-go";
  import CodeHtml from "plexus-ui:code-html";
  import CodeCss from "plexus-ui:code-css";
  import CodeLegacy from "plexus-ui:code-legacy";

  /** `key`: the file's name for this view (repository, commit and path); `keep`: the keys of the open tabs. */
  let { text = null, key = "", path = "", mark: markProp = null, keep: keepProp = null, jump: jumpProp = null, onmark = null } = $props();
  /** A prop as a value: a bound mark hands every prop as a string, an object as its JSON (the sheet's tiles). */
  const valueOf = (v, none) => { if (typeof v !== "string") return v ?? none; try { return JSON.parse(v); } catch { return none; } };
  const mark = $derived(valueOf(markProp, null));
  const keep = $derived(valueOf(keepProp, null));
  const jump = $derived(valueOf(jumpProp, null));
  const COMPONENTS = { rust: CodeRust, javascript: CodeJavascript, python: CodePython, json: CodeJson, yaml: CodeYaml, go: CodeGo, html: CodeHtml, css: CodeCss, legacy: CodeLegacy };
  const lang = $derived(langOfPath(path));
  const Painter = $derived(lang && PAINTER[lang] ? COMPONENTS[PAINTER[lang]] : null);

  // ── the marked lines ─────────────────────────────────────────────────
  const setMark = StateEffect.define();
  const LINE = Decoration.line({ class: "cm-marked" });
  const NO = new (class extends GutterMarker { elementClass = "cm-marked-no"; })();
  const clampMark = (doc, m) => {
    if (!m) return null;
    const a = Math.max(1, Math.min(m[0], doc.lines));
    return [a, Math.max(a, Math.min(m[1], doc.lines))];
  };
  const markField = StateField.define({
    create: () => ({ m: null, lines: Decoration.none, nos: RangeSet.empty }),
    update(v, tr) {
      for (const e of tr.effects) if (e.is(setMark)) {
        const m = clampMark(tr.state.doc, e.value);
        const lines = new RangeSetBuilder(), nos = new RangeSetBuilder();
        if (m) for (let n = m[0]; n <= m[1]; n++) { const at = tr.state.doc.line(n).from; lines.add(at, at, LINE); nos.add(at, at, NO); }
        v = { m, lines: lines.finish(), nos: nos.finish() };
      }
      return v;
    },
    provide: (f) => [EditorView.decorations.from(f, (v) => v.lines), gutterLineClass.from(f, (v) => v.nos)],
  });

  // ── the painting: ranges `[{ from, to, cls }]` over a prefix `[0, to)`, drawn for the lines on screen ──
  const setPaint = StateEffect.define();
  const paintField = StateField.define({
    create: () => null,
    update(v, tr) { for (const e of tr.effects) if (e.is(setPaint)) v = e.value; return v; },
  });
  const MARKS = new Map();
  const markOf = (cls) => { let d = MARKS.get(cls); if (!d) MARKS.set(cls, (d = Decoration.mark({ class: cls }))); return d; };
  let wantMore = null; // the view's wish to have painted further, said outside an update
  const painting = ViewPlugin.fromClass(class {
    constructor(v) { this.decorations = this.build(v); }
    update(u) {
      if (u.viewportChanged || u.startState.field(paintField, false) !== u.state.field(paintField, false)) this.decorations = this.build(u.view);
    }
    build(v) {
      const p = v.state.field(paintField, false);
      if (!p) return Decoration.none;
      const { from, to } = v.viewport;
      if (to > p.to - 2000 && p.to < v.state.doc.length) wantMore?.();
      const out = new RangeSetBuilder();
      let lo = 0, hi = p.ranges.length;
      while (lo < hi) { const mid = (lo + hi) >> 1; if (p.ranges[mid].to <= from) lo = mid + 1; else hi = mid; }
      for (let i = lo; i < p.ranges.length; i++) {
        const r = p.ranges[i];
        if (r.from >= to) break;
        if (r.to > r.from && r.cls) out.add(r.from, r.to, markOf(r.cls));
      }
      return out.finish();
    }
  }, { decorations: (v) => v.decorations });

  // ── the view ─────────────────────────────────────────────────────────
  let host = $state(null);
  let view = null;
  let current = null; // the key drawn now
  const saved = new Map(); // key → { state, top, left }
  let painted = $state(false);

  function extensions() {
    return [
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      EditorView.contentAttributes.of({ tabindex: "0", "aria-label": "the file's lines", "aria-readonly": "true" }),
      lineNumbers({
        domEventHandlers: {
          mousedown(v, line, ev) {
            if (ev.button !== 0) return false;
            const n = v.state.doc.lineAt(line.from).number;
            const was = v.state.field(markField).m;
            onmark?.(ev.shiftKey && was ? [Math.min(was[0], n), Math.max(was[0], n)] : [n, n]);
            ev.preventDefault();
            return true;
          },
        },
      }),
      markField,
      paintField,
      painting,
    ];
  }

  /** The marked lines, scrolled to the first (a third down) — this view's scroller only, never the page. */
  function toMark(m) {
    if (!view || !m) return;
    const place = () => {
      if (!view) return;
      const doc = view.state.doc, at = clampMark(doc, m);
      const top = view.lineBlockAt(doc.line(at[0]).from).top;
      view.scrollDOM.scrollTop = Math.max(0, top - view.scrollDOM.clientHeight / 3);
    };
    place();
    // Once CodeMirror has measured the lines (a line's height is known only then), placed again: after its
    // measure, which runs in the next frame and keeps its own scroll anchor.
    requestAnimationFrame(() => requestAnimationFrame(place));
  }

  function show(k, t, m) {
    if (!view || t == null) return;
    if (current === k) {
      if (view.state.field(markField).m?.join() !== clampMark(view.state.doc, m)?.join()) view.dispatch({ effects: setMark.of(m) });
      return;
    }
    if (current) saved.set(current, { state: view.state, top: view.scrollDOM.scrollTop, left: view.scrollDOM.scrollLeft });
    const kept = saved.get(k);
    current = k;
    if (kept) {
      view.setState(kept.state);
      if (view.state.field(markField).m?.join() !== clampMark(view.state.doc, m)?.join()) view.dispatch({ effects: setMark.of(m) });
      view.scrollDOM.scrollTop = kept.top;
      view.scrollDOM.scrollLeft = kept.left;
    } else {
      view.setState(EditorState.create({ doc: t, extensions: extensions() }));
      view.scrollDOM.scrollTop = 0;
      view.scrollDOM.scrollLeft = 0;
      if (m) { view.dispatch({ effects: setMark.of(m) }); toMark(m); }
    }
    painted = !!view.state.field(paintField);
    paintSoon();
  }

  // ── painting, in idle time, a prefix at a time ───────────────────────
  const PREFIX = 400_000, MORE = 200_000;
  let idleAt = null;
  const idle = (f) => (globalThis.requestIdleCallback ? globalThis.requestIdleCallback(f, { timeout: 300 }) : setTimeout(f, 16));
  const unidle = (h) => (globalThis.cancelIdleCallback ? globalThis.cancelIdleCallback(h) : clearTimeout(h));
  function paintSoon() {
    if (idleAt != null || !view || !lang || !current) return;
    idleAt = idle(() => {
      idleAt = null;
      const k = current; // what is drawn when the browser is idle, not when it was asked
      if (!view || !k) return;
      const kept = globalThis.mengerCodeLanguages?.get(lang);
      if (!kept) return; // its painter is on its way: `menger:code-language` says when
      const doc = view.state.doc, p = view.state.field(paintField);
      const to = Math.min(doc.length, Math.max(PREFIX, view.viewport.to + MORE));
      if (p && p.to >= to) return;
      let ranges;
      try { ranges = kept.paint(doc.sliceString(0, to)); } catch { ranges = []; }
      if (!view || current !== k) return;
      view.dispatch({ effects: setPaint.of({ ranges, to }) });
      painted = true;
    });
  }
  wantMore = () => paintSoon();

  onMount(() => {
    view = new EditorView({ parent: host, state: EditorState.create({ doc: "", extensions: extensions() }) });
    host.codeView = view;
    const arrived = (ev) => { if (ev.detail === lang) paintSoon(); };
    addEventListener("menger:code-language", arrived);
    untrack(() => show(key, text, mark));
    return () => removeEventListener("menger:code-language", arrived);
  });
  onDestroy(() => { if (idleAt != null) unidle(idleAt); view?.destroy(); view = null; });

  $effect(() => {
    const k = key, t = text, m = mark;
    untrack(() => show(k, t, m));
  });
  // A line asked for by number (code-ide's box): scrolled to, here.
  $effect(() => {
    const j = jump;
    if (j?.mark) untrack(() => toMark(j.mark));
  });
  // A tab closed: its state goes.
  $effect(() => {
    const open = keep;
    if (!Array.isArray(open)) return;
    for (const k of [...saved.keys()]) if (!open.includes(k)) saved.delete(k);
  });
</script>

<div class="civ" data-surface="code-ide-view" data-key={key} data-lang={lang ?? "plain"} data-painted={painted ? "yes" : "no"} bind:this={host}></div>
{#if Painter}<div class="painter" hidden aria-hidden="true"><Painter code="" {lang}/></div>{/if}

<style>
  .civ { height: 100%; min-height: 0; min-width: 0; background: var(--panel); }
  .civ :global(.cm-editor) { height: 100%; color: var(--ink); background: var(--panel); font-size: 12.5px; }
  .civ :global(.cm-editor.cm-focused) { outline: none; }
  .civ :global(.cm-scroller) { font-family: var(--font-text); line-height: 1.55; }
  .civ :global(.cm-content) { padding: 6px 0; }
  .civ :global(.cm-content:focus-visible), .civ :global(.cm-content[data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: -2px; }
  .civ :global(.cm-line) { padding: 0 12px 0 4px; }
  .civ :global(.cm-gutters) { color: var(--muted); background: var(--panel); border: 0; }
  .civ :global(.cm-lineNumbers .cm-gutterElement) { min-width: 4ch; padding: 0 12px 0 8px; cursor: pointer; user-select: none; }
  .civ :global(.cm-lineNumbers .cm-gutterElement:hover), .civ :global(.cm-lineNumbers .cm-gutterElement[data-sheet~=hover]) { color: var(--accent); }
  .civ :global(.cm-line.cm-marked), .civ :global(.cm-gutterElement.cm-marked-no) { background: var(--code-mark); }
  .civ :global(.cm-gutterElement.cm-marked-no) { color: var(--ink); }
  /* Painted by its language: @lezer/highlight's classes, the theme's code tokens (code-paint's). */
  .civ :global(.tok-keyword), .civ :global(.tok-atom), .civ :global(.tok-bool) { color: var(--code-keyword); }
  .civ :global(.tok-string), .civ :global(.tok-string2), .civ :global(.tok-literal), .civ :global(.tok-url) { color: var(--code-string); }
  .civ :global(.tok-number) { color: var(--code-number); }
  .civ :global(.tok-comment) { color: var(--code-comment); font-style: italic; }
  .civ :global(.tok-typeName), .civ :global(.tok-className), .civ :global(.tok-namespace) { color: var(--code-type); }
  .civ :global(.tok-definition), .civ :global(.tok-labelName) { color: var(--code-def); }
  .civ :global(.tok-propertyName) { color: var(--code-property); }
  .civ :global(.tok-meta), .civ :global(.tok-macroName) { color: var(--code-meta); }
  .civ :global(.tok-operator), .civ :global(.tok-punctuation) { color: var(--muted); }
  .civ :global(.tok-inserted) { color: var(--diff-add-fg); }
  .civ :global(.tok-deleted) { color: var(--diff-del-fg); }
  .civ :global(.tok-invalid) { color: var(--warn); }
  .civ :global(.tok-heading), .civ :global(.tok-strong) { font-weight: var(--weight-strong); }
  .civ :global(.tok-emphasis) { font-style: italic; }
  .painter { display: none; }
</style>
