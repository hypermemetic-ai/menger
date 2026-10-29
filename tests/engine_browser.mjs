#!/usr/bin/env node
// UI-6's testing surface, in a browser: the engine on a throwaway menger.
//
//   node tests/engine_browser.mjs [--seed N] [--release] [--headed] [--keep]
//
// A throwaway server (its own ports, OS-assigned; its own files under
// /tmp/claude-1000/; `--auth-provider key` with a key made here as
// `--admin`, a second key as `--ui-signer`; `--seed-synthetic N` tickets,
// default 1000 — `--seed 100000` is acceptance 3's measurement). Two
// bundles are built with scripts/ui-build.sh — ui/ as it is (v1), and a
// copy whose hello text says v2 (the engine built once) — signed with
// `ui-sign` and pushed through synapse (`ui push`). Chromium from
// Playwright (perf/node_modules, else the main checkout's), signed in with
// a session minted for the admin key (scripts/throwaway-login.mjs).
//
//   the_page_shows_rows_and_hears_a_create_and_a_push
//     after sign-in /app/ lists the seeded tickets (tickets.list's first
//     200, in order); a ticket created over synapse appears without a
//     reload; a second push with the hello text changed swaps it in place
//     (the DOM text changes; one navigation entry, a window marker kept).
//   a_right_click_swaps_a_section_to_another_version
//     a right-click on the section opens the engine's menu; picking the
//     older version shows the older text and `ui slots list` names it;
//     `ui slots clear` returns the section to the live version's text.
//
// Timings (acceptance 3) are printed: navigation → first rows, and a
// create's call → its row on screen. Exit 0 when every test passed.
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";
import { makeKey, readToken, sessionCookies, writeSession } from "../scripts/throwaway-login.mjs";

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
    const deadline = Date.now() + 300_000;
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
 *  data items' contents. Its cache stays in the work directory. */
function synapse(server, sessionFile, ...argv) {
  const env = { ...process.env, XDG_CACHE_HOME: path.join(server.work, "cache") };
  const out = run("synapse", ["--no-hold", "--token-file", sessionFile, "-j", "-P", String(server.ws), "menger", ...argv], { env });
  const items = out.split("\n").filter(Boolean).map((l) => JSON.parse(l));
  const err = items.find((i) => i.type === "error");
  if (err) throw new Error(`synapse ${argv.join(" ")}: ${JSON.stringify(err)}`);
  return items.filter((i) => i.type === "data").map((i) => i.content);
}

/** A bundle pushed: signed by the signer's seed file over its digest. UI-3's
 *  store reads `contract:` and `abi:` lines off the manifest it is sent, so
 *  those lead, and manifest.json follows them as it is. */
function push(server, sessionFile, signerFile, tar) {
  const stem = tar.replace(/\.tar$/, "");
  const manifest = JSON.parse(fs.readFileSync(`${stem}.d/manifest.json`, "utf8"));
  fs.writeFileSync(`${stem}.b64`, fs.readFileSync(tar).toString("base64"));
  fs.writeFileSync(`${stem}.sig`, run(path.join(TARGET, "debug", "ui-sign"), [signerFile, tar]).trim());
  fs.writeFileSync(`${stem}.manifest`, `contract: ${manifest.contract}\nabi: ${manifest.abi}\n${JSON.stringify(manifest, null, 2)}\n`);
  const [pushed] = synapse(server, sessionFile, "ui", "push", "--bundle", `@${stem}.b64`, "--manifest", `@${stem}.manifest`, "--signature", `@${stem}.sig`);
  if (pushed?.type !== "pushed") throw new Error(`ui push: ${JSON.stringify(pushed)}`);
  return pushed.version;
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
    await sleep(50);
  }
  throw new Error(`assertion failed: ${what} (waited ${ms} ms)${last ? `: ${last.message}` : ""}`);
}

