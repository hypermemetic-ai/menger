<!-- V2-50 (main's TICKETS-36): **the collision page** — what a name that fits
     several tickets opens. The owner, 2026-10-30: "a collision in a url like
     /<COLLISION-1> should show all the collisions and their parents".

     page-ticket draws it in its place when `get` answers `ambiguous { id,
     candidates, choices }` (TICKETS-35): one row per choice, in the store's
     order — its address (the hosts muted, its id in the accent), kind,
     status, title, its parent (id and title, a link by its uid) and where it
     lives (its epic; its organization too when the choices span several),
     its link by uid and a copy link. A row opens its ticket at its own
     `#/t/<uid>/<ID>-<slug>`. A choice out of the reader's reach is its
     address alone and no link.

     Keys: ↓ ↑ (j k) choose among the rows that open, wrapping; ⏎ opens; `c`
     copies the chosen ticket's link (`<origin>/t/<uid>/…`, the line under the
     rows says what); Esc is the page's own (page-ticket: the board).

     Marks: `[data-surface=ticket-collision][data-asked]`, `[data-heading]`,
     `[data-lede]`, each row `li[data-choice=<i>]` (`aria-selected`,
     `data-id` its key, `[data-out]` out of reach) with `[data-field=hosts|id|
     kind|status|title|parent|place|link]` and `[data-action=copy-link]`, and
     `[data-said]`. -->
<script module>
  export const uses = ["tickets.get"];

  /** An address as a row draws it: its hosts, spaced, ending in their separator, and its id —
   *  what follows the last `/` or `:` (`hypermemetic:MENGER/STORE/PERF-19` →
   *  [`hypermemetic : MENGER / STORE /`, `PERF-19`]). */
  export function split(address) {
    const a = String(address ?? "");
    const at = Math.max(a.lastIndexOf("/"), a.lastIndexOf(":"));
    if (at < 0) return ["", a];
    const hosts = a.slice(0, at + 1).replaceAll(":", " : ").replaceAll("/", " / ").split(/\s+/).filter(Boolean).join(" ");
    return [hosts, a.slice(at + 1)];
  }

  const WORDS = ["no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve"];
  /** A count in words, as a heading says it (to twelve), else digits. */
  export const count = (n) => WORDS[n] ?? String(n);
  /** The page's heading: what the name does — `names two tickets`. */
  export const heading = (n) => `names ${count(n)} tickets`;
  /** Why: the shortcodes that share the name's letters, or how many answer to the name. */
  export function lede(asked, rows) {
    const code = (r) => (r.id.lastIndexOf("-") > 0 ? r.id.slice(0, r.id.lastIndexOf("-")) : null);
    const codes = rows.map(code);
    const n = count(rows.length);
    const N = n.charAt(0).toUpperCase() + n.slice(1);
    return codes.length && codes[0] && codes.every((c) => c === codes[0])
      ? `${N} shortcodes are named ${codes[0]}, each in its own place.`
      : `${N} tickets answer to ${asked}.`;
  }

  /** The row a key moves the choice to from `at`: the next that opens down (or up), wrapping; `at` when none does. */
  export function step(at, rows, down) {
    const n = rows.length;
    for (let k = 1; k <= n; k++) {
      const i = down ? (at + k) % n : (((at - k) % n) + n) % n;
      if (rows[i].href) return i;
    }
    return at;
  }
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  import { ticketHref, permalink, shownId, copyText, baseOf } from "./ticket-links.svelte";

  let { id } = $props();
  const b = bus();
  const base = baseOf();
  const got = live({ snapshot: "tickets.get", params: () => ({ id }), key: "id" });
  const answer = $derived(got.rows.find((r) => Array.isArray(r.choices)));
  const asked = $derived(answer?.id ?? id);
  const choices = $derived(answer?.choices?.length ? answer.choices : (answer?.candidates ?? []).map((address) => ({ address })));

  /** One row per choice, in the store's order. A choice with a uid opens at its route by uid; one
   *  without (out of the reader's reach) is its address alone. */
  const rows = $derived.by(() => {
    const firstOrg = choices.find((c) => c.org)?.org;
    const orgsDiffer = choices.some((c) => c.org && c.org !== firstOrg);
    return choices.map((c) => {
      const [hosts, idShown] = split(c.address);
      const t = { id: c.id, uid: c.uid, title: c.title ?? "" };
      const reachable = !!(c.uid && c.id);
      const p = reachable ? c.parent : null;
      return {
        hosts, id: idShown,
        key: reachable ? c.id : null,
        href: reachable ? ticketHref(t, base) : null,
        path: reachable ? permalink(t, "") : null,
        kind: reachable ? c.kind : null,
        status: reachable ? c.status : null,
        title: reachable ? c.title : null,
        place: reachable && c.epic ? (orgsDiffer && c.org && c.org !== firstOrg ? `${c.org} · ${c.epic}` : c.epic) : null,
        parent: p ? { id: shownId(p.id), title: p.title, href: ticketHref(p.uid ? p : { id: p.address }, base) } : null,
        link: reachable ? t : null,
      };
    });
  });
  let at = $state(-1);
  const chosen = $derived(at >= 0 && rows[at]?.href ? at : Math.max(0, rows.findIndex((r) => r.href)));
  let said = $state("");

  function choose(i, focus) {
    at = i;
    if (focus) document.querySelector(`[data-surface=ticket-collision] [data-choice="${i}"]`)?.focus();
  }
  function open(i) {
    const r = rows[i];
    if (r?.href) b.navigate(r.href);
  }
  async function copy(i) {
    const r = rows[i];
    const link = r?.link ? permalink(r.link) : null;
    if (!link) {
      said = "Only its address is yours to see: it has no link to copy.";
      return;
    }
    said = (await copyText(link)) ? `copied ${link}` : `The clipboard is not available — the link: ${link}`;
  }
  function clicked(ev, i) {
    if (ev.target.closest("a")) return; // the parent's link: followed as a link
    if (!rows[i].href) return;
    choose(i, false);
    if (ev.target.closest("[data-action=copy-link]")) copy(i);
    else open(i);
  }

  b.keys({
    arrowdown: { to: () => rows.length && choose(step(chosen, rows, true), true), label: "next choice" },
    j: { to: () => rows.length && choose(step(chosen, rows, true), true), label: "next choice" },
    arrowup: { to: () => rows.length && choose(step(chosen, rows, false), true), label: "previous choice" },
    k: { to: () => rows.length && choose(step(chosen, rows, false), true), label: "previous choice" },
    enter: { to: () => open(chosen), label: "open the chosen ticket", in_controls: true },
    c: { to: () => copy(chosen), label: "copy the chosen ticket's link" },
  });
