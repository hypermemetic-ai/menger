#!/usr/bin/env node
// UI-8's testing surface, in a browser: the first pages on a throwaway menger.
//
//   node tests/pages_browser.mjs [--seed N] [--debug] [--no-build] [--headed] [--keep] [--only NAME[,NAME]] [--board rhx,sv]
//
// FRAME-14: every test of the board runs on both boards — the rhx one (`#/`,
// named `NAME`) and the Svelte one (`#/sv/`, svelte/1, named `NAME_sv`),
// which draw the same marks — and `a_section_swaps_to_the_svelte_one` puts
// each Svelte component in its section of the rhx board by right-click.
// `--board` runs one of them.
//
// A throwaway server (scripts/ui-throwaway.mjs: OS-assigned ports, its
// files under /tmp/claude-1000/, `--auth-provider key` with a key made here
// as `--admin`, a second as `--ui-signer`), seeded `--seed-synthetic N`
// (default 100,000), release build (`--debug` for the debug one). Two
// bundles from plexus-ui build, signed with plexus-ui sign and pushed over
// synapse: v1 is ui/ as it is; v2 the same with one line added to
// board-rows.rhx and to board-rows-sv.svelte (`<p class="board-newer">`),
// so the two differ in the boards alone. v2 is pushed last, so it is live. Chromium from Playwright,
// signed in with the shell's session cookie (scripts/throwaway-login.mjs).
//
// Measured in the page (scripts/ui-throwaway.mjs's INIT, before the
// engine): every frame the page sends, stamped; the frame that carries a
// `created` ticket; each row as it appears; what `.board-count` shows.
//
// The testing hooks the pages draw (UI-8's body: the named three, and
// `data-*` attributes for the rest):
//
//   /app/ (#/, #/f/:filter)  board-layout: sections `[data-layout=board-layout][data-slot=nav|main|palette]`;
//                       in its own template the filter line, `input[data-filter-input]`, its refusal `[data-filter-refused]`
//   main: board-rows    `.board-scroll[data-row-height]` (the scrolling element, one fixed pitch in px),
//                       `.board-row[data-id]` (a row held; its `[data-field=title]`, `[data-field=status]`),
//                       `.board-count` (the tickets' number)
//   nav: nav-tree       `[data-nav-mode=tree|flat]` (the switch); in Tree `[data-nav-group=<shortcode>]` with
//                       `[data-rollup-done]`, `[data-rollup-total]`, `[data-nav-toggle]`, its open children
//                       `[data-nav-row=<id>]`; in Flat `[data-nav-flat-row=<id>]`
//   palette             `input[data-palette-input]` (`/` focuses it), `[data-palette-hit=<id>]` (find, in
//                       order), `[data-search-hit=<id>]` (search's, below)
//   #/ticket/:id        ticket-page: `[data-ticket-title]`, `[data-ticket-status]`, `[data-ticket-kind]`,
//                       `[data-chip=parent|blocker|unlocks][data-chip-id]` each with `[data-chip-title]`,
//                       `[data-ticket-body]`, `[data-ticket-child=<id>]`, `select[data-status-menu]`
//
// The Svelte board (FRAME-14), the same marks under its own names:
//
//   /app/#/sv/ (#/sv/f/:filter)  board-frame, an rhx layout (a section is the host's, and only an
//                       rhx template marks one): `[data-layout=board-frame][data-slot=top|nav|main|palette]`,
//                       holding board-layout-sv (the top bar: `input[data-filter-input]`,
//                       `[data-filter-refused]`), nav-tree-sv, board-rows-sv and jump-palette-sv
//   #/sv/ticket/:id     ticket-page-sv; its links and chips stay under #/sv/
//
// Exit 0 when every test passed, 1 when one failed, 2 when setup failed.
import fs from "node:fs";
import path from "node:path";
import { makeKey, readToken, sessionCookies, writeSession } from "../scripts/throwaway-login.mjs";
import { INIT, TARGET, Throwaway, build, bundle, playwright, push, sentCalls, sleep, synapse, workDir } from "../scripts/ui-throwaway.mjs";

const args = process.argv.slice(2);
const flag = (name) => args.includes(name);
const opt = (name, dflt) => (args.includes(name) ? args[args.indexOf(name) + 1] : dflt);
const SEED = Number(opt("--seed", "100000"));
const PROFILE = flag("--debug") ? "debug" : "release";
const ONLY = opt("--only", null)?.split(",");
// SURFACE-1: a feature group's cases live in tests/pages/<group>.mjs and run after the board's;
// `--group NAME` runs that file's cases alone (the board's own are skipped), `--group all` every file.
const GROUP = opt("--group", null);
let inGroup = false;
const PAGE = 200; // the board's page
const HELD_MAX = 2_500; // rows held after a scroll
const CREATE_MS = 100; // a created ticket's frame → its row and count
const QUIET_MS = 80; // the palette's pause
const KEY_MS = 20; // FRAME-25: one key per 20 ms inside a burst, on the page's clock
const QUIET_CEIL_MS = 300; // … and no longer than this
const SCROLL_ROW = Math.min(50_000, SEED); // the row the scroll goes to (1-based)
const ROW_PITCH = 28; // board-rows.rhx's `data-row-height`: both boards scroll at one pitch

// FRAME-14: the two boards. `tag` ends each test's name; `epics` are the
// shortcodes the board's tests make (first in the store's order, apart per
// board, so the second run finds its own); `module` is what a section
// fetches for the board's rows from another version.
const BOARDS = {
  rhx: {
    name: "rhx", tag: "", route: "", filterRoute: "#/f/", ticketRoute: "#/ticket/",
    layout: "board-layout", top: "board-layout", nav: "nav-tree", main: "board-rows", palette: "jump-palette", page: "ticket-page",
    sections: [["nav", "nav-tree"], ["main", "board-rows"], ["palette", "jump-palette"]],
    module: "board-rows.json", epics: ["AAAA", "AAAB"],
  },
  sv: {
    name: "sv", tag: "_sv", route: "#/sv/", filterRoute: "#/sv/f/", ticketRoute: "#/sv/ticket/",
    layout: "board-frame", top: "board-layout-sv", nav: "nav-tree-sv", main: "board-rows-sv", palette: "jump-palette-sv", page: "ticket-page-sv",
    sections: [["top", "board-layout-sv"], ["nav", "nav-tree-sv"], ["main", "board-rows-sv"], ["palette", "jump-palette-sv"]],
    module: "board-rows-sv.js", epics: ["AAAC", "AAAD"],
    siblings: ["nav-flat-sv", "nav-children-sv", "ticket-chip-sv"], // FRAME-25: imported by their parents, components in their own right
  },
};
const BOARD_NAMES = opt("--board", "rhx,sv").split(",");
for (const b of BOARD_NAMES) if (!BOARDS[b]) throw new Error(`--board: ${b} is not rhx or sv`);

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

const digits = (text) => Number(String(text ?? "").replace(/[^0-9]/g, ""));
const trimmed = (text) => String(text ?? "").trim();

/** The extents as the board reads them: each shortcode's tickets are its
 *  sequences 1..last_seq but its holes, in shortcode order. */
function rowsOf(extents) {
  const parts = extents.map((e) => ({ epic: e.epic, holes: new Set(e.holes ?? []), n: e.last_seq - (e.holes?.length ?? 0) }));
  const total = parts.reduce((a, p) => a + p.n, 0);
  /** Row k (1-based) → its id, by arithmetic. */
  const idAt = (k) => {
    for (const p of parts) {
      if (k > p.n) {
        k -= p.n;
        continue;
      }
      let seq = 0;
      while (k > 0) if (!p.holes.has(++seq)) k--;
      return `${p.epic}-${seq}`;
    }
    return null;
  };
  return { total, idAt };
}

