#!/usr/bin/env node
// UI-8's testing surface, in a browser: the first pages on a throwaway menger.
//
//   node tests/pages_browser.mjs [--seed N] [--debug] [--no-build] [--headed] [--keep] [--only NAME[,NAME]]
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
// The testing hooks the pages draw (UI-8's body: the named three, and
// `data-*` attributes for the rest):
//
//   /app/ (#/, #/f/:filter)  board-layout: sections `[data-layout=board-layout][data-slot=nav|main|palette]`;
//                       the filter line `input[data-filter-input]`, its refusal `[data-filter-refused]`
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
const PAGE = 200; // the board's page
const HELD_MAX = 2_500; // rows held after a scroll
const CREATE_MS = 100; // a created ticket's frame → its row and count
const QUIET_MS = 80; // the palette's pause
const QUIET_CEIL_MS = 300; // … and no longer than this
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
async function test(name, body) {
  if (ONLY && !ONLY.includes(name)) return;
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
  const extentsNow = () => sy("tickets", "nav", "extents").filter((e) => e.type === "extent");
  const groupsNow = () => sy("tickets", "nav", "groups").filter((e) => e.type === "group").map((e) => e.group);
  const summaries = (...a) => sy("tickets", "list", "--summary", "true", ...a).filter((e) => e.type === "summary").map((e) => e.summary);

  for (const b of [v1, v2]) {
    const answer = push(server, session, signerKey, b);
    if (answer?.type !== "pushed" || answer.version !== b.version) throw new Error(`ui push of ${path.basename(b.tar)}: ${JSON.stringify(answer)}`);
  }

  const { chromium } = await playwright();
  const browser = await chromium.launch({ headless: !flag("--headed") });

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
  const section = (page, slot) => page.locator(`[data-layout=board-layout][data-slot=${slot}]`);
  const main = (page) => section(page, "main");
  const nav = (page) => section(page, "nav");
  const palette = (page) => section(page, "palette");
  const rowIds = (loc) => loc.evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));
  const attrs = (loc, name) => loc.evaluateAll((els, n) => els.map((e) => e.getAttribute(n)), name);
  const count = async (page) => digits(await main(page).locator(".board-count").textContent({ timeout: 1000 }));
  const boardUp = (page) => eventually("the board shows its first rows' titles", async () => trimmed(await main(page).locator(".board-row[data-id] [data-field=title]").first().textContent({ timeout: 1000 })).length > 0);

  try {
    // Setup's last step: the bundle is what the page draws — the layout's
    // three sections, each with its default component (a descendant of
    // its section), and the ticket route.
    {
      const { page, context } = await openPage();
      for (const [slot, component] of [["nav", "nav-tree"], ["main", "board-rows"], ["palette", "jump-palette"]]) {
        await eventually(`setup: the ${slot} section holds ${component}`, async () => (await section(page, slot).locator(component).count()) === 1).catch((e) => {
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
      const { total } = rowsOf(extentsNow());
      const expected = summaries("--limit", String(PAGE));
      const { page } = await openPage();
      await boardUp(page);
      await sleep(1_500); // what else the page asks, it asks now
      const calls = await sentCalls(page);
      const ext = calls.filter((c) => c.method === "tickets.nav.extents");
      assert(ext.length === 1, `nav.extents asked once (${ext.length})`);
      const lists = boardLists(calls);
      assert(lists.length === 1, `one list for the window (${lists.length}: ${JSON.stringify(lists.map((l) => l.params))})`);
      assert(lists[0].t >= ext[0].t, "the list after the extents");
      assert(lists[0].params.limit === PAGE && lists[0].params.summary === true, `the page is limit ${PAGE}, summary: ${JSON.stringify(lists[0].params)}`);
      assert(lists[0].params.after == null, "the first page has no after");
      const rows = main(page).locator(".board-row[data-id]");
      const shown = await rowIds(rows);
      const want = expected.slice(0, shown.length);
      assert(JSON.stringify(shown) === JSON.stringify(want.map((t) => t.id)), `the rows are list's first, in order: ${shown.slice(0, 5).join(" ")} … vs ${want.slice(0, 5).map((t) => t.id).join(" ")} …`);
      const titles = await rows.locator("[data-field=title]").evaluateAll((els) => els.map((e) => e.textContent.trim()));
      assert(JSON.stringify(titles) === JSON.stringify(want.map((t) => t.title.trim())), "each row shows its title from list");
      assert((await count(page)) === total, `the count is the extents' sum, ${total}`);
      const firstRow = await page.evaluate(() => window.__pages.firstRow);
      assert(firstRow != null, "the first row was stamped once its title showed");
      console.log(`     first row (titled): ${firstRow.toFixed(0)} ms from timeOrigin · ${shown.length} rows drawn · ${total} tickets`);
    });

    await test("a_scroll_asks_only_the_page_it_needs", async () => {
      const { idAt } = rowsOf(extentsNow());
      const before = idAt(SCROLL_ROW - 1);
      const target = idAt(SCROLL_ROW);
      const next = summaries("--after", before, "--limit", "1")[0]?.id;
      assert(next === target, `row ${SCROLL_ROW} by arithmetic is the one after row ${SCROLL_ROW - 1} (${target} vs list's ${next})`);

      const { page } = await openPage();
      await boardUp(page);
      const origin = await marked(page);
      await sleep(500);
      const asked = boardLists(await sentCalls(page));
      assert(asked.length === 1, `one board list before the scroll (${asked.length})`);
      const scroller = main(page).locator(".board-scroll");
      const pitch = Number(await scroller.getAttribute("data-row-height", { timeout: 1000 }));
      assert(pitch > 0, `the board says its row pitch (${pitch})`);
      await scroller.evaluate((el, top) => (el.scrollTop = top), (SCROLL_ROW - 1) * pitch);
      const row = main(page).locator(`.board-row[data-id="${target}"]`);
      await eventually(`row ${SCROLL_ROW} (${target}) is drawn`, async () => (await row.count()) === 1, 5_000);
      await sleep(500);
      const lists = boardLists(await sentCalls(page)).slice(asked.length);
      assert(lists.length === 1, `one list for the scroll (${lists.length}: ${JSON.stringify(lists.map((l) => l.params))})`);
      assert(lists[0].params.after === before, `the list starts after row ${SCROLL_ROW - 1}, ${before}: ${JSON.stringify(lists[0].params)}`);
      const all = boardLists(await sentCalls(page));
      assert(all.every((l) => l.params.limit != null && l.params.limit <= PAGE), `every board list asks at most ${PAGE}: ${JSON.stringify(all.map((l) => l.params.limit))}`);
      const [rowTop, scrollTop] = await Promise.all([row.evaluate((e) => e.getBoundingClientRect().top), scroller.evaluate((e) => e.getBoundingClientRect().top)]);
      assert(Math.abs(rowTop - scrollTop) <= pitch, `row ${SCROLL_ROW} is at the top of the board (${rowTop.toFixed(0)} vs ${scrollTop.toFixed(0)}, pitch ${pitch})`);
      const held = await main(page).locator(".board-row[data-id]").count();
      assert(held < HELD_MAX, `the rows held stay under ${HELD_MAX} (${held})`);
      await sameDocument(page, origin, "after the scroll");
      console.log(`     ${held} rows held after the scroll to row ${SCROLL_ROW}`);
    });

    await test("a_created_ticket_appears_without_a_reload", async () => {
      // A shortcode first in the store's order, so its tickets are the board's first rows.
      const [root] = sy("tickets", "create", "--epic", "AAAA", "--kind", "scope", "--title", "the board's first shortcode");
      assert(root?.type === "created", `the scope was made: ${JSON.stringify(root)}`);
      const total = rowsOf(extentsNow()).total;
      const { page, seen } = await openPage();
      await eventually("the board shows the new shortcode's root", async () => (await main(page).locator(`.board-row[data-id="${root.ticket.id}"]`).count()) === 1);
      assert((await count(page)) === total, `the count before is ${total}`);
      const origin = await marked(page);

      const [created] = sy("tickets", "create", "--epic", "AAAA", "--parent", root.ticket.id, "--title", "made by the pages test");
      const id = created?.ticket?.id;
      assert(id, `tickets create answered a ticket: ${JSON.stringify(created)}`);
      await eventually(`the created ticket ${id} is on the board`, async () => (await main(page).locator(`.board-row[data-id="${id}"]`).count()) === 1, 5_000);
      await eventually(`the count reads ${total + 1}`, async () => (await count(page)) === total + 1, 2_000);
      const t = await page.evaluate((id) => ({ frame: window.__pages.created[id], row: window.__pages.boardRows[id], counts: window.__pages.counts }), id);
      const counted = t.counts.find((c) => digits(c.text) === total + 1)?.t;
      assert(t.frame != null && t.row != null && counted != null, `the frame, the row and the count were stamped: ${JSON.stringify({ frame: t.frame, row: t.row, counted })}`);
      console.log(`     created frame → row ${(t.row - t.frame).toFixed(1)} ms, → count ${(counted - t.frame).toFixed(1)} ms (budget ${CREATE_MS} ms)`);
      assert(t.row - t.frame < CREATE_MS, `the row within ${CREATE_MS} ms of its frame (${(t.row - t.frame).toFixed(1)} ms)`);
      assert(counted - t.frame < CREATE_MS, `the count within ${CREATE_MS} ms of the frame (${(counted - t.frame).toFixed(1)} ms)`);
      await sameDocument(page, origin, "after the create");
      assert(seen.sockets === 1, `one socket (${seen.sockets})`);
    });

    await test("a_status_change_patches_the_row_in_place", async () => {
      const { page } = await openPage();
      await boardUp(page);
      const origin = await marked(page);
      const row = main(page).locator(".board-row[data-id]").nth(3);
      const id = await row.getAttribute("data-id");
      const was = trimmed(await row.locator("[data-field=status]").textContent({ timeout: 1000 }));
      const to = was === "Ready" ? "Pending" : "Ready";
      await row.evaluate((e) => (e.__tagged = true));
      const asked = (await sentCalls(page)).length;
      const [changed] = sy("tickets", "set_status", "--id", id, "--status", to);
      assert(changed?.type === "status_changed", `set_status answered: ${JSON.stringify(changed)}`);
      const same = main(page).locator(`.board-row[data-id="${id}"]`);
      await eventually(`${id}'s row shows ${to}`, async () => trimmed(await same.locator("[data-field=status]").textContent({ timeout: 1000 })) === to, 3_000);
      assert(await same.evaluate((e) => e.__tagged === true), "the same element, patched in place");
      await sleep(300);
      const lists = (await sentCalls(page)).slice(asked).filter((c) => c.method === "tickets.list");
      assert(lists.length === 0, `no list for a status change (${lists.length})`);
      await sameDocument(page, origin, "after the status change");
    });

    await test("a_new_shortcode_appears_on_the_open_board", async () => {
      const total = rowsOf(extentsNow()).total;
      const { page } = await openPage();
      await boardUp(page);
      assert((await count(page)) === total, `the count before is ${total}`);
      const origin = await marked(page);
      const [made] = sy("tickets", "create", "--epic", "AAAB", "--kind", "scope", "--title", "a shortcode born while the board is open");
      const id = made?.ticket?.id;
      assert(id, `the scope was made: ${JSON.stringify(made)}`);
      await eventually(`${id}'s row appears`, async () => (await main(page).locator(`.board-row[data-id="${id}"]`).count()) === 1, 3_000);
      await eventually(`the count reads ${total + 1}`, async () => (await count(page)) === total + 1, 2_000);
      await sameDocument(page, origin, "after the new shortcode");
    });

    await test("the_nav_opens_a_shortcode_to_one_page", async () => {
      const groups = groupsNow();
      const { page } = await openPage();
      const tree = nav(page).locator("[data-nav-group]");
      await eventually("Tree draws a row per shortcode", async () => (await tree.count()) === groups.length);
      for (const g of groups) {
        const row = nav(page).locator(`[data-nav-group="${g.shortcode}"]`);
        const done = digits(await row.locator("[data-rollup-done]").first().textContent({ timeout: 1000 }));
        const all = digits(await row.locator("[data-rollup-total]").first().textContent({ timeout: 1000 }));
        assert(done === g.done && all === g.total, `${g.shortcode}'s roll-up is groups' ${g.done} of ${g.total} (shows ${done} of ${all})`);
      }
      const opened = groups.filter((g) => g.root).sort((a, b) => b.total - a.total).slice(0, 2);
      for (const g of opened) {
        const asked = (await sentCalls(page)).length;
        await nav(page).locator(`[data-nav-group="${g.shortcode}"] [data-nav-toggle]`).first().click({ timeout: 2000 });
        const expected = summaries("--parent", g.root, "--limit", String(PAGE)).map((t) => t.id);
        const rows = nav(page).locator(`[data-nav-group="${g.shortcode}"] [data-nav-row]`);
        await eventually(`${g.shortcode} opens to its root's children`, async () => (await rows.count()) > 0, 5_000);
        await sleep(300);
        const shown = await attrs(rows, "data-nav-row");
        assert(JSON.stringify(shown) === JSON.stringify(expected.slice(0, shown.length)), `${g.shortcode}'s rows are list { parent: ${g.root} }'s, in order`);
        const slice = (await sentCalls(page)).slice(asked);
        assert(slice.length === 1 && slice[0].method === "tickets.list" && slice[0].params.parent === g.root, `the open is exactly one list { parent: ${g.root} }: ${JSON.stringify(slice.map((c) => [c.method, c.params]))}`);
        assert(slice[0].params.limit != null && slice[0].params.limit <= PAGE, `one page (limit ${slice[0].params.limit})`);
      }
    });

    await test("the_nav_roll_ups_follow_group_changed", async () => {
      const g = groupsNow().filter((x) => x.root && !x.shortcode.startsWith("AAA"))[0];
      const { page } = await openPage();
      const total = nav(page).locator(`[data-nav-group="${g.shortcode}"] [data-rollup-total]`).first();
      await eventually(`${g.shortcode}'s roll-up is drawn`, async () => digits(await total.textContent({ timeout: 1000 })) === g.total);
      await nav(page).locator(`[data-nav-group="${g.shortcode}"] [data-nav-toggle]`).first().click({ timeout: 2000 });
      await eventually(`${g.shortcode} is open`, async () => (await nav(page).locator(`[data-nav-group="${g.shortcode}"] [data-nav-row]`).count()) > 0, 5_000);
      const asked = (await sentCalls(page)).length;
      const [made] = sy("tickets", "create", "--epic", g.epic, "--parent", g.root, "--title", "a child the roll-up counts");
      assert(made?.ticket?.id, `made: ${JSON.stringify(made)}`);
      await eventually(`the roll-up total reads ${g.total + 1}`, async () => digits(await total.textContent({ timeout: 1000 })) === g.total + 1, 3_000);
      const calls = (await sentCalls(page)).slice(asked);
      assert(!calls.some((c) => c.method === "tickets.nav.groups"), "no groups call: group_changed did it");
    });

    await test("the_nav_switches_tree_and_flat", async () => {
      const expected = summaries("--limit", String(PAGE)).map((t) => t.id);
      const { page } = await openPage();
      await eventually("Tree is drawn", async () => (await nav(page).locator("[data-nav-group]").count()) > 0);
      const asked = (await sentCalls(page)).length;
      await nav(page).locator("[data-nav-mode=flat]").click({ timeout: 2000 });
      const flat = nav(page).locator("[data-nav-flat-row]");
      await eventually("Flat draws its rows", async () => (await flat.count()) > 0, 5_000);
      await sleep(300);
      const lists = (await sentCalls(page)).slice(asked).filter((c) => c.method === "tickets.list");
      assert(lists.length === 1, `Flat is one list (${lists.length}: ${JSON.stringify(lists.map((l) => l.params))})`);
      const p = lists[0].params;
      assert(p.summary === true && p.limit === PAGE && p.parent == null && p.after == null && (p.filter ?? null) === null, `Flat's list is { filter, summary, limit: ${PAGE} }: ${JSON.stringify(p)}`);
      assert((await nav(page).locator("[data-nav-group]").count()) === 0, "Flat draws no shortcode rows");
      const shown = await attrs(flat, "data-nav-flat-row");
      assert(JSON.stringify(shown) === JSON.stringify(expected.slice(0, shown.length)) && shown.length === Math.min(PAGE, expected.length), `Flat's rows are the store's order, one page (${shown.length})`);
      await nav(page).locator("[data-nav-mode=tree]").click({ timeout: 2000 });
      await eventually("Tree again", async () => (await nav(page).locator("[data-nav-group]").count()) > 0);
    });

    await test("a_ticket_page_draws_the_body_without_scripts", async () => {
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

      const { page } = await openPage(`#/ticket/${t.id}`);
      const tp = page.locator("ticket-page");
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
      for (const not of ["__ranScript", "background"]) assert(!text.includes(not), `the body does not show the dropped "${not}"`);
      assert(trimmed(await bodyEl.locator("strong").textContent({ timeout: 1000 })) === "weight", "the markdown is drawn (**weight** is strong)");
      assert((await bodyEl.locator("li").count()) === 2, "the list is drawn");
      await bodyEl.locator("a").first().click({ timeout: 1000 }).catch(() => {});
      await sleep(300);
      const ran = await page.evaluate(() => {
        const b = document.querySelector("ticket-page [data-ticket-body]");
        const all = [...(b?.querySelectorAll("*") ?? [])];
        return {
          script: window.__ranScript === true,
          handler: window.__ranHandler === true,
          link: window.__ranLink === true,
          bg: getComputedStyle(document.body).backgroundColor,
          banned: b?.querySelectorAll("script, style, iframe").length ?? -1,
          onAttrs: all.flatMap((e) => [...e.attributes].filter((a) => a.name.toLowerCase().startsWith("on")).map((a) => `${e.tagName}.${a.name}`)),
          jsUrls: all.flatMap((e) => [...e.attributes].filter((a) => /^\s*javascript:/i.test(a.value)).map((a) => `${e.tagName}.${a.name}`)),
        };
      });
      assert(!ran.script && !ran.handler && !ran.link, `nothing in the body ran: ${JSON.stringify(ran)}`);
      assert(ran.bg !== "rgb(1, 2, 3)", `the body's style did not apply (${ran.bg})`);
      assert(ran.banned === 0, `no <script>, <style> or <iframe> in the body (${ran.banned})`);
      assert(ran.onAttrs.length === 0 && ran.jsUrls.length === 0, `no on* attribute, no javascript: URL: ${JSON.stringify(ran)}`);
      for (const [chip, other] of [["parent", parent], ["blocker", blocker]]) {
        const c = tp.locator(`[data-chip=${chip}][data-chip-id="${other.id}"]`);
        assert((await c.count()) === 1, `a ${chip} chip for ${other.id}`);
        await eventually(`the ${chip} chip resolves to "${other.title}"`, async () => trimmed(await c.locator("[data-chip-title]").textContent({ timeout: 1000 })) === other.title.trim(), 3_000);
      }

      // set_status from the menu.
      const to = t.status === "Ready" ? "Pending" : "Ready";
      await tp.locator("select[data-status-menu]").selectOption(to, { timeout: 2000 });
      await eventually(`the server has ${t.id} at ${to}`, async () => sy("tickets", "get", "--id", t.id).find((e) => e.type === "ticket")?.ticket?.status === to, 3_000);
      await eventually(`the page shows ${to}`, async () => trimmed(await tp.locator("[data-ticket-status]").textContent({ timeout: 1000 })) === to, 3_000);

      // The blocker's page, by the hash: it unlocks the ticket.
      const origin = await marked(page);
      await page.evaluate((id) => (location.hash = `#/ticket/${id}`), blocker.id);
      await eventually("the blocker's page draws", async () => trimmed(await tp.locator("[data-ticket-title]").textContent({ timeout: 1000 })) === blocker.title.trim());
      const u = tp.locator(`[data-chip=unlocks][data-chip-id="${t.id}"]`);
      await eventually(`an unlocks chip for ${t.id}, resolved`, async () => trimmed(await u.locator("[data-chip-title]").textContent({ timeout: 1000 })) === t.title.trim(), 3_000);
      await sameDocument(page, origin, "after the hash moved");
    });

    await test("the_palette_finds_and_searches", async () => {
      const groups = groupsNow().filter((g) => g.root);
      const [made] = sy("tickets", "create", "--epic", groups.at(-1).epic, "--parent", groups.at(-1).root, "--title", "the map-12 lantern");
      assert(made?.ticket?.id, `the lantern was made: ${JSON.stringify(made)}`);
      const query = "map-12";
      const found = sy("tickets", "find", "--query", query).filter((e) => e.type === "summary").map((e) => e.summary.id);
      const hits = sy("tickets", "search", "--query", query).filter((e) => e.type === "hit").map((e) => e.summary.id);
      const foundMap = sy("tickets", "find", "--query", "map").filter((e) => e.type === "summary").map((e) => e.summary.id);
      assert(found.length > 0 && hits.length > 0, `find and search answer ${query} (${found.length}, ${hits.length})`);
      assert(JSON.stringify(foundMap) !== JSON.stringify(found), "map and map-12 find different tickets (else newest-only proves nothing)");

      const { page } = await openPage();
      await boardUp(page);
      await page.locator("body").click({ position: { x: 5, y: 5 } }).catch(() => {});
      await page.keyboard.press("/");
      const input = palette(page).locator("input[data-palette-input]");
      await eventually("`/` focuses the palette's field", async () => input.evaluate((e) => e === document.activeElement), 2_000);

      // Typed at once: one find, after the pause and within its ceiling.
      let asked = (await sentCalls(page)).length;
      await page.keyboard.type(query, { delay: 20 });
      await eventually("the find hits are drawn", async () => (await palette(page).locator("[data-palette-hit]").count()) > 0, 3_000);
      await sleep(500);
      const lastKey = await page.evaluate(() => window.__pages.lastKey);
      let calls = (await sentCalls(page)).slice(asked);
      const finds = calls.filter((c) => c.method === "tickets.find");
      assert(finds.length === 1, `one find (${finds.length}: ${JSON.stringify(finds.map((f) => f.params.query))})`);
      assert(finds[0].params.query === query, `the find asks the whole query: ${JSON.stringify(finds[0].params)}`);
      const pause = finds[0].t - lastKey;
      assert(pause >= QUIET_MS && pause <= QUIET_CEIL_MS, `the find ${QUIET_MS}–${QUIET_CEIL_MS} ms after the last key (${pause.toFixed(0)} ms)`);
      const searches = calls.filter((c) => c.method === "tickets.search");
      assert(searches.length === 1 && searches[0].params.query === query, `one search, for the whole query: ${JSON.stringify(searches.map((s) => s.params.query))}`);
      const first = await palette(page).locator("[data-palette-hit]").first().getAttribute("data-palette-hit");
      assert(first === found[0], `the find's first hit is first (${first} vs ${found[0]})`);
      const searched = await attrs(palette(page).locator("[data-search-hit]"), "data-search-hit");
      assert(searched.includes(hits[0]), `search's best hit is shown below (${hits[0]} in ${JSON.stringify(searched)})`);

      // Newest only: `map`, a pause long enough to ask it, then `-12`.
      await input.fill("");
      await sleep(400);
      asked = (await sentCalls(page)).length;
      await input.focus();
      await page.keyboard.type("map", { delay: 20 });
      await sleep(150);
      await page.keyboard.type("-12", { delay: 20 });
      await sleep(800);
      calls = (await sentCalls(page)).slice(asked).filter((c) => c.method === "tickets.find");
      assert(calls.at(-1)?.params.query === query, `the last find is ${query}: ${JSON.stringify(calls.map((c) => c.params.query))}`);
      const drawn = await attrs(palette(page).locator("[data-palette-hit]"), "data-palette-hit");
      assert(JSON.stringify(drawn) === JSON.stringify(found.slice(0, drawn.length)) && drawn.length > 0, `only ${query}'s hits are drawn: ${JSON.stringify(drawn)} vs ${JSON.stringify(found)}`);
    });

    await test("the_filter_line_filters_the_board_and_the_nav", async () => {
      const filter = "status Ready";
      const expected = summaries("--filter", filter, "--limit", String(PAGE)).map((t) => t.id);
      const { page } = await openPage();
      await boardUp(page);
      const asked = (await sentCalls(page)).length;
      const input = page.locator("[data-layout=board-layout] input[data-filter-input]");
      await input.fill(filter, { timeout: 2000 });
      await input.press("Enter");
      await eventually("the route carries the filter", async () => (await page.evaluate(() => location.hash)) === "#/f/status%20Ready", 2_000);
      await eventually("the board shows the filtered rows", async () => {
        const shown = await rowIds(main(page).locator(".board-row[data-id]"));
        return shown.length > 0 && JSON.stringify(shown) === JSON.stringify(expected.slice(0, shown.length));
      }, 5_000);
      const calls = (await sentCalls(page)).slice(asked);
      assert(calls.some((c) => c.method === "tickets.list" && c.params.filter === filter && c.params.parent == null), `the board's list carries the filter: ${JSON.stringify(calls.filter((c) => c.method === "tickets.list").map((c) => c.params))}`);
      assert(calls.some((c) => c.method === "tickets.nav.counts" && c.params.filter === filter), `nav counts carries the filter: ${JSON.stringify(calls.map((c) => c.method))}`);

      const input2 = page.locator("[data-layout=board-layout] input[data-filter-input]");
      await input2.fill("text foo", { timeout: 2000 });
      await input2.press("Enter");
      const refused = page.locator("[data-layout=board-layout] [data-filter-refused]");
      await eventually("the refused chunk is shown under the field", async () => trimmed(await refused.textContent({ timeout: 1000 })).length > 0, 5_000);
      const said = await refused.textContent();
      assert(/text/.test(said), `the refusal names the chunk: ${JSON.stringify(said)}`);
    });

    await test("sign_in_returns_to_the_route", async () => {
      const t = summaries("--limit", "3")[2];
      const { page, context } = await openPage(`#/ticket/${t.id}`, { signedIn: false });
      await eventually("no session: the login page", async () => new URL(page.url()).pathname === "/login", 5_000);
      assert(new URL(page.url()).hash === `#/ticket/${t.id}`, `the route rides along: ${page.url()}`);
      await context.addCookies(sessionCookies(server.origin, token));
      await page.reload();
      await eventually(`signed in: back on #/ticket/${t.id}`, async () => {
        const u = new URL(page.url());
        return u.pathname === "/app/" && u.hash === `#/ticket/${t.id}`;
      }, 5_000);
      await eventually("the ticket is drawn", async () => trimmed(await page.locator("ticket-page [data-ticket-title]").textContent({ timeout: 1000 })) === t.title.trim(), 5_000);
    });

    await test("a_section_of_the_board_is_swapped_by_right_click", async () => {
      const { page, seen } = await openPage();
      await eventually("v2's board draws", async () => (await main(page).locator(".board-newer").count()) === 1);
      await boardUp(page);
      await eventually("the nav's rows", async () => (await nav(page).locator("[data-nav-group]").count()) > 0);
      const origin = await marked(page);
      await page.evaluate(() => (document.querySelector("[data-layout=board-layout][data-slot=nav] nav-tree").__kept = true));
      const asked = (await sentCalls(page)).length;

      await main(page).locator(".board-row[data-id]").first().click({ button: "right" });
      const menu = page.locator("[data-engine-menu]");
      await eventually("the engine's menu opens on the board", async () => menu.isVisible());
      await eventually("the menu offers the older board", async () => (await menu.locator(`[data-version="${v1.version}"]`).count()) === 1, 3_000);
      assert((await menu.locator(`[data-version="${v2.version}"]`).count()) === 0, "the live version is not offered");
      const fetchedBefore = seen.requests.length;
      await menu.locator(`[data-version="${v1.version}"]`).click();
      await eventually("the older board draws", async () => (await main(page).locator(".board-newer").count()) === 0 && (await main(page).locator("board-rows").count()) === 1);
      await boardUp(page);
      const fetched = seen.requests.slice(fetchedBefore);
      assert(fetched.some((u) => u.endsWith(`/app/${v1.version}/components/board-rows.json`)), `v1's board-rows.json was fetched: ${fetched.join(" ")}`);
      const kept = await page.evaluate(() => document.querySelector("[data-layout=board-layout][data-slot=nav] nav-tree")?.__kept === true);
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
