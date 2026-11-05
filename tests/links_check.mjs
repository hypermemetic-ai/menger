// V2-42 (TICKETS-36 on v2): every link the UI makes to a ticket comes from
// one helper, ui/src/ticket-links.svelte. A check over ui/src that refuses,
// naming the file and line, a ticket's route spelled anywhere else:
//
//   - in a svelte/1 component (and anything but the helper): `"#/ticket/"`,
//     `"#/t/"`, `"#/edit/"` or `"ticket/"` as code — a string the component
//     builds a route from — outside a comment;
//   - in an rhx/1 component: any of them but the name form, `"#/ticket/" +`
//     — rhx/1 imports no code, so an rhx twin links by name, the form the
//     helper itself gives a row without a uid, and the page it opens puts the
//     uid form in the address bar.
//
// And the helper itself, by its module script: the slug, the tail, the uid
// link, the name link for a row without one, what a route names, and the
// address a page settles on. `node tests/links_check.mjs [DIR]` prints each
// refusal and exits 1; tests/pages/links.mjs runs both as its first cases.
import fs from "node:fs";
import path from "node:path";

const HELPER = "ticket-links.svelte";
const ROUTE = /["'`](?:#\/ticket\/|#\/t\/|#\/edit\/|ticket\/)/g;
const NAME_FORM = /"#\/ticket\/"\s*\+/;

/** A file's lines that are code: no `//` comment line, nothing inside an HTML comment or a JS block comment. */
function codeLines(text) {
  const out = [];
  let inHtml = false;
  let inBlock = false;
  text.split("\n").forEach((line, i) => {
    let code = "";
    for (let j = 0; j < line.length; j++) {
      if (inHtml) { const e = line.indexOf("-->", j); if (e < 0) { j = line.length; continue; } inHtml = false; j = e + 2; continue; }
      if (inBlock) { const e = line.indexOf("*/", j); if (e < 0) { j = line.length; continue; } inBlock = false; j = e + 1; continue; }
      if (line.startsWith("<!--", j)) { inHtml = true; j += 3; continue; }
      if (line.startsWith("/*", j)) { inBlock = true; j += 1; continue; }
      if (line.startsWith("//", j) && !/:$/.test(code)) break;
      code += line[j];
    }
    if (code.trim()) out.push([i + 1, code]);
  });
  return out;
}

/** Every ticket route built outside the helper, as `file:line: text`. */
export function checkDir(dir) {
  const refused = [];
  for (const f of fs.readdirSync(dir).sort()) {
    if (f === HELPER || !(f.endsWith(".svelte") || f.endsWith(".rhx"))) continue;
    for (const [n, code] of codeLines(fs.readFileSync(path.join(dir, f), "utf8"))) {
      const hits = [...code.matchAll(ROUTE)];
      if (!hits.length) continue;
      if (f.endsWith(".rhx") && hits.every((h) => NAME_FORM.test(code.slice(h.index)))) continue;
      refused.push(`ui/src/${f}:${n}: ${code.trim().slice(0, 160)} — make it with ticket-links.svelte (ticketHref, editHref)`);
    }
  }
  return refused;
}

/** The helper's module script as an ES module. */
export async function helper(dir) {
  const src = fs.readFileSync(path.join(dir, HELPER), "utf8");
  const m = /<script module>([\s\S]*?)<\/script>/.exec(src);
  if (!m) throw new Error(`${HELPER}: no <script module>`);
  return import("data:text/javascript;charset=utf-8," + encodeURIComponent(m[1]));
}

/** The helper's cases, main's TICKETS-36 ones ported: each failure in words. */
export async function helperCases(dir) {
  const h = await helper(dir);
  const bad = [];
  const eq = (got, want, what) => { if (got !== want) bad.push(`${what}: ${JSON.stringify(got)} ≠ ${JSON.stringify(want)}`); };
  // The slug: the title folded to lower-case ASCII words joined by `-`, cut at a word to 48 at most.
  eq(h.slug("Compression on the WebSocket"), "compression-on-the-websocket", "slug");
  eq(h.slug("The app's look — themes, tokens and the dark set"), "the-apps-look-themes-tokens-and-the-dark-set", "an apostrophe joins");
  eq(h.slug("A service’s tokens leak into another's frame"), "a-services-tokens-leak-into-anothers-frame", "both apostrophes join");
  eq(h.slug("Café — crème brûlée: déjà vu, Œuvre & Straße"), "cafe-creme-brulee-deja-vu-oeuvre-strasse", "a Latin letter folds");
  eq(h.slug("日本語のテスト dark mode 🌙"), "dark-mode", "anything else parts words");
  eq(h.slug("Ünïcödé ÅØÆ ĳ"), "unicode-aoae-ij", "folds");
  eq(h.slug("Spring catalogue colours — which four swatches print true"), "spring-catalogue-colours-which-four-swatches", "cut at a word");
  eq(h.slug("Dark mode follows the system until you pick one, and then it stays"), "dark-mode-follows-the-system-until-you-pick-one", "47 of 48");
  for (const title of ["x ".repeat(60), "word ".repeat(30), "a".repeat(10) + " " + "b".repeat(40)]) {
    const s = h.slug(title);
    if (!(s.length <= h.SLUG_MAX && !s.endsWith("-") && !s.startsWith("-"))) bad.push(`slug ${JSON.stringify(title)} → ${s}`);
  }
  eq(h.slug("x ".repeat(60)).length, 47, "24 words of one letter");
  eq(h.slug("z".repeat(70)), "z".repeat(48), "one word longer than the most: cut inside it");
  eq(h.slug(""), "", "empty");
  eq(h.slug("  —— ✓ "), "", "wordless");
  eq(h.slug("v2.0 (alpha) / beta_3"), "v2-0-alpha-beta-3", "punctuation parts");
  // The link: `#/t/<uid>/<ID>-<slug>`, the id as a row shows it.
  eq(h.tail("PERF-19", "Compression on the WebSocket"), "PERF-19-compression-on-the-websocket", "tail");
  eq(h.tail("PERF.115-19", "the schema answers 3.24 MB"), "PERF-19-the-schema-answers-3-24-mb", "a namesake's key shown as its id");
  eq(h.tail("KILN-2.1", "load the shelves"), "KILN-2.1-load-the-shelves", "a subtask keeps its id");
  eq(h.tail("SYNAPSE-CC-9", ""), "SYNAPSE-CC-9", "an empty title: the id alone");
  eq(h.tail("THEME-3", "日本語"), "THEME-3", "no ASCII word: the id alone");
  eq(h.ticketHref({ uid: "7k3mq2xa", id: "PERF-19", title: "Compression on the WebSocket" }, "#/"), "#/t/7k3mq2xa/PERF-19-compression-on-the-websocket", "by uid");
  eq(h.ticketHref({ uid: "7K3MQ2XA", id: "THEME-3", title: "" }, "#/sv/"), "#/sv/t/7k3mq2xa/THEME-3", "under #/sv/, the uid lower case");
  eq(h.ticketHref({ id: "NOUID-1" }, "#/"), "#/ticket/NOUID-1", "no uid: by its name, which the page resolves");
  eq(h.ticketHref({ id: "X-1", uid: "not-a-uid" }, "#/"), "#/ticket/X-1", "a uid that is none: by its name");
  eq(h.namedHref({ uid_of: { "KILN-1": "m4c8tz1e" } }, "KILN-1", "#/"), "#/t/m4c8tz1e/KILN-1", "a named ticket by the uid its row holds");
  eq(h.editHref({ uid: "7k3mq2xa", id: "PERF-19", title: "Compression" }), "#/t/7k3mq2xa/PERF-19-compression/edit", "the editor's");
  eq(h.editHref({ id: "PERF-19" }), "#/edit/PERF-19", "the editor's, by name");
  // What a route names, and the address a page settles on.
  eq(JSON.stringify(h.named("#/t/7K3MQ2XA/PERF-19-x?chat=1")), JSON.stringify({ uid: "7k3mq2xa" }), "a uid route, case-blind, its tail ignored");
  eq(JSON.stringify(h.named("#/sv/ticket/hypermemetic:PLEXUS/PERF-19")), JSON.stringify({ name: "hypermemetic:PLEXUS/PERF-19" }), "a name route");
  eq(h.named("#/waves/PERF"), null, "not a ticket's route");
  const t = { uid: "7k3mq2xa", id: "PERF-19", title: "Compression" };
  eq(h.settled("#/ticket/PERF-19", t), "#/t/7k3mq2xa/PERF-19-compression", "an old link settles on the uid form");
  eq(h.settled("#/sv/ticket/PERF-19?full", t), "#/sv/t/7k3mq2xa/PERF-19-compression?full", "its prefix and query kept");
  eq(h.settled("#/edit/PERF-19", t), "#/t/7k3mq2xa/PERF-19-compression/edit", "the editor's old link settles on its uid form");
  eq(h.settled("#/t/7k3mq2xa/OLD-4-compression", t), "#/t/7k3mq2xa/PERF-19-compression", "a moved ticket's stale tail is put right");
  eq(h.settled("#/t/7k3mq2xa/OLD-4/edit", t), "#/t/7k3mq2xa/PERF-19-compression/edit", "in the editor too");
  eq(h.settled("#/t/7k3mq2xa/PERF-19-compression", t), null, "the address it has");
  eq(h.settled("#/t/q2r9wd4c/PERF-19", t), null, "another uid's page is left alone");
  eq(h.settled("#/ticket/PERF-19", { id: "PERF-19" }), null, "no uid: nothing to settle on");
  return bad;
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const dir = process.argv[2] ?? path.join(path.dirname(new URL(import.meta.url).pathname), "..", "ui", "src");
  const refused = checkDir(dir);
  const bad = await helperCases(dir);
  for (const r of [...refused, ...bad]) console.log(r);
  console.log(`${refused.length} link(s) built outside ${HELPER}, ${bad.length} helper case(s) failed`);
  process.exit(refused.length || bad.length ? 1 : 0);
}
