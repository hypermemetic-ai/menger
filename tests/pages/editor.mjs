// EDITOR-3: the editor — editor-body at #/editor/:id on the throwaway: the
// body in CodeMirror, formed in place and exact; one amend 600 ms after the
// last key; a refusal in the bar; an outside amend landing around the caret;
// a dirty draft merged with an outside write at the pause. Run alone by
// `node tests/pages_browser.mjs --group editor`.
//
// Marks: `[data-surface=editor-body]`, `[data-field=title]`, `[data-field=body]`
// (CodeMirror's content), `[data-field=write-state][data-state=idle|saving|saved|
// refused|merged|outside]`, `[data-refused]`, `[data-hand=NAME]`, `[data-action=revisions]`,
// `[data-loading]`, `[data-empty]`; a formed line carries `.eb-h1`, `.eb-li`, …
import { spawn } from "node:child_process";
import { sentCalls } from "../../scripts/ui-throwaway.mjs";

const EPIC = "EDIT";
const PAUSE = 600;

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, fs, path, session, server } = ctx;
  const created = (out, what) => {
    const c = out.find((e) => e.type === "created");
    if (!c) throw new Error(`create ${what}: ${JSON.stringify(out)}`);
    return c.ticket.id;
  };
  const root = created(sy("tickets", "create", "--epic", EPIC, "--kind", "scope", "--title", "the editor's tickets"), "root");
  const bodyFile = (name, text) => { const f = path.join(path.dirname(session), `editor-${name}.md`); fs.writeFileSync(f, text); return `@${f}`; };
  const make = (title, body) => created(sy("tickets", "create", "--epic", EPIC, "--parent", root, "--title", title, "--body", bodyFile(title.replace(/\W+/g, "-"), body)), title);
  const get = (id) => sy("tickets", "get", "--id", id).find((e) => e.type === "ticket")?.ticket;
  const amends = async (page) => (await sentCalls(page)).filter((c) => c.method === "tickets.amend");
  /** An outside hand's amend, asynchronous: the test keeps typing while it is on its way, so the editor's
   *  pause has not ended when the write lands (scripts/ui-throwaway.mjs's synapse, without the wait). */
  const outside = (id, file) => new Promise((done, fail) => {
    const child = spawn("synapse", ["--no-hold", "--token-file", session, "-j", "-P", String(server.ws), "menger", "tickets", "amend", "--id", id, "--body", file],
      { env: { ...process.env, XDG_CACHE_HOME: path.join(server.work, "cache") }, stdio: ["ignore", "ignore", "pipe"] });
    let err = "";
    child.stderr.on("data", (d) => (err += d));
    child.on("exit", (code) => (code === 0 ? done() : fail(new Error(`outside amend failed (${code}): ${err}`))));
  });
  const open = async (id) => {
    const { context, page } = await openPage(`#/editor/${id}`);
    const ed = page.locator("[data-surface=editor-body]");
    await eventually("the editor is drawn", async () => (await ed.locator(".cm-content[data-field=body]").count()) === 1);
    return { context, page, ed, content: ed.locator(".cm-content") };
  };
  const lineText = (ed, n) => ed.locator(".cm-line").nth(n - 1).innerText();
  const state = (ed) => ed.locator("[data-field=write-state]");
  const shots = process.env.PAGES_SHOTS;

  await test("the_body_is_formed_in_place_and_exact", async () => {
    const SRC = "# The editor\n\nSome *words*, **weight** and `code` with a [link](https://menger.sh).\n\n- one\n- two\n\n```js\nlet a = 1;\n```\n\n> quoted\n\n\\*escaped\\*  __odd__\n";
    const id = make("formed and exact", SRC);
    const { context, page, ed, content } = await open(id);
    try {
      // Formed: the heading's line, its mark hidden once the caret leaves it (the editor opens with the
      // caret at the top, on the heading); the bullets; the fence.
      assert((await ed.locator(".cm-line.eb-h1").count()) === 1, "one h1 line");
      assert((await lineText(ed, 1)) === "# The editor", "at the top, the caret's own line shows its mark");
      await ed.locator(".cm-line.eb-quote").click();
      await eventually("the heading forms once the caret leaves it", async () => (await lineText(ed, 1)) === "The editor");
      for (const [cls, text] of [[".eb-em", "words"], [".eb-strong", "weightodd"], [".eb-code", "code"]]) assert((await ed.locator(cls).allInnerTexts()).join("") === text, `${cls} marks ${text}: ${JSON.stringify(await ed.locator(cls).allInnerTexts())}`);
      assert((await ed.locator(".eb-link").count()) === 1 && (await ed.locator(".eb-link").getAttribute("title")) === "https://menger.sh", "the link's text, its URL as the title");
      assert((await ed.locator(".eb-bullet").count()) === 2, "two bullets drawn for the list marks");
      assert((await ed.locator(".cm-line.eb-fence").count()) === 3, "the fence's three lines");
      assert((await ed.locator(".cm-line.eb-quote").count()) === 1, "the quote's line");
      // The caret's line shows its source: click the heading.
      await ed.locator(".cm-line.eb-h1").click();
      await eventually("the heading's mark shows on the caret's line", async () => (await lineText(ed, 1)) === "# The editor");
      // Exact: one key at the end, and the store reads the source as written plus that key.
      await page.keyboard.press("Control+End");
      await page.keyboard.type("!");
      await eventually("the amend lands", async () => (await state(ed).getAttribute("data-state")) === "saved", 5_000);
      const stored = get(id).body;
      assert(stored === SRC + "!", `the source is exact: ${JSON.stringify(stored.slice(-40))}`);
      const sent = await amends(page);
      assert(sent.length === 1 && sent[0].params.body === SRC + "!", `one amend, the body whole: ${sent.length}`);
      if (shots) {
        await page.screenshot({ path: path.join(shots, "editor-body-1440-light.png") });
        await page.evaluate(() => (document.documentElement.dataset.theme = "dark"));
        await page.screenshot({ path: path.join(shots, "editor-body-1440-dark.png") });
        await page.setViewportSize({ width: 390, height: 844 });
        await page.screenshot({ path: path.join(shots, "editor-body-390-dark.png") });
        await page.evaluate(() => (document.documentElement.dataset.theme = "light"));
        await page.screenshot({ path: path.join(shots, "editor-body-390-light.png") });
      }
    } finally {
      await context.close();
    }
  });

  await test("one_amend_per_pause_and_the_title_too", async () => {
    const id = make("the pause", "a line");
    const { context, page, ed, content } = await open(id);
    try {
      await content.click();
      await page.keyboard.press("Control+End");
      await page.keyboard.type(" and some", { delay: 30 });
      await sleep(PAUSE / 3);
      assert((await amends(page)).length === 0, "nothing sent while the keys keep coming");
      await page.keyboard.type(" more", { delay: 30 });
      await eventually("saved", async () => (await state(ed).getAttribute("data-state")) === "saved", 5_000);
      assert((await amends(page)).length === 1, `one amend for the whole burst: ${(await amends(page)).length}`);
      assert(get(id).body === "a line and some more", `the store holds the burst: ${JSON.stringify(get(id).body)}`);
      // The title: the same pause, amend { title }.
      const title = ed.locator("[data-field=title]");
      await title.fill("the pause, renamed");
      await eventually("the title lands", async () => get(id).title === "the pause, renamed", 5_000);
      const sent = await amends(page);
      assert(sent.length === 2 && sent[1].params.title === "the pause, renamed" && sent[1].params.body == null, `the title's own amend: ${JSON.stringify(sent.map((s) => Object.keys(s.params)))}`);
      // Blur sends at once.
      await content.click();
      await page.keyboard.press("Control+End");
      await page.keyboard.type("!");
      await title.focus();
      await eventually("the blur's amend", async () => get(id).body === "a line and some more!", 2_000);
    } finally {
      await context.close();
    }
  });

  await test("a_refusal_is_said_in_the_bar_and_the_draft_stays", async () => {
    const id = make("refused", "fine\n");
    const { context, page, ed, content } = await open(id);
    try {
      await content.click();
      await page.keyboard.press("Control+End");
      // Two inputs with one id: the service refuses the write, naming it (V2-20).
      await page.keyboard.insertText('<m-input id="twice" kind="text"><m-question>a?</m-question></m-input>\n\n<m-input id="twice" kind="text"><m-question>b?</m-question></m-input>\n');
      await eventually("the refusal in the bar", async () => (await ed.locator("[data-field=write-state][data-refused]").count()) === 1, 5_000);
      const said = await state(ed).innerText();
      assert(/twice/.test(said), `the service's words name the input: ${said}`);
      assert(get(id).body === "fine\n", "the store is as it was");
      assert((await lineText(ed, 2)).includes("twice"), "the draft stays in the editor");
      if (shots) await page.screenshot({ path: path.join(shots, "editor-body-refused.png") });
    } finally {
      await context.close();
    }
  });

  await test("an_outside_amend_lands_around_the_caret", async () => {
    const id = make("two hands", "# Head\n\nfirst line\n\nlast line\n");
    const { context, page, ed, content } = await open(id);
    try {
      await ed.locator(".cm-line").first().click();
      await page.keyboard.press("End");
      await eventually("the mark shows on the caret's line", async () => (await lineText(ed, 1)) === "# Head");
      sy("tickets", "amend", "--id", id, "--body", bodyFile("outside", "# Head\n\nfirst line\n\nlast line, changed outside\n"));
      await eventually("the outside change is in the editor", async () => (await lineText(ed, 5)) === "last line, changed outside", 5_000);
      await eventually("the bar names the hand", async () => (await state(ed).getAttribute("data-state")) === "outside" && (await state(ed).getAttribute("data-hand")) === "owner", 5_000);
      assert((await lineText(ed, 1)) === "# Head", "the caret stayed on the heading: its mark still shows");
      await page.keyboard.type("!");
      await eventually("our key lands where the caret was", async () => get(id).body === "# Head!\n\nfirst line\n\nlast line, changed outside\n", 5_000);
      assert((await amends(page)).length === 1, "one amend of ours; the outside one was never resent");
    } finally {
      await context.close();
    }
  });

  await test("a_dirty_draft_merges_an_outside_write_at_the_pause", async () => {
    const id = make("merged", "# Head\n\nfirst line\n\nlast line\n");
    const { context, page, ed, content } = await open(id);
    try {
      await ed.locator(".cm-line").first().click();
      await page.keyboard.press("End");
      await page.keyboard.type(" o");
      // Inside the pause: another hand changes the last line — on its way while the keys keep coming, so
      // the pause has not ended when it lands.
      const theirs = outside(id, bodyFile("theirs", "# Head\n\nfirst line\n\nlast line, theirs\n"));
      await page.keyboard.type("ur", { delay: 150 });
      await theirs;
      await page.keyboard.type("s");
      await eventually("the merge is said", async () => (await state(ed).getAttribute("data-state")) === "merged", 8_000);
      assert(get(id).body === "# Head ours\n\nfirst line\n\nlast line, theirs\n", `both hands in the store: ${JSON.stringify(get(id).body)}`);
      assert((await lineText(ed, 5)) === "last line, theirs", "theirs is in the editor");
      assert(/owner/.test(await state(ed).innerText()), `the bar names whose: ${await state(ed).innerText()}`);
      // Where they meet, ours wins and the bar says theirs is in the revision.
      await page.keyboard.type("!");
      const clash = outside(id, bodyFile("clash", "# Head theirs\n\nfirst line\n\nlast line, theirs\n"));
      await page.keyboard.type("!", { delay: 150 });
      await clash;
      await page.keyboard.type("!");
      await eventually("ours won the line", async () => get(id).body === "# Head ours!!!\n\nfirst line\n\nlast line, theirs\n", 8_000);
      await eventually("the bar says what the revision keeps", async () => /revision/.test(await state(ed).innerText()), 5_000);
      if (shots) await page.screenshot({ path: path.join(shots, "editor-body-merged.png") });
    } finally {
      await context.close();
    }
  });

  await test("no_ticket_and_the_loading_state", async () => {
    const { context, page } = await openPage("#/editor/ZZZZ-999999");
    try {
      await eventually("no such ticket", async () => (await page.locator("[data-surface=editor-body] [data-empty]").count()) === 1, 5_000);
    } finally {
      await context.close();
    }
  });
};
