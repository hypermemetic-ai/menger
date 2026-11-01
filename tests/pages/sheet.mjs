// SURFACE-10: the sheet — every component of ui/src, in every state it has,
// in both schemes, at #/sheet (sheet-page, svelte/1). Run alone by
// `node tests/pages_browser.mjs --group sheet`.
//
// Marks: the page `[data-surface=sheet-page]` (`[data-sheet-id]`: the ticket
// the parts are drawn for — the first `tickets.list --limit 1` gives, or
// `#/sheet/:id`); a scheme `[data-sheet-scheme=light|dark]`, which is the
// theme's wrapper `[data-plexus-theme=light|dark]`; in it a component
// `[data-sheet-component=<name>]`, its states `[data-sheet-state=plain|hover|
// focus|selected|checked|disabled|loading|empty|refused]`, each holding the
// component's bound mark `[data-component=<name>]`. A state shown without the
// pointer is an element the sheet marked `[data-sheet~=<state>]` (APP-114's
// aliases: `:hover` also matches `[data-sheet~=hover]`, and so on).
//
// The screenshots, sheet-light.png and sheet-dark.png (each scheme's column,
// whole), are written into the work directory and their paths printed.
import { execFileSync } from "node:child_process";

/** The colours a sheet may not write: CSS's named ones a value could hold. */
const NAMED = ["white", "black", "red", "green", "blue", "gray", "grey", "silver", "yellow", "orange", "purple", "pink", "brown", "navy", "teal", "maroon", "olive", "lime", "aqua", "fuchsia", "cyan", "magenta", "gold", "indigo", "violet", "crimson", "tomato", "coral", "salmon", "khaki", "beige", "ivory", "tan", "wheat", "plum", "orchid", "turquoise", "lightgray", "lightgrey", "darkgray", "darkgrey", "whitesmoke", "gainsboro"];