// ── the tests ────────────────────────────────────────────────────────────
const results = [];
const pages = [];
/** FRAME-25: bursts of keys typed inside the page, so the page's clock
 *  times them, not Node's: each burst's characters appended to the field
 *  `sel` one per `KEY_MS` from an in-page timer (its value set, an `input`
 *  dispatched), `gap` ms between bursts; each key's time and the field's
 *  value after it kept in `__pages.keys` (and `__pages.lastKey`). Answers
 *  the bursts as the clock made them — keys split wherever a gap reached
 *  `QUIET_MS` — each `{ value, last }` (the query a correct debounce asks,
 *  and its last key's time), and whether they are the bursts asked for: a
 *  key late by a loaded machine breaks the test's precondition, not the
 *  board. */
async function typeBursts(page, sel, bursts, gap) {
  const keys = await page.evaluate(({ sel, bursts, gap, keyMs }) => new Promise((done) => {
    const P = window.__pages;
    const el = document.querySelector(sel);
    P.keys = [];
    const plan = [];
    for (const [i, b] of bursts.entries()) for (const [j, ch] of [...b].entries()) plan.push({ ch, wait: j === 0 && i > 0 ? gap : keyMs });
    const step = (k) => {
      if (k === plan.length) return done(P.keys);
      setTimeout(() => {
        el.value += plan[k].ch;
        el.dispatchEvent(new Event("input", { bubbles: true }));
        P.lastKey = performance.now();
        P.keys.push({ t: P.lastKey, value: el.value });
        step(k + 1);
      }, k === 0 ? 0 : plan[k].wait);
    };
    step(0);
  }), { sel, bursts, gap, keyMs: KEY_MS });
  const made = [];
  for (const [i, k] of keys.entries()) {
    if (i === 0 || k.t - keys[i - 1].t >= QUIET_MS) made.push({ value: k.value, last: k.t });
    else Object.assign(made.at(-1), { value: k.value, last: k.t });
  }
  let typed = "";
  const asked = bursts.map((b) => (typed += b));
  return { made, kept: JSON.stringify(made.map((m) => m.value)) === JSON.stringify(asked), keys };
}

