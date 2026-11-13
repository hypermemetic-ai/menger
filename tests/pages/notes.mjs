// A note written from anywhere (note-quick, placed by app-top's bound mark):
// ctrl+alt+n (cmd+option+n on a Mac) opens a small window on any page with
// the top bar, the bar's `note` button too; ctrl+enter keeps it as a doc
// (`docs.create { title, body }`, the first line its title when none is
// typed), escape closes it and keeps nothing. Run alone by
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
      await page.keyboard.press("Control+Alt+n");
      await eventually(`${route}: ctrl+alt+n opens the note window`, async () => (await win(page).count()) === 1, 3_000);
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
      await page.keyboard.press("Control+Alt+n");
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
};
