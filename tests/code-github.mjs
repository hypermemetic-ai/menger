// CODE-3: a stand-in for GitHub on loopback, for the code service of a throwaway (its hidden
// `--code-github-at IP:PORT`; tests/pages_browser.mjs starts it, tests/pages/code-ide.mjs drives it) —
// what tests/wire_code.rs's `stand_in()` serves, in Node, over synthetic repositories:
//
//   GET /<owner>/<name>.git/info/refs?service=git-upload-pack   git's protocol-v0 listing (pkt-lines: the
//                                                               HEAD line with `symref=HEAD:refs/heads/<default>`,
//                                                               each branch, each tag and an annotated tag's `^{}`)
//   GET /<owner>/<name>/tar.gz/<full or short hash>             a gzip tar: a pax global header `comment=<hash>`,
//                                                               then every entry under `<name>-<short>/`
//
// A repository it does not hold answers the listing 401 (GitHub's answer for a missing or a private one)
// and an archive 404. `move(repo, branch, label)` moves a branch to another of the repository's commits;
// `asked` is every request's path, in order. Nothing here reaches the internet.
//
// Run alone, it stays up for a server started by hand: `node tests/code-github.mjs [PORT]` prints its
// address, for `menger … --code-github-at <it>`; Ctrl-C stops it.
import crypto from "node:crypto";
import http from "node:http";
import zlib from "node:zlib";

/** A commit's hash, from its label: stable from run to run. */
export const hashOf = (label) => crypto.createHash("sha1").update(`code-ide stand-in ${label}`).digest("hex");

function pkt(s) {
  return (Buffer.byteLength(s) + 4).toString(16).padStart(4, "0") + s;
}

/** git's listing: the HEAD line first, then the branches and the tags. */
function listing(repo) {
  const head = repo.branches[repo.head];
  let t = pkt("# service=git-upload-pack\n") + "0000";
  t += pkt(`${head} HEAD\0multi_ack thin-pack side-band symref=HEAD:refs/heads/${repo.head} agent=git/stand-in\n`);
  for (const [name, c] of Object.entries(repo.branches)) t += pkt(`${c} refs/heads/${name}\n`);
  for (const [name, c] of Object.entries(repo.tags)) {
    const object = hashOf(`tag object ${name}`); // an annotated tag: its own object, then the commit it points at
    t += pkt(`${object} refs/tags/${name}\n`);
    t += pkt(`${c} refs/tags/${name}^{}\n`);
  }
  return Buffer.from(t + "0000");
}

// ── a tar, by hand (ustar), and gzipped ─────────────────────────────────
function octal(n, width) {
  return n.toString(8).padStart(width - 1, "0") + "\0";
}
function header(name, size, type, mtime) {
  const h = Buffer.alloc(512);
  let prefix = "";
  if (Buffer.byteLength(name) > 100) {
    const cut = name.lastIndexOf("/", name.length - 2);
    prefix = name.slice(0, cut);
    name = name.slice(cut + 1);
  }
  h.write(name, 0, 100, "utf8");
  h.write(octal(type === "5" ? 0o755 : 0o644, 8), 100, "ascii");
  h.write(octal(0, 8), 108, "ascii");
  h.write(octal(0, 8), 116, "ascii");
  h.write(octal(size, 12), 124, "ascii");
  h.write(octal(mtime, 12), 136, "ascii");
  h.write("        ", 148, "ascii");
  h.write(type, 156, "ascii");
  h.write("ustar\0", 257, "ascii");
  h.write("00", 263, "ascii");
  h.write(prefix, 345, 155, "utf8");
  let sum = 0;
  for (const b of h) sum += b;
  h.write(sum.toString(8).padStart(6, "0") + "\0 ", 148, "ascii");
  return h;
}
const pad = (n) => Buffer.alloc((512 - (n % 512)) % 512);

