#!/usr/bin/env node
// COMPILE-4: the Node half of the `diagrams` service.
//
// The vendored mermaid — the very file the app serves — loaded into a jsdom
// window, answering `mermaid.parse` over line-delimited JSON on stdio. The
// Rust service (diagrams/src/node.rs) owns this process: it starts it on the
// first call, keeps it warm, and kills it when a diagram outlives the limit
// or the service goes away.
//
//   node mermaid-parse.js --bundle <path to mermaid-<version>.min.js>
//
// The protocol, one JSON object per line (a line ends at "\n" and nowhere
// else):
//
//   this program, once, first:  {"type":"ready","node":"v24…","jsdom":"30.1.1","load_ms":110}
//                         or:   {"type":"fatal","missing":"jsdom"|"node"|"bundle","message":"…"}
//                                (and then it exits — nothing here can serve)
//   the service, per diagram:   {"id":7,"source":"graph LR\n  A --> B"}
//   this program, per request:  {"id":7,"type":"parsed","diagram_type":"flowchart-v2"}
//                         or:   {"id":7,"type":"parse_error","message":"Parse error on line 2: …","line":2}
//                         or:   {"id":7,"type":"error","message":"…"}   (a request it could not read)
//
// Requests are answered one at a time, in the order they arrive: mermaid
// keeps each diagram type's state in module-level singletons, so two parses
// in flight at once could read each other's. The service sends one at a
// time anyway (it times each diagram from the moment it is written); the
// queue here is what makes that safe rather than lucky.
//
// stdout is the protocol and nothing else may write to it: Node's own
// console and jsdom's virtual console (where mermaid's logging lands) both go
// to stderr, which the service reads as a log.
//
// Only `mermaid.parse` is ever called — never render — under
// `securityLevel: 'strict'` (a key in mermaid's `secure` list, so no
// `%%{init}%%` in a diagram can loosen it). jsdom is created with
// `runScripts: 'outside-only'` and no resource loading: nothing a diagram
// says can run a script in the window or fetch anything.
'use strict';

const fs = require('fs');
const path = require('path');
const vm = require('vm');

// -- stdout is the protocol ---------------------------------------------------

const protocol = process.stdout;
const log = new console.Console({ stdout: process.stderr, stderr: process.stderr });
globalThis.console = log;

function send(message) {
  protocol.write(JSON.stringify(message) + '\n');
}

// A piece this program cannot serve without. Said once, on stdout where the
// service is waiting for `ready`, and then the process ends.
function fatal(missing, message) {
  send({ type: 'fatal', missing, message });
  process.exitCode = 1;
  // Let the line drain before exiting: process.exit() can cut a pipe write
  // short.
  protocol.end(() => process.exit(1));
}

// -- what this program needs ----------------------------------------------------

function bundleArg() {
  const at = process.argv.indexOf('--bundle');
  return at >= 0 ? process.argv[at + 1] : undefined;
}

// jsdom 30 (the lockfile's) needs one of these; older Node fails inside its
// require with a message about syntax, which names nothing a reader can act on.
function nodeIsRecentEnough() {
  const [major, minor, patch] = process.versions.node.split('.').map(Number);
  return (
    (major === 22 && (minor > 22 || (minor === 22 && patch >= 2))) ||
    (major === 24 && minor >= 15) ||
    major >= 26
  );
}

