// EDITOR-13: every key the app answers is the engine's (FRAME-36's `keys()` /
// `bus.keys()`), and the engine captures it — a bound chord never reaches the
// page or the browser, whatever has focus, the editor's commands included.
// `node tests/pages_browser.mjs --group keys`.
//
// The spy is board-keys's: a window capture listener the test adds before the
// page's scripts records each press and reads `defaultPrevented` once the
// host had it; a document listener says whether the press reached the page.
// On each route — `#/`, `#/edit/:id`, `#/doc/:id`, `#/ticket/:id`, `#/board` —
// each page-wide and editor chord is pressed with focus in the editor (where
// there is one), in an input, on a button and on the body, and must be the
// host's: prevented, never heard by the page, and its fold run where it shows.
//
// Marks: the editor's `.cm-content[data-field=body-source]` (`[data-field=body]`'s
// `.editorView`), the top's `[data-form=search] input` and
// `[data-action="palette.open"]`, the palette's `input[data-palette-input]`,
// the host's overlay `[data-engine-keys]` with its `[data-chord]` rows.
import { checkDir } from "../keys_check.mjs";

const EPIC = "KEYS";

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, fs, path, session } = ctx;

  const SPY = () => {
    window.__keys = [];
    addEventListener("keydown", (e) => {
      const rec = { key: e.key, ctrl: e.ctrlKey, prevented: null, reached: false };
      window.__keys.push(rec);
      setTimeout(() => (rec.prevented = e.defaultPrevented), 0);
    }, true);
    document.addEventListener("keydown", () => { const r = window.__keys.at(-1); if (r) r.reached = true; }, true);
  };

  const created = (out, what) => {
    const c = out.find((e) => e.type === "created");
    if (!c) throw new Error(`create ${what}: ${JSON.stringify(out)}`);
    return c.ticket?.id ?? c.doc?.id;
  };
  const bodyFile = (name, text) => { const f = path.join(path.dirname(session), `keys-${name}.md`); fs.writeFileSync(f, text); return `@${f}`; };
  const root = created(sy("tickets", "create", "--epic", EPIC, "--kind", "scope", "--title", "the keys' tickets"), "root");
  const make = (title, body) => created(sy("tickets", "create", "--epic", EPIC, "--parent", root, "--title", title, "--body", bodyFile(title.replace(/\W+/g, "-"), body)), title);
  const BODY = "[data-surface=editor-body] [data-field=body]";
  const src = (page) => page.evaluate((s) => document.querySelector(s)?.editorView?.state.doc.toString(), BODY);

  /** The route, with the spy in before the page's scripts. */
  async function at(route, ready) {
    const { context, page } = await openPage(route);
    await context.addInitScript(SPY);
    await page.reload();
    await eventually(`${route} is drawn`, () => ready(page), 8_000);
    await sleep(300);
    return { context, page };
  }
  const editorUp = async (page) => (await page.locator(".cm-content[data-field=body-source]").count()) === 1;
  const topUp = async (page) => (await page.locator("[data-surface=app-top] [data-form=search] input").count()) === 1;

  /** Focus put in `where`: the editor (the caret at its end), the top's search input, a button, the body. */
  async function focus(page, where) {
    await page.evaluate(() => document.activeElement?.blur?.());
    if (where === "editor") await page.evaluate((s) => { const v = document.querySelector(s).editorView; v.focus(); v.dispatch({ selection: { anchor: v.state.doc.length } }); }, BODY);
    else if (where === "input") await page.locator("[data-surface=app-top] [data-form=search] input").focus();
    else if (where === "button") await page.locator('[data-surface=app-top] [data-action="palette.open"]').focus();
    else await page.evaluate(() => document.body.focus());
  }
  /** One press: what the spy saw. */
  async function press(page, chord) {
    const n = await page.evaluate(() => window.__keys.length);
    await page.keyboard.press(chord);
    await sleep(40);
    return page.evaluate((n) => window.__keys.slice(n), n);
  }
  const closePalette = async (page) => {
    if ((await page.locator("input[data-palette-input]").count()) === 0) return;
    await page.locator("input[data-palette-input]").focus();
    await page.keyboard.press("Escape");
    await eventually("the palette closes", async () => (await page.locator("input[data-palette-input]").count()) === 0, 2_000);
  };

  /** Every chord on `route` in every focus: the host's alone. `extra(page, chord, where)` checks what the fold did. */
  async function sweep(route, ready, chords, focuses, extra = async () => {}) {
    const reds = [];
    const { context, page } = await at(route, ready);
    try {
      for (const where of focuses) {
        for (const chord of chords) {
          await closePalette(page);
          await focus(page, where);
          const before = await src(page);
          const seen = await press(page, chord);
          const last = seen.at(-1);
          if (!last || last.prevented !== true || last.reached) reds.push(`${route} ${chord} in ${where}: ${JSON.stringify(last)}`);
          else {
            const why = await extra(page, chord, where, before);
            if (why) reds.push(`${route} ${chord} in ${where}: ${why}`);
          }
        }
      }
    } finally {
      await context.close();
    }
    return reds;
  }

  // The documented page-wide set, and the editor's commands.
  const PAGE = ["Control+k", "Control+s"];
  const EDITOR = ["Control+s", "Control+b", "Control+i", "Control+k", "Control+Enter"];

  /** On the editor: each command did its work in the editor, and ctrl+k off it is the palette. */
  const editorRan = async (page, chord, where, before) => {
    if (where === "editor") {
      if (chord === "Control+b") { const now = await src(page); if (now === before) return "bold ran nothing"; await page.keyboard.press("Control+z"); }
      if (chord === "Control+i") { const now = await src(page); if (now === before) return "italic ran nothing"; await page.keyboard.press("Control+z"); }
      if (chord === "Control+k") {
        const now = await src(page);
        if ((await page.locator("input[data-palette-input]").count()) > 0) return "ctrl+k in the editor opened the palette, not a link";
        if (now === before) return "link ran nothing";
        await page.keyboard.press("Control+z");
      }
      if (chord === "Control+Enter") {
        await page.keyboard.press("Escape");
      }
    } else if (chord === "Control+k") {
      await sleep(100);
      if ((await page.locator("input[data-palette-input]").count()) === 0) return "ctrl+k did not open the palette";
    }
    return null;
  };
  const paletteRan = async (page, chord) => {
    if (chord !== "Control+k") return null;
    await sleep(100);
    return (await page.locator("input[data-palette-input]").count()) === 0 ? "ctrl+k did not open the palette" : null;
  };

  await test("no_component_hears_a_chord_of_its_own", async () => {
    const refused = checkDir(path.join(path.dirname(new URL(import.meta.url).pathname), "..", "..", "ui", "src"));
    assert(refused.length === 0, `tests/keys_check.mjs refuses:\n  ${refused.join("\n  ")}`);
  });

  await test("the_editors_commands_are_the_hosts_in_every_focus", async () => {
    const id = make("keys in the editor", "first line\n\nsecond line\n");
    const doc = sy("docs", "create", "--title", "Keys doc", "--body", bodyFile("doc", "a doc's line\n")).find((e) => e.type === "created").doc.id;
    const reds = [
      ...(await sweep(`#/edit/${id}`, editorUp, EDITOR, ["editor", "input", "button", "body"], editorRan)),
      ...(await sweep(`#/doc/${doc}`, editorUp, EDITOR, ["editor", "input", "button", "body"], editorRan)),
      // `#/` in a fresh browser: the screen, with no ticket remembered — the page-wide chords and the editor's.
      ...(await sweep("#/", async (page) => (await topUp(page)) && (await page.locator("[data-surface=editor-screen]").count()) === 1,
        PAGE, ["input", "button", "body"], editorRan)),
    ];
    assert(reds.length === 0, `${reds.length} chords reached the page or the browser:\n  ${reds.join("\n  ")}`);
  });

  await test("ctrl_s_on_the_editor_saves_whatever_has_focus", async () => {
    const id = make("keys save", "kept\n");
    const { context, page } = await at(`#/edit/${id}`, editorUp);
    try {
      await focus(page, "editor");
      await page.keyboard.type(" more");
      await focus(page, "body");
      const seen = await press(page, "Control+s");
      assert(seen.at(-1)?.prevented === true && !seen.at(-1)?.reached, `ctrl+s on the body is the host's: ${JSON.stringify(seen.at(-1))}`);
      await eventually("saved", async () => (sy("tickets", "get", "--id", id).find((e) => e.type === "ticket")?.ticket.body ?? "").includes("more"), 3_000);
    } finally {
      await context.close();
    }
  });

  await test("the_page_wide_chords_are_the_hosts_on_every_route", async () => {
    const id = make("keys on a ticket", "a body\n");
    const reds = [
      ...(await sweep(`#/ticket/${id}`, topUp, PAGE, ["input", "button", "body"], paletteRan)),
      ...(await sweep("#/board", topUp, PAGE, ["input", "button", "body"], paletteRan)),
    ];
    assert(reds.length === 0, `${reds.length} chords reached the page or the browser:\n  ${reds.join("\n  ")}`);
  });

  await test("the_overlay_lists_the_editors_chords", async () => {
    const id = make("keys overlay", "a body\n");
    const { context, page } = await at(`#/edit/${id}`, editorUp);
    try {
      await focus(page, "body");
      await press(page, "?");
      await eventually("the overlay opens", async () => (await page.locator("[data-engine-keys]").count()) === 1, 2_000);
      const chords = await page.locator("[data-engine-keys] [data-chord]").evaluateAll((els) => els.map((e) => e.getAttribute("data-chord")));
      // PAGES_SHOTS=DIR: the overlay at 1440 and 390, light and dark (the ticket's before and after).
      if (process.env.PAGES_SHOTS) for (const [w, h] of [[1440, 900], [390, 844]]) for (const theme of ["light", "dark"]) {
        await page.setViewportSize({ width: w, height: h });
        await page.emulateMedia({ colorScheme: theme });
        await page.evaluate((th) => (document.documentElement.dataset.theme = th), theme);
        await sleep(300);
        await page.screenshot({ path: path.join(process.env.PAGES_SHOTS, `keys-overlay-${w}-${theme}.png`) });
      }
      const missing = ["ctrl+s", "ctrl+b", "ctrl+i", "ctrl+k", "ctrl+enter"].filter((c) => !chords.includes(c));
      assert(missing.length === 0, `the overlay lacks ${missing.join(", ")} (has ${chords.join(", ")})`);
    } finally {
      await context.close();
    }
  });
};
