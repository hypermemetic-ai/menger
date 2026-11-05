// V2-42 (TICKETS-35 and TICKETS-36 on v2): a ticket's link is its uid.
// `#/t/<uid>/<ID>-<slug>` opens the ticket by its uid whatever its id is now
// (page-t: `[data-surface=ticket-by-uid][data-uid]`, the ticket's own page
// inside it, `[data-not-found]` for a uid naming nothing); an old link,
// `#/ticket/<ID>` or `#/edit/<ID>`, still opens and the address bar is put
// in the uid form in place (no history entry); a moved ticket's old link
// lands on it and its tail is put right; the links the UI makes are the
// uid form (a search row, a parent chip), from one helper (tests/links_check.mjs).
import { checkDir, helperCases } from "../links_check.mjs";

export default async (ctx) => {
  const { test, assert, eventually, openPage, sy, groupsNow, path } = ctx;
  const src = path.join(path.dirname(new URL(import.meta.url).pathname), "..", "..", "ui", "src");
  const hash = (page) => page.evaluate(() => location.hash);
  const text = async (loc) => String((await loc.first().textContent({ timeout: 1000 })) ?? "").trim();
  let n = 0;
  const root = () => groupsNow().filter((g) => g.root && !g.shortcode.startsWith("AAA")).at(-1);
  /** A fresh ticket under the last shortcode's root (or `parent`): its row, as `create` answers it. */
  function made(title, parent = null) {
    const g = root();
    const [m] = sy("tickets", "create", "--epic", g.epic, "--parent", parent ?? g.root, "--title", `${title} ${++n}`, "--kind", "build");
    assert(m?.ticket?.id && /^[0-9a-z]{8}$/.test(m.ticket.uid ?? ""), `made "${title}" with a uid: ${JSON.stringify(m)}`);
    return m.ticket;
  }
  const head = (page) => page.locator("[data-surface=ticket-page] [data-surface=ticket-head]");
  const shownId = async (page) => text(head(page).locator("[data-field=id]"));

  await test("no_ticket_link_is_built_outside_the_helper", async () => {
    const refused = checkDir(src);
    assert(refused.length === 0, `tests/links_check.mjs refuses:\n  ${refused.join("\n  ")}`);
  });

  await test("the_helper_makes_the_uid_link", async () => {
    const bad = await helperCases(src);
    assert(bad.length === 0, `ticket-links.svelte:\n  ${bad.join("\n  ")}`);
  });

  await test("a_uid_route_opens_its_ticket", async () => {
    const t = made("the kiln by its uid");
    const slugged = `#/t/${t.uid}/${t.id}-the-kiln-by-its-uid-${n}`;
    for (const [route, settles] of [[slugged, slugged], [`#/t/${t.uid}`, slugged], [`#/t/${t.uid}/WRONG-9-a-stale-tail`, slugged]]) {
      const { context, page } = await openPage(route);
      try {
        await eventually(`${route} draws ${t.id}`, async () => (await shownId(page)) === t.id, 10_000);
        assert((await page.locator(`[data-surface=ticket-by-uid][data-uid="${t.uid}"]`).count()) === 1, "page-t holds the page");
        await eventually(`${route}: the address is ${settles}`, async () => (await hash(page)) === settles, 5_000);
      } finally {
        await context.close();
      }
    }
    const { context, page } = await openPage("#/t/zzzzzzzz/NONE-1");
    try {
      await eventually("a uid naming nothing says so", async () => (await page.locator("[data-surface=ticket-by-uid] [data-not-found]").count()) === 1, 10_000);
    } finally {
      await context.close();
    }
  });

  await test("an_old_route_resolves_and_is_rewritten_to_the_uid_form", async () => {
    const t = made("an old link's ticket");
    const want = `#/t/${t.uid}/${t.id}-an-old-links-ticket-${n}`;
    const { context, page } = await openPage("#/board");
    try {
      await page.evaluate((id) => (location.hash = `#/ticket/${id}`), t.id);
      await eventually(`#/ticket/${t.id} draws it`, async () => (await shownId(page)) === t.id, 10_000);
      await eventually(`the address becomes ${want}`, async () => (await hash(page)) === want, 5_000);
      // In place: back goes to the board, not to the old link.
      await page.evaluate(() => history.back());
      await eventually("back is the board: the rewrite made no history entry", async () => (await hash(page)) === "#/board", 5_000);
      await page.evaluate(() => history.forward());
      await eventually("forward is the uid form", async () => (await hash(page)) === want, 5_000);
      // The editor's old route too: `#/edit/<ID>` → `#/t/<uid>/<ID>-<slug>/edit`, the editor on it.
      await page.evaluate((id) => (location.hash = `#/edit/${id}`), t.id);
      await eventually("the editor opens it", async () => (await page.locator(`[data-surface=editor-screen][data-ticket="${t.id}"]`).count()) === 1, 10_000);
      await eventually(`the address becomes ${want}/edit`, async () => (await hash(page)) === `${want}/edit`, 5_000);
      // And the editor's uid route opens the editor by the uid.
      await page.goto("about:blank");
      await page.goto(`${ctx.server.origin}/app/${want}/edit`);
      await eventually("#/t/<uid>/…/edit: the editor on it", async () => (await page.locator(`[data-surface=editor-screen][data-ticket="${t.id}"]`).count()) === 1, 10_000);
      const link = page.locator("[data-surface=editor-screen] a[data-field=current]");
      await eventually("its link to the page is the uid form", async () => (await link.getAttribute("href")) === want, 5_000);
    } finally {
      await context.close();
    }
  });

  await test("a_moved_ticket_s_old_link_still_lands", async () => {
    const parent = made("a parent to move under");
    const t = made("moved under its parent", parent.id);
    const old = `#/t/${t.uid}/${t.id}-moved-under-its-parent-${n}`;
    const [moved] = sy("tickets", "rename", "--id", t.id, "--to", `${parent.id}.1`);
    assert(moved?.type === "renamed" || moved?.tickets || moved?.ticket, `renamed: ${JSON.stringify(moved)}`);
    const [now] = sy("tickets", "get", "--id", t.uid);
    assert(now?.ticket?.id === `${parent.id}.1` && now.ticket.uid === t.uid, `the uid kept through the move: ${JSON.stringify(now)}`);
    const { context, page } = await openPage(old);
    try {
      await eventually(`its old link draws it as ${parent.id}.1`, async () => (await shownId(page)) === `${parent.id}.1`, 10_000);
      const want = `#/t/${t.uid}/${parent.id}.1-moved-under-its-parent-${n}`;
      await eventually(`the stale tail put right: ${want}`, async () => (await hash(page)) === want, 5_000);
    } finally {
      await context.close();
    }
  });

  await test("the_links_the_ui_makes_are_by_uid", async () => {
    const parent = made("zorbleq a parent");
    const t = made("zorbleq a row the search links", parent.id);
    const { context, page } = await openPage("#/search/zorbleq");
    try {
      const row = page.locator(`[data-result=find][data-id="${t.id}"]`);
      await eventually(`the search finds ${t.id}`, async () => (await row.count()) > 0, 10_000);
      const href = await row.first().getAttribute("href");
      assert(href === `#/t/${t.uid}/${t.id}-zorbleq-a-row-the-search-links-${n}`, `the row's link: ${href}`);
      // The ticket's page: its parent chip by the parent's uid.
      await row.first().click();
      await eventually(`${t.id}'s page`, async () => (await shownId(page)) === t.id, 10_000);
      const chip = head(page).locator(`[data-chip=parent][data-chip-id="${parent.id}"]`);
      await eventually("the parent chip links by its uid", async () => ((await chip.getAttribute("href")) ?? "").startsWith(`#/t/${parent.uid}/${parent.id}-`), 5_000);
    } finally {
      await context.close();
    }
  });
};
