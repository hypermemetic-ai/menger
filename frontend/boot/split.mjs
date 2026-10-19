// APP-92: the first paint — trunk's post_build hook (Trunk.toml), run in
// the staging directory's build before it becomes dist/.
//
// 1. index.html keeps only what a load paints first — the tokens and the
//    boot sheet's rules, everything above `/* APP-92: split */` in its one
//    <style>. The rest of the stylesheet goes to `app-<hash>.css`, named in
//    `<meta name="menger-app-css">` (the boot script loads it) and preloaded.
//    A few KB to the first paint where it was 419 KB.
// 2. sw.js, the service worker (boot/sw.js), learns the build: its files to
//    keep (the app's script and wasm, the stylesheet, the faces) and a
//    version, so a new build replaces the last one's files.
//
// The shell serves both (src/shell.rs: sw.js at the root, the stylesheet
// under /app/, public and immutable by its hash).
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

const dir = process.env.TRUNK_STAGING_DIR;
if (!dir) throw new Error("split.mjs runs as trunk's post_build hook (TRUNK_STAGING_DIR)");
const base = (process.env.TRUNK_PUBLIC_URL || "/app/").replace(/\/?$/, "/");
const MARK = "/* APP-92: split */";

const file = path.join(dir, "index.html");
let html = fs.readFileSync(file, "utf8");
const at = html.indexOf(MARK);
const end = html.indexOf("</style>", at);
if (at < 0 || end < 0) throw new Error(`index.html has no \`${MARK}\` inside its <style>`);
const css = html.slice(at + MARK.length, end);
const hash = (s) => crypto.createHash("sha256").update(s).digest("hex").slice(0, 16);
const name = `app-${hash(css)}.css`;
fs.writeFileSync(path.join(dir, name), css);
const href = `${base}${name}`;
html = html.slice(0, at) + html.slice(end);
const charset = '<meta charset="utf-8">';
if (!html.includes(charset)) throw new Error("index.html has no <meta charset>");
html = html.replace(charset, `${charset}\n<meta name="menger-app-css" content="${href}">\n<link rel="preload" as="style" href="${href}">`);
fs.writeFileSync(file, html);

const code = fs.readdirSync(dir).filter((f) => /^menger-app-.*\.(js|wasm)$/.test(f));
if (!code.some((f) => f.endsWith(".wasm"))) throw new Error("no wasm in the build");
const fonts = fs.existsSync(path.join(dir, "fonts")) ? fs.readdirSync(path.join(dir, "fonts")).filter((f) => f.endsWith(".woff2")).map((f) => `fonts/${f}`) : [];
const keep = [...code, name, ...fonts].sort().map((f) => `${base}${f}`);
const version = `${new Date().toISOString().replace(/\D/g, "").slice(0, 14)}-${hash(keep.join("\n"))}`;
const sw = fs
  .readFileSync(new URL("./sw.js", import.meta.url), "utf8")
  .replace('"__VERSION__"', JSON.stringify(version))
  .replace('["__KEEP__"]', JSON.stringify(keep));
fs.writeFileSync(path.join(dir, "sw.js"), sw);
console.log(`APP-92: index.html ${Buffer.byteLength(html)} B, ${name} ${Buffer.byteLength(css)} B; sw.js ${version}, ${keep.length} files`);
