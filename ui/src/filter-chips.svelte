<!-- SHELL.176-3: the board's filter as chips, Linear's way (owner,
     2026-11-13: "I want a linear like filter on that board"), drawn by
     board-layout-sv's top bar under the filter line. Two views — Recent
     (`#/board`, board-frame's default: `changed within 7d · order changed`)
     and All (`· order code`) — then one chip per rule of the route's filter,
     `property · operator · values · ×`, then + Filter (the key `f`) and
     Clear all. The chips ARE the filter: each is one rule of FILTER-3's
     language (`chipOf` reads one, `ruleOf` writes it back), and every edit
     navigates to the one filter string they make (`#/f/<filter>`), so the
     line above, the rows, the facets and the nav read what they read today.
     A rule the bar cannot draw as a property (an `or`, a group, `text`,
     `saved`, `entered …`) stays as written, a plain chip of its text, held
     in the line; a top-level `or` makes the whole filter one such chip. The
     menu: the properties, then a property's values — with their counts from
     `nav facets { filter }` (which sets the field's own rules aside: what
     picking the value would give) where the service counts that field;
     typed for label, under and moved by; the windows for changed and
     created. Its keys are bound while it is open (app-top's rule). -->
<script module>
  export const uses = ["tickets.nav.facets"];

  export const RECENT = "changed within 7d · order changed";
  const WINDOWS = ["today", "yesterday", "this week", "within 24h", "within 7d", "within 30d"];
  const FLAGS = ["ready", "blocked", "asking", "has commits"];
  /** The properties the menu offers: [prop, label, how its values come]. */
  export const PROPS = [
    ["status", "status", "facet"], ["kind", "kind", "facet"], ["severity", "severity", "facet"],
    ["in", "shortcode", "facet"], ["category", "category", "facet"], ["label", "label", "typed"],
    ["under", "under", "typed"], ["changed", "changed", "window"], ["created", "created", "window"],
    ["by", "moved by", "typed"], ...FLAGS.map((f) => [f, f, "flag"]),
  ];

  /** Splits at each top-level `sep` (outside quotes and parentheses), as `re` matches it. */
  function splitTop(text, re) {
    const out = [];
    let depth = 0, quoted = false, from = 0;
    for (let i = 0; i < text.length; i++) {
      const c = text[i];
      if (c === '"') quoted = !quoted;
      else if (!quoted && c === "(") depth++;
      else if (!quoted && c === ")") depth--;
      else if (!quoted && depth === 0) {
        const m = re.exec(text.slice(i));
        if (m && m.index === 0) {
          out.push(text.slice(from, i));
          i += m[0].length - 1;
          from = i + 1;
        }
      }
    }
    out.push(text.slice(from));
    return out.map((s) => s.trim());
  }
  /** `{ rules, display }`: the filter's top-level `and`-ed rules (one, whole, when it holds a top-level `or`) and its display part. */
  export function split(filter) {
    const [body, ...rest] = splitTop(filter ?? "", /^·/);
    const display = rest.filter(Boolean).join(" · ");
    if (body === "") return { rules: [], display };
    if (splitTop(body, /^\s+or\s+/i).length > 1) return { rules: [body], display };
    return { rules: splitTop(body, /^\s+and\s+/i).filter(Boolean), display };
  }
  const unq = (v) => v.replace(/^"(.*)"$/, "$1");
  const q = (v) => (/[\s,()]/.test(v) ? `"${v}"` : v);
  const VALUE = '("[^"]*"|[^\\s(),"]+)';
  /** A rule as a chip `{ prop, neg, values }`, or null when the bar cannot draw it as a property. */
  export function chipOf(rule) {
    let r = rule.trim(), neg = false;
    if (/^not\s+/i.test(r)) { neg = true; r = r.replace(/^not\s+/i, ""); }
    let m;
    if ((m = new RegExp(`^(status|kind|severity|label|category|in)\\s+in\\s*\\((.+)\\)$`, "i").exec(r)) && m[1].toLowerCase() !== "in")
      return { prop: m[1].toLowerCase(), neg, values: m[2].split(",").map((v) => unq(v.trim())).filter(Boolean) };
    if ((m = /^in\s*\((.+)\)$/i.exec(r))) return { prop: "in", neg, values: m[1].split(",").map((v) => unq(v.trim())).filter(Boolean) };
    if ((m = new RegExp(`^(status|kind|severity|label|category|in|under)\\s+${VALUE}$`, "i").exec(r)) && !(m[1].toLowerCase() === "status" && /^in$/i.test(m[2])))
      return { prop: m[1].toLowerCase(), neg, values: [unq(m[2])] };
    if (!neg && (m = /^(changed|created)\s+(today|yesterday|this week|within \S+)$/i.exec(r))) return { prop: m[1].toLowerCase(), neg, values: [m[2].toLowerCase()] };
    if (!neg && (m = new RegExp(`^by\\s+${VALUE}$`, "i").exec(r))) return { prop: "by", neg, values: [unq(m[1])] };
    const flag = FLAGS.find((f) => f === r.toLowerCase());
    if (flag) return { prop: flag, neg, values: [] };
    return null;
  }
  /** A chip as its rule, in the language's own spelling. */
  export function ruleOf(c) {
    const not = c.neg ? "not " : "";
    if (FLAGS.includes(c.prop)) return not + c.prop;
    if (c.prop === "changed" || c.prop === "created") return `${c.prop} ${c.values[0]}`;
    if (c.values.length > 1) return c.prop === "in" ? `${not}in (${c.values.map(q).join(", ")})` : `${not}${c.prop} in (${c.values.map(q).join(", ")})`;
    return `${not}${c.prop} ${q(c.values[0])}`;
  }
  /** The filter `rules` (each a chip or a rule's text) and `display` make. */
  export function compose(rules, display) {
    const body = rules.map((r) => (typeof r === "string" ? (rules.length > 1 && /\s+or\s+/i.test(r) && !/^\(.*\)$/.test(r) ? `(${r})` : r) : ruleOf(r))).join(" and ");
    return display ? (body ? `${body} · ${display}` : `· ${display}`) : body;
  }
  export function opOf(c) {
    if (c.prop === "changed" || c.prop === "created") return "";
    if (FLAGS.includes(c.prop) || c.values.length < 2) return c.neg ? "is not" : "is";
    return c.neg ? "is none of" : "is any of";
  }
</script>

<script>
  import { tick } from "svelte";
  import { bus, live } from "plexus-ui/host";

  let { filter = null } = $props();
  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  const board = base === "#/" ? "#/board" : base;
  const LABEL = Object.fromEntries(PROPS.map(([p, l]) => [p, l]));
  const HOW = Object.fromEntries(PROPS.map(([p, , h]) => [p, h]));

  const parsed = $derived(split(filter ?? ""));
  const rules = $derived(parsed.rules.map((r) => chipOf(r) ?? r));
  const recent = $derived(/(^|·)\s*order changed\s*($|·)/i.test(parsed.display));

  // The menu: null, or { at: the chip's index (null: a new one), prop (null: the properties) }.
  let menu = $state(null);
  let query = $state("");
  let active = $state(0);
  let typed = $state("");
  let field = $state(null);
  // The counts' filter: the route's rules `nav facets` counts (cells: not a window, `under`, `by` or a
  // rule as text, for which it answers nothing), less the chip being edited — and what it leaves out, said.
  const COUNTED = new Set(["status", "kind", "severity", "in", "category", "label", ...FLAGS]);
  const countable = (r) => typeof r !== "string" && COUNTED.has(r.prop);
  const facetFilter = $derived(compose(rules.filter((r, i) => !(menu && i === menu.at) && countable(r)), ""));
  const uncounted = $derived(rules.filter((r) => !countable(r)).map((r) => (typeof r === "string" ? r : ruleOf(r))));
  const facets = live({ snapshot: "tickets.nav.facets", params: () => ({ filter: facetFilter || null }), key: "field",
                        when: () => menu !== null && menu.prop !== null && HOW[menu.prop] === "facet" });

  const editing = $derived(menu && menu.at !== null ? rules[menu.at] : null);
  const options = $derived.by(() => {
    if (!menu) return [];
    const needle = query.trim().toLowerCase();
    const keep = (o) => needle === "" || o.label.toLowerCase().includes(needle);
    if (menu.prop === null) return PROPS.map(([p, l]) => ({ id: p, label: l })).filter(keep);
    const how = HOW[menu.prop];
    if (how === "window") return WINDOWS.map((w) => ({ id: w, label: w, on: editing?.values?.[0] === w })).filter(keep);
    if (how === "facet") {
      const counted = facets.rows.filter((r) => r.field === menu.prop).map((r) => ({ id: r.value, label: r.value, n: r.held, on: !!editing?.values?.includes(r.value) }));
      const held = (editing?.values ?? []).filter((v) => !counted.some((o) => o.id === v)).map((v) => ({ id: v, label: v, n: 0, on: true }));
      return [...held, ...counted].filter(keep);
    }
    return (editing?.values ?? []).map((v) => ({ id: v, label: v, on: true }));
  });
  $effect(() => {
    if (active >= options.length) active = Math.max(0, options.length - 1);
  });

  function go(next) {
    const text = compose(next, parsed.display);
    const t = (text === "" ? "· order code" : text).replaceAll("%", "%25").replaceAll("/", "%2F").replaceAll("#", "%23");
    b.navigate(base + "f/" + t);
  }
  async function open(at = null, prop = null) {
    menu = { at, prop };
    query = "";
    typed = "";
    active = 0;
    await tick();
    field?.focus();
  }
  function close() {
    menu = null;
  }
  function pickProp(p) {
    if (HOW[p] === "flag") {
      go([...rules, { prop: p, neg: false, values: [] }]);
      close();
      return;
    }
    open(null, p);
  }
  /** A value ticked or unticked on the chip being made or edited. */
  function pickValue(v) {
    const p = menu.prop;
    const next = [...rules];
    if (HOW[p] === "window" || p === "under" || p === "by") {
      const chip = { prop: p, neg: editing?.neg ?? false, values: [v] };
      if (menu.at === null) next.push(chip); else next[menu.at] = chip;
      go(next);
      close();
      return;
    }
    if (menu.at === null) {
      next.push({ prop: p, neg: false, values: [v] });
      menu = { at: next.length - 1, prop: p };
    } else {
      const had = editing.values.includes(v);
      const values = had ? editing.values.filter((x) => x !== v) : [...editing.values, v];
      if (values.length === 0) {
        next.splice(menu.at, 1);
        close();
      } else {
        next[menu.at] = { ...editing, values };
      }
    }
    go(next);
  }
  function choose(o) {
    if (!o) return;
    if (menu.prop === null) pickProp(o.id);
    else pickValue(o.id);
  }
  function enter() {
    if (menu.prop !== null && !["facet", "window"].includes(HOW[menu.prop]) && typed.trim() !== "") {
      const v = typed.trim();
      typed = "";
      if (!editing?.values?.includes(v)) pickValue(v);
      return;
    }
    choose(options[active]);
  }
  function negate(i) {
    const next = [...rules];
    next[i] = { ...rules[i], neg: !rules[i].neg };
    go(next);
  }
  function remove(i) {
    close();
    go(rules.filter((_, j) => j !== i));
  }
  function clearAll() {
    close();
    go([]);
  }

  b.keys({ f: { to: () => open(), label: "add a filter" } });
  $effect(() => {
    if (!menu) return;
    return b.keys({
      "arrowdown": { to: () => (active = options.length ? (active + 1) % options.length : 0), in_inputs: true, only: true, label: "the next choice" },
      "arrowup": { to: () => (active = options.length ? (active - 1 + options.length) % options.length : 0), in_inputs: true, only: true, label: "the choice before" },
      "enter": { to: enter, in_inputs: true, only: true, label: "pick it" },
      "escape": { to: close, in_inputs: true, only: true, label: "close the filter menu" },
    });
  });
</script>

<div class="filter-chips" data-filter-chips data-surface="filter-chips">
  <div class="fc-views" role="group" aria-label="view">
    <button class="fc-view" type="button" data-view="recent" aria-pressed={recent ? "true" : "false"} onclick={() => b.navigate(board)}>Recent</button>
    <button class="fc-view" type="button" data-view="all" aria-pressed={recent ? "false" : "true"} onclick={() => b.navigate(base + "f/· order code")}>All</button>
  </div>
  <span class="fc-sep" aria-hidden="true"></span>
  {#each rules as r, i (i + ":" + (typeof r === "string" ? r : ruleOf(r)))}
    {#if typeof r === "string"}
      <span class="fc-chip" data-chip="raw" data-rule={r} title="held in the filter line: the chips cannot say it">
        <button class="fc-part fc-raw" type="button" onclick={() => b.emit("filter.focus", {})}>{r}</button>
        <button class="fc-part fc-x" type="button" data-remove aria-label={"remove " + r} onclick={() => remove(i)}>×</button>
      </span>
    {:else}
      <span class="fc-chip" data-chip={r.prop} data-rule={ruleOf(r)} data-editing={menu?.at === i ? "" : undefined}>
        <span class="fc-part fc-prop">{LABEL[r.prop] ?? r.prop}</span>
        {#if opOf(r)}<button class="fc-part fc-op" type="button" data-op title="is / is not" onclick={() => negate(i)}>{opOf(r)}</button>{/if}
        {#if r.values.length}<button class="fc-part fc-vals" type="button" data-values onclick={() => open(i, r.prop)}>{r.values.join(", ")}</button>{/if}
        <button class="fc-part fc-x" type="button" data-remove aria-label={"remove " + ruleOf(r)} onclick={() => remove(i)}>×</button>
      </span>
    {/if}
  {/each}
  <span class="fc-anchor">
    <button class="fc-add" type="button" data-add-filter aria-haspopup="listbox" aria-expanded={menu ? "true" : "false"} onclick={() => (menu ? close() : open())}>+ Filter <kbd>F</kbd></button>
    {#if menu}
      <div class="fc-scrim" aria-hidden="true" onclick={close}></div>
      <div class="fc-menu" data-filter-menu={menu.prop ?? "props"} role="dialog" aria-label="add a filter">
        {#if menu.prop !== null}<div class="fc-menu-head"><button class="fc-back" type="button" data-back onclick={() => open(null, null)} aria-label="back to the properties">‹</button>{LABEL[menu.prop]}</div>{/if}
        {#if menu.prop !== null && !["facet", "window"].includes(HOW[menu.prop])}
          <input class="fc-field" data-menu-input bind:this={field} bind:value={typed} spellcheck="false" autocomplete="off"
                 placeholder={menu.prop === "label" ? "a label, then Enter" : menu.prop === "under" ? "a ticket id or shortcode, then Enter" : "me, system or a principal, then Enter"}/>
        {:else}
          <input class="fc-field" data-menu-input bind:this={field} bind:value={query} spellcheck="false" autocomplete="off"
                 placeholder={menu.prop === null ? "Filter by…" : "Pick values…"}/>
        {/if}
        <ul class="fc-options" role="listbox" aria-label={menu.prop === null ? "properties" : "values"}>
          {#if menu.prop !== null && HOW[menu.prop] === "facet" && facets.status === "pending"}
            <li class="fc-note" data-loading>counting…</li>
          {:else if menu.prop !== null && HOW[menu.prop] === "facet" && (facets.status === "error" || facets.status === "refused")}
            <li class="fc-note" data-refused>{facets.message ?? "not counted for this filter"}</li>
          {/if}
          {#if menu.prop !== null && HOW[menu.prop] === "facet" && uncounted.length}
            <li class="fc-note" data-uncounted>counted without {uncounted.join(", ")}</li>
          {/if}
          {#each options as o, k (o.id)}
            <li><button class="fc-option" type="button" role="option" data-option={o.id} aria-selected={k === active ? "true" : "false"}
                        data-active={k === active ? "" : undefined} data-on={o.on ? "" : undefined}
                        onmousedown={(ev) => ev.preventDefault()} onmouseenter={() => (active = k)} onclick={() => choose(o)}>
              {#if menu.prop !== null && HOW[menu.prop] !== "window"}<span class="fc-check" aria-hidden="true"></span>{/if}
              <span class="fc-label">{o.label}</span>
              {#if o.n != null}<span class="fc-n" data-count>{o.n}</span>{/if}
            </button></li>
          {/each}
        </ul>
      </div>
    {/if}
  </span>
  {#if rules.length}<button class="fc-clear" type="button" data-clear-all onclick={clearAll}>Clear all</button>{/if}
</div>

<style>
  .filter-chips { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; min-width: 0; font-size: 12px; }
  .fc-views { display: inline-flex; gap: 2px; }
  .fc-view { padding: 1px 10px; font: inherit; color: var(--muted); background: none; border: var(--border) solid transparent; border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease), color var(--motion-fast) var(--ease); }
  .fc-view:hover, .fc-view:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }
  .fc-view[aria-pressed=true], .fc-view:global([data-sheet~=checked]) { color: var(--ink); background: var(--chip-bg); border-color: var(--chip-line); }
  .fc-view:focus-visible, .fc-add:focus-visible, .fc-part:focus-visible, .fc-clear:focus-visible,
  .fc-view:global([data-sheet~=focus]), .fc-add:global([data-sheet~=focus]), .fc-part:global([data-sheet~=focus]), .fc-clear:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .fc-sep { width: var(--border); height: 16px; background: var(--line); margin: 0 2px; }
  .fc-chip { display: inline-flex; align-items: stretch; max-width: 100%; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius); overflow: hidden; }
  .fc-chip[data-editing] { border-color: var(--accent); }
  .fc-part { display: inline-flex; align-items: center; padding: 1px 7px; font: inherit; color: inherit; background: none; border: 0; white-space: nowrap; }
  .fc-part + .fc-part { border-left: var(--border) solid var(--chip-line); }
  button.fc-part { cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  button.fc-part:hover, button.fc-part:global([data-sheet~=hover]) { color: var(--ink); background: var(--chip-hover); }
  .fc-prop { color: var(--ink); }
  .fc-vals { color: var(--accent); max-width: 280px; overflow: hidden; text-overflow: ellipsis; }
  .fc-raw { color: var(--ink); font-family: var(--mono); max-width: 420px; overflow: hidden; text-overflow: ellipsis; }
  .fc-x { padding: 1px 6px; color: var(--muted); }
  .fc-anchor { position: relative; display: inline-flex; }
  .fc-add { padding: 1px 8px; font: inherit; color: var(--muted); background: none; border: var(--border) dashed var(--chip-line); border-radius: var(--radius); cursor: pointer; transition: color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .fc-add:hover, .fc-add:global([data-sheet~=hover]) { color: var(--ink); border-color: var(--muted); }
  .fc-add kbd { margin-left: 4px; padding: 0 4px; font: inherit; font-size: 10px; border: var(--border) solid var(--line); border-radius: var(--radius-xs); }
  .fc-clear { margin-left: auto; font: inherit; color: var(--muted); background: none; border: 0; cursor: pointer; }
  .fc-clear:hover, .fc-clear:global([data-sheet~=hover]) { color: var(--ink); }
  .fc-scrim { position: fixed; inset: 0; z-index: 9; }
  .fc-menu { position: absolute; z-index: 10; top: calc(100% + 6px); left: 0; width: 260px; padding: 4px; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-lg); box-shadow: 0 6px 18px var(--shadow); }
  .fc-menu-head { display: flex; gap: 6px; align-items: center; padding: 2px 4px 4px; color: var(--muted); }
  .fc-back { padding: 0 6px; font: inherit; color: var(--muted); background: none; border: 0; border-radius: var(--radius); cursor: pointer; }
  .fc-back:hover, .fc-back:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }
  .fc-field { width: 100%; margin-bottom: 4px; padding: 5px 8px; font: inherit; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .fc-field:focus-visible, .fc-field:global([data-sheet~=focus]) { outline: none; border-color: var(--focus); }
  .fc-options { max-height: 280px; overflow: auto; margin: 0; padding: 0; list-style: none; }
  .fc-option { display: flex; gap: 8px; align-items: center; width: 100%; height: var(--pitch); padding: 0 8px; font: inherit; color: var(--ink); text-align: left; background: none; border: 0; border-radius: var(--radius); cursor: pointer; }
  .fc-option[data-active] { background: var(--hover); }
  .fc-check { width: 12px; height: 12px; flex: none; border: var(--border) solid var(--line); border-radius: var(--radius-xs); }
  .fc-option[data-on] .fc-check { background: var(--accent); border-color: var(--accent); }
  .fc-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fc-n { margin-left: auto; padding: 0 6px; font-size: 11px; line-height: 16px; color: var(--chip-fg); background: var(--chip-bg); border-radius: var(--radius-pill); font-variant-numeric: tabular-nums; }
  .fc-note { padding: 4px 8px; font-size: 11px; color: var(--muted); }
  .fc-note[data-refused] { color: var(--warn); }
  @media (prefers-reduced-motion: reduce) {
    .fc-view, button.fc-part, .fc-add { transition: none; }
  }
</style>
