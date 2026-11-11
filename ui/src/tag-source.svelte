<!-- COMPS-16: a tag's own source in a body — what ticket-view, tag-components and a component that saves
     itself back into its tag (tweet-writer) read the same way, so the place a tag is drawn and the place it
     is saved are one. A tag in block form is its open line alone (`<tweet-writer>`, no indent; a name with
     a hyphen, never a house mark's `m-*`) and its close line alone (`</tweet-writer>`), every line between
     its children, outside fences and multi-line comments. `tagSpans(src)` is each, in order, by offset into
     `src` (`to` the end of the close line); a tag's place among the body's tags of its name is its index
     there. `childrenOf(source)` is a tag's children as structure (`{ tag, attrs, text }`, the text and the
     values decoded); `escText` / `escAttr` write them back. A module alone: no markup, no call. -->
<script module>
  export const uses = [];

  /** COMPS-23: the text a drawn tag just wrote of itself (its own save) — what draws the body skips drawing
   *  that tag again for it, so the component stays as it is (its caret, its focus). The newest few are kept. */
  // Kept on the page, not in this module: a sibling is bundled into each component that imports it, so the
  // writer's copy of this file and ticket-view's are two modules.
  const own = (globalThis[Symbol.for("menger.tag-source.own")] ??= []);
  export function wroteOwn(text) { own.push(String(text)); if (own.length > 32) own.shift(); }
  export const isOwn = (text) => own.includes(String(text));

  const FENCE = /^ {0,3}(`{3,}|~{3,})/;
  const ATTRS = `(?:\\s(?:[^>"']|"[^"]*"|'[^']*')*)?`;
  const NAME = "[a-z][a-z0-9]*(?:-[a-z0-9]+)+";
  const OPEN = new RegExp(`^<(${NAME})(${ATTRS})>\\s*$`);

  function fenceEnd(lines, i) {
    const m = FENCE.exec(lines[i]);
    const ch = m[1][0], len = m[1].length;
    for (let k = i + 1; k < lines.length; k++) {
      const c = /^ {0,3}(`{3,}|~{3,})\s*$/.exec(lines[k]);
      if (c && c[1][0] === ch && c[1].length >= len) return k + 1;
    }
    return lines.length;
  }

  /** The name of the block-form tag opened on `line`, or null (a house mark's `m-*` is not one). */
  export function openName(line) {
    const m = OPEN.exec(String(line ?? ""));
    return m && !m[1].startsWith("m-") ? m[1] : null;
  }

  /** The line after the close of the tag `name` opened on line `i` (its own name nested counted), or -1. */
  export function closeLine(lines, i, name) {
    const open = new RegExp(`^<${name}${ATTRS}>\\s*$`), close = new RegExp(`^</${name}>\\s*$`);
    let depth = 0;
    for (let k = i; k < lines.length; k++) {
      if (open.test(lines[k])) depth++;
      else if (close.test(lines[k]) && --depth === 0) return k + 1;
    }
    return -1;
  }

  /** Each block-form tag of `src`, in order: `{ name, from, to, text, n }` — `n` its index among the tags of
   *  its name. Fences and multi-line comments outside a tag hold none; what is inside a tag is its own. */
  export function tagSpans(src) {
    const text = String(src ?? "");
    const lines = text.split("\n");
    const at = [0];
    for (const l of lines) at.push(at[at.length - 1] + l.length + 1);
    const out = [];
    const seen = new Map();
    for (let i = 0; i < lines.length; i++) {
      if (FENCE.test(lines[i])) { i = fenceEnd(lines, i) - 1; continue; }
      if (/^ {0,3}<!--/.test(lines[i]) && !lines[i].includes("-->")) {
        while (i + 1 < lines.length && !lines[i].includes("-->")) i++;
        continue;
      }
      const name = openName(lines[i]);
      if (!name) continue;
      const end = closeLine(lines, i, name);
      if (end < 0) continue;
      const n = seen.get(name) ?? 0;
      seen.set(name, n + 1);
      const from = at[i], to = at[end] - 1;
      out.push({ name, from, to, text: text.slice(from, to), n });
      i = end - 1;
    }
    return out;
  }

  const ENTITY = { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'", "#39": "'", "#x27": "'" };
  /** `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;` decoded; any other kept as written. */
  export const decode = (t) => String(t ?? "").replace(/&(amp|lt|gt|quot|apos|#39|#x27);/g, (_, e) => ENTITY[e]);
  /** Words as a tag's text: `&` and `<` escaped, so no word closes or opens a tag. */
  export const escText = (t) => String(t ?? "").replace(/&/g, "&amp;").replace(/</g, "&lt;");
  /** A value as an attribute's, double-quoted. */
  export const escAttr = (t) => String(t ?? "").replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");

  /** An open tag's attributes, in order, their values decoded: `[[name, value], …]`. */
  export function attrsOf(s) {
    const out = [];
    for (const m of String(s ?? "").matchAll(/([a-zA-Z_:][\w:.-]*)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?/g)) {
      out.push([m[1].toLowerCase(), decode(m[2] ?? m[3] ?? m[4] ?? "")]);
    }
    return out;
  }

  /** A block-form tag's children (`source` its whole text, open line to close line) as structure: each
   *  child tag `{ tag, attrs: [[name, value], …], text }`, its text everything to its own close, decoded.
   *  Words between children are not a child. */
  export function childrenOf(source) {
    const lines = String(source ?? "").split("\n");
    const inner = lines.slice(1, -1).join("\n");
    const out = [];
    const re = new RegExp(`<(${NAME})(${ATTRS})>`, "g");
    let m;
    while ((m = re.exec(inner))) {
      const close = `</${m[1]}>`;
      const end = inner.indexOf(close, re.lastIndex);
      if (end < 0) break;
      out.push({ tag: m[1], attrs: attrsOf(m[2]), text: decode(inner.slice(re.lastIndex, end)) });
      re.lastIndex = end + close.length;
    }
    return out;
  }
</script>
