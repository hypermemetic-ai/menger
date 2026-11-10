// EDITOR-8: the rendering is the editor — editor-body at #/edit/:id on the
// throwaway: the ticket drawn by ticket-view, a CodeMirror 6 live preview
// over the body's markdown — every block the caret is not in drawn as
// ticket-view draws it (`.blk`), the caret's block its markdown (`.cm-line`);
// one amend 600 ms after the last key; a refusal on the line under the canvas;
// an outside amend landing without moving the caret; a dirty draft merged with
// an outside write at the pause; house marks drawn as cards, kept byte for
// byte; the documents beside it with + new. V2-29: the documents are the
// docs service's — + new makes a doc, typing reaches it as Yjs updates
// (`docs.update`), `#/doc/:id` reloads it, two pages on one doc type together
// and see each other here, and a ticket still edits at `#/edit/:id`. Run alone by
// `node tests/pages_browser.mjs --group editor`.
//
// Marks: `[data-surface=editor-body]`, `[data-surface=ticket-view][data-editing]`,
// `[data-field=title]` (ticket-view's heading, editable), `[data-field=body]` (its body,
// holding the editor; `.editorView` its view), `.cm-content[data-field=body-source]`,
// `.blk[data-block]` (a drawn block), `[data-field=write-state][data-state=idle|saving|saved|
// refused|merged|outside]`, `[data-refused]`, `[data-hand=NAME]`, `[data-action=revisions]`,
// `[data-loading]`, `[data-empty]`; the screen's `[data-field=documents]`, `[data-doc=ID]`,
// `[data-action=new]`, `[data-field=find]`, `[data-action=docs]`.
import { spawn } from "node:child_process";
import { makeKey, writeSession } from "../../scripts/throwaway-login.mjs";
import { sentCalls, synapse } from "../../scripts/ui-throwaway.mjs";