</script>

<section class="ticket-collision" data-surface="ticket-collision" data-asked={asked}>
  <div class="chead"><span class="idchip" data-field="asked">{shownId(String(asked))}</span><h1 data-heading>{heading(rows.length)}</h1></div>
  <p class="lede" data-lede>{lede(asked, rows)} <span class="muted">Pick the one you meant; each opens at a link of its own.</span></p>
  <ol class="choices" role="listbox" aria-label={"tickets named " + shownId(String(asked))}>
    {#each rows as r, i (i)}
      <li class="choice" class:out={!r.href} data-choice={i} data-id={r.key} data-out={r.href ? null : ""} role="option"
          tabindex={r.href && chosen === i ? 0 : -1} aria-selected={r.href && chosen === i ? "true" : "false"} aria-disabled={r.href ? null : "true"}
          onclick={(ev) => clicked(ev, i)} onfocus={() => r.href && (at = i)}>
        <i class="kmark" data-field="kind" data-kind={r.kind ?? ""} title={r.kind ?? ""} aria-hidden="true"></i>
        <span class="addr"><span class="hosts" data-field="hosts">{r.hosts}</span><span class="id" data-field="id">{r.id}</span></span>
        {#if r.status}<span class="status" data-field="status" data-status={r.status}>{r.status}</span>{/if}
        {#if r.title != null}<span class="ctitle" data-field="title">{r.title}</span>{/if}
        {#if r.parent}
          <span class="cparent" data-field="parent">under <a href={r.parent.href} title={r.parent.id + " — " + r.parent.title}>{r.parent.id}</a> <span class="pt">{r.parent.title}</span></span>
        {/if}
        {#if !r.href}<span class="cout">not yours to read</span>{/if}
        {#if r.href}
          <span class="meta">
            <span data-field="place">{r.place ?? ""}</span>
            <span class="link" data-field="link">{r.path}</span>
            <button class="copy" type="button" data-action="copy-link">copy link</button>
          </span>
        {/if}
      </li>
    {/each}
  </ol>
  <p class="foot">
    <span class="keys"><kbd>↑</kbd> <kbd>↓</kbd> choose · <kbd>⏎</kbd> open · <kbd>c</kbd> copy link · <kbd>esc</kbd> board</span>
    <span class="said" data-said aria-live="polite">{said}</span>
  </p>
</section>

<style>
  .ticket-collision { max-width: 980px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .chead { display: flex; flex-wrap: wrap; gap: 10px; align-items: baseline; }
  .idchip { padding: 0 8px; font-size: 13px; line-height: 22px; color: var(--accent); font-weight: var(--weight-medium); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  h1 { margin: 0; font-family: var(--font-display); font-size: 22px; line-height: 1.25; font-weight: var(--weight-strong); }
  .lede { margin: 8px 0 14px; }
  .muted { color: var(--muted); }
  .choices { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; }
  .choice { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 4px 10px; align-items: baseline; padding: 10px 12px; cursor: pointer; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-lg); transition: border-color var(--motion-fast) var(--ease), background var(--motion-fast) var(--ease); }
  .choice:hover, .choice:global([data-sheet~=hover]) { border-color: var(--muted); }
  .choice[aria-selected=true], .choice:global([data-sheet~=selected]) { background: var(--sel); border-color: var(--accent); }
  .choice:focus-visible, .choice:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .choice.out { cursor: default; color: var(--muted); background: var(--panel); border-style: dashed; }
  .addr { min-width: 0; overflow-wrap: anywhere; }
  .hosts { margin-right: 4px; color: var(--muted); font-size: 12px; }
  .id { color: var(--accent); font-weight: var(--weight-medium); }
  .ctitle { grid-column: 2 / -1; font-weight: var(--weight-medium); overflow-wrap: anywhere; }
  .cparent { grid-column: 2 / -1; font-size: 12px; color: var(--muted); overflow-wrap: anywhere; }
  .cparent a { margin: 0 4px; color: var(--accent); text-decoration: none; }
  .cparent a:hover { text-decoration: underline; }
  .cout { grid-column: 2 / -1; font-size: 12px; font-style: italic; }
  .meta { grid-column: 2 / -1; display: flex; flex-wrap: wrap; gap: 6px 12px; align-items: center; font-size: 11px; color: var(--muted); }
  .link { overflow-wrap: anywhere; }
  .copy { margin-left: auto; padding: 1px 8px; color: var(--ink); font: inherit; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: border-color var(--motion-fast) var(--ease); }
  .copy:hover, .copy:global([data-sheet~=hover]) { border-color: var(--muted); }
  .copy:focus-visible, .copy:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .status { padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .status[data-status=Pending] { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .status[data-status=Ready] { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .status[data-status=Coding] { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .status[data-status="In Review"] { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .status[data-status=Complete] { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .status[data-status=Blocked] { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .status[data-status=Superseded] { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .status[data-status=Canceled] { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .status[data-status=Epic] { background: var(--st-epic-bg); color: var(--st-epic-fg); }
  .status[data-status=Doc] { background: var(--st-doc-bg); color: var(--st-doc-fg); }
  .kmark { display: inline-block; width: 8px; height: 8px; background: var(--kind-build); border-radius: 50%; }
  .kmark[data-kind=""] { background: var(--line); }
  .kmark[data-kind=scope] { background: var(--kind-scope); border-radius: var(--radius-xs); transform: rotate(45deg) scale(.85); }
  .kmark[data-kind=gap] { background: var(--kind-gap); border-radius: 0; clip-path: polygon(50% 0, 100% 100%, 0 100%); }
  .kmark[data-kind=spike] { background: none; border: 2px solid var(--kind-spike); }
  .kmark[data-kind=execution] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  .kmark[data-kind=doc] { width: 7px; height: 9px; background: none; border: var(--border) solid var(--kind-doc); border-radius: var(--radius-xs); }
  .foot { display: flex; flex-wrap: wrap; gap: 6px 16px; margin: 14px 0 0; font-size: 11px; color: var(--muted); }
  .said { overflow-wrap: anywhere; }
  kbd { display: inline-block; min-width: 16px; padding: 0 4px; font-family: var(--font-text); font-size: 10px; line-height: 15px; text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
  @media (max-width: 600px) {
    .link { display: none; }
    .keys { display: none; }
  }
  @media (prefers-reduced-motion: reduce) {
    .choice, .copy { transition: none; }
  }
</style>
