// COMPS-15: a ticket's body as a thread — tweet-thread at #/thread/:id (page-thread).
// The body's form is MENGER-52's: the head (everything before the first `## Post N`) kept as it is;
// each `## Post N` a post, its words, a `[name](media:<64 hex>)` line alone its video; any other
// heading ends the posts and is the tail, kept as it is. Saving writes the posts numbered from 1.
//
// Marks: `[data-surface=tweet-thread]`, `[data-post=<n>]` (1-based; `[data-over]` past 280),
// `[data-field=text|count|video|over|title|state]` (`[data-count]`, `[data-level=ok|near|over]` on the count),
// `[data-action=add|remove|up|down|save]`, `[data-state=loading|saved|dirty|saving|refused|missing]`.
import fs from "node:fs";
import { sentCalls } from "../../scripts/ui-throwaway.mjs";

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, server, path } = ctx;
  const EPIC = "THRD";
  const HASH = "ab".repeat(32);
  const src = path.join(path.dirname(new URL(import.meta.url).pathname), "..", "..", "ui", "src", "tweet-thread.svelte");
  const get = (id) => sy("tickets", "get", "--id", id).find((e) => e.type === "ticket")?.ticket;
  let n = 0;
  const bodyFile = (body) => {
    const f = path.join(server.work, `thread-${++n}.md`);
    fs.writeFileSync(f, body);
    return f;
  };
  const make = (title, body = null, ...flags) => {
    const out = sy("tickets", "create", "--epic", EPIC, "--title", title, ...(body == null ? [] : ["--body", `@${bodyFile(body)}`]), ...flags);
    const c = out.find((e) => e.type === "created");
    if (!c) throw new Error(`create ${title}: ${JSON.stringify(out)}`);
    return c.ticket.id;
  };
  const root = make("the threads' shortcode", null, "--kind", "scope");
  const child = (title, body) => make(title, body, "--parent", root, "--kind", "build");

  /** The component's module script — parseThread, serializeThread, countOf — run here, in node. */
  const pure = () => {
    const text = fs.readFileSync(src, "utf8");
    const m = /<script module>([\s\S]*?)<\/script>/.exec(text);
    assert(m, "tweet-thread has a <script module>");
    const code = m[1].replace(/^\s*export\s+/gm, "");
    return new Function(`${code}\nreturn { parseThread, serializeThread, countOf, LIMIT, NEAR };`)();
  };

  const HEAD = "**A thread for the showreel.**\n\nWhat goes out, post by post — kept as it is.\n\n";
  const TWO = `${HEAD}## Post 1\n\nThe first post's words,\nover two lines.\n\n\n\n## Post 2\n\nThe second, with its clip.\n\n[clip.mp4](media:${HASH})\n`;

  await test("the_form_round_trips", async () => {
    const { parseThread, serializeThread, countOf } = pure();
    const t = parseThread(TWO);
    assert(t.head === HEAD, `the head, byte for byte: ${JSON.stringify(t.head)}`);
    assert(t.posts.length === 2 && t.tail === "", `two posts, no tail: ${JSON.stringify(t)}`);
    assert(t.posts[0].text === "The first post's words,\nover two lines." && t.posts[0].video === null, `post 1: ${JSON.stringify(t.posts[0])}`);
    assert(t.posts[1].text === "The second, with its clip." && t.posts[1].video?.name === "clip.mp4" && t.posts[1].video?.hash === HASH, `post 2: ${JSON.stringify(t.posts[1])}`);
    const out = serializeThread(t);
    const want = `${HEAD}## Post 1\n\nThe first post's words,\nover two lines.\n\n## Post 2\n\nThe second, with its clip.\n\n[clip.mp4](media:${HASH})\n\n`;
    assert(out === want, `serialized:\n${JSON.stringify(out)}\nwanted:\n${JSON.stringify(want)}`);
    assert(serializeThread(parseThread(out)) === out, "a serialized thread is a fixed point");
    // A tail: any other heading ends the posts and is kept as it is; `post` in any case starts one.
    const tailed = `head\n## post 1\n\n  [a b.mov](media:${HASH})  \nwords\n\n\n\n\nmore\n### Notes\n\nkept  \n\n## Post 9 after the notes\n`;
    const p = parseThread(tailed);
    assert(p.head === "head\n" && p.posts.length === 1 && p.posts[0].text === "words\n\nmore" && p.posts[0].video?.name === "a b.mov", `tailed: ${JSON.stringify(p)}`);
    assert(p.tail === "### Notes\n\nkept  \n\n## Post 9 after the notes\n", `the tail: ${JSON.stringify(p.tail)}`);
    assert(serializeThread(p) === `head\n## Post 1\n\nwords\n\nmore\n\n[a b.mov](media:${HASH})\n\n### Notes\n\nkept  \n\n## Post 9 after the notes\n`, `tailed out: ${JSON.stringify(serializeThread(p))}`);
    // No post: the whole body is the head; posts written after it, set apart.
    const none = parseThread("just a head");
    assert(none.head === "just a head" && none.posts.length === 0 && none.tail === "" && none.bare === true, `none: ${JSON.stringify(none)}`);
    assert(serializeThread({ ...none, posts: [{ text: "one", video: null }] }) === "just a head\n\n## Post 1\n\none\n\n", "posts after a bare head");
    // A second video line in one post is words; a hash not of 64 lowercase hex is words.
    const two = parseThread(`## Post 1\n[a](media:${HASH})\n[b](media:${HASH})\n[c](media:${HASH.toUpperCase()})\n`);
    assert(two.posts[0].video.name === "a" && two.posts[0].text === `[b](media:${HASH})\n[c](media:${HASH.toUpperCase()})`, `one video a post: ${JSON.stringify(two)}`);
    assert(countOf("👍👍 é") === 4 && countOf("") === 0, "counted by code points");
  });

  await test("a_thread_is_edited_added_to_and_saved_to_its_body", async () => {
    const id = child("a thread to post", TWO);
    const { context, page, seen } = await openPage(`#/thread/${id}`);
    try {
      const tt = page.locator("[data-surface=tweet-thread]");
      const state = tt.locator("[data-field=state]");
      const post = (k) => tt.locator(`[data-post="${k}"]`);
      await eventually("both posts are drawn", async () => (await tt.locator("[data-post]").count()) === 2);
      assert((await tt.locator("[data-field=title]").textContent()).trim() === "a thread to post", "the ticket's title in the header");
      assert((await state.getAttribute("data-state")) === "saved", "as read: saved");
      assert((await post(1).locator("[data-field=text]").inputValue()) === "The first post's words,\nover two lines.", "post 1's words");
      assert((await post(1).locator("[data-field=count]").getAttribute("data-count")) === "39", "post 1's count");
      assert((await post(2).locator("[data-field=count]").textContent()).replace(/\s+/g, " ").trim() === "26 / 280", "post 2's count, against 280");
      const video = post(2).locator("video[data-field=video]");
      assert((await video.count()) === 1 && (await post(1).locator("video").count()) === 0, "the clip under post 2 alone");
      assert((await video.getAttribute("src")) === `${server.origin}/media/${HASH}`, `the clip from this origin's /media: ${await video.getAttribute("src")}`);
      assert((await video.getAttribute("controls")) !== null && (await video.getAttribute("preload")) === "metadata", "playable, its metadata only until played");
      assert(await post(1).locator("[data-action=up]").isDisabled(), "the first moves no higher");
      assert(!(await post(1).locator("[data-action=remove]").isDisabled()), "two posts: either may go");

      // Moved down and back: the thread as it was, saved.
      await post(1).locator("[data-action=down]").click();
      await eventually("post 1 moved down", async () => (await post(2).locator("[data-field=text]").inputValue()).startsWith("The first"));
      assert((await state.getAttribute("data-state")) === "dirty", "a move is unsaved");
      await post(2).locator("[data-action=up]").click();
      await eventually("and back: saved", async () => (await state.getAttribute("data-state")) === "saved");

      // Words typed into the first; a third added under the second, the focus in it.
      const first = post(1).locator("[data-field=text]");
      await first.click();
      await first.press("Control+End");
      await page.keyboard.type(" Typed here.");
      await eventually("unsaved changes", async () => (await state.getAttribute("data-state")) === "dirty");
      assert((await state.textContent()).trim() === "unsaved changes", "said");
      assert(get(id).body === TWO, "nothing is written while typing");
      await post(2).locator("[data-action=add]").click();
      await eventually("a third post", async () => (await tt.locator("[data-post]").count()) === 3);
      await eventually("the focus in the new post", async () => page.evaluate(() => document.activeElement?.closest("[data-post]")?.getAttribute("data-post") === "3"));
      await page.keyboard.type("The third, added.");
      assert((await post(3).locator("[data-field=count]").getAttribute("data-count")) === "17", "the third's count");
      await tt.locator("[data-action=save]").click();
      const want = `${HEAD}## Post 1\n\nThe first post's words,\nover two lines. Typed here.\n\n## Post 2\n\nThe second, with its clip.\n\n[clip.mp4](media:${HASH})\n\n## Post 3\n\nThe third, added.\n\n`;
      await eventually("the body holds the three posts", async () => get(id).body === want, 5_000).catch(() => {
        throw new Error(`the body:\n${JSON.stringify(get(id).body)}\nwanted:\n${JSON.stringify(want)}`);
      });
      assert(get(id).body.startsWith(HEAD), "the head untouched");
      await eventually("saved", async () => (await state.getAttribute("data-state")) === "saved");

      // Past 280: red, "over by 1", and still saved — ctrl+s, from the box.
      const third = post(3).locator("[data-field=text]");
      const long = "x".repeat(270) + " 👍👍👍👍👍👍👍👍👍👍"; // 281 code points
      await third.fill(long);
      const count = post(3).locator("[data-field=count]");
      await eventually("over", async () => (await count.getAttribute("data-level")) === "over");
      assert((await count.getAttribute("data-count")) === "281", "281 by code points");
      assert((await post(3).getAttribute("data-over")) !== null, "the post says over");
      if (process.env.PAGES_SHOTS) await page.screenshot({ path: path.join(process.env.PAGES_SHOTS, "tweet-thread.png"), fullPage: true });
      assert((await post(3).locator("[data-field=over]").textContent()).trim() === "over by 1", "over by 1");
      const colour = (loc) => loc.evaluate((e) => getComputedStyle(e).color);
      const warn = await page.evaluate(() => getComputedStyle(document.querySelector("[data-surface=tweet-thread]")).getPropertyValue("--warn").trim());
      assert(warn !== "", "the theme's --warn");
      assert((await colour(count)) !== (await colour(post(1).locator("[data-field=count]"))), "the over count is drawn apart from an ok one");
      await third.fill("x".repeat(260));
      await eventually("near: amber from 260", async () => (await count.getAttribute("data-level")) === "near");
      await third.fill(long);
      await third.press("Control+s");
      await eventually("the over post is saved", async () => get(id).body.includes(`## Post 3\n\n${long}\n\n`), 5_000);
      await eventually("saved", async () => (await state.getAttribute("data-state")) === "saved");

      // Removed: the third goes, the rest renumbered on save.
      await post(1).locator("[data-action=remove]").click();
      await eventually("two left", async () => (await tt.locator("[data-post]").count()) === 2);
      await tt.locator("[data-action=save]").click();
      await eventually("post 2 is now post 1", async () => get(id).body === `${HEAD}## Post 1\n\nThe second, with its clip.\n\n[clip.mp4](media:${HASH})\n\n## Post 2\n\n${long}\n\n`, 5_000);
      const errors = seen.console.filter((l) => /^(error|pageerror)/.test(l) && !/media|404|Failed to load resource/i.test(l));
      assert(errors.length === 0, `no error in the page: ${errors.join("\n")}`);
    } finally {
      await context.close();
    }
  });

  await test("an_unchanged_thread_saves_the_same_body", async () => {
    const tail = "\n## Notes\n\nkept as it is\n";
    const body = `${HEAD}## Post 1\n\nalone\n\n[clip.mp4](media:${HASH})\n\n${tail.slice(1)}`;
    const id = child("a thread with a tail", body);
    const { context, page } = await openPage(`#/thread/${id}`);
    try {
      const tt = page.locator("[data-surface=tweet-thread]");
      await eventually("one post", async () => (await tt.locator("[data-post]").count()) === 1);
      assert(await tt.locator("[data-post='1'] [data-action=remove]").isDisabled(), "the only post cannot go");
      await tt.locator("[data-action=save]").click();
      await eventually("saved", async () => (await tt.locator("[data-field=state]").getAttribute("data-state")) === "saved");
      assert(get(id).body === body, `the body as it was:\n${JSON.stringify(get(id).body)}`);
    } finally {
      await context.close();
    }
  });

  await test("a_body_with_no_post_shows_one_and_saves_after_its_head", async () => {
    const id = child("no thread yet", "Only a head here.\n");
    const { context, page } = await openPage(`#/thread/${id}`);
    try {
      const tt = page.locator("[data-surface=tweet-thread]");
      await eventually("one empty post", async () => (await tt.locator("[data-post]").count()) === 1);
      assert((await tt.locator("[data-post='1'] [data-field=text]").inputValue()) === "", "empty");
      await tt.locator("[data-post='1'] [data-field=text]").fill("first words");
      await tt.locator("[data-action=save]").click();
      await eventually("appended after the head", async () => get(id).body === "Only a head here.\n\n## Post 1\n\nfirst words\n\n", 5_000);
    } finally {
      await context.close();
    }
  });

  // The owner: "isn't the ticket itself a component" — the tag in a body, drawn natively in the ticket's page.
  await test("the_tag_in_its_own_ticket_draws_the_thread_natively_and_saves", async () => {
    const id = child("a thread in its own ticket", "placeholder");
    const head = `The thread for the reel, drawn below.\n\n<tweet-thread id="${id}"></tweet-thread>\n\n`;
    const body = `${head}## Post 1\n\nThe first, in the ticket.\n\n## Post 2\n\nThe second.\n\n[clip.mp4](media:${HASH})\n\n`;
    sy("tickets", "amend", "--id", id, "--body", `@${bodyFile(body)}`);
    assert(get(id).body === body, "the body as written");
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    try {
      const host = page.locator("[data-surface=tag-component][data-name=tweet-thread]");
      await eventually("the tag is drawn natively", async () => (await host.count()) === 1 && (await host.getAttribute("data-mode")) === "native", 20_000).catch(async (e) => {
        const what = await page.locator("[data-ticket-body]").first().innerHTML({ timeout: 1000 }).catch(() => "(no body)");
        throw new Error(`${e.message}\nthe body drawn: ${what.slice(0, 1500)}\nconsole: ${seen.console.slice(-10).join("\n")}`);
      });
      const tt = host.locator("[data-surface=tweet-thread]");
      await eventually("the thread inside the ticket, its two posts", async () => (await tt.locator("[data-post]").count()) === 2, 15_000);
      assert((await tt.getAttribute("data-ticket")) === id, "its ticket, from the tag's id");
      assert((await tt.locator("[data-field=state]").getAttribute("data-state")) === "saved", "as read: saved");
      const second = tt.locator("[data-post='2'] [data-field=text]");
      await second.click();
      await second.press("Control+End");
      await page.keyboard.type(" Edited in the ticket.");
      await eventually("unsaved", async () => (await tt.locator("[data-field=state]").getAttribute("data-state")) === "dirty");
      await tt.locator("[data-action=save]").click();
      const want = `${head}## Post 1\n\nThe first, in the ticket.\n\n## Post 2\n\nThe second. Edited in the ticket.\n\n[clip.mp4](media:${HASH})\n\n`;
      await eventually("the body holds the edit, the tag line kept", async () => get(id).body === want, 5_000).catch(() => {
        throw new Error(`the body:\n${JSON.stringify(get(id).body)}\nwanted:\n${JSON.stringify(want)}`);
      });
      // The page draws the body again; the thread comes back saved, with the edit, and nothing saves again.
      const again = page.locator("[data-surface=tag-component][data-name=tweet-thread] [data-surface=tweet-thread]");
      await eventually("drawn again, saved, with the edit", async () =>
        (await again.locator("[data-field=state]").getAttribute("data-state")) === "saved"
        && (await again.locator("[data-post='2'] [data-field=text]").inputValue()) === "The second. Edited in the ticket.", 15_000);
      const amends = (await sentCalls(page)).filter((c) => c.method === "tickets.amend");
      await sleep(1500);
      const later = (await sentCalls(page)).filter((c) => c.method === "tickets.amend");
      assert(amends.length === 1 && later.length === 1, `one amend, no save loop: ${amends.length} then ${later.length}`);
      assert(get(id).body === want, "the body unchanged since");
      const looped = seen.console.filter((l) => /effect_update_depth_exceeded/.test(l));
      assert(!looped.length, `no effect loops: ${looped.join("\n")}`);
    } finally {
      await context.close();
    }
  });

  await test("a_tag_without_its_ticket_says_so", async () => {
    const id = child("a tag with no id", "Here:\n\n<tweet-thread></tweet-thread>\n");
    const { context, page } = await openPage(`#/ticket/${id}`);
    try {
      const tt = page.locator("[data-surface=tag-component][data-name=tweet-thread] [data-surface=tweet-thread]");
      await eventually("drawn, saying it names no ticket", async () => (await tt.locator("[data-field=state]").getAttribute("data-state")) === "missing", 20_000);
      assert(/give the tag its ticket/.test(await tt.locator("[data-field=state]").textContent()), "the note");
      assert(get(id).body === "Here:\n\n<tweet-thread></tweet-thread>\n", "nothing written");
    } finally {
      await context.close();
    }
  });

  await test("a_ticket_that_is_not_there_says_so", async () => {
    const { context, page } = await openPage(`#/thread/${EPIC}-99999`);
    try {
      const tt = page.locator("[data-surface=tweet-thread]");
      await eventually("missing", async () => (await tt.locator("[data-field=state]").getAttribute("data-state")) === "missing");
      assert((await tt.locator("[data-post]").count()) === 0, "no post drawn");
      assert(await tt.locator("[data-action=save]").isDisabled(), "nothing to save");
    } finally {
      await context.close();
    }
  });
};