function start() {
  const npmCi = `npm ci --prefix ${__dirname}`;
  if (!nodeIsRecentEnough()) {
    return fatal(
      'node',
      `Node.js ${process.version} is too old for the diagrams service: jsdom 30 needs ` +
        `^22.22.2, ^24.15.0 or >=26 — install a newer Node.js (https://nodejs.org) ` +
        `or point the service at one with --node`
    );
  }

  // The jsdom the lockfile pins, from this directory's own node_modules —
  // never one that happens to sit in a parent directory.
  let JSDOM, VirtualConsole, jsdomVersion;
  const jsdomDir = path.join(__dirname, 'node_modules', 'jsdom');
  try {
    ({ JSDOM, VirtualConsole } = require(jsdomDir));
    jsdomVersion = require(path.join(jsdomDir, 'package.json')).version;
  } catch (e) {
    if (e && e.code === 'MODULE_NOT_FOUND') {
      return fatal('jsdom', `jsdom is not installed (no ${jsdomDir}) — run \`${npmCi}\``);
    }
    return fatal('jsdom', `jsdom at ${jsdomDir} failed to load (${String(e && e.message)}) — run \`${npmCi}\``);
  }

  const bundle = bundleArg();
  let src;
  try {
    src = fs.readFileSync(bundle, 'utf8');
  } catch (e) {
    return fatal(
      'bundle',
      `the mermaid bundle is not readable at ${bundle} (${e.code || e.message}) — it is ` +
        `vendored at frontend/vendor/mermaid/ in menger's checkout; pass --bundle <path>`
    );
  }

  const t0 = performance.now();
  const virtualConsole = new VirtualConsole();
  virtualConsole.forwardTo(log);
  const dom = new JSDOM('<!doctype html><html><body></body></html>', {
    runScripts: 'outside-only',
    pretendToBeVisual: true,
    virtualConsole,
  });
  // jsdom's window has no structuredClone (a browser's does), and mermaid
  // calls it while parsing some types: without it a valid `pie` or `radar`
  // is reported broken — and the failed parse leaves state behind that
  // breaks the next one. Node's own is lent to the window.
  dom.window.structuredClone = structuredClone;
  // The bundle is an IIFE that defines `globalThis.mermaid`; it has to run
  // inside the window's realm (a plain window.eval of it does not work —
  // COMPILE-3).
  try {
    new vm.Script(src, { filename: path.basename(bundle) }).runInContext(dom.getInternalVMContext());
  } catch (e) {
    return fatal('bundle', `the mermaid bundle at ${bundle} did not load: ${String(e && e.message)}`);
  }
  const mermaid = dom.window.mermaid;
  if (!mermaid || typeof mermaid.parse !== 'function') {
    return fatal('bundle', `${bundle} loaded but defined no \`mermaid.parse\` — is it mermaid's IIFE build?`);
  }
  mermaid.initialize({ startOnLoad: false, securityLevel: 'strict' });

  send({
    type: 'ready',
    node: process.version,
    jsdom: jsdomVersion,
    load_ms: Math.round(performance.now() - t0),
  });
  serve(mermaid);
}

// -- serving ----------------------------------------------------------------------

// A request is one line, and a line ends at "\n" — nowhere else. (Not
// readline: it also ends a line at U+2028 and U+2029, which JSON carries
// unescaped, so a diagram holding one would arrive as three broken
// requests and never be answered.)
function serve(mermaid) {
  let queue = Promise.resolve();
  let partial = [];
  process.stdin.setEncoding('utf8');
  process.stdin.on('data', (chunk) => {
    let start = 0;
    let nl;
    while ((nl = chunk.indexOf('\n', start)) >= 0) {
      partial.push(chunk.slice(start, nl));
      const line = partial.join('');
      partial = [];
      start = nl + 1;
      if (line.trim() !== '') queue = queue.then(() => answer(mermaid, line));
    }
    if (start < chunk.length) partial.push(chunk.slice(start));
  });
  // The service closed our stdin — it has finished with us, or it is gone.
  // Either way nothing more will be asked: finish what is queued and leave,
  // so a service killed outright never strands a Node process.
  process.stdin.on('end', () => {
    queue.then(() => process.exit(0));
  });
}

async function answer(mermaid, line) {
  let request;
  try {
    request = JSON.parse(line);
  } catch (e) {
    return send({ id: null, type: 'error', message: `not a JSON request: ${String(e.message)}` });
  }
  const id = request && Number.isSafeInteger(request.id) ? request.id : null;
  if (id === null || typeof request.source !== 'string') {
    return send({ id, type: 'error', message: 'a request is {"id": <integer>, "source": <string>}' });
  }
  try {
    const result = await mermaid.parse(request.source);
    if (result && typeof result.diagramType === 'string') {
      return send({ id, type: 'parsed', diagram_type: result.diagramType });
    }
    return send({ id, type: 'parse_error', message: 'mermaid.parse returned no diagram', line: null });
  } catch (e) {
    const message = String((e && e.message) || e);
    let line = null;
    try {
      line = sourceLine(mermaid, request.source, mermaidLine(e, message));
    } catch (mapping) {
      log.error('could not map the error line back to the source:', mapping);
    }
    return send({ id, type: 'parse_error', message, line });
  }
}

// -- where an error is, in the caller's source --------------------------------------

// The line mermaid names, 1-based, in the text it actually parsed: jison's
// "Parse error on line 2:", "Lexical error on line 2.", and the langium
// parsers' "Lexer error on line 2, column 10" / "Parse error on line 3,
// column 2" all say "on line N" — the number the browser shows the reader.
function mermaidLine(e, message) {
  const said = /\bon line (\d+)/.exec(message);
  if (said) return Number(said[1]);
  const loc = e && e.hash && e.hash.loc;
  if (loc && Number.isSafeInteger(loc.first_line)) return loc.first_line;
  return null;
}

