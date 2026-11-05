// EDITOR-13: the engine is the only place a shortcut is bound. A check over
// ui/src that refuses, naming the file and line:
//
//   - a keydown handler of a component's own — svelte `onkeydown={…}`, rhx
//     `on:keydown={…}` / `on:window-keydown={…}`, `addEventListener("keydown", …)`,
//     CodeMirror's `domEventHandlers({ keydown … })` — that reads a modifier
//     (`ctrlKey`, `metaKey`, `altKey`; rhx `ev.ctrl`, `ev.meta`, `ev.alt`), in
//     itself or in a function of the file it calls: a modified key is a chord,
//     and a chord is declared to the engine (`keys()` / `bus.keys()`);
//   - a CodeMirror keymap entry with a modifier (`key: "Mod-s"`, `Ctrl-`,
//     `Cmd-`, `Meta-`, `Alt-`): the editor's commands are declared through
//     `bus.keys` and dispatched into CodeMirror from there;
//   - a chord the browser never yields (ctrl+t, ctrl+w, ctrl+n, ctrl+tab, their
//     shift and meta twins) in a `keys(…)` / `.keys(…)` literal.
//
// Plain keys stay a field's own: Enter in an input, Escape, the arrows in a
// list. `node tests/keys_check.mjs [DIR]` prints each refusal and exits 1;
// tests/pages/keys.mjs runs it as its first case.
import fs from "node:fs";
import path from "node:path";

const MOD_READ = /\b(?:ctrlKey|metaKey|altKey)\b|\bev\.(?:ctrl|meta|alt)\b/;
const CM_MOD = /\bkey\s*:\s*["'`](?:Mod|Ctrl|Cmd|Meta|Alt)-/;
const RESERVED = /["'`]((?:ctrl|meta)\+(?:shift\+)?(?:t|w|n|tab))["'`]\s*:/;

/** The text from `open` (an index of `(` `{` or `[`) to its balanced close, strings and comments skipped. */
function balanced(src, open) {
  const pairs = { "(": ")", "{": "}", "[": "]" };
  const stack = [];
  for (let i = open; i < src.length; i++) {
    const c = src[i];
    if (c === '"' || c === "'" || c === "`") {
      for (i++; i < src.length && src[i] !== c; i++) if (src[i] === "\\") i++;
      continue;
    }
    if (c === "/" && src[i + 1] === "/") { while (i < src.length && src[i] !== "\n") i++; continue; }
    if (c === "/" && src[i + 1] === "*") { i = src.indexOf("*/", i + 2); if (i < 0) return src.slice(open); i++; continue; }
    if (pairs[c]) stack.push(pairs[c]);
    else if (c === ")" || c === "}" || c === "]") {
      stack.pop();
      if (!stack.length) return src.slice(open, i + 1);
    }
  }
  return src.slice(open);
}

/** A function of the file by name: `function f(…) {…}`, `fn f(…) {…}` (rhx), `const f = (…) => …` (to its line's end or its block). */
function bodyOf(src, name) {
  const decl = new RegExp(`(?:function\\s+${name}\\s*\\(|\\bfn\\s+${name}\\s*\\(|(?:const|let)\\s+${name}\\s*=)`);
  const m = decl.exec(src);
  if (!m) return null;
  const from = m.index + m[0].length;
  const brace = src.indexOf("{", from);
  const eol = src.indexOf("\n", from);
  if (brace >= 0 && (eol < 0 || brace < eol || /^\s*\([^)]*\)\s*$/.test(src.slice(from, brace)))) return src.slice(m.index, brace) + balanced(src, brace);
  return src.slice(m.index, eol < 0 ? undefined : eol);
}

/** Every name the handler's text calls or names, then what those read — two levels, the file's own functions. */
function reads(src, text, depth = 2, seen = new Set()) {
  if (MOD_READ.test(text)) return true;
  if (!depth) return false;
  for (const [, name] of text.matchAll(/\b([A-Za-z_$][\w$]*)\b/g)) {
    if (seen.has(name)) continue;
    seen.add(name);
    const body = bodyOf(src, name);
    if (body && reads(src, body, depth - 1, seen)) return true;
  }
  return false;
}

const lineOf = (src, at) => src.slice(0, at).split("\n").length;

/** Every refusal in one file's source: `{ line, message }`. */
export function checkSource(src) {
  const out = [];
  const said = (at) => out.push({ line: lineOf(src, at), message: "a keydown handler reads a modifier — a chord is declared to the engine (keys() / bus.keys()), never heard by a component's own handler" });
  // onkeydown={…}, on:keydown={…}, on:window-keydown={…}: the attribute's braces.
  for (const m of src.matchAll(/\bon(?::window-|:)?keydown\s*=\s*\{/g)) if (reads(src, balanced(src, m.index + m[0].length - 1))) said(m.index);
  // addEventListener("keydown", …): the call's parentheses.
  for (const m of src.matchAll(/addEventListener\(\s*["']keydown["']/g)) if (reads(src, balanced(src, m.index + "addEventListener".length))) said(m.index);
  // domEventHandlers({ keydown: … }): the value, an arrow's block or a name.
  for (const m of src.matchAll(/\bkeydown\s*:\s*/g)) {
    const at = m.index + m[0].length;
    const arrow = /^(?:\([^)]*\)|[\w$]+)\s*=>\s*/.exec(src.slice(at));
    const from = at + (arrow ? arrow[0].length : 0);
    const text = src[from] === "{" || src[from] === "(" ? balanced(src, from) : src.slice(from).split(/[,}\n]/)[0];
    if (reads(src, text)) said(m.index);
  }
  for (const m of src.matchAll(new RegExp(CM_MOD.source, "g"))) {
    out.push({ line: lineOf(src, m.index), message: `a CodeMirror keymap binds a modified key (${src.slice(m.index, m.index + 20).split(",")[0]}) — declare it through bus.keys and dispatch it into the editor` });
  }
  for (const m of src.matchAll(new RegExp(RESERVED.source, "g"))) {
    out.push({ line: lineOf(src, m.index), message: `\`${m[1]}\` is the browser's — no page ever hears it; bind another chord` });
  }
  return out;
}

/** Every refusal under `dir` (ui/src): `file:line: message`. */
export function checkDir(dir) {
  const out = [];
  for (const f of fs.readdirSync(dir).sort()) {
    if (!/\.(svelte|rhx|js|mjs)$/.test(f)) continue;
    const src = fs.readFileSync(path.join(dir, f), "utf8");
    for (const r of checkSource(src)) out.push(`${path.join(path.basename(dir) === "src" ? "ui/src" : dir, f)}:${r.line}: ${r.message}`);
  }
  return out;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const dir = process.argv[2] ?? path.join(path.dirname(new URL(import.meta.url).pathname), "..", "ui", "src");
  const refused = checkDir(dir);
  for (const r of refused) console.log(r);
  console.log(refused.length ? `${refused.length} refused` : "keys: every chord is the engine's");
  process.exit(refused.length ? 1 : 0);
}
