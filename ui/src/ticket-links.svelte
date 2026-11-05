<!-- V2-42 (TICKETS-36 on v2): a ticket's link, made in one place. Every link
     the UI makes to a ticket is by its uid: `#/t/<uid>/<ID>-<slug>` (and the
     editor's `#/t/<uid>/<ID>-<slug>/edit`). Only the uid is read — page-t
     opens the ticket by it, whatever its id is now; `<ID>-<slug>` is for the
     reader, and the page puts a stale one right in place. A ticket the UI
     knows no uid for (a row it never met, a server before TICKETS-35) is
     linked by its name, `#/ticket/<name>` (`#/edit/<name>`), which still
     resolves — and the page then rewrites the address to the uid form
     (`settle`). A svelte/1 component imports what it needs from here:

       import { ticketHref } from "./ticket-links.svelte";

     Nothing else in ui/src spells a ticket's route: tests/links_check.mjs
     refuses one. rhx/1 imports no code, so the rhx twins spell two forms
     inline, which the check allows: by uid, `"#/t/" + r.uid + "/" + r.id`
     (V2-50: no slug, which the page puts right in place), and by name for a
     row with no uid (ticket-chip holds an id alone; the nav's shortcode root
     comes in a group, which carries no uid). V2-50 adds the link to share,
     `permalink` (`<origin>/t/<uid>/…`, which the shell answers `302` into the
     app), and `copyText`. As a component it is a plain link to the ticket. -->
<script module>
  export const uses = [];

  /** The most a slug holds, cut at a word. */
  export const SLUG_MAX = 48;

  // A Latin letter the decomposition leaves whole, as a slug spells it.
  const SPECIAL = { ß: "ss", æ: "ae", Æ: "ae", œ: "oe", Œ: "oe", ø: "o", Ø: "o", đ: "d", Đ: "d", ð: "d", Ð: "d", ħ: "h", Ħ: "h", ı: "i", ĳ: "ij", Ĳ: "ij", ł: "l", Ł: "l", ŀ: "l", Ŀ: "l" };

  /** A Latin letter's base (`é` → `e`, `ß` → `ss`, `Œ` → `oe`); null for anything else. */
  function fold(c) {
    if (SPECIAL[c]) return SPECIAL[c];
    const base = c.normalize("NFD").replace(/[̀-ͯ]/g, "");
    return /^[A-Za-z]$/.test(base) ? base.toLowerCase() : null;
  }

  /** A title's slug: its words in lower-case ASCII — a Latin letter folded to its base, an
   *  apostrophe inside a word dropped, anything else between words — joined by `-`, as many as
   *  fit in SLUG_MAX (a first word longer than that, cut). Empty for a title with no such word. */
  export function slug(title) {
    const words = [];
    let word = "";
    for (const c of String(title ?? "")) {
      if (/^[A-Za-z0-9]$/.test(c)) word += c.toLowerCase();
      else if (fold(c) != null) word += fold(c);
      else if (c === "'" || c === "’" || c === "ʼ") { /* `app's` is one word: `apps` */ }
      else if (word) { words.push(word); word = ""; }
    }
    if (word) words.push(word);
    let out = "";
    for (const w of words) {
      const room = out ? SLUG_MAX - out.length - 1 : SLUG_MAX;
      if (w.length > room) {
        if (!out) out = w.slice(0, SLUG_MAX);
        break;
      }
      out = out ? out + "-" + w : w;
    }
    return out;
  }

  /** A key as a row shows it: a namesake's (`PERF.115-19`) as its letters and number, `PERF-19`. */
  export function shownId(id) {
    const at = id.lastIndexOf("-");
    if (at < 0) return id;
    const [code, n] = [id.slice(0, at), id.slice(at + 1)];
    const dot = code.indexOf(".");
    return dot >= 0 && /^[0-9]+$/.test(code.slice(dot + 1)) ? code.slice(0, dot) + "-" + n : id;
  }

  /** `<ID>-<slug>`: the readable tail of a ticket's link; the id alone when the title has no word. */
  export function tail(id, title) {
    const s = slug(title);
    return s ? `${shownId(id)}-${s}` : shownId(id);
  }

  /** The route prefix the page is under: `#/sv/` on the Svelte board's alias, else `#/`. */
  export function baseOf(hash = typeof location === "undefined" ? "#/" : location.hash) {
    return hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  }

  /** A uid as a route spells it: 8 characters of Crockford's base32, lower case; else null. */
  export function uidOf(u) {
    const s = typeof u === "string" ? u.trim().toLowerCase() : "";
    return /^[0-9a-hjkmnp-tv-z]{8}$/.test(s) ? s : null;
  }

  /** The link to a ticket — `t` a row, a summary or `{ id, uid?, title? }`: by its uid,
   *  `#/t/<uid>/<ID>-<slug>`; by its name, `#/ticket/<id>`, when it has none. */
  export function ticketHref(t, base = baseOf()) {
    const uid = uidOf(t?.uid);
    return uid ? `${base}t/${uid}/${tail(t.id, t.title)}` : `${base}ticket/${t.id}`;
  }

  /** The link to a ticket `t` names (its parent, a blocker, an unlock): by the uid its `uid_of` holds. */
  export function namedHref(t, id, base = baseOf()) {
    return ticketHref({ id, uid: t?.uid_of?.[id] }, base);
  }

  /** The editor's link to a ticket: `#/t/<uid>/<ID>-<slug>/edit`, else `#/edit/<id>`. */
  export function editHref(t) {
    const uid = uidOf(t?.uid);
    return uid ? `#/t/${uid}/${tail(t.id, t.title)}/edit` : `#/edit/${t.id}`;
  }

  /** V2-50 (main's TICKETS-36): the link to share — `<origin>/t/<uid>/<ID>-<slug>`, which the shell
   *  answers `302` into the app's route by uid (`origin` "" gives the path alone); null for a ticket
   *  with no uid. What Copy link copies, on the page, in its actions and on the collision page. */
  export function permalink(t, origin = typeof location === "undefined" ? "" : location.origin) {
    const uid = uidOf(t?.uid);
    return uid ? `${origin}/t/${uid}/${tail(t.id, t.title)}` : null;
  }

  /** `text` on the clipboard: true when it went, false when the clipboard is not the page's. */
  export async function copyText(text) {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      return false;
    }
  }

  /** What a route names a ticket by: `{ uid }` for `#/t/<uid>[/…]`, `{ name }` for
   *  `#/ticket/<name>` or `#/edit/<name>` (either under `#/sv/`); null for any other. */
  export function named(hash) {
    const h = String(hash ?? "").split("?")[0].replace(/^#\/sv\//, "#/");
    let m = /^#\/t\/([^/]+)(?:\/|$)/.exec(h);
    if (m) return uidOf(m[1]) ? { uid: uidOf(m[1]) } : null;
    m = /^#\/(?:ticket|edit)\/(.+?)\/?$/.exec(h);
    return m ? { name: m[1] } : null;
  }

  /** V2-42: the address the page `hash` should have now that it holds `t` — the uid form, with
   *  its tail as the ticket's now and the query kept — or null when it has it already, when `t`
   *  has no uid, or when `hash` names no ticket. The editor's routes stay the editor's. */
  export function settled(hash, t) {
    const at = named(hash);
    if (!at || !uidOf(t?.uid) || (at.uid && at.uid !== uidOf(t.uid))) return null;
    const [path, ...rest] = String(hash).split("?");
    const query = rest.length ? "?" + rest.join("?") : "";
    const edit = /^#\/(?:sv\/)?edit\//.test(path) || /^#\/(?:sv\/)?t\/[^/]+\/[^/]+\/edit\/?$/.test(path);
    const want = edit ? editHref(t) : ticketHref(t, baseOf(path));
    return path === want ? null : want + query;
  }

  /** Put the address bar right in place — no new history entry, no route change: an old link
   *  (`#/ticket/<id>`, `#/edit/<id>`) becomes the uid form, a stale tail the ticket's now. */
  export function settle(t) {
    if (typeof location === "undefined") return;
    const want = settled(location.hash, t);
    if (want) history.replaceState(history.state, "", want);
  }
</script>

<script>
  let { id, uid = null, title = null } = $props();
</script>

<a class="ticket-link" data-id={id} href={ticketHref({ id, uid, title })}>{id}</a>
