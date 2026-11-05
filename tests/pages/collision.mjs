// V2-50: main's TICKETS-36 collision checks (scripts/collision-check.{sh,mjs}
// at 2922ae5e), ported to v2's pages. Two shortcodes named CLX in two
// namespaces — CLA/CLB's CLX-2 (keyed CLX-2) beside CLC's (keyed CLX.N-2),
// synthetic — and a reader of CLA's tree alone:
//
//   - #/ticket/CLX-2 is the collision page (ticket-collision, in page-ticket's
//     place): two rows in the store's order, each its address (hosts, id),
//     kind, status, title and parent — a link by uid — and its own link by
//     uid; the first chosen; no horizontal scroll at 1440 or 390.
//   - ↓ ↑ j k choose, wrapping; c copies the chosen link; ⏎ opens it at
//     #/t/<uid>/CLX-2-…; Back is the collision page again; a click on the
//     other row opens it; Esc goes to the board.
//   - Copy link on the ticket's page, and in its actions, is the permalink
//     <origin>/t/<uid>/<ID>-<slug>.
//   - #/ticket/<full address> is replaced by its #/t/… (no history entry);
//     #/ticket/NOPE-1 and #/t/<not a uid> are not found; a stale slug is
//     rewritten, warm and cold (the uid in capitals).
//   - The boards' rows for the original link by its uid — the Svelte board's
//     and the rhx twin's.
//   - The shell: /CLX-2 lands on the collision page, /t/<uid>/x on the page.
//   - A reader of CLA's tree alone, asking for CLX-2, is taken to the one
//     ticket the name fits for them.
//
// Marks: ticket-collision's (`[data-surface=ticket-collision][data-asked]`,
// `[data-heading]`, `li[data-choice]` with `[data-field=hosts|id|kind|status|
// title|parent|place|link]`, `[data-action=copy-link]`, `[data-said]`).
import path from "node:path";
import { makeKey, sessionCookies, writeSession } from "../../scripts/throwaway-login.mjs";