// mermaid numbers the lines of the text it parsed, which is not the text it
// was given: before a parser sees a diagram, mermaid drops YAML frontmatter,
// `%%{init}%%` directives, `%%` comment lines and leading blank lines, and
// the flowchart parser also drops blank lines after a `}`. A source that
// starts with a `title:` frontmatter has its error reported three lines
// early. So the same steps are replayed here on a string whose every
// character remembers its source line, and mermaid's line is looked up in
// it. The regular expressions are mermaid 11.17.2's (`preprocessDiagram`:
// cleanupText, extractFrontMatter, removeDirectives, cleanupComments; and
// flowchart's `flowParser.parse`), copied from the vendored bundle: a bundle
// upgrade re-reads them (the README's upgrade steps say so), and if one
// drifts only `line` can be wrong — the verdict and the message are
// mermaid's own.
const CLEANUP_LINE_ENDINGS = /\r\n?/g;
const CLEANUP_TAG_QUOTES = /<(\w+)([^>]*)>/g;
const FRONTMATTER = /^([^\S\n\r]*)-{3}\s*[\n\r](.*?)[\n\r]\1-{3}\s*[\n\r]+/s;
const DIRECTIVE = /%{2}{\s*(?:(\w+)\s*:|(\w+))\s*(?:(\w+)|((?:(?!}%{2}).|\r?\n)*))?\s*(?:}%{2})?/gi;
const COMMENT_LINE = /^\s*%%(?!{)[^\n]+\n?/gm;
const LEADING_SPACE = /^\s+/g;
const FLOWCHART_BRACE_NEWLINES = /}\s*\n/g;

function sourceLine(mermaid, source, line) {
  if (line === null || line < 1) return null;
  let t = traced(source);
  t = replaceTraced(t, CLEANUP_LINE_ENDINGS, () => '\n');
  t = replaceTraced(t, CLEANUP_TAG_QUOTES, (m) =>
    '<' + m[1] + m[2].replace(/="([^"]*)"/g, "='$1'") + '>'
  );
  const front = FRONTMATTER.exec(t.text);
  if (front) t = { text: t.text.slice(front[0].length), lines: t.lines.slice(front[0].length) };
  t = replaceTraced(t, DIRECTIVE, () => '');
  t = replaceTraced(t, COMMENT_LINE, () => '');
  t = replaceTraced(t, LEADING_SPACE, () => '');
  let type = null;
  try {
    type = mermaid.detectType(source);
  } catch (_) {
    // No type: nothing type-specific was done to the text either.
  }
  if (type === 'flowchart' || type === 'flowchart-v2' || type === 'flowchart-elk') {
    t = replaceTraced(t, FLOWCHART_BRACE_NEWLINES, () => '}\n');
  }

  // The first character of mermaid's line `line`, and the source line it
  // came from. An empty last line has no character: it is the line after
  // the last newline.
  let at = 0;
  for (let n = 1; n < line; n++) {
    const nl = t.text.indexOf('\n', at);
    if (nl < 0) return null; // mermaid named a line its own text does not have
    at = nl + 1;
  }
  if (at < t.text.length) return t.lines[at];
  if (at > 0) return t.lines[at - 1] + 1;
  return null;
}

// The source, each character paired with its 1-based line. A line ends at
// "\n", "\r\n" or a lone "\r" — the endings mermaid's cleanupText folds into
// "\n".
function traced(source) {
  const lines = new Array(source.length);
  let line = 1;
  for (let i = 0; i < source.length; i++) {
    lines[i] = line;
    const c = source[i];
    if (c === '\n' || (c === '\r' && source[i + 1] !== '\n')) line++;
  }
  return { text: source, lines };
}

// `text.replace(re, f)`, keeping every surviving character's line. A
// replacement as long as its match keeps the match's lines character for
// character (the tag-quote rewrite); any other takes the line of the
// match's first character.
function replaceTraced(t, re, replacement) {
  let text = '';
  const lines = [];
  let last = 0;
  for (const m of t.text.matchAll(re)) {
    text += t.text.slice(last, m.index);
    for (let i = last; i < m.index; i++) lines.push(t.lines[i]);
    const r = replacement(m);
    text += r;
    for (let i = 0; i < r.length; i++) {
      lines.push(r.length === m[0].length ? t.lines[m.index + i] : t.lines[m.index]);
    }
    last = m.index + m[0].length;
  }
  text += t.text.slice(last);
  for (let i = last; i < t.text.length; i++) lines.push(t.lines[i]);
  return { text, lines };
}

start();
