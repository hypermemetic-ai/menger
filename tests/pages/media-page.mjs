// EDITOR-32: the media page, #/media (page-media) — every kept file the reader may see, as a grid:
// an image a thumbnail from /media/<hash> (in a `data-zoomable` grid, never inside a link or a
// control, so EDITOR-31's zoom opens it), any other kind its card; under each, its tickets (by
// ticket-links) and documents (#/doc/<id>); chips by kind; more pages on "more" or on scroll.
// Run alone by `node tests/pages_browser.mjs --group media-page`. Who may see which is the
// wire's (tests/wire_media.rs: media_list_answers_each_reader_what_it_may_see_and_where_it_is_used).
//
// Marks: `[data-surface=media-page]`, `[data-kind-chip=<kind>|all][aria-pressed]`,
// `[data-media-grid][data-zoomable]`, a file `[data-hash][data-kind]` (`img[data-thumb]`,
// `[data-field=name|size|kind]`, `[data-action=download]`), `[data-owner=ticket|doc][data-id]`,
// `[data-action=more]`, `[data-empty]`.

import zlib from "node:zlib";

/** A real `w`×`h` PNG, `tint` making two of a size differ. */
function png(w, h, tint = 0) {
  const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = (buf) => { let c = 0xffffffff; for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type, "ascii"), data]);
    const c = Buffer.alloc(4); c.writeUInt32BE(crc(td));
    return Buffer.concat([len, td, c]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2;
  const rows = [];
  for (let y = 0; y < h; y++) {
    const row = Buffer.alloc(1 + w * 3);
    for (let x = 0; x < w; x++) { row[1 + x * 3] = (40 + x + tint) % 256; row[2 + x * 3] = 120; row[3 + x * 3] = 200; }
    rows.push(row);
  }
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", ihdr), chunk("IDAT", zlib.deflateSync(Buffer.concat(rows))), chunk("IEND", Buffer.alloc(0))]).toString("base64");
}

/** The first bytes of an MP4 (`ftyp`), `n` long. */
function mp4(n) {
  const b = Buffer.alloc(n);
  b.writeUInt32BE(24, 0); b.write("ftypisom", 4, "ascii"); b.write("isomavc1", 16, "ascii");
  return b.toString("base64");
}

