// UI-8: the harness the pages' browser test (tests/pages_browser.mjs) and
// the pages' probe (perf/pages-probe.mjs) share — the setup UI-6's engine
// browser test had (that test lives with the engine now, ../plexus-ui's
// `npm test`, FRAME-7), lifted out so both run it the same way:
//
//   build(profile)                 menger from $CARGO_TARGET_DIR (else ./target), and the plexus-ui tool
//                                  (scripts/plexus-ui, ../plexus-ui's cli/, FRAME-8)
//   bundle(work, name, edit?)      plexus-ui build → <work>/<name>.tar; `edit(dir)` changes a copy of ui/ first
//   new Throwaway(work).start(…)   a menger on OS-assigned ports, its files under the work directory,
//                                  `--auth-provider key`, an admin key and a ui signer, `--seed-synthetic N`
//   synapse(server, session, …)    synapse against it, `-j`'s data contents (the error item last)
//   push(server, session, signer, tar)   signed with plexus-ui sign, pushed through `ui push`
//   playwright()                   Chromium from perf/node_modules (this checkout's, else the main one's)
//
// Never removes anything: the caller removes the work directory. Below
// /tmp/claude-UID/ the caller removes it by `scripts/cleanup-gate.sh --path
// DIR`; in a scratch root — a $TMPDIR of ~/.cache/menger-perf-scratch, or
// one MENGER_SCRATCH_DIRS names — scripts/cleanup-gate.sh takes a ui8-*
// directory nothing holds once idle an hour, at the next hourly sweep (RUN-34).
// tests/pages_browser.mjs removes its own ui8-pages-… when its run ends
// (V2-38); one a killed run left below /tmp/claude-UID/ the sweep takes too.
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";

export const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const TARGET = process.env.CARGO_TARGET_DIR || path.join(REPO, "target");

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function playwright() {
  for (const dir of [path.join(REPO, "perf"), path.join(REPO, "..", "menger", "perf")]) {
    try {
      const at = createRequire(path.join(dir, "package.json")).resolve("playwright");
      return await import(pathToFileURL(at.replace(/index\.js$/, "index.mjs")).href);
    } catch {
      /* the next */
    }
  }
  throw new Error("no Playwright: `npm ci` in perf/ (or the main checkout's perf/)");
}