// ── the tests ────────────────────────────────────────────────────────────
const results = [];
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
  console.log(`work: ${work}`);

  // Build: the server, the signer, and the two bundles (the engine once).
  run("cargo", ["build", ...(PROFILE === "release" ? ["--release"] : []), "-p", "menger", "--bin", "menger"], { cwd: REPO, stdio: ["ignore", "ignore", "inherit"] });
  run("cargo", ["build", "-p", "menger-ui-service", "--bin", "ui-sign"], { cwd: REPO, stdio: ["ignore", "ignore", "inherit"] });
  const v1Tar = path.join(work, "v1.tar");
  run(path.join(REPO, "scripts", "ui-build.sh"), ["-o", v1Tar], { cwd: REPO, stdio: ["ignore", "ignore", "inherit"] });
  const ui2 = path.join(work, "ui2");
  fs.cpSync(path.join(REPO, "ui"), ui2, { recursive: true });
  for (const f of [...fs.readdirSync(path.join(ui2, "src")).map((n) => path.join(ui2, "src", n)), ...fs.readdirSync(path.join(ui2, "golden")).map((n) => path.join(ui2, "golden", n))]) {
    fs.writeFileSync(f, fs.readFileSync(f, "utf8").replaceAll(HELLO_V1, HELLO_V2));
  }
  const v2Tar = path.join(work, "v2.tar");
  run(path.join(REPO, "scripts", "ui-build.sh"), ["--ui", ui2, "--engine", path.join(work, "v1.d"), "-o", v2Tar], { cwd: REPO, stdio: ["ignore", "ignore", "inherit"] });

  // The keys: the admin, and the ui signer.
  const adminKey = path.join(work, "admin.key");
  const signerKey = path.join(work, "signer.key");
  const admin = makeKey(adminKey);
  const signerHex = makeKey(signerKey).replace(/^key:/, "");

  const server = new Throwaway(work);
  await server.start(admin, signerHex);
  const session = path.join(work, "session");
  await writeSession(`ws://127.0.0.1:${server.ws}`, adminKey, session, "owner");
  const token = readToken(session);

  const { chromium } = await playwright();
  const browser = await chromium.launch({ headless: !flag("--headed") });
  const context = await browser.newContext();
  // The shell's session cookie (the page), and the credential cookie the
  // socket's upgrade reads (cookies are per host, not per port).
  await context.addCookies([...sessionCookies(server.origin, token), { name: "access_token", value: token, url: `${server.origin}/`, sameSite: "Lax" }]);
  const page = await context.newPage();
  const consoleLines = [];
  page.on("console", (m) => consoleLines.push(`${m.type()}: ${m.text()}`));
  page.on("pageerror", (e) => consoleLines.push(`pageerror: ${e.message}`));

  let v1 = null;
  let v2 = null;
  const ensureV2 = () => (v2 ??= push(server, session, signerKey, v2Tar));
  const helloText = () => page.locator("[data-slot=main] .hello").textContent({ timeout: 1000 });
  const navigations = () => page.evaluate(() => performance.getEntriesByType("navigation").length);

  try {
    v1 = push(server, session, signerKey, v1Tar);
    console.log(`v1 ${v1}`);

    await test("the_page_shows_rows_and_hears_a_create_and_a_push", async () => {
      const expected = synapse(server, session, "tickets", "list", "--summary", "true", "--limit", "200")
        .filter((e) => e.type === "summary")
        .map((e) => e.summary.id);
      assert(expected.length === Math.min(200, SEED), `the server lists ${expected.length} of ${SEED} seeded tickets`);

      const navStart = Date.now();
      await page.goto(`${server.origin}/app/`);
      await eventually("the page lists the seeded tickets", async () => (await page.locator("li[data-id]").count()) > 0);
      const firstRowsMs = await page.evaluate(() => performance.now());
      const shown = await page.locator("li[data-id]").evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));
      assert(JSON.stringify(shown) === JSON.stringify(expected), `the rows are tickets.list's, in order (shown ${shown.length}, expected ${expected.length})`);
      assert((await page.locator("[data-slot=main] .status").textContent()) === "live", "the collection is live");
      assert((await helloText()) === HELLO_V1, "v1's hello");
      console.log(`     navigation → first rows: ${firstRowsMs.toFixed(0)} ms (page) / ${Date.now() - navStart} ms (wall)`);
      await page.evaluate(() => (window.__ui6_marker = "kept"));

      // A create over synapse: on screen without a reload.
      const epic = synapse(server, session, "tickets", "get", "--id", expected[0])[0]?.ticket?.epic;
      assert(epic, "the first ticket's epic");
      const t0 = Date.now();
      const [created] = synapse(server, session, "tickets", "create", "--epic", epic, "--title", "made by the browser test");
      const id = created?.ticket?.id;
      assert(id, `tickets create answered a ticket: ${JSON.stringify(created)}`);
      await eventually(`the created ticket ${id} is on screen`, async () => (await page.locator(`li[data-id="${id}"]`).count()) === 1, 5_000);
      console.log(`     create call → its row on screen: ${Date.now() - t0} ms (synapse's round trip included)`);
      assert((await navigations()) === 1, "no reload after the create");

      // A second push: the hello text swapped in place.
      ensureV2();
      console.log(`     v2 ${v2}`);
      await eventually("the second push's hello text is drawn", async () => (await helloText()) === HELLO_V2);
      assert((await navigations()) === 1, "one navigation: no reload on a push");
      assert((await page.evaluate(() => window.__ui6_marker)) === "kept", "the page's state survived the push");
      assert((await page.locator(`li[data-id="${id}"]`).count()) === 1, "the rows still shown after the swap");
    });

    await test("a_right_click_swaps_a_section_to_another_version", async () => {
      ensureV2();
      if (!page.url().includes("/app/")) await page.goto(`${server.origin}/app/`);
      await eventually("v2's hello before the right-click", async () => (await helloText()) === HELLO_V2);

      await page.locator("[data-slot=main]").click({ button: "right" });
      const menu = page.locator("[data-engine-menu]");
      await eventually("the engine's menu opens on the section", async () => menu.isVisible());
      assert((await menu.locator('[data-component="hello-list"]').count()) === 1, "the menu lists the live bundle's components");
      const older = menu.locator(`[data-version="${v1}"]`);
      assert((await older.count()) === 1, "the menu lists the section's component's older version");
      await older.click();
      await eventually("the section shows the older version's text", async () => (await helloText()) === HELLO_V1);
      assert((await navigations()) === 1, "no reload on the swap");

      const slots = synapse(server, session, "ui", "slots", "list", "--layout", "page-shell");
      const main = slots.find((s) => s.slot === "main");
      assert(main?.component === "hello-list" && main?.version === v1, `ui slots list names the older version: ${JSON.stringify(slots)}`);

      synapse(server, session, "ui", "slots", "clear", "--layout", "page-shell", "--slot", "main");
      await eventually("clear returns the section to the live version", async () => (await helloText()) === HELLO_V2);
      assert((await navigations()) === 1, "no reload on the clear");
    });
  } finally {
    if (results.some((r) => !r.ok)) {
      console.log("the page's console:");
      for (const l of consoleLines.slice(-30)) console.log(`     ${l}`);
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
