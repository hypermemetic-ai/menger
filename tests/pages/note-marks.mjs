// NOTES-5: what an agent leaves on a note — a mark (`notes.mark`) on a range
// of its text: the words underlined on every open page (in the accent when
// the mark carries a link, which a click opens), its line of state in the
// mono face under the block that holds it, with who left it, and a dot in
// that block's margin; the underline follows its words as the note is
// typed; every word deleted, it is drawn nowhere. The note's text is never
// written by a mark. Run alone by `node tests/pages_browser.mjs --group note-marks`.
//
// Marks: a drawn block `[data-marked]`; its underline `.nm-u[data-mark=<id>]`
// (`.nm-linked`, `[data-mark-link]`, `role=link`); a line of state
// `[data-mark-state=<id>]`; in the caret's block CodeMirror's own
// `.nm-u[data-mark]`, the line `.nm-dot` and `.nm-states`.
//
// PAGES_SHOTS=DIR: a marked note at 1440 and 390, light and dark (`note-marked-*`).
export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, fs, path, session, ticketAt } = ctx;
  const BODY = "[data-surface=editor-body] [data-field=body]";
  const scratch = path.dirname(session);
  const file = (name, text) => { const f = path.join(scratch, `note-marks-${name}`); fs.writeFileSync(f, text); return `@${f}`; };
  const note = (title, body) => sy("notes", "create", "--title", title, "--body", file(title.replace(/\W+/g, "-"), body)).find((e) => e.type === "created").note.id;
  const get = (id) => sy("notes", "get", "--id", id).find((e) => e.type === "note")?.note;
  const marks = (id) => sy("notes", "marks", "--id", id).filter((e) => e.type === "mark").map((e) => e.mark);
  const mark = (id, from, to, ...flags) => {
    const out = sy("notes", "mark", "--id", id, "--from", String(from), "--to", String(to), ...flags);
    const m = out.find((e) => e.type === "marked");
    if (!m) throw new Error(`notes.mark: ${JSON.stringify(out)}`);
    return m.mark;
  };
  const openNote = async (id) => {
    const { context, page } = await openPage(`#/note/${id}`);
    const ed = page.locator(`[data-surface=editor-body][data-kind=note][data-ticket="${id}"]`);
    await eventually("the note is drawn", async () => (await ed.locator(".cm-content[data-field=body-source]").count()) === 1, 10_000);
    return { context, page, ed };
  };
  const caretAt = (page, pos) => page.evaluate(({ s, pos }) => { const v = document.querySelector(s).editorView; v.focus(); v.dispatch({ selection: { anchor: pos } }); }, { s: BODY, pos });
  const blur = (page) => page.evaluate((s) => document.querySelector(s).editorView.contentDOM.blur(), BODY);
  const underline = (ed, m) => ed.locator(`.nm-u[data-mark="${m}"]`);
  const underlined = async (ed, m) => (await underline(ed, m).allInnerTexts()).join("");

  const SERVICE_MAP = "Draw every service as a node on a graph.\n\nLet me connect two nodes by clicking one, then the other; clicking a line takes it away.\n\nThe names come from the service registry, so a new service shows up without anyone drawing it.\n\nA stopped service stays, drawn hollow.\n";
  const at = (text, words) => [SERVICE_MAP.indexOf(words), SERVICE_MAP.indexOf(words) + words.length];

  await test("a_sentence_became_a_ticket_underlined_on_every_page_and_its_link_opens", async () => {
    const ticket = sy("tickets", "create", "--epic", "KILN", "--kind", "scope", "--title", "the service map draws every service as a node").find((e) => e.type === "created").ticket.id;
    const id = note("The service map", SERVICE_MAP);
    const one = await openNote(id);
    const two = await openNote(id);
    try {
      await sleep(500);
      const before = get(id);
      const [a, z] = at(SERVICE_MAP, "Draw every service as a node on a graph.");
      const sent = Date.now();
      const m = mark(id, a, z, "--link", ticket, "--label", `filed ${ticket}`);
      for (const [k, { ed }] of [one, two].entries()) {
        await eventually(`page ${k + 1}: the words underlined`, async () => (await underlined(ed, m.mark)) === "Draw every service as a node on a graph.", 2_000);
      }
      assert(Date.now() - sent < 2_500, `on every open page within two seconds (${Date.now() - sent} ms)`);
      for (const { ed } of [one, two]) {
        const u = underline(ed, m.mark).first();
        assert((await u.getAttribute("data-mark-link")) === ticket && (await u.getAttribute("role")) === "link", "linked");
        const [colour, accent] = await u.evaluate((e) => [getComputedStyle(e).color, getComputedStyle(document.documentElement).getPropertyValue("--accent")]);
        assert(colour !== "" && accent !== "", `in the accent: ${colour}`);
        const state = ed.locator(`[data-mark-state="${m.mark}"]`);
        assert((await state.innerText()).includes(`filed ${ticket}`) && (await state.innerText()).includes(m.by), `its line of state, with who left it: ${await state.innerText()}`);
        assert((await state.evaluate((e) => getComputedStyle(e).fontFamily)).includes("Mono"), "in the mono face");
        assert((await ed.locator("[data-marked]").count()) === 1, "a dot in the margin of the block that holds it");
        assert((await state.evaluate((e) => e.parentElement.hasAttribute("data-marked"))), "under the block that holds it");
      }
      // Three words linked to an address of the app, and a line of state alone.
      const [ra, rz] = at(SERVICE_MAP, "the service registry");
      const reg = mark(id, ra, rz, "--link", "#/code/plexus-registry");
      const [sa, sz] = at(SERVICE_MAP, "A stopped service stays, drawn hollow.");
      const plain = mark(id, sa, sz, "--label", "built node-legend v1");
      await eventually("three words linked", async () => (await underlined(one.ed, reg.mark)) === "the service registry", 3_000);
      await eventually("an underline with no link", async () => (await underlined(one.ed, plain.mark)) === "A stopped service stays, drawn hollow.", 3_000);
      assert(!(await underline(one.ed, plain.mark).first().getAttribute("class")).includes("nm-linked"), "no link: in the ink");
      // The text is not touched.
      const after = get(id);
      assert(after.body === before.body && after.revision === before.revision, `the body and revision as they were: ${before.revision} → ${after.revision}`);
      if (process.env.PAGES_SHOTS) {
        for (const [w, h] of [[1440, 900], [390, 844]]) for (const theme of ["light", "dark"]) {
          await one.page.setViewportSize({ width: w, height: h });
          await one.page.evaluate((th) => (document.documentElement.dataset.theme = th), theme);
          await sleep(400);
          await one.page.screenshot({ path: path.join(process.env.PAGES_SHOTS, `note-marked-${w}-${theme}.png`) });
        }
        await one.page.setViewportSize({ width: 1440, height: 900 });
        await one.page.evaluate(() => (document.documentElement.dataset.theme = "light"));
        await sleep(300);
      }
      // A click on the underline opens the ticket.
      await underline(one.ed, m.mark).first().click();
      await eventually("the ticket opened", async () => ticketAt(await one.page.evaluate(() => location.hash), ticket), 5_000);
      // An address of the app, by the keyboard.
      await underline(two.ed, reg.mark).first().focus();
      await two.page.keyboard.press("Enter");
      await eventually("the app's address opened", async () => (await two.page.evaluate(() => location.hash)).startsWith("#/code/plexus-registry"), 5_000);
    } finally {
      await one.context.close();
      await two.context.close();
    }
  });

  await test("a_mark_follows_its_words_while_the_note_is_typed", async () => {
    const id = note("The text moves", SERVICE_MAP);
    const { context, page, ed } = await openNote(id);
    try {
      const [a, z] = at(SERVICE_MAP, "Let me connect two nodes");
      const m = mark(id, a, z, "--label", "built graph-nodes v3");
      await eventually("underlined", async () => (await underlined(ed, m.mark)) === "Let me connect two nodes", 3_000);
      // A paragraph written above it, typed.
      await caretAt(page, 0);
      await page.keyboard.type("A paragraph above.\n\n");
      // A word added inside it, typed in its own block: CodeMirror's underline in the caret's block.
      const inside = SERVICE_MAP.indexOf("two nodes") + "A paragraph above.\n\n".length;
      await caretAt(page, inside);
      await page.keyboard.type("more ");
      await eventually("underlined in the caret's block, the new word included", async () => (await underlined(ed, m.mark)) === "Let me connect more two nodes", 5_000);
      assert((await ed.locator(".cm-line.nm-dot").count()) === 1, "the caret's block keeps its dot");
      assert((await ed.locator(".nm-states [data-mark-state]").innerText()).includes("built graph-nodes v3"), "and its line of state");
      // Away from it, drawn: still on the same words.
      await blur(page);
      await eventually("drawn on the same words", async () => (await underlined(ed, m.mark)) === "Let me connect more two nodes" && (await ed.locator(`[data-marked] [data-mark-state="${m.mark}"]`).count()) === 1, 5_000);
      // notes.marks says where they are now.
      await eventually("notes.marks says where they are now", async () => {
        const now = marks(id).find((x) => x.mark === m.mark);
        const body = get(id).body;
        return now && now.words === "Let me connect more two nodes" && body.slice(now.from, now.to) === now.words && now.from === a + 20;
      }, 5_000);
    } finally {
      await context.close();
    }
  });

  await test("a_mark_whose_words_are_deleted_is_drawn_nowhere", async () => {
    const id = note("The words go", SERVICE_MAP);
    const { context, page, ed } = await openNote(id);
    try {
      const [a, z] = at(SERVICE_MAP, "A stopped service stays, drawn hollow.");
      const m = mark(id, a, z, "--label", "built node-legend v1");
      await eventually("underlined", async () => (await underline(ed, m.mark).count()) > 0, 3_000);
      sy("notes", "save", "--id", id, "--body", file("cut", SERVICE_MAP.replace("A stopped service stays, drawn hollow.\n", "")));
      await eventually("drawn nowhere", async () => (await underline(ed, m.mark).count()) === 0 && (await ed.locator(`[data-mark-state="${m.mark}"]`).count()) === 0 && (await ed.locator("[data-marked]").count()) === 0, 5_000);
      const now = marks(id).find((x) => x.mark === m.mark);
      assert(now?.detached === true && now.from == null, `listed as detached: ${JSON.stringify(now)}`);
    } finally {
      await context.close();
    }
  });
};
