// UI-8: the harness the pages' browser test (tests/pages_browser.mjs) and
// the pages' probe (perf/pages-probe.mjs) share — UI-6's engine_browser.mjs
// setup, lifted out so both run it the same way:
//
//   build(profile)                 menger, ui-sign and ui-compiler from $CARGO_TARGET_DIR (else ./target)
//   bundle(work, name, edit?)      scripts/ui-build.sh → <work>/<name>.tar; `edit(dir)` changes a copy of ui/ first
//   new Throwaway(work).start(…)   a menger on OS-assigned ports, its files under the work directory,
//                                  `--auth-provider key`, an admin key and a ui signer, `--seed-synthetic N`
//   synapse(server, session, …)    synapse against it, `-j`'s data contents (the error item last)
//   push(server, session, signer, tar)   signed with ui-sign, pushed through `ui push`
//   playwright()                   Chromium from perf/node_modules (this checkout's, else the main one's)
//
// Never removes anything: the work directory's path is printed, and
// scripts/cleanup-gate.sh takes it.
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

/** A work directory under /tmp/claude-1000/ named `prefix-…`. */
export function workDir(prefix) {
  const base = "/tmp/claude-1000";
  fs.mkdirSync(base, { recursive: true });
  return fs.mkdtempSync(path.join(base, `${prefix}-`));
}

/** The server, the signer and the compiler, built for `profile`. */
export function build(profile) {
  const p = profile === "release" ? ["--release"] : [];
  const quiet = { cwd: REPO, stdio: ["ignore", "ignore", "inherit"] };
  run("cargo", ["build", ...p, "-p", "menger", "--bin", "menger"], quiet);
  // FRAME-5: the signer is the framework's (../plexus-ui), built into this target.
  run("cargo", ["build", ...p, "--manifest-path", path.join(REPO, "../plexus-ui/Cargo.toml"), "--target-dir", TARGET, "-p", "plexus-ui-service", "--bin", "ui-sign"], quiet);
}

/** ui/ (or a copy `edit` changed) built into `<work>/<name>.tar`; the
 *  engine taken from `engine` (a staging directory) when given. Its hash. */
export function bundle(work, name, { edit, engine } = {}) {
  const tar = path.join(work, `${name}.tar`);
  const argv = ["-o", tar];
  if (edit) {
    const ui = path.join(work, `${name}-ui`);
    fs.cpSync(path.join(REPO, "ui"), ui, { recursive: true });
    edit(ui);
    argv.push("--ui", ui);
  }
  if (engine) argv.push("--engine", engine);
  run(path.join(REPO, "scripts", "ui-build.sh"), argv, { cwd: REPO, stdio: ["ignore", "ignore", "inherit"] });
  return { tar, stage: tar.replace(/\.tar$/, ".d"), version: run("sha256sum", [tar]).split(" ")[0] };
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
    for (const v of ["MENGER_DB_PATH", "MENGER_AUTH_PROVIDERS", "MENGER_ADMINS", "MENGER_UI_DB_PATH", "MENGER_UI_SIGNERS", "MENGER_APP_DIST", "RUST_LOG"]) delete env[v];
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
    ];
    this.child = spawn(path.join(TARGET, this.profile, "menger"), argv, { env, stdio: ["ignore", "ignore", "pipe"] });
    this.child.stderr.on("data", (d) => (this.stderr += d.toString()));
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
  const out = items.filter((i) => i.type === "data").map((i) => i.content);
  const err = items.find((i) => i.type === "error");
  if (err) out.push({ type: "error", ...(err.content ?? err) });
  if (r.status !== 0 && !items.length) throw new Error(`synapse ${argv.join(" ")} failed (${r.status}): ${r.stderr}`);
  return out;
}

/** A bundle pushed: signed over its digest, its manifest.json as built.
 *  The `pushed` event, or what came instead. */
export function push(server, sessionFile, signerFile, { tar, stage }) {
  const stem = tar.replace(/\.tar$/, "");
  fs.writeFileSync(`${stem}.b64`, fs.readFileSync(tar).toString("base64"));
  fs.writeFileSync(`${stem}.sig`, run(path.join(TARGET, server.profile, "ui-sign"), [signerFile, tar]).trim());
  const [answer] = synapse(server, sessionFile, "ui", "push", "--bundle", `@${stem}.b64`, "--manifest", `@${stem}.d/manifest.json`, "--signature", `@${stem}.sig`);
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