export default async (ctx) => {
  const { test, assert, eventually, openPage, sy, server, session } = ctx;
  const hash = (page) => page.evaluate(() => location.hash);
  const text = async (loc) => String((await loc.first().textContent({ timeout: 1000 })) ?? "").replace(/\s+/g, " ").trim();
  const one = (events, what) => {
    const t = events.find((e) => e.ticket)?.ticket;
    assert(t?.id, `${what}: ${JSON.stringify(events)}`);
    return t;
  };
  const make = (what, ...flags) => one(sy("tickets", "create", ...flags), what);

  // The two namesakes (synthetic), and both Ready so the boards show them.
  const cla = make("CLA's root", "--epic", "CLA", "--kind", "scope", "--title", "collisions, the first place");
  const clb = make("CLB under it", "--epic", "CLB", "--kind", "scope", "--title", "the store and the server", "--parent", cla.id);
  const p1 = make("CLX's root under CLB", "--epic", "CLX", "--kind", "scope", "--title", "Magnitude — how menger degrades", "--parent", clb.id);
  const orig = make("the original CLX-2", "--epic", "CLX", "--title", "Compression on the WebSocket", "--parent", p1.id);
  const clc = make("CLC's root", "--epic", "CLC", "--kind", "scope", "--title", "collisions, the other place");
  const p2 = make("CLX's namesake root under CLC", "--epic", "CLX", "--kind", "scope", "--title", "Self-description cached", "--parent", clc.id);
  const name = make("the namesake's CLX-2", "--epic", `CLC/CLX`, "--kind", "gap", "--title", "the schema answers 3.24 MB", "--parent", p2.id);
  for (const t of [orig, name]) sy("tickets", "set_status", "--id", t.uid, "--status", "Ready");
  const ORIG = `#/t/${orig.uid}/CLX-2-compression-on-the-websocket`;
  const NAMESAKE = `#/t/${name.uid}/CLX-2-the-schema-answers-3-24-mb`;
  const coll = (page) => page.locator("[data-surface=ticket-collision]");
  const rows = (page) => coll(page).locator("li[data-choice]");
  const title = (page) => text(page.locator("[data-surface=ticket-page] [data-surface=ticket-head] [data-field=title]"));
  const chosen = (page) => rows(page).evaluateAll((els) => els.findIndex((e) => e.getAttribute("aria-selected") === "true"));
  const overflow = (page) => page.evaluate(() => document.scrollingElement.scrollWidth - innerWidth);

  await test("the_seed_names_two_tickets_clx_2", async () => {
    assert(orig.id === "CLX-2", `the original is keyed CLX-2: ${orig.id}`);
    assert(/^CLX\.\d+-2$/.test(name.id), `the namesake is keyed CLX.N-2: ${name.id}`);
    const [amb] = sy("tickets", "get", "--id", "CLX-2");
    assert(amb?.type === "ambiguous" && amb.choices?.length === 2, `get --id CLX-2 is ambiguous with two choices: ${JSON.stringify(amb)}`);
  });

  await test("a_name_that_fits_two_is_the_collision_page", async () => {
    for (const width of [1440, 390]) {
      const { context, page } = await openPage("#/ticket/CLX-2");
      try {
        await page.setViewportSize({ width, height: width < 800 ? 844 : 900 });
        await eventually("#/ticket/CLX-2 draws the collision page", async () => (await rows(page).count()) === 2, 15_000);
        assert((await coll(page).getAttribute("data-asked")) === "CLX-2", "it says the name asked");
        assert((await text(coll(page).locator("[data-heading]"))) === "names two tickets", "the heading says two");
        assert((await text(coll(page).locator("[data-lede]"))).startsWith("Two shortcodes are named CLX, each in its own place."), "the lede says why");
        const r = await rows(page).evaluateAll((els) => els.map((el) => {
          const f = (n) => el.querySelector(`[data-field=${n}]`);
          return {
            hosts: f("hosts")?.textContent.trim(), id: f("id")?.textContent.trim(), kind: f("kind")?.getAttribute("data-kind"),
            status: f("status")?.textContent.trim(), title: f("title")?.textContent.trim(),
            parent: f("parent")?.querySelector("a")?.getAttribute("href"), parentId: f("parent")?.querySelector("a")?.textContent.trim(),
            parentTitle: f("parent")?.querySelector(".pt")?.textContent.trim(), link: f("link")?.textContent.trim(),
            selected: el.getAttribute("aria-selected"), key: el.getAttribute("data-id"),
          };
        }));
        const [a, b] = r;
        assert(/\bCLA \/ CLB \/$/.test(a.hosts ?? "") && a.id === "CLX-2", `row 1 is CLA/CLB's CLX-2, its hosts muted: ${a.hosts} ${a.id}`);
        assert(/\bCLC \/$/.test(b.hosts ?? "") && b.id === "CLX-2", `row 2 is CLC's CLX-2: ${b.hosts} ${b.id}`);
        assert(a.kind === "build" && a.status === "Ready" && a.title === "Compression on the WebSocket", `row 1's kind, status, title: ${JSON.stringify(a)}`);
        assert(b.kind === "gap" && b.status === "Ready" && b.title === "the schema answers 3.24 MB", `row 2's kind, status, title: ${JSON.stringify(b)}`);
        assert(a.parent === `#/t/${p1.uid}/CLX-1-magnitude-how-menger-degrades` && a.parentId === "CLX-1" && a.parentTitle === "Magnitude — how menger degrades", `row 1's parent, by uid: ${a.parent}`);
        assert(b.parent === `#/t/${p2.uid}/CLX-1-self-description-cached` && b.parentTitle === "Self-description cached", `row 2's parent, by uid: ${b.parent}`);
        assert(a.link === `/t/${orig.uid}/CLX-2-compression-on-the-websocket` && b.link === `/t/${name.uid}/CLX-2-the-schema-answers-3-24-mb`, `each row's link by its uid: ${a.link} · ${b.link}`);
        assert(a.selected === "true" && b.selected === "false", "the first row is chosen");
        assert(a.key === orig.id && b.key === name.id, `each row is keyed by its ticket: ${a.key}, ${b.key}`);
        assert((await overflow(page)) <= 0, `no horizontal scroll at ${width}: ${await overflow(page)} px over`);
        const linkShown = await rows(page).first().locator("[data-field=link]").isVisible();
        assert(width < 800 ? !linkShown : linkShown, `the link line ${width < 800 ? "is hidden on a phone" : "shows on a desktop"}`);
        // COLLISION_SHOTS=DIR: the page at this width, light and dark, for the ticket.
        if (process.env.COLLISION_SHOTS) {
          for (const scheme of ["light", "dark"]) {
            await page.emulateMedia({ colorScheme: scheme });
            await page.waitForTimeout(300);
            await page.screenshot({ path: path.join(process.env.COLLISION_SHOTS, `collision-${width}-${scheme}.png`), fullPage: width < 800 });
          }
        }
      } finally {
        await context.close();
      }
    }
  });

  await test("the_collision_page_s_keys_choose_copy_open_and_go_back", async () => {
    const { context, page } = await openPage("#/board");
    try {
      await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin: server.origin });
      await page.evaluate(() => (location.hash = "#/ticket/CLX-2"));
      await eventually("the collision page", async () => (await rows(page).count()) === 2, 15_000);
      await coll(page).locator("[data-heading]").click();
      await page.keyboard.press("ArrowDown");
      await eventually("↓ chooses the second", async () => (await chosen(page)) === 1, 3_000);
      assert(await page.evaluate(() => document.activeElement?.getAttribute("data-choice") === "1"), "and focuses it");
      await page.keyboard.press("ArrowDown");
      await eventually("↓ past the last wraps to the first", async () => (await chosen(page)) === 0, 3_000);
      await page.keyboard.press("j");
      await eventually("j the next", async () => (await chosen(page)) === 1, 3_000);
      await page.keyboard.press("k");
      await eventually("k the previous", async () => (await chosen(page)) === 0, 3_000);
      await page.keyboard.press("c");
      const want = `${server.origin}/t/${orig.uid}/CLX-2-compression-on-the-websocket`;
      await eventually("c copies the chosen link, and says so", async () => (await text(coll(page).locator("[data-said]"))) === `copied ${want}`, 5_000);
      assert((await page.evaluate(() => navigator.clipboard.readText())) === want, "the clipboard holds it");
      await page.keyboard.press("Enter");
      await eventually(`⏎ opens it at its uid: ${ORIG}`, async () => (await hash(page)) === ORIG, 10_000);
      await eventually("and its page draws (⏎)", async () => (await title(page)) === "Compression on the WebSocket", 10_000).catch(async (e) => { throw new Error(`${e.message} | drawn: ${(await page.evaluate(() => document.body.innerText)).slice(0, 400)}`); });
      await page.goBack();
      await eventually("Back returns to the collision page", async () => (await hash(page)) === "#/ticket/CLX-2" && (await rows(page).count()) === 2, 10_000);
      await rows(page).nth(1).locator("[data-field=title]").click();
      await eventually(`a click opens the other at its uid: ${NAMESAKE}`, async () => (await hash(page)) === NAMESAKE, 10_000);
      await eventually("and its page draws (a click)", async () => (await title(page)) === "the schema answers 3.24 MB", 10_000).catch(async (e) => { throw new Error(`${e.message} | drawn: ${(await page.evaluate(() => document.body.innerText)).slice(0, 400)}`); });
      // Esc on the collision page: the board.
      await page.evaluate(() => (location.hash = "#/ticket/CLX-2"));
      await eventually("the collision page again", async () => (await rows(page).count()) === 2, 10_000);
      await coll(page).locator("[data-heading]").click();
      await page.keyboard.press("Escape");
      await eventually("Esc goes to the board", async () => (await hash(page)) === "#/board", 5_000);
    } finally {
      await context.close();
    }
  });

  await test("copy_link_is_the_permalink_by_uid", async () => {
    const { context, page } = await openPage(NAMESAKE);
    try {
      await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin: server.origin });
      await eventually("the page draws", async () => (await title(page)) === "the schema answers 3.24 MB", 10_000);
      const want = `${server.origin}/t/${name.uid}/CLX-2-the-schema-answers-3-24-mb`;
      const head = page.locator("[data-surface=ticket-head] [data-action=copy-link]");
      assert((await head.count()) === 1, "the page's head has Copy link");
      await head.click();
      await eventually("it says it copied", async () => (await text(head)) === "Copied", 5_000);
      assert((await page.evaluate(() => navigator.clipboard.readText())) === want, "Copy link is the permalink by uid");
      await page.evaluate(() => navigator.clipboard.writeText(""));
      const actions = page.locator("[data-surface=ticket-actions] [data-action=copy-link]");
      await eventually("its actions have Copy link too", async () => (await actions.count()) === 1, 10_000);
      await actions.click();
      await eventually("the actions' Copy link copies it", async () => (await page.evaluate(() => navigator.clipboard.readText())) === want, 5_000);
    } finally {
      await context.close();
    }
  });

  await test("names_resolve_to_the_uid_route_or_not_found", async () => {
    const { context, page } = await openPage("#/board");
    try {
      assert(/:CLA\/CLB\/CLX-2$/.test(orig.address ?? ""), `the original's full address: ${orig.address}`);
      const before = await page.evaluate((a) => { location.hash = `#/ticket/${a}`; return history.length; }, orig.address);
      await eventually(`#/ticket/${orig.address} becomes ${ORIG}`, async () => (await hash(page)) === ORIG, 10_000);
      assert((await page.evaluate(() => history.length)) === before, "in place: no history entry");
      await eventually("the address's page draws", async () => (await title(page)) === "Compression on the WebSocket", 10_000);
      await page.evaluate(() => (location.hash = "#/ticket/NOPE-1"));
      await eventually("#/ticket/NOPE-1 is not found", async () => (await text(page.locator("[data-surface=ticket-page] [data-not-found]"))).startsWith("No ticket NOPE-1 here"), 10_000);
      await page.evaluate(() => (location.hash = "#/t/CLX-2/x"));
      await eventually("#/t/<not a uid> is not found, never read as a name", async () => (await page.locator("[data-surface=ticket-by-uid] [data-not-found]").count()) === 1, 10_000);
      assert((await rows(page).count()) === 0, "no collision page for a route by uid");
      await page.evaluate((u) => (location.hash = `#/t/${u}/CLX-2-a-stale-title`), orig.uid);
      await eventually(`a stale slug is rewritten (warm): ${ORIG}`, async () => (await hash(page)) === ORIG, 10_000);
    } finally {
      await context.close();
    }
    const cold = await openPage(`#/t/${name.uid.toUpperCase()}/STORE-9-what-it-was-called`);
    try {
      await eventually(`a stale slug is rewritten (cold, the uid in capitals): ${NAMESAKE}`, async () => (await hash(cold.page)) === NAMESAKE, 15_000);
    } finally {
      await cold.context.close();
    }
  });

  await test("the_boards_rows_link_the_original_by_its_uid", async () => {
    const filter = encodeURIComponent(`under ${clb.id}`);
    for (const [route, row, b] of [[`#/f/${filter}`, `[data-layout=board-frame][data-slot=main] .board-row[data-id="${orig.id}"]`, "sv"], [`#/rhx/f/${filter}`, `[data-layout=board-layout][data-slot=main] .board-row[data-id="${orig.id}"]`, "rhx"]]) {
      const { context, page } = await openPage(route);
      try {
        const r = page.locator(row);
        await eventually(`the ${b} board shows ${orig.id}`, async () => (await r.count()) > 0, 15_000);
        const href = (await r.first().getAttribute("href")) ?? (await r.first().locator("a").first().getAttribute("href"));
        const ok = b === "sv" ? href === ORIG : href === `#/t/${orig.uid}/${orig.id}`;
        assert(ok, `the ${b} board's row for the original links by its uid: ${href}`);
        await r.first().click();
        await eventually(`and opens it (${b})`, async () => (await title(page)) === "Compression on the WebSocket", 10_000);
        await eventually(`its address settles on ${ORIG} (${b})`, async () => (await hash(page)) === ORIG, 10_000);
      } finally {
        await context.close();
      }
    }
  });

  await test("the_shell_sends_a_name_and_a_uid_link_into_the_app", async () => {
    const { context, page } = await openPage("#/board");
    try {
      await page.goto(`${server.origin}/CLX-2`);
      await eventually("/CLX-2 lands on the collision page", async () => (await rows(page).count()) === 2, 15_000);
      assert(new URL(page.url()).pathname === "/app/" && (await hash(page)) === "#/ticket/CLX-2", `at /app/#/ticket/CLX-2: ${page.url()}`);
      await page.goto(`${server.origin}/t/${name.uid}/x`);
      await eventually(`/t/<uid>/x opens its page, the slug rewritten: ${NAMESAKE}`, async () => (await hash(page)) === NAMESAKE, 15_000);
      await eventually("its page draws", async () => (await title(page)) === "the schema answers 3.24 MB", 10_000);
    } finally {
      await context.close();
    }
    const out = await openPage("", { signedIn: false });
    try {
      await out.page.goto(`${server.origin}/CLX-2`);
      assert(new URL(out.page.url()).pathname === "/login", `signed out: the sign-in page first: ${out.page.url()}`);
    } finally {
      await out.context.close();
    }
  });

  await test("a_reader_of_one_namespace_is_taken_to_its_ticket", async () => {
    const work = path.dirname(session);
    const keyFile = path.join(work, "clx-reader.key");
    const principal = makeKey(keyFile);
    const g = sy("members", "grant", "--principal", principal, "--scope", `under ${cla.id}`, "--role", "viewer");
    assert(g.some((e) => e.type === "grant"), `granted under ${cla.id}: ${JSON.stringify(g)}`);
    const file = path.join(work, "clx-reader.session");
    const s = await writeSession(`ws://127.0.0.1:${server.ws}`, keyFile, file, "clx-reader");
    const { context, page } = await openPage("", { signedIn: false });
    try {
      await context.clearCookies();
      await context.addCookies(sessionCookies(server.origin, s.token));
      await page.goto(`${server.origin}/app/#/ticket/CLX-2`);
      await eventually(`the name fits one ticket for them: ${ORIG}`, async () => (await hash(page)) === ORIG, 15_000);
      assert((await rows(page).count()) === 0, "no collision page");
    } finally {
      await context.close();
    }
  });
};
