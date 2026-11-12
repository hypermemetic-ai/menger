// `<spec-outline></spec-outline>` in a ticket's body draws that body's outline, read as an OpenSpec change
// (spec-outline.svelte): the counts, the sections as links, each group of requirements a line that opens, a
// requirement with no scenario marked, the open questions; a click scrolls the body to the heading; an amend
// redraws it. Run alone by `node tests/pages_browser.mjs --group spec`.
//
// Marks: `[data-surface=spec-outline]`, `[data-stat=requirements|scenarios|tasks|questions]`, `[data-section]`,
// `[data-group=<name>]` (`[aria-expanded]`), `[data-req=<name>]` (`[data-bare]`), `[data-question]`.

const EPIC = "SPEC";

export default async (ctx) => {
  const { test, assert: ok, eventually, openPage, sy, fs, path, session } = ctx;
  const same = (got, want, what = "") => ok(JSON.stringify(got) === JSON.stringify(want), `${what || "got"} ${JSON.stringify(got)}, wanted ${JSON.stringify(want)}`);
  const assert = { equal: same, deepEqual: same, match: (got, re) => ok(re.test(got), `${JSON.stringify(got)} does not match ${re}`) };
  const bodyFile = (name, text) => { const f = path.join(path.dirname(session), `spec-${name}.md`); fs.writeFileSync(f, text); return `@${f}`; };
  const createdId = (out, what) => {
    const c = out.find((e) => e.type === "created" || e.ticket?.id);
    if (!c) throw new Error(`create ${what}: ${JSON.stringify(out)}`);
    return c.ticket.id;
  };
  const filler = Array.from({ length: 40 }, (_, i) => `Line ${i + 1} of the reasons.`).join("\n\n");
  const BODY = (tasks) => `<spec-outline></spec-outline>

# The kiln

## Why

${filler}

## Requirements

### firing

#### Requirement: A firing can be paused

The kiln SHALL hold its temperature when a firing is paused.

##### Scenario: paused at the hold
- **WHEN** a firing is paused at 1040
- **THEN** it holds 1040

##### Scenario: resumed
- **WHEN** it is resumed
- **THEN** the schedule carries on

#### Requirement: A firing is logged

Every firing SHALL be logged.

### glazes

#### Requirement: A glaze names its cone

A glaze SHALL name the cone it matures at.

##### Scenario: cone six
- **WHEN** a glaze is added with cone 6
- **THEN** it is listed under cone 6

## Design

\`\`\`md
#### Requirement: Inside a fence, never counted
- [ ] nor this
\`\`\`

### Open Questions

1. **Gas or electric.** Which kiln first?
2. Who loads the shelves?

## Tasks

${tasks}
`;
  const root = createdId(sy("tickets", "create", "--epic", EPIC, "--kind", "scope", "--title", "the kiln's spec", "--body", bodyFile("a", BODY("- [x] 1. Wire the kiln\n- [ ] 2. Fire it\n- [ ] 3. Unload"))), "root");
  const S = "[data-surface=ticket-body] [data-surface=spec-outline]";
  const open = async () => {
    const opened = await openPage(`#/ticket/${root}`);
    await eventually("the outline is drawn", async () => (await opened.page.locator(S).count()) === 1, 15_000);
    return opened;
  };
  const stat = (page, name) => page.locator(`${S} [data-stat=${name}] b`).innerText();

  await test("spec_outline_counts_what_the_body_holds", async () => {
    const { context, page } = await open();
    try {
      assert.equal(await stat(page, "requirements"), "3");
      assert.equal(await stat(page, "scenarios"), "3");
      assert.equal(await stat(page, "tasks"), "1/3");
      assert.equal(await stat(page, "questions"), "2");
      assert.deepEqual(await page.locator(`${S} [data-section]`).allInnerTexts(), ["Why", "Requirements", "Design", "Tasks"]);
      assert.deepEqual(await page.locator(`${S} [data-group]`).evaluateAll((els) => els.map((e) => e.dataset.group)), ["firing", "glazes"]);
      assert.deepEqual(await page.locator(`${S} [data-question]`).allInnerTexts(), ["Gas or electric", "Who loads the shelves?"]);
    } finally { await context.close(); }
  });

  await test("spec_outline_a_group_opens_and_a_requirement_scrolls_to_its_heading", async () => {
    const { context, page } = await open();
    try {
      const firing = page.locator(`${S} [data-group=firing]`);
      assert.equal(await firing.getAttribute("aria-expanded"), "false");
      assert.match(await firing.innerText(), /1 with no scenario/);
      assert.equal(await page.locator(`${S} [data-req]`).count(), 0);
      await firing.click();
      assert.equal(await firing.getAttribute("aria-expanded"), "true");
      assert.deepEqual(await page.locator(`${S} [data-req]`).evaluateAll((els) => els.map((e) => [e.dataset.req, e.dataset.bare != null])),
        [["A firing can be paused", false], ["A firing is logged", true]]);
      const heading = page.locator("[data-surface=ticket-body] :is(h1, h2, h3, h4, h5, h6)", { hasText: "Requirement: A firing is logged" });
      const inView = () => heading.evaluate((h) => { const r = h.getBoundingClientRect(); return r.top >= 0 && r.bottom <= innerHeight; });
      assert.equal(await inView(), false, "the heading starts below the fold");
      await page.locator(`${S} [data-req="A firing is logged"]`).click();
      await eventually("the heading is scrolled into view", inView, 5_000);
    } finally { await context.close(); }
  });

  await test("spec_outline_follows_an_amend", async () => {
    const { context, page } = await open();
    try {
      sy("tickets", "amend", "--id", root, "--body", bodyFile("b", BODY("- [x] 1. Wire the kiln\n- [x] 2. Fire it\n- [ ] 3. Unload\n- [ ] 4. Sort")));
      await eventually("the tasks count moves", async () => (await stat(page, "tasks")) === "2/4", 10_000);
    } finally { await context.close(); }
  });
};
