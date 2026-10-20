#!/usr/bin/env node
// CHAT-78: a markdown table in a chat message keeps each cell at least as
// wide as its longest word, and a table wider than its bubble scrolls inside
// the bubble instead of crushing a column or pushing the page sideways. The
// check takes frontend/index.html's own stylesheet (no wasm, no server) and
// lays the markup the chat pane draws for a message (#chatpane > .pmb > .md)
// around it, at 390 and 1440, light and dark. Nothing to start:
//
//   node scripts/chat-table-check.mjs
//
// Exit 1 on a failure.
import { createRequire } from "node:module";
import fs from "node:fs";
const require = createRequire(process.env.PLAYWRIGHT_FROM ? `${process.env.PLAYWRIGHT_FROM}/` : new URL("../perf/", import.meta.url));
const pw = require("playwright");

const html = fs.readFileSync(new URL("../frontend/index.html", import.meta.url), "utf8");
const css = [...html.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/g)].map((m) => m[1]).join("\n");

// The owner's table (a message of 2026-10-20): an empty header cell, long
// second column, short first column.
const narrow = `<table><thead><tr><th></th><th>Features</th></tr></thead><tbody>
<tr><td>Core</td><td>Ticket filing, the board and the chat pane, all three under one filter and one sign-in</td></tr>
<tr><td>Support</td><td>Screenshots, comments and the annotation mode, with images in every box</td></tr>
<tr><td>Later</td><td>Offline reading, a phone's share sheet and the rest of the menu</td></tr>
</tbody></table>`;
// A table no bubble holds: eight columns of long unbroken words.
const wide = `<table><thead><tr>${"<th>identifier</th>".repeat(8)}</tr></thead><tbody>
<tr>${"<td>supercalifragilisticexpialidocious</td>".repeat(8)}</tr></tbody></table>`;

const page_html = (table) => `<!doctype html><html><head><meta charset="utf-8"><style>${css}</style></head>
<body><main id="chatpane"><div class="turn"><div class="pmb"><div class="md">${table}</div></div></div></main></body></html>`;

const fails = [];
const ok = (cond, what) => { if (!cond) fails.push(what); console.log(`${cond ? "ok  " : "FAIL"} ${what}`); };

const browser = await pw.chromium.launch();
try {
  for (const width of [390, 1440]) {
    for (const theme of ["light", "dark"]) {
      const tag = `${width} ${theme}`;
      const page = await browser.newPage({ viewport: { width, height: 900 } });
      // The bubble's width at each device: a phone's bubble is the screen less its margins.
      const paneWidth = width === 390 ? 342 : 560;

      // 1. The owner's table: no cell narrower than its longest word.
      await page.setContent(page_html(narrow));
      await page.evaluate(({ theme, paneWidth }) => {
        document.documentElement.setAttribute("data-theme", theme);
        document.getElementById("chatpane").style.width = `${paneWidth}px`;
      }, { theme, paneWidth });
      const cells = await page.evaluate(() => [...document.querySelectorAll("#chatpane td, #chatpane th")].map((cell) => {
        const longest = cell.textContent.trim().split(/\s+/).reduce((a, b) => (b.length > a.length ? b : a), "");
        const cs = getComputedStyle(cell);
        const pad = parseFloat(cs.paddingLeft) + parseFloat(cs.paddingRight);
        // An absolutely placed probe, nowrap, holding the longest word: its width is the cell's floor.
        const probe = document.createElement("span");
        probe.textContent = longest;
        probe.style.cssText = "position:absolute;white-space:nowrap;visibility:hidden";
        cell.appendChild(probe);
        const floor = probe.getBoundingClientRect().width + pad;
        probe.remove();
        return { text: cell.textContent.trim(), width: cell.getBoundingClientRect().width, floor, longest };
      }));
      for (const c of cells) {
        ok(c.width + 0.5 >= c.floor, `${tag}: cell "${c.text.slice(0, 24)}" is ${c.width.toFixed(0)}px, its longest word "${c.longest}" needs ${c.floor.toFixed(0)}px`);
      }
      const bubble = await page.evaluate(() => document.querySelector("#chatpane .md").getBoundingClientRect().width);
      const pageSideways = await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth);
      ok(!pageSideways, `${tag}: the owner's table does not push the page sideways`);
      ok(bubble <= paneWidth + 0.5, `${tag}: the table stays inside the bubble (${bubble.toFixed(0)}px of ${paneWidth}px)`);

      // 2. A wide table: it scrolls inside the bubble.
      await page.setContent(page_html(wide));
      await page.evaluate(({ theme, paneWidth }) => {
        document.documentElement.setAttribute("data-theme", theme);
        document.getElementById("chatpane").style.width = `${paneWidth}px`;
      }, { theme, paneWidth });
      const wideState = await page.evaluate(() => {
        const table = document.querySelector("#chatpane table");
        const scroller = table.parentElement;
        return {
          page: document.documentElement.scrollWidth > window.innerWidth,
          // The table, or its wrapper, is the box that scrolls: it holds more than it shows.
          overflows: [table, scroller].some((el) => el.scrollWidth > el.clientWidth + 1 && getComputedStyle(el).overflowX !== "visible"),
          bubble: document.querySelector("#chatpane .pmb").getBoundingClientRect().width,
        };
      });
      ok(!wideState.page, `${tag}: a wide table does not push the page sideways`);
      ok(wideState.overflows, `${tag}: a wide table scrolls inside the bubble`);
      ok(wideState.bubble <= paneWidth + 0.5, `${tag}: a wide table leaves the bubble its width (${wideState.bubble.toFixed(0)}px of ${paneWidth}px)`);
      await page.close();
    }
  }
} finally {
  await browser.close();
}

if (fails.length) {
  console.log(`\n${fails.length} failing`);
  process.exit(1);
}
console.log("\nall checks pass");