/** `files` (path → text or bytes) as GitHub's archive of `commit`: gzip, a pax header naming it, one top directory. */
export function archive(repoName, commit, files, mtime = 1_790_000_000) {
  const parts = [];
  const body = ` comment=${commit}\n`;
  let len = body.length + 2;
  while (String(len).length + body.length !== len) len++;
  const pax = Buffer.from(`${len}${body}`);
  parts.push(header("pax_global_header", pax.length, "g", mtime), pax, pad(pax.length));
  const top = `${repoName.split("/")[1]}-${commit.slice(0, 7)}/`;
  parts.push(header(top, 0, "5", mtime));
  const dirs = new Set();
  for (const path of Object.keys(files).sort()) {
    const segs = path.split("/");
    for (let i = 1; i < segs.length; i++) {
      const d = segs.slice(0, i).join("/");
      if (!dirs.has(d)) { dirs.add(d); parts.push(header(`${top}${d}/`, 0, "5", mtime)); }
    }
    const bytes = Buffer.isBuffer(files[path]) ? files[path] : Buffer.from(files[path]);
    parts.push(header(`${top}${path}`, bytes.length, "0", mtime), bytes, pad(bytes.length));
  }
  parts.push(Buffer.alloc(1024));
  return zlib.gzipSync(Buffer.concat(parts));
}

// ── the repositories ────────────────────────────────────────────────────
const CONES = `//! The cones a firing may name.

pub const CONES: [&str; 5] = ["06", "04", "5", "6", "10"];

pub fn known(cone: &str) -> Result<(), Refusal> {
    if CONES.contains(&cone) {
        return Ok(());
    }
    Err(Refusal::unknown_cone(cone, &CONES))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cone_nobody_fires() {
        assert!(known("7").is_err());
    }
}
`;
/** A file of 20,000 lines (Rust, so it is painted too): the first says what it is. */
export const BIG_LINES = 20_000;
function big() {
  const out = ["// the big file: 20000 lines, each a constant"];
  for (let n = 2; n <= BIG_LINES; n++) out.push(`pub const C${n}: u32 = ${n}; // line ${n} of the big file`);
  return out.join("\n") + "\n";
}
/** A PNG's first bytes: a NUL in the first 8 KiB, so the service answers `binary`. */
const PNG = Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13]), Buffer.alloc(1200, 7)]);

function kiln() {
  const base = {
    "src/cones.rs": CONES,
    "src/log.rs": "use crate::{cones, store, Firing, Refusal};\n\npub fn log(f: Firing) -> Result<(), Refusal> {\n    cones::known(&f.cone)?;\n    store::append(f)\n}\n",
    "src/store.rs": "use crate::Firing;\n\npub fn append(f: Firing) -> Result<(), crate::Refusal> {\n    // one row a firing\n    Ok(())\n}\n",
    "src/lib.rs": "pub mod cones;\npub mod log;\npub mod store;\n",
    "src/big.rs": big(),
    "ui/src/firing-log.svelte": "<script module>\n  export const uses = [\"kiln.log\", \"kiln.watch\"];\n</script>\n\n<p class=\"log\">the firings</p>\n",
    "ui/app.js": "// the page's entry\nexport function start(at) {\n  const n = 42;\n  return `kiln at ${at}: ${n}`;\n}\n",
    "ui/routes.json": "{\n  \"#/\": \"firing-log\"\n}\n",
    "tools/fire.py": "def fire(cone: str) -> bool:\n    \"\"\"Fire to a cone.\"\"\"\n    return cone in (\"06\", \"5\", \"6\")\n",
    "assets/logo.png": PNG,
    "Cargo.toml": "[package]\nname = \"kiln\"\nversion = \"1.3.0\"\n",
    "README.md": "# kiln\n\nA firing log. Cones 06 to 10.\n",
  };
  const moved = { ...base, "README.md": "# kiln\n\nA firing log. Cones 06 to 10, and a glaze book.\n" };
  delete moved["src/log.rs"];
  const commits = {
    [hashOf("kiln main")]: base,
    [hashOf("kiln main moved")]: moved, // main's next tip: src/log.rs gone
    [hashOf("kiln glazes")]: { ...base, "src/glaze.rs": "pub fn celadon() -> Glaze {\n    Glaze::new(\"celadon\", 6)\n}\n" },
    [hashOf("kiln v1.2.0")]: { "src/cones.rs": CONES, "src/lib.rs": "pub mod cones;\n", "README.md": "# kiln\n" },
  };
  return {
    head: "main",
    branches: { main: hashOf("kiln main"), glazes: hashOf("kiln glazes"), "feature/slash-name": hashOf("kiln glazes") },
    tags: { "v1.2.0": hashOf("kiln v1.2.0") },
    commits,
  };
}

