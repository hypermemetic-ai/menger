// COMPS-16: the tweet builder as a component whose own content is the thread — `<tweet-writer>` holding a
// `<tweet-post>` a post, written in a body in block form (its open and close lines each alone), drawn by its
// tag as the composer (ui/src/tweet-writer.svelte), and saved by rewriting that one tag in the body
// (`tickets.get`, the tag replaced, `tickets.amend`), every other byte kept. tag-source finds a tag's span
// and its place among the body's tags of its name; ticket-view makes it one block and hands its source;
// tag-components hands the native component its source, children, place and ticket.
//
// Marks: `[data-surface=tweet-writer]` (`data-ticket`, `data-n`, `data-where=ticket|editor|doc|none|loose`),
// `[data-post=<n>]` (1-based; `[data-over]` past 280), `[data-field=text|count|video|over|state|note]`,
// `[data-action=add|remove|up|down|save]`, `[data-state=saved|dirty|saving|refused|changed|readonly]`.
import fs from "node:fs";
import { sentCalls } from "../../scripts/ui-throwaway.mjs";

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, server, path } = ctx;
  const EPIC = "WRTR";
  const HASH = "ab".repeat(32);
  const here = path.dirname(new URL(import.meta.url).pathname);
  const srcOf = (name) => fs.readFileSync(path.join(here, "..", "..", "ui", "src", `${name}.svelte`), "utf8");
  const get = (id) => sy("tickets", "get", "--id", id).find((e) => e.type === "ticket")?.ticket;
  let k = 0;
  const bodyFile = (body) => {
    const f = path.join(server.work, `writer-${++k}.md`);
    fs.writeFileSync(f, body);
    return f;
  };
  const make = (title, body = null, ...flags) => {
    const out = sy("tickets", "create", "--epic", EPIC, "--title", title, ...(body == null ? [] : ["--body", `@${bodyFile(body)}`]), ...flags);
    const c = out.find((e) => e.type === "created");
    if (!c) throw new Error(`create ${title}: ${JSON.stringify(out)}`);
    return c.ticket.id;
  };
  const root = make("the writers' shortcode", null, "--kind", "scope");
  const child = (title, body) => make(title, body, "--parent", root, "--kind", "build");

  /** The module scripts — tag-source's and tweet-writer's (its import of tag-source dropped) — run here, in node. */
  const pure = () => {
    const mod = (name) => {
      const m = /<script module>([\s\S]*?)<\/script>/.exec(srcOf(name));
      assert(m, `${name} has a <script module>`);
      return m[1].replace(/^\s*import .*$/gm, "").replace(/^\s*export\s+/gm, "").replace(/^\s*const uses = .*$/gm, "");
    };
    return new Function(`${mod("tag-source")}\n${mod("tweet-writer")}\nreturn { tagSpans, childrenOf, escText, decode, postsOf, serializeWriter, replaceWriter, videoOf, countOf };`)();
  };

  const BEFORE = "**A thread for the showreel.**\n\nWhat goes out, post by post — kept as it is.  \n\n";
  const WRITER = `<tweet-writer>\n<tweet-post video="media:${HASH}" name="1-chat.mp4">Every tool we build starts with a clip.</tweet-post>\n<tweet-post>The first paragraph.\n\nThe second paragraph, after a blank line.</tweet-post>\n</tweet-writer>`;
  const AFTER = "\n\nAnd words after it, *kept*.\n\n```\n<tweet-writer>\n<tweet-post>in a fence: not a writer</tweet-post>\n</tweet-writer>\n```\n";
  const BODY = `${BEFORE}${WRITER}${AFTER}`;

  await test("the_tag_round_trips", async () => {
    const { tagSpans, childrenOf, postsOf, serializeWriter, replaceWriter, videoOf, countOf } = pure();
    const spans = tagSpans(BODY);
    assert(spans.length === 1 && spans[0].name === "tweet-writer" && spans[0].n === 0, `one writer, the fenced one none: ${JSON.stringify(spans)}`);
    assert(spans[0].from === BEFORE.length && spans[0].text === WRITER, "its span is the tag, open line to close line");
    const posts = postsOf(childrenOf(WRITER));
    assert(posts.length === 2 && posts[1].text === "The first paragraph.\n\nThe second paragraph, after a blank line.", `two posts, the paragraphs kept: ${JSON.stringify(posts)}`);
    assert(JSON.stringify(videoOf(posts[0].attrs)) === JSON.stringify({ hash: HASH, name: "1-chat.mp4" }) && videoOf(posts[1].attrs) === null, "the video from the attributes");
    assert(serializeWriter(posts) === WRITER, `unchanged, the same text:\n${serializeWriter(posts)}`);
    // Words that would end the tag, open one, or read as an entity are written escaped and read back as typed.
    const nasty = "a < b && c &lt; d </tweet-post>\n</tweet-writer>\n<tweet-writer>";
    const out = serializeWriter([{ text: nasty, attrs: [["name", 'say "hi" & <go>']] }]);
    assert(!out.slice(0, -"</tweet-writer>".length).includes("</tweet-writer>") && out.split("\n").length === 5, `escaped: ${out}`);
    const back = postsOf(childrenOf(out));
    assert(back.length === 1 && back[0].text === nasty && back[0].attrs[0][1] === 'say "hi" & <go>', `read back as typed: ${JSON.stringify(back)}`);
    assert(tagSpans(`x\n\n${out}\n`).length === 1, "still one writer");
    // The body with the tag replaced: every other byte kept; a writer changed or gone refuses.
    const next = serializeWriter([...posts].reverse());
    const r = replaceWriter(BODY, 0, WRITER, next);
    assert(r.body === `${BEFORE}${next}${AFTER}`, "only the tag replaced");
    assert(replaceWriter(BODY, 0, WRITER + "x", next).refused === "changed", "changed since drawn: refused");
    assert(replaceWriter(BODY, 1, WRITER, next).refused === "gone", "no second writer: refused");
    // Two writers: each found by its place.
    const two = `a\n\n${WRITER}\n\nb\n\n${WRITER}\n`;
    const s2 = tagSpans(two);
    assert(s2.length === 2 && s2[1].n === 1, "two, told apart by their order");
    assert(replaceWriter(two, 1, WRITER, "<tweet-writer>\n</tweet-writer>").body === `a\n\n${WRITER}\n\nb\n\n<tweet-writer>\n</tweet-writer>\n`, "the second alone");
    assert(countOf("👍👍 é") === 4, "counted by code points");
  });

  await test("the_writer_draws_its_posts_and_saves_into_its_tag", async () => {
    const id = child("a writer in a body", BODY);
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    try {
      const tw = page.locator("[data-surface=tag-component][data-name=tweet-writer] [data-surface=tweet-writer]");
      await eventually("the writer drawn natively, its two posts", async () => (await tw.locator("[data-post]").count()) === 2, 20_000).catch(async (e) => {
        const what = await page.locator("[data-ticket-body]").first().innerHTML({ timeout: 1000 }).catch(() => "(no body)");
        throw new Error(`${e.message}\nthe body drawn: ${what.slice(0, 2000)}\nconsole: ${seen.console.slice(-10).join("\n")}`);
      });
      assert((await page.locator("[data-surface=tag-component][data-name=tweet-writer]").getAttribute("data-mode")) === "native", "native");
      assert((await tw.getAttribute("data-ticket")) === id && (await tw.getAttribute("data-n")) === "0" && (await tw.getAttribute("data-where")) === "ticket", "told its ticket and its place, on the drawn page (EDITOR-29)");
      const state = tw.locator("[data-field=state]");
      const post = (n) => tw.locator(`[data-post="${n}"]`);
      const words = (n) => post(n).locator("[data-field=text]").inputValue();
      assert((await state.getAttribute("data-state")) === "saved", "as read: saved");
      assert((await words(1)) === "Every tool we build starts with a clip.", "post 1's words");
      assert((await words(2)) === "The first paragraph.\n\nThe second paragraph, after a blank line.", "post 2's two paragraphs");
      const video = post(1).locator("video[data-field=video]");
      assert((await video.count()) === 1 && (await post(2).locator("video").count()) === 0, "the clip under post 1 alone");
      assert((await video.getAttribute("src")) === `${server.origin}/media/${HASH}`, "from this origin's /media");
      assert((await post(1).locator("[data-field=count]").getAttribute("data-count")) === "39", "post 1's count");
      // The words around it drawn as ever, the fenced one a code card, not a writer.
      assert(await page.locator("[data-ticket-body]").evaluate((b) => b.textContent.includes("And words after it")), "the words after it drawn");
      assert((await page.locator("[data-surface=tweet-writer]").count()) === 1, "one writer: the fenced tag is code");

      // Typed, a post added after post 2 and moved up to be the second (COMPS-23: a pause saves by itself).
      const first = post(1).locator("[data-field=text]");
      await first.click();
      await first.press("Control+End");
      await page.keyboard.type(" Typed: a < b & </tweet-post> c.");
      await eventually("unsaved", async () => (await state.getAttribute("data-state")) === "dirty");
      await post(2).locator("[data-action=add]").click();
      await eventually("a third post", async () => (await tw.locator("[data-post]").count()) === 3);
      await eventually("the focus in it", async () => page.evaluate(() => document.activeElement?.closest("[data-post]")?.getAttribute("data-post") === "3"));
      await page.keyboard.type("The third, added.");
      await post(3).locator("[data-action=up]").click();
      await eventually("moved up", async () => (await words(2)) === "The third, added.");
      // Every box shows all its words (the two paragraphs too, after the move): none is cut off.
      await eventually("no box cut off", async () => (await tw.locator("[data-field=text]").evaluateAll((ts) => ts.every((t) => t.scrollHeight <= t.clientHeight + 1))), 3_000);
      if (process.env.PAGES_SHOTS) await page.screenshot({ path: path.join(process.env.PAGES_SHOTS, "tweet-writer.png"), fullPage: true });
      // ctrl+s from inside the writer saves it (app-top holds the chord page-wide; the writer's is heard too).
      await post(2).locator("[data-field=text]").press("Control+s");
      const tag = `<tweet-writer>\n<tweet-post video="media:${HASH}" name="1-chat.mp4">Every tool we build starts with a clip. Typed: a &lt; b &amp; &lt;/tweet-post> c.</tweet-post>\n<tweet-post>The third, added.</tweet-post>\n<tweet-post>The first paragraph.\n\nThe second paragraph, after a blank line.</tweet-post>\n</tweet-writer>`;
      const want = `${BEFORE}${tag}${AFTER}`;
      await eventually("the body holds the tag with three posts in the new order", async () => get(id).body === want, 8_000).catch(() => {
        throw new Error(`the body:\n${JSON.stringify(get(id).body)}\nwanted:\n${JSON.stringify(want)}`);
      });
      const body = get(id).body;
      assert(body.startsWith(BEFORE) && body.endsWith(AFTER), "the body outside the tag, byte for byte");
      // Drawn again from the saved body: saved, the words as typed, and nothing saves again.
      await eventually("drawn again, saved, the words as typed", async () =>
        (await page.locator("[data-surface=tweet-writer]").count()) === 1
        && (await page.locator("[data-surface=tweet-writer] [data-field=state]").getAttribute("data-state")) === "saved"
        && (await page.locator("[data-surface=tweet-writer] [data-post='1'] [data-field=text]").inputValue()) === "Every tool we build starts with a clip. Typed: a < b & </tweet-post> c.", 15_000);
      const amends = (await sentCalls(page)).filter((c) => c.method === "tickets.amend");
      await sleep(1500);
      const later = (await sentCalls(page)).filter((c) => c.method === "tickets.amend");
      assert(amends.length === 1 && later.length === 1, `one amend, no save loop: ${amends.length} then ${later.length}`);
      // Past 280: over, and saved all the same (the button).
      const again = page.locator("[data-surface=tweet-writer]");
      const third = again.locator("[data-post='3'] [data-field=text]");
      await third.fill("y".repeat(281));
      await eventually("over", async () => (await again.locator("[data-post='3'] [data-field=count]").getAttribute("data-level")) === "over");
      assert((await again.locator("[data-post='3'] [data-field=over]").textContent()).trim() === "over by 1", "over by 1");
      await again.locator("[data-action=save]").click();
      await eventually("the over post saved", async () => get(id).body.includes(`<tweet-post>${"y".repeat(281)}</tweet-post>\n</tweet-writer>`), 8_000);
      assert(get(id).body.startsWith(BEFORE) && get(id).body.endsWith(AFTER), "still byte for byte outside");
      const errors = seen.console.filter((l) => /^(error|pageerror)/.test(l) && !/media|404|Failed to load resource/i.test(l));
      assert(errors.length === 0, `no error in the page: ${errors.join("\n")}`);
      assert(!seen.console.some((l) => /effect_update_depth_exceeded/.test(l)), "no effect loops");
    } finally {
      await context.close();
    }
  });

  await test("two_writers_in_one_body_save_apart", async () => {
    const A = "<tweet-writer>\n<tweet-post>first writer</tweet-post>\n</tweet-writer>";
    const B = "<tweet-writer>\n<tweet-post>second writer</tweet-post>\n</tweet-writer>";
    const body = `top\n\n${A}\n\nmiddle\n\n${B}\n\nend\n`;
    const id = child("two writers", body);
    const { context, page } = await openPage(`#/ticket/${id}`);
    try {
      const tw = page.locator("[data-surface=tweet-writer]");
      await eventually("two writers", async () => (await tw.count()) === 2 && (await tw.nth(1).locator("[data-post]").count()) === 1, 20_000);
      assert((await tw.nth(0).getAttribute("data-n")) === "0" && (await tw.nth(1).getAttribute("data-n")) === "1", "told apart by their order");
      const box = (w) => tw.nth(w).locator("[data-post='1'] [data-field=text]");
      await box(0).fill("first writer, edited");
      await box(1).fill("second writer, edited");
      // COMPS-23: each saves by itself, one at a time; neither writes over the other.
      const both = `top\n\n<tweet-writer>\n<tweet-post>first writer, edited</tweet-post>\n</tweet-writer>\n\nmiddle\n\n<tweet-writer>\n<tweet-post>second writer, edited</tweet-post>\n</tweet-writer>\n\nend\n`;
      await eventually("both saved, apart", async () => get(id).body === both, 8_000).catch(() => { throw new Error(`the body: ${JSON.stringify(get(id).body)}`); });
    } finally {
      await context.close();
    }
  });

  // A save never overwrites a writer it did not draw: the sheet's writer is drawn as its ticket's first, from
  // a source that ticket's body does not hold — its save refuses, saying so, and the body is untouched.
  await test("a_writer_changed_elsewhere_is_not_overwritten", async () => {
    const body = "<tweet-writer>\n<tweet-post>someone else's words</tweet-post>\n</tweet-writer>\n";
    const id = child("a writer changed elsewhere", body);
    const { context, page } = await openPage(`#/sheet/${id}`);
    try {
      const tw = page.locator("[data-sheet-component=tweet-writer] [data-sheet-state=plain] [data-surface=tweet-writer]").first();
      await eventually("the sheet's writer", async () => (await tw.count()) === 1 && (await tw.locator("[data-post]").count()) === 2, 30_000);
      await tw.locator("[data-post='1'] [data-field=text]").fill("mine");
      await tw.locator("[data-action=save]").click();
      const state = tw.locator("[data-field=state]");
      await eventually("refused: changed", async () => (await state.getAttribute("data-state")) === "changed", 8_000);
      assert(/changed since it was drawn/.test(await state.textContent()), `said: ${await state.textContent()}`);
      assert(get(id).body === body, "the body untouched");
      assert(!(await sentCalls(page)).some((c) => c.method === "tickets.amend"), "no amend sent");
    } finally {
      await context.close();
    }
  });

  // In the editor the block under the caret is the tag's source; away from it the writer is drawn and — EDITOR-24 —
  // used as on the ticket's page: it saves through the editor (`body.tag`), which writes the body.
  await test("in_the_editor_the_tag_is_source_at_the_caret_and_drawn_away_from_it", async () => {
    const id = child("a writer in the editor", BODY);
    const { context, page } = await openPage(`#/edit/${id}`);
    const SEL = "[data-surface=editor-body] [data-field=body]";
    try {
      await eventually("the editor", async () => (await page.locator(`${SEL} .cm-content`).count()) === 1, 15_000);
      const tw = page.locator(`${SEL} [data-surface=tweet-writer]`);
      await eventually("the writer drawn in its block", async () => (await tw.count()) === 1 && (await tw.locator("[data-post]").count()) === 2, 20_000);
      assert((await tw.getAttribute("data-where")) === "editor", "told it is in the editor");
      assert((await tw.locator("[data-field=note]").count()) === 0, "nothing says drawn only");
      assert(!(await tw.locator("[data-post='1'] [data-field=text]").evaluate((t) => t.readOnly)), "its boxes are written in");
      assert((await tw.locator("[data-action=save]").count()) === 1 && (await tw.locator("[data-action=add]").count()) === 2, "save, and a + a post");
      // A click in its box stays there: the block is not entered. Typed, a pause saves it through the editor,
      // the writer never drawn again and the focus kept.
      await tw.evaluate((e) => { e.dataset.same = "yes"; });
      const box = tw.locator("[data-post='2'] [data-field=text]");
      await box.click();
      await sleep(400);
      assert((await tw.count()) === 1, "still drawn after a click in it");
      await page.keyboard.press("Control+End");
      await page.keyboard.type(" Typed.");
      await eventually("saved through the editor", async () => get(id).body === BODY.replace("after a blank line.", "after a blank line. Typed."), 8_000)
        .catch(() => { throw new Error(`the body: ${JSON.stringify(get(id).body)}`); });
      await eventually("the writer says saved", async () => (await tw.locator("[data-field=state]").getAttribute("data-state")) === "saved");
      await sleep(600);
      assert((await tw.getAttribute("data-same")) === "yes", "the writer was never drawn again");
      assert(await box.evaluate((e) => document.activeElement === e), "the focus never left the box");
      // The caret in the tag: its source, editable text.
      const at = BEFORE.length + WRITER.indexOf("The first paragraph.") + "The first".length;
      await page.evaluate(({ s, at }) => { const v = document.querySelector(s).editorView; v.focus(); v.dispatch({ selection: { anchor: at } }); }, { s: SEL, at });
      await eventually("the tag's source shown", async () => (await tw.count()) === 0
        && (await page.locator(`${SEL} .cm-line`).evaluateAll((ls) => ls.some((l) => l.textContent.includes('<tweet-post video="media:')))), 5_000);
      await page.keyboard.type(" (edited)");
      await eventually("the editor saved it", async () => (await page.locator("[data-field=write-state]").getAttribute("data-state")) === "saved"
        && get(id).body === BODY.replace("after a blank line.", "after a blank line. Typed.").replace("The first paragraph.", "The first (edited) paragraph."), 10_000)
        .catch(() => { throw new Error(`the body: ${JSON.stringify(get(id).body)}`); });
      // The caret out of it: drawn again, the edit in it.
      await page.evaluate(({ s }) => { const v = document.querySelector(s).editorView; v.dispatch({ selection: { anchor: 0 } }); }, { s: SEL });
      await eventually("drawn again with the edit", async () => (await tw.count()) === 1
        && (await tw.locator("[data-post='2'] [data-field=text]").inputValue()).startsWith("The first (edited) paragraph."), 8_000);
    } finally {
      await context.close();
    }
  });

  await test("it_saves_by_itself_and_is_not_drawn_again", async () => {
    // COMPS-23: a pause writes the thread; the writer's own save redraws nothing under the caret.
    const id = child("saves by itself", BODY);
    const { context, page } = await openPage(`#/ticket/${id}`);
    try {
      const w = page.locator("[data-surface=tweet-writer]");
      await eventually("the writer", async () => (await w.locator("[data-post]").count()) === 2, 15_000);
      await w.evaluate((e) => { e.dataset.same = "yes"; });
      const box = w.locator("[data-post='2'] textarea");
      await box.click();
      await page.keyboard.press("Control+End");
      await page.keyboard.type(" One.");
      await eventually("the first pause saved", async () => get(id).body.includes("after a blank line. One."), 6_000);
      await sleep(600); // the body's redraw has come and gone
      await page.keyboard.type(" Two.");
      await eventually("the second pause saved", async () => get(id).body.includes("after a blank line. One. Two."), 6_000);
      await sleep(600);
      assert((await w.getAttribute("data-same")) === "yes", "the writer was never drawn again");
      assert(await box.evaluate((e) => document.activeElement === e), "the focus never left the box");
      assert((await w.locator("[data-field=state]").getAttribute("data-state")) === "saved", "it says saved");
      const body = get(id).body;
      assert(body.startsWith(BEFORE) && body.endsWith(AFTER), "the body outside the tag byte for byte");
      const amends = (await sentCalls(page)).filter((x) => x.method === "tickets.amend").length;
      assert(amends === 2, `one write a pause, none by a click: ${amends}`);
      // + and a move save too, with no click on save.
      await w.locator("[data-post='2'] [data-action=up]").click();
      await eventually("the move saved", async () => /<tweet-writer>\n<tweet-post>The first paragraph\./.test(get(id).body), 6_000);
    } finally {
      await context.close();
    }
  });

  // EDITOR-24: a document is written on the same editor, so the writer saves there too — into the doc's text.
  await test("in_a_document_it_saves_into_the_document", async () => {
    const docId = sy("docs", "create", "--title", "a writer in a document", "--body", `@${bodyFile(BODY)}`).find((e) => e.type === "created")?.doc.id;
    assert(docId, "the doc made");
    const { context, page } = await openPage(`#/doc/${docId}`);
    try {
      const tw = page.locator("[data-surface=tweet-writer]");
      await eventually("drawn in the document", async () => (await tw.count()) === 1 && (await tw.locator("[data-post]").count()) === 2, 20_000);
      assert((await tw.getAttribute("data-where")) === "doc", "told it is in a document");
      assert((await tw.locator("[data-field=note]").count()) === 0 && (await tw.locator("[data-action=save]").count()) === 1, "it saves here");
      await tw.evaluate((e) => { e.dataset.same = "yes"; });
      const box = tw.locator("[data-post='2'] [data-field=text]");
      await box.click();
      await page.keyboard.press("Control+End");
      await page.keyboard.type(" In the doc.");
      const text = () => page.evaluate(() => document.querySelector("[data-surface=editor-body] [data-field=body]").editorView.state.doc.toString());
      await eventually("the doc's text holds it", async () => (await text()) === BODY.replace("after a blank line.", "after a blank line. In the doc."), 8_000)
        .catch(async () => { throw new Error(`the doc: ${JSON.stringify(await text())}`); });
      await eventually("the writer says saved", async () => (await tw.locator("[data-field=state]").getAttribute("data-state")) === "saved");
      await eventually("sent to the docs service", async () => (await sentCalls(page)).some((c) => c.method === "docs.update"), 5_000);
      assert((await tw.getAttribute("data-same")) === "yes" && (await box.evaluate((e) => document.activeElement === e)), "never drawn again, the focus kept");
      assert(!(await sentCalls(page)).some((c) => c.method === "tickets.amend"), "no ticket written");
    } finally {
      await context.close();
    }
    // Opened again: the words are the document's.
    const again = await openPage(`#/doc/${docId}`);
    try {
      const tw = again.page.locator("[data-surface=tweet-writer]");
      await eventually("kept", async () => (await tw.count()) === 1 && (await tw.locator("[data-post='2'] [data-field=text]").inputValue()).endsWith("after a blank line. In the doc."), 20_000);
    } finally {
      await again.context.close();
    }
  });
};