export default async (ctx) => {
  const { test, assert, eventually, openPage, sy, fs, path, session } = ctx;
  const scratch = path.dirname(session);
  const file = (name, text) => { const f = path.join(scratch, `media-page-${name}`); fs.writeFileSync(f, text); return `@${f}`; };
  const made = (...a) => {
    const c = sy("tickets", "create", ...a).find((e) => e.type === "created");
    assert(c, `created: ${a.join(" ")}`);
    return c.ticket;
  };
  const put = (flag, id, name, data) => {
    const [stored] = sy("media", "put", flag, id, "--name", name, "--data", file(`${name}.b64`, data));
    assert(stored?.type === "stored", `media put ${name}: ${JSON.stringify(stored)}`);
    return stored.hash;
  };

  // An image on two tickets; a video on the first; a PDF on a document.
  made("--epic", "MEDIAPG", "--kind", "scope", "--title", "the media page");
  const one = made("--epic", "MEDIAPG", "--title", "the first with a picture");
  const two = made("--epic", "MEDIAPG", "--title", "the second with the same picture");
  const IMG = png(64, 48, 7);
  const img = put("--ticket", one.id, "kiln.png", IMG);
  assert(put("--ticket", two.id, "kiln.png", IMG) === img, "the same bytes: one file");
  const video = put("--ticket", one.id, "firing.mp4", mp4(64));
  const docId = sy("docs", "create", "--title", "a deck", "--body", file("doc.md", "a deck\n")).find((e) => e.type === "created").doc.id;
  const pdf = put("--doc", docId, "deck.pdf", Buffer.from("%PDF-1.4 the deck").toString("base64"));

  await test("media_page_an_image_on_two_tickets_drawn_once", async () => {
    const { context, page, seen } = await openPage("#/media");
    try {
      const root = page.locator("[data-surface=media-page]");
      const tile = root.locator(`[data-hash="${img}"]`);
      await eventually("the image is drawn", async () => (await tile.count()) === 1, 15_000);
      assert((await root.locator(`[data-hash="${img}"]`).count()) === 1, "once");
      // A thumbnail from /media/<hash>, loaded, never inside a link or a control, in a zoomable grid.
      const thumb = tile.locator("img[data-thumb]");
      assert((await thumb.getAttribute("src")).endsWith(`/media/${img}`), "from /media/<hash>");
      await eventually("the thumbnail loads", async () => thumb.evaluate((e) => e.complete && e.naturalWidth === 64), 10_000);
      assert(await thumb.evaluate((e) => !e.closest("a, button, label") && !!e.closest("[data-media-grid][data-zoomable]")), "zoomable: in the marked grid, in no link or control");
      // Its tickets, by the one link helper; the download.
      const owners = await tile.locator("[data-owner=ticket]").evaluateAll((as) => as.map((a) => [a.dataset.id, a.getAttribute("href")]));
      assert(JSON.stringify(owners.map((o) => o[0]).sort()) === JSON.stringify([one.id, two.id].sort()), `both tickets: ${JSON.stringify(owners)}`);
      for (const [id, href] of owners) assert(href === `#/ticket/${id}` || new RegExp(`^#/t/[0-9a-z]{8}/`).test(href), `${id}'s link from ticket-links: ${href}`);
      assert((await tile.locator("[data-action=download]").getAttribute("href")).endsWith(`/media/${img}`), "the download");
      assert((await tile.locator("[data-field=name]").textContent()).trim() === "kiln.png", "its name");
      // The PDF is a card, its document linked.
      const card = root.locator(`[data-hash="${pdf}"]`);
      await eventually("the pdf's card", async () => (await card.count()) === 1);
      assert((await card.getAttribute("data-kind")) === "pdf" && (await card.locator("img").count()) === 0, "a card, no thumbnail");
      assert((await card.locator(`[data-owner=doc][data-id="${docId}"]`).getAttribute("href")) === `#/doc/${docId}`, "the document by #/doc/<id>");
      // A ticket's link goes to its page.
      await tile.locator(`[data-owner=ticket][data-id="${one.id}"]`).click();
      await eventually("the ticket's page", async () => /^#\/(t\/|ticket\/)/.test(await page.evaluate(() => location.hash)));
      assert(!seen.console.some((l) => l.startsWith("pageerror")), `no page error: ${seen.console.filter((l) => l.startsWith("pageerror")).join(" | ")}`);
    } finally {
      await context.close();
    }
  });

  await test("media_page_a_kind_chip_shows_that_kind_alone", async () => {
    const { context, page } = await openPage("#/media");
    try {
      const root = page.locator("[data-surface=media-page]");
      await eventually("the grid", async () => (await root.locator(`[data-hash="${img}"]`).count()) === 1, 15_000);
      await root.locator("[data-kind-chip=video]").click();
      await eventually("videos alone", async () => {
        const kinds = await root.locator("[data-hash]").evaluateAll((ts) => ts.map((t) => t.dataset.kind));
        return kinds.length >= 1 && kinds.every((k) => k === "video");
      });
      assert((await root.locator(`[data-hash="${video}"]`).count()) === 1, "the video is there");
      assert((await root.locator("[data-kind-chip=video]").getAttribute("aria-pressed")) === "true", "its chip pressed");
      await root.locator("[data-kind-chip=audio]").click();
      await eventually("no audio: said", async () => (await root.locator("[data-empty]").count()) === 1);
      await root.locator("[data-kind-chip=all]").click();
      await eventually("every kind again", async () => (await root.locator(`[data-hash="${pdf}"], [data-hash="${img}"]`).count()) === 2);
    } finally {
      await context.close();
    }
  });

  await test("media_page_more_pages_on_scroll", async () => {
    // Enough images for two pages of 48.
    const many = made("--epic", "MEDIAPG", "--title", "a ticket with many pictures");
    for (let i = 0; i < 50; i++) put("--ticket", many.id, `many-${i}.png`, png(4, 4, i + 20));
    const { context, page } = await openPage("#/media");
    try {
      const root = page.locator("[data-surface=media-page]");
      await eventually("the first page", async () => (await root.locator("[data-hash]").count()) >= 48, 15_000);
      await root.locator("[data-action=more]").scrollIntoViewIfNeeded().catch(() => {});
      await page.mouse.wheel(0, 100_000);
      await eventually("the next page, on scroll", async () => (await root.locator("[data-hash]").count()) >= 53, 15_000);
      assert((await root.locator(`[data-hash="${img}"]`).count()) === 1, "the oldest image, from the second page, once");
    } finally {
      await context.close();
    }
  });
};