const EPIC = "EDIT";
const PAUSE = 600;

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, fs, path, session, server, fake } = ctx;
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
  const BODY = "[data-surface=editor-body] [data-field=body]";
  /** The component modules the page fetched, by name (UI-19: a file is asked by its content URL, read back
   *  through the page's files map to its version's path). */
  const modulesLoaded = (page) => page.evaluate(() => {
    const imports = JSON.parse(document.getElementById("plexus-ui-files")?.textContent || "{}").imports || {};
    const back = {};
    // A file is named twice — `/app/f/<name>.js` and `/app/<version>/components/<name>.js` — in an order that
    // follows the version's hash: the components/ name wins whichever comes first.
    for (const [p, url] of Object.entries(imports)) { const at = new URL(url, location.href).pathname; if (!back[at] || /components\//.test(p)) back[at] = p; }
    return performance.getEntriesByType("resource").map((r) => { const at = new URL(r.name).pathname; return back[at] ?? at; })
      .map((p) => /components\/([a-z0-9-]+)\.js$/.exec(p)?.[1]).filter(Boolean);
  });
  const open = async (id, route = `#/edit/${id}`) => {
    const { context, page } = await openPage(route);
    const ed = page.locator("[data-surface=editor-body]");
    await eventually("the editor is drawn", async () => (await ed.locator(".cm-content[data-field=body-source]").count()) === 1);
    return { context, page, ed, content: ed.locator(".cm-content") };
  };
  /** The editor's document and caret, read from its view. */
  const src = (page) => page.evaluate((s) => document.querySelector(s).editorView.state.doc.toString(), BODY);
  const caret = (page) => page.evaluate((s) => document.querySelector(s).editorView.state.selection.main.head, BODY);
  /** The caret at `pos` in the source, the editor focused (as a click would leave it). */
  const caretAt = (page, pos) => page.evaluate(({ s, pos }) => { const v = document.querySelector(s).editorView; v.focus(); v.dispatch({ selection: { anchor: pos } }); }, { s: BODY, pos });
  const state = (ed) => ed.locator("[data-field=write-state]");
  /** Each drawn block's HTML, Svelte's anchors (comments) aside. */
  const drawnHtml = (page, scope) => page.locator(`${scope} .blk`).evaluateAll((els) => els.map((e) => {
    const c = e.cloneNode(true);
    for (const m of c.querySelectorAll("[data-component]")) { m.removeAttribute("data-layout"); m.replaceChildren(); } // a bound mark's fill is its own
    return c.innerHTML.replace(/<!--[^]*?-->/g, "");
  }));
  const shots = process.env.PAGES_SHOTS;
  /** `name`-1440-light … `name`-390-dark: both widths, both themes, the transitions ended; the page left at 1440, light. */
  const shoot = async (page, name, fullPage = false) => {
    if (!shots) return;
    for (const [w, h] of [[1440, 900], [390, 844]]) for (const theme of ["light", "dark"]) {
      await page.setViewportSize({ width: w, height: h });
      await page.evaluate((th) => (document.documentElement.dataset.theme = th), theme);
      await sleep(300);
      await page.screenshot({ path: path.join(shots, `${name}-${w}-${theme}.png`), fullPage });
    }
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.evaluate(() => (document.documentElement.dataset.theme = "light"));
  };

  const HOUSE = `<m-summary>

<m-field data-field="standing">**The kiln fires twice a week.**</m-field>

<m-stat value="2">firings a week</m-stat>
<m-stat value="6" tone="pass">cone</m-stat>

</m-summary>

## The glaze

A paragraph to *edit*, with \\escapes\\ and  two spaces.

<m-decision data-field="decision" id="cone">
<m-question data-field="question">Which cone?</m-question>
<m-option data-field="option" name="Six" value="6" recommended></m-option>
<m-option data-field="option" name="Five" value="5"></m-option>
<m-recommendation data-field="recommendation" by="builder">**Six.** It holds.</m-recommendation>
</m-decision>

<m-cases label="the firings">

| a | b |
| -- | -- |
| 1 | 2 |

</m-cases>

\`\`\`mermaid
graph TD
  A --> B
\`\`\`

<m-status data-field="status">

<m-step date="2026-11-02T07:31Z" status="Ready" by="owner">"make it so"</m-step>

</m-status>
`;

  await test("the_surface_draws_every_block_as_the_view_does", async () => {
    const id = make("the same drawing", HOUSE);
    const { context, page, ed } = await open(id);
    try {
      await eventually("every block drawn while the focus is elsewhere", async () => (await ed.locator(".blk").count()) > 7 && (await ed.locator(".cm-line").count()) === 0, 5_000);
      const pieces = await ed.locator(".blk").count();
      // The cards: the house's marks, kept by the sanitizer, drawn by ticket-view's sheet.
      for (const sel of ["m-summary m-field[data-field=standing] strong", "m-summary m-stat[value='6'][tone=pass]", "m-decision[data-id=cone] m-option[name=Six][recommended]", "m-cases table", "m-status m-step[status=Ready][by=owner]", "pre code.language-mermaid", "[data-surface=mermaid-diagram][data-state=drawn] svg"]) {
        await eventually(`drawn: ${sel}`, async () => (await ed.locator(`.blk ${sel}`).count()) === 1, 10_000);
      }
      assert((await ed.locator(".blk [onclick], .blk script, .blk [data-slot]:not([data-component=mermaid-diagram])").count()) === 0, "nothing live in a drawn block (a diagram's bound mark aside: EDITOR-9)");
      const card = await ed.locator("m-summary").first().evaluate((e) => getComputedStyle(e).borderLeftStyle);
      assert(card === "none", "a summary is typography, no box (EDITOR-9)");
      const mine = await drawnHtml(page, "[data-surface=editor-body]");
      await shoot(page, "editor-surface");
      // The ticket page draws the same blocks, byte for byte.
      await page.evaluate((id) => (location.hash = `#/ticket/${id}`), id);
      await eventually("the view", async () => (await page.locator("[data-surface=ticket-body] .blk").count()) === pieces, 5_000);
      const theirs = await drawnHtml(page, "[data-surface=ticket-body]");
      assert(JSON.stringify(mine) === JSON.stringify(theirs), `the editor draws what the view draws:\n${JSON.stringify(mine).slice(0, 400)}\n${JSON.stringify(theirs).slice(0, 400)}`);
      await sleep(PAUSE + 200);
      assert(get(id).body === HOUSE && (await amends(page)).length === 0, "looking writes nothing");
    } finally {
      await context.close();
    }
  });

  await test("a_click_puts_the_caret_in_the_block_and_typing_saves", async () => {
    const id = make("type into it", "# The editor\n\nSome *words* here.\n\nlast\n");
    const { context, page, ed } = await open(id);
    try {
      const para = ed.locator(".blk p").filter({ hasText: "Some words here." });
      await eventually("drawn", async () => (await para.count()) === 1);
      // A click at the end of the drawn text: the block opens as its markdown, the caret where it was clicked.
      const box = await para.boundingBox();
      const words = await para.evaluate((p) => { const r = document.createRange(); r.selectNodeContents(p); const b = r.getBoundingClientRect(); return { x: b.right - 2, y: b.top + b.height / 2 }; });
      await page.mouse.click(words.x, words.y);
      await eventually("its markdown, in place", async () => (await ed.locator(".cm-line").filter({ hasText: "Some" }).count()) === 1, 3_000);
      assert((await ed.locator(".blk").count()) === 2, "the other two blocks stay drawn");
      assert((await ed.locator(".eb-em").innerText()) === "words", "the emphasis formed in the source");
      const at = await caret(page);
      assert(at >= "# The editor\n\nSome *words* here".length - 1 && at <= "# The editor\n\nSome *words* here.".length, `the caret where it was clicked: ${at}`);
      await page.keyboard.press("End");
      await page.keyboard.type(" And more.");
      await eventually("saved", async () => (await state(ed).getAttribute("data-state")) === "saved", 5_000);
      assert(get(id).body === "# The editor\n\nSome *words* here. And more.\n\nlast\n", `the source exact: ${JSON.stringify(get(id).body)}`);
      const sent = await amends(page);
      assert(sent.length === 1, `one amend, the body whole: ${sent.length}`);
      assert(box != null, "a box");
    } finally {
      await context.close();
    }
  });

  await test("a_heading_by_hash_space_and_the_marks_by_keys", async () => {
    const id = make("shortcuts", "first\n");
    const { context, page, ed } = await open(id);
    try {
      await caretAt(page, "first".length);
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("# A heading");
      await eventually("a heading line, formed", async () => (await ed.locator(".cm-line.eb-h1").count()) === 1, 3_000);
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("- one");
      await page.keyboard.press("Enter");
      await page.keyboard.type("two");
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await page.keyboard.type("bold");
      await page.keyboard.press("Shift+Home");
      await page.keyboard.press("Control+b");
      await page.keyboard.press("End");
      await page.keyboard.type(" and ");
      await page.keyboard.press("Control+i");
      await page.keyboard.type("it");
      await eventually("saved", async () => { const b = get(id).body; return b.startsWith("first\n\n# A heading\n\n- one\n- two\n") && b.trimEnd().endsWith("\n**bold** and *it*"); }, 5_000);
      await page.keyboard.press("Escape");
      await eventually("drawn once the focus leaves", async () => (await ed.locator(".blk h1").innerText()) === "A heading" && (await ed.locator(".blk li").count()) === 2 && (await ed.locator(".blk strong").innerText()) === "bold", 3_000);
    } finally {
      await context.close();
    }
  });

  await test("one_amend_per_pause_and_the_title_too", async () => {
    const id = make("the pause", "a line");
    const { context, page, ed } = await open(id);
    try {
      await caretAt(page, "a line".length);
      await page.keyboard.type(" and some", { delay: 30 });
      await sleep(PAUSE / 3);
      assert((await amends(page)).length === 0, "nothing sent while the keys keep coming");
      await page.keyboard.type(" more", { delay: 30 });
      await eventually("saved", async () => (await state(ed).getAttribute("data-state")) === "saved", 5_000);
      assert((await amends(page)).length === 1, `one amend for the whole burst: ${(await amends(page)).length}`);
      assert(get(id).body === "a line and some more", `the store holds the burst: ${JSON.stringify(get(id).body)}`);
      // The title: ticket-view's heading, edited in place, the same pause.
      const title = ed.locator("[data-surface=ticket-view] [data-field=title]");
      assert((await title.innerText()) === "the pause", "the title drawn in the view's heading");
      await title.click();
      await page.keyboard.press("End");
      await page.keyboard.type(", renamed");
      await eventually("the title lands", async () => get(id).title === "the pause, renamed", 5_000);
      const sent = await amends(page);
      assert(sent.length === 2 && sent[1].params.title === "the pause, renamed" && sent[1].params.body == null, `the title's own amend: ${JSON.stringify(sent.map((s) => Object.keys(s.params)))}`);
      // Enter takes the caret to the body; a blur sends at once.
      await page.keyboard.press("Enter");
      await caretAt(page, (await src(page)).length);
      await page.keyboard.type("!");
      await title.click();
      await eventually("the blur's amend", async () => get(id).body === "a line and some more!", 2_000);
    } finally {
      await context.close();
    }
  });

  await test("a_refusal_is_said_under_the_canvas_and_the_draft_stays", async () => {
    const id = make("refused", "fine\n");
    const { context, page, ed } = await open(id);
    try {
      await caretAt(page, "fine\n".length);
      // Two inputs with one id: the service refuses the write, naming it (V2-20).
      await page.keyboard.insertText('\n<m-input id="twice" kind="text"><m-question>a?</m-question></m-input>\n\n<m-input id="twice" kind="text"><m-question>b?</m-question></m-input>\n');
      await eventually("the refusal under the canvas", async () => (await ed.locator("[data-field=write-state][data-refused]").count()) === 1, 5_000);
      const said = await state(ed).innerText();
      assert(/twice/.test(said), `the service's words name the input: ${said}`);
      assert(get(id).body === "fine\n", "the store is as it was");
      assert((await src(page)).includes('id="twice"'), "the draft stays in the editor");
      if (shots) await page.screenshot({ path: path.join(shots, "editor-body-refused.png") });
    } finally {
      await context.close();
    }
  });

  await test("an_outside_amend_lands_without_moving_the_caret", async () => {
    const id = make("two hands", "# Head\n\nfirst line\n\nlast line\n");
    const { context, page, ed } = await open(id);
    try {
      await caretAt(page, "# He".length);
      sy("tickets", "amend", "--id", id, "--body", bodyFile("outside", "intro, added outside\n\n# Head\n\nfirst line\n\nlast line, changed outside\n"));
      await eventually("the outside change is in the editor", async () => (await src(page)) === "intro, added outside\n\n# Head\n\nfirst line\n\nlast line, changed outside\n", 5_000);
      await eventually("the line names the hand", async () => (await state(ed).getAttribute("data-state")) === "outside" && (await state(ed).getAttribute("data-hand")) === "owner", 5_000);
      assert((await caret(page)) === "intro, added outside\n\n# He".length, `the caret moved with its text: ${await caret(page)}`);
      assert((await ed.locator(".blk").filter({ hasText: "changed outside" }).count()) === 1, "the changed block drawn anew");
      await page.keyboard.type("!");
      await eventually("our key lands where the caret was", async () => get(id).body === "intro, added outside\n\n# He!ad\n\nfirst line\n\nlast line, changed outside\n", 5_000);
      assert((await amends(page)).length === 1, "one amend of ours; the outside one was never resent");
    } finally {
      await context.close();
    }
  });

  await test("a_dirty_draft_merges_an_outside_write_at_the_pause", async () => {
    const id = make("merged", "# Head\n\nfirst line\n\nlast line\n");
    const { context, page, ed } = await open(id);
    try {
      await caretAt(page, "# Head".length);
      await page.keyboard.type(" o");
      const theirs = outside(id, bodyFile("theirs", "# Head\n\nfirst line\n\nlast line, theirs\n"));
      await page.keyboard.type("ur", { delay: 150 });
      await theirs;
      await page.keyboard.type("s");
      await eventually("the merge is said", async () => (await state(ed).getAttribute("data-state")) === "merged", 8_000);
      assert(get(id).body === "# Head ours\n\nfirst line\n\nlast line, theirs\n", `both hands in the store: ${JSON.stringify(get(id).body)}`);
      assert((await src(page)).endsWith("last line, theirs\n"), "theirs is in the editor");
      assert(/owner/.test(await state(ed).innerText()), `the line names whose: ${await state(ed).innerText()}`);
      await page.keyboard.type("!");
      const clash = outside(id, bodyFile("clash", "# Head theirs\n\nfirst line\n\nlast line, theirs\n"));
      await page.keyboard.type("!", { delay: 150 });
      await clash;
      await page.keyboard.type("!");
      await eventually("ours won the line", async () => get(id).body === "# Head ours!!!\n\nfirst line\n\nlast line, theirs\n", 8_000);
      await eventually("the line says what the revision keeps", async () => /revision/.test(await state(ed).innerText()), 5_000);
    } finally {
      await context.close();
    }
  });

  await test("house_blocks_survive_an_edit_byte_for_byte", async () => {
    const id = make("kept", HOUSE);
    const { context, page, ed } = await open(id);
    try {
      // Into the paragraph, a word typed; every other byte the store held, unchanged.
      const at = HOUSE.indexOf("to *edit*") + 3;
      await caretAt(page, at);
      await page.keyboard.type("really ");
      const want = HOUSE.slice(0, at) + "really " + HOUSE.slice(at);
      await eventually("saved", async () => get(id).body === want, 5_000);
      // Into a house mark: its source when the caret is in it, its tag and attributes untouched by an edit of its words.
      const q = HOUSE.indexOf("Which cone?") + "Which cone".length;
      await caretAt(page, q + "really ".length);
      await eventually("the question's line as its source, alone", async () => (await ed.locator(".cm-line").filter({ hasText: "<m-question" }).count()) === 1, 3_000);
      assert((await ed.locator(".cm-line").filter({ hasText: "<m-decision" }).count()) === 0 && (await ed.locator(".cm-line").filter({ hasText: "<m-option" }).count()) === 0, "the mark's other lines stay drawn");
      await page.keyboard.type(", in the end");
      const want2 = want.replace("Which cone?", "Which cone, in the end?");
      await eventually("saved again", async () => get(id).body === want2, 5_000);
      for (const tag of HOUSE.match(/<\/?m-[^>]*>/g)) assert(get(id).body.includes(tag), `kept: ${tag}`);
      await page.keyboard.press("Escape");
      await eventually("the card again, its question changed", async () => (await ed.locator(".blk m-decision m-question").innerText()) === "Which cone, in the end?", 3_000);
      await shoot(page, "editor-cards");
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

  // ── the screen: one surface, the documents beside it ─────────────────────
  const hash = (page) => page.evaluate(() => location.hash);

  await test("one_surface_and_the_documents_beside_it", async () => {
    const id = make("the first screen", "# One surface\n\nwritten where it is read\n");
    const { context, page } = await openPage(`#/edit/${id}`);
    try {
      const screen = page.locator("[data-surface=editor-screen]");
      await eventually("the surface", async () => (await screen.locator("[data-surface=editor-body] [data-surface=ticket-view][data-editing] .cm-content").count()) === 1);
      assert((await screen.locator("[data-surface=ticket-body], [data-surface=ticket-head], [data-half]").count()) === 0, "nothing drawn twice: no second pane");
      assert((await screen.locator("[data-surface=ticket-questions]").count()) === 1, "the questions under it");
      assert((await page.locator("[data-layout=page-editor][data-slot=top] [data-surface=app-top]").count()) === 1, "the app's top on the page");
      await eventually("the document in the list, recent", async () => (await screen.locator(`[data-field=documents] [data-doc=${id}][data-recent][data-current]`).count()) === 1, 5_000);
      await page.evaluate(() => (location.hash = "#/"));
      await eventually("#/ opens the last document", async () => (await screen.getAttribute("data-ticket")) === id, 5_000);
      await shoot(page, "editor-screen");
    } finally {
      await context.close();
    }
  });

  // ── V2-29: documents of the docs service, on the same surface ─────────────
  const docOf = (id) => sy("docs", "get", "--id", id).find((e) => e.type === "doc")?.doc;
  const docUpdates = async (page) => (await sentCalls(page)).filter((c) => c.method === "docs.update");
  const openDoc = async (id, page0) => {
    const { context, page } = page0 ?? (await openPage(`#/doc/${id}`));
    const ed = page.locator(`[data-surface=editor-body][data-kind=doc][data-ticket="${id}"]`);
    await eventually("the doc is drawn", async () => (await ed.locator(".cm-content[data-field=body-source]").count()) === 1, 8_000);
    return { context, page, ed };
  };

  await test("new_makes_a_doc_and_typing_reaches_the_docs_service", async () => {
    const first = make("switch from here", "start\n");
    const { context, page } = await open(first);
    try {
      const screen = page.locator("[data-surface=editor-screen]");
      await eventually("new is ready", async () => !(await screen.locator("[data-field=documents] [data-action=new]").isDisabled()), 5_000);
      await screen.locator("[data-field=documents] [data-action=new]").click();
      await eventually("the new doc opens at #/doc/<id>", async () => /^#\/doc\/[0-9a-z]{8}$/.test(await hash(page)), 5_000);
      const made = (await hash(page)).replace("#/doc/", "");
      const d = docOf(made);
      assert(d && d.title === "Untitled" && d.revision === 1, `a doc called Untitled: ${JSON.stringify(d)}`);
      assert(!sy("tickets", "list", "--filter", "kind doc", "--summary", "true").some((e) => e.summary?.title === "Untitled" && e.summary?.created_unix > Date.now() / 1000 - 60), "no ticket was made");
      await eventually("in the documents", async () => (await screen.locator(`[data-field=documents] [data-doc="${made}"]`).count()) >= 1, 5_000);
      const { ed } = await openDoc(made, { context, page });
      assert((await screen.locator("[data-surface=ticket-questions]").count()) === 0, "a doc has no questions");
      await caretAt(page, 0);
      await page.keyboard.type("a new page");
      await eventually("typing reaches the doc", async () => docOf(made).body === "a new page", 5_000);
      const sent = await docUpdates(page);
      assert(sent.length >= 1 && sent.every((c) => typeof c.params.update === "string" && c.params.session), `as Yjs updates with the page's session: ${sent.length}`);
      assert((await amends(page)).length === 0, "and never tickets.amend");
      await eventually("said under the canvas", async () => (await state(ed).getAttribute("data-state")) === "saved", 5_000);
      // Straight to the ticket, inside a batch: what was typed lands on its own doc.
      await caretAt(page, "a new page".length);
      await page.keyboard.type(" more");
      await screen.locator(`[data-field=documents] [data-doc=${first}]`).first().click();
      // V2-42: by its uid, `#/t/<uid>/<ID>-<slug>/edit` (the recent row carries the uid), else by its id.
      await eventually("the ticket again, in the editor", async () => { const h = await hash(page); return h === `#/edit/${first}` || new RegExp(`^#/t/[0-9a-z]{8}/${first}(-[a-z0-9-]*)?/edit$`).test(h); }, 5_000);
      await eventually("the doc kept what was typed", async () => docOf(made).body === "a new page more", 5_000);
      assert(get(first).body === "start\n", "and the ticket is untouched");
      // The search finds docs by title.
      sy("docs", "rename", "--id", made, "--title", "The kiln, weekly");
      await screen.locator("[data-field=find]").fill("kiln week");
      await eventually("found", async () => (await screen.locator(`[data-field=documents] [data-doc="${made}"]`).count()) === 1, 5_000);
      await shoot(page, "editor-docs");
    } finally {
      await context.close();
    }
  });

  await test("a_doc_reloads_at_its_route", async () => {
    const id = sy("docs", "create", "--title", "Reloaded", "--body", bodyFile("reloaded", "# Kept\n\nas written\n")).find((e) => e.type === "created").doc.id;
    const { context, page, ed } = await openDoc(id);
    try {
      await eventually("drawn as the view draws it", async () => (await ed.locator(".blk h1").count()) === 1, 5_000);
      await caretAt(page, (await src(page)).length);
      await page.keyboard.type("more");
      await eventually("kept", async () => docOf(id).body === "# Kept\n\nas written\nmore", 5_000);
      await page.reload();
      await openDoc(id, { context, page });
      await eventually("the same after a reload", async () => (await src(page)) === "# Kept\n\nas written\nmore", 5_000);
      // An agent's whole-body save over synapse arrives on the open page, merged.
      sy("docs", "save", "--id", id, "--body", bodyFile("reloaded-2", "# Kept\n\nas written by an agent\nmore"));
      await eventually("the agent's words arrive live", async () => (await src(page)).includes("by an agent"), 5_000);
      // The history scrubber replays it at its first change.
      await ed.locator("[data-action=history]").click();
      await eventually("the scrubber", async () => (await ed.locator("[data-field=scrubber]").count()) === 1, 5_000);
      await ed.locator("[data-field=scrubber]").evaluate((el) => { el.value = "0"; el.dispatchEvent(new Event("input", { bubbles: true })); });
      await eventually("the doc as it was made", async () => (await ed.locator("[data-field=replayed]").innerText()).includes("as written") && !(await ed.locator("[data-field=replayed]").innerText()).includes("agent"), 5_000);
      await shoot(page, "editor-doc-history");
    } finally {
      await context.close();
    }
  });

  await test("two_pages_on_one_doc_type_together_and_see_each_other", async () => {
    const id = sy("docs", "create", "--title", "Paired", "--body", bodyFile("paired", "left right\n")).find((e) => e.type === "created").doc.id;
    const one = await openDoc(id);
    const two = await openDoc(id);
    try {
      await eventually("each sees two here", async () => (await one.ed.locator("[data-field=present] [data-present]").count()) === 2 && (await two.ed.locator("[data-field=present] [data-present]").count()) === 2, 8_000);
      // Both type at once, at the two ends.
      await caretAt(one.page, 0);
      await caretAt(two.page, "left right".length);
      await Promise.all([one.page.keyboard.type("ONE "), two.page.keyboard.type(" TWO")]);
      const want = "ONE left right TWO\n";
      await eventually("both pages and the service converge", async () => (await src(one.page)) === want && (await src(two.page)) === want && docOf(id).body === want, 8_000);
      // A caret of the other, in their colour.
      await eventually("the other's caret", async () => (await one.page.locator(".cm-ySelectionCaret").count()) >= 1, 5_000);
      await shoot(one.page, "editor-doc-paired");
      await two.context.close();
      await eventually("they leave", async () => (await one.ed.locator("[data-field=present] [data-present]").count()) === 1, 8_000);
    } finally {
      await one.context.close();
      await two.context.close().catch(() => {});
    }
  });

  await test("a_phone_has_the_documents_in_a_drawer", async () => {
    const id = make("on a phone", "# Phone\n\none surface\n");
    const { context, page } = await openPage(`#/edit/${id}`);
    try {
      await page.setViewportSize({ width: 390, height: 844 });
      const screen = page.locator("[data-surface=editor-screen]");
      await eventually("the surface", async () => await screen.locator("[data-surface=editor-body] .cm-content").isVisible());
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
      assert(overflow <= 0, `no page scroll sideways: ${overflow}`);
      assert(!(await screen.locator("[data-field=documents]").isVisible()) || (await screen.locator("[data-field=documents]").boundingBox()).x < -100, "the list is away");
      await screen.locator("[data-action=docs]").click();
      await eventually("the drawer", async () => (await screen.getAttribute("data-drawer")) === "open" && (await screen.locator("[data-field=documents]").boundingBox()).x >= 0, 3_000);
      if (shots) await page.screenshot({ path: path.join(shots, "editor-screen-390-drawer.png") });
    } finally {
      await context.close();
    }
  });

  await test("escape_twice_goes_to_the_board", async () => {
    const id = make("one key from the board", "# Esc\n");
    const { context, page } = await openPage(`#/edit/${id}`);
    try {
      await page.locator("[data-surface=editor-body] .cm-content").waitFor();
      await caretAt(page, 2);
      await page.keyboard.press("Escape");
      await page.keyboard.press("Escape");
      await eventually("the board", async () => (await hash(page)) === "#/board" && (await page.locator("[data-layout=board-frame]").count()) > 0, 5_000);
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
    const { context, page } = await open(id);
    try {
      await page.evaluate((s) => { const v = document.querySelector(s).editorView; v.focus(); v.dispatch({ selection: { anchor: 4, head: 12 } }); }, BODY);
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

  await test("a_pasted_image_lands_as_media_and_is_drawn", async () => {
    const id = make("an image", "above\n\nbelow\n");
    const { context, page, ed } = await open(id);
    try {
      await caretAt(page, "above".length);
      assert((await pasteFile(page, PNG_2X1, "image/png", "the curve.png")) === 1, "the upload card stands at once");
      assert(!get(id).body.includes("uploading"), "no placeholder in the source");
      await eventually("the source names the image", async () => /^above\n\n!\[the curve\]\(media:[0-9a-f]{64}\)\n\nbelow\n$/.test(get(id).body), 8_000);
      assert((await ed.locator("[data-upload]").count()) === 0, "the card is gone");
      const hash = /media:([0-9a-f]{64})/.exec(get(id).body)[1];
      await caretAt(page, 0);
      const img = ed.locator(".blk img");
      await eventually("drawn as its block, from the shell", async () => (await img.count()) === 1 && (await img.getAttribute("src")).endsWith(`/media/${hash}`), 5_000);
      assert((await img.evaluate((i) => new Promise((r) => (i.complete ? r(i.naturalWidth) : (i.onload = () => r(i.naturalWidth)))))) === 2, "with its pixels");
    } finally {
      await context.close();
    }
  });

  await test("a_selection_on_a_doc_is_asked_in_the_margin_and_answered_live", async () => {
    const id = sy("docs", "create", "--title", "Asked", "--body", bodyFile("asked", "The kiln fires twice a week.\n\nAnother paragraph.\n")).find((e) => e.type === "created").doc.id;
    const { context, page, ed } = await openDoc(id);
    try {
      // Select "twice a week" and ask an agent about it.
      await page.evaluate(({ s }) => { const v = document.querySelector(s).editorView; v.focus(); v.dispatch({ selection: { anchor: 15, head: 27 } }); }, { s: BODY });
      await ed.locator("[data-action=ask-open]").click();
      await ed.locator("[data-ask] [data-field=instruction]").fill("Still twice?");
      await ed.locator("[data-ask] [data-field=to-whom]").selectOption("agent");
      await ed.locator("[data-ask] [data-action=ask]").click();
      const card = ed.locator("[data-field=margin] [data-thread]");
      await eventually("a card in the margin", async () => (await card.count()) === 1 && (await card.innerText()).includes("twice a week"), 5_000);
      const thread = await card.getAttribute("data-thread");
      const waiting = sy("docs", "waiting", "--to", "agent").filter((e) => e.type === "thread");
      assert(waiting.some((e) => e.thread.thread === thread && e.thread.quote === "twice a week"), "in the agents' queue");
      // The card stands beside its range.
      const [cardTop, rangeTop] = await page.evaluate(({ s, t }) => {
        const v = document.querySelector(s).editorView;
        return [document.querySelector(`[data-thread="${t}"]`).getBoundingClientRect().top, v.coordsAtPos(15).top];
      }, { s: BODY, t: thread });
      assert(Math.abs(cardTop - rangeTop) < 40, `beside its text: card ${cardTop}, range ${rangeTop}`);
      // An agent answers over synapse: it arrives live.
      sy("docs", "reply", "--id", id, "--thread", thread, "--body", "Yes, until the spring.");
      await eventually("the answer, live", async () => (await card.innerText()).includes("until the spring"), 5_000);
      // Someone types before the range: the range follows the words.
      await caretAt(page, 0);
      await page.keyboard.type("Note: ");
      await eventually("the range moved with the text", async () => {
        const t = sy("docs", "threads", "--id", id).find((e) => e.type === "thread")?.thread;
        return t && t.from === 21 && t.to === 33;
      }, 5_000);
      await caretAt(page, 25);
      await eventually("underlined in the caret's block", async () => (await ed.locator(`.dm-range[data-thread="${thread}"]`).innerText().catch(() => "")) === "twice a week", 5_000);
      // Answered here too, then resolved: it folds, counted.
      await card.locator("[data-field=reply]").fill("Thanks.");
      await card.locator("[data-action=reply]").click();
      await eventually("two answers", async () => (await card.locator("[data-reply]").count()) === 2, 5_000);
      await shoot(page, "editor-doc-margin");
      await card.locator("[data-action=resolve]").click();
      await eventually("folded and counted", async () => (await ed.locator("[data-field=margin] [data-thread]").count()) === 0 && (await ed.locator("[data-field=resolved-count]").innerText()).includes("1 resolved"), 5_000);
    } finally {
      await context.close();
    }
  });

  await test("a_pasted_image_in_a_doc_is_the_docs", async () => {
    const id = sy("docs", "create", "--title", "With a picture", "--body", bodyFile("picture", "above\n")).find((e) => e.type === "created").doc.id;
    const { context, page, ed } = await openDoc(id);
    try {
      await caretAt(page, "above".length);
      await pasteFile(page, PNG_2X1, "image/png", "the curve.png");
      await eventually("the doc names the image", async () => /!\[the curve\]\(media:[0-9a-f]{64}\)/.test(docOf(id).body), 8_000);
      const put = (await sentCalls(page)).find((c) => c.method === "media.put");
      assert(put.params.doc === id && put.params.ticket === undefined, `put for the doc: ${JSON.stringify(Object.keys(put.params))}`);
      await caretAt(page, 0);
      await eventually("drawn from the shell", async () => (await ed.locator(".blk img").count()) === 1, 5_000);
    } finally {
      await context.close();
    }
  });

  await test("a_truncated_image_is_said_and_leaves_no_line", async () => {
    const id = make("no image", "text\n");
    const { context, page, ed } = await open(id);
    try {
      await caretAt(page, 4);
      // A PNG's signature and no header: the one kind of file the media service refuses.
      await pasteFile(page, btoa(String.fromCharCode(0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, 0x49, 0x48)), "image/png", "cut.png");
      await eventually("the refusal stands where it was pasted", async () => /truncated/.test((await ed.locator("[data-upload][data-refused]").textContent({ timeout: 500 })) ?? ""), 5_000);
      assert(get(id).body === "text\n", "nothing written");
      await page.keyboard.type("x");
      await eventually("it goes with the next key", async () => (await ed.locator("[data-upload]").count()) === 0, 3_000);
    } finally {
      await context.close();
    }
  });

  // ── EDITOR-10: a doc absorbs anything from outside ────────────────────────
  /** A paste of several things at once: `items` [{ b64?, bytes?: n, head?, type, name }], and `data` { type: text }. */
  const pasteAll = (page, items, data = {}) => page.evaluate(({ items, data }) => {
    const dt = new DataTransfer();
    for (const it of items) {
      let bytes;
      if (it.b64) bytes = Uint8Array.from(atob(it.b64), (c) => c.charCodeAt(0));
      else {
        bytes = new Uint8Array(it.bytes);
        for (let i = 0; i < bytes.length; i += 4096) bytes[i] = (i / 4096) % 251;
        const head = Uint8Array.from(atob(it.head), (c) => c.charCodeAt(0));
        bytes.set(head, 0);
      }
      dt.items.add(new File([bytes], it.name, { type: it.type }));
    }
    for (const [k, v] of Object.entries(data)) dt.setData(k, v);
    document.querySelector("[data-surface=editor-body] .cm-content").dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
  }, { items, data });
  const MP4_HEAD = btoa(String.fromCharCode(0, 0, 0, 0x20) + "ftypisom");
  const away = async (page) => { await page.evaluate((s) => document.querySelector(s).editorView.contentDOM.blur(), BODY); await sleep(50); };

  await test("a_large_video_goes_up_in_chunks_with_progress_and_plays", async () => {
    const id = make("a video", "above\n");
    const { context, page, ed } = await open(id);
    try {
      await caretAt(page, "above".length);
      const seen = [];
      await page.exposeFunction("sawProgress", (p) => seen.push(p));
      await page.evaluate(() => new MutationObserver(() => { const p = document.querySelector("[data-upload][data-progress]")?.getAttribute("data-progress"); if (p) window.sawProgress(Number(p)); }).observe(document.body, { subtree: true, attributes: true, childList: true }));
      await pasteAll(page, [{ bytes: 9 * 1024 * 1024 + 7, head: MP4_HEAD, type: "video/mp4", name: "walk through.mp4" }]);
      await eventually("the source names the file", async () => /^above\n\n\[walk through\.mp4\]\(media:[0-9a-f]{64}\)\n?$/.test(get(id).body), 60_000);
      const calls = (await sentCalls(page)).map((c) => c.method);
      assert(calls.includes("media.begin") && calls.filter((m) => m === "media.chunk").length === 3 && calls.includes("media.finish") && !calls.includes("media.put"), `in chunks: ${calls.filter((m) => m.startsWith("media.")).join(" ")}`);
      assert(seen.some((p) => p > 0 && p < 100), `progress said: ${seen.join(",")}`);
      const hash = /media:([0-9a-f]{64})/.exec(get(id).body)[1];
      await away(page);
      const video = ed.locator(`.blk [data-media=video] video[src$="/media/${hash}"]`);
      await eventually("drawn as a player", async () => (await video.count()) === 1, 5_000);
      const meta = await video.evaluate((v) => new Promise((r) => { if (v.readyState >= 1) r(v.readyState); v.onloadedmetadata = () => r(v.readyState); v.onerror = () => r(`error ${v.error?.code}`); setTimeout(() => r(`state ${v.readyState}`), 4000); }));
      // A synthetic MP4 has no moov: the browser asks for it by range and says so; a real one plays. Either way it asked.
      assert(meta !== undefined, "the player read it");
      // The read-only view draws it the same.
      const ro = await openPage(`#/ticket/${id}`);
      try {
        await eventually("the ticket page draws the player", async () => (await ro.page.locator(`[data-surface=ticket-body] video[src$="/media/${hash}"]`).count()) === 1, 10_000);
        // COMPS-22: at its end the caption offers to play it again; pressing it goes back to the start and the offer goes.
        const player = ro.page.locator(`[data-surface=ticket-body] video[src$="/media/${hash}"]`);
        const again = ro.page.locator("[data-surface=ticket-body] [data-media=video] [data-action=play-again]");
        assert((await again.count()) === 1 && !(await again.isVisible()), "no offer before the end");
        await player.evaluate((v) => v.dispatchEvent(new Event("ended")));
        await eventually("the offer shows at the end", async () => again.isVisible(), 3_000);
        await again.click();
        await eventually("the offer goes once pressed", async () => !(await again.isVisible()), 3_000);
        assert((await player.evaluate((v) => v.currentTime)) === 0, "back at the start");
        if (shots) await ro.page.screenshot({ path: path.join(shots, "media-video-ticket.png") });
      } finally {
        await ro.context.close();
      }
    } finally {
      await context.close();
    }
  });

  await test("a_pdf_and_any_file_pasted_together_draw_as_a_viewer_and_a_card", async () => {
    const id = make("files", "files:\n");
    const { context, page, ed } = await open(id);
    try {
      await caretAt(page, "files:".length);
      await pasteAll(page, [{ b64: btoa("%PDF-1.4\n%fake pdf\n"), type: "application/pdf", name: "deck.pdf" }, { b64: btoa("PK\u0003\u0004 not much of a zip"), type: "application/zip", name: "notes.zip" }]);
      await eventually("both in the source", async () => /\[deck\.pdf\]\(media:[0-9a-f]{64}\)/.test(get(id).body) && /\[notes\.zip\]\(media:[0-9a-f]{64}\)/.test(get(id).body), 10_000);
      const pdf = /\[deck\.pdf\]\(media:([0-9a-f]{64})\)/.exec(get(id).body)[1];
      await away(page);
      await eventually("the PDF in the browser's viewer", async () => (await ed.locator(`.blk [data-media=pdf] iframe[src$="/media/${pdf}"]`).count()) === 1, 5_000);
      const card = ed.locator(".blk [data-media=file]");
      await eventually("the file as a card", async () => (await card.count()) === 1, 5_000);
      assert((await card.locator(".mx-name").textContent()) === "notes.zip" && /bytes/.test(await card.locator(".mx-size").textContent()), "its name and size");
      const dl = card.locator("a[download]");
      assert((await dl.getAttribute("download")) === "notes.zip", "a download");
      const r = await page.request.get(await dl.getAttribute("href"));
      assert(r.status() === 200 && r.headers()["content-type"] === "application/octet-stream" && /^attachment;/.test(r.headers()["content-disposition"]), `served as an attachment: ${JSON.stringify(r.headers())}`);
      if (shots) await page.screenshot({ path: path.join(shots, "media-pdf-file.png"), fullPage: true });
    } finally {
      await context.close();
    }
  });

  await test("a_url_pasted_alone_becomes_a_card_and_stays_its_url", async () => {
    const id = make("a link card", "before\n\n\n\nafter\n");
    const { context, page, ed } = await open(id);
    try {
      const url = `${fake.origin}/page`;
      await caretAt(page, "before\n\n".length);
      await pasteAll(page, [], { "text/plain": url });
      await eventually("the URL alone on its line", async () => get(id).body === `before\n\n${url}\n\nafter\n`, 5_000);
      await away(page);
      const card = ed.locator(".blk a.mx-card[data-media=card]");
      await eventually("drawn as its card", async () => (await card.count()) === 1, 10_000);
      assert((await card.locator(".mx-title").textContent()) === "The local page" && (await card.locator(".mx-site").textContent()) === "Fake", "its title and site");
      assert((await card.getAttribute("href")) === url, "it links where it was");
      const img = card.locator("img");
      assert(/\/media\/[0-9a-f]{64}$/.test(await img.getAttribute("src")), "its image kept as media, never hot-linked");
      assert((await img.evaluate((i) => new Promise((r) => (i.complete ? r(i.naturalWidth) : (i.onload = () => r(i.naturalWidth)))))) === 2, "with its pixels");
      if (shots) await page.screenshot({ path: path.join(shots, "media-link-card.png") });
    } finally {
      await context.close();
    }
  });

  await test("pasted_html_becomes_clean_markdown_its_images_kept", async () => {
    const id = make("pasted html", "\n");
    const { context, page } = await open(id);
    try {
      await caretAt(page, 0);
      const dot = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
      const html = `<meta charset="utf-8"><b style="font-weight:normal" id="docs-internal-guid-1"><h2>Release notes</h2>
        <p>Some <strong>bold</strong>, <em>italic</em> and <span style="font-weight:700">styled</span> words, a <a href="https://example.com/x">link</a> and <code>code</code>.</p>
        <ul><li>one</li><li>two</li></ul>
        <table><thead><tr><th>a</th><th>b</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table>
        <pre><code>let x = 1;</code></pre>
        <p><img src="${fake.origin}/picture.png" alt="the picture"></p><p><img src="data:image/png;base64,${dot}" alt="dot"></p>
        <script>alert(1)</script></b>`;
      await pasteAll(page, [], { "text/html": html, "text/plain": "Release notes Some bold…" });
      await eventually("the markdown, its images kept", async () => /!\[dot\]\(media:[0-9a-f]{64}\)/.test(get(id).body ?? ""), 10_000);
      const body = get(id).body;
      for (const want of ["## Release notes", "**bold**", "_italic_", "**styled**", "[link](https://example.com/x)", "`code`", "- one\n- two", "| a | b |", "| 1 | 2 |", "```\nlet x = 1;\n```"]) {
        assert(body.includes(want), `says ${JSON.stringify(want)}:\n${body}`);
      }
      assert(/!\[the picture\]\(media:[0-9a-f]{64}\)/.test(body), `the remote image kept, not linked:\n${body}`);
      assert(!body.includes(fake.origin) && !body.includes("alert") && !body.includes("data:"), `nothing hot-linked, no script:\n${body}`);
      // Plain text stays plain.
      await caretAt(page, 0);
      await pasteAll(page, [], { "text/html": "<span>just words</span>", "text/plain": "just *words*" });
      await eventually("plain", async () => get(id).body.startsWith("just *words*"), 5_000);
    } finally {
      await context.close();
    }
  });

  /** A one-page PDF that says "deck.pdf" (offsets computed). */
  const MINI_PDF = (() => {
    const objs = ["<< /Type /Catalog /Pages 2 0 R >>", "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
      "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 120] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
      "<< /Length 44 >>\nstream\nBT /F1 24 Tf 40 60 Td (deck.pdf) Tj ET\nendstream", "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>"];
    let out = "%PDF-1.4\n";
    const at = [];
    objs.forEach((o, i) => { at.push(out.length); out += `${i + 1} 0 obj\n${o}\nendobj\n`; });
    const xref = out.length;
    out += `xref\n0 ${objs.length + 1}\n0000000000 65535 f \n${at.map((n) => `${String(n).padStart(10, "0")} 00000 n \n`).join("")}trailer\n<< /Size ${objs.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
    return out;
  })();

  await test("every_kind_draws_in_the_editor_and_on_the_ticket_page", async () => {
    const id = make("every kind", "# Every kind\n\nWhat a doc absorbs, drawn.\n");
    const { context, page, ed } = await open(id);
    try {
      await caretAt(page, (await src(page)).length);
      await pasteAll(page, [
        { b64: PNG_2X1, type: "image/png", name: "a picture.png" },
        { bytes: 64 * 1024, head: MP4_HEAD, type: "video/mp4", name: "clip.mp4" },
        { bytes: 32 * 1024, head: btoa("ID3\u0004\u0000\u0000\u0000\u0000"), type: "audio/mpeg", name: "voice.mp3" },
        { b64: btoa(MINI_PDF), type: "application/pdf", name: "deck.pdf" },
        { b64: btoa("PK\u0003\u0004 a zip"), type: "application/zip", name: "notes.zip" },
      ]);
      await eventually("all five in the source", async () => (get(id).body.match(/\(media:[0-9a-f]{64}\)/g) ?? []).length === 5, 15_000);
      await caretAt(page, (await src(page)).length);
      await page.keyboard.press("Enter");
      await page.keyboard.press("Enter");
      await pasteAll(page, [], { "text/plain": `${fake.origin}/page` });
      await eventually("and the link", async () => get(id).body.includes(`${fake.origin}/page`), 5_000);
      await away(page);
      for (const kind of ["video", "audio", "pdf", "file", "card"]) {
        await eventually(`the ${kind} drawn in the editor`, async () => (await ed.locator(`.blk [data-media=${kind}]`).count()) === 1, 10_000)
          .catch((e) => { throw new Error(`${e.message}: ${JSON.stringify(get(id).body)}`); });
      }
      assert((await ed.locator(".blk img[src*='/media/']").count()) >= 2, "the picture and the card's image");
      await shoot(page, "media-editor", true);
      const ro = await openPage(`#/ticket/${id}`);
      try {
        const body = ro.page.locator("[data-surface=ticket-body]");
        for (const kind of ["video", "audio", "pdf", "file", "card"]) {
          await eventually(`the ${kind} drawn on the ticket page`, async () => (await body.locator(`[data-media=${kind}]`).count()) === 1, 10_000);
        }
        await shoot(ro.page, "media-ticket", true);
      } finally {
        await ro.context.close();
      }
    } finally {
      await context.close();
    }
  });

  await test("an_image_dragged_from_another_tab_is_fetched_and_kept", async () => {
    const id = make("a dragged image", "x\n");
    const { context, page } = await open(id);
    try {
      await caretAt(page, 1);
      await page.evaluate(({ src }) => {
        const dt = new DataTransfer();
        dt.setData("text/html", `<img src="${src}" alt="">`);
        dt.setData("text/uri-list", src);
        const c = document.querySelector("[data-surface=editor-body] .cm-content").getBoundingClientRect();
        document.querySelector("[data-surface=editor-body] .cm-content").dispatchEvent(new DragEvent("drop", { dataTransfer: dt, bubbles: true, cancelable: true, clientX: c.left + 5, clientY: c.top + 5 }));
      }, { src: `${fake.origin}/picture.png` });
      await eventually("kept and named", async () => /!\[picture\]\(media:[0-9a-f]{64}\)/.test(get(id).body), 10_000).catch(async (e) => {
        throw new Error(`${e.message}; body ${JSON.stringify(get(id).body)}; sent ${(await sentCalls(page)).filter((c) => c.method.startsWith("media.")).map((c) => JSON.stringify(c)).join(" ")}; upload ${await page.locator("[data-upload]").allTextContents()}`);
      });
      assert(!get(id).body.includes(fake.origin), "never hot-linked");
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
  /** The first block's two lines selected, the request asked. */
  const request = async (page, ed) => {
    await page.evaluate((s) => { const v = document.querySelector(s).editorView; v.focus(); v.dispatch({ selection: { anchor: 0, head: v.state.doc.line(2).to } }); }, BODY);
    await eventually("the button wakes", async () => !(await ed.locator("[data-action=ask-open]").isDisabled()), 2_000);
    await page.keyboard.press("Control+Enter");
    await ed.locator("[data-ask][data-state=open] [data-field=instruction]").fill("tighten these");
    await ed.locator("[data-action=ask]").click();
    await eventually("asked", async () => (await ed.locator("[data-ask=ask-1][data-state=asked]").count()) === 1, 5_000);
  };
  const WRIT = "first line\nsecond line\n\nthird line\n\nfourth line\n";

  await test("a_selection_and_a_line_become_a_question_queued_now", async () => {
    const id = make("asked", WRIT);
    const { context, page, ed } = await open(id);
    try {
      await request(page, ed);
      const q = question(id);
      assert(q?.state === "open" && q.question === "tighten these — lines 1–2: «first line second line»", `the question: ${JSON.stringify(q)}`);
      const entry = sy("tickets", "queued", "--id", id).flatMap((e) => (e.type === "rows" ? e.items : [e])).find((e) => e.id === id)?.entry;
      assert(entry?.urgency === "now" && JSON.stringify(entry).includes("ask-1"), `queued now, naming it: ${JSON.stringify(entry)}`);
      await sleep(300);
      assert((await ed.locator(".eb-hand").count()) === 0, "nothing marked for the ask's own write");
    } finally {
      await context.close();
    }
  });

  await test("an_agents_amend_is_marked_until_a_click_accepts_it", async () => {
    const id = make("written in", WRIT);
    const { context, page, ed } = await open(id);
    try {
      await request(page, ed);
      const body = get(id).body.replace("first line\nsecond line", "one tight line");
      agent("tickets", "amend", "--id", id, "--body", bodyFile("agent", body));
      await eventually("the agent's block is marked with its name", async () => (await ed.locator(".blk.eb-hand[data-hand=scribe]").count()) === 1, 8_000);
      assert((await ed.locator(".eb-hand").innerText()).includes("one tight line"), "the changed block");
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

  await test("an_edit_takes_the_mark_off_its_block_and_the_request_stays", async () => {
    const id = make("edited over", WRIT);
    const { context, page, ed } = await open(id);
    try {
      await request(page, ed);
      const body = get(id).body.replace("third line", "third, rewritten").replace("fourth line", "fourth, rewritten");
      agent("tickets", "amend", "--id", id, "--body", bodyFile("agent2", body));
      await eventually("two blocks marked", async () => (await ed.locator(".eb-hand").count()) === 2, 8_000);
      const at = (await src(page)).indexOf("third, rewritten") + "third".length;
      await caretAt(page, at);
      await page.keyboard.type("!");
      await page.keyboard.press("Escape");
      await eventually("one mark left", async () => (await ed.locator(".eb-hand").count()) === 1, 3_000);
      assert(question(id)?.state === "open", "the request stays open");
    } finally {
      await context.close();
    }
  });

  await test("a_refused_request_is_said_on_its_line", async () => {
    const id = make("no ask", "text\n\n\`\`\`\nopen fence\n");
    const { context, page, ed } = await open(id);
    try {
      await page.evaluate((s) => { const v = document.querySelector(s).editorView; v.focus(); v.dispatch({ selection: { anchor: 0, head: 4 } }); }, BODY);
      await page.keyboard.press("Control+Enter");
      await ed.locator("[data-field=instruction]").fill("anything");
      await ed.locator("[data-action=ask]").click();
      await eventually("the service's words", async () => (await ed.locator("[data-ask][data-state=open] [data-refused]").count()) === 1, 5_000);
      assert(question(id) == null, "nothing asked");
    } finally {
      await context.close();
    }
  });

  // V2-20 under the surface: a decision drawn as a card above is answered in the questions below it.
  await test("a_decision_drawn_above_is_answered_below", async () => {
    const id = make("answered", HOUSE);
    const { context, page } = await open(id);
    try {
      const opt = page.locator("[data-surface=editor-screen] [data-surface=ticket-questions] [data-input=cone] [data-option='6']");
      await opt.click();
      await eventually("answered", async () => (await page.locator("[data-surface=ticket-questions] [data-input=cone]").getAttribute("data-state")) === "answered", 5_000);
    } finally {
      await context.close();
    }
  });

  // ── EDITOR-9: a click lands where it is clicked, the page stays put ─────
  const LONG = Array.from({ length: 60 }, (_, k) => `Paragraph ${k} holds the word${k} among others, written plainly so the line is long enough to wrap once or twice in the column.`).join("\n\n") + "\n";
  /** The screen box of the `at`-th character of `word` in the drawn block holding it. */
  const charBox = (page, word, at) => page.evaluate(({ word, at }) => {
    for (const blk of document.querySelectorAll("[data-surface=editor-body] .blk")) {
      const w = document.createTreeWalker(blk, NodeFilter.SHOW_TEXT);
      for (let n = w.nextNode(); n; n = w.nextNode()) {
        const i = n.data.indexOf(word);
        if (i < 0) continue;
        const r = document.createRange();
        r.setStart(n, i + at); r.setEnd(n, i + at + 1);
        const b = r.getBoundingClientRect();
        return { x: b.left + b.width * 0.25, y: b.top + b.height / 2, top: b.top };
      }
    }
    return null;
  }, { word, at });
  /** The page scrolled so `word`'s block sits 200 px below the scroller's top (CodeMirror draws only what is near the view). */
  const reveal = async (page, word) => {
    await page.evaluate(({ s, word }) => {
      const v = document.querySelector(s).editorView;
      const sc = document.querySelector("[data-surface=editor-screen] .page");
      const b = v.lineBlockAt(v.state.doc.toString().indexOf(word));
      sc.scrollTop += v.documentTop + b.top - sc.getBoundingClientRect().top - 200;
    }, { s: BODY, word });
    await eventually(`${word} drawn`, async () => (await charBox(page, word, 0)) != null, 3_000);
    await sleep(300); // CodeMirror measures what it drew
  };
  const caretTop = (page) => page.evaluate((s) => { const v = document.querySelector(s).editorView; return v.coordsAtPos(v.state.selection.main.head, 1)?.top ?? null; }, BODY);

  await test("a_click_far_down_lands_on_its_character_and_the_page_stays", async () => {
    const id = make("a long body", LONG);
    const { context, page, ed } = await open(id);
    try {
      await eventually("drawn", async () => (await ed.locator(".blk").count()) > 5, 5_000);
      await reveal(page, "word40");
      for (const [word, n] of [["word40", 40], ["word44", 44], ["word38", 38]]) {
        if (!(await charBox(page, word, 2))) await reveal(page, word);
        const box = await charBox(page, word, 2);
        assert(box, `the drawn ${word}`);
        // The page as the reader sees it: the drawn block above the clicked one, on screen.
        const above = `word${n - 1}`;
        const aboveBefore = await charBox(page, above, 0);
        await page.mouse.click(box.x, box.y);
        const want = LONG.indexOf(word) + 2;
        await eventually(`the caret at ${word}'s third character`, async () => (await caret(page)) === want, 2_000);
        await sleep(150); // a frame or two for the measure
        const top = await caretTop(page);
        assert(Math.abs(top - box.top) <= 2, `${word}: the clicked character stays where it was on screen: ${box.top} → ${top}`);
        const aboveAfter = await charBox(page, above, 0);
        if (aboveBefore && aboveAfter) assert(Math.abs(aboveAfter.top - aboveBefore.top) <= 2, `${word}: the text above stays where it was on screen (the page did not move): ${aboveBefore.top} → ${aboveAfter.top}`);
      }
      assert((await amends(page)).length === 0, "clicks write nothing");
    } finally {
      await context.close();
    }
  });

  await test("arrows_cross_blocks_without_a_jump", async () => {
    const id = make("arrows", LONG);
    const { context, page, ed } = await open(id);
    try {
      await eventually("drawn", async () => (await ed.locator(".blk").count()) > 5, 5_000);
      await reveal(page, "word30");
      const box = await charBox(page, "word30", 0);
      await page.mouse.click(box.x, box.y);
      for (let k = 0; k < 6; k++) {
        const was = await caretTop(page);
        await page.keyboard.press("ArrowDown");
        await sleep(120);
        const now = await caretTop(page);
        assert(now - was >= -2 && now - was <= 60, `ArrowDown ${k}: the caret moves a line or a block's gap, never a jump: ${was} → ${now}`);
      }
      for (let k = 0; k < 6; k++) {
        const was = await caretTop(page);
        await page.keyboard.press("ArrowUp");
        await sleep(120);
        const now = await caretTop(page);
        assert(was - now >= -2 && was - now <= 60, `ArrowUp ${k}: ${was} → ${now}`);
      }
    } finally {
      await context.close();
    }
  });

  await test("no_highlight_where_the_caret_is", async () => {
    const id = make("no highlight", "# Head\n\nA plain paragraph to sit in.\n\n" + HOUSE);
    const { context, page, ed } = await open(id);
    try {
      await eventually("drawn", async () => (await ed.locator(".blk").count()) > 5, 5_000);
      const at = "# Head\n\nA plain".length;
      await caretAt(page, at);
      await eventually("the paragraph's line as markdown", async () => (await ed.locator(".cm-line").filter({ hasText: "A plain paragraph" }).count()) === 1, 3_000);
      const looks = await page.evaluate(() => [...document.querySelectorAll("[data-surface=editor-body] .cm-line")].map((l) => {
        const c = getComputedStyle(l);
        return { bg: c.backgroundColor, border: [c.borderTopWidth, c.borderRightWidth, c.borderBottomWidth, c.borderLeftWidth].join(" "), outline: c.outlineStyle, shadow: c.boxShadow, cls: l.className };
      }));
      for (const l of looks) {
        assert(l.bg === "rgba(0, 0, 0, 0)" && l.border === "0px 0px 0px 0px" && l.outline === "none" && l.shadow === "none", `no tint, border, outline or shadow on the caret's line: ${JSON.stringify(l)}`);
        assert(!/active|highlight|card-src|selected/.test(l.cls), `no highlight class: ${l.cls}`);
      }
      const drawnBefore = await drawnHtml(page, "[data-surface=editor-body]");
      assert(drawnBefore.length > 5 && (await ed.locator(".blk:is(.eb-active, .active, [data-active])").count()) === 0, "the drawn blocks carry no mark of the caret");
      // In a house mark: the caret's line is a plain line, as every other.
      const q = (await src(page)).indexOf("Which cone") + 3;
      await caretAt(page, q);
      await eventually("the question's line", async () => (await ed.locator(".cm-line").filter({ hasText: "<m-question" }).count()) === 1, 3_000);
      const [line, card] = await page.evaluate(() => {
        const l = [...document.querySelectorAll("[data-surface=editor-body] .cm-line")].find((x) => x.textContent.includes("<m-question"));
        const c = document.querySelector("[data-surface=editor-body] .blk m-summary");
        const s = (e) => { const k = getComputedStyle(e); return { bg: k.backgroundColor, outline: k.outlineStyle, shadow: k.boxShadow }; };
        return [s(l), s(c)];
      });
      assert(line.bg === "rgba(0, 0, 0, 0)" && card.bg === "rgba(0, 0, 0, 0)" && line.outline === "none" && line.shadow === "none", `no frame, no tint: ${JSON.stringify(line)} / ${JSON.stringify(card)}`);
    } finally {
      await context.close();
    }
  });

  await test("an_unknown_mark_draws_its_words_with_a_caption", async () => {
    const BODY_UNKNOWN = 'Before.\n\n<m-foo label="X">**bold**</m-foo>\n\n<m-bar data-field="notes">\n\nInside *the* bar.\n\n</m-bar>\n\n<volt-poll name="Poll"></volt-poll>\n\n```zz-unknown\ncode in no known language\n```\n';
    const id = make("unknown marks", BODY_UNKNOWN);
    const { context, page, ed } = await open(id);
    try {
      for (const [scope, where] of [["[data-surface=editor-body]", "the editor"], ["[data-surface=ticket-body]", "the view"]]) {
        if (where === "the view") await page.evaluate((id) => (location.hash = `#/ticket/${id}`), id);
        const foo = page.locator(`${scope} .blk m-foo`);
        await eventually(`${where}: m-foo drawn`, async () => (await foo.count()) === 1, 5_000);
        assert((await foo.locator("strong").innerText()) === "bold", `${where}: its words as markdown, bold`);
        assert((await foo.getAttribute("data-caption")) === "X", `${where}: captioned X`);
        const before = await foo.evaluate((e) => getComputedStyle(e, "::before").content);
        assert(before === '"X"', `${where}: the caption drawn: ${before}`);
        const bar = page.locator(`${scope} .blk m-bar`).first();
        assert((await bar.getAttribute("data-caption")) === "notes" && (await page.locator(`${scope} .blk m-bar em`).innerText()) === "the", `${where}: a box of markdown, captioned by its field`);
        const poll = page.locator(`${scope} .blk m-el[data-tag=volt-poll]`);
        assert((await poll.count()) === 1 && (await poll.getAttribute("data-caption")) === "Poll", `${where}: a custom tag kept inert, captioned`);
        assert((await page.locator(`${scope} .blk pre code.language-zz-unknown`).innerText()).includes("code in no known language"), `${where}: an unknown language is a code block`);
        const look = await foo.evaluate((e) => { const c = getComputedStyle(e); return c.color + " " + c.borderTopColor; });
        assert(!/rgb\(2[0-9]{2}, [0-9]{1,2}, [0-9]{1,2}\)/.test(look), `${where}: no warning colour: ${look}`);
      }
    } finally {
      await context.close();
    }
  });

  await test("mermaid_is_drawn_by_pele_and_a_refusal_shows_its_source", async () => {
    const id = make("diagrams", "```mermaid\nflowchart TD\n  A[one] --> B[two]\n```\n\n```mermaid\nflowchart TD\n  A[one --> \n```\n");
    const { context, page, ed, } = await open(id);
    try {
      const drawn = ed.locator(".blk [data-surface=mermaid-diagram][data-state=drawn] svg");
      await eventually("Pele drew the first", async () => (await drawn.count()) === 1, 10_000);
      assert(/one/.test(await drawn.textContent()) && /two/.test(await drawn.textContent()), "its labels in the SVG");
      const refused = ed.locator(".blk [data-surface=mermaid-diagram][data-state=refused]");
      await eventually("the second refused, said", async () => (await refused.count()) === 1, 10_000);
      assert(/mermaid/.test(await refused.locator("[data-refused]").innerText()), "labelled");
      assert(await ed.locator(".blk").nth(1).locator("pre code.language-mermaid").isVisible(), "its source shown under it");
      assert(!(await ed.locator(".blk").nth(0).locator("pre code.language-mermaid").isVisible()), "the drawn one's source hidden");
      const loaded = await modulesLoaded(page);
      assert(loaded.includes("mermaid-diagram"), `Pele its own module: ${JSON.stringify(loaded)}`);
    } finally {
      await context.close();
    }
  });

  // ── EDITOR-9: one smooth surface; code as a card ─────────────────────────
  const CODE = 'Some code:\n\n```rust\nfn main() {\n    let answer: u32 = 42; // the answer\n    println!("{}", answer);\n}\n```\n\n```nonsense\nplain <words> & "quotes"\n```\n\n<m-code lang="python" path="tools/run.py" why="how it runs">\n\n```\ndef run(x):\n    return x + 1\n```\n\n</m-code>\n';

  await test("no_house_mark_draws_a_box", async () => {
    const id = make("one smooth surface", HOUSE + "\n" + CODE);
    const { context, page } = await open(id);
    try {
      for (const [scope, where] of [["[data-surface=editor-body]", "the editor"], ["[data-surface=ticket-body]", "the view"]]) {
        if (where === "the view") await page.evaluate((id) => (location.hash = `#/ticket/${id}`), id);
        await eventually(`${where}: drawn`, async () => (await page.locator(`${scope} .blk m-summary`).count()) > 0, 5_000);
        const boxed = await page.evaluate((scope) => [...document.querySelectorAll(`${scope} .blk, ${scope} .blk *`)].filter((e) => {
          if (e.closest(".code-card, pre, code, table")) return false; // code keeps its tint; a table its rules
          const c = getComputedStyle(e);
          const border = ["Top", "Right", "Bottom", "Left"].some((k) => c[`border${k}Style`] !== "none" && parseFloat(c[`border${k}Width`]) > 0);
          const bg = c.backgroundColor !== "rgba(0, 0, 0, 0)" && c.backgroundColor !== "transparent";
          return border || bg || c.boxShadow !== "none" || (e.localName.startsWith("m-") && parseFloat(c.borderRadius) > 0 && bg);
        }).map((e) => `${e.localName}.${e.className}`), scope);
        const marks = boxed.filter((x) => !x.startsWith("blockquote") && !x.startsWith("hr") && !x.startsWith("a."));
        assert(marks.length === 0, `${where}: no house mark (or block) draws a border, a background or a shadow: ${JSON.stringify(marks.slice(0, 8))}`);
      }
    } finally {
      await context.close();
    }
  });

  await test("code_draws_as_one_card_highlighted_and_copyable", async () => {
    const id = make("code", CODE);
    const { context, page, ed } = await open(id);
    try {
      await page.evaluate(() => { window.__copied = []; navigator.clipboard.writeText = async (t) => { window.__copied.push(t); }; });
      const cards = ed.locator(".blk [data-surface=code-card]");
      await eventually("three code cards", async () => (await cards.count()) === 3, 5_000);
      const rust = cards.nth(0);
      assert((await rust.getAttribute("data-lang")) === "rust" && (await rust.locator("[data-field=lang]").innerText()) === "rust", "labelled rust");
      await eventually("rust painted, as its own module", async () => (await rust.locator("[data-surface=code-paint][data-state=painted] .tok-keyword").count()) > 0, 10_000);
      const kw = await rust.locator(".tok-keyword").allInnerTexts();
      assert(kw.includes("fn") && kw.includes("let"), `keywords painted: ${JSON.stringify(kw)}`);
      assert((await rust.locator(".tok-comment").innerText()).includes("the answer"), "the comment painted");
      const colour = await rust.locator(".tok-keyword").first().evaluate((e) => getComputedStyle(e).color);
      assert(colour !== (await rust.evaluate((e) => getComputedStyle(e).color)), `a keyword's colour is its token's: ${colour}`);
      const loaded = await modulesLoaded(page);
      assert(loaded.includes("code-rust") && loaded.includes("code-python") && !loaded.includes("code-go"), `each language its own module, fetched when used: ${JSON.stringify(loaded)}`);
      const nonsense = cards.nth(1);
      assert((await nonsense.getAttribute("data-lang")) === "plain" && (await nonsense.locator("[data-field=lang]").innerText()) === "nonsense", "an unknown language: plain, labelled as written");
      assert((await nonsense.locator("pre.plain").innerText()) === 'plain <words> & "quotes"' && (await nonsense.locator("[data-surface=code-paint]").count()) === 0, "plain monospace in the same card");
      const py = cards.nth(2);
      assert((await py.getAttribute("data-lang")) === "python" && (await py.locator("[data-field=path]").innerText()) === "tools/run.py" && (await py.locator("[data-field=why]").innerText()) === "how it runs", "<m-code>'s language, path and why");
      const wraps = await rust.locator(".scroll").evaluate((e) => getComputedStyle(e.querySelector("pre")).whiteSpace);
      assert(wraps === "pre", `code never wraps: ${wraps}`);
      await rust.locator("[data-action=copy]").click();
      await eventually("copied", async () => (await page.evaluate(() => window.__copied.length)) === 1, 2_000);
      const copied = await page.evaluate(() => window.__copied[0]);
      assert(copied === 'fn main() {\n    let answer: u32 = 42; // the answer\n    println!("{}", answer);\n}', `the exact code: ${JSON.stringify(copied)}`);
      // The caret in the fence: its source, painted by the same parser.
      const at = (await src(page)).indexOf("let answer");
      await caretAt(page, at);
      await eventually("the fence's source, highlighted", async () => (await ed.locator(".cm-line.eb-fence .tok-keyword").count()) > 0, 5_000);
      await page.keyboard.press("Escape");
      // Drawn again (painting runs in idle time), then compared.
      await eventually("the editor's cards painted again", async () => (await ed.locator("[data-surface=code-card] [data-surface=code-paint][data-state=painted]").count()) === 2, 10_000);
      const mine = await drawnHtml(page, "[data-surface=editor-body]");
      await page.evaluate((id) => (location.hash = `#/ticket/${id}`), id);
      await eventually("the view's cards", async () => (await page.locator("[data-surface=ticket-body] [data-surface=code-card]").count()) === 3, 5_000);
      await eventually("the view painted", async () => (await page.locator("[data-surface=ticket-body] [data-surface=code-paint][data-state=painted]").count()) === 2, 10_000);
      const theirs = await drawnHtml(page, "[data-surface=ticket-body]");
      assert(JSON.stringify(mine) === JSON.stringify(theirs), "the view draws the code as the editor does");
      await shoot(page, "code-card");
    } finally {
      await context.close();
    }
  });
};
