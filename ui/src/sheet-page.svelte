<!-- SURFACE-10: the sheet — every component of ui/src, in every state it
     has, in both schemes, at #/sheet (svelte/1). Each is placed by its bound
     mark (FRAME-29: `plexus-ui:<name>`, whatever its profile), with props
     from the seeded tickets: the ticket is `#/sheet/:id`'s, else the first
     `tickets.list { limit: 1 }` answers. Two columns, the theme's wrappers
     `data-plexus-theme="light"` and `"dark"`, side by side; in each, a
     component under a heading naming it, its states as tiles:
       plain     — as it is;
       hover, focus, selected, checked, disabled — held without the pointer:
                   the sheet marks the tile's first row and first control
                   (never a bound mark, `[data-slot]`; a component that holds
                   another element names it, HOLDS) `data-sheet="<state>"`,
                   and each component's sheet draws
                   `[data-sheet~=<state>]` as its `:hover`, `:focus-visible`,
                   selected, pressed or `:disabled` (APP-114's aliases);
       loading   — the mark's `data-sheet="loading"` (the prop `sheet`),
                   which the component draws as its skeleton;
       empty, refused — real answers: a filter no ticket matches, a filter
                   the language refuses, a ticket that is not there.
     A component with no state shows as it is. The layouts and the pages
     are drawn small. A store with no ticket the reader can see says so. -->
<script module>
  export const uses = ["tickets.list"];
</script>

<script>
  import { live } from "plexus-ui/host";
  import AppTop from "plexus-ui:app-top";
  import BoardFrame from "plexus-ui:board-frame";
  import BoardLayout from "plexus-ui:board-layout";
  import BoardLayoutSv from "plexus-ui:board-layout-sv";
  import BoardRows from "plexus-ui:board-rows";
  import BoardRowsSv from "plexus-ui:board-rows-sv";
  import BulkBar from "plexus-ui:bulk-bar";
  import FilterLine from "plexus-ui:filter-line";
  import FilterLineSv from "plexus-ui:filter-line-sv";
  import FiltersSaved from "plexus-ui:filters-saved";
  import HelloList from "plexus-ui:hello-list";
  import JumpPalette from "plexus-ui:jump-palette";
  import JumpPaletteSv from "plexus-ui:jump-palette-sv";
  import KeysHelp from "plexus-ui:keys-help";
  import NavChildren from "plexus-ui:nav-children";
  import NavChildrenSv from "plexus-ui:nav-children-sv";
  import NavCounts from "plexus-ui:nav-counts";
  import NavFacets from "plexus-ui:nav-facets";
  import NavFlat from "plexus-ui:nav-flat";
  import NavFlatSv from "plexus-ui:nav-flat-sv";
  import NavTree from "plexus-ui:nav-tree";
  import NavTreeSv from "plexus-ui:nav-tree-sv";
  import OrgsList from "plexus-ui:orgs-list";
  import PageActions from "plexus-ui:page-actions";
  import PageBlockers from "plexus-ui:page-blockers";
  import PageBulk from "plexus-ui:page-bulk";
  import PageCounts from "plexus-ui:page-counts";
  import PageEdit from "plexus-ui:page-edit";
  import PageFacets from "plexus-ui:page-facets";
  import PageFilters from "plexus-ui:page-filters";
  import PageNew from "plexus-ui:page-new";
  import PageOrgs from "plexus-ui:page-orgs";
  // V2-31: the quota pages.
  import PageQuota from "plexus-ui:page-quota";
  import PageUsage from "plexus-ui:page-usage";
  import QuotaTable from "plexus-ui:quota-table";
  import QuotaUsage from "plexus-ui:quota-usage";
  import PageReady from "plexus-ui:page-ready";
  import PageReleases from "plexus-ui:page-releases";
  import PageSearch from "plexus-ui:page-search";
  import PageShell from "plexus-ui:page-shell";
  import PageTicket from "plexus-ui:page-ticket";
  import PageWaves from "plexus-ui:page-waves";
  import SearchPage from "plexus-ui:search-page";
  import TicketActions from "plexus-ui:ticket-actions";
  import TicketBody from "plexus-ui:ticket-body";
  import TicketView from "plexus-ui:ticket-view";
  import MermaidDiagram from "plexus-ui:mermaid-diagram";
  import CodeCard from "plexus-ui:code-card";
  import CodePaint from "plexus-ui:code-paint";
  import CodeRust from "plexus-ui:code-rust";
  import CodeJavascript from "plexus-ui:code-javascript";
  import CodePython from "plexus-ui:code-python";
  import CodeJson from "plexus-ui:code-json";
  import CodeYaml from "plexus-ui:code-yaml";
  import CodeGo from "plexus-ui:code-go";
  import CodeHtml from "plexus-ui:code-html";
  import CodeCss from "plexus-ui:code-css";
  import CodeLegacy from "plexus-ui:code-legacy";
  import TicketChip from "plexus-ui:ticket-chip";
  import TicketChipSv from "plexus-ui:ticket-chip-sv";
  import TicketCommits from "plexus-ui:ticket-commits";
  import TicketCreate from "plexus-ui:ticket-create";
  import TicketEdit from "plexus-ui:ticket-edit";
  import EditorBody from "plexus-ui:editor-body";
  import DocMargin from "plexus-ui:doc-margin";
  import MediaDraw from "plexus-ui:media-draw";
  import MediaIngest from "plexus-ui:media-ingest";
  import EditorScreen from "plexus-ui:editor-screen";
  import PageEditor from "plexus-ui:page-editor";
  import TicketHead from "plexus-ui:ticket-head";
  import TicketHistory from "plexus-ui:ticket-history";
  import TicketPage from "plexus-ui:ticket-page";
  import TicketPicker from "plexus-ui:ticket-picker";
  import TicketRelations from "plexus-ui:ticket-relations";
  import TicketRevisions from "plexus-ui:ticket-revisions";
  import TicketQuestions from "plexus-ui:ticket-questions";
  import QueuePage from "plexus-ui:queue-page";
  import PageQueue from "plexus-ui:page-queue";
  import TicketSubtasks from "plexus-ui:ticket-subtasks";
  import UiVersions from "plexus-ui:ui-versions";
  import WorkBlocker from "plexus-ui:work-blocker";
  import WorkBlockers from "plexus-ui:work-blockers";
  import WorkReady from "plexus-ui:work-ready";
  import WorkReadyRow from "plexus-ui:work-ready-row";
  import WorkWaves from "plexus-ui:work-waves";

  let { id = null } = $props();

  // The ticket the parts are drawn for: the route's, else the first the store lists.
  const first = live({ snapshot: "tickets.list", params: { limit: 1, summary: true, fields: ["id"] }, key: "id" });
  const tid = $derived(id ?? first.rows.find((r) => r.id)?.id ?? null);

  const EMPTY = "status Ready and status Draft"; // a filter no ticket answers
  const REFUSED = "status Nope"; // a filter the language refuses
  const NONE = "ZZZZ-999999"; // a ticket that is not there

  // The held states, and what of a tile the sheet marks for each: the first
  // of every kind of row and control in it.
  const HOLD = {
    hover: ["[data-id]", "a[href]", "button:not([disabled])", "input", "select", "[role=option]", "[role=button]", "li", "summary", "label"],
    focus: ["[data-id]", "a[href]", "button:not([disabled])", "input", "select", "textarea", "[role=option]", "[tabindex]", "summary"],
    selected: ["[data-id]", "[role=option]", "li", "a[href]", "[aria-pressed]"],
    checked: ["[data-id]", "[aria-pressed]", "[aria-checked]", "input[type=checkbox]", "[role=radio]", "button"],
    disabled: ["button", "input", "select", "textarea"],
  };

  // Each component: its group, its props for a ticket, the states it draws
  // (its sheet's held ones — tests/apart.rs holds each to its alias — then
  // what props or the loading mark give), and whether it is drawn small.
  const F = (t) => ({});
  const ID = (t) => ({ id: t });
  const fil = { empty: { filter: EMPTY }, refused: { filter: REFUSED } };
  const gone = { empty: { id: NONE } };
  // The palettes' held rows: the second hit, hovered or selected — never
  // the first, which the palette selects itself.
  const HITS = { hover: ["li:nth-child(2) > .palette-hit"], focus: [".palette-input"], selected: ["li:nth-child(2) > .palette-hit"] };
  // V2-20: every kind of question in every state, and a queue with every
  // urgency and a snooze: the sheet's own rows, so a store that asks nothing
  // still shows them.
  const Q = (id, kind, state, more = {}) => ({ id, kind, state, question: `A ${kind} question, ${state}?`, options: [], answer: null, history: [], ...more });
  const OPTIONS = [{ value: "a", label: "Archive", recommended: true }, { value: "b", label: "Delete", recommended: false }];
  const QUESTIONS = (t) => [
    Q("c-open", "choice", "open", { ticket: t, options: OPTIONS, recommendation: "Archive: holds are real." }),
    Q("c-said", "choice", "answered", { ticket: t, options: OPTIONS, answer: { value: { type: "choice", option: "a" }, rationale: "holds", name: "the owner" } }),
    Q("m-open", "multi", "open", { ticket: t, options: OPTIONS }),
    Q("m-said", "multi", "answered", { ticket: t, options: OPTIONS, answer: { value: { type: "multi", options: ["b"] } } }),
    Q("t-open", "text", "open", { ticket: t }),
    Q("t-said", "text", "answered", { ticket: t, answer: { value: { type: "text", text: "ship it" } } }),
    Q("closed", "choice", "closed", { ticket: t, options: OPTIONS, answer: { value: { type: "choice", option: "b" } } }),
    Q("gone", "text", "withdrawn", { ticket: t }),
  ];
  const AT = 1_800_000_000;
  const ROW = (id, urgency, reason, more = {}) => ({ id, ticket: { id, title: `the ticket ${id}` }, entry: { ticket: id, urgency, reason, queued_unix: AT - 7200, ...more } });
  const QUEUED = [
    ROW("SHEET-1", "blocking", "idle until it is answered", { input: "landing" }),
    ROW("SHEET-2", "now", "something waits", { inputs: ["a", "b"] }),
    ROW("SHEET-3", "today", "before the day is out"),
    ROW("SHEET-4", "today", "asleep for an hour", { snoozed_until_unix: AT + 3600 }),
    ROW("SHEET-5", "whenever", "when there is time", { queued_unix: AT }),
  ];
  const HOLD_Q = { hover: [".tq-opt"], focus: [".tq-opt"], selected: [".tq-opt"], checked: [".tq-opt"] };
  const HOLD_ROW = { hover: [".q-row"], focus: [".q-id"], selected: [".q-row"] };
  const SHEET = [
    ["the board", [
      ["board-rows-sv", BoardRowsSv, F, ["hover", "focus", "selected", "checked"], fil],
      ["board-rows", BoardRows, F, ["hover", "focus"], fil],
      ["app-top", AppTop, (t) => ({ here: "board" }), ["hover", "focus"], {}],
      ["board-layout-sv", BoardLayoutSv, F, ["hover", "focus"], { refused: { filter: REFUSED } }],
      ["filter-line-sv", FilterLineSv, (t) => ({ filter: "status Ready" }), ["hover", "focus"], { refused: { filter: REFUSED } }],
      ["filter-line", FilterLine, (t) => ({ filter: "status Ready" }), ["hover", "focus"], { refused: { filter: REFUSED } }],
      ["bulk-bar", BulkBar, (t) => ({ ids: t }), ["hover", "focus", "disabled"], { loading: { sheet: "loading" } }, false, { disabled: ["[data-action=dry-run]"] }],
      ["keys-help", KeysHelp, F, [], {}],
      ["ticket-chip-sv", TicketChipSv, ID, ["hover", "focus"], gone],
      ["ticket-chip", TicketChip, ID, ["hover", "focus"], gone],
    ]],
    ["the nav", [
      ["nav-tree-sv", NavTreeSv, F, ["hover", "focus", "checked"], fil, false, { checked: ["[data-nav-mode=flat]"] }],
      ["nav-tree", NavTree, F, ["hover", "focus", "checked"], fil, false, { checked: ["[data-nav-mode=flat]"] }],
      ["nav-flat-sv", NavFlatSv, F, ["hover", "focus"], fil],
      ["nav-flat", NavFlat, F, ["hover", "focus"], {}],
      ["nav-children-sv", NavChildrenSv, (t) => ({ parent: t }), ["hover", "focus"], { empty: { parent: NONE } }],
      ["nav-children", NavChildren, (t) => ({ parent: t }), ["hover", "focus"], { empty: { parent: NONE } }],
      ["nav-facets", NavFacets, (t) => ({ filter: "status Ready" }), ["hover", "focus", "selected"], fil],
      ["nav-counts", NavCounts, (t) => ({ filter: "status Ready", label: "ready" }), ["hover", "focus"], fil],
      ["filters-saved", FiltersSaved, F, ["hover", "focus"], {}],
    ]],
    ["the palette and the search", [
      ["jump-palette-sv", JumpPaletteSv, (t) => ({ sheet: code(t) }), ["hover", "focus", "selected"], { empty: { sheet: "zzqqxxjj" } }, false, HITS],
      ["jump-palette", JumpPalette, (t) => ({ sheet: code(t) }), ["hover", "focus"], { empty: { sheet: "zzqqxxjj" } }, false, HITS],
      // STUDIO-12: its jump button is app-top's now — disabled is the pager's prev, on a query the bodies answer.
      ["search-page", SearchPage, (t) => ({ q: t }), ["hover", "focus", "disabled", "selected"], { disabled: { q: "ready" }, loading: { q: t0(), sheet: "loading" }, empty: { q: "zzqqxxjj" }, refused: { q: "ready", filter: REFUSED } }, true, { disabled: ["[data-action=prev]"] }],
    ]],
    ["the ticket", [
      ["ticket-page", TicketPage, ID, ["hover", "focus"], gone, true],
      ["ticket-head", TicketHead, ID, ["hover", "focus"], gone],
      ["ticket-body", TicketBody, ID, [], gone],
      ["ticket-view", TicketView, (t) => ({ title: t, status: "Ready", body: "## A ticket\n\nDrawn from its **props**, no call.\n\n- a list\n- of two\n\n<m-summary>\n\n<m-field data-field=\"standing\">**A house card**, drawn as the editor draws it.</m-field>\n\n<m-stat value=\"2\" tone=\"pass\">blocks</m-stat>\n\n</m-summary>\n" }), [], { empty: { body: "" } }],
      // EDITOR-9: a mermaid fence drawn by Pele, and one it refuses.
      ["mermaid-diagram", MermaidDiagram, (t) => ({ source: "flowchart LR\n  A[a click] --> B[the caret]" }), [], { refused: { source: "flowchart TD\n  A[open --> \n" } }],
      // EDITOR-9: code — the card every fence draws as, its painter, and a painter per language.
      ["code-card", CodeCard, (t) => ({ code: "fn main() {\n    println!(\"{}\", 42);\n}", info: "rust", path: "src/main.rs", why: "the entry" }), ["hover", "focus"], {}],
      ["code-paint", CodePaint, (t) => ({ code: "plain text, no language" }), [], {}],
      ["code-rust", CodeRust, (t) => ({ code: "fn main() { let x: u32 = 1; }" }), [], {}],
      ["code-javascript", CodeJavascript, (t) => ({ code: "const n = 1; // a comment", lang: "typescript" }), [], {}],
      ["code-python", CodePython, (t) => ({ code: "def f(x):\n    return x + 1  # one more" }), [], {}],
      ["code-json", CodeJson, (t) => ({ code: "{ \"id\": 1, \"ok\": true }" }), [], {}],
      ["code-yaml", CodeYaml, (t) => ({ code: "name: menger\nports: [4455, 4456]" }), [], {}],
      ["code-go", CodeGo, (t) => ({ code: "func main() { fmt.Println(1) }" }), [], {}],
      ["code-html", CodeHtml, (t) => ({ code: "<p class=\"x\">hi</p>" }), [], {}],
      ["code-css", CodeCss, (t) => ({ code: ".x { color: red; }" }), [], {}],
      ["code-legacy", CodeLegacy, (t) => ({ code: "cargo test -p menger --test apart  # a selection", lang: "shell" }), [], {}],
      ["ticket-relations", TicketRelations, ID, ["hover", "focus"], { loading: { sheet: "loading" } }],
      ["ticket-subtasks", TicketSubtasks, ID, ["hover", "focus"], { loading: { sheet: "loading" } }],
      ["ticket-commits", TicketCommits, ID, ["hover", "focus"], {}],
      ["ticket-history", TicketHistory, ID, ["hover"], { loading: { sheet: "loading" }, ...gone }],
      ["ticket-revisions", TicketRevisions, ID, ["hover", "focus", "selected"], { loading: { sheet: "loading" }, ...gone }],
      ["ticket-edit", TicketEdit, ID, ["hover", "focus", "disabled", "checked"], { checked: { sheet: "open" }, loading: { sheet: "loading" } }, false, { checked: ["[role=radio][aria-checked=false]"] }],
      // V2-30: a doc's margin, alone — its threads as a list (a ticket's id: none).
      ["doc-margin", DocMargin, ID, [], {}],
      // EDITOR-8: the rendering as the editor; hover and focus held on its revisions link (EDITOR-9: a block has no highlight).
      ["editor-body", EditorBody, ID, ["hover", "focus"], { loading: { sheet: "loading" }, ...gone }, false, { hover: [".revs"], focus: [".revs"] }],
      // EDITOR-10: what a doc absorbed, drawn — a file that is none here stays its link; the ingest draws nothing of its own.
      ["media-draw", MediaDraw, () => ({ body: `[deck.pdf](media:${"0".repeat(64)})\n\nA doc absorbs anything from outside.` }), [], {}],
      ["media-ingest", MediaIngest, F, [], {}],
      // EDITOR-8: the writing screen, small; selected held on a document of the list, disabled on *new*.
      ["editor-screen", EditorScreen, ID, ["hover", "focus", "selected", "disabled"], { loading: { sheet: "loading" }, empty: { id: NONE } }, true, { hover: [".doc"], focus: [".id"], selected: [".doc"], disabled: ["[data-action=new]"] }],
      ["ticket-actions", TicketActions, ID, ["hover", "focus", "disabled", "checked"], { loading: { sheet: "loading" } }],
      ["work-blockers", WorkBlockers, ID, [], { loading: { sheet: "loading" } }],
      ["work-blocker", WorkBlocker, (t) => ({ id: t, side: "blocker" }), ["hover", "focus"], { loading: { sheet: "loading" } }],
    ]],
    ["the writes", [
      ["ticket-create", TicketCreate, (t) => ({ parent: t }), ["hover", "focus", "disabled", "checked"], { loading: { sheet: "loading" } }, true, { checked: [".tab"], disabled: [".go"] }],
      ["ticket-picker", TicketPicker, (t) => ({ name: "sheet", label: "a ticket" }), ["hover", "focus"], {}],
    ]],
    ["the queue", [
      ["ticket-questions", TicketQuestions, (t) => ({ id: t, sample: JSON.stringify(QUESTIONS(t)) }), ["hover", "focus", "selected", "checked"], { loading: { sheet: "loading", sample: null }, empty: { id: NONE, sample: null } }, false, HOLD_Q],
      ["queue-page", QueuePage, () => ({ sample: JSON.stringify(QUEUED) }), ["hover", "focus", "selected"], { loading: { sheet: "loading", sample: "[]" }, empty: { sample: "[]" } }, true, HOLD_ROW],
    ]],
    ["the order of work", [
      ["work-ready", WorkReady, F, ["hover", "focus"], { loading: { sheet: "loading" }, ...fil }],
      ["work-ready-row", WorkReadyRow, ID, ["hover", "focus"], {}],
      ["work-waves", WorkWaves, ID, ["hover", "focus", "checked"], { loading: { id: t0(), sheet: "loading" } }, true],
    ]],
    ["the admin pages", [
      ["orgs-list", OrgsList, F, ["hover"], { loading: { sheet: "loading" } }],
      ["quota-usage", QuotaUsage, F, [], { loading: { sheet: "loading" } }],
      ["quota-table", QuotaTable, F, ["hover", "focus", "disabled"], { loading: { sheet: "loading" } }],
      ["ui-versions", UiVersions, F, ["hover", "focus", "disabled"], { loading: { sheet: "loading" } }, true],
    ]],
    // STUDIO-12: the pages — each route's rhx page, placing its components by their marks (#/sheet's own, page-sheet, aside: it places this).
    ["the pages", [
      ["page-ticket", PageTicket, ID, [], { loading: { sheet: "loading" }, ...gone }, true],
      ["page-queue", PageQueue, F, [], {}, true],
      ["page-new", PageNew, (t) => ({ parent: t }), [], {}, true],
      ["page-search", PageSearch, (t) => ({ q: t }), [], { empty: { q: "zzqqxxjj" } }, true],
      ["page-ready", PageReady, F, [], fil, true],
      ["page-waves", PageWaves, ID, [], {}, true],
      ["page-filters", PageFilters, F, [], {}, true],
      ["page-orgs", PageOrgs, F, [], {}, true],
      ["page-usage", PageUsage, F, [], {}, true],
      ["page-quota", PageQuota, F, [], {}, true],
      ["page-releases", PageReleases, F, [], {}, true],
      ["page-bulk", PageBulk, (t) => ({ ids: t }), [], {}, true],
      ["page-blockers", PageBlockers, ID, [], {}, true],
      ["page-edit", PageEdit, ID, [], {}, true],
      ["page-editor", PageEditor, ID, [], {}, true],
      ["page-actions", PageActions, ID, [], {}, true],
      ["page-facets", PageFacets, (t) => ({ filter: "status Ready" }), [], fil, true],
      ["page-counts", PageCounts, (t) => ({ filter: "status Ready" }), [], fil, true],
    ]],
    ["the layouts", [
      ["board-frame", BoardFrame, F, [], {}, true],
      ["board-layout", BoardLayout, F, ["hover", "focus"], {}, true],
      ["page-shell", PageShell, F, [], {}, true],
      ["hello-list", HelloList, F, [], {}],
    ]],
  ];
  // A ticket's shortcode: a query the palettes answer with many hits.
  function code(t) { return t.replace(/-[0-9.]+$/, ""); }
  // What a component holds for a state, where HOLD's first-of-each is not it.
  const HOLDS = Object.fromEntries(SHEET.flatMap(([, entries]) => entries.filter((e) => e[6]).map((e) => [e[0], e[6]])));
  // A placeholder the ticket fills in, read when the tiles are drawn.
  function t0() { return "@ticket"; }
  const fill = (p, t) => Object.fromEntries(Object.entries(p).map(([k, v]) => [k, v === "@ticket" ? t : v]));

  /** The tiles of one component for ticket `t`. */
  function tiles(entry, t) {
    const [name, , base, held, given] = entry;
    const props = base(t);
    const out = [{ state: "plain", props, hold: null }];
    for (const s of held) out.push({ state: s, props: given[s] ? { ...props, ...fill(given[s], t) } : props, hold: s });
    for (const s of ["loading", "empty", "refused"]) if (given[s]) out.push({ state: s, props: { ...props, ...fill(given[s], t) }, hold: null });
    return out;
  }

  // The held states: the first row and control of each held tile marked
  // data-sheet="<state>", again whenever a component draws anew.
  let root = $state(null);
  function hold() {
    for (const frame of root.querySelectorAll("[data-sheet-hold]")) {
      const state = frame.dataset.sheetHold;
      const name = frame.closest("[data-sheet-component]")?.dataset.sheetComponent;
      // Never a bound mark ([data-slot]): the alias belongs on a row or a control inside.
      for (const sel of HOLDS[name]?.[state] ?? HOLD[state]) {
        if (frame.querySelector(`:is(${sel}):not([data-slot])[data-sheet~="${state}"]`)) continue;
        const el = frame.querySelector(`:is(${sel}):not([data-slot])`);
        if (el) el.setAttribute("data-sheet", state);
      }
    }
  }
  $effect(() => {
    if (!root) return;
    hold();
    const seen = new MutationObserver(hold);
    seen.observe(root, { childList: true, subtree: true });
    return () => seen.disconnect();
  });
</script>

<main class="sheet" data-surface="sheet-page" data-sheet-id={tid} bind:this={root}>
  <header class="sheet-head">
    <h1>the sheet</h1>
    <p>Every component of the bundle, in every state it has, in both schemes{tid ? ` — drawn for ${tid}` : ""}.</p>
  </header>
  {#if tid}
    <div class="schemes">
      {#each ["light", "dark"] as scheme (scheme)}
        <div class="scheme" data-plexus-theme={scheme} data-sheet-scheme={scheme}>
          <h2 class="scheme-name">{scheme}</h2>
          {#each SHEET as [group, entries] (group)}
            <h3 class="group">{group}</h3>
            {#each entries as entry (entry[0])}
              {@const C = entry[1]}
              <section class="comp" data-sheet-component={entry[0]} data-small={entry[5] ? "true" : "false"}>
                <h4 class="comp-name">{entry[0]}</h4>
                <div class="tiles">
                  {#each tiles(entry, tid) as tile (tile.state)}
                    <figure class="tile" data-sheet-state={tile.state}>
                      <figcaption>{tile.state}</figcaption>
                      <div class="frame" data-sheet-hold={tile.hold}>
                        <C {...tile.props}/>
                      </div>
                    </figure>
                  {/each}
                </div>
              </section>
            {/each}
          {/each}
        </div>
      {/each}
    </div>
  {:else if first.status === "live"}
    <p class="waiting" data-empty>No ticket to draw the sheet for: the store holds none you can read. Create one, or open #/sheet/ID.</p>
  {:else}
    <p class="waiting" data-loading>asking the store for a ticket…</p>
  {/if}
</main>

<style>
  .sheet { min-height: 100vh; padding: 16px; background: var(--bg); color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .sheet-head h1 { margin: 0; font-family: var(--font-display); font-size: 20px; font-weight: var(--weight-strong); }
  .sheet-head p { margin: 4px 0 12px; color: var(--muted); }
  .schemes { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); border: var(--border) solid var(--line); border-radius: var(--radius-lg); overflow: hidden; }
  .scheme { min-width: 0; padding: 12px 14px 20px; background: var(--bg); color: var(--ink); }
  .scheme + .scheme { border-left: var(--border) solid var(--line); }
  .scheme-name { margin: 0 0 8px; font-size: 15px; font-weight: var(--weight-bold); }
  .group { margin: 18px 0 4px; font-size: 11px; font-weight: var(--weight-medium); letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
  .comp { padding: 8px 0 10px; border-top: var(--border) solid var(--line); }
  .comp-name { margin: 0 0 6px; font-size: 12px; font-weight: var(--weight-strong); color: var(--accent); }
  .tiles { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 8px; }
  .tile { margin: 0; min-width: 0; overflow: hidden; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .tile figcaption { padding: 2px 8px; font-size: 10px; color: var(--muted); background: var(--card); border-bottom: var(--border) solid var(--line); }
  .tile[data-sheet-state=loading] figcaption, .tile[data-sheet-state=empty] figcaption { color: var(--ink); }
  .tile[data-sheet-state=refused] figcaption { color: var(--warn); }
  .frame { position: relative; height: 200px; overflow: hidden; contain: layout paint; background: var(--bg); }
  .frame > :global([data-component]) { display: block; height: 100%; zoom: .85; }
  .comp[data-small=true] .frame { height: 260px; }
  .comp[data-small=true] .frame > :global([data-component]) { height: 200%; zoom: .5; }
  .waiting { color: var(--muted); }
  @media (max-width: 1100px) {
    .schemes { grid-template-columns: minmax(0, 1fr); }
    .scheme + .scheme { border-left: 0; border-top: var(--border) solid var(--line); }
  }
</style>
