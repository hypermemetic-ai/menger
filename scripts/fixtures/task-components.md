TICKETS-23's check (scripts/task-sheet-check.sh) defines these on its throwaway: TICKETS-10's `m-task` and `m-tasks` as filed there (the tracker's TICKETS-10 is their source; this is the check's copy), and a stand-in `m-decision` (the house's is a component ticket a throwaway lacks) whose options answer through `menger.input.submit`.

```component m-task id=sheet-lock status=Ready name="Lock the page under a sheet" files="frontend/src/sheet.rs" model=sonnet verify="cargo test --bin menger-app -- sheet"
<template><section class="t" part="row"><div class="h" part="head" role="button" tabindex="0" aria-expanded="false"><span class="box" part="box" aria-hidden="true"></span><span class="m"><code class="id" part="id"></code><span class="st" part="status"></span><span class="nm" part="title"></span><a class="from" part="from" hidden></a></span><span class="tg" part="toggle" aria-hidden="true"></span></div><div class="ps" part="params"></div><div class="x" part="body"><slot></slot></div></section></template>
<style>
:host{display:block;margin:.35rem 0;font-family:var(--sans);color:var(--ink)}
.t{border:var(--border) solid var(--line);border-radius:var(--radius-lg);background:var(--card);padding:.5rem .7rem}
:host([data-in-list]){margin:0}
:host([data-in-list]) .t{border:0;border-radius:0;background:transparent;border-top:var(--border) solid var(--line)}
:host([data-in-list]:first-of-type) .t{border-top:0}
.h{display:grid;grid-template-columns:auto minmax(0,1fr) auto;align-items:start;gap:.5rem;cursor:pointer;min-width:0;outline:none;border-radius:var(--radius)}
.m{display:flex;flex-wrap:wrap;align-items:center;gap:.3rem .5rem;min-width:0}
.h:focus-visible{box-shadow:0 0 0 2px var(--accent)}
.box{flex:none;margin-top:.1rem;display:inline-grid;place-items:center;width:1rem;height:1rem;border-radius:var(--radius-sm);border:1.5px solid var(--sf,var(--muted));background:transparent;font:var(--weight-bold) calc(11px * var(--text-scale))/1 var(--sans);color:var(--sb,var(--card))}
.done .box{background:var(--sf);border-color:var(--sf)}
.done .box::after{content:"✓"}
.off .box::after{content:"–";color:var(--sf)}
.id{font:calc(12px * var(--text-scale))/1.4 var(--mono);color:var(--accent)}
.st{flex:none;font:var(--weight-strong) calc(10.5px * var(--text-scale))/1 var(--sans);letter-spacing:.03em;padding:.22rem .5rem;border-radius:var(--radius-pill);background:var(--sb,var(--chip-bg));color:var(--sf,var(--chip-fg));white-space:nowrap}
.nm{flex:1 1 12rem;min-width:0;font:var(--weight-strong) calc(14px * var(--text-scale))/1.35 var(--font-display);overflow-wrap:anywhere}
.done .nm,.off .nm{color:var(--muted)}
.off .nm{text-decoration:line-through}
.from{font:calc(11.5px * var(--text-scale))/1.4 var(--mono);color:var(--muted);text-decoration:none;white-space:nowrap}
.from:hover{color:var(--accent);text-decoration:underline}
.from[hidden]{display:none}
.tg{display:inline-grid;place-items:center;width:1.2rem;height:1.2rem;color:var(--muted);font-size:calc(14px * var(--text-scale));line-height:1;transition:transform var(--motion-fast) var(--ease)}
.h:hover .tg{color:var(--accent)}
.tg::before{content:"";width:.42rem;height:.42rem;border-right:1.5px solid currentColor;border-bottom:1.5px solid currentColor;transform:translateX(-1px) rotate(-45deg)}
.open .tg{transform:rotate(90deg)}
.ps{display:flex;flex-wrap:wrap;gap:.3rem;margin:.4rem 0 0 1.5rem}
.ps:empty{display:none}
.p{display:inline-flex;align-items:baseline;gap:.3rem;max-width:100%;padding:.12rem .45rem;border:var(--border) solid var(--chip-line);border-radius:var(--radius-md);background:var(--chip-bg);color:var(--chip-fg);font:calc(11px * var(--text-scale))/1.45 var(--mono);overflow-wrap:anywhere}
.p b{font:var(--weight-medium) calc(10px * var(--text-scale))/1.45 var(--sans);letter-spacing:.04em;text-transform:uppercase;color:var(--muted)}
.x{display:none;margin:.45rem 0 .1rem 1.5rem;padding-left:.75rem;border-left:2px solid var(--line);font-size:calc(13.5px * var(--text-scale));min-width:0;overflow-wrap:anywhere}
.open .x{display:block}
@media (prefers-reduced-motion:reduce){.tg{transition:none}}
</style>
<docs>
  <summary>One task inside its ticket (TICKETS-10: the owner answered its form decision, a task is a block in the body). A row: a check box, the task's id, its status as a chip, its title, its parameters as small labelled chips, and its direction beneath. A click or Enter on the row opens the task in a sheet on its ticket's page: its direction as a full ticket body, markdown, components, inputs and mockups, drawn by the page's own renderer (TICKETS-23). Elsewhere, and inside a task's sheet, it folds open in place. Several in a row, or inside an m-tasks, read as a checklist.</summary>
  <attr name="id" type="string" required>The task's id, a short slug unique in the ticket.</attr>
  <attr name="name" type="string">The task's title, on the row.</attr>
  <attr name="status" type="enum(Draft|Pending|Ready|Coding|In Review|Complete|Canceled)">The task's status, the ticket's words (default Draft). Written by hand until the server tracks tasks.</attr>
  <attr name="files" type="string">The files it touches, comma-separated.</attr>
  <attr name="section" type="string">The section of the code or the page it works in.</attr>
  <attr name="feature" type="string">The feature it serves, a ticket id or a word.</attr>
  <attr name="depends" type="string">Other task ids in this ticket it waits on, comma-separated.</attr>
  <attr name="model" type="enum(opus|sonnet|haiku)">The model it is delegated to.</attr>
  <attr name="verify" type="string">The command that checks it: the selected tests (CI-3).</attr>
  <attr name="from" type="ticket">The ticket it was before it was folded in: a link to it.</attr>
  <attr name="open" type="boolean">Start with the direction unfolded.</attr>
  <slot>The direction: a few lines of plain text, or markdown between blank lines; in its sheet it is a full ticket body.</slot>
  <part name="row">The row.</part>
  <part name="head">The row's line: box, id, status, title, from, toggle; a click opens the task's sheet, or folds the direction in place.</part>
  <part name="box">The check box: ticked at Complete, struck at Canceled.</part>
  <part name="id">The task's id.</part>
  <part name="status">The status chip.</part>
  <part name="title">The title.</part>
  <part name="from">The link to the old ticket.</part>
  <part name="toggle">The fold's arrow.</part>
  <part name="params">The parameters' chips.</part>
  <part name="param" scripted>One parameter's chip: its label and its value.</part>
  <part name="body">The direction, when folded open in place.</part>
  <example><m-tasks><m-task id="sheet-lock" status="Coding" name="Lock the page under a sheet" files="frontend/src/sheet.rs, frontend/index.html" section="the page lock" feature="CHAT-31" model="sonnet" verify="cargo test --bin menger-app -- sheet">Lock the document while a sheet is open; restore its scroll on close unless the sheet navigated.</m-task><m-task id="restore" status="Complete" name="Restore the scroll" depends="sheet-lock" model="haiku">Put the scroll back on close.</m-task></m-tasks></example>
  ~~~markdown
  <m-tasks>

  <m-task id="sheet-lock" status="Ready" name="Lock the page under a sheet" files="frontend/src/sheet.rs" model="sonnet" verify="cargo test --bin menger-app -- sheet">

  Lock the document while a sheet is open; restore its scroll on close unless the sheet navigated.

  </m-task>

  </m-tasks>
  ~~~
  Blank lines inside a task let its direction hold markdown. Self-contained: it needs no m-house.
</docs>
<script>
const $ = (q) => this.root.querySelector(q), doc = this.ownerDocument;
const ST = ['Draft', 'Pending', 'Ready', 'Coding', 'In Review', 'Complete', 'Canceled'];
const raw = String(this.attr('status') || 'Draft').trim();
const st = ST.find((s) => s.toLowerCase() === raw.toLowerCase()) || raw;
const k = st.toLowerCase().replace(/\s+/g, '-');
const t = $('.t'), h = $('.h');
t.style.setProperty('--sb', 'var(--st-' + k + '-bg)');
t.style.setProperty('--sf', 'var(--st-' + k + '-fg)');
if (st === 'Complete') t.classList.add('done');
if (st === 'Canceled' || st === 'Superseded') t.classList.add('off');
if (this.parentElement && this.parentElement.localName === 'm-tasks') this.setAttribute('data-in-list', '');
const id = String(this.attr('id') || '').trim();
$('.id').textContent = id;
$('.st').textContent = st;
$('.nm').textContent = String(this.attr('name') || '').trim();
const from = String(this.attr('from') || '').trim().toUpperCase();
if (from) {
  const a = $('.from');
  a.hidden = false;
  a.textContent = 'was ' + from;
  a.href = '#/ticket/' + from;
  a.title = 'This task was the ticket ' + from;
  a.onclick = (e) => { e.preventDefault(); e.stopPropagation(); this.menger.navigate('#/ticket/' + from); };
}
['files', 'section', 'feature', 'depends', 'model', 'verify'].forEach((n) => {
  const v = String(this.attr(n) || '').trim();
  if (!v) return;
  const c = doc.createElement('span'), b = doc.createElement('b');
  c.className = 'p';
  c.setAttribute('part', 'param');
  b.textContent = n;
  c.append(b, v);
  $('.ps').append(c);
});
if (this._mtOpen === undefined) this._mtOpen = this.attr('open') !== null;
const paint = () => { t.classList.toggle('open', this._mtOpen); h.setAttribute('aria-expanded', String(this._mtOpen)); };
paint();
// TICKETS-23: on its ticket's page the row opens the task's sheet, a full
// ticket body drawn by the page's own renderer; where none opens (a draft,
// a chat message, a cited window, a task's own sheet) it folds in place.
const toggle = () => {
  if (id && this.menger.task && this.menger.task.open(id)) return;
  this._mtOpen = !this._mtOpen;
  paint();
};
h.onclick = toggle;
h.onkeydown = (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggle(); } };
</script>
```

```component m-tasks
<template><section class="l" part="list"><header class="hd" part="head"><span class="lb" part="label">Tasks</span><span class="n" part="count"></span><span class="bar" part="bar"><i></i></span></header><div class="rows" part="rows"><slot></slot></div></section></template>
<style>
:host{display:block;margin:.6rem 0;font-family:var(--sans);color:var(--ink)}
.l{border:var(--border) solid var(--line);border-radius:var(--radius-xl);background:var(--card);overflow:hidden}
.hd{display:flex;align-items:center;gap:.6rem;padding:.5rem .75rem;background:var(--panel);border-bottom:var(--border) solid var(--line)}
.lb{font:var(--weight-strong) calc(12px * var(--text-scale))/1 var(--sans);letter-spacing:.06em;text-transform:uppercase;color:var(--muted)}
.n{font:var(--weight-bold) calc(13px * var(--text-scale))/1 var(--mono);color:var(--ink);font-variant-numeric:tabular-nums}
.bar{flex:1;max-width:12rem;height:6px;border-radius:var(--radius-pill);background:var(--chip-bg);border:var(--border) solid var(--chip-line);overflow:hidden;margin-left:auto}
.bar i{display:block;height:100%;width:0;background:var(--st-complete-fg);transition:width var(--motion-fast) var(--ease)}
.rows{padding:.15rem .75rem}
@media (prefers-reduced-motion:reduce){.bar i{transition:none}}
</style>
<docs>
  <summary>A checklist of m-task rows under a head that rolls them up: Complete over the tasks not Canceled, `3/5 done`, with a bar.</summary>
  <attr name="label" type="string">The head's label (default Tasks).</attr>
  <slot>m-task children, in the order to do them.</slot>
  <part name="list">The checklist.</part>
  <part name="head">The head: label, count, bar.</part>
  <part name="label">The label.</part>
  <part name="count">The roll-up, done/total.</part>
  <part name="bar">The roll-up's bar.</part>
  <part name="rows">The rows.</part>
  <example><m-tasks><m-task id="a" status="Complete" name="First"></m-task><m-task id="b" status="Ready" name="Second"></m-task></m-tasks></example>
  The roll-up follows a task's status while the page is open.
</docs>
<script>
const $ = (q) => this.root.querySelector(q);
$('.lb').textContent = this.attr('label') || 'Tasks';
const count = () => {
  const ts = Array.from(this.querySelectorAll(':scope > m-task'));
  const norm = (t) => String(t.getAttribute('status') || 'Draft').trim().toLowerCase();
  const live = ts.filter((t) => !['canceled', 'superseded'].includes(norm(t)));
  const done = live.filter((t) => norm(t) === 'complete').length;
  $('.n').textContent = done + '/' + live.length + ' done';
  $('.bar i').style.width = (live.length ? (100 * done) / live.length : 0) + '%';
  $('.bar').title = done + ' of ' + live.length + ' tasks Complete' + (ts.length > live.length ? ' (' + (ts.length - live.length) + ' canceled)' : '');
};
count();
const mo = new MutationObserver(count);
mo.observe(this, { childList: true, subtree: true, attributes: true, attributeFilter: ['status'] });
return () => mo.disconnect();
</script>
```

```component m-decision
<template><div class="d" part="box"><div class="q" part="question"></div><div class="o" part="options"></div><p class="a" part="answer"></p></div></template>
<style>
:host{display:block;margin:.6rem 0;font-family:var(--sans);color:var(--ink)}
.d{border:var(--border) solid var(--line);border-radius:var(--radius-lg);background:var(--card);padding:.6rem .8rem}
.q{font-weight:var(--weight-strong);margin-bottom:.4rem}
.o{display:flex;flex-wrap:wrap;gap:.4rem}
button{font:inherit;padding:.3rem .7rem;border:var(--border) solid var(--line);border-radius:var(--radius-md);background:var(--panel);color:var(--ink);cursor:pointer}
button[aria-pressed="true"]{border-color:var(--accent);color:var(--accent)}
.a{margin:.4rem 0 0;color:var(--muted);font-size:.85em}
.a:empty{display:none}
</style>
<script>
const $ = (q) => this.root.querySelector(q), doc = this.ownerDocument;
const id = String(this.attr('id') || '').trim();
// Drawn from its children, again whenever they change (a frame may set
// them after the element is defined).
const draw = () => {
  const q = this.querySelector('m-question');
  $('.q').textContent = q ? q.textContent.trim() : '';
  $('.o').replaceChildren();
  Array.from(this.querySelectorAll('m-option')).forEach((o) => {
    const value = o.getAttribute('value') || o.getAttribute('name');
    const b = doc.createElement('button');
    b.textContent = o.getAttribute('name') || value;
    b.setAttribute('data-value', value);
    b.onclick = () => {
      this.menger.input.submit(id, { type: 'choice', option: value }).then(
        () => { $('.a').textContent = 'answered: ' + value; this.root.querySelectorAll('button').forEach((x) => x.setAttribute('aria-pressed', String(x === b))); },
        (e) => { $('.a').textContent = 'refused: ' + e.message; }
      );
    };
    $('.o').append(b);
  });
};
draw();
new MutationObserver(draw).observe(this, { childList: true, subtree: true });
</script>
```
