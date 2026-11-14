// EDITOR-30: a choice between images, picked like a radio — `<image-pick>` in block form around an
// `<m-decision id>` whose `<m-option>`s carry `image="media:<hash>"` (ui/src/image-pick.svelte), drawn by its
// tag as one radio group, the pick the decision's answer (`tickets.inputs.answer`), drawn from the answer as it
// stands (`tickets.inputs.list`, live on `tickets.watch`).
//
// Marks: `[data-surface=image-pick]` (`data-ticket`, `data-input`, `data-picked`, `data-state=saved|saving|
// refused|unsaved|loading|empty`), `[role=radio][data-option=<value>]` (`aria-checked`), `[data-field=question|
// state|caption|full]`.
//
// The images are small PNGs made here, one colour each; IMAGE_PICK_DIR names a directory of PNGs to use instead
// (the first four, by name), and PAGES_SHOTS a directory for the screenshots it takes: one picked, another
// after a click, the keyboard's focus, dark, and 390 px wide.
import fs from "node:fs";
import zlib from "node:zlib";
import { makeKey, readToken, writeSession } from "../../scripts/throwaway-login.mjs";
import { sentCalls } from "../../scripts/ui-throwaway.mjs";

/** A `w`×`h` PNG of one colour `[r, g, b]`, with a darker band across its top. */
function png(w, h, [r, g, b]) {
  const crc = (buf) => zlib.crc32(buf);
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type, "latin1"), data]);
    const c = Buffer.alloc(4); c.writeUInt32BE(crc(td) >>> 0);
    return Buffer.concat([len, td, c]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2;
  const raw = Buffer.alloc((w * 3 + 1) * h);
  for (let y = 0; y < h; y++) {
    const k = y < h / 5 ? 0.6 : 1;
    for (let x = 0; x < w; x++) raw.set([r * k, g * k, b * k], y * (w * 3 + 1) + 1 + x * 3);
  }
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("IDAT", zlib.deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, server, session, path } = ctx;
  const EPIC = "PICK";
  const here = path.dirname(new URL(import.meta.url).pathname);
  const shots = process.env.PAGES_SHOTS || "";
  const shot = async (page, name) => { if (shots) await page.screenshot({ path: path.join(shots, `image-pick-${name}.png`), fullPage: true }); };
  const srcOf = (name) => fs.readFileSync(path.join(here, "..", "..", "ui", "src", `${name}.svelte`), "utf8");
  let k = 0;
  const file = (name, data) => { const f = path.join(server.work, `pick-${++k}-${name}`); fs.writeFileSync(f, data); return f; };
  const make = (title, body = null, ...flags) => {
    const out = sy("tickets", "create", "--epic", EPIC, "--title", title, ...(body == null ? [] : ["--body", `@${file("body.md", body)}`]), ...flags);
    const c = out.find((e) => e.type === "created");
    if (!c) throw new Error(`create ${title}: ${JSON.stringify(out)}`);
    return c.ticket.id;
  };
  const root = make("the pickers' shortcode", null, "--kind", "scope");
  const answerOf = (id, input) => sy("tickets", "inputs", "list", "--ticket", id).find((e) => e.input?.id === input)?.input?.answer?.value?.option ?? null;

  /** image-pick's module script (its import of tag-source swapped for tag-source's own), run here in node. */
  const pure = () => {
    const mod = (name) => {
      const m = /<script module>([\s\S]*?)<\/script>/.exec(srcOf(name));
      assert(m, `${name} has a <script module>`);
      return m[1].replace(/^\s*import .*$/gm, "").replace(/^\s*export\s+/gm, "").replace(/^\s*const uses = .*$/gm, "");
    };
    return new Function(`${mod("tag-source")}\n${mod("image-pick")}\nreturn { readPick, moved, imageUrl };`)();
  };

  await test("the_tag_reads_as_the_server_reads_its_decision", async () => {
    const { readPick, moved, imageUrl } = pure();
    const H = "Ab".repeat(32);
    const src = `<image-pick>\n<m-decision id="study" data-field="decision">\n<m-question data-field="question">Which <b>study</b>?</m-question>\n`
      + `<m-option name="A" image="media:${H}">**Sunk.** words &amp; more</m-option>\n<m-option name="B" value="b-val" image="media:${H}">\nTwo\nlines\n</m-option>\n`
      + `<m-option name="A">a second A: not an option</m-option>\n<m-option>no name: not an option</m-option>\n</m-decision>\n</image-pick>`;
    const r = readPick(src);
    assert(r.input === "study" && r.question === "Which study?", `the input and its question: ${JSON.stringify(r)}`);
    assert(r.options.length === 2 && r.options[0].value === "A" && r.options[0].label === "A" && r.options[0].text === "**Sunk.** words &amp; more", `A: ${JSON.stringify(r.options[0])}`);
    assert(r.options[1].value === "b-val" && r.options[1].label === "B" && r.options[1].text === "Two\nlines", `B, by its value: ${JSON.stringify(r.options[1])}`);
    assert(readPick("<image-pick>\n<m-decision>\n<m-option name=\"A\"></m-option>\n</m-decision>\n</image-pick>").input === null, "no id: no input");
    assert(imageUrl(`media:${H}`) === `/media/${H.toLowerCase()}`, "media: → this origin's /media, lower-cased");
    assert(imageUrl("https://example.com/a.png") === null && imageUrl("javascript:x") === null, "another origin, or no image: none");
    assert(imageUrl("data:image/png;base64,iVBORw0KGgo=") === "data:image/png;base64,iVBORw0KGgo=", "a data: image as it is");
    assert(moved("ArrowRight", 3, 4) === 0 && moved("ArrowLeft", 0, 4) === 3 && moved("ArrowDown", 1, 4) === 2 && moved("End", 0, 4) === 3 && moved("Home", 2, 4) === 0 && moved("x", 0, 4) === null, "the arrows wrap; Home, End");
  });

  // The images, kept by the media service for the ticket.
  const COLOURS = [[220, 226, 236], [196, 214, 238], [232, 214, 196], [206, 230, 206]];
  const given = process.env.IMAGE_PICK_DIR ? fs.readdirSync(process.env.IMAGE_PICK_DIR).filter((f) => /\.png$/i.test(f)).sort().slice(0, 4) : [];
  const putImage = (id, i) => {
    const bytes = given[i] ? fs.readFileSync(path.join(process.env.IMAGE_PICK_DIR, given[i])) : png(320, 200, COLOURS[i]);
    const out = sy("media", "put", "--ticket", id, "--name", `study-${i}.png`, "--data", `@${file(`study-${i}.b64`, bytes.toString("base64"))}`);
    const s = out.find((e) => e.type === "stored");
    if (!s) throw new Error(`media put: ${JSON.stringify(out)}`);
    return s.hash;
  };
  const LETTERS = ["A", "B", "C", "D"];
  const WORDS = ["Sunk: a hairline and a shadow under the top edge.", "Sunk: a shadow round the whole perimeter.", "Sunk into a bezel.", "Flat: a hairline alone."];
  const BEFORE = "Four studies of one canvas.\n\n";
  const AFTER = "\n\nAnd words after it, *kept*.\n";
  const pickTag = (hashes) => `<image-pick>\n<m-decision id="study">\n<m-question>Which study goes on to film?</m-question>\n`
    + hashes.map((h, i) => `<m-option name="${LETTERS[i]}" image="media:${h}">**${WORDS[i].split(":")[0]}.** ${WORDS[i]}</m-option>`).join("\n")
    + `\n</m-decision>\n</image-pick>`;

  // The ticket: made, then its images put for it, then its body written with the tag.
  const id = make("four studies", "drafting", "--parent", root);
  const hashes = LETTERS.map((_, i) => putImage(id, i));
  const BODY = `${BEFORE}${pickTag(hashes)}${AFTER}`;
  const am = sy("tickets", "amend", "--id", id, "--body", `@${file("body.md", BODY)}`);
  assert(am.some((e) => e.type === "amended"), `amended: ${JSON.stringify(am)}`);

  const radios = (page) => page.locator("[data-surface=image-pick] [role=radio]");
  const checked = async (page) => (await radios(page).evaluateAll((rs) => rs.filter((r) => r.getAttribute("aria-checked") === "true").map((r) => r.dataset.option)));

  await test("a_click_picks_one_and_the_pick_is_the_answer", async () => {
    const inputs = sy("tickets", "inputs", "list", "--ticket", id).filter((e) => e.input).map((e) => e.input);
    assert(inputs.length === 1 && inputs[0].id === "study" && inputs[0].kind === "choice" && inputs[0].options.map((o) => o.value).join() === "A,B,C,D", `the server reads the decision inside the tag: ${JSON.stringify(inputs)}`);
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    try {
      const ip = page.locator("[data-surface=tag-component][data-name=image-pick] [data-surface=image-pick]");
      await eventually("drawn natively, four images", async () => (await radios(page).count()) === 4, 20_000).catch(async (e) => {
        const what = await page.locator("[data-ticket-body]").first().innerHTML({ timeout: 1000 }).catch(() => "(no body)");
        throw new Error(`${e.message}\nthe body drawn: ${what.slice(0, 2000)}\nconsole: ${seen.console.slice(-10).join("\n")}`);
      });
      assert((await page.locator("[data-surface=tag-component][data-name=image-pick]").getAttribute("data-mode")) === "native", "native");
      assert((await ip.getAttribute("data-ticket")) === id && (await ip.getAttribute("data-input")) === "study", "told its ticket; its input the decision's id");
      assert((await ip.locator("[role=radiogroup]").getAttribute("aria-label")) === "Which study goes on to film?", "the group named by its question");
      await eventually("every image loaded from this origin's /media", async () => (await ip.locator("img").evaluateAll((is) => is.length === 4 && is.every((i) => i.complete && i.naturalWidth > 0 && new URL(i.src).pathname.startsWith("/media/")))), 10_000);
      await eventually("nothing picked yet", async () => (await ip.getAttribute("data-state")) === "saved" && (await checked(page)).length === 0);
      assert((await ip.locator("[data-field=state]").textContent()).trim() === "nothing picked yet", "said");
      assert((await radios(page).evaluateAll((rs) => rs.map((r) => r.tabIndex).join())) === "0,-1,-1,-1", "one tab stop: the first, nothing picked");
      assert(await page.locator("[data-ticket-body]").evaluate((b) => b.textContent.includes("And words after it")), "the words around it drawn");

      await radios(page).nth(2).click();
      await eventually("C alone picked", async () => (await checked(page)).join() === "C");
      await eventually("the answer is C", async () => answerOf(id, "study") === "C", 8_000);
      await eventually("saved, said", async () => (await ip.getAttribute("data-state")) === "saved" && /^picked C/.test((await ip.locator("[data-field=state]").textContent()).trim()));
      await shot(page, "picked-c");

      await radios(page).nth(1).click();
      await eventually("B alone picked", async () => (await checked(page)).join() === "B");
      await eventually("the answer is B", async () => answerOf(id, "study") === "B", 8_000);
      await eventually("B the tab stop", async () => (await radios(page).evaluateAll((rs) => rs.map((r) => r.tabIndex).join())) === "-1,0,-1,-1");
      await shot(page, "picked-b");

      // The keyboard: from the picked one, right twice — D picked and focused, one answer once the keys rest.
      const before = (await sentCalls(page)).filter((c) => c.method === "tickets.inputs.answer").length;
      await radios(page).nth(1).focus();
      await page.keyboard.press("ArrowRight");
      await page.keyboard.press("ArrowRight");
      await eventually("D picked and focused", async () => (await checked(page)).join() === "D" && (await page.evaluate(() => document.activeElement?.dataset?.option)) === "D");
      await eventually("the answer is D", async () => answerOf(id, "study") === "D", 8_000);
      await sleep(800);
      const sent = (await sentCalls(page)).filter((c) => c.method === "tickets.inputs.answer");
      assert(sent.length - before === 1 && sent.at(-1).params.value.option === "D", `one answer for the run of keys: ${JSON.stringify(sent.slice(before).map((c) => c.params.value))}`);
      await shot(page, "keyboard-d");
      // Space on another picks it at once; the right arrow from the last wraps to the first.
      await page.keyboard.press("ArrowRight");
      await eventually("wrapped to A", async () => (await checked(page)).join() === "A");
      await eventually("the answer is A", async () => answerOf(id, "study") === "A", 8_000);

      // Another's answer (an agent's, here) is drawn: the pick follows the watch.
      sy("tickets", "inputs", "answer", "--ticket", id, "--input", "study", "--value.type", "choice", "--value.option", "C");
      await eventually("C picked, from the watch", async () => (await checked(page)).join() === "C", 8_000);
      // The ticket's questions show the same input, answered C.
      await eventually("the questions say C", async () => (await page.locator("[data-surface=ticket-questions] [data-input=study] [data-option=C]").getAttribute("aria-pressed").catch(() => null)) === "true", 8_000);

      const errors = seen.console.filter((l) => /^(error|pageerror)/.test(l) && !/404|Failed to load resource/i.test(l));
      assert(errors.length === 0, `no error in the page: ${errors.join("\n")}`);
      assert(!seen.console.some((l) => /effect_update_depth_exceeded/.test(l)), "no effect loops");
    } finally {
      await context.close();
    }
  });

  await test("dark_and_narrow", async () => {
    if (!shots) return;
    const { context, page } = await openPage(`#/ticket/${id}`);
    try {
      await eventually("drawn, C picked", async () => (await checked(page)).join() === "C", 20_000);
      await page.emulateMedia({ colorScheme: "dark" });
      await sleep(300);
      await shot(page, "dark");
      await page.emulateMedia({ colorScheme: "light" });
      await page.setViewportSize({ width: 390, height: 844 });
      await sleep(400);
      const wide = await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1);
      assert(wide, "no sideways scroll at 390");
      await shot(page, "narrow");
    } finally {
      await context.close();
    }
  });

  await test("a_reader_who_may_not_answer_is_refused_and_the_answer_stays", async () => {
    const keyFile = path.join(path.dirname(session), "pick-viewer.key");
    const viewer = makeKey(keyFile);
    sy("members", "grant", "--principal", viewer, "--scope", `under ${root}`, "--role", "viewer");
    const viewerSession = path.join(path.dirname(session), "pick-viewer.session");
    await writeSession(`ws://127.0.0.1:${server.ws}`, keyFile, viewerSession, "pick-viewer");
    const { context, page } = await openPage(`#/ticket/${id}`, { as: readToken(viewerSession) });
    try {
      const ip = page.locator("[data-surface=image-pick]");
      await eventually("drawn for the viewer, C picked", async () => (await checked(page)).join() === "C", 20_000);
      await radios(page).nth(0).click();
      await eventually("refused, said", async () => (await ip.getAttribute("data-state")) === "refused", 8_000);
      const said = (await ip.locator("[data-field=state]").textContent()).trim();
      assert(/needs tickets\.inputs\.answer|forbidden/i.test(said), `the service's words: ${said}`);
      assert((await checked(page)).join() === "C", "the answered pick drawn again");
      assert(answerOf(id, "study") === "C", "the answer unchanged");
    } finally {
      await context.close();
    }
  });

  await test("with_no_decision_id_it_says_it_saves_nothing", async () => {
    const t = make("a pick that saves nothing", `<image-pick>\n<m-decision>\n<m-option name="A" image="media:${hashes[0]}">one</m-option>\n<m-option name="B" image="media:${hashes[1]}">two</m-option>\n</m-decision>\n</image-pick>\n`, "--parent", root);
    const { context, page } = await openPage(`#/ticket/${t}`);
    try {
      const ip = page.locator("[data-surface=image-pick]");
      await eventually("drawn", async () => (await radios(page).count()) === 2, 20_000);
      assert((await ip.getAttribute("data-state")) === "unsaved", "unsaved");
      await radios(page).nth(1).click();
      await eventually("B picked here", async () => (await checked(page)).join() === "B");
      assert(/give its m-decision an id/.test(await ip.locator("[data-field=state]").textContent()), "says why");
      assert(!(await sentCalls(page)).some((c) => c.method === "tickets.inputs.answer"), "nothing sent");
    } finally {
      await context.close();
    }
  });
};