async function test(name, body) {
  if (ONLY && !ONLY.includes(name)) return;
  if (GROUP && !inGroup) return;
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
  const work = workDir("ui8-pages");
  console.log(`work: ${work} · seed ${SEED} · ${PROFILE} · target ${TARGET}`);

  if (!flag("--no-build")) build(PROFILE);
  const v1 = bundle(work, "v1");
  const v2 = bundle(work, "v2", {
    engine: v1.engine,
    edit: (ui) => {
      for (const f of ["board-rows.rhx", "board-rows-sv.svelte"]) {
        const at = path.join(ui, "src", f);
        if (!fs.existsSync(at)) continue;
        // FRAME-30: and its count in another token — a sheet that moves with its version.
        const src = fs.readFileSync(at, "utf8");
        if (!src.includes(".board-count { color: var(--ink);")) throw new Error(`${f}: its sheet's board-count rule moved; the v2 edit needs it`);
        fs.writeFileSync(at, src.replace(".board-count { color: var(--ink);", ".board-count { color: var(--accent);"));
        fs.appendFileSync(at, '<p class="board-newer">the newer board</p>\n');
      }
    },
  });
  console.log(`v1 ${v1.version}\nv2 ${v2.version}`);

  const adminKey = path.join(work, "admin.key");
  const signerKey = path.join(work, "signer.key");
  const admin = makeKey(adminKey);
  const signerHex = makeKey(signerKey).replace(/^key:/, "");
  const server = new Throwaway(work, PROFILE);
  const t0 = Date.now();
  server.keep = flag("--keep");
  const session = path.join(work, "session");
  let browser;
  // FRAME-14: from the start on, a failure anywhere stops the server.
  try {
    await server.start(admin, signerHex, SEED);
    console.log(`server up, ${SEED} seeded, in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
    await writeSession(`ws://127.0.0.1:${server.ws}`, adminKey, session, "owner");
    const token = readToken(session);
    const sy = (...a) => synapse(server, session, ...a);
    const extentsNow = () => sy("tickets", "nav", "extents").filter((e) => e.type === "extent");
    const groupsNow = () => sy("tickets", "nav", "groups").filter((e) => e.type === "group").map((e) => e.group);
    const summaries = (...a) => sy("tickets", "list", "--summary", "true", ...a).filter((e) => e.type === "summary").map((e) => e.summary);

    for (const b of [v1, v2]) {
      const answer = push(server, session, signerKey, b);
      if (answer?.type !== "pushed" || answer.version !== b.version) throw new Error(`ui push of ${path.basename(b.tar)}: ${JSON.stringify(answer)}`);
    }

    const { chromium } = await playwright();
    browser = await chromium.launch({ headless: !flag("--headed") });

    /** A page (signed in unless `signedIn` is false), its console kept; the
     *  engine's frames counted from INIT. */
    async function openPage(route = "", { signedIn = true } = {}) {
      const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
      if (signedIn) await context.addCookies(sessionCookies(server.origin, token));
      await context.addInitScript(INIT);
      const page = await context.newPage();
      const seen = { sockets: 0, console: [], requests: [] };
      page.on("websocket", () => seen.sockets++);
      page.on("request", (r) => seen.requests.push(r.url()));
      page.on("console", (m) => seen.console.push(`${m.type()}: ${m.text()}`));
      page.on("pageerror", (e) => seen.console.push(`pageerror: ${e.message}`));
      pages.push(seen);
      await page.goto(`${server.origin}/app/${route}`);
      return { context, page, seen };
    }
    const marked = (page) => page.evaluate(() => ((window.__pages_marker = "kept"), performance.timeOrigin));
    async function sameDocument(page, origin, step) {
      const now = await page.evaluate(() => ({ marker: window.__pages_marker, origin: performance.timeOrigin }));
      assert(now.marker === "kept" && now.origin === origin, `no reload: ${step}`);
    }
    const boardLists = (calls) => calls.filter((c) => c.method === "tickets.list" && c.params.parent == null);
    /** A board's section: `[data-layout=<its layout>][data-slot=<slot>]`. */
    const section = (page, B, slot) => page.locator(`[data-layout=${B.layout}][data-slot=${slot}]`);
    const main = (page, B) => section(page, B, "main");
    const nav = (page, B) => section(page, B, "nav");
    const palette = (page, B) => section(page, B, "palette");
    const rowIds = (loc) => loc.evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));
    const attrs = (loc, name) => loc.evaluateAll((els, n) => els.map((e) => e.getAttribute(n)), name);
    const count = async (page, B) => digits(await main(page, B).locator(".board-count").textContent({ timeout: 1000 }));
    const boardUp = (page, B) => eventually(`the ${B.name} board shows its first rows' titles`, async () => trimmed(await main(page, B).locator(".board-row[data-id] [data-field=title]").first().textContent({ timeout: 1000 })).length > 0);

    /** One of the board's tests, on board `B`: named `name` on the rhx board
     *  and `name_sv` on the Svelte one; failed at once, saying why, when the
     *  board's own first test found it not drawn. */
    const onBoard = (B, name, body) =>
      test(`${name}${B.tag}`, async () => {
        if (B.down) throw new Error(`the ${B.name} board is not drawn (the_bundle_draws_the_board${B.tag}): ${B.down}`);
        await body();
      });

    /** Every test of the board, on board `B`. */
    async function boardTests(B) {
      // The board's first test: the bundle is what the page draws — the
      // layout's sections, each with its default component (a descendant of
      // its section), and the ticket route.
      await test(`the_bundle_draws_the_board${B.tag}`, async () => {
        const { page, context, seen } = await openPage(B.route);
        try {
          for (const [slot, component] of B.sections) {
            await eventually(`${B.route || "#/"}: the ${slot} section of ${B.layout} holds ${component}`, async () => (await section(page, B, slot).locator(component).count()) === 1).catch((e) => {
              throw new Error(`${e.message}\n${pages.at(-1).console.slice(-10).join("\n")}`);
            });
          }
          // FRAME-26: the bundle's one svelte/1 runtime, asked once on either
          // board — the entrypoint's modulepreload and the modules' import
          // share one fetch — and on the rhx board no svelte/1 module asked.
          const asked = seen.requests.map((u) => new URL(u).pathname).filter((p) => /^\/app\/[0-9a-f]{64}\/components\/[^/]+\.js$/.test(p));
          const runtimes = asked.filter((p) => p.endsWith("/svelte-runtime.js"));
          assert(runtimes.length === 1, `${B.name} board: the svelte/1 runtime asked once, not ${runtimes.length}× (${asked.join(", ")})`);
          if (!B.tag) assert(asked.length === 1, `the rhx board asks no svelte/1 module: ${asked.join(", ")}`);
          else assert(asked.length > 1, `the Svelte board's modules beside the runtime: ${asked.join(", ")}`);
          console.log(`     ${B.name} board: ${asked.map((p) => path.basename(p)).join(", ")}`);
          await page.evaluate((r) => (location.hash = r), `${B.ticketRoute}NONE-1`);
          await eventually(`${B.ticketRoute}:id draws ${B.page}`, async () => (await page.locator(B.page).count()) === 1);
          // FRAME-25: the Svelte board's siblings are components in their own
          // right — svelte/1 entries of the live manifest, each its own module
          // served — so a right-click can put one in a section.
          for (const name of B.siblings ?? []) {
            const entry = await page.evaluate(async (name) => {
              const at = performance.getEntriesByType("resource").map((e) => new URL(e.name).pathname).find((p) => /^\/app\/[^/]+\/manifest\.json$/.test(p));
              if (!at) return { error: "no manifest.json fetched" };
              const m = await (await fetch(at)).json();
              const e = m.components?.[name];
              const js = e ? await fetch(at.replace(/manifest\.json$/, `components/${name}.js`)) : null;
              return { profile: e?.profile, module: e?.module ?? null, served: js?.ok ?? false };
            }, name);
            assert(entry.profile === "svelte/1" && /^[0-9a-f]{64}$/.test(entry.module ?? "") && entry.served, `${name} is a svelte/1 component of the live manifest with its own module, served: ${JSON.stringify(entry)}`);
          }
        } catch (e) {
          B.down = e.message.split("\n")[0];
          throw e;
        } finally {
          await context.close();
        }
        pages.pop();
      });

      // FRAME-30: the board wears its components' sheets on the theme's
      // tokens: theme.css from the entrypoint, every component's sheet
      // adopted (no <link> but the theme's), the rows drawn by their own
      // rules, light and dark both from theme.css — each screenshotted.
      await onBoard(B, "the_board_wears_its_sheets_on_the_theme", async () => {
        const { page, seen } = await openPage(B.route);
        await boardUp(page, B);
        const paths = seen.requests.map((u) => new URL(u).pathname);
        assert(paths.some((p) => /^\/app\/[0-9a-f]{64}\/theme\.css$/.test(p)), `theme.css loaded: ${paths.filter((p) => p.endsWith(".css")).join(", ")}`);
        assert(!paths.some((p) => /\/components\/[^/]+\.css$/.test(p)), `no component's sheet fetched (adopted from components.json and the modules): ${paths.filter((p) => p.endsWith(".css")).join(", ")}`);
        const looks = (theme) => page.evaluate(([sel, theme]) => {
          if (theme) document.documentElement.dataset.theme = theme;
          const m = document.querySelector(sel);
          const css = (q, k) => { const e = m.querySelector(q); return e ? getComputedStyle(e)[k] : null; };
          return {
            links: [...document.querySelectorAll("link[rel=stylesheet]")].map((l) => new URL(l.href).pathname.replace(/^\/app\/[0-9a-f]+\//, "")),
            adopted: document.adoptedStyleSheets.length,
            head: css(".board-head", "backgroundColor"),
            row: css(".board-row", "height"),
            grid: css(".board-row", "display"),
            id: css(".row-id", "color"),
            page: getComputedStyle(document.body).backgroundColor,
            status: [...m.querySelectorAll(".board-row[data-status=Ready] .row-status, .board-row[data-status=Pending] .row-status")].slice(0, 1).map((e) => getComputedStyle(e).backgroundColor)[0] ?? null,
          };
        }, [`[data-layout=${B.layout}][data-slot=main] ${B.main}`, theme]);
        const light = await looks("light");
        assert(JSON.stringify(light.links) === '["theme.css"]', `one <link>, the theme's: ${JSON.stringify(light.links)}`);
        assert(light.adopted >= 4, `the board's components' sheets adopted: ${light.adopted}`);
        assert(light.grid === "grid" && light.row === "28px", `a row drawn by board-rows' own sheet (grid, the pitch): ${JSON.stringify(light)}`);
        assert(light.page === "rgb(251, 250, 247)" && light.head === "rgb(255, 255, 255)" && light.id === "rgb(59, 91, 219)", `light: --bg, --panel, --accent from theme.css: ${JSON.stringify(light)}`);
        const shots = process.env.PAGES_SHOTS;
        if (shots) await page.screenshot({ path: path.join(shots, `board-${B.name}-light.png`) });
        const dark = await looks("dark");
        assert(dark.page === "rgb(20, 20, 18)" && dark.head === "rgb(28, 27, 25)" && dark.id === "rgb(142, 166, 255)", `dark: --bg, --panel, --accent from theme.css: ${JSON.stringify(dark)}`);
        assert(dark.status !== light.status || dark.status === null, `a status chip's ground moves with the theme: ${light.status} → ${dark.status}`);
        if (shots) await page.screenshot({ path: path.join(shots, `board-${B.name}-dark.png`) });
        console.log(`     ${B.name}: ${light.adopted} sheets adopted; light ${light.head}/${light.status}, dark ${dark.head}/${dark.status}`);
      });

      await onBoard(B, "the_board_asks_extents_then_one_page", async () => {
        const { total } = rowsOf(extentsNow());
        const expected = summaries("--limit", String(PAGE));
        const { page } = await openPage(B.route);
        await boardUp(page, B);
        await sleep(1_500); // what else the page asks, it asks now
        const calls = await sentCalls(page);
        const ext = calls.filter((c) => c.method === "tickets.nav.extents");
        assert(ext.length === 1, `nav.extents asked once (${ext.length})`);
        const lists = boardLists(calls);
        assert(lists.length === 1, `one list for the window (${lists.length}: ${JSON.stringify(lists.map((l) => l.params))})`);
        // FRAME-17: the first page depends on no extents (after: null) — on
        // the rhx board it is the first snapshot asked (its changes stream
        // just before it), right after the boot.
        if (!B.tag) {
          const firstRead = calls.find((c) => !c.method.endsWith(".watch"));
          assert(firstRead?.method === "tickets.list", `the board's list is the first call (${calls.slice(0, 4).map((c) => c.method).join(", ")})`);
        }
        assert(lists[0].params.limit === PAGE && lists[0].params.summary === true, `the page is limit ${PAGE}, summary: ${JSON.stringify(lists[0].params)}`);
        assert(lists[0].params.after == null, "the first page has no after");
        const rows = main(page, B).locator(".board-row[data-id]");
        const shown = await rowIds(rows);
        const want = expected.slice(0, shown.length);
        assert(JSON.stringify(shown) === JSON.stringify(want.map((t) => t.id)), `the rows are list's first, in order: ${shown.slice(0, 5).join(" ")} … vs ${want.slice(0, 5).map((t) => t.id).join(" ")} …`);
        const titles = await rows.locator("[data-field=title]").evaluateAll((els) => els.map((e) => e.textContent.trim()));
        assert(JSON.stringify(titles) === JSON.stringify(want.map((t) => t.title.trim())), "each row shows its title from list");
        assert((await count(page, B)) === total, `the count is the extents' sum, ${total}`);
        const firstRow = await page.evaluate(() => window.__pages.firstRow);
        assert(firstRow != null, "the first row was stamped once its title showed");
        // FRAME-17: the host marks its steps on the page's clock, in order (CONTRACT.md's host
        // chapter); a mark set twice keeps its first. On the rhx board the route's snapshots
        // are asked before any section mounts; on the Svelte board a `live` is module code,
        // asked when its module mounts — first-call is the frame's first section's (`top`,
        // board-layout-sv's nav.counts), before the main section.
        const marks = await page.evaluate(() => {
          const m = {};
          for (const e of performance.getEntriesByType("mark")) if (e.name.startsWith("plexus-ui:")) m[e.name] ??= e.startTime;
          return m;
        });
        const shownMarks = Object.entries(marks).map(([n, t]) => `${n} ${t.toFixed(0)}`).join(", ") || "none";
        const slots = B.sections.map(([slot]) => `section-mounted:${slot}`);
        const lacking = ["first-call", "components-parsed", ...slots, "layout-mounted", "first-answer", "first-row"].filter((n) => marks[`plexus-ui:${n}`] == null);
        assert(!lacking.length, `the board's page carries the host's marks; it lacks ${lacking.join(", ")} (it has: ${shownMarks})`);
        const m = (n) => marks[`plexus-ui:${n}`];
        assert(m("components-parsed") <= m("first-call") && m("first-call") < m("section-mounted:main") && m("section-mounted:main") <= m("layout-mounted"), `components parsed, the first call, the main section mounted, then the layout: ${shownMarks}`);
        if (!B.tag) assert(slots.every((n) => m("first-call") < m(n)), `the rhx board's first call before any section mounts: ${shownMarks}`);
        assert(m("first-call") < m("first-answer") && m("first-answer") <= m("first-row") && m("first-row") <= firstRow, `the first call, its answer, its rows, no later than the board's first titled row (${firstRow.toFixed(0)}): ${shownMarks}`);
        console.log(`     first row (titled): ${firstRow.toFixed(0)} ms from timeOrigin · ${shown.length} rows drawn · ${total} tickets`);
      });

      await onBoard(B, "a_scroll_asks_only_the_page_it_needs", async () => {
        const { idAt } = rowsOf(extentsNow());
        const before = idAt(SCROLL_ROW - 1);
        const target = idAt(SCROLL_ROW);
        const next = summaries("--after", before, "--limit", "1")[0]?.id;
        assert(next === target, `row ${SCROLL_ROW} by arithmetic is the one after row ${SCROLL_ROW - 1} (${target} vs list's ${next})`);

        const { page } = await openPage(B.route);
        await boardUp(page, B);
        const origin = await marked(page);
        await sleep(500);
        const asked = boardLists(await sentCalls(page));
        assert(asked.length === 1, `one board list before the scroll (${asked.length})`);
        const scroller = main(page, B).locator(".board-scroll");
        const pitch = Number(await scroller.getAttribute("data-row-height", { timeout: 1000 }));
        assert(pitch === ROW_PITCH, `the board says its row pitch, the rhx board's ${ROW_PITCH} px (${pitch})`);
        await scroller.evaluate((el, top) => (el.scrollTop = top), (SCROLL_ROW - 1) * pitch);
        const row = main(page, B).locator(`.board-row[data-id="${target}"]`);
        await eventually(`row ${SCROLL_ROW} (${target}) is drawn`, async () => (await row.count()) === 1, 5_000);
        await sleep(500);
        const lists = boardLists(await sentCalls(page)).slice(asked.length);
        assert(lists.length === 1, `one list for the scroll (${lists.length}: ${JSON.stringify(lists.map((l) => l.params))})`);
        assert(lists[0].params.after === before, `the list starts after row ${SCROLL_ROW - 1}, ${before}: ${JSON.stringify(lists[0].params)}`);
        const all = boardLists(await sentCalls(page));
        assert(all.every((l) => l.params.limit != null && l.params.limit <= PAGE), `every board list asks at most ${PAGE}: ${JSON.stringify(all.map((l) => l.params.limit))}`);
        const [rowTop, scrollTop] = await Promise.all([row.evaluate((e) => e.getBoundingClientRect().top), scroller.evaluate((e) => e.getBoundingClientRect().top)]);
        assert(Math.abs(rowTop - scrollTop) <= pitch, `row ${SCROLL_ROW} is at the top of the board (${rowTop.toFixed(0)} vs ${scrollTop.toFixed(0)}, pitch ${pitch})`);
        const held = await main(page, B).locator(".board-row[data-id]").count();
        assert(held < HELD_MAX, `the rows held stay under ${HELD_MAX} (${held})`);
        await sameDocument(page, origin, "after the scroll");
        console.log(`     ${held} rows held after the scroll to row ${SCROLL_ROW}`);
      });

      await onBoard(B, "a_created_ticket_appears_without_a_reload", async () => {
        // A shortcode near the front of the store's order (AAAA, or AAAC after the rhx
        // board's AAAA and AAAB), so its tickets are among the board's first rows.
        const [root] = sy("tickets", "create", "--epic", B.epics[0], "--kind", "scope", "--title", "the board's first shortcode");
        assert(root?.type === "created", `the scope was made: ${JSON.stringify(root)}`);
        const total = rowsOf(extentsNow()).total;
        const { page, seen } = await openPage(B.route);
        await eventually("the board shows the new shortcode's root", async () => (await main(page, B).locator(`.board-row[data-id="${root.ticket.id}"]`).count()) === 1);
        assert((await count(page, B)) === total, `the count before is ${total}`);
        const origin = await marked(page);

        const [created] = sy("tickets", "create", "--epic", B.epics[0], "--parent", root.ticket.id, "--title", "made by the pages test");
        const id = created?.ticket?.id;
        assert(id, `tickets create answered a ticket: ${JSON.stringify(created)}`);
        await eventually(`the created ticket ${id} is on the board`, async () => (await main(page, B).locator(`.board-row[data-id="${id}"]`).count()) === 1, 5_000);
        await eventually(`the count reads ${total + 1}`, async () => (await count(page, B)) === total + 1, 2_000);
        const t = await page.evaluate((id) => ({ frame: window.__pages.created[id], row: window.__pages.boardRows[id], counts: window.__pages.counts }), id);
        const counted = t.counts.find((c) => digits(c.text) === total + 1)?.t;
        assert(t.frame != null && t.row != null && counted != null, `the frame, the row and the count were stamped: ${JSON.stringify({ frame: t.frame, row: t.row, counted })}`);
        console.log(`     created frame → row ${(t.row - t.frame).toFixed(1)} ms, → count ${(counted - t.frame).toFixed(1)} ms (budget ${CREATE_MS} ms)`);
        assert(t.row - t.frame < CREATE_MS, `the row within ${CREATE_MS} ms of its frame (${(t.row - t.frame).toFixed(1)} ms)`);
        assert(counted - t.frame < CREATE_MS, `the count within ${CREATE_MS} ms of the frame (${(counted - t.frame).toFixed(1)} ms)`);
        await sameDocument(page, origin, "after the create");
        assert(seen.sockets === 1, `one socket (${seen.sockets})`);
      });

      await onBoard(B, "a_status_change_patches_the_row_in_place", async () => {
        const { page } = await openPage(B.route);
        await boardUp(page, B);
        const origin = await marked(page);
        const row = main(page, B).locator(".board-row[data-id]").nth(3);
        const id = await row.getAttribute("data-id");
        const was = trimmed(await row.locator("[data-field=status]").textContent({ timeout: 1000 }));
        const to = was === "Ready" ? "Pending" : "Ready";
        await row.evaluate((e) => (e.__tagged = true));
        const asked = (await sentCalls(page)).length;
        const [changed] = sy("tickets", "set_status", "--id", id, "--status", to);
        assert(changed?.type === "status_changed", `set_status answered: ${JSON.stringify(changed)}`);
        const same = main(page, B).locator(`.board-row[data-id="${id}"]`);
        await eventually(`${id}'s row shows ${to}`, async () => trimmed(await same.locator("[data-field=status]").textContent({ timeout: 1000 })) === to, 3_000);
        assert(await same.evaluate((e) => e.__tagged === true), "the same element, patched in place");
        await sleep(300);
        const lists = (await sentCalls(page)).slice(asked).filter((c) => c.method === "tickets.list");
        assert(lists.length === 0, `no list for a status change (${lists.length})`);
        await sameDocument(page, origin, "after the status change");
      });

      await onBoard(B, "a_new_shortcode_appears_on_the_open_board", async () => {
        const total = rowsOf(extentsNow()).total;
        const { page } = await openPage(B.route);
        await boardUp(page, B);
        assert((await count(page, B)) === total, `the count before is ${total}`);
        const origin = await marked(page);
        const [made] = sy("tickets", "create", "--epic", B.epics[1], "--kind", "scope", "--title", "a shortcode born while the board is open");
        const id = made?.ticket?.id;
        assert(id, `the scope was made: ${JSON.stringify(made)}`);
        await eventually(`${id}'s row appears`, async () => (await main(page, B).locator(`.board-row[data-id="${id}"]`).count()) === 1, 3_000);
        await eventually(`the count reads ${total + 1}`, async () => (await count(page, B)) === total + 1, 2_000);
        await sameDocument(page, origin, "after the new shortcode");
      });

      await onBoard(B, "the_nav_opens_a_shortcode_to_one_page", async () => {
        const groups = groupsNow();
        const { page } = await openPage(B.route);
        const tree = nav(page, B).locator("[data-nav-group]");
        await eventually("Tree draws a row per shortcode", async () => (await tree.count()) === groups.length);
        for (const g of groups) {
          const row = nav(page, B).locator(`[data-nav-group="${g.shortcode}"]`);
          const done = digits(await row.locator("[data-rollup-done]").first().textContent({ timeout: 1000 }));
          const all = digits(await row.locator("[data-rollup-total]").first().textContent({ timeout: 1000 }));
          assert(done === g.done && all === g.total, `${g.shortcode}'s roll-up is groups' ${g.done} of ${g.total} (shows ${done} of ${all})`);
        }
        const opened = groups.filter((g) => g.root).sort((a, b) => b.total - a.total).slice(0, 2);
        for (const g of opened) {
          const asked = (await sentCalls(page)).length;
          await nav(page, B).locator(`[data-nav-group="${g.shortcode}"] [data-nav-toggle]`).first().click({ timeout: 2000 });
          const expected = summaries("--parent", g.root, "--limit", String(PAGE)).map((t) => t.id);
          const rows = nav(page, B).locator(`[data-nav-group="${g.shortcode}"] [data-nav-row]`);
          await eventually(`${g.shortcode} opens to its root's children`, async () => (await rows.count()) > 0, 5_000);
          await sleep(300);
          const shown = await attrs(rows, "data-nav-row");
          assert(JSON.stringify(shown) === JSON.stringify(expected.slice(0, shown.length)), `${g.shortcode}'s rows are list { parent: ${g.root} }'s, in order`);
          const slice = (await sentCalls(page)).slice(asked);
          assert(slice.length === 1 && slice[0].method === "tickets.list" && slice[0].params.parent === g.root, `the open is exactly one list { parent: ${g.root} }: ${JSON.stringify(slice.map((c) => [c.method, c.params]))}`);
          assert(slice[0].params.limit != null && slice[0].params.limit <= PAGE, `one page (limit ${slice[0].params.limit})`);
        }
      });

      await onBoard(B, "the_nav_roll_ups_follow_group_changed", async () => {
        const g = groupsNow().filter((x) => x.root && !x.shortcode.startsWith("AAA"))[0];
        const { page } = await openPage(B.route);
        const total = nav(page, B).locator(`[data-nav-group="${g.shortcode}"] [data-rollup-total]`).first();
        await eventually(`${g.shortcode}'s roll-up is drawn`, async () => digits(await total.textContent({ timeout: 1000 })) === g.total);
        await nav(page, B).locator(`[data-nav-group="${g.shortcode}"] [data-nav-toggle]`).first().click({ timeout: 2000 });
        await eventually(`${g.shortcode} is open`, async () => (await nav(page, B).locator(`[data-nav-group="${g.shortcode}"] [data-nav-row]`).count()) > 0, 5_000);
        const asked = (await sentCalls(page)).length;
        const [made] = sy("tickets", "create", "--epic", g.epic, "--parent", g.root, "--title", "a child the roll-up counts");
        assert(made?.ticket?.id, `made: ${JSON.stringify(made)}`);
        await eventually(`the roll-up total reads ${g.total + 1}`, async () => digits(await total.textContent({ timeout: 1000 })) === g.total + 1, 3_000);
        const calls = (await sentCalls(page)).slice(asked);
        assert(!calls.some((c) => c.method === "tickets.nav.groups"), "no groups call: group_changed did it");
      });

      await onBoard(B, "the_nav_switches_tree_and_flat", async () => {
        const expected = summaries("--limit", String(PAGE)).map((t) => t.id);
        const { page } = await openPage(B.route);
        await eventually("Tree is drawn", async () => (await nav(page, B).locator("[data-nav-group]").count()) > 0);
        const asked = (await sentCalls(page)).length;
        await nav(page, B).locator("[data-nav-mode=flat]").click({ timeout: 2000 });
        const flat = nav(page, B).locator("[data-nav-flat-row]");
        await eventually("Flat draws its rows", async () => (await flat.count()) > 0, 5_000);
        await sleep(300);
        const lists = (await sentCalls(page)).slice(asked).filter((c) => c.method === "tickets.list");
        assert(lists.length === 1, `Flat is one list (${lists.length}: ${JSON.stringify(lists.map((l) => l.params))})`);
        const p = lists[0].params;
        assert(p.summary === true && p.limit === PAGE && p.parent == null && p.after == null && (p.filter ?? null) === null, `Flat's list is { filter, summary, limit: ${PAGE} }: ${JSON.stringify(p)}`);
        assert((await nav(page, B).locator("[data-nav-group]").count()) === 0, "Flat draws no shortcode rows");
        const shown = await attrs(flat, "data-nav-flat-row");
        assert(JSON.stringify(shown) === JSON.stringify(expected.slice(0, shown.length)) && shown.length === Math.min(PAGE, expected.length), `Flat's rows are the store's order, one page (${shown.length})`);
        await nav(page, B).locator("[data-nav-mode=tree]").click({ timeout: 2000 });
        await eventually("Tree again", async () => (await nav(page, B).locator("[data-nav-group]").count()) > 0);
      });

      await onBoard(B, "a_ticket_page_draws_the_body_without_scripts", async () => {
        const groups = groupsNow().filter((g) => g.root && !g.shortcode.startsWith("AAA"));
        const parent = sy("tickets", "get", "--id", groups[0].root).find((e) => e.type === "ticket").ticket;
        const blocker = summaries("--parent", parent.id, "--limit", "1")[0];
        const body = path.join(server.work, "body.md");
        fs.writeFileSync(
          body,
          [
            "A paragraph with **weight** in it.",
            "",
            "<script>window.__ranScript = true</script>",
            "<style>body { background: rgb(1, 2, 3) !important; }</style>",
            '<img src="x" onerror="window.__ranHandler = true">',
            "",
            "[a link](javascript:window.__ranLink=true)",
            "",
            '<iframe srcdoc="<script>parent.__ranFrame=1</script>"></iframe>',
            "",
            '<svg onload="window.__ranSvg=1"><circle r="4"/></svg>',
            "",
            '<p><a href="data:text/html,<script>alert(1)</script>">a data link</a> <a href="JaVaScRiPt:window.__ranMixed=1">a mixed-case link</a> <a href="vbscript:msgbox(1)">a vb link</a></p>',
            "",
            '<form action="javascript:window.__ranForm=1"><button>send</button></form>',
            "",
            "- the first item",
            "- the second item",
            "",
            "the last line",
            "",
          ].join("\n"),
        );
        const [made] = sy("tickets", "create", "--epic", parent.epic, "--parent", parent.id, "--blocked-by", blocker.id, "--title", "a body with a script", "--body", `@${body}`);
        const t = made?.ticket;
        assert(t?.id, `the ticket was made: ${JSON.stringify(made)}`);
        const [child] = sy("tickets", "create", "--epic", parent.epic, "--parent", t.id, "--title", "a child of the page");
        assert(child?.ticket?.id, `the child was made: ${JSON.stringify(child)}`);

        const { page } = await openPage(`${B.ticketRoute}${t.id}`);
        const tp = page.locator(B.page);
        await eventually("the ticket page draws the title", async () => trimmed(await tp.locator("[data-ticket-title]").textContent({ timeout: 1000 })) === t.title.trim());
        await eventually("the child is drawn", async () => (await tp.locator(`[data-ticket-child="${child.ticket.id}"]`).count()) === 1, 3_000);
        await sleep(500);
        assert(trimmed(await tp.locator("[data-ticket-status]").textContent({ timeout: 1000 })) === t.status, `the status is ${t.status}`);
        assert(trimmed(await tp.locator("[data-ticket-kind]").textContent({ timeout: 1000 })) === t.kind, `the kind is ${t.kind}`);
        const kids = (await sentCalls(page)).filter((c) => c.method === "tickets.list");
        assert(kids.length === 1 && kids[0].params.parent === t.id, `one list { parent: ${t.id} } (${JSON.stringify(kids.map((k) => k.params))})`);
        const bodyEl = tp.locator("[data-ticket-body]");
        const text = await bodyEl.textContent({ timeout: 1000 });
        for (const want of ["A paragraph with weight in it.", "the first item", "the last line", "a link"]) assert(text.includes(want), `the body says "${want}": ${JSON.stringify(text)}`);
        for (const not of ["__ranScript", "background", "__ranFrame", "__ranSvg"]) assert(!text.includes(not), `the body does not show the dropped "${not}"`);
        assert(trimmed(await bodyEl.locator("strong").textContent({ timeout: 1000 })) === "weight", "the markdown is drawn (**weight** is strong)");
        assert((await bodyEl.locator("li").count()) === 2, "the list is drawn");
        await bodyEl.locator("a").first().click({ timeout: 1000 }).catch(() => {});
        // A button the body kept, pressed (a form posting to javascript: would run now).
        await page.evaluate((sel) => document.querySelectorAll(`${sel} [data-ticket-body] button`).forEach((b) => b.click()), B.page);
        await sleep(300);
        const ran = await page.evaluate((sel) => {
          const b = document.querySelector(`${sel} [data-ticket-body]`);
          const all = [...(b?.querySelectorAll("*") ?? [])];
          return {
            script: window.__ranScript === true,
            handler: window.__ranHandler === true,
            link: window.__ranLink === true,
            frame: window.__ranFrame === 1,
            svg: window.__ranSvg === 1,
            mixed: window.__ranMixed === 1,
            form: window.__ranForm === 1,
            bg: getComputedStyle(document.body).backgroundColor,
            banned: b?.querySelectorAll("script, style, iframe, svg, form, object, embed").length ?? -1,
            onAttrs: all.flatMap((e) => [...e.attributes].filter((a) => a.name.toLowerCase().startsWith("on")).map((a) => `${e.tagName}.${a.name}`)),
            jsUrls: all.flatMap((e) => [...e.attributes].filter((a) => /^\s*(javascript|vbscript|data):/i.test(a.value)).map((a) => `${e.tagName}.${a.name}`)),
          };
        }, B.page);
        assert(!ran.script && !ran.handler && !ran.link && !ran.frame && !ran.svg && !ran.mixed && !ran.form, `nothing in the body ran: ${JSON.stringify(ran)}`);
        assert(ran.bg !== "rgb(1, 2, 3)", `the body's style did not apply (${ran.bg})`);
        assert(ran.banned === 0, `no <script>, <style>, <iframe>, <svg>, <form>, <object> or <embed> in the body (${ran.banned})`);
        assert(ran.onAttrs.length === 0 && ran.jsUrls.length === 0, `no on* attribute, no javascript:, vbscript: or data: URL: ${JSON.stringify(ran)}`);
        for (const [chip, other] of [["parent", parent], ["blocker", blocker]]) {
          const c = tp.locator(`[data-chip=${chip}][data-chip-id="${other.id}"]`);
          assert((await c.count()) === 1, `a ${chip} chip for ${other.id}`);
          assert((await c.getAttribute("href")) === `${B.ticketRoute}${other.id}`, `the ${chip} chip stays on the ${B.name} board (${await c.getAttribute("href")})`);
          await eventually(`the ${chip} chip resolves to "${other.title}"`, async () => trimmed(await c.locator("[data-chip-title]").textContent({ timeout: 1000 })) === other.title.trim(), 3_000);
        }

        // set_status from the menu.
        const to = t.status === "Ready" ? "Pending" : "Ready";
        await tp.locator("select[data-status-menu]").selectOption(to, { timeout: 2000 });
        await eventually(`the server has ${t.id} at ${to}`, async () => sy("tickets", "get", "--id", t.id).find((e) => e.type === "ticket")?.ticket?.status === to, 3_000);
        await eventually(`the page shows ${to}`, async () => trimmed(await tp.locator("[data-ticket-status]").textContent({ timeout: 1000 })) === to, 3_000);

        // The blocker's page, by the hash: it unlocks the ticket.
        const origin = await marked(page);
        await page.evaluate((r) => (location.hash = r), `${B.ticketRoute}${blocker.id}`);
        await eventually("the blocker's page draws", async () => trimmed(await tp.locator("[data-ticket-title]").textContent({ timeout: 1000 })) === blocker.title.trim());
        const u = tp.locator(`[data-chip=unlocks][data-chip-id="${t.id}"]`);
        await eventually(`an unlocks chip for ${t.id}, resolved`, async () => trimmed(await u.locator("[data-chip-title]").textContent({ timeout: 1000 })) === t.title.trim(), 3_000);
        await sameDocument(page, origin, "after the hash moved");
      });

      await onBoard(B, "the_palette_finds_and_searches", async () => {
        const groups = groupsNow().filter((g) => g.root);
        const [made] = sy("tickets", "create", "--epic", groups.at(-1).epic, "--parent", groups.at(-1).root, "--title", "the map-12 lantern");
        assert(made?.ticket?.id, `the lantern was made: ${JSON.stringify(made)}`);
        const query = "map-12";
        const found = sy("tickets", "find", "--query", query).filter((e) => e.type === "summary").map((e) => e.summary.id);
        const hits = sy("tickets", "search", "--query", query).filter((e) => e.type === "hit").map((e) => e.summary.id);
        assert(found.length > 0 && hits.length > 0, `find and search answer ${query} (${found.length}, ${hits.length})`);
        // For newest-only, a query never asked before (a live already held
        // for `map-12` is answered from the engine's cache, no frame): `zap`
        // finds both of these, `zap-34` one.
        for (const title of ["the zap-34 kettle", "the zap-56 kettle"]) sy("tickets", "create", "--epic", groups.at(-1).epic, "--parent", groups.at(-1).root, "--title", title);
        const zap = sy("tickets", "find", "--query", "zap").filter((e) => e.type === "summary").map((e) => e.summary.id);
        const zap34 = sy("tickets", "find", "--query", "zap-34").filter((e) => e.type === "summary").map((e) => e.summary.id);
        assert(zap34.length > 0 && JSON.stringify(zap) !== JSON.stringify(zap34), `zap and zap-34 find different tickets (${zap.length}, ${zap34.length})`);

        const { page } = await openPage(B.route);
        await boardUp(page, B);
        await sleep(300);
        const atBoot = (await sentCalls(page)).filter((c) => c.method === "tickets.find" || c.method === "tickets.search");
        assert(atBoot.length === 0, `the palette asks nothing before a key (FRAME-16): ${JSON.stringify(atBoot.map((c) => [c.method, c.params]))}`);
        await page.locator("body").click({ position: { x: 5, y: 5 } }).catch(() => {});
        await page.keyboard.press("/");
        const input = palette(page, B).locator("input[data-palette-input]");
        await eventually("`/` focuses the palette's field", async () => input.evaluate((e) => e === document.activeElement), 2_000);

        // Typed at once: one find, after the pause and within its ceiling.
        // FRAME-25: typed in the page; a burst the clock split is typed again,
        // once, after the palette let its collections go.
        const inputSel = `[data-layout=${B.layout}][data-slot=palette] input[data-palette-input]`;
        let asked = (await sentCalls(page)).length;
        let typed = await typeBursts(page, inputSel, [query], 0);
        if (!typed.kept) {
          console.log(`     the page's clock split ${query} (${JSON.stringify(typed.made.map((m) => m.value))}): typed again`);
          await input.fill("");
          await sleep(6_000);
          asked = (await sentCalls(page)).length;
          typed = await typeBursts(page, inputSel, [query], 0);
          assert(typed.kept, `the page typed ${query} as one burst twice running (${JSON.stringify(typed.keys.map((k) => Math.round(k.t)))}): the machine is too loaded for this case`);
        }
        await eventually("the find hits are drawn", async () => (await palette(page, B).locator("[data-palette-hit]").count()) > 0, 3_000);
        await sleep(500);
        const lastKey = typed.made.at(-1).last;
        let calls = (await sentCalls(page)).slice(asked);
        const finds = calls.filter((c) => c.method === "tickets.find");
        assert(finds.length === 1, `one find (${finds.length}: ${JSON.stringify(finds.map((f) => f.params.query))})`);
        assert(finds[0].params.query === query, `the find asks the whole query: ${JSON.stringify(finds[0].params)}`);
        const pause = finds[0].t - lastKey;
        assert(pause >= QUIET_MS && pause <= QUIET_CEIL_MS, `the find ${QUIET_MS}–${QUIET_CEIL_MS} ms after the last key (${pause.toFixed(0)} ms)`);
        const searches = calls.filter((c) => c.method === "tickets.search");
        assert(searches.length === 1 && searches[0].params.query === query, `one search, for the whole query: ${JSON.stringify(searches.map((s) => s.params.query))}`);
        const first = await palette(page, B).locator("[data-palette-hit]").first().getAttribute("data-palette-hit");
        assert(first === found[0], `the find's first hit is first (${first} vs ${found[0]})`);
        const searched = await attrs(palette(page, B).locator("[data-search-hit]"), "data-search-hit");
        assert(searched.includes(hits[0]), `search's best hit is shown below (${hits[0]} in ${JSON.stringify(searched)})`);

        // Newest only: `zap`, a pause long enough to ask it, then `-34`.
        await input.fill("");
        await sleep(400);
        asked = (await sentCalls(page)).length;
        await input.focus();
        typed = await typeBursts(page, inputSel, ["zap", "-34"], 150);
        if (!typed.kept) {
          console.log(`     the page's clock made ${JSON.stringify(typed.made.map((m) => m.value))} of zap, -34: typed again`);
          await input.fill("");
          await sleep(6_000);
          asked = (await sentCalls(page)).length;
          await input.focus();
          typed = await typeBursts(page, inputSel, ["zap", "-34"], 150);
          assert(typed.kept, `the page typed zap, -34 as two bursts twice running (${JSON.stringify(typed.keys.map((k) => Math.round(k.t)))}): the machine is too loaded for this case`);
        }
        await sleep(800);
        calls = (await sentCalls(page)).slice(asked).filter((c) => c.method === "tickets.find");
        assert(calls.at(-1)?.params.query === "zap-34", `the last find is zap-34: ${JSON.stringify(calls.map((c) => c.params.query))}`);
        // FRAME-25: one find per burst of keys typed within the pause — `zap`, then `zap-34`, nothing between.
        assert(JSON.stringify(calls.map((c) => c.params.query)) === JSON.stringify(typed.made.map((m) => m.value)), `one find per burst (${JSON.stringify(typed.made.map((m) => m.value))}): ${JSON.stringify(calls.map((c) => c.params.query))}`);
        const drawn = await attrs(palette(page, B).locator("[data-palette-hit]"), "data-palette-hit");
        assert(JSON.stringify(drawn) === JSON.stringify(zap34.slice(0, drawn.length)) && drawn.length > 0, `only zap-34's hits are drawn: ${JSON.stringify(drawn)} vs ${JSON.stringify(zap34)}`);

        // Enter jumps to the first hit, on this board.
        await page.keyboard.press("Enter");
        await eventually(`Enter goes to ${B.ticketRoute}${zap34[0]}`, async () => (await page.evaluate(() => location.hash)) === `${B.ticketRoute}${zap34[0]}`, 2_000);
      });

      await onBoard(B, "the_filter_line_filters_the_board_and_the_nav", async () => {
        const filter = "status Ready";
        const expected = summaries("--filter", filter, "--limit", String(PAGE)).map((t) => t.id);
        const { page } = await openPage(B.route);
        await boardUp(page, B);
        const asked = (await sentCalls(page)).length;
        const input = page.locator(`${B.top} input[data-filter-input]`);
        await input.fill(filter, { timeout: 2000 });
        await input.press("Enter");
        await eventually("the route carries the filter", async () => (await page.evaluate(() => location.hash)) === `${B.filterRoute}status%20Ready`, 2_000);
        await eventually("the board shows the filtered rows", async () => {
          const shown = await rowIds(main(page, B).locator(".board-row[data-id]"));
          return shown.length > 0 && JSON.stringify(shown) === JSON.stringify(expected.slice(0, shown.length));
        }, 5_000);
        const calls = (await sentCalls(page)).slice(asked);
        assert(calls.some((c) => c.method === "tickets.list" && c.params.filter === filter && c.params.parent == null), `the board's list carries the filter: ${JSON.stringify(calls.filter((c) => c.method === "tickets.list").map((c) => c.params))}`);
        assert(calls.some((c) => c.method === "tickets.nav.counts" && c.params.filter === filter), `nav counts carries the filter: ${JSON.stringify(calls.map((c) => c.method))}`);

        const input2 = page.locator(`${B.top} input[data-filter-input]`);
        await input2.fill("text foo", { timeout: 2000 });
        await input2.press("Enter");
        const refused = page.locator(`${B.top} [data-filter-refused]`);
        await eventually("the refused chunk is shown under the field", async () => trimmed(await refused.textContent({ timeout: 1000 })).length > 0, 5_000);
        const said = await refused.textContent();
        assert(/text/.test(said), `the refusal names the chunk: ${JSON.stringify(said)}`);
      });

      await onBoard(B, "sign_in_returns_to_the_route", async () => {
        const t = summaries("--limit", "3")[2];
        const { page, context } = await openPage(`${B.ticketRoute}${t.id}`, { signedIn: false });
        await eventually("no session: the login page", async () => new URL(page.url()).pathname === "/login", 5_000);
        assert(new URL(page.url()).hash === `${B.ticketRoute}${t.id}`, `the route rides along: ${page.url()}`);
        await context.addCookies(sessionCookies(server.origin, token));
        await page.reload();
        await eventually(`signed in: back on ${B.ticketRoute}${t.id}`, async () => {
          const u = new URL(page.url());
          return u.pathname === "/app/" && u.hash === `${B.ticketRoute}${t.id}`;
        }, 5_000);
        await eventually("the ticket is drawn", async () => trimmed(await page.locator(`${B.page} [data-ticket-title]`).textContent({ timeout: 1000 })) === t.title.trim(), 5_000);
      });

      await onBoard(B, "a_section_of_the_board_is_swapped_by_right_click", async () => {
        const { page, seen } = await openPage(B.route);
        await eventually("v2's board draws", async () => (await main(page, B).locator(".board-newer").count()) === 1);
        await boardUp(page, B);
        // FRAME-30: a sheet is its version's — v2's count in --accent, v1's in --ink.
        const countColour = () => main(page, B).locator(".board-count").evaluate((e) => getComputedStyle(e).color);
        assert((await countColour()) === "rgb(59, 91, 219)", `v2's sheet: the count in --accent (${await countColour()})`);
        await eventually("the nav's rows", async () => (await nav(page, B).locator("[data-nav-group]").count()) > 0);
        const origin = await marked(page);
        const navSel = `[data-layout=${B.layout}][data-slot=nav] ${B.nav}`;
        await page.evaluate((s) => (document.querySelector(s).__kept = true), navSel);
        const asked = (await sentCalls(page)).length;

        await main(page, B).locator(".board-row[data-id]").first().click({ button: "right" });
        const menu = page.locator("[data-engine-menu]");
        await eventually("the engine's menu opens on the board", async () => menu.isVisible());
        await eventually("the menu offers the older board", async () => (await menu.locator(`[data-version="${v1.version}"]`).count()) === 1, 3_000);
        assert((await menu.locator(`[data-version="${v2.version}"]`).count()) === 0, "the live version is not offered");
        const fetchedBefore = seen.requests.length;
        await menu.locator(`[data-version="${v1.version}"]`).click();
        await eventually("the older board draws", async () => (await main(page, B).locator(".board-newer").count()) === 0 && (await main(page, B).locator(B.main).count()) === 1);
        await boardUp(page, B);
        await eventually("v1's sheet swapped in without a reload: the count in --ink", async () => (await countColour()) === "rgb(29, 28, 26)", 3_000);
        const fetched = seen.requests.slice(fetchedBefore);
        assert(fetched.some((u) => u.endsWith(`/app/${v1.version}/components/${B.module}`)), `v1's ${B.module} was fetched: ${fetched.join(" ")}`);
        const kept = await page.evaluate((s) => document.querySelector(s)?.__kept === true, navSel);
        assert(kept, "the nav is the same element, untouched");
        const after = (await sentCalls(page)).slice(asked);
        assert(!after.some((c) => c.method === "tickets.nav.groups"), "the nav asked nothing again");
        const slots = sy("ui", "slots", "list", "--layout", B.layout);
        const m = slots.find((s) => s.type === "assignment" && s.slot === "main");
        assert(m?.component === B.main && m?.version === v1.version, `ui slots list names the older board: ${JSON.stringify(slots)}`);
        await sameDocument(page, origin, "after the pick");
        sy("ui", "slots", "clear", "--layout", B.layout, "--slot", "main");
        await eventually("clear returns the newer board", async () => (await main(page, B).locator(".board-newer").count()) === 1);
      });
    }

    for (const name of BOARD_NAMES) await boardTests(BOARDS[name]);

    // FRAME-14: a right-click puts the Svelte one in a section of the rhx
    // board — each of the three, beside rhx components on one page and one
    // socket — and `ui slots clear` puts the rhx one back.
    if (BOARD_NAMES.includes("rhx") && BOARD_NAMES.includes("sv")) {
      await test("a_section_swaps_to_the_svelte_one", async () => {
        const R = BOARDS.rhx;
        const S = BOARDS.sv;
        if (S.down) throw new Error(`the sv board is not drawn (the_bundle_draws_the_board_sv): ${S.down}`);
        const { page, seen } = await openPage(R.route);
        await boardUp(page, R);
        await eventually("the nav's rows", async () => (await nav(page, R).locator("[data-nav-group]").count()) > 0);
        const origin = await marked(page);
        try {
          for (const [slot, rhx, sv] of [["main", R.main, S.main], ["nav", R.nav, S.nav], ["palette", R.palette, S.palette]]) {
            await section(page, R, slot).dispatchEvent("contextmenu", { button: 2 });
            const menu = page.locator("[data-engine-menu]");
            await eventually(`the engine's menu opens on the ${slot} section`, async () => menu.isVisible());
            await eventually(`the menu offers ${sv}`, async () => (await menu.locator(`[data-component="${sv}"]`).count()) === 1, 3_000);
            await menu.locator(`[data-component="${sv}"]`).click();
            await eventually(`the ${slot} section holds ${sv}, not ${rhx}`, async () => (await section(page, R, slot).locator(sv).count()) === 1 && (await section(page, R, slot).locator(rhx).count()) === 0, 5_000);
            const slots = sy("ui", "slots", "list", "--layout", R.layout);
            const a = slots.find((s) => s.type === "assignment" && s.slot === slot);
            assert(a?.component === sv, `ui slots list names ${sv} in ${slot}: ${JSON.stringify(slots)}`);
          }
          // The Svelte ones work there, among the rhx layout's own parts.
          await boardUp(page, R);
          assert((await main(page, R).locator(`${S.main} .board-row[data-id]`).count()) > 0, "the Svelte rows are drawn in the rhx board's main section");
          await eventually("the Svelte nav draws its shortcodes", async () => (await nav(page, R).locator(`${S.nav} [data-nav-group]`).count()) > 0, 5_000);
          await page.locator("body").click({ position: { x: 5, y: 5 } }).catch(() => {});
          await page.keyboard.press("/");
          const input = palette(page, R).locator(`${S.palette} input[data-palette-input]`);
          await eventually("`/` focuses the Svelte palette's field", async () => input.evaluate((e) => e === document.activeElement), 2_000);
          await page.keyboard.press("Escape");
          await sameDocument(page, origin, "after the swaps");
          assert(seen.sockets === 1, `one socket for both profiles (${seen.sockets})`);
        } finally {
          for (const slot of ["main", "nav", "palette"]) sy("ui", "slots", "clear", "--layout", R.layout, "--slot", slot);
        }
        for (const [slot, rhx] of [["main", R.main], ["nav", R.nav], ["palette", R.palette]]) {
          await eventually(`clear puts ${rhx} back in ${slot}`, async () => (await section(page, R, slot).locator(rhx).count()) === 1, 5_000);
        }
      });
    }
    // SURFACE-1: the groups' cases, each file given the runner's helpers.
    const groupsDir = path.join(path.dirname(new URL(import.meta.url).pathname), "pages");
    const groupFiles = fs.existsSync(groupsDir) ? fs.readdirSync(groupsDir).filter((f) => f.endsWith(".mjs")).sort() : [];
    const wanted = GROUP && GROUP !== "all" ? groupFiles.filter((f) => f === `${GROUP}.mjs`) : groupFiles;
    if (GROUP && GROUP !== "all" && !wanted.length) throw new Error(`--group ${GROUP}: no tests/pages/${GROUP}.mjs (have: ${groupFiles.join(", ") || "none"})`);
    inGroup = true;
    const ctx = { test, assert, eventually, sleep, typeBursts, openPage, marked, sameDocument, sy, summaries, extentsNow, groupsNow,
      section, main, nav, palette, rowIds, attrs, boardUp, BOARDS, server, token, session, v1, v2, pages, fs, path };
    for (const f of wanted) {
      console.log(`group ${f.replace(/\.mjs$/, "")}`);
      const m = await import(path.join(groupsDir, f));
      await m.default(ctx);
    }
  } finally {
    if (results.some((r) => !r.ok)) {
      for (const [i, seen] of pages.entries()) {
        const lines = seen.console.filter((l) => !l.startsWith("debug"));
        if (!lines.length) continue;
        console.log(`page ${i + 1}'s console:`);
        for (const l of lines.slice(-12)) console.log(`     ${l}`);
      }
    }
    await browser?.close();
    if (!server.keep) server.stop();
    else console.log(`kept: ws ${server.ws}, shell ${server.origin} (pid ${server.child?.pid}), session ${session}`);
  }

  const failed = results.filter((r) => !r.ok).length;
  console.log(`${results.length - failed} passed, ${failed} failed`);
  process.exit(failed ? 1 : 0);
}

main().catch((e) => {
  console.error(`setup failed: ${e.stack || e.message}`);
  process.exit(2);
});
