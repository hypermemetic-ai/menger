#!/usr/bin/env node
// UI-8's testing surface, in a browser: the first pages on a throwaway menger.
//
//   node tests/pages_browser.mjs [--seed N] [--debug] [--no-build] [--headed] [--keep] [--only NAME]
//
// A throwaway server (scripts/ui-throwaway.mjs: OS-assigned ports, its
// files under /tmp/claude-1000/, `--auth-provider key` with a key made here
// as `--admin`, a second as `--ui-signer`), seeded `--seed-synthetic N`
// (default 100,000), release build (`--debug` for the debug one). Two
// bundles from scripts/ui-build.sh, signed with ui-sign and pushed over
// synapse: v1 is ui/ as it is; v2 the same with one line added to
// board-rows.svelte (`<p class="board-newer">`), so the two differ in the
// board alone. v2 is pushed last, so it is live. Chromium from Playwright,
// signed in with the shell's session cookie (scripts/throwaway-login.mjs).
//
// Measured in the page (scripts/ui-throwaway.mjs's INIT, before the
// engine): every frame the page sends, stamped; the frame that carries a
// `created` ticket; each row as it appears; what `.board-count` shows.
//
// What the pages draw, which these tests read (the pages' DOM contract):
//
//   /app/ (#/)          board-layout: sections `[data-slot=nav|main|palette][data-layout=board-layout]`
//   main: board-rows    `.board-scroll[data-row-height]` (the scrolling element; its row pitch in px),
//                       `.board-row[data-id]` (one per row held), `.board-count` (the tickets' number)
//   nav: nav-tree       `.nav-group[data-shortcode]` per shortcode, its `.nav-rollup` ("done/total",
//                       `groups`' numbers), its `.nav-toggle` (opens it), its children `.nav-row[data-id]`
//   palette             `input.palette-input` (`/` focuses it), `.palette-hit[data-id]` (find, in order),
//                       `.palette-search-hit[data-id]` (search's hits, below)
//   #/ticket/:id        ticket-page: `.ticket-title`, `.ticket-status`, `.ticket-kind`,
//                       `.chip[data-chip=parent|blocker|unlocks][data-id]` each with `.chip-title`,
//                       `.ticket-body` (the body, markdown drawn; no <script>, no <style>)
//
//   the_board_asks_extents_then_one_page
//   a_scroll_asks_only_the_page_it_needs
//   a_created_ticket_appears_without_a_reload
//   the_nav_opens_a_shortcode_to_one_page
//   a_ticket_page_draws_the_body_without_scripts
//   the_palette_finds_and_searches
//   a_section_of_the_board_is_swapped_by_right_click
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
const ONLY = opt("--only", null);
const PAGE = 200; // the board's page
const HELD_MAX = 2_500; // rows held after a scroll
const CREATE_MS = 100; // a created ticket's frame → its row and count
const QUIET_MS = 80; // the palette's pause
const SCROLL_ROW = Math.min(50_000, SEED); // the row the scroll goes to (1-based)

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

/** The extents as the board reads them: each shortcode's tickets are its
 *  sequences 1..last_seq but its holes, in shortcode order. */