/** Each literal colour in `css` (a sheet's text): `#hex`, a colour function, a named colour in a value. */
export function literals(css) {
  const out = [];
  const clean = css.replace(/\/\*[\s*\S]*?\*\//g, "").replace(/"[^"]*"|'[^']*'/g, '""');
  for (const decl of clean.split(/[;{}]/)) {
    const colon = decl.indexOf(":");
    if (colon < 0) continue;
    const prop = decl.slice(0, colon).trim();
    if (!/^-?-?[a-z-]+$/.test(prop)) continue; // a selector's pseudo-class, not a declaration
    const value = decl.slice(colon + 1).replace(/var\(--[a-z0-9-]+/g, "var(");
    const hex = value.match(/#[0-9a-fA-F]{3,8}\b/);
    if (hex && [4, 5, 7, 9].includes(hex[0].length)) out.push(`${prop}: ${hex[0]}`);
    const fn = value.match(/\b(rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(/i);
    if (fn) out.push(`${prop}: ${fn[1]}(…)`);
    for (const w of value.toLowerCase().match(/[a-z-]+/g) ?? []) if (NAMED.includes(w)) out.push(`${prop}: ${w}`);
  }
  return out;
}

export default async (ctx) => {
  const { test, assert, eventually, openPage, v1, v2, sy, groupsNow, fs, path, session } = ctx;
  const work = path.dirname(session);
  const repo = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..", "..");
  const names = fs.readdirSync(path.join(repo, "ui", "src"))
    .map((f) => f.replace(/\.(rhx|svelte)$/, ""))
    .filter((n) => n !== "sheet-page" && n !== "page-sheet") // STUDIO-12: page-sheet places the sheet
    .sort();
  const tar = (...a) => execFileSync("tar", a, { encoding: "utf8", maxBuffer: 64 << 20 });
  const sheets = () => tar("-tf", v2.tar).split("\n").filter((f) => /^components\/[^/]+\.css$/.test(f)).map((f) => [f, tar("-xOf", v2.tar, f)]);
  const scheme = (page, s) => page.locator(`[data-surface=sheet-page] [data-sheet-scheme=${s}]`);
  const live = () => sy("ui", "versions").find((e) => e.type === "version" && e.live)?.hash;
  /** Every section on the sheet filled — the layouts' and the pages' own, inside their tiles, not only the tiles' marks. */
  const sectionsFilled = (page) => eventually("every section inside the sheet's tiles is filled", async () => {
    const bare = await page.locator("[data-surface=sheet-page] [data-sheet-state] [data-slot]").evaluateAll((els) =>
      els.filter((e) => e.childElementCount === 0).map((e) => `${e.closest("[data-sheet-scheme]").dataset.sheetScheme}/${e.closest("[data-sheet-component]").dataset.sheetComponent}/${e.closest("[data-sheet-state]").dataset.sheetState}: ${e.dataset.layout ?? "?"}.${e.dataset.slot}`));
    if (bare.length) throw new Error(`unfilled: ${bare.slice(0, 20).join(", ")}${bare.length > 20 ? ` (+${bare.length - 20})` : ""}`);
    return true;
  }, 60_000);

  // The checks read what they are given.
  assert(JSON.stringify(literals(".a { color: #fff; background: var(--bg, white); border: 1px solid rgb(0 0 0); } .b:hover { color: var(--ink) }")) === JSON.stringify(["color: #fff", "background: white", "border: rgb(…)"]), `literals: ${JSON.stringify(literals(".a { color: #fff; background: var(--bg, white); border: 1px solid rgb(0 0 0); }"))}`);
  assert(literals(".a { color: var(--ink); grid-template-areas: \"white\"; transition: color var(--motion-fast) } .b[data-x=red] { margin: 0 }").length === 0, "a token, a string, a selector: no literal");

  // The sheet's ticket: one with something in every part — a body, a child,
  // a revision, two moves — under the last shortcode's root, drawn at #/sheet/:id.
  const g = groupsNow().filter((x) => x.root && !x.shortcode.startsWith("AAA")).at(-1);
  const bodyFile = path.join(work, "sheet-body.md");
  fs.writeFileSync(bodyFile, "## Problem\n\nThe sheet draws every part of this ticket.\n\n- a list\n- of two\n");
  const [made] = sy("tickets", "create", "--epic", g.epic, "--parent", g.root, "--title", "the sheet's ticket", "--body", `@${bodyFile}`);
  const sheetId = made?.ticket?.id;
  assert(sheetId, `the sheet's ticket made: ${JSON.stringify(made)}`);
  sy("tickets", "create", "--epic", g.epic, "--parent", sheetId, "--title", "a child on the sheet");
  sy("tickets", "amend", "--id", sheetId, "--title", "the sheet's ticket, amended");
  sy("tickets", "set_status", "--id", sheetId, "--status", "Ready");
  sy("tickets", "set_status", "--id", sheetId, "--status", "Coding");

  async function sheetPage(route = `#/sheet/${sheetId}`) {
    const o = await openPage(route);
    await eventually("the sheet draws its two schemes", async () => (await o.page.locator("[data-surface=sheet-page] [data-sheet-scheme]").count()) === 2, 30_000);
    return o;
  }

  await test("the_sheet_shows_every_component", async () => {
    // Drawn on v1, ui/ as it is (the runner's v2 carries its test edits to the boards); v2 put back after.
    const was = live();
    sy("ui", "rollback", "--to", v1.version);
    await eventually("v1 is live", async () => live() === v1.version);
    const { page, context, seen } = await sheetPage();
    try {
      for (const s of ["light", "dark"]) {
        const col = scheme(page, s);
        assert((await col.getAttribute("data-plexus-theme")) === s, `the ${s} scheme is the theme's wrapper, data-plexus-theme=${s}`);
        const missing = [];
        for (const n of names) {
          if ((await col.locator(`[data-sheet-component="${n}"] [data-sheet-state=plain] [data-component="${n}"]`).count()) < 1) missing.push(n);
        }
        assert(missing.length === 0, `${s}: no plain ${missing.join(", ")} on the sheet`);
      }
      // Every mark filled: the host drew each component (none failed, none left bare).
      await eventually("every mark on the sheet is filled", async () => {
        const bare = await page.locator("[data-surface=sheet-page] [data-sheet-state] > .frame > [data-component]").evaluateAll((els) =>
          els.filter((e) => e.hasAttribute("data-failed") || (e.childElementCount === 0 && !e.hasAttribute("data-sheet-quiet"))).map((e) => `${e.closest("[data-sheet-scheme]").dataset.sheetScheme}/${e.dataset.component}/${e.closest("[data-sheet-state]").dataset.sheetState}`));
        if (bare.length) throw new Error(`bare or failed: ${bare.slice(0, 20).join(", ")}${bare.length > 20 ? ` (+${bare.length - 20})` : ""}`);
        return true;
      }, 60_000);
      const id = await page.locator("[data-surface=sheet-page]").getAttribute("data-sheet-id");
      assert(id === sheetId, `the sheet draws the route's ticket: ${id}`);
      const failed = seen.console.filter((l) => /pageerror|cycle:/.test(l));
      assert(failed.length === 0, `the sheet draws without a page error or a cycle:\n${failed.slice(0, 10).join("\n")}`);

      assert((await page.locator(".board-newer").count()) === 0, "the sheet is drawn from v1, not the runner's v2");
      await sectionsFilled(page);

      // The screenshots: each scheme whole, every section filled.
      await page.setViewportSize({ width: 2400, height: 1200 });
      await sectionsFilled(page);
      await page.waitForTimeout(1000);
      for (const s of ["light", "dark"]) {
        const file = path.join(work, `sheet-${s}.png`);
        await scheme(page, s).screenshot({ path: file, animations: "disabled" });
        assert(fs.statSync(file).size > 10_000, `${file} holds the sheet`);
        console.log(`     ${file}`);
      }
    } finally {
      await context.close();
      sy("ui", "rollback", "--to", was);
      await eventually("the version the runner left is live again", async () => live() === was);
    }
  });

  await test("every_state_alias_draws", async () => {
    const { page, context } = await sheetPage();
    try {
      // Every held state is an element the sheet marked with its alias.
      await eventually("every held state is marked", async () => {
        const unheld = await page.locator("[data-surface=sheet-page] [data-sheet-state]").evaluateAll((tiles) =>
          tiles.filter((t) => ["hover", "focus", "selected", "checked", "disabled"].includes(t.dataset.sheetState) && !t.querySelector(`[data-sheet~="${t.dataset.sheetState}"]`))
               .map((t) => `${t.closest("[data-sheet-scheme]").dataset.sheetScheme}/${t.closest("[data-sheet-component]").dataset.sheetComponent}/${t.dataset.sheetState}`));
        if (unheld.length) throw new Error(`no element holds: ${unheld.join(", ")}`);
        return true;
      }, 60_000);
      // A selected row and a held hover paint otherwise than the plain row, in both schemes.
      const bg = (loc) => loc.evaluate((e) => getComputedStyle(e).backgroundColor);
      const seenBg = {};
      for (const s of ["light", "dark"]) {
        for (const state of ["selected", "hover"]) {
          const tile = scheme(page, s).locator(`[data-sheet-component=board-rows-sv] [data-sheet-state=${state}]`);
          const held = tile.locator(`.board-row[data-sheet~=${state}]`).first();
          const plain = tile.locator(".board-row[data-id]:not([data-sheet])").first();
          await eventually(`${s}: board-rows-sv's ${state} tile draws rows`, async () => (await held.count()) === 1 && (await plain.count()) === 1);
          const [a, b] = [await bg(held), await bg(plain)];
          assert(a !== b, `${s}: the ${state} row's background (${a}) differs from the plain row's (${b})`);
          seenBg[`${s}/${state}`] = a;
        }
      }
      assert(seenBg["light/selected"] !== seenBg["dark/selected"], `the selected row is the scheme's own: ${JSON.stringify(seenBg)}`);
      for (const s of ["light", "dark"]) assert(seenBg[`${s}/hover`] !== seenBg[`${s}/selected`], `${s}: a hovered row and a selected one differ: ${JSON.stringify(seenBg)}`);
      // A disabled control is dimmed: bulk-bar's held button in the disabled ink, its plain twin not.
      for (const s of ["light", "dark"]) {
        const col = scheme(page, s);
        const held = col.locator("[data-sheet-component=bulk-bar] [data-sheet-state=disabled] [data-action=dry-run][data-sheet~=disabled]").first();
        const plain = col.locator("[data-sheet-component=bulk-bar] [data-sheet-state=plain] [data-action=dry-run]:not([disabled])").first();
        await eventually(`${s}: bulk-bar's disabled tile holds a button`, async () => (await held.count()) === 1 && (await plain.count()) === 1);
        const [ink, a, b] = await held.evaluate((e, p) => {
          const probe = document.createElement("i");
          probe.style.color = "var(--disabled)";
          e.closest("[data-sheet-scheme]").append(probe);
          const ink = getComputedStyle(probe).color;
          probe.remove();
          return [ink, getComputedStyle(e).color, p];
        }, await plain.evaluate((e) => getComputedStyle(e).color));
        assert(a === ink && a !== b, `${s}: the disabled button is in --disabled (${ink}): ${a}, the plain one ${b}`);
      }
    } finally {
      await context.close();
    }
  });

  // SURFACE-16: ticket-view, drawn from its props alone, in each state it has.
  await test("the_sheet_holds_ticket_view", async () => {
    const { page, context } = await sheetPage();
    try {
      for (const s of ["light", "dark"]) {
        const tile = (state) => scheme(page, s).locator(`[data-sheet-component=ticket-view] [data-sheet-state=${state}] [data-component=ticket-view]`);
        await eventually(`${s}: the plain view`, async () => (await tile("plain").locator("[data-surface=ticket-view] [data-ticket-body] h2").count()) === 1, 30_000);
        assert((await tile("plain").locator("[data-field=status]").getAttribute("data-status")) === "Ready", `${s}: the status, from its prop`);
        await eventually(`${s}: the empty view`, async () => (await tile("empty").locator("[data-surface=ticket-view] [data-field=body][data-empty]").count()) === 1);
      }
    } finally {
      await context.close();
    }
  });

  await test("no_literal_colour_in_any_sheet", async () => {
    const all = sheets();
    assert(all.length > 20, `the bundle's sheets: ${all.length}`);
    const found = all.flatMap(([f, css]) => literals(css).map((l) => `${f}: ${l}`));
    assert(found.length === 0, `a literal colour outside theme.css:\n  ${found.join("\n  ")}`);
    assert(tar("-tf", v2.tar).split("\n").includes("theme.css"), "the bundle carries theme.css");
  });

  await test("every_token_resolves", async () => {
    const theme = tar("-xOf", v2.tar, "theme.css");
    const defined = new Set([...theme.matchAll(/(--[a-z0-9-]+)\s*:/g)].map((m) => m[1]));
    const missing = [];
    const used = new Set();
    for (const [f, css] of sheets()) {
      const own = new Set([...css.matchAll(/(--[a-z0-9-]+)\s*:/g)].map((m) => m[1]));
      for (const m of css.matchAll(/var\((--[a-z0-9-]+)/g)) {
        used.add(m[1]);
        if (!defined.has(m[1]) && !own.has(m[1])) missing.push(`${f}: ${m[1]}`);
      }
    }
    assert(missing.length === 0, `a token the theme lacks:\n  ${[...new Set(missing)].join("\n  ")}`);
    // In the page, each scheme gives every token a value — on #/sheet alone, drawn for the first ticket the store lists.
    const { page, context } = await sheetPage("#/sheet");
    assert(/^[A-Z][A-Z0-9]*-\d+/.test((await page.locator("[data-surface=sheet-page]").getAttribute("data-sheet-id")) ?? ""), "#/sheet draws the first ticket listed");
    try {
      const empty = await page.evaluate((tokens) => {
        const out = [];
        for (const el of document.querySelectorAll("[data-sheet-scheme]")) {
          const cs = getComputedStyle(el);
          for (const t of tokens) if (!cs.getPropertyValue(t).trim()) out.push(`${el.dataset.sheetScheme}: ${t}`);
        }
        return out;
      }, [...used]);
      assert(empty.length === 0, `a token with no value on the sheet:\n  ${empty.join("\n  ")}`);
    } finally {
      await context.close();
    }
  });
};
