<!-- SURFACE-24: what a revision is, said the same in the two lists of them —
     ticket-actions' "Restore a revision" picker and the Revisions section
     (ticket-revisions). The owner, 2026-11-15, of a ticket whose design
     iterations are its revisions: "but... why can't I see every iteration
     anymore?" and "I can't see them in the revision picker". A row said rev,
     when, who and the fields; nothing said which iteration it was, and the
     picker held the newest five.

     As a component: a revision's label — the ticket's title as that revision
     keeps it (`revisions.list` answers it on each row) and, when it has one,
     the revision's message ("restored title, body from revision 5"),
     `[data-field=rev-title]` and `[data-field=rev-message]`. Its module gives
     both lists the same paging: `revisionRows` folds the live newest page,
     the older pages asked and the newest revision `watch` told of into the
     rows newest first, as far down as they run without a gap (a page that
     moved up under newer revisions leaves none unsaid: "older" asks from
     there); `olderPage` asks the page below a rev. -->
<script module>
  export const uses = ["tickets.revisions.list"];

  /** The revisions one page asks, live or older. */
  export const REV_PAGE = 50;

  const two = (n) => String(n).padStart(2, "0");
  /** A revision's moment, `MM-DD HH:MM` (UTC); "just now" for a row `revised` renewed. */
  export function revWhen(u) {
    if (u == null) return "just now";
    const d = new Date(u * 1000);
    return `${two(d.getUTCMonth() + 1)}-${two(d.getUTCDate())} ${two(d.getUTCHours())}:${two(d.getUTCMinutes())}`;
  }
  /** Who made it: their name, or the principal cut short. */
  export const revWho = (r) => r.name ?? (r.actor ? r.actor.slice(0, 10) + "…" : "");

  /** One ticket's rows newest first: the live page's, the older pages' and the newest that `watch`
   *  told of (`told`: `revised { rev, fields, title }`, a write V2-37 folded into the newest rev renewing
   *  its row), down to the first rev missing. `more`: a rev below the last row is kept; `newest`: the count. */
  export function revisionRows(live, older, told) {
    const by = new Map();
    for (const r of older) if (r.rev != null) by.set(r.rev, r);
    for (const r of live) if (r.rev != null) by.set(r.rev, r);
    const t = told ?? null;
    const top = Math.max(0, ...by.keys());
    const newest = Math.max(t?.rev ?? 0, top);
    const rows = [];
    for (let n = top; n >= 1; n--) {
      const r = by.get(n);
      if (!r) break;
      rows.push(t && n === t.rev && (String(r.fields ?? []) !== String(t.fields ?? []) || (t.title && t.title !== r.title))
        ? { ...r, fields: t.fields, title: t.title || r.title, at_unix: null } : r);
    }
    const last = rows.at(-1)?.rev ?? 1;
    return { rows, newest, more: last > 1 };
  }

  /** The page of `id`'s revisions below `before`, asked once — older revisions do not change. */
  export async function olderPage(b, id, before) {
    const r = await b.call("tickets.revisions.list", { id, before, limit: REV_PAGE });
    if (!r?.ok) return { rows: [], message: r?.message ?? "refused" };
    const events = r.events ?? [];
    return { rows: events.filter((e) => e?.type === "revision" || (e?.rev != null && e?.ticket)).map((e) => e.revision ?? e), message: null };
  }
</script>

<script>
  let { r } = $props();
</script>

<span class="revision-label"><span class="rev-t" data-field="rev-title">{r.title ?? ""}</span>{#if r.message}<span class="rev-m" data-field="rev-message">{r.message}</span>{/if}</span>

<style>
  .revision-label { display: flex; gap: 6px; align-items: baseline; min-width: 0; overflow: hidden; font-size: 12px; white-space: nowrap; }
  .rev-t { overflow: hidden; text-overflow: ellipsis; color: var(--ink); }
  .rev-m { flex: 0 1 auto; overflow: hidden; text-overflow: ellipsis; font-size: 11px; color: var(--muted); font-style: italic; }
  .rev-m::before { content: "· "; font-style: normal; }
</style>
