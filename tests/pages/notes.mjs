// A note written from anywhere (note-quick, placed by app-top's bound mark):
// ctrl+shift+l opens a small window on any page with the top bar — the global
// keys' chord (keys-global emits `note.open`), ctrl+alt+n too, and the bar's
// `note` button; in a field and in the editor as well. ctrl+enter keeps it
// as a doc (`docs.create { title, body }`, the first line its title when
// none is typed), escape closes it and keeps nothing. Run alone by
// `node tests/pages_browser.mjs --group notes`.
//
// Marks: `[data-surface=note-quick]`, `[data-note-open]` (the window),
// `[data-field=note-title]`, `[data-field=note-body]`, `[data-action="note.keep"]`,
// `[data-note-kept=ID]` (the word that it was kept, linking the note),
// `[data-refused]`, and app-top's `[data-action="note.open"]`.
import { sentCalls } from "../../scripts/ui-throwaway.mjs";

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy } = ctx;
  const doc = (id) => sy("docs", "get", "--id", id).find((e) => e.type === "doc")?.doc;
  const win = (page) => page.locator("[data-surface=note-quick] [data-note-open]");

  await test("a_note_is_written_from_any_page", async () => {
    // The board (board-frame) and the writing screen (page-editor): two layouts, one top bar each.
    for (const route of ["#/board", "#/"]) {
      const { page, context } = await openPage(route);
      await page.locator("[data-surface=app-top]").first().waitFor({ timeout: 15_000 });
      await sleep(300);
      // Each of the note's chords opens it; escape closes it.
      await page.keyboard.press("Control+Alt+n");
      await eventually(`${route}: ctrl+alt+n opens the note window`, async () => (await win(page).count()) === 1, 3_000);
      await page.keyboard.press("Escape");
      await eventually(`${route}: escape closes it`, async () => (await win(page).count()) === 0, 2_000);
      // `g n`, plain keys outside a text field: the chord no browser takes.
      await page.locator("body").click({ position: { x: 5, y: 5 } }).catch(() => {});
      await page.keyboard.press("g");
      await page.keyboard.press("n");
      await eventually(`${route}: g n opens the note window`, async () => (await win(page).count()) === 1, 3_000);
      await page.keyboard.press("Escape");
      await eventually(`${route}: escape closes it`, async () => (await win(page).count()) === 0, 2_000);
      await page.keyboard.press("Control+Shift+L");
      await eventually(`${route}: ctrl+shift+l opens the note window`, async () => (await win(page).count()) === 1, 3_000);
      const body = win(page).locator("[data-field=note-body]");
      assert(await body.evaluate((e) => e === document.activeElement), `${route}: the note's text has the focus`);
      const keep = win(page).locator("[data-action=\"note.keep\"]");
      assert(await keep.isDisabled(), `${route}: an empty note cannot be kept`);

      // Escape closes it and keeps nothing.
      await body.fill("thrown away");
      await page.keyboard.press("Escape");
      await eventually(`${route}: escape closes it`, async () => (await win(page).count()) === 0, 2_000);
      assert(!(await sentCalls(page)).some((c) => c.method === "docs.create"), `${route}: escape keeps nothing`);

      // Opened again it is empty; the first line is the title; ctrl+enter keeps it.
      await page.keyboard.press("Control+Shift+L");
      await eventually(`${route}: opened again`, async () => (await win(page).count()) === 1, 2_000);
      assert((await body.inputValue()) === "", `${route}: opened again, the note is empty`);
      const text = `# kiln note ${route}\n\nfire twice a week`;
      await body.fill(text);
      await page.keyboard.press("Control+Enter");
      await eventually(`${route}: kept, the window closes`, async () => (await win(page).count()) === 0, 5_000);
      const kept = page.locator("[data-note-kept]");
      await eventually(`${route}: it says the note was kept`, async () => (await kept.count()) === 1, 2_000);
      const id = await kept.getAttribute("data-note-kept");
      const d = doc(id);
      assert(d && d.title === `kiln note ${route}`, `${route}: kept under its first line: ${JSON.stringify(d?.title)}`);
      assert(d.body === text, `${route}: its body is the note as typed: ${JSON.stringify(d.body)}`);
      assert((await kept.locator("a").getAttribute("href")) === `#/doc/${id}`, `${route}: the word links the note`);
      await context.close();
    }
  });

  // The app has one address: the bare one (`/#/board`) goes to `/app/#/board`, where the keys are bound.
  await test("the_bare_address_goes_to_the_app", async () => {
    const { page, context } = await openPage("#/board");
    await page.goto(`${ctx.server.origin}/#/board`);
    await page.locator("[data-surface=app-top]").first().waitFor({ timeout: 15_000 });
    await sleep(500);
    const at = await page.evaluate(() => location.pathname + location.hash);
    assert(at === "/app/#/board", `the bare address goes to /app/, the route kept: ${at}`);
    await page.keyboard.press("g");
    await page.keyboard.press("n");
    await eventually("g n opens the note window", async () => (await win(page).count()) === 1, 3_000);
    await context.close();
  });

  await test("a_note_has_its_typed_title_and_opens_from_the_bar", async () => {
    const { page, context } = await openPage("#/board");
    await page.locator("[data-surface=app-top] [data-action=\"note.open\"]").first().click({ timeout: 15_000 });
    await eventually("the bar's note button opens the window", async () => (await win(page).count()) === 1, 3_000);
    await win(page).locator("[data-field=note-title]").fill("glaze list");
    await win(page).locator("[data-field=note-body]").fill("celadon\ntenmoku");
    await win(page).locator("[data-action=\"note.keep\"]").click();
    const kept = page.locator("[data-note-kept]");
    await eventually("kept", async () => (await kept.count()) === 1, 5_000);
    const d = doc(await kept.getAttribute("data-note-kept"));
    assert(d?.title === "glaze list" && d.body === "celadon\ntenmoku", `the typed title and the text: ${JSON.stringify(d)}`);
    await context.close();
  });

  await test("a_note_opens_from_a_field_and_from_the_editor", async () => {
    const id = sy("docs", "create", "--title", "Kiln log").find((e) => e.type === "created").doc.id;
    const { page, context } = await openPage(`#/doc/${id}`);
    const text = page.locator(".cm-content[data-field=body-source]");
    await text.waitFor({ timeout: 15_000 });
    await sleep(300);
    // The caret in the editor's text, then in the top's search box: the chord is the host's in both.
    for (const [where, field] of [["the editor", text], ["the search box", page.locator("[data-surface=app-top] [data-form=search] input")]]) {
      await field.click();
      await page.keyboard.press("Control+Shift+L");
      await eventually(`ctrl+shift+l in ${where} opens the note window`, async () => (await win(page).count()) === 1, 3_000);
      assert(await win(page).locator("[data-field=note-body]").evaluate((e) => e === document.activeElement), `from ${where}, the note's text has the focus`);
      await page.keyboard.press("Escape");
      await eventually(`escape closes it (${where})`, async () => (await win(page).count()) === 0, 2_000);
    }
    assert(!(await sentCalls(page)).some((c) => c.method === "docs.create"), "nothing was kept");
    await context.close();
  });
};
