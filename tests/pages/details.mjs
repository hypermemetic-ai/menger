// EDITOR-22: `<m-details summary="One short sentence.">…</m-details>` in a body is one line that opens to show
// its details — on the ticket page and in a document (the editor's drawn blocks), made by ticket-view from the
// mark the sanitizer kept (m-details.svelte): a real button with `aria-expanded`, the sentence as inline
// markdown (a pill, code), an optional status dot and the `ticket`'s pill at the line's end; several in a row
// a list; what is open kept across a live update, never across a reload, never written; closed details
// `hidden=until-found` (find-in-page reaches them) and open in print. Run alone by
// `node tests/pages_browser.mjs --group details`.
//
// Marks: `m-details[data-folds]` (`[data-det-open]` while open), its `.det-line` (m-details' `[data-surface=
// m-details]`: `button.det-mark[aria-expanded]`, `.det-dot[data-status]`, `.det-sum`, `.det-ticket`) and
// `.det-body`; ticket-view's `.blk.det`; the editor's `[data-surface=editor-body]`.

const EPIC = "FOLD";

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, fs, path, session } = ctx;
  const bodyFile = (name, text) => { const f = path.join(path.dirname(session), `details-${name}.md`); fs.writeFileSync(f, text); return `@${f}`; };
  const createdId = (out, what) => {
    const c = out.find((e) => e.type === "created" || e.ticket?.id);
    if (!c) throw new Error(`create ${what}: ${JSON.stringify(out)}`);
    return c.ticket.id;
  };
  const get = (id) => sy("tickets", "get", "--id", id).find((e) => e.type === "ticket")?.ticket;
  const root = createdId(sy("tickets", "create", "--epic", EPIC, "--kind", "scope", "--title", "the folds' tickets"), "root");
  const make = (title, body = "") => createdId(sy("tickets", "create", "--epic", EPIC, "--parent", root, "--title", title, ...(body ? ["--body", bodyFile(title.replace(/\W+/g, "-"), body)] : [])), title);
  const a = make("the kiln");
  const b = make("the glaze");
  const c = make("the shelves");

  /** Three in a row, the second `open`: a pill and code in the first's sentence, its `ticket` at the line's
   *  end, a pill, a table and a list inside it; a `>` in the second's sentence; the third written with no
   *  blank line after its open tag. */
  const BODY = `Three lines of work.

<m-details summary="Fire the kiln ${a} once, at \`cone 6\`." status="done" ticket="${c}">

The kiln held ${b} for the glaze.

| step | took |
| -- | -- |
| load | 1h |

- a list
- of two

</m-details>

<m-details summary="Glaze -> test tiles first." status="running" open>

Tiles are drying; zebra-quartz is the word here.

</m-details>

<m-details summary="Unload and sort." status="needs-you">
Nothing yet: **kiln-opal** is inside.
</m-details>

After the folds.
`;

  const scopeT = "[data-surface=ticket-body]";
  const scopeD = "[data-surface=editor-body]";
  const ticketPage = async (id) => {
    const opened = await openPage(`#/ticket/${id}`);
    await eventually(`#/ticket/${id} draws its folds`, async () => (await opened.page.locator(`${scopeT} m-details[data-folds]`).count()) === 3, 15_000);
    return opened;
  };
  /** Each fold of `scope`: its button's state, whether its body is hidden, its dot, its sentence's left. */
  const folds = (page, scope) => page.locator(`${scope} m-details[data-folds]`).evaluateAll((els) => els.map((e) => {
    const btn = e.querySelector(".det-mark");
    const body = e.querySelector(":scope > .det-body");
    const sum = e.querySelector(".det-sum");
    const dot = e.querySelector(".det-dot");
    return {
      expanded: btn?.getAttribute("aria-expanded"), tag: btn?.localName, type: btn?.getAttribute("type"),
      hidden: body?.getAttribute("hidden"), height: body?.getBoundingClientRect().height ?? -1,
      dot: dot?.getAttribute("data-status") ?? null, dotColour: dot ? getComputedStyle(dot).backgroundColor : null,
      sentence: sum?.innerText ?? "", left: sum?.getBoundingClientRect().left ?? -1, top: sum?.getBoundingClientRect().top ?? -1,
    };
  }));
  const shots = process.env.DETAILS_SHOTS || process.env.PAGES_SHOTS;
  const shoot = async (page, name) => {
    if (!shots) return;
    fs.mkdirSync(shots, { recursive: true });
    for (const [w, h] of [[1440, 900], [390, 844]]) for (const theme of ["light", "dark"]) {
      await page.setViewportSize({ width: w, height: h });
      await page.evaluate((th) => (document.documentElement.dataset.theme = th), theme);
      await sleep(300);
      await page.screenshot({ path: path.join(shots, `${name}-${w}-${theme}.png`), fullPage: true });
    }
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.evaluate(() => (document.documentElement.dataset.theme = "light"));
  };
  const host = make("three lines of work", BODY);

  await test("a_fold_is_one_line_that_opens_and_closes", async () => {
    const { context, page, seen } = await ticketPage(host);
    try {
      let got = await folds(page, scopeT);
      assert(got.every((f) => f.tag === "button" && f.type === "button"), `each line's mark is a real button: ${JSON.stringify(got)}`);
      assert(JSON.stringify(got.map((f) => f.expanded)) === JSON.stringify(["false", "true", "false"]), `closed, open (\`open\`), closed: ${JSON.stringify(got.map((f) => f.expanded))}`);
      assert(got[0].hidden === "until-found" && got[2].hidden === "until-found" && got[1].hidden == null, `the closed ones hidden until found: ${JSON.stringify(got.map((f) => f.hidden))}`);
      assert(got[0].height === 0 && got[1].height > 0, `closed takes no room: ${got.map((f) => f.height)}`);
      assert(got[1].sentence.includes("Glaze -> test tiles first."), `a sentence with a > in it: ${got[1].sentence}`);
      assert(got[0].sentence.startsWith("Fire the kiln") && (await page.locator(`${scopeT} m-details .det-sum code`).first().innerText()) === "cone 6", "the sentence is inline markdown: its code is code");
      // A click opens it; again closes it.
      const first = page.locator(`${scopeT} m-details[data-folds] .det-mark`).first();
      await first.click();
      got = await folds(page, scopeT);
      assert(got[0].expanded === "true" && got[0].hidden == null && got[0].height > 0, `a click opens it: ${JSON.stringify(got[0])}`);
      assert((await page.locator(`${scopeT} m-details[data-folds]`).first().locator(".det-body table td").count()) > 0, "what it holds is drawn as the body draws anything (a table)");
      await first.click();
      got = await folds(page, scopeT);
      assert(got[0].expanded === "false" && got[0].hidden === "until-found", "again closes it");
      // EDITOR-24: the page's body is the editor, so the sentence is the block's text: a click there is the
      // caret's (its source shows, the other two stay drawn); the caret out again, the fold is drawn as it was left.
      await page.locator(`${scopeT} m-details[data-folds] .det-sum`).nth(2).click({ position: { x: 4, y: 4 } });
      await eventually("the sentence's block is its source", async () => (await page.locator(`${scopeT} m-details[data-folds]`).count()) === 2 && (await page.locator(`${scopeT} .cm-line`).evaluateAll((ls) => ls.some((l) => l.textContent.includes("<m-details")))), 5_000);
      await page.keyboard.press("Escape");
      await eventually("drawn again, closed", async () => { const f = await folds(page, scopeT); return f.length === 3 && f[2].expanded === "false"; }, 5_000);
      // Keyboard: Tab reaches the mark, Enter and Space toggle it.
      await first.focus();
      assert(await first.evaluate((e) => document.activeElement === e), "the mark takes focus");
      await page.keyboard.press("Enter");
      assert((await folds(page, scopeT))[0].expanded === "true", "Enter opens it");
      await page.keyboard.press(" ");
      assert((await folds(page, scopeT))[0].expanded === "false", "Space closes it");
      await page.keyboard.press("Tab");
      assert(await page.evaluate(() => document.activeElement?.closest?.("m-details") != null || document.activeElement?.classList?.contains("tpill")), "Tab moves on through the lines");
      // The dots: the theme's stage colours, three different ones.
      assert(JSON.stringify(got.map((f) => f.dot)) === JSON.stringify(["done", "running", "needs-you"]), `a dot each: ${JSON.stringify(got.map((f) => f.dot))}`);
      const colours = new Set(got.map((f) => f.dotColour));
      assert(colours.size === 3 && ![...colours].some((x) => /rgba\(0, 0, 0, 0\)|transparent/.test(x)), `painted from the tokens: ${[...colours]}`);
      assert(!seen.console.some((l) => /^(error|pageerror)/.test(l)), `no error on the page: ${seen.console.filter((l) => /^(error|pageerror)/.test(l)).join(" | ")}`);
      assert(get(host).body === BODY, "the stored body untouched");
    } finally {
      await context.close();
    }
  });

  await test("a_folds_sentence_and_inside_hold_pills", async () => {
    const { context, page } = await ticketPage(host);
    try {
      const one = page.locator(`${scopeT} m-details[data-folds]`).first();
      await eventually("the pills in the first fold", async () => (await one.locator("a.tpill[data-pill]").count()) === 3, 10_000);
      assert((await one.locator(`.det-sum a.tpill[data-pill="${a}"]`).count()) === 1, "an id in the sentence is a pill");
      assert((await one.locator(`.det-body a.tpill[data-pill="${b}"]`).count()) === 1, "an id inside is a pill (closed or open)");
      assert((await one.locator(`.det-ticket a.tpill[data-pill="${c}"]`).count()) === 1, "`ticket` is a pill at the line's end");
      assert((await one.locator(".det-mark a").count()) === 0, "no link inside the button");
      // A click on the sentence's pill follows it, and does not open the fold.
      const before = (await folds(page, scopeT))[0].expanded;
      await one.locator(`.det-sum a.tpill[data-pill="${a}"]`).click();
      await eventually(`the pill opens ${a}`, async () => (await page.evaluate(() => location.hash)).includes(a), 5_000);
      assert(before === "false", "it was closed");
    } finally {
      await context.close();
    }
  });

  await test("several_folds_read_as_a_list", async () => {
    const { context, page } = await ticketPage(host);
    try {
      const rows = () => page.locator(`${scopeT} .blk.det`).evaluateAll((els) => els.map((e) => {
        const s = getComputedStyle(e);
        const r = e.getBoundingClientRect();
        return { top: r.top, bottom: r.bottom, border: s.borderTopWidth, borderStyle: s.borderTopStyle, pad: s.paddingTop, bg: getComputedStyle(e.querySelector("m-details")).backgroundColor, box: getComputedStyle(e.querySelector("m-details")).borderTopStyle };
      }));
      const r = await rows();
      assert(r.length === 3, `three folds, three blocks: ${r.length}`);
      assert(r[0].border === "0px" && r[1].border === "1px" && r[1].borderStyle === "solid" && r[2].border === "1px", `a hairline between, none above the first: ${JSON.stringify(r)}`);
      assert(r.every((x) => /rgba\(0, 0, 0, 0\)|transparent/.test(x.bg) && x.box === "none"), `no card around each: ${JSON.stringify(r)}`);
      assert(r[1].pad === "0px" && r[2].pad === "0px", `tight: no gap but the line's own: ${JSON.stringify(r.map((x) => x.pad))}`);
      const lefts = (await folds(page, scopeT)).map((f) => f.left);
      assert(new Set(lefts).size === 1, `the sentences line up: ${lefts}`);
      await page.locator(`${scopeT} m-details[data-folds] .det-mark`).first().click();
      const after = (await folds(page, scopeT)).map((f) => f.left);
      assert(JSON.stringify(after) === JSON.stringify(lefts), `opening one moves no sentence sideways: ${lefts} → ${after}`);
      await page.locator(`${scopeT} m-details[data-folds] .det-mark`).first().click();
      // Three rows, one open (the second, by `open`): the screenshots.
      await shoot(page, "details-ticket");
    } finally {
      await context.close();
    }
  });

  await test("what_is_open_stays_open_across_a_live_update", async () => {
    const id = make("folds kept open", BODY);
    const { context, page } = await ticketPage(id);
    try {
      await page.locator(`${scopeT} m-details[data-folds] .det-mark`).first().click();
      await page.locator(`${scopeT} m-details[data-folds] .det-mark`).nth(1).click();
      let got = await folds(page, scopeT);
      assert(got[0].expanded === "true" && got[1].expanded === "false", "the first opened, the second closed");
      // Another writer's edit: a paragraph above every fold, and words inside the first.
      const edited = BODY.replace("Three lines of work.", "Three lines of work, amended.\n\nA line above them.").replace("for the glaze.", "for the glaze, twice.");
      sy("tickets", "amend", "--id", id, "--body", bodyFile("amended", edited));
      await eventually("the edit drawn", async () => (await page.locator(`${scopeT} [data-ticket-body]`).innerText()).includes("A line above them."), 10_000);
      await eventually("the first's edit drawn", async () => (await page.locator(`${scopeT} m-details[data-folds]`).first().locator(".det-body").innerText()).includes("twice"), 10_000);
      got = await folds(page, scopeT);
      assert(JSON.stringify(got.map((f) => f.expanded)) === JSON.stringify(["true", "false", "false"]), `what was open stays open, what was closed stays closed: ${JSON.stringify(got.map((f) => f.expanded))}`);
      assert(get(id).body === edited, "nothing written to the body by opening");
      // A reload forgets: the marks' own `open` again.
      await page.reload();
      await eventually("drawn again", async () => (await page.locator(`${scopeT} m-details[data-folds]`).count()) === 3, 15_000);
      got = await folds(page, scopeT);
      assert(JSON.stringify(got.map((f) => f.expanded)) === JSON.stringify(["false", "true", "false"]), `a reload is the body's own: ${JSON.stringify(got.map((f) => f.expanded))}`);
    } finally {
      await context.close();
    }
  });

  await test("find_in_page_reaches_closed_details_and_print_shows_them", async () => {
    const { context, page } = await ticketPage(host);
    try {
      assert((await folds(page, scopeT))[2].expanded === "false", "the third is closed");
      // Find-in-page: the browser's search reaches the words inside the closed third (it is `hidden=until-found`,
      // not `display:none`), and the `beforematch` the find bar fires on it opens it. (Playwright cannot drive
      // the find bar itself: `window.find` searches but fires no `beforematch`, so the event is sent as it would.)
      assert((await folds(page, scopeT))[2].hidden === "until-found", "closed, hidden until found");
      assert(await page.evaluate(() => window.find("kiln-opal")), "window.find reaches the closed words");
      await page.locator(`${scopeT} m-details[data-folds] > .det-body`).nth(2).evaluate((b) => b.dispatchEvent(new Event("beforematch")));
      await eventually("the match opened it", async () => (await folds(page, scopeT))[2].expanded === "true", 3_000);
      assert((await folds(page, scopeT))[2].hidden == null, "shown");
      // Printing: every fold open.
      await page.reload();
      await eventually("drawn again", async () => (await page.locator(`${scopeT} m-details[data-folds]`).count()) === 3, 15_000);
      await page.emulateMedia({ media: "print" });
      const h = (await folds(page, scopeT)).map((f) => f.height);
      assert(h.every((x) => x > 0), `printed, each shows what it holds: ${h}`);
      await page.emulateMedia({ media: "screen" });
      assert((await folds(page, scopeT))[0].height === 0, "on screen, closed again");
    } finally {
      await context.close();
    }
  });

  await test("in_the_editor_the_mark_opens_it_and_the_caret_stays_out", async () => {
    const docId = sy("docs", "create", "--title", "folds in a document", "--body", bodyFile("doc", BODY)).find((e) => e.type === "created")?.doc.id;
    const { context, page } = await openPage(`#/doc/${docId}`);
    try {
      await eventually("the folds drawn in the document", async () => (await page.locator(`${scopeD} m-details[data-folds]`).count()) === 3, 15_000);
      const head = () => page.evaluate(() => { const v = document.querySelector("[data-surface=editor-body] [data-field=body]").editorView; return { head: v.state.selection.main.head, focus: v.hasFocus }; });
      const sourceShown = () => page.locator(`${scopeD} .cm-line`).evaluateAll((ls) => ls.some((l) => l.textContent.includes("<m-details")));
      const was = await head();
      await page.locator(`${scopeD} m-details[data-folds] .det-mark`).first().click();
      let got = await folds(page, scopeD);
      assert(got[0].expanded === "true" && got[0].height > 0, `the mark opens it in the editor: ${JSON.stringify(got[0])}`);
      assert(!(await sourceShown()), "its source is not shown: the caret did not enter it");
      const now = await head();
      assert(now.head === was.head && !now.focus, `the caret stayed where it was: ${JSON.stringify(was)} → ${JSON.stringify(now)}`);
      await page.locator(`${scopeD} m-details[data-folds] .det-mark`).first().click();
      assert((await folds(page, scopeD))[0].expanded === "false", "and closes it");
      // A click on the sentence is the caret's: the block shows its source, as every block does.
      await page.locator(`${scopeD} m-details[data-folds] .det-sum`).first().click({ position: { x: 4, y: 4 } });
      await eventually("the caret's block is its source", sourceShown, 5_000);
      assert((await page.locator(`${scopeD} m-details[data-folds]`).count()) === 2, "the other two still drawn");
      // The caret out again: the fold drawn, as it was left (closed).
      await page.evaluate(() => { const v = document.querySelector("[data-surface=editor-body] [data-field=body]").editorView; v.dispatch({ selection: { anchor: v.state.doc.length } }); });
      await eventually("drawn again", async () => (await page.locator(`${scopeD} m-details[data-folds]`).count()) === 3, 5_000);
      await shoot(page, "details-doc");
    } finally {
      await context.close();
    }
  });

  await test("a_ticket_and_a_doc_draw_the_folds_the_same", async () => {
    const docId = sy("docs", "create", "--title", "the same folds", "--body", bodyFile("same", BODY)).find((e) => e.type === "created")?.doc.id;
    /** Each drawn block's HTML, Svelte's anchors and a bound mark's fill aside. */
    const drawnHtml = (page, scope) => page.locator(`${scope} .blk`).evaluateAll((els) => els.map((e) => {
      const c = e.cloneNode(true);
      for (const m of c.querySelectorAll("[data-component]")) { m.removeAttribute("data-layout"); m.replaceChildren(); }
      return c.innerHTML.replace(/<!--[^]*?-->/g, "");
    }));
    const settle = async (page, scope) => {
      await eventually(`${scope}: every pill`, async () => (await page.locator(`${scope} a.tpill[data-pill]`).count()) === 3, 15_000);
      await sleep(300);
      return drawnHtml(page, scope);
    };
    const t = await ticketPage(host);
    const asTicket = await settle(t.page, scopeT);
    await t.context.close();
    const d = await openPage(`#/doc/${docId}`);
    try {
      const asDoc = await settle(d.page, scopeD);
      assert(asTicket.length === asDoc.length && asTicket.length === 5, `as many blocks: ${asTicket.length} · ${asDoc.length}`);
      for (let k = 0; k < asTicket.length; k++) assert(asTicket[k] === asDoc[k], `block ${k} the same:\n  ticket ${asTicket[k].slice(0, 400)}\n  doc    ${asDoc[k].slice(0, 400)}`);
    } finally {
      await d.context.close();
    }
  });
};
