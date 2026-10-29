#!/usr/bin/env node
// UI-6's testing surface, in a browser: the engine on a throwaway menger.
//
//   node tests/engine_browser.mjs [--seed N] [--release] [--headed] [--keep]
//
// A throwaway server (its own ports, OS-assigned; its own files under
// /tmp/claude-1000/; `--auth-provider key` with a key made here as
// `--admin`, a second key as `--ui-signer`; `--seed-synthetic N` tickets,
// default 1000 — `--seed 100000 --release` is acceptance 3's measurement).
// Everything is built and spawned from $CARGO_TARGET_DIR (else ./target).
// Two bundles are built with scripts/ui-build.sh — ui/ as it is (v1), and a
// copy whose hello text says v2 (the engine built once) — signed with
// `ui-sign` and pushed through synapse (`ui push`, manifest.json as built).
// Chromium from Playwright (perf/node_modules, else the main checkout's),
// signed in with the shell's session cookie alone, minted for the admin key
// (scripts/throwaway-login.mjs).
//
// An init script on every page counts its WebSockets (`page.on('websocket')`
// too), wraps them so a test can close one, stamps `performance.now()` on
// the frame that carries a `created` ticket, and a MutationObserver stamps
// each row (`li[data-id]`) as it appears — the timings are the page's own.
//
//   the_page_shows_rows_and_hears_a_create_and_a_push
//     v1 pushed; after sign-in /app/#/hello (UI-8 gave #/ to the board) lists the seeded tickets
//     (tickets.list's first 200, in order); a ticket created over synapse
//     appears without a reload, < 100 ms after its frame; the socket closed
//     under the page: `resyncing`, then `live` within about a second, and a
//     later create still appears; v2 pushed: the hello text swaps in place.
//     One socket until the close, two after; a window marker and
//     performance.timeOrigin unchanged after every step (no reload).
//   a_right_click_swaps_a_section_to_another_version
//     a fresh page; a right-click on the section opens the engine's menu
//     (the live components, and exactly one other version: v1, never v2);
//     picking v1 fetches v1's hello-list.json and .rhai, shows the older
//     text, and `ui slots list` names it; `ui slots clear` returns it. A
//     viewer's page: the same pick is refused in the menu and `ui slots
//     list` is unchanged.
//   the_dom_events_reach_a_fold
//     a real scroll of the rows, a keydown and a checkbox reach the
//     component's folds with scroll_top / scroll_height / client_height,
//     key and checked (UI-10).
//
// Exit 0 when every test passed, 1 when one failed, 2 when setup failed.
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";
import { makeKey, mintSession, principalOf, readToken, sessionCookies, writeSession } from "../scripts/throwaway-login.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(HERE, "..");
const args = process.argv.slice(2);
const flag = (name) => args.includes(name);
const opt = (name, dflt) => (args.includes(name) ? args[args.indexOf(name) + 1] : dflt);
const SEED = Number(opt("--seed", "1000"));
const PROFILE = flag("--release") ? "release" : "debug";
const TARGET = process.env.CARGO_TARGET_DIR || path.join(REPO, "target");
const HELLO_V1 = "Hello from v1";
const HELLO_V2 = "Hello from v2";
const FIRST_ROW_MS = 300;
const CREATE_MS = 100;