function menger() {
  const shell = ["//! The shell: the app's pages and the sign-in, behind one policy."];
  for (let n = 2; n <= 240; n++) shell.push(n >= 183 && n <= 187 ? `pub const PAGE_${n}: &str = "a line of the policy, ${n}";` : `// line ${n} of src/shell.rs`);
  const files = {
    "src/shell.rs": shell.join("\n") + "\n",
    "src/main.rs": "fn main() {\n    menger::run();\n}\n",
    "ui/src/board-frame.svelte": "<script module>\n  export const uses = [];\n</script>\n\n<div data-slot=\"main\"></div>\n",
    "ui/src/code-ide.svelte": "<script module>\n  export const uses = [\"code.open\"];\n</script>\n",
    "ui/routes.json": "{\n  \"#/board\": \"board-frame\"\n}\n",
    "README.md": "# menger\n\nA plan is a program whose leaves are prose.\n",
  };
  return { head: "main", branches: { main: hashOf("menger main"), v2: hashOf("menger v2") }, tags: {}, commits: { [hashOf("menger main")]: files, [hashOf("menger v2")]: files } };
}

/** The stand-in, listening on 127.0.0.1 (`port`, else one the OS gives): `{ port, at, asked, repos, move, hashOf, close }`. */
export async function githubStandIn(port = 0) {
  const repos = { "hypermemetic-ai/kiln": kiln(), "hypermemetic-ai/menger": menger() };
  const archives = new Map();
  const asked = [];
  const srv = http.createServer((req, res) => {
    asked.push(req.url);
    const send = (status, type, body) => { res.writeHead(status, { "content-type": type, "content-length": body.length }); res.end(body); };
    const url = new URL(req.url, "http://stand-in");
    const refs = /^\/([^/]+\/[^/]+)\.git\/info\/refs$/.exec(url.pathname);
    if (refs && url.searchParams.get("service") === "git-upload-pack") {
      const repo = repos[refs[1].toLowerCase()];
      if (!repo) { res.writeHead(401, { "www-authenticate": 'Basic realm="GitHub"', "content-length": 0 }); return res.end(); }
      return send(200, "application/x-git-upload-pack-advertisement", listing(repo));
    }
    const tar = /^\/([^/]+\/[^/]+)\/tar\.gz\/([0-9a-f]{7,40})$/.exec(url.pathname);
    if (tar) {
      const name = tar[1].toLowerCase(), repo = repos[name];
      const full = repo && Object.keys(repo.commits).find((c) => c.startsWith(tar[2]));
      if (!full) return send(404, "text/plain", Buffer.from("404: Not Found"));
      const key = `${name}@${full}`;
      if (!archives.has(key)) archives.set(key, archive(name, full, repo.commits[full]));
      return send(200, "application/x-gzip", archives.get(key));
    }
    send(404, "text/plain", Buffer.from("404: Not Found"));
  });
  await new Promise((r) => srv.listen(port, "127.0.0.1", r));
  srv.unref();
  return {
    port: srv.address().port,
    at: `127.0.0.1:${srv.address().port}`,
    asked,
    repos,
    hashOf,
    /** `branch` of `repo` moved to the commit labelled `label`. */
    move(repo, branch, label) { repos[repo].branches[branch] = hashOf(label); },
    close: () => new Promise((r) => srv.close(r)),
  };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const g = await githubStandIn(Number(process.argv[2] ?? 0));
  console.log(`GitHub stand-in at ${g.at}: hypermemetic-ai/kiln, hypermemetic-ai/menger (hypermemetic-ai/secret: not public)`);
  setInterval(() => {}, 1 << 30);
}