function rowsOf(extents) {
  const parts = extents.map((e) => ({ epic: e.epic, last: e.last_seq, holes: new Set(e.holes ?? []), n: e.last_seq - (e.holes?.length ?? 0) }));
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
async function test(name, body) {
  if (ONLY && ONLY !== name) return;
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
    engine: v1.stage,
    edit: (ui) => fs.appendFileSync(path.join(ui, "src", "board-rows.svelte"), '<p class="board-newer">the newer board</p>\n'),
  });
  console.log(`v1 ${v1.version}\nv2 ${v2.version}`);

  const adminKey = path.join(work, "admin.key");
  const signerKey = path.join(work, "signer.key");
  const admin = makeKey(adminKey);
  const signerHex = makeKey(signerKey).replace(/^key:/, "");
  const server = new Throwaway(work, PROFILE);
  const t0 = Date.now();
  await server.start(admin, signerHex, SEED);
  console.log(`server up, ${SEED} seeded, in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
  const session = path.join(work, "session");
  await writeSession(`ws://127.0.0.1:${server.ws}`, adminKey, session, "owner");
  const token = readToken(session);
  const sy = (...a) => synapse(server, session, ...a);

  for (const b of [v1, v2]) {
    const answer = push(server, session, signerKey, b);
    if (answer?.type !== "pushed" || answer.version !== b.version) throw new Error(`ui push of ${path.basename(b.tar)}: ${JSON.stringify(answer)}`);
  }

  const { chromium } = await playwright();
  const browser = await chromium.launch({ headless: !flag("--headed") });

  /** A page signed in, its console kept; the engine's frames counted from INIT. */
  async function openPage(route = "") {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
    await context.addCookies(sessionCookies(server.origin, token));
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
  const main = (page) => page.locator("[data-layout=board-layout][data-slot=main]");
  const nav = (page) => page.locator("[data-layout=board-layout][data-slot=nav]");
  const palette = (page) => page.locator("[data-layout=board-layout][data-slot=palette]");
  const shownRows = (page) => main(page).locator(".board-row[data-id]").evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));

  try {
    // Setup's last step: the bundle is what the page draws — the layout's
    // three sections, each with its default component, and the ticket route.
    {
      const { page, context } = await openPage();
      for (const [slot, component] of [["nav", "nav-tree"], ["main", "board-rows"], ["palette", "jump-palette"]]) {
        await eventually(`setup: the ${slot} section holds ${component}`, async () => (await page.locator(`[data-layout=board-layout][data-slot=${slot}] > ${component}`).count()) === 1).catch((e) => {
          throw new Error(`${e.message}\n${pages.at(-1).console.slice(-10).join("\n")}`);
        });
      }
      await page.evaluate(() => (location.hash = "#/ticket/NONE-1"));
      await eventually("setup: #/ticket/:id draws ticket-page", async () => (await page.locator("ticket-page").count()) === 1);
      await context.close();
      pages.pop();
      console.log("setup: the bundle draws board-layout's sections and ticket-page");
    }

    await test("the_board_asks_extents_then_one_page", async () => {
      const extents = sy("tickets", "nav", "extents").filter((e) => e.type === "extent");
      const { total } = rowsOf(extents);
      const expected = sy("tickets", "list", "--summary", "true", "--limit", String(PAGE)).filter((e) => e.type === "summary").map((e) => e.summary.id);
      const { page } = await openPage();
      await eventually("the board draws its first rows", async () => (await main(page).locator(".board-row[data-id]").count()) > 0);
      await sleep(1_500); // what else the page asks, it asks now
      const calls = await sentCalls(page);
      const ext = calls.filter((c) => c.method === "tickets.nav.extents");
      assert(ext.length === 1, `nav.extents asked once (${ext.length})`);
      const lists = boardLists(calls);
      assert(lists.length === 1, `one list for the window (${lists.length}: ${JSON.stringify(lists.map((l) => l.params))})`);
      assert(lists[0].t >= ext[0].t, "the list after the extents");
      assert(lists[0].params.limit === PAGE && lists[0].params.summary === true, `the page is limit ${PAGE}, summary: ${JSON.stringify(lists[0].params)}`);
      assert(lists.slice(1).every((l) => l.params.after != null), "no list without after past the first");
      const shown = await shownRows(page);
      assert(JSON.stringify(shown) === JSON.stringify(expected.slice(0, shown.length)), `the rows are list's first, in order: ${shown.slice(0, 5).join(" ")} … vs ${expected.slice(0, 5).join(" ")} …`);
      assert(digits(await main(page).locator(".board-count").textContent({ timeout: 1000 })) === total, `the count is the extents' sum, ${total}`);
      const firstRow = await page.evaluate(() => window.__pages.firstRow);
      console.log(`     first row: ${firstRow?.toFixed(0)} ms from timeOrigin · ${shown.length} rows drawn · ${total} tickets`);
    });

    await test("a_scroll_asks_only_the_page_it_needs", async () => {
      const extents = sy("tickets", "nav", "extents").filter((e) => e.type === "extent");
      const { idAt } = rowsOf(extents);
      const before = idAt(SCROLL_ROW - 1);
      const target = idAt(SCROLL_ROW);
      const next = sy("tickets", "list", "--after", before, "--limit", "1", "--summary", "true").find((e) => e.type === "summary")?.summary.id;
      assert(next === target, `row ${SCROLL_ROW} by arithmetic is the one after row ${SCROLL_ROW - 1} (${target} vs list's ${next})`);

      const { page } = await openPage();
      await eventually("the board draws its first rows", async () => (await main(page).locator(".board-row[data-id]").count()) > 0);
      const origin = await marked(page);
      await sleep(500);
      const asked = boardLists(await sentCalls(page)).length;
      const pitch = Number(await main(page).locator(".board-scroll").getAttribute("data-row-height", { timeout: 1000 }));
      assert(pitch > 0, `the board says its row pitch (${pitch})`);
      await main(page).locator(".board-scroll").evaluate((el, top) => (el.scrollTop = top), (SCROLL_ROW - 1) * pitch);
      await eventually(`row ${SCROLL_ROW} (${target}) is drawn`, async () => (await main(page).locator(`.board-row[data-id="${target}"]`).count()) === 1, 5_000);
      await sleep(500);
      const lists = boardLists(await sentCalls(page)).slice(asked);
      assert(lists.length === 1, `one list for the scroll (${lists.length}: ${JSON.stringify(lists.map((l) => l.params))})`);
      assert(lists[0].params.after === before, `the list starts after row ${SCROLL_ROW - 1}, ${before}: ${JSON.stringify(lists[0].params)}`);
      const held = await main(page).locator(".board-row[data-id]").count();
      assert(held < HELD_MAX, `the rows held stay under ${HELD_MAX} (${held})`);
      await sameDocument(page, origin, "after the scroll");
      console.log(`     ${held} rows held after the scroll to row ${SCROLL_ROW}`);
    });

    await test("a_created_ticket_appears_without_a_reload", async () => {
      // A shortcode first in the store's order, so its tickets are the board's first rows.
      const [root] = sy("tickets", "create", "--epic", "AAAA", "--kind", "scope", "--title", "the board's first shortcode");
      assert(root?.type === "created", `the scope was made: ${JSON.stringify(root)}`);
      const total = rowsOf(sy("tickets", "nav", "extents").filter((e) => e.type === "extent")).total;
      const { page, seen } = await openPage();
      await eventually("the board shows the new shortcode's root", async () => (await main(page).locator(`.board-row[data-id="${root.ticket.id}"]`).count()) === 1);
      assert(digits(await main(page).locator(".board-count").textContent({ timeout: 1000 })) === total, `the count before is ${total}`);
      const origin = await marked(page);

      const [created] = sy("tickets", "create", "--epic", "AAAA", "--parent", root.ticket.id, "--title", "made by the pages test");
      const id = created?.ticket?.id;
      assert(id, `tickets create answered a ticket: ${JSON.stringify(created)}`);
      await eventually(`the created ticket ${id} is on the board`, async () => (await main(page).locator(`.board-row[data-id="${id}"]`).count()) === 1, 5_000);
      await eventually(`the count reads ${total + 1}`, async () => digits(await main(page).locator(".board-count").textContent({ timeout: 1000 })) === total + 1, 2_000);
      const t = await page.evaluate((id) => ({ frame: window.__pages.created[id], row: window.__pages.boardRows[id], counts: window.__pages.counts }), id);
      const counted = t.counts.find((c) => digits(c.text) === total + 1)?.t;
      assert(t.frame != null && t.row != null && counted != null, `the frame, the row and the count were stamped: ${JSON.stringify({ frame: t.frame, row: t.row, counted })}`);
      console.log(`     created frame → row ${(t.row - t.frame).toFixed(1)} ms, → count ${(counted - t.frame).toFixed(1)} ms (budget ${CREATE_MS} ms)`);
      assert(t.row - t.frame < CREATE_MS, `the row within ${CREATE_MS} ms of its frame (${(t.row - t.frame).toFixed(1)} ms)`);
      assert(counted - t.frame < CREATE_MS, `the count within ${CREATE_MS} ms of the frame (${(counted - t.frame).toFixed(1)} ms)`);
      await sameDocument(page, origin, "after the create");
      assert(seen.sockets === 1, `one socket (${seen.sockets})`);
    });

    await test("the_nav_opens_a_shortcode_to_one_page", async () => {
      const groups = sy("tickets", "nav", "groups").filter((e) => e.type === "group").map((e) => e.group);
      const { page } = await openPage();
      await eventually("the nav draws a row per shortcode", async () => (await nav(page).locator(".nav-group[data-shortcode]").count()) === groups.length);
      for (const g of groups) {
        const text = await nav(page).locator(`.nav-group[data-shortcode="${g.shortcode}"] .nav-rollup`).first().textContent({ timeout: 1000 });
        const [done, all] = String(text).split("/").map(digits);
        assert(done === g.done && all === g.total, `${g.shortcode}'s roll-up is groups' ${g.done}/${g.total} (shows ${JSON.stringify(text)})`);
      }
      const opened = [...groups].filter((g) => g.root).sort((a, b) => b.total - a.total).slice(0, 2);
      for (const g of opened) {
        const asked = (await sentCalls(page)).length;
        await nav(page).locator(`.nav-group[data-shortcode="${g.shortcode}"] .nav-toggle`).first().click({ timeout: 2000 });
        const expected = sy("tickets", "list", "--parent", g.root, "--summary", "true", "--limit", String(PAGE)).filter((e) => e.type === "summary").map((e) => e.summary.id);
        const rows = nav(page).locator(`.nav-group[data-shortcode="${g.shortcode}"] .nav-row[data-id]`);
        await eventually(`${g.shortcode} opens to its root's children`, async () => (await rows.count()) > 0, 5_000);
        await sleep(300);
        const shown = await rows.evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));
        assert(JSON.stringify(shown) === JSON.stringify(expected.slice(0, shown.length)), `${g.shortcode}'s rows are list { parent: ${g.root} }'s, in order`);
        const lists = (await sentCalls(page)).slice(asked).filter((c) => c.method === "tickets.list");
        assert(lists.length === 1 && lists[0].params.parent === g.root, `one list { parent: ${g.root} } for the open: ${JSON.stringify(lists.map((l) => l.params))}`);
        assert(lists[0].params.limit != null && lists[0].params.limit <= PAGE, `one page (limit ${lists[0].params.limit})`);
      }
    });

    await test("a_ticket_page_draws_the_body_without_scripts", async () => {
      const groups = sy("tickets", "nav", "groups").filter((e) => e.type === "group").map((e) => e.group).filter((g) => g.root && !g.root.startsWith("AAAA"));
      const parent = sy("tickets", "get", "--id", groups[0].root).find((e) => e.type === "ticket").ticket;
      const blocker = sy("tickets", "list", "--parent", parent.id, "--summary", "true", "--limit", "1").find((e) => e.type === "summary").summary;
      const body = path.join(server.work, "body.md");
      fs.writeFileSync(
        body,
        [
          "A paragraph with **weight** in it.",
          "",
          "<script>window.__ranScript = true</script>",
          "<style>body { background: rgb(1, 2, 3) !important; }</style>",
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

      const { page } = await openPage(`#/ticket/${t.id}`);
      const tp = page.locator("ticket-page");
      await eventually("the ticket page draws the title", async () => (await tp.locator(".ticket-title").textContent({ timeout: 1000 })) === t.title);
      await sleep(300);
      assert((await tp.locator(".ticket-status").textContent({ timeout: 1000 })).trim() === t.status, `the status is ${t.status}`);
      assert((await tp.locator(".ticket-kind").textContent({ timeout: 1000 })).trim() === t.kind, `the kind is ${t.kind}`);
      const text = await tp.locator(".ticket-body").textContent({ timeout: 1000 });
      for (const want of ["A paragraph with weight in it.", "the first item", "the last line"]) assert(text.includes(want), `the body says "${want}": ${JSON.stringify(text)}`);
      for (const not of ["__ranScript", "background"]) assert(!text.includes(not), `the body does not show the dropped "${not}"`);
      assert((await tp.locator(".ticket-body strong").textContent({ timeout: 1000 })) === "weight", "the markdown is drawn (**weight** is strong)");
      assert((await tp.locator(".ticket-body li").count()) === 2, "the list is drawn");
      const ran = await page.evaluate(() => ({ script: window.__ranScript === true, bg: getComputedStyle(document.body).backgroundColor, scripts: document.querySelectorAll("ticket-page script").length, styles: document.querySelectorAll("ticket-page style").length }));
      assert(!ran.script, "the body's script did not run");
      assert(ran.bg !== "rgb(1, 2, 3)", `the body's style did not apply (${ran.bg})`);
      assert(ran.scripts === 0 && ran.styles === 0, `no <script> or <style> in the page (${ran.scripts}, ${ran.styles})`);
      for (const [chip, other] of [["parent", parent], ["blocker", blocker]]) {
        const c = tp.locator(`.chip[data-chip=${chip}][data-id="${other.id}"]`);
        assert((await c.count()) === 1, `a ${chip} chip for ${other.id}`);
        await eventually(`the ${chip} chip resolves to "${other.title}"`, async () => (await c.locator(".chip-title").textContent({ timeout: 1000 })) === other.title, 3_000);
      }

      // The blocker's page, by the hash: it unlocks the ticket.
      const origin = await marked(page);
      await page.evaluate((id) => (location.hash = `#/ticket/${id}`), blocker.id);
      await eventually("the blocker's page draws", async () => (await tp.locator(".ticket-title").textContent({ timeout: 1000 })) === blocker.title);
      const u = tp.locator(`.chip[data-chip=unlocks][data-id="${t.id}"]`);
      await eventually(`an unlocks chip for ${t.id}, resolved`, async () => (await u.locator(".chip-title").textContent({ timeout: 1000 })) === t.title, 3_000);
      await sameDocument(page, origin, "after the hash moved");
    });

    await test("the_palette_finds_and_searches", async () => {
      const groups = sy("tickets", "nav", "groups").filter((e) => e.type === "group").map((e) => e.group).filter((g) => g.root);
      const [made] = sy("tickets", "create", "--epic", groups.at(-1).epic, "--parent", groups.at(-1).root, "--title", "the map-12 lantern");
      assert(made?.ticket?.id, `the lantern was made: ${JSON.stringify(made)}`);
      const query = "map-12";
      const found = sy("tickets", "find", "--query", query).filter((e) => e.type === "summary").map((e) => e.summary.id);
      const hits = sy("tickets", "search", "--query", query).filter((e) => e.type === "hit").map((e) => e.summary.id);
      assert(found.length > 0 && hits.length > 0, `find and search answer ${query} (${found.length}, ${hits.length})`);

      const { page } = await openPage();
      await eventually("the board is up", async () => (await main(page).locator(".board-row[data-id]").count()) > 0);
      await page.locator("body").click({ position: { x: 5, y: 5 } }).catch(() => {});
      await page.keyboard.press("/");
      await eventually("`/` focuses the palette's field", async () => page.evaluate(() => !!document.activeElement?.matches?.("[data-slot=palette] input.palette-input")), 2_000);
      const asked = (await sentCalls(page)).length;
      await page.keyboard.type(query, { delay: 30 });
      await eventually("the find hits are drawn", async () => (await palette(page).locator(".palette-hit[data-id]").count()) > 0, 3_000);
      await sleep(500);
      const lastKey = await page.evaluate(() => window.__pages.lastKey);
      const calls = (await sentCalls(page)).slice(asked);
      const finds = calls.filter((c) => c.method === "tickets.find");
      assert(finds.length === 1, `one find (${finds.length}: ${JSON.stringify(finds.map((f) => f.params.query))})`);
      assert(finds[0].params.query === query, `the find asks the whole query: ${JSON.stringify(finds[0].params)}`);
      assert(finds[0].t - lastKey >= QUIET_MS, `the find after ${QUIET_MS} ms of quiet (${(finds[0].t - lastKey).toFixed(0)} ms after the last key)`);
      const searches = calls.filter((c) => c.method === "tickets.search");
      assert(searches.length === 1 && searches[0].params.query === query, `one search, for the whole query: ${JSON.stringify(searches.map((s) => s.params.query))}`);
      const first = await palette(page).locator(".palette-hit[data-id]").first().getAttribute("data-id");
      assert(first === found[0], `the find's first hit is first (${first} vs ${found[0]})`);
      const searched = await palette(page).locator(".palette-search-hit[data-id]").evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));
      assert(searched.includes(hits[0]), `search's best hit is shown below (${hits[0]} in ${JSON.stringify(searched)})`);
    });

    await test("a_section_of_the_board_is_swapped_by_right_click", async () => {
      const { page, seen } = await openPage();
      await eventually("v2's board draws", async () => (await main(page).locator(".board-newer").count()) === 1);
      await eventually("the board's rows", async () => (await main(page).locator(".board-row[data-id]").count()) > 0);
      await eventually("the nav's rows", async () => (await nav(page).locator(".nav-group").count()) > 0);
      const origin = await marked(page);
      await page.evaluate(() => (document.querySelector("[data-layout=board-layout][data-slot=nav]").firstElementChild.__kept = true));
      const asked = (await sentCalls(page)).length;

      await main(page).locator(".board-row[data-id]").first().click({ button: "right" });
      const menu = page.locator("[data-engine-menu]");
      await eventually("the engine's menu opens on the board", async () => menu.isVisible());
      await eventually("the menu offers the older board", async () => (await menu.locator(`[data-version="${v1.version}"]`).count()) === 1, 3_000);
      assert((await menu.locator(`[data-version="${v2.version}"]`).count()) === 0, "the live version is not offered");
      const fetchedBefore = seen.requests.length;
      await menu.locator(`[data-version="${v1.version}"]`).click();
      await eventually("the older board draws", async () => (await main(page).locator(".board-newer").count()) === 0 && (await main(page).locator("board-rows").count()) === 1);
      await eventually("the older board has its rows", async () => (await main(page).locator(".board-row[data-id]").count()) > 0);
      const fetched = seen.requests.slice(fetchedBefore);
      assert(fetched.some((u) => u.endsWith(`/app/${v1.version}/components/board-rows.json`)), `v1's board-rows.json was fetched: ${fetched.join(" ")}`);
      const kept = await page.evaluate(() => document.querySelector("[data-layout=board-layout][data-slot=nav]").firstElementChild?.__kept === true);
      assert(kept, "the nav is the same element, untouched");
      const after = (await sentCalls(page)).slice(asked);
      assert(!after.some((c) => c.method === "tickets.nav.groups"), "the nav asked nothing again");
      const slots = sy("ui", "slots", "list", "--layout", "board-layout");
      const m = slots.find((s) => s.type === "assignment" && s.slot === "main");
      assert(m?.component === "board-rows" && m?.version === v1.version, `ui slots list names the older board: ${JSON.stringify(slots)}`);
      await sameDocument(page, origin, "after the pick");
      sy("ui", "slots", "clear", "--layout", "board-layout", "--slot", "main");
      await eventually("clear returns the newer board", async () => (await main(page).locator(".board-newer").count()) === 1);
    });
  } finally {
    if (results.some((r) => !r.ok)) {
      for (const [i, seen] of pages.entries()) {
        const lines = seen.console.filter((l) => !l.startsWith("debug"));
        if (!lines.length) continue;
        console.log(`page ${i + 1}'s console:`);
        for (const l of lines.slice(-12)) console.log(`     ${l}`);
      }
    }
    await browser.close();
    if (!flag("--keep")) server.stop();
    else console.log(`kept: ws ${server.ws}, shell ${server.origin} (pid ${server.child.pid}), session ${session}`);
  }

  const failed = results.filter((r) => !r.ok).length;
  console.log(`${results.length - failed} passed, ${failed} failed`);
  process.exit(failed ? 1 : 0);
}

main().catch((e) => {
  console.error(`setup failed: ${e.stack || e.message}`);
  process.exit(2);
});