// ── Playwright, from perf/node_modules (this checkout's, else main's) ────
async function playwright() {
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

// ── helpers ──────────────────────────────────────────────────────────────
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function freePort() {
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

function run(cmd, argv, opts = {}) {
  const r = spawnSync(cmd, argv, { encoding: "utf8", maxBuffer: 256 * 1024 * 1024, ...opts });
  if (r.status !== 0) throw new Error(`${cmd} ${argv.join(" ")} failed (${r.status}):\n${r.stderr || r.stdout}`);
  return r.stdout;
}

/** A throwaway work directory under /tmp/claude-1000/ (never removed by
 *  this script: its path is printed; scripts/cleanup-gate.sh takes it). */
function workDir() {
  const base = "/tmp/claude-1000";
  fs.mkdirSync(base, { recursive: true });
  return fs.mkdtempSync(path.join(base, "ui6-browser-"));
}

class Throwaway {
  constructor(work) {
    this.work = work;
    this.stderr = "";
  }

  async start(adminPrincipal, signerHex) {
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
      "--seed-synthetic", String(SEED),
      "--seed-value", "6",
    ];
    this.child = spawn(path.join(TARGET, PROFILE, "menger"), argv, { env, stdio: ["ignore", "ignore", "pipe"] });
    this.child.stderr.on("data", (d) => (this.stderr += d.toString()));
    const deadline = Date.now() + 600_000;
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

/** synapse against the throwaway, signed in by the session file; `-j`'s
 *  data items' contents, the error item last when there is one. Its cache
 *  stays in the work directory. */
function synapse(server, sessionFile, ...argv) {
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

/** A bundle pushed: signed by the signer's seed file over its digest, its
 *  manifest.json sent as built. The `pushed` event, or what came instead. */
function push(server, sessionFile, signerFile, tar) {
  const stem = tar.replace(/\.tar$/, "");
  fs.writeFileSync(`${stem}.b64`, fs.readFileSync(tar).toString("base64"));
  fs.writeFileSync(`${stem}.sig`, run(path.join(TARGET, PROFILE, "ui-sign"), [signerFile, tar]).trim());
  const [answer] = synapse(server, sessionFile, "ui", "push", "--bundle", `@${stem}.b64`, "--manifest", `@${stem}.d/manifest.json`, "--signature", `@${stem}.sig`);
  return answer;
}

function assert(cond, msg) {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}

async function eventually(what, fn, ms = 10_000) {
  const deadline = Date.now() + ms;
  let last;
  while (Date.now() < deadline) {
    try {
      const v = await fn();
      if (v) return v;
    } catch (e) {
      last = e;
    }
    await sleep(25);
  }
  throw new Error(`assertion failed: ${what} (waited ${ms} ms)${last ? `: ${last.message}` : ""}`);
}

/** What every page runs first: its sockets counted and kept (to close
 *  one), a `created` ticket's frame stamped, each row stamped on sight. */
const INIT = () => {
  window.__ui6 = { sockets: [], created: {}, rows: {}, firstRow: null };
  const Native = window.WebSocket;
  window.WebSocket = class extends Native {
    constructor(...a) {
      super(...a);
      window.__ui6.sockets.push(this);
      this.addEventListener("message", (e) => {
        const t = performance.now();
        if (typeof e.data !== "string" || !e.data.includes('"created"')) return;
        try {
          const id = JSON.parse(e.data)?.params?.result?.content?.ticket?.id;
          if (id && !(id in window.__ui6.created)) window.__ui6.created[id] = t;
        } catch {
          /* not a frame we read */
        }
      });
    }
  };
  new MutationObserver((records) => {
    const t = performance.now();
    for (const r of records)
      for (const n of r.addedNodes) {
        if (n.nodeType !== 1) continue;
        const rows = n.matches?.("li[data-id]") ? [n] : [...(n.querySelectorAll?.("li[data-id]") ?? [])];
        for (const li of rows) {
          const id = li.getAttribute("data-id");
          if (!(id in window.__ui6.rows)) window.__ui6.rows[id] = t;
          window.__ui6.firstRow ??= t;
        }
      }
  }).observe(document, { childList: true, subtree: true });
};

/** A page signed in with `token`, its sockets and requests counted. */
async function openPage(browser, server, token) {
  const context = await browser.newContext();
  await context.addCookies(sessionCookies(server.origin, token));
  await context.addInitScript(INIT);
  const page = await context.newPage();
  const seen = { sockets: 0, requests: [], console: [] };
  page.on("websocket", () => seen.sockets++);
  page.on("request", (r) => seen.requests.push(r.url()));
  page.on("console", (m) => seen.console.push(`${m.type()}: ${m.text()}`));
  page.on("pageerror", (e) => seen.console.push(`pageerror: ${e.message}`));
  return { context, page, seen };
}

/** The page has not reloaded since `before` (its marker and timeOrigin). */
async function sameDocument(page, before, step) {
  const now = await page.evaluate(() => ({ marker: window.__ui6_marker, origin: performance.timeOrigin }));
  assert(now.marker === "kept" && now.origin === before, `no reload: ${step}`);
}

// ── the tests ────────────────────────────────────────────────────────────
const results = [];
const pages = [];
async function test(name, body) {
  const t0 = Date.now();
  try {
    await body();
    results.push({ name, ok: true });
    console.log(`ok   ${name} (${Date.now() - t0} ms)`);
  } catch (e) {
    results.push({ name, ok: false, error: e });
    console.log(`FAIL ${name}\n     ${e.message.split("\n").join("\n     ")}`);
  }
}

async function main() {
  const work = workDir();
  console.log(`work: ${work} · seed ${SEED} · ${PROFILE} · target ${TARGET}`);

  // Build: the server, the signer, and the two bundles (the engine once).
  const cargoProfile = PROFILE === "release" ? ["--release"] : [];
  run("cargo", ["build", ...cargoProfile, "-p", "menger", "--bin", "menger"], { cwd: REPO, stdio: ["ignore", "ignore", "inherit"] });
  run("cargo", ["build", ...cargoProfile, "-p", "menger-ui-service", "--bin", "ui-sign"], { cwd: REPO, stdio: ["ignore", "ignore", "inherit"] });
  const v1Tar = path.join(work, "v1.tar");
  run(path.join(REPO, "scripts", "ui-build.sh"), ["-o", v1Tar], { cwd: REPO, stdio: ["ignore", "ignore", "inherit"] });
  const ui2 = path.join(work, "ui2");
  fs.cpSync(path.join(REPO, "ui"), ui2, { recursive: true });
  for (const dir of ["src", "golden"]) {
    for (const n of fs.readdirSync(path.join(ui2, dir))) {
      const f = path.join(ui2, dir, n);
      fs.writeFileSync(f, fs.readFileSync(f, "utf8").replaceAll(HELLO_V1, HELLO_V2));
    }
  }
  const v2Tar = path.join(work, "v2.tar");
  run(path.join(REPO, "scripts", "ui-build.sh"), ["--ui", ui2, "--engine", path.join(work, "v1.d"), "-o", v2Tar], { cwd: REPO, stdio: ["ignore", "ignore", "inherit"] });
  const v1 = run("sha256sum", [v1Tar]).split(" ")[0];
  const v2 = run("sha256sum", [v2Tar]).split(" ")[0];
  console.log(`v1 ${v1}\nv2 ${v2}`);

  // The keys: the admin, the ui signer, a viewer.
  const adminKey = path.join(work, "admin.key");
  const signerKey = path.join(work, "signer.key");
  const viewerKey = path.join(work, "viewer.key");
  const admin = makeKey(adminKey);
  const signerHex = makeKey(signerKey).replace(/^key:/, "");
  makeKey(viewerKey);

  const server = new Throwaway(work);
  await server.start(admin, signerHex);
  const session = path.join(work, "session");
  await writeSession(`ws://127.0.0.1:${server.ws}`, adminKey, session, "owner");
  const token = readToken(session);
  synapse(server, session, "auth", "grant", "--principal", principalOf(viewerKey), "--role", "viewer", "--scope", "*");
  const viewerToken = (await mintSession(`ws://127.0.0.1:${server.ws}`, viewerKey, "viewer")).token;

  const { chromium } = await playwright();
  const browser = await chromium.launch({ headless: !flag("--headed") });
  const pushed = new Map();
  const pushOnce = (tar, version) => {
    if (!pushed.has(version)) pushed.set(version, push(server, session, signerKey, tar));
    const answer = pushed.get(version);
    assert(answer?.type === "pushed" && answer.version === version, `ui push of ${path.basename(tar)} answered pushed ${version}: ${JSON.stringify(answer)}`);
  };
  const hello = (page) => page.locator("[data-slot=main] .hello").textContent({ timeout: 1000 });
  const status = (page) => page.locator("[data-slot=main] .status").textContent({ timeout: 1000 });

  try {
    await test("the_page_shows_rows_and_hears_a_create_and_a_push", async () => {
      pushOnce(v1Tar, v1);
      const expected = synapse(server, session, "tickets", "list", "--summary", "true", "--limit", "200")
        .filter((e) => e.type === "summary")
        .map((e) => e.summary.id);
      assert(expected.length === Math.min(200, SEED), `the server lists ${expected.length} of ${SEED} seeded tickets`);

      const { page, seen } = await openPage(browser, server, token);
      pages.push(seen);
      await page.goto(`${server.origin}/app/#/hello`);
      await eventually("the page lists the seeded tickets", async () => (await page.locator("li[data-id]").count()) === expected.length);
      const shown = await page.locator("li[data-id]").evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));
      assert(JSON.stringify(shown) === JSON.stringify(expected), `the rows are tickets.list's, in order (shown ${shown.length}, expected ${expected.length})`);
      assert((await status(page)) === "live", "the collection is live");
      assert((await hello(page)) === HELLO_V1, "v1's hello");
      const firstRow = await page.evaluate(() => window.__ui6.firstRow);
      console.log(`     first row: ${firstRow.toFixed(0)} ms from timeOrigin (budget ${FIRST_ROW_MS} ms)`);
      const origin = await page.evaluate(() => ((window.__ui6_marker = "kept"), performance.timeOrigin));
      assert(seen.sockets === 1, `one socket (${seen.sockets})`);

      // A create over synapse: on screen without a reload, < 100 ms after its frame.
      const epic = synapse(server, session, "tickets", "get", "--id", expected[0])[0]?.ticket?.epic;
      assert(epic, "the first ticket's epic");
      const create = async (title) => {
        const [created] = synapse(server, session, "tickets", "create", "--epic", epic, "--title", title);
        const id = created?.ticket?.id;
        assert(id, `tickets create answered a ticket: ${JSON.stringify(created)}`);
        await eventually(`the created ticket ${id} is on screen`, async () => (await page.locator(`li[data-id="${id}"]`).count()) === 1, 5_000);
        return id;
      };
      const id = await create("made by the browser test");
      const t = await page.evaluate((id) => ({ frame: window.__ui6.created[id], row: window.__ui6.rows[id] }), id);
      assert(t.frame != null && t.row != null, `the frame and the row were both stamped: ${JSON.stringify(t)}`);
      const createMs = t.row - t.frame;
      console.log(`     created frame → its row: ${createMs.toFixed(1)} ms (budget ${CREATE_MS} ms)`);
      await sameDocument(page, origin, "after the create");
      assert(seen.sockets === 1, `one socket after the create (${seen.sockets})`);
      assert(firstRow < FIRST_ROW_MS, `the first row within ${FIRST_ROW_MS} ms of timeOrigin (${firstRow.toFixed(0)} ms)`);
      assert(createMs < CREATE_MS, `a created ticket on screen within ${CREATE_MS} ms of its frame (${createMs.toFixed(1)} ms)`);

      // The socket closed under the page: resyncing, then live, by itself.
      const closedAt = Date.now();
      await page.evaluate(() => window.__ui6.sockets.at(-1).close());
      await eventually("resyncing once the socket is gone", async () => (await status(page)) === "resyncing", 2_000);
      await eventually("live again after the reconnect", async () => (await status(page)) === "live", 3_000);
      console.log(`     socket closed → live again: ${Date.now() - closedAt} ms`);
      assert(seen.sockets === 2, `one new socket for the reconnect (${seen.sockets})`);
      await create("made after the reconnect");
      await sameDocument(page, origin, "after the reconnect");

      // A second push: the hello text swapped in place, the rows as they
      // were (the resync after the reconnect asked tickets.list again, so a
      // ticket made before it may be past the first 200: compare, not look up).
      const rowsBefore = await page.locator("li[data-id]").evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));
      pushOnce(v2Tar, v2);
      await eventually("the second push's hello text is drawn", async () => (await hello(page)) === HELLO_V2);
      await sameDocument(page, origin, "after the push");
      const rowsAfter = await page.locator("li[data-id]").evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));
      assert(rowsAfter.length > 0 && JSON.stringify(rowsAfter) === JSON.stringify(rowsBefore), `the rows as they were after the swap (${rowsBefore.length} → ${rowsAfter.length})`);
      assert(seen.sockets === 2, `no socket opened for the push (${seen.sockets})`);
    });

    await test("a_right_click_swaps_a_section_to_another_version", async () => {
      pushOnce(v1Tar, v1);
      pushOnce(v2Tar, v2);
      const { page, seen } = await openPage(browser, server, token);
      pages.push(seen);
      await page.goto(`${server.origin}/app/#/hello`);
      await eventually("v2's hello before the right-click", async () => (await hello(page)) === HELLO_V2);
      const origin = await page.evaluate(() => ((window.__ui6_marker = "kept"), performance.timeOrigin));
      const slotsBefore = synapse(server, session, "ui", "slots", "list", "--layout", "page-shell");

      await page.locator("[data-slot=main] .hello").click({ button: "right" });
      const menu = page.locator("[data-engine-menu]");
      await eventually("the engine's menu opens on the section", async () => menu.isVisible());
      assert((await menu.locator('[data-component="hello-list"]').count()) === 1, "the menu lists the live bundle's components");
      await eventually("the menu lists the other version", async () => (await menu.locator("[data-version]").count()) === 1, 3_000);
      assert((await menu.locator(`[data-version="${v2}"]`).count()) === 0, "the live version is not offered");
      const older = menu.locator(`[data-version="${v1}"]`);
      assert((await older.count()) === 1, "the older version is offered");
      const fetchedBefore = seen.requests.length;
      await older.click();
      await eventually("the section shows the older version's text", async () => (await hello(page)) === HELLO_V1);
      const fetched = seen.requests.slice(fetchedBefore);
      for (const f of ["hello-list.json", "hello-list.rhai"]) {
        assert(fetched.some((u) => u.endsWith(`/app/${v1}/components/${f}`)), `the engine fetched v1's ${f}: ${fetched.join(" ")}`);
      }
      await sameDocument(page, origin, "after the pick");

      const slots = synapse(server, session, "ui", "slots", "list", "--layout", "page-shell");
      const main = slots.find((s) => s.type === "assignment" && s.slot === "main");
      assert(main?.component === "hello-list" && main?.version === v1, `ui slots list names the older version: ${JSON.stringify(slots)}`);

      // A viewer's pick is refused in the menu; nothing is written.
      const viewer = await openPage(browser, server, viewerToken);
      pages.push(viewer.seen);
      await viewer.page.goto(`${server.origin}/app/#/hello`);
      await eventually("the viewer's page shows the section", async () => (await hello(viewer.page)) === HELLO_V1);
      await viewer.page.locator("[data-slot=main] .hello").click({ button: "right" });
      const vmenu = viewer.page.locator("[data-engine-menu]");
      await eventually("the viewer's menu opens", async () => vmenu.isVisible());
      await vmenu.locator('[data-component="hello-list"]').click();
      await eventually("the refusal is shown in the menu", async () => ((await vmenu.locator("[data-refused]").textContent({ timeout: 1000 })) ?? "").length > 0, 3_000);
      const after = synapse(server, session, "ui", "slots", "list", "--layout", "page-shell");
      assert(JSON.stringify(after) === JSON.stringify(slots), `the viewer's pick wrote nothing: ${JSON.stringify(after)}`);

      synapse(server, session, "ui", "slots", "clear", "--layout", "page-shell", "--slot", "main");
      await eventually("clear returns the section to the live version", async () => (await hello(page)) === HELLO_V2);
      await sameDocument(page, origin, "after the clear");
      assert(seen.sockets === 1, `one socket throughout (${seen.sockets})`);
      assert(JSON.stringify(slotsBefore.filter((s) => s.type === "assignment")) === "[]", "no assignment before the pick");
    });
    await test("the_dom_events_reach_a_fold", async () => {
      pushOnce(v1Tar, v1);
      const { page, seen } = await openPage(browser, server, token);
      pages.push(seen);
      await page.goto(`${server.origin}/app/#/hello`);
      await eventually("the rows drawn", async () => (await page.locator("[data-slot=main] li[data-id]").count()) > 20);
      const said = () => page.locator("[data-slot=main] .seen").textContent({ timeout: 1000 });

      // A real scroll of the list: its metrics reach the fold.
      await page.locator("[data-slot=main] ul.rows").evaluate((el) => (el.scrollTop = 300));
      const scroll = await eventually("a scroll reaches the fold", async () => /^scroll (\d+)\/(\d+)\/(\d+)$/.exec((await said()) ?? ""));
      const [top, height, client] = scroll.slice(1).map(Number);
      assert(top > 0 && height > client && client > 0, `scroll_top ${top}, scroll_height ${height}, client_height ${client}`);

      // A keydown: its key.
      await page.locator("[data-slot=main] input.keys").press("k");
      await eventually("a keydown's key reaches the fold", async () => (await said()) === "key k");

      // A checkbox: checked.
      await page.locator("[data-slot=main] input.tick").check();
      await eventually("a checkbox's checked reaches the fold", async () => (await said()) === "checked true");
      await page.locator("[data-slot=main] input.tick").uncheck();
      await eventually("and unchecked", async () => (await said()) === "checked false");
    });
  } finally {
    if (results.some((r) => !r.ok)) {
      for (const [i, seen] of pages.entries()) {
        console.log(`page ${i + 1}'s console:`);
        for (const l of seen.console.slice(-20)) console.log(`     ${l}`);
      }
    }
    await browser.close();
    if (!flag("--keep")) server.stop();
    else console.log(`kept: ws ${server.ws}, shell ${server.origin} (pid ${server.child.pid})`);
  }

  const failed = results.filter((r) => !r.ok).length;
  console.log(`${results.length - failed} passed, ${failed} failed`);
  process.exit(failed ? 1 : 0);
}

main().catch((e) => {
  console.error(`setup failed: ${e.stack || e.message}`);
  process.exit(2);
});