export function freePort() {
  return new Promise((resolve, reject) => {
    const s = net.createServer();
    s.unref();
    s.on("error", reject);
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

function tcpUp(port) {
  return new Promise((resolve) => {
    const c = net.connect(port, "127.0.0.1");
    c.on("connect", () => (c.destroy(), resolve(true)));
    c.on("error", () => resolve(false));
  });
}

export function run(cmd, argv, opts = {}) {
  const r = spawnSync(cmd, argv, { encoding: "utf8", maxBuffer: 256 * 1024 * 1024, ...opts });
  if (r.status !== 0) throw new Error(`${cmd} ${argv.join(" ")} failed (${r.status}):\n${r.stderr || r.stdout}`);
  return r.stdout;
}

/** A work directory `prefix-…` (callers name it `ui8-…`) under $TMPDIR, else
 *  /tmp/claude-1000/. The caller removes it: below /tmp/claude-UID/ by
 *  `scripts/cleanup-gate.sh --path DIR` (shape `session`; the sweep never
 *  takes those). In a scratch root (~/.cache/menger-perf-scratch,
 *  MENGER_SCRATCH_DIRS) it is shape `ui8-work` once idle an hour — taken by
 *  `--path DIR`, or at the next hourly sweep. The marks it keys on are the
 *  server's tickets.db, ui.db and admin.key. */
export function workDir(prefix) {
  // FRAME-14: $TMPDIR when it is set (a seeded million wants more than the
  // tmpfs holds), else /tmp/claude-1000 as before.
  const base = process.env.TMPDIR || "/tmp/claude-1000";
  fs.mkdirSync(base, { recursive: true });
  return fs.mkdtempSync(path.join(base, `${prefix}-`));
}

/** The server, built for `profile`, and the plexus-ui tool. */
export function build(profile) {
  const p = profile === "release" ? ["--release"] : [];
  const quiet = { cwd: REPO, stdio: ["ignore", "ignore", "inherit"] };
  run("cargo", ["build", ...p, "-p", "menger", "--bin", "menger"], quiet);
  run(TOOL, ["--help"], quiet);
}

/** FRAME-14: where the tool finds svelte and esbuild for a svelte/1 build — the framework checkout's
 *  (V2-47: scripts/plexus-ui-src --node-modules, the tag of the plexus-ui version menger takes, `npm ci`
 *  there once); V2-29: `PLEXUS_UI_NODE_MODULES` names another (a plexus-ui worktree's, before its
 *  dependencies merge). Asked the first time a bundle is built. */
let nodeModules = process.env.PLEXUS_UI_NODE_MODULES || "";
export function NODE_MODULES() {
  if (!nodeModules) nodeModules = path.join(run(path.join(REPO, "scripts", "plexus-ui-src"), ["--node-modules", REPO]).trim(), "node_modules");
  return nodeModules;
}

/** The plexus-ui tool (FRAME-8): the framework's cli/, built into this checkout's target by scripts/plexus-ui. */
export const TOOL = path.join(REPO, "scripts", "plexus-ui");

/** ui/ (or a copy `edit` changed) built into `<work>/<name>.tar` by
 *  `plexus-ui build`; the engine taken from `engine` (a directory holding
 *  engine.js and engine.wasm) when given, else built once into
 *  `<work>/engine` (`build --prebuild`). Its hash, and its manifest's text. */
export function bundle(work, name, { edit, engine } = {}) {
  const tar = path.join(work, `${name}.tar`);
  let ui = path.join(REPO, "ui");
  if (edit) {
    ui = path.join(work, `${name}-ui`);
    fs.cpSync(path.join(REPO, "ui"), ui, { recursive: true });
    edit(ui);
  }
  const quiet = { cwd: REPO, stdio: ["ignore", "ignore", "inherit"] };
  if (!engine) {
    engine = path.join(work, "engine");
    if (!fs.existsSync(path.join(engine, "engine.wasm"))) run(TOOL, ["build", "--prebuild", "--engine", engine], quiet);
  }
  // FRAME-14: ui/src holds svelte/1 components, compiled in Node with
  // the framework checkout's svelte and esbuild (`npm ci` there); a copy `edit` made
  // under the work directory finds no node_modules above it.
  run(TOOL, ["build", ui, "--engine", engine, "--node-modules", NODE_MODULES(), "-o", tar], quiet);
  const manifest = run("tar", ["-xOf", tar, "manifest.json"]);
  return { tar, engine, manifest, version: run("sha256sum", [tar]).split(" ")[0] };
}

export class Throwaway {
  constructor(work, profile) {
    this.work = work;
    this.profile = profile;
    this.stderr = "";
  }

  async start(adminPrincipal, signerHex, seed) {
    this.ws = await freePort();
    this.shell = await freePort();
    const env = { ...process.env, PLEXUS_NO_REGISTRY: "1" };
    for (const v of ["MENGER_DB_PATH", "MENGER_AUTH_PROVIDERS", "MENGER_ADMINS", "MENGER_UI_DB_PATH", "MENGER_UI_SIGNERS", "MENGER_MEDIA_DB_PATH", "MENGER_DOCS_DB_PATH", "MENGER_APP_DIST", "RUST_LOG"]) delete env[v];
    const argv = [
      "--port", String(this.ws),
      "--shell-port", String(this.shell),
      "--auth-provider", "key",
      "--admin", adminPrincipal,
      "--db-path", path.join(this.work, "tickets.db"),
      "--data-dir", this.work,
      "--ui-db-path", path.join(this.work, "ui.db"),
      "--ui-signer", signerHex,
      "--seed-synthetic", String(seed),
      "--seed-value", "6",
      // EDITOR-10: the ip:port pairs a test's local server stands in for the internet at.
      ...(this.fetchAllow ?? []).flatMap((a) => ["--media-fetch-allow", a]),
    ];
    this.child = spawn(path.join(TARGET, this.profile, "menger"), argv, { env, stdio: ["ignore", "ignore", "pipe"] });
    this.child.stderr.on("data", (d) => (this.stderr += d.toString()));
    // FRAME-14: whatever ends the caller, the server goes with it (unless kept).
    process.once("exit", () => {
      if (!this.keep) this.stop();
    });
    const deadline = Date.now() + 1_800_000;
    while (Date.now() < deadline) {
      if (this.child.exitCode !== null) throw new Error(`menger exited at start:\n${this.stderr}`);
      if ((await tcpUp(this.ws)) && (await tcpUp(this.shell))) return;
      await sleep(100);
    }
    throw new Error("menger never answered");
  }

  get origin() {
    return `http://127.0.0.1:${this.shell}`;
  }

  stop() {
    this.child?.kill("SIGKILL");
  }
}

/** synapse against the throwaway, signed in by the session file: `-j`'s
 *  data items' contents, the error item last when there is one. */
export function synapse(server, sessionFile, ...argv) {
  const env = { ...process.env, XDG_CACHE_HOME: path.join(server.work, "cache") };
  const r = spawnSync("synapse", ["--no-hold", "--token-file", sessionFile, "-j", "-P", String(server.ws), "menger", ...argv], { env, encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
  const items = (r.stdout || "").split("\n").filter(Boolean).flatMap((l) => {
    try {
      return [JSON.parse(l)];
    } catch {
      return [];
    }
  });
  // PERF-15: a page's `rows` read as the per-row events it carries (SYNAPSE-36 prints it as one line).
  const out = items.filter((i) => i.type === "data").flatMap((i) => (i.content?.type === "rows" ? i.content.items : [i.content]));
  const err = items.find((i) => i.type === "error");
  if (err) out.push({ type: "error", ...(err.content ?? err) });
  if (r.status !== 0 && !items.length) throw new Error(`synapse ${argv.join(" ")} failed (${r.status}): ${r.stderr}`);
  return out;
}

/** A bundle pushed: signed over its digest, its manifest.json as built.
 *  The `pushed` event, or what came instead. */
export function push(server, sessionFile, signerFile, { tar, manifest }) {
  const stem = tar.replace(/\.tar$/, "");
  fs.writeFileSync(`${stem}.b64`, fs.readFileSync(tar).toString("base64"));
  fs.writeFileSync(`${stem}.sig`, run(TOOL, ["sign", signerFile, tar]).trim());
  fs.writeFileSync(`${stem}.manifest.json`, manifest);
  const [answer] = synapse(server, sessionFile, "ui", "push", "--bundle", `@${stem}.b64`, "--manifest", `@${stem}.manifest.json`, "--signature", `@${stem}.sig`);
  return answer;
}

/** Every frame the page sent through `<service>.call`, as `{ t, method,
 *  params }` (t: the page's performance.now() when sent). */
export async function sentCalls(page) {
  const raw = await page.evaluate(() => window.__pages.sent.slice());
  return raw.flatMap(({ t, d }) => {
    try {
      const f = JSON.parse(d);
      if (typeof f.method !== "string" || !f.method.endsWith(".call")) return [];
      return [{ t, method: f.params?.method, params: f.params?.params ?? {} }];
    } catch {
      return [];
    }
  });
}

/** What every page runs before the engine: its sockets kept (to close
 *  one), every frame it sends stamped (`__pages.sent`), the frame that
 *  carries a `created` ticket stamped (`__pages.created[id]`), each row —
 *  any element with `data-id` — stamped as it appears (`__pages.rows[id]`;
 *  a `.board-row` in `__pages.boardRows[id]` too; `__pages.firstRow` when
 *  the first `.board-row` shows its title, `[data-field=title]`), each text `.board-count` shows (`__pages.counts`), the last key pressed
 *  (`__pages.lastKey`), and the engine's wasm memory kept
 *  (`__pages.memory`). Times are the page's own (`performance.now()`). */
export const INIT = () => {
  window.__pages = { sockets: [], sent: [], created: {}, rows: {}, boardRows: {}, counts: [], firstRow: null, lastKey: null, memory: null };
  const P = window.__pages;
  const Native = window.WebSocket;
  const send = Native.prototype.send;
  Native.prototype.send = function (d) {
    P.sent.push({ t: performance.now(), d: typeof d === "string" ? d : "" });
    return send.call(this, d);
  };
  window.WebSocket = class extends Native {
    constructor(...a) {
      super(...a);
      P.sockets.push(this);
      this.addEventListener("message", (e) => {
        const t = performance.now();
        if (typeof e.data !== "string" || !e.data.includes('"created"')) return;
        try {
          const c = JSON.parse(e.data)?.params?.result?.content;
          const id = c?.type === "created" ? c?.ticket?.id : null;
          if (id && !(id in P.created)) P.created[id] = t;
        } catch {
          /* not a frame we read */
        }
      });
    }
  };
  // The engine's memory: wasm-bindgen instantiates through one of these.
  const keep = (r) => {
    const inst = r?.instance ?? r;
    if (inst?.exports?.memory) P.memory = inst.exports.memory;
    return r;
  };
  for (const f of ["instantiate", "instantiateStreaming"]) {
    const orig = WebAssembly[f];
    if (orig) WebAssembly[f] = (...a) => orig.apply(WebAssembly, a).then(keep);
  }
  addEventListener("keydown", () => (P.lastKey = performance.now()), true);
  new MutationObserver((records) => {
    const t = performance.now();
    for (const r of records)
      for (const n of r.addedNodes) {
        if (n.nodeType !== 1) continue;
        const rows = n.matches?.("[data-id]") ? [n] : [];
        rows.push(...(n.querySelectorAll?.("[data-id]") ?? []));
        for (const el of rows) {
          const id = el.getAttribute("data-id");
          if (!(id in P.rows)) P.rows[id] = t;
          if (!el.classList.contains("board-row")) continue;
          if (!(id in P.boardRows)) P.boardRows[id] = t;
        }
      }
    // The first row counts once it shows its title, as `list` answered it.
    if (P.firstRow == null) {
      for (const el of document.querySelectorAll(".board-row[data-id] [data-field=title]")) {
        if (el.textContent.trim()) {
          P.firstRow = t;
          break;
        }
      }
    }
    for (const r of records) {
      const el = r.target.nodeType === 3 ? r.target.parentElement : r.target;
      const count = el?.closest?.(".board-count") ?? (el?.querySelector?.(".board-count") || null);
      if (count && P.counts.at(-1)?.text !== count.textContent) P.counts.push({ t, text: count.textContent });
    }
  }).observe(document, { childList: true, subtree: true, characterData: true });
};
