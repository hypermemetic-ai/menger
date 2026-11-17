// EDITOR-31: an image in a body opens large on a click (ui/src/image-zoom.svelte, placed by app-top). One image
// put by synapse (`media put`) and named in a ticket's body and a doc's: a click on it on `#/ticket/:id` opens the
// modal, fitted to the window and captioned by its alt text, and Escape closes it; a click on it in the editor
// (`#/note/:id`) opens it too, and the block stays drawn with no caret in it; an image inside a link follows the
// link, and a click with ctrl held opens no zoom.
//
// Marks: `dialog[data-surface=image-zoom]` (`open`, `data-open`), `[data-field=image|caption]`,
// `[data-action=close|open-original]`.
import zlib from "node:zlib";

/** A `w`×`h` PNG of one colour. */
function png(w, h, [r, g, b]) {
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type, "latin1"), data]);
    const c = Buffer.alloc(4); c.writeUInt32BE(zlib.crc32(td) >>> 0);
    return Buffer.concat([len, td, c]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2;
  const raw = Buffer.alloc((w * 3 + 1) * h);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) raw.set([r, g, b], y * (w * 3 + 1) + 1 + x * 3);
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("IDAT", zlib.deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, fs, path, server } = ctx;
  let k = 0;
  const file = (name, data) => { const f = path.join(server.work, `zoom-${++k}-${name}`); fs.writeFileSync(f, data); return `@${f}`; };
  const IMG = png(1600, 900, [90, 140, 170]).toString("base64");
  const put = (owner, id) => {
    const [stored] = sy("media", "put", `--${owner}`, id, "--mime", "image/png", "--data", file("img.b64", IMG));
    assert(stored?.type === "stored" && stored.width === 1600, `media put: ${JSON.stringify(stored)}`);
    return stored.hash;
  };
  const body = (hash) => `Before the picture.\n\n![the kiln at cone 6](media:${hash})\n\nA linked one: [![linked](media:${hash})](https://example.com/away)\n\nAfter the picture.\n`;

  const root = sy("tickets", "create", "--epic", "ZOOM", "--kind", "scope", "--title", "the zoom's tickets").find((e) => e.type === "created").ticket.id;
  const tid = sy("tickets", "create", "--epic", "ZOOM", "--parent", root, "--title", "a ticket with a picture", "--body", file("t0.md", "placeholder\n")).find((e) => e.type === "created").ticket.id;
  const tHash = put("ticket", tid);
  sy("tickets", "amend", "--id", tid, "--body", file("t1.md", body(tHash)));

  const ZOOM = "dialog[data-surface=image-zoom]";
  const zoom = (page) => page.evaluate((s) => {
    const d = document.querySelector(s);
    const img = d?.querySelector("[data-field=image]");
    const r = img?.getBoundingClientRect();
    return d ? { open: d.open, src: img?.getAttribute("src") ?? null, caption: d.querySelector("[data-field=caption]")?.textContent ?? null, w: r?.width ?? 0, h: r?.height ?? 0, vw: innerWidth, vh: innerHeight } : null;
  }, ZOOM);
  const BODY_IMG = (scope) => `${scope} img[alt="the kiln at cone 6"] >> visible=true`;

  await test("a_click_on_an_image_in_a_ticket_opens_it_large", async () => {
    const { context, page } = await openPage(`#/ticket/${tid}`);
    try {
      const img = page.locator(BODY_IMG("[data-surface=ticket-view]")).first();
      await img.waitFor({ timeout: 20_000 }).catch(async (e) => {
        const seen = await page.evaluate(() => [...document.querySelectorAll("[data-surface=ticket-view] img")].map((i) => { const r = i.getBoundingClientRect(); return `${i.alt}:${r.width}x${r.height}:${i.closest("[hidden],details:not([open])") ? "hid" : "-"}`; }));
        throw new Error(`${e.message}; images: ${JSON.stringify(seen)}`);
      });
      assert((await page.locator(ZOOM).count()) === 1, "one zoom on the page");
      assert(!(await zoom(page)).open, "closed before the click");
      await img.click();
      let z = null;
      await eventually("the zoom opens", async () => (z = await zoom(page))?.open, 5000);
      assert(z.src.endsWith(`/media/${tHash}`), `it shows the image: ${z.src}`);
      assert(z.caption === "the kiln at cone 6", `captioned by its alt: ${z.caption}`);
      assert(z.w <= z.vw && z.h <= z.vh && z.w > 300, `fitted to the window: ${JSON.stringify(z)}`);
      await page.keyboard.press("Escape");
      await eventually("Escape closes it", async () => !(await zoom(page)).open, 3000);

      // A click outside the image closes it; the close button does.
      await img.click();
      await eventually("open again", async () => (await zoom(page)).open, 3000);
      await page.mouse.click(5, (await zoom(page)).vh - 5);
      await eventually("a click outside closes it", async () => !(await zoom(page)).open, 3000);
      await img.click();
      await eventually("open again", async () => (await zoom(page)).open, 3000);
      await page.locator(`${ZOOM} [data-action=close]`).click();
      await eventually("close closes it", async () => !(await zoom(page)).open, 3000);

      // ctrl held: the browser's, no zoom.
      await img.click({ modifiers: ["Control"] });
      await sleep(300);
      assert(!(await zoom(page)).open, "a ctrl-click opens no zoom");

      // An image inside a link follows the link (the click is the link's, not the zoom's).
      // (the window's listener, heard after the page's, keeps the page here)
      const linkedOpened = await page.evaluate(() => {
        const img = document.querySelector("[data-surface=ticket-view] a img[alt=linked]");
        if (!img) return "no linked image";
        let followed = false;
        const keep = (ev) => { followed = !ev.defaultPrevented; ev.preventDefault(); };
        window.addEventListener("click", keep, { once: true });
        img.click();
        return followed;
      });
      assert(linkedOpened === true, `the link is followed: ${linkedOpened}`);
      assert(!(await zoom(page))?.open, "a linked image opens no zoom");
    } finally { await context.close(); }
  });

  await test("a_click_on_an_image_in_the_editor_opens_it_and_leaves_the_caret_out", async () => {
    const docId = sy("notes", "create", "--title", "a doc with a picture", "--body", file("d0.md", "placeholder\n")).find((e) => e.type === "created").note.id;
    const dHash = put("note", docId);
    sy("notes", "save", "--id", docId, "--body", file("d1.md", body(dHash)));
    const { context, page } = await openPage(`#/note/${docId}`);
    try {
      const img = page.locator(BODY_IMG("[data-surface=editor-body]")).first();
      await img.waitFor({ timeout: 20_000 });
      await img.click();
      await eventually("the zoom opens", async () => (await zoom(page))?.open, 5000);
      const caretIn = await page.evaluate(() => {
        const v = document.querySelector("[data-surface=editor-body] [data-field=body]")?.editorView;
        return { focused: v?.hasFocus ?? null, markdown: document.querySelector("[data-surface=editor-body] .cm-content")?.textContent.includes("](media:") ?? null };
      });
      assert(!caretIn.markdown, `the block stays drawn, its markdown not shown: ${JSON.stringify(caretIn)}`);
      await page.keyboard.press("Escape");
      await eventually("Escape closes it", async () => !(await zoom(page)).open, 3000);
    } finally { await context.close(); }
  });
};
