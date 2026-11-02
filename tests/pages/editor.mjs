// EDITOR-3: the editor — editor-body at #/edit/:id on the throwaway: the
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
import { makeKey, writeSession } from "../../scripts/throwaway-login.mjs";
import { sentCalls, synapse } from "../../scripts/ui-throwaway.mjs";

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
    const { context, page } = await openPage(`#/edit/${id}`);
    const ed = page.locator("[data-surface=editor-body]");
    await eventually("the editor is drawn", async () => (await ed.locator(".cm-content[data-field=body]").count()) === 1);
    return { context, page, ed, content: ed.locator(".cm-content") };
  };
  const lineText = (ed, n) => ed.locator(".cm-line").nth(n - 1).innerText();
  const state = (ed) => ed.locator("[data-field=write-state]");
  const shots = process.env.PAGES_SHOTS;
  /** `name`-1440-light … `name`-390-dark: both widths, both themes, the transitions ended; the page left at 1440, light. */
  const shoot = async (page, name) => {
    if (!shots) return;
    for (const [w, h] of [[1440, 900], [390, 844]]) for (const theme of ["light", "dark"]) {
      await page.setViewportSize({ width: w, height: h });
      await page.evaluate((th) => (document.documentElement.dataset.theme = th), theme);
      await sleep(300);
      await page.screenshot({ path: path.join(shots, `${name}-${w}-${theme}.png`) });
    }
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.evaluate(() => (document.documentElement.dataset.theme = "light"));
  };

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
      assert((await ed.locator(".cm-content").innerText()).includes('id="twice"'), "the draft stays in the editor (the second block as source: one id twice draws no card)");
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
    const { context, page } = await openPage("#/edit/ZZZZ-999999");
    try {
      await eventually("no such ticket", async () => (await page.locator("[data-surface=editor-body] [data-empty]").count()) === 1, 5_000);
    } finally {
      await context.close();
    }
  });

  // ── EDITOR-4: the first screen ───────────────────────────────────────────
  // Marks: `[data-surface=editor-screen]` (`[data-ticket]`, `[data-pane=editor|rendered]`), `[data-half=editor|rendered]`,
  // `[data-action=new]`, `[data-action=pane][data-to]`, `[data-welcome]`, `[data-field=current]`.
  const hash = (page) => page.evaluate(() => location.hash);

  await test("the_first_screen_is_the_editor_with_the_ticket_rendered_beside_it", async () => {
    const id = make("the first screen", "# Beside\n\nwritten on the left, read on the right\n");
    const { context, page } = await openPage(`#/edit/${id}`);
    try {
      const screen = page.locator("[data-surface=editor-screen]");
      await eventually("both halves", async () => (await screen.locator("[data-half=editor] [data-surface=editor-body] .cm-content").count()) === 1 && (await screen.locator("[data-half=rendered] [data-surface=ticket-head]").count()) === 1);
      assert((await screen.locator("[data-half=rendered] [data-surface=ticket-body]").count()) === 1 && (await screen.locator("[data-half=rendered] [data-surface=ticket-questions]").count()) === 1, "the body and the questions on the right");
      assert((await page.locator("[data-layout=page-editor][data-slot=top] [data-surface=app-top]").count()) === 1, "the app's top on the page");
      assert((await page.locator("[data-surface=app-top] [data-link=board]").getAttribute("href")) === "#/board", "the top's board link goes to #/board");
      // A key on the left lands on the right, as everyone else sees it.
      const content = screen.locator("[data-half=editor] .cm-content");
      await content.click();
      await page.keyboard.press("Control+End");
      await page.keyboard.type("!");
      await eventually("the rendered half redraws", async () => ((await screen.locator("[data-half=rendered] [data-ticket-body]").innerText()) || "").includes("read on the right\n!") || ((await screen.locator("[data-half=rendered] [data-ticket-body]").innerText()) || "").includes("!"), 5_000);
      // Then `#/` is this ticket: the last opened.
      await page.evaluate(() => (location.hash = "#/"));
      await eventually("#/ opens the last ticket", async () => (await page.locator("[data-surface=editor-screen]").getAttribute("data-ticket")) === id, 5_000);
      assert((await hash(page)) === "#/", "the route stays #/");
      const shots = process.env.PAGES_SHOTS;
      if (shots) {
        await eventually("drawn for the shot", async () => (await screen.locator("[data-half=rendered] [data-ticket-body] h1").count()) === 1, 5_000);
        await page.screenshot({ path: path.join(shots, "editor-screen-1440-light.png") });
        await page.evaluate(() => (document.documentElement.dataset.theme = "dark"));
        await sleep(300); // the colours' transitions end
        await page.screenshot({ path: path.join(shots, "editor-screen-1440-dark.png") });
        await page.evaluate(() => (document.documentElement.dataset.theme = "light"));
      }
    } finally {
      await context.close();
    }
  });

  await test("a_phone_shows_one_pane_and_a_switch", async () => {
    const id = make("on a phone", "# Phone\n\none pane at a time\n");
    const { context, page } = await openPage(`#/edit/${id}`);
    try {
      await page.setViewportSize({ width: 390, height: 844 });
      const screen = page.locator("[data-surface=editor-screen]");
      await eventually("the editor pane", async () => (await screen.getAttribute("data-pane")) === "editor" && (await screen.locator("[data-half=editor] .cm-content").isVisible()));
      assert(!(await screen.locator("[data-half=rendered]").isVisible()), "the rendered half is hidden on the phone");
      const shots = process.env.PAGES_SHOTS;
      if (shots) {
        await page.screenshot({ path: path.join(shots, "editor-screen-390-light.png") });
        await page.evaluate(() => (document.documentElement.dataset.theme = "dark"));
        await sleep(300); // the colours' transitions end
        await page.screenshot({ path: path.join(shots, "editor-screen-390-dark.png") });
        await page.evaluate(() => (document.documentElement.dataset.theme = "light"));
      }
      await screen.locator("[data-action=pane][data-to=rendered]").click();
      await eventually("the rendered pane", async () => (await screen.getAttribute("data-pane")) === "rendered" && (await screen.locator("[data-half=rendered] [data-ticket-body]").isVisible()));
      assert(!(await screen.locator("[data-half=editor]").isVisible()), "the editor is hidden in turn");
      if (shots) await page.screenshot({ path: path.join(shots, "editor-screen-390-rendered.png") });
    } finally {
      await context.close();
    }
  });

  await test("with_no_last_ticket_new_makes_a_draft_and_opens_it", async () => {
    const { context, page } = await openPage("#/");
    try {
      const screen = page.locator("[data-surface=editor-screen]");
      await eventually("the welcome", async () => (await screen.locator("[data-welcome]").count()) === 1);
      await eventually("new is ready once a shortcode is known", async () => !(await screen.locator("[data-welcome] [data-action=new]").isDisabled()), 5_000);
      await screen.locator("[data-welcome] [data-action=new]").click();
      await eventually("the new ticket opens", async () => /^#\/edit\/[A-Z]+-\d+$/.test(await hash(page)), 5_000);
      const made = (await hash(page)).replace("#/edit/", "");
      const t = get(made);
      assert(t.status === "Draft" && t.title === "Untitled", `a Draft called Untitled: ${t.status} ${t.title}`);
      await eventually("its editor", async () => (await screen.locator("[data-half=editor] .cm-content").count()) === 1);
      assert((await screen.getAttribute("data-ticket")) === made, "the screen is the new ticket's");
    } finally {
      await context.close();
    }
  });

  await test("escape_and_g_b_go_to_the_board", async () => {
    const id = make("one key from the board", "# Esc\n");
    const { context, page } = await openPage(`#/edit/${id}`);
    try {
      const content = page.locator("[data-surface=editor-screen] [data-half=editor] .cm-content");
      await content.click();
      await page.keyboard.press("Escape");
      await eventually("the board", async () => (await hash(page)) === "#/board" && (await page.locator("[data-layout=board-frame]").count()) > 0, 5_000);
      // And the ticket page's board key lands there too.
      await page.evaluate((id) => (location.hash = `#/ticket/${id}`), id);
      await page.locator("[data-surface=ticket-head]").waitFor();
      await page.keyboard.press("g");
      await page.keyboard.press("b");
      await eventually("g b from the ticket page", async () => (await hash(page)) === "#/board", 5_000);
    } finally {
      await context.close();
    }
  });

  // ── EDITOR-5: links and images, against the real media service ───────────
  const PNG_2X1 = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR42mP8z8DwnwEJMAIAKQgD/2zM2wkAAAAASUVORK5CYII=";
  /** A paste of a file on the editor; answers how many upload cards stood the moment it was dispatched. */
  const pasteFile = (page, b64, type, name) => page.evaluate(({ b64, type, name }) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    const dt = new DataTransfer();
    dt.items.add(new File([bytes], name, { type }));
    document.querySelector("[data-surface=editor-body] .cm-content").dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
    return document.querySelectorAll("[data-upload]").length;
  }, { b64, type, name });

  await test("a_url_pasted_over_a_selection_links_it", async () => {
    const id = make("a link", "see the site\n");
    const { context, page, ed, content } = await open(id);
    try {
      await content.click();
      await page.keyboard.press("Control+Home");
      for (let i = 0; i < 4; i++) await page.keyboard.press("ArrowRight");
      for (let i = 0; i < 8; i++) await page.keyboard.press("Shift+ArrowRight");
      await page.evaluate(() => {
        const dt = new DataTransfer();
        dt.setData("text/plain", "https://menger.sh/x");
        document.querySelector("[data-surface=editor-body] .cm-content").dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
      });
      await eventually("the link is in the store", async () => get(id).body === "see [the site](https://menger.sh/x)\n", 5_000);
    } finally {
      await context.close();
    }
  });

  await test("a_pasted_image_lands_as_media_and_renders", async () => {
    const id = make("an image", "above\n\nbelow\n");
    const { context, page, ed, content } = await open(id);
    try {
      await content.click();
      await page.keyboard.press("Control+Home");
      await page.keyboard.press("End");
      assert((await pasteFile(page, PNG_2X1, "image/png", "the curve.png")) === 1, "the upload card stands at once");
      assert(!get(id).body.includes("uploading"), "no placeholder in the source");
      await eventually("the source names the image", async () => /^above\n\n!\[the curve\]\(media:[0-9a-f]{64}\)\n\nbelow\n$/.test(get(id).body), 8_000);
      assert((await ed.locator("[data-upload]").count()) === 0, "the card is gone");
      const hash = /media:([0-9a-f]{64})/.exec(get(id).body)[1];
      await eventually("the editor draws it", async () => (await ed.locator(`[data-image="${hash}"]`).count()) === 1, 5_000);
      assert((await ed.locator(`[data-image="${hash}"]`).evaluate((i) => new Promise((r) => (i.complete ? r(i.naturalWidth) : (i.onload = () => r(i.naturalWidth)))))) === 2, "with its pixels");
      await page.evaluate((id) => (location.hash = `#/edit/${id}`), id);
      const img = page.locator("[data-half=rendered] [data-ticket-body] img");
      await eventually("the rendered half draws it", async () => (await img.count()) === 1 && (await img.getAttribute("src")).endsWith(`/media/${hash}`), 5_000);
      if (shots) {
        // A picture worth looking at: drawn in the page, pasted as any image is.
        const big = await page.evaluate(() => {
          const c = Object.assign(document.createElement("canvas"), { width: 640, height: 300 });
          const x = c.getContext("2d");
          x.fillStyle = "#3b5bdb"; x.fillRect(0, 0, 640, 300);
          x.fillStyle = "#ffffff"; x.fillRect(40, 40, 260, 220); x.fillRect(340, 40, 260, 220);
          return c.toDataURL("image/png").split(",")[1];
        });
        await ed.locator(".cm-content").click();
        await page.keyboard.press("Control+End");
        await pasteFile(page, big, "image/png", "the first screen.png");
        await eventually("the picture is in the editor", async () => (await ed.locator("[data-image]").count()) === 2, 8_000);
        await eventually("and on the right", async () => (await img.count()) === 2, 5_000);
        await shoot(page, "editor-image");
      }
    } finally {
      await context.close();
    }
  });

  await test("a_refused_image_is_said_and_leaves_no_line", async () => {
    const id = make("no image", "text\n");
    const { context, page, ed, content } = await open(id);
    try {
      await content.click();
      await pasteFile(page, btoa("this is no image"), "image/png", "fake.png");
      await eventually("the refusal stands where it was pasted", async () => (await ed.locator("[data-upload][data-refused]").count()) === 1, 5_000);
      assert(get(id).body === "text\n", "nothing written");
      await page.keyboard.type("x");
      await eventually("it goes with the next key", async () => (await ed.locator("[data-upload]").count()) === 0, 3_000);
    } finally {
      await context.close();
    }
  });

  await test("an_image_too_large_is_refused_before_the_socket", async () => {
    const id = make("too large", "text\n");
    const { context, page, ed, content } = await open(id);
    try {
      await content.click();
      const before = (await sentCalls(page)).filter((c) => c.method === "media.put").length;
      await page.evaluate(() => {
        const dt = new DataTransfer();
        dt.items.add(new File([new Uint8Array(7 * 1024 * 1024 + 1)], "huge.png", { type: "image/png" }));
        document.querySelector("[data-surface=editor-body] .cm-content").dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
      });
      await eventually("said in place", async () => /7 MiB/.test((await ed.locator("[data-upload][data-refused]").textContent({ timeout: 500 })) ?? ""), 3_000);
      assert((await sentCalls(page)).filter((c) => c.method === "media.put").length === before, "no media.put sent");
    } finally {
      await context.close();
    }
  });

  // ── EDITOR-6: write in at the owner's request ────────────────────────────
  // The agent is a key of its own, an editor, so its amend carries its name.
  const agentKey = path.join(server.work, "agent-writes.key");
  const agentSession = path.join(server.work, "agent-writes.session");
  const agentPrincipal = makeKey(agentKey);
  sy("members", "grant", "--principal", agentPrincipal, "--scope", "*", "--role", "editor");
  await writeSession(`ws://127.0.0.1:${server.ws}`, agentKey, agentSession, "scribe");
  const agent = (...a) => synapse(server, agentSession, ...a);
  const question = (id) => sy("tickets", "inputs", "list", "--ticket", id).flatMap((e) => (e.type === "rows" ? e.items : [e])).map((e) => e.input ?? e).find((i) => i.id === "ask-1");
  /** Lines 1–2 selected, the request asked. */
  const request = async (page, ed) => {
    await ed.locator(".cm-content").click();
    await page.keyboard.press("Control+Home");
    await page.keyboard.press("Shift+ArrowDown");
    await page.keyboard.press("Shift+End");
    await eventually("the bar's button wakes", async () => !(await ed.locator("[data-action=ask-open]").isDisabled()), 2_000);
    await page.keyboard.press("Control+Enter");
    await ed.locator("[data-ask][data-state=open] [data-field=instruction]").fill("tighten these");
    await ed.locator("[data-action=ask]").click();
    await eventually("asked", async () => (await ed.locator("[data-ask=ask-1][data-state=asked]").count()) === 1, 5_000);
  };

  await test("a_selection_and_a_line_become_a_question_queued_now", async () => {
    const id = make("asked", "first line\nsecond line\nthird line\n");
    const { context, page, ed } = await open(id);
    try {
      await request(page, ed);
      const q = question(id);
      assert(q?.state === "open" && q.question === "tighten these — lines 1–2: «first line second line»", `the question: ${JSON.stringify(q)}`);
      const entry = sy("tickets", "queued", "--id", id).flatMap((e) => (e.type === "rows" ? e.items : [e])).find((e) => e.id === id)?.entry;
      assert(entry?.urgency === "now" && JSON.stringify(entry).includes("ask-1"), `queued now, naming it: ${JSON.stringify(entry)}`);
      await sleep(300);
      assert((await ed.locator("[data-hand]").count()) === 0 || (await state(ed).getAttribute("data-state")) !== "outside", "the owner's own ask is nobody's hand");
      assert((await ed.locator(".eb-hand").count()) === 0, "no line marked for the ask's own write");
    } finally {
      await context.close();
    }
  });

  await test("an_agents_amend_is_marked_until_a_click_accepts_it", async () => {
    const id = make("written in", "first line\nsecond line\nthird line\n");
    const { context, page, ed } = await open(id);
    try {
      await request(page, ed);
      const body = get(id).body.replace("first line\nsecond line", "one tight line");
      agent("tickets", "amend", "--id", id, "--body", bodyFile("agent", body));
      await eventually("the agent's line is marked with its name", async () => (await ed.locator('.eb-hand[data-hand=scribe]').count()) === 1, 8_000);
      assert((await ed.locator(".eb-hand").innerText()).includes("one tight line"), "the changed line");
      assert((await ed.locator("[data-ask=ask-1][data-state=answered]").count()) === 1, "the request says it was answered");
      await shoot(page, "editor-hand");
      await ed.locator(".eb-hand").click();
      await eventually("accepted", async () => (await ed.locator("[data-ask=ask-1][data-state=accepted]").count()) === 1, 5_000);
      assert((await ed.locator(".eb-hand").count()) === 0, "the mark is gone");
      await eventually("the input is closed", async () => question(id)?.state === "closed", 5_000);
    } finally {
      await context.close();
    }
  });

  await test("an_edit_takes_the_mark_off_its_line_and_the_request_stays", async () => {
    const id = make("edited over", "first line\nsecond line\nthird line\n");
    const { context, page, ed } = await open(id);
    try {
      await request(page, ed);
      const body = get(id).body.replace("first line", "first, rewritten").replace("third line", "third, rewritten");
      agent("tickets", "amend", "--id", id, "--body", bodyFile("agent2", body));
      await eventually("two lines marked", async () => (await ed.locator(".eb-hand").count()) === 2, 8_000);
      await ed.locator(".cm-content").focus(); // the request's line is gone, and the focus with it
      await page.keyboard.press("Control+Home");
      await page.keyboard.press("End");
      await page.keyboard.type("!");
      await eventually("one mark left", async () => (await ed.locator(".eb-hand").count()) === 1, 3_000);
      assert(question(id)?.state === "open", "the request stays open");
    } finally {
      await context.close();
    }
  });

  await test("a_refused_request_is_said_on_its_line", async () => {
    const id = make("no ask", "text\n\n```\nopen fence\n");
    const { context, page, ed } = await open(id);
    try {
      await ed.locator(".cm-content").click();
      await page.keyboard.press("Control+Home");
      await page.keyboard.press("Shift+End");
      await page.keyboard.press("Control+Enter");
      await ed.locator("[data-field=instruction]").fill("anything");
      await ed.locator("[data-action=ask]").click();
      await eventually("the service's words", async () => (await ed.locator("[data-ask][data-state=open] [data-refused]").count()) === 1, 5_000);
      assert(question(id) == null, "nothing asked");
    } finally {
      await context.close();
    }
  });

  // ── EDITOR-7: the house's two marks as cards ─────────────────────────────
  const HOUSE = `<m-summary>

<m-field data-field="standing">**The kiln fires twice a week.**</m-field>

<m-stat value="2">firings a week</m-stat>
<m-stat value="6" tone="pass">cone</m-stat>

</m-summary>

## The glaze

<m-decision data-field="decision" id="cone">
<m-question data-field="question">Which cone?</m-question>
<m-option data-field="option" name="Six" value="6" recommended></m-option>
<m-option data-field="option" name="Five" value="5"></m-option>
<m-recommendation data-field="recommendation" by="builder">**Six.** It holds.</m-recommendation>
</m-decision>

<m-cases label="kept as source">

| a | b |
| -- | -- |
| 1 | 2 |

</m-cases>
`;

  await test("the_two_house_marks_are_cards_and_their_source_is_a_click_away", async () => {
    const id = make("cards", HOUSE);
    const { context, page, ed } = await open(id);
    try {
      await ed.locator(".cm-line").filter({ hasText: "The glaze" }).click();
      await eventually("two cards", async () => (await ed.locator("[data-card]").count()) === 2, 5_000);
      const sum = ed.locator("[data-card=m-summary]");
      assert((await sum.locator(".standing").innerText()) === "The kiln fires twice a week.", "the standing, as text");
      assert(JSON.stringify(await sum.locator(".stat b").allInnerTexts()) === '["2","6"]', "the stats");
      const dec = ed.locator("[data-card=m-decision][data-id=cone]");
      assert((await dec.locator(".q").innerText()) === "Which cone?", "the question");
      assert(JSON.stringify(await dec.locator(".opt").allInnerTexts()) === '["Six","Five"]' && (await dec.locator(".opt[data-rec]").innerText()) === "Six", "the options, the recommended ringed");
      assert((await ed.locator(".cm-line").filter({ hasText: "<m-cases" }).count()) === 1 && (await ed.locator("[data-card=m-cases]").count()) === 0, "m-cases stays source");
      await shoot(page, "editor-cards");
      await dec.click();
      await eventually("its source, in place", async () => (await ed.locator("[data-card-open=m-decision]").count()) === 1 && (await ed.locator("[data-card=m-decision]").count()) === 0, 3_000);
      await ed.locator(".cm-line").filter({ hasText: "The glaze" }).click();
      await eventually("the card again", async () => (await ed.locator("[data-card]").count()) === 2, 3_000);
      await sleep(PAUSE + 300);
      assert(get(id).body === HOUSE, "the source is unchanged by any of it");
      assert((await amends(page)).length === 0, "nothing written");
      // The decision is answered in the rendered half, as V2-20 has it.
      await page.evaluate((id) => (location.hash = `#/edit/${id}`), id);
      const opt = page.locator("[data-half=rendered] [data-surface=ticket-questions] [data-input=cone] [data-option='6']");
      await opt.click();
      await eventually("answered on the right", async () => (await page.locator("[data-half=rendered] [data-input=cone]").getAttribute("data-state")) === "answered", 5_000);
    } finally {
      await context.close();
    }
  });

  await test("a_malformed_mark_draws_no_card", async () => {
    const id = make("broken", `<m-decision id="x">\n<m-question>open?</m-question>\n\ntext after\n`);
    const { context, page, ed } = await open(id);
    try {
      await ed.locator(".cm-line").filter({ hasText: "text after" }).click();
      await eventually("marked, not carded", async () => (await ed.locator("[data-mark-error=m-decision]").count()) === 1, 5_000);
      assert((await ed.locator("[data-card]").count()) === 0, "no card");
    } finally {
      await context.close();
    }
  });
};
