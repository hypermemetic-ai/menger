import type { Kind, TicketRef } from '../notes/types';
import type { WatchEnd } from '../themes/types';
/** MNG-16: what an `amend` moved — its title, its body. */
export type AmendedField = "title" | "body";
/** Where a comment was left: the app page, the place on it, what was there
in words, and the ticket the element belongs to (when it belongs to one).

`route`, `path` and `label` are required and not blank; `excerpt` may be
empty (an element with no text). Each is trimmed. They are `#[serde(default)]`
so that one left out arrives as empty and is refused *in band*, naming
it — not as a transport error before the method runs. */
export interface Anchor {
    /** What was there, in words: its text, e.g. `DEMAND-1` (may be empty). */
    excerpt?: string;
    /** What was there, in words: the element's name, e.g. `Parent chip`. */
    label?: string;
    /** Where on the page: the element's path, e.g.
  `[data-tag="ticket DEMAND-1"]`. `list { path }` matches a prefix of it. */
    path?: string;
    /** The app page, as its route: `#/ticket/DEMAND-2`, `#/waves/RETURNS`,
  `#/` (the board) — always beginning `#/`. */
    route?: string;
    /** The ticket the element belongs to, e.g. `DEMAND-2` — omitted when it
  belongs to none. It must name a ticket; kept as that ticket's id. */
    ticket?: string | null;
    /** Set by the server, never by the caller: `ticket`'s `updated_unix`
  when the comment was added — so a reader can tell the ticket has
  changed since (its body may no longer hold `path`). Omitted with no
  `ticket`; a value sent is replaced. */
    version?: number | null;
}
/** INPUT-3: an input's current answer — the value, its rationale (`null`
when none was given), who gave it and when (unix seconds). The latest
`answered` of its history, unless a `cleared` came after it. */
export interface Answer {
    atUnix: number;
    rationale?: string | null;
    source: AnswerSource;
    value: InputValue;
}
/** INPUT-3: who made a change to an input (INPUT-1's language) — `app`, a
viewer's click, which the host says on its `answer`; `api`, everything
else (synapse, an agent). The only "who" until CONTEXT adds identity. */
export type AnswerSource = never;
/** ACCESS-48: a ticket or a shortcode as a call named it, echoed back —
`not_found`'s and `ambiguous`'s `id`: what was asked, by any name
[`TicketRef`] or [`ShortcodeRef`] takes (the rule [`Under`] reads). */
export type Asked = string;
/** COMP-8: one declared attribute — `<attr name="board" type="shortcode"
required>The shortcode to count.</attr>`. `name` is lowercased (HTML
folds attribute names); `kind` is the `type` (`string` when omitted);
`required` is whether the `required` attribute is present;
`description` the element's text, as [`ComponentDocs::summary`] is. */
export interface AttrDoc {
    description: string;
    kind: AttrKind;
    name: string;
    required: boolean;
}
export interface AttrKindString {
    type: 'string';
}
export interface AttrKindNumber {
    type: 'number';
}
export interface AttrKindBoolean {
    type: 'boolean';
}
export interface AttrKindShortcode {
    type: 'shortcode';
}
export interface AttrKindTicket {
    type: 'ticket';
}
export interface AttrKindEnum {
    type: 'enum';
    values: string[];
}
/** COMP-8: what an attribute's value must be — an `<attr type=…>`, written
`string` (the default), `number`, `boolean`, `shortcode` (an existing
shortcode, e.g. `RETURNS`), `ticket` (an existing ticket id) or
`enum(a|b|c)` (one of those words). Internally tagged on the wire, as
[`Placement`] is: `{"type": "shortcode"}`, `{"type": "enum", "values":
["a", "b"]}`. [`FromStr`] reads the written form and [`fmt::Display`]
writes it back. */
export type AttrKind = AttrKindString | AttrKindNumber | AttrKindBoolean | AttrKindShortcode | AttrKindTicket | AttrKindEnum;
export declare function isAttrKindString(e: AttrKind): e is AttrKindString;
export declare function isAttrKindNumber(e: AttrKind): e is AttrKindNumber;
export declare function isAttrKindBoolean(e: AttrKind): e is AttrKindBoolean;
export declare function isAttrKindShortcode(e: AttrKind): e is AttrKindShortcode;
export declare function isAttrKindTicket(e: AttrKind): e is AttrKindTicket;
export declare function isAttrKindEnum(e: AttrKind): e is AttrKindEnum;
/** Where a write comes from: app, api (default) or pipeline (synapse is api, merge pipeline) */
export type CallerSource = "app" | "api" | "pipeline";
/** ORG-3: a shortcode's letters — ASCII letters and digits, upper case (a
code is read case-blind and kept upper case), 1 to [`CODE_MAX`].

Not an identity: two shortcodes of one code live in two namespaces. So
it has no `Hash`, no `Ord` and no `Borrow<str>` — nothing can key a map
or a set by it (the module's examples hold that to the compiler) — and its `Display` is
its letters, never an address. It compares equal to another code of the
same letters, which is what a search over the letters asks; the store
compares shortcodes by [`ShortcodeId`]. */
export type Code = string;
/** One comment, as kept. */
export interface Comment {
    anchor: Anchor;
    /** When it was added, unix seconds. */
    at: number;
    /** Who left it: its name (ACCESS-19: signed by the caller — its
  principal when it has no name; `owner` with login off). */
    author: string;
    /** What it says (markdown, as written). */
    body: string;
    /** Its id: 1, 2, … in the order comments were added, never reused. */
    id: CommentId;
    /** APP-23: `changes` for a change request; omitted for a comment. */
    intent?: CommentIntent;
    /** IDENTITY-21: the Plexus principal of who left it, when they were
  signed in — `author` is only the name they go by. */
    principal?: string | null;
    state: CommentState;
}
/** COMMENT-2: a comment's number, from 1 in the order kept. */
export type CommentId = number;
/** What a comment is: a comment, or a change request (any case) */
export type CommentIntent = "comment" | "changes";
/** Where a comment stands (any case) */
export type CommentState = "open" | "resolved";
/** SDLC-9: a commit — a hexadecimal object name of 7 to 64 characters
(kept lower case), optionally a repository and `@` before it
(`77df9a0`, `plexus-macros@4f7114a`). Nothing is trimmed: an entry is
a reference or it is not. Compared as its text — the server holds
no repository (moved here from `store::commits::reference`). */
export type CommitRef = string;
/** COMP-3: one entry of the **component registry** — a custom element some
ticket defined in a ` ```component <name> ` block (COMP-1 decisions 2, 3,
pins 6–8), as the server keeps it.

- `name` — the tag: a custom element's name, lowercase, with a hyphen
  (`m-wave-summary`).
- `source` — the block's content, exactly as written between its fences
  (a `<template>`, an optional `<style>`, an optional `<script>`).
- `owner` — the ticket whose body defines it; `null` for an **orphan**: a
  component whose definition left its ticket while something still
  references it. An orphan is read-only; a ticket that defines its name
  again **adopts** it.
- `references` — what uses it: each ticket whose body holds `<name` as an
  HTML tag (outside code spans and fences, once per ticket), in the
  store's `(epic, seq)` order, then each component whose source holds it,
  by name.

- `docs` (COMP-8) — the source's `<docs>` element, parsed
  ([`ComponentDocs`]); `null` when the source holds none.
- `preview` (COMP-8) — the attributes written after the tag on the
  block's opening line (` ```component m-x board=RETURNS label="Platform" `),
  `[name, value]` in the order written; a value may be double-quoted, and
  a bare name's value is `""`. What a live preview of the definition is
  rendered with.

Unlike [`Ticket`], `owner` and `docs` are written out as `null` rather
than skipped: every component has the same keys, and `null` is what
"orphan" (or "no docs yet") looks like. */
export interface Component {
    docs?: ComponentDocs | null;
    name: string;
    owner?: string | null;
    preview: unknown[][];
    references: string[];
    source: string;
}
/** COMP-8: a component's help — its source's one `<docs>` element, parsed.

```html
<docs>
  <summary>How many of a board's tickets are workable now.</summary>
  <attr name="board" type="shortcode" required>The shortcode to count.</attr>
  <slot name="title">Optional title markup.</slot>
  <example><m-wave-summary board="RETURNS"></m-wave-summary></example>
  Longer help, as markdown prose.
</docs>
```

- `summary` — the `<summary>`'s text, tags dropped, whitespace collapsed,
  trimmed (`""` when there is none).
- `attrs` — one [`AttrDoc`] per `<attr>`, in the order written.
- `slots` — one [`SlotDoc`] per `<slot>`, in the order written.
- `parts` — THEME-5: one [`PartDoc`] per `<part>`, in the order written:
  what a theme may skin with `m-name::part(name)`. Each names a `part`
  the component's `<template>` carries (or, marked `scripted`, one its
  script makes) — a documented part the template lacks is refused.
- `examples` — each `<example>`'s inner HTML as written, its common
  indentation and the blank lines around it removed.
- `help` — everything else inside `<docs>`, as markdown: the lines the
  elements above stood on removed, the rest de-indented and trimmed. */
export interface ComponentDocs {
    attrs: AttrDoc[];
    examples: string[];
    help: string;
    parts?: PartDoc[];
    slots: SlotDoc[];
    summary: string;
}
/** Where a page starts: strictly after the ticket a `page` event named
as its `next` (`MNG-9`). Opaque to the caller — today a ticket's
key, folded to upper case as the store keeps it. */
export type Cursor = string;
/** What becomes of a deleted ticket's children: reparent or delete */
export type DeleteChildren = "reparent" | "delete";
/** APP-24: how a ticket ended for an agent waiting on it. */
export type Ending = "canceled" | "deleted";
/** FILTER-4: a field `facets` counts by — one of [`FACET_FIELDS`]. */
export type FacetField = string;
/** FILTER-3's language, as text (`ready and kind build and in SCALE`) —
one the language reads (every bad chunk named), at most
[`crate::limits::FILTER_MAX`] bytes, kept as written: the server reads it with its
cells, ANDs the caller's reach on, and answers what it admits.
Blank is a filter that narrows nothing. */
export type FilterText = string;
/** MNG-10: one shortcode's roll-up — the row AGS-20's nav draws for it,
computed by the server so a client can draw the tree without holding
every ticket.

The numbers are the nav's, exactly (`frontend/src/nav.rs`, `plan()` and
the group row's rollup): `total` counts every ticket (SDLC-11: but the
docs) in the shortcode
row's subtree as the nav nests it — its own tickets, a ticket of another
shortcode filed under one of them, and every sub-shortcode hosted
beneath it, at any depth — and `done` counts the `Complete` ones among
them (`Superseded` is not done; the nav's rule). `root` is the
shortcode's lowest-seq `scope` ticket (`None` only for a prefix that
never had one). `host` is set when the shortcode is a **sub-shortcode**
(AGS-20): the ticket, in another shortcode, that its root is filed
under and that its row nests beneath — `None` for a top-level row.

Like [`TicketSummary`], the optional fields are written out as `null`
rather than skipped: every group has the same keys.

SDLC-11: a doc has no completion, so `done/total` is the work alone —
`total` counts every ticket of the subtree but its docs, and a scope's
done-ness ignores doc children. `docs` counts the docs beside it, so a
shortcode that holds only docs (a doc collection, rooted at a doc) is
still a row: a group is drawn when it holds anything ([`Group::drawn`]),
and one that arrives with `total: 0` and `docs: 0` is a row no longer
drawn. `root` is the shortcode's lowest-seq scope — or doc: a doc may
root a shortcode as a scope may.

AGS-23: the nav draws a shortcode that has a root *as* its root ticket's
row, so the group carries what that row shows — `root_row`, the root's
title, status and kind, read from the server's cache — and the top level
still paints from `groups {}` alone, with no `get` per shortcode. It is
kept current the way the counts are: a change to the root's title,
status or kind (SDLC-13: `set_kind`) is a change to its group, and
`group_changed` carries it. `None` exactly when `root` is. */
export interface Group {
    /** ORG-3: the shortcode's full address, `org:ROOT/…/SC`. */
    address?: string;
    /** SDLC-11: the docs in the row's subtree, which `total` leaves out. */
    docs: number;
    done: number;
    /** The shortcode's letters, uppercase. ORG-3: what the row shows; two
  namesakes are told apart by `shortcode` and `address`. */
    epic: Code;
    /** The ticket the shortcode is hosted under, if any. */
    host?: TicketId | null;
    /** ACCESS-48: the root's key. */
    root?: TicketId | null;
    /** AGS-23: what the root's own row shows. */
    rootRow?: RootRow | null;
    /** ORG-3: the shortcode's identity — what a row of the nav is keyed by. */
    shortcode?: ShortcodeId;
    total: number;
}
/** INPUT-3: one **input** — a component in a ticket's body that asks for an
answer (INPUT-1), as the store keeps it: the ticket it is in, its `id`
(unique in that ticket), its kind, what the body declares of it — the
question (`null` without an `<m-question>`), the options in the body's
order, the recommendation (the `<m-recommendation>`'s text, else the
label of the option marked `recommended`; `null` with neither) — and
what has been answered: its state, its current answer (`null` when it
holds none) and its whole history, oldest first.

The declaration follows the body: every write of the body re-reads it.
An answer stays what was recorded when a later body changes the options
it named — the history is a record, never rewritten. */
export interface Input {
    answer?: Answer | null;
    history: InputHistoryEntry[];
    id: InputId;
    kind: InputKind;
    options: InputOption[];
    question?: string | null;
    recommendation?: string | null;
    state: InputState;
    ticket: TicketId;
}
/** INPUT-3: what one entry of an input's history did — an answer recorded
(`answered`), the answer cleared (`cleared`), the input frozen
(`closed`) or unfrozen (`reopened`). */
export type InputAction = never;
/** INPUT-3: one change an input went through, oldest first — the action,
the value and rationale of an `answered` (`null` for the others), who
made it and when (unix seconds). Every change is kept, a removed input's
included. */
export interface InputHistoryEntry {
    action: InputAction;
    atUnix: number;
    rationale?: string | null;
    source: AnswerSource;
    value?: InputValue | null;
}
/** INPUT-1: an input's id — the `id` attribute of its `<m-decision>` or
`<m-input>`, trimmed, unique in its ticket. */
export type InputId = string;
/** INPUT-3: what an **input** asks for (INPUT-1 decision 2, the owner's
fixed set): `choice` — one of its options (an `<m-decision>`); `multi` —
any of its options, a set; `text` — a free reply (both an `<m-input>`).
Lowercase on the wire, as [`Kind`] is. */
export type InputKind = "choice" | "multi" | "text";
/** INPUT-3: one option an input declares — `value` is what an answer names
(an `<m-option>`'s `value`, else its `name`; an `<m-choice>`'s `value`),
`label` what a reader sees (the `name`, else the value; an `<m-choice>`'s
text, else its value), `recommended` whether the option is marked so. */
export interface InputOption {
    label: string;
    recommended: boolean;
    value: string;
}
/** Who answers or clears an input: app (a viewer's click) or api (default) */
export type InputSource = "app" | "api";
/** Where an input stands (any case) */
export type InputState = "open" | "answered" | "closed" | "removed" | "withdrawn";
export interface InputValueChoice {
    type: 'choice';
    option: string;
}
export interface InputValueMulti {
    type: 'multi';
    options: string[];
}
export interface InputValueText {
    type: 'text';
    text: string;
}
/** INPUT-3: an answer's value, typed by its input's kind — internally
tagged, as [`Placement`] is: `{"type": "choice", "option": "archive"}`
(one option's `value`), `{"type": "multi", "options": ["a", "b"]}` (a set
of them — stored in the order the body declares them, each once; it may
be empty: none ticked), `{"type": "text", "text": "…"}` (a reply, not
blank). Through synapse, a param's fields are dotted flags:
`--value.type choice --value.option archive`. */
export type InputValue = InputValueChoice | InputValueMulti | InputValueText;
export declare function isInputValueChoice(e: InputValue): e is InputValueChoice;
export declare function isInputValueMulti(e: InputValue): e is InputValueMulti;
export declare function isInputValueText(e: InputValue): e is InputValueText;
/** MENGER-12: a label — ASCII letters, digits and `-`, 1 to 32, kept
lower case (`Needs-You` is `needs-you`): [`crate::labels::normalize`]. */
export type Label = string;
/** SEARCH-2: a stretch of a search hit's title or passage, in order — `hit`
when it is a word the query matched, which a client marks; the stretches
of one title or passage, joined, are its text. */
export interface Marked {
    hit: boolean;
    text: string;
}
/** WAVE-10: a path inside a shortcode, `_MENGER/…`, exactly as written
— what `create --path` gives a doc, and what `SHORTCODE/_MENGER/…`
finds it by. */
export type MengerPath = string;
/** MENGER-13: why — a transition's message, an answer's rationale, a
tombstone's reason — trimmed, at most [`MESSAGE_MAX`] bytes. Blank is
no message. */
export type Message = string;
/** THEME-5: one declared part — `<part name="label">The field's
label.</part>` — a piece of the component's shadow tree that a theme may
skin with `m-name::part(label)` (looks only, THEME-1's `skins`).
`scripted`: the component's script makes it (`el.part = …`), so the
template is not asked for it. */
export interface PartDoc {
    description: string;
    name: string;
    scripted?: boolean;
}
export interface PlacementWave {
    type: 'wave';
    n: number;
}
export interface PlacementDone {
    type: 'done';
}
export interface PlacementMissingBlocker {
    type: 'missing_blocker';
}
/** WAVE-4: where a ticket sits in the order of work — what `waves` says of
each ticket it places, derived from `blocked_by` over the whole graph by
MNG-4's rule (a blocker is satisfied iff it is a `Complete`,
`Superseded` or `Canceled` ticket; an id naming no ticket never is). */
export type Placement = PlacementWave | PlacementDone | PlacementMissingBlocker;
export declare function isPlacementWave(e: Placement): e is PlacementWave;
export declare function isPlacementDone(e: Placement): e is PlacementDone;
export declare function isPlacementMissingBlocker(e: Placement): e is PlacementMissingBlocker;
/** WAVE-6: one ticket whose kept placement a write moved — its id and the
placement it has now (a `placements_changed` carries one per ticket). */
export interface PlacementChange {
    id: string;
    placement: Placement;
}
/** What one queue write did — a [`QueueRecord`]'s action. */
export type QueueAction = "queued" | "raised" | "requeued" | "snoozed" | "unqueued" | "closed" | "changes_requested";
/** A ticket in the queue, as it stands. */
export interface QueueEntry {
    /** When it was last queued (a requeue moves it; a snooze does not). */
    askedUnix: number;
    /** Who queued it last: a principal (`anonymous` with login off). */
    by: string;
    /** The input on the ticket whose answer is what is asked — required at
  `blocking`; closing it takes the ticket out of the queue. */
    input?: string | null;
    /** Their display name, when they have one. */
    name?: string | null;
    /** When it was first queued — "oldest" in the queue's order. */
    queuedUnix: number;
    /** Why the owner should look. */
    reason: string;
    /** Out of the alerting surfaces until then; `None`: not snoozed. */
    snoozedUntilUnix?: number | null;
    source: TransitionSource;
    ticket: string;
    urgency: Urgency;
}
/** One line of a ticket's queue history — what `history` yields beside its
transitions: who queued it, why and at what urgency; who snoozed it and
until when; who took it out. */
export interface QueueRecord {
    action: QueueAction;
    actor: string;
    atUnix: number;
    /** APP-23: a change request's comment (its id). */
    comment?: number | null;
    /** The queue log's order (its own, not the transitions'). */
    id: number;
    input?: string | null;
    name?: string | null;
    reason?: string | null;
    source: TransitionSource;
    ticket: string;
    /** A snooze's end. */
    untilUnix?: number | null;
    /** The urgency the write left (queued, raised, requeued, snoozed), or
  the one it had (unqueued, closed). */
    urgency: Urgency;
}
/** MENGER-39: why a ticket is in front of the owner — a [`Message`]'s
rule, and not blank. */
export type Reason = string;
/** APP-24: what answered an agent waiting on a ticket (`tickets.await_reply`)
— the first response on it since the call's `since`. */
export type Reply = never;
/** AGS-23: a shortcode root's row as the nav draws it at the top of its
shortcode — the three fields of the root ticket a row shows beside its
id ([`Group::root`]). */
export interface RootRow {
    kind: Kind;
    status: Status;
    title: string;
}
/** SEARCH-2: one word of a `search`, as the query was read — and the
index's words it also matched, being one or two edits away (a typo).
`also` is empty for a word the index holds — as itself, or as the start
of its words — in more than a few tickets: such a word is never
corrected (one it holds in a few, only to far commoner words). A phrase (`"…"`, or a word joined by `-`, `_`, `.`,
`/`) is one entry, its words joined by spaces, never corrected. */
export interface SearchWord {
    also: string[];
    word: string;
}
/** How bad a gap is (any case) */
export type Severity = "Critical" | "High" | "Medium" | "Low";
/** A severity to set, or none to clear it */
export type SeverityChange = "Critical" | "High" | "Medium" | "Low" | "none";
/** A shortcode's identity, minted by the store: its code for a shortcode made before ORG-3, CODE.N for a namesake made after */
export type ShortcodeId = string;
/** ACT-6: one ticket a `reshortcode` moved — the id it had and the id it
has. The old one keeps naming it (the store's pins). */
export interface ShortcodeMove {
    from: string;
    to: string;
}
/** COMP-8: one declared slot — `<slot name="title">Optional title
markup.</slot>`; `name` is `null` for the default slot. */
export interface SlotDoc {
    description: string;
    name?: string | null;
}
/** A ticket's status — the canonical spellings; `active` (Coding) and `in review` are read too, any case */
export type Status = "Pending" | "Ready" | "Coding" | "In Review" | "Blocked" | "Complete" | "Idea" | "Epic" | "Superseded" | "Canceled" | "Doc";
export interface Ticket {
    /** ORG-3: the ticket's full address, `org:ROOT/…/SC-N`
  (`hypermemetic:MENGER/THEME-3`) — what tells two `THEME-3`s apart,
  and what a link may name it by. */
    address?: string;
    /** Ticket ids that must complete before this one starts. */
    blockedBy: TicketName[];
    /** Markdown body (Problem / Required behavior / Acceptance criteria …). */
    body: string;
    /** SCALE-4: how many tickets name this one as their `parent` — derived
  and kept by the store. It replaces AGS-19's `children`, the list of
  every child id, which made a shortcode root's row (every ticket of
  the shortcode hangs under it, MNG-14) as large as the shortcode. The
  children themselves come by page: `list { parent }`. */
    childCount: number;
    /** SDLC-9: the commits that finished this ticket, in the order they
  were linked (`tickets.link_commits`) — each a commit reference, a
  hexadecimal object name of 7 to 64 characters stored lowercase,
  optionally `repository@` before it. `[]` when none. Recorded, not
  verified: the server has no repository. At most 256 (SCALE-4's
  lesson — a list on a row is bounded). */
    commits: CommitRef[];
    createdUnix: number;
    /** The shortcode's letters, upper case (e.g. `MNG`). ORG-3: what a row
  shows — two shortcodes of one code are told apart by `shortcode`
  and `address`, never by this. */
    epic: Code;
    /** ACCESS-10: how many of `blocked_by` the reader may not see — dropped
  from the list and counted here, so readiness stays honest. Omitted
  (0) for a reader who sees them all. */
    hiddenBlockedBy?: number;
    /** ACCESS-10: how many of `unlocks` the reader may not see, as
  `hidden_blocked_by`. */
    hiddenUnlocks?: number;
    /** `EPIC-N`, allocated by the store. ORG-3: the ticket's key — opaque
  and immutable ([`crate::TicketId`]): today's id for today's tickets,
  its shortcode's id, `-` and its number for a newborn. */
    id: TicketId;
    kind: Kind;
    /** MENGER-12: the ticket's labels — free-form tags (`needs-you`, …),
  each lower-case letters, digits and `-` ([`crate::labels`]), sorted,
  each once, at most 16; `[]` when none. Moved by `tickets.label`
  alone, and asked by the filter cell `label <name>`. `default`: a
  server from before MENGER-12 sends none. */
    labels?: Label[];
    /** ACT-6: the ids this ticket had before a `reshortcode` moved it,
  sorted — each still names it (`get THEME-9` answers LOOK-3). Filled
  by `get` alone; omitted on the wire when there is none, and never on
  a summary (the row is the cache's, and an alias is not one of its
  fields). */
    movedFrom?: string[];
    /** AGS-19: the ticket this one is an output of, if any. Any epic; the
  parent must exist at creation and the chain is acyclic. */
    parent?: TicketId | null;
    /** ACCESS-10: the parent is one the reader may not see — `parent` is
  then none. Omitted (false) otherwise. */
    parentHidden?: boolean;
    /** WAVE-10: the ticket's **path address** inside its shortcode, if it
  has one — `<SHORTCODE>/<path>` names it (`get { id:
  "PLEXUS/_MENGER/board" }`), and no other ticket of the shortcode
  holds the same path. An address, not an id: the ticket keeps its
  `EPIC-N`, and the store's order, extents, paging and readiness never
  read it. Only the reserved prefix `_MENGER/` is accepted, on a doc
  filed under its shortcode's scope root — menger's own documents
  (WAVE-1 decision 11). Omitted on the wire when there is none. */
    path?: string | null;
    /** WAVE-6: where the ticket sits in the order of work ([`Placement`]),
  kept by the store — equal at every moment to WAVE-4's rule over the
  whole `blocked_by` graph, moved by the write that moves it (a birth
  that resolves a forward reference, a status crossing `Complete` /
  `Superseded` / `Canceled`), and announced by `placements_changed`. `None` —
  omitted on the wire — for a doc, which has no place in the order of
  work. */
    placement?: Placement | null;
    /** Sequence number within the epic (the `N` in `EPIC-N`). */
    seq: number;
    severity?: Severity | null;
    /** ORG-3: the shortcode's identity ([`ShortcodeId`]) — what to key a
  ticket's shortcode by. The code itself for a shortcode made before
  ORG-3, `CODE.N` for a namesake. */
    shortcode?: ShortcodeId;
    status: Status;
    title: string;
    /** Ticket ids that can start once this one completes. */
    unlocks: TicketName[];
    updatedUnix: number;
}
/** ORG-3: a ticket's key — the immutable, opaque identity a ticket keeps
for good (ORG-6 finding 1): today's id text for today's tickets,
[`ShortcodeId::key`] for a newborn. What every row that names a ticket
names it by; an address resolves *to* one. Its JSON is the id string, as
it always was. */
export type TicketId = string;
/** ACCESS-48: a ticket as a `blocked_by` / `unlocks` list records it —
its key ([`TicketId`]) when it is a ticket, or, for a forward
reference (a blocker not born yet) and a dangling one (a blocker
deleted, or never born), the name as the write recorded it: the key
it will take (`MNG-9`), or the address as written folded
(`MENGER/THEME-12`), which a birth of that name settles into its key
(`unlocks_changed`). Minted by the store alone
(`Store::name_ticket`); the JSON is the text, as it always was. */
export type TicketName = string;
/** MNG-9: a ticket row without its body — every field of [`Ticket`]
except `body`.

The body is ~70 % of a ticket's weight (PERF-4: 98 MiB of `list` at
100,000 tickets, against ~30 MiB of everything-but-bodies), and a
board, a nav or a picker shows none of it — they show what is here.
A consumer that needs one body calls `get` for that one ticket.

Unlike `Ticket`, `severity` and `parent` are written out as `null`
rather than skipped: a row shape a client destructures should have
the same keys in every row. */
export interface TicketSummary {
    /** ORG-3: as on [`Ticket`]. */
    address?: string;
    blockedBy: TicketName[];
    childCount: number;
    /** SDLC-9: as on [`Ticket`] — on the row, so the board, the nav and
  the ticket page all have it without a `get`. */
    commits: CommitRef[];
    createdUnix: number;
    epic: Code;
    /** ACCESS-10: as on [`Ticket`]. */
    hiddenBlockedBy?: number;
    /** ACCESS-10: as on [`Ticket`]. */
    hiddenUnlocks?: number;
    id: TicketId;
    kind: Kind;
    /** MENGER-12: as on [`Ticket`] — on the row, so the board, the nav and
  Home's "Needs you" draw a ticket's label chips without a `get`. */
    labels?: Label[];
    parent?: TicketId | null;
    /** ACCESS-10: as on [`Ticket`] — omitted unless the reader may not see
  the parent. */
    parentHidden?: boolean;
    /** WAVE-10: as on [`Ticket`] — on the row, so the nav can hide
  `_MENGER/` documents without a `get`. Unlike `severity` and
  `parent`, omitted when there is none (the ticket's contract): every
  ticket but menger's own documents would carry a `null`. */
    path?: string | null;
    /** WAVE-6: as on [`Ticket`] — the kept placement, on the row, so the
  board, the nav and a body's `{wave: ID}` have it without a `get`;
  omitted for a doc. */
    placement?: Placement | null;
    seq: number;
    severity?: Severity | null;
    /** ORG-3: as on [`Ticket`]. */
    shortcode?: ShortcodeId;
    status: Status;
    title: string;
    unlocks: TicketName[];
    updatedUnix: number;
}
/** Heartbeat. */
export interface TicketsEventHealth {
    type: 'health';
    ticketCount: number;
    unixNow: number;
    version: string;
}
/** A ticket was created (yielded by `create`, and by `watch` on every creation). */
export interface TicketsEventCreated {
    type: 'created';
    ticket: Ticket;
}
/** One ticket, as it exists now (yielded by `get`, and per-row by
`list` and `ready_set`). */
export interface TicketsEventTicket {
    type: 'ticket';
    ticket: Ticket;
}
/** MNG-9: one ticket row without its body — what `list` and
`ready_set` yield per row instead of `ticket` when the call asked
for `summary`, and what `find` always yields (SCALE-6). */
export interface TicketsEventSummary {
    type: 'summary';
    summary: TicketSummary;
}
/** MNG-9: where a `list` / `ready_set` / `find` walk stands, yielded once as
the last data event of the call. `next` is the cursor to pass as
the next call's `after` — `null` when nothing more matches, which
is how a client knows the walk is over — and `remaining` counts
the matches beyond this page under the same filters.

FILTER-4: `first` is the position of the page's first row among the
matches, 0-based — for an empty page, the position it would start
at — given by `list` when it was asked a `filter` or an `at`, and
omitted (not null) otherwise, so every other page is byte for byte
what it was. A position plus the page is how a windowed client
knows where the rows it holds sit. */
export interface TicketsEventPage {
    type: 'page';
    first: number | null;
    next: string | null;
    remaining: number;
}
/** A ticket's status changed (yielded by `set_status`, and by `watch`). */
export interface TicketsEventStatusChanged {
    type: 'status_changed';
    previous: Status;
    ticket: Ticket;
}
/** SDLC-13: a ticket's kind changed (yielded by `set_kind`, and by
`watch`) — and its status with it, when the change crossed the doc
line or the call gave one; `previous` is the kind it had. The whole
ticket rides along, its new placement included, so applying it is
one upsert, as `status_changed` is. First of its write's events: the
`group_changed`, `placements_changed` and (on a filtered watch)
`entered` / `left` it caused follow, as they follow a status change.
`set_kind`'s own answer to a call that changed nothing has `previous`
equal to the ticket's kind; `watch` never carries that one. */
export interface TicketsEventKindChanged {
    type: 'kind_changed';
    previous: Kind;
    ticket: Ticket;
}
/** MNG-16: a ticket's title and/or body was replaced (yielded by
`amend`, and by `watch` for every amend that changed something).
`changed` names what moved — `"title"`, `"body"`, both in that
order, or nothing: `amend`'s own answer to a no-op, which `watch`
never carries. The full ticket rides along, body included, so a
watch-only client — and an open ticket page — needs no `get`. */
export interface TicketsEventAmended {
    type: 'amended';
    changed: AmendedField[];
    ticket: Ticket;
}
/** SDLC-9: commits were linked to a ticket (yielded by `link_commits`,
and by `watch` for every link that changed something). `added` is
what was newly linked, in the order given — the ticket's `commits`
end with it — or nothing: `link_commits`' own answer when every
entry was already linked, which `watch` never carries. The whole
ticket rides along, as with every write after birth. */
export interface TicketsEventCommitsLinked {
    type: 'commits_linked';
    added: CommitRef[];
    ticket: Ticket;
}
/** SDLC-9: commits were unlinked from a ticket (yielded by
`unlink_commits`, and by `watch` for every unlink that changed
something). `removed` is what came off, in the order given; empty
when none of it was linked, which `watch` never carries. */
export interface TicketsEventCommitsUnlinked {
    type: 'commits_unlinked';
    removed: CommitRef[];
    ticket: Ticket;
}
/** MENGER-12: a ticket's labels moved (yielded by `label`, and by
`watch` for every `label` that changed something). `added` and
`removed` are what moved, in the order given — both empty is
`label`'s own answer to a call that moved nothing, which `watch`
never carries. The whole ticket rides along, its `labels` as the
write left them. */
export interface TicketsEventLabelsChanged {
    type: 'labels_changed';
    added: Label[];
    removed: Label[];
    ticket: Ticket;
}
/** An existing ticket's `unlocks` gained `added`: a newly created
ticket named it in `blocked_by` (yielded by `watch`). Carries the
full updated ticket so a watch-only client stays coherent. */
export interface TicketsEventUnlocksChanged {
    type: 'unlocks_changed';
    added: TicketName;
    ticket: Ticket;
}
/** MENGER-30: an existing ticket's `unlocks` lost `removed`, which no
longer waits on it (`update { unblock }`; yielded by `watch`). The
full updated ticket, as `unlocks_changed`. */
export interface TicketsEventUnlocksRemoved {
    type: 'unlocks_removed';
    removed: TicketName;
    ticket: Ticket;
}
/** MENGER-30: a ticket's `blocked_by` moved after birth (`update`;
yielded by `watch`) — `added` and `removed` as they moved. The whole
ticket rides along; each blocker's side follows, `unlocks_changed` or
`unlocks_removed`, then the placements it moved. */
export interface TicketsEventBlockersChanged {
    type: 'blockers_changed';
    added: TicketName[];
    removed: TicketName[];
    ticket: Ticket;
}
/** MENGER-30: a ticket's severity moved (`update`; yielded by `watch`);
`previous` the one it had (`null`: none). The whole ticket rides along. */
export interface TicketsEventSeverityChanged {
    type: 'severity_changed';
    previous: Severity | null;
    ticket: Ticket;
}
/** MENGER-30: `update`'s answer — the ticket as the write left it, and
the aspects that moved, in this order: `title`, `body`, `kind`,
`status`, `parent`, `severity`, `blocked_by`. Empty when nothing
would change, which wrote nothing; `watch` never carries it (it
carries each aspect's own event). */
export interface TicketsEventUpdated {
    type: 'updated';
    changed: string[];
    ticket: Ticket;
}
/** ACT-2: `update_many`'s answer, and its write's last event on `watch`
(before a filter's `entered` / `left` / `recount`) — `ids` the tickets
it changed (each said by its own events before this), `changed` the
aspects that moved in any, in `updated`'s order (`labels` last). The
answer also carries every ticket selected as the write left it, in
the selection's order (`tickets`), and the ids nothing changed for
(`unchanged`); on `watch` both are empty. */
export interface TicketsEventUpdatedMany {
    type: 'updated_many';
    changed: string[];
    ids: string[];
    tickets: Ticket[];
    unchanged: string[];
}
/** ACT-5: `delete`'s answer, and its write's event on `watch` — `ids` the
tickets it removed, in the store's order (the selection and, with
`children: delete`, every descendant), each one's tombstone beside it.
`get` on a deleted id answers it too, for that one id. Before it on
`watch`: each moved child's `parent_changed`, each unblocked ticket's
`blockers_changed`, each surviving parent's `child_removed`, each
blocker's `unlocks_removed`; after it the groups and placements that
moved, and a filter's `left`. */
export interface TicketsEventDeleted {
    type: 'deleted';
    ids: TicketId[];
    tombstones: Tombstone[];
}
/** ACT-5: `undelete`'s answer, and its write's event on `watch` — the
tickets brought back (whole), in the store's order, with their
children and blocks put back where nothing has moved them since. */
export interface TicketsEventUndeleted {
    type: 'undeleted';
    ids: TicketId[];
    tickets: Ticket[];
}
/** ACT-6: `reshortcode`'s answer, and its write's event on `watch` —
each moved ticket's old id and new one (`moves`, in the order they
were numbered), and every moved ticket as the write left it
(`tickets`, the same order; a scope `new_scope` made first of all).
An old id names its new ticket from then on. After it on `watch`: the
groups and placements that moved, and a filter's `left` / `entered`. */
export interface TicketsEventReshortcoded {
    type: 'reshortcoded';
    moves: ShortcodeMove[];
    tickets: Ticket[];
}
/** AGS-19: the ticket `id` gained the child `added` — a child was born
naming it as `parent`, or was moved under it by `set_parent`
(yielded by `watch`). SCALE-4: the parent's id and its new
`child_count`, not the parent: the whole parent carried its whole
list of children, 2 MB under a root of 163,530. A client that
mirrors rows sets the count; one that shows the parent's children
adds `added` to what it holds. */
export interface TicketsEventChildrenChanged {
    type: 'children_changed';
    added: string;
    childCount: number;
    id: string;
}
/** MNG-12: a ticket's `parent` was set after birth (yielded by
`set_parent`, and by `watch`); `previous` is the parent it had, if
any. Carries the full child. */
export interface TicketsEventParentChanged {
    type: 'parent_changed';
    previous: string | null;
    ticket: Ticket;
}
/** MNG-12: the ticket `id` lost the child `removed` — that child was
re-parented elsewhere (yielded by `watch`). SCALE-4: the ex-parent's
id and its new `child_count`, as `ChildrenChanged`. */
export interface TicketsEventChildRemoved {
    type: 'child_removed';
    childCount: number;
    id: string;
    removed: string;
}
/** SCALE-5: this `watch` fell more than the store's broadcast holds
behind, and the channel dropped `missed` changes before it could
read them (`missed` ≥ 1: the count the channel reports). Yielded
only by `watch`, exactly once per gap, in place of the changes lost
— whatever its `epic` filter, since the subscriber cannot know
whether a lost change was in its epic — and then the stream goes on
with the oldest change still held. Not an error and not an end:
it means "what you hold may be stale — fetch again what you show". */
export interface TicketsEventLagged {
    type: 'lagged';
    missed: number;
}
/** ACCESS-12: this `watch` ended — its caller, verified again when the
grants moved or its sign-in ended, was refused (`reason`, and why in
words). The last event of the stream. */
export interface TicketsEventWatchClosed {
    type: 'watch_closed';
    message: string;
    reason: WatchEnd;
}
/** MNG-10: one shortcode's roll-up (yielded per shortcode by `groups`,
in shortcode order). */
export interface TicketsEventGroup {
    type: 'group';
    group: Group;
}
/** MNG-10: a shortcode's roll-up moved (yielded by `watch`) — a status
change crossed `Complete`, a ticket was born into its subtree, or a
`set_parent` reshaped it; AGS-23: or its root's row did — the root's
status changed (any change, not only across `Complete`) or `amend`
changed its title. Carries the whole group as it is now; it
follows the ticket events of the same mutation, innermost group
first. A group whose shortcode no longer draws a row of its own
arrives once with `total: 0` (SDLC-11: and `docs: 0`). */
export interface TicketsEventGroupChanged {
    type: 'group_changed';
    group: Group;
}
/** SCALE-16: where one shortcode ends in the store's order — its
highest sequence (yielded per shortcode by `extents`, in shortcode
order). Ids are allocated highest-plus-one and nothing deletes, so
the shortcode holds `last_seq` tickets and its row k is `EPIC-k`;
a database edited by hand may hold a hole, and then this is still
the highest sequence — a page after the row before the hole starts
at the next ticket, while `after` naming the missing id itself is
`list`'s in-band error for a cursor that names no ticket. Not a
roll-up: unlike `Group::total` it counts only the
shortcode's own tickets, and every shortcode that holds one has an
extent whether or not the nav draws it a row. No watch event keeps
it: `created` carries the newborn's `epic` and `seq`, which is its
shortcode's new `last_seq`.
ACT-5: and the sequences below `last_seq` the shortcode no longer
holds — deleted tickets (whose numbers are never reused), or a hand
edit's hole — so a client's arithmetic skips them: the shortcode holds
`last_seq - holes.len()` tickets. Absent when it holds every one.
ORG-3: `epic` the shortcode's letters, `shortcode` its identity —
what a client keys the extent by, and what a row's id is built from
(`{shortcode}-{seq}`, the key it was born with). */
export interface TicketsEventExtent {
    type: 'extent';
    epic: Code;
    holes: number[];
    lastSeq: number;
    shortcode: ShortcodeId;
}
/** FILTER-4: how many tickets of one shortcode match a filter (yielded
per shortcode by `counts`, in shortcode order, only for a shortcode
that holds a match) — the filtered `extents`. A sum over the kept
counts of the cells the filter admits, never a test of a ticket.
ORG-3: `epic` the letters, `shortcode` the identity, as `extent`. */
export interface TicketsEventCount {
    type: 'count';
    epic: Code;
    held: number;
    shortcode: ShortcodeId;
}
/** FILTER-4: one menu count (yielded by `facets`): how many tickets
match `base` and `filter` once `filter`'s own groups for `field` are
set aside, among those whose `field` is `value`. `field` is spelled
as the language spells it (`status`, `category`, `kind`, `severity`,
`blocked`, `ready`, `has commits`, `in`); `value` is canonical — a
status as `set_status` stores it, a kind, a severity or a category in
lower case, `yes` / `no` for the three flags, a shortcode for `in`. */
export interface TicketsEventFacet {
    type: 'facet';
    field: FacetField;
    held: number;
    value: string;
}
/** FILTER-4: a write moved this ticket into a `watch { filter }`'s
filter — born into it, or its status, its blockers' or its commits
moved it there (yielded by `watch { filter }` only, after every other
event of the write).
ORG-3: `epic` the letters, `shortcode` the identity, as `extent`. */
export interface TicketsEventEntered {
    type: 'entered';
    epic: Code;
    id: string;
    seq: number;
    shortcode: ShortcodeId;
}
/** FILTER-4: a write moved this ticket out of a `watch { filter }`'s
filter (yielded by `watch { filter }` only, after every other event
of the write). */
export interface TicketsEventLeft {
    type: 'left';
    epic: Code;
    id: string;
    seq: number;
    shortcode: ShortcodeId;
}
/** FILTER-4: one write moved more than 256 tickets into or out of a
`watch { filter }`'s filter: instead of those `entered` and `left`,
one of these per shortcode it moved, with the filter's count of that
shortcode as the write left it. A client sets its count and asks for
its window again, as after `lagged`. */
export interface TicketsEventRecount {
    type: 'recount';
    epic: Code;
    held: number;
    shortcode: ShortcodeId;
}
/** WAVE-4: one ticket's place in the order of work (yielded per ticket
by `waves`): its row, its [`Placement`] — computed over the whole
`blocked_by` graph, whatever shortcodes were asked for — and
`waiting_on`, its unsatisfied direct blockers (ids that name no
ticket included) in `blocked_by` order, distinct; empty for `Done`. */
export interface TicketsEventPlaced {
    type: 'placed';
    placement: Placement;
    summary: TicketSummary;
    waitingOn: TicketName[];
}
/** WAVE-4: the tally that closes a `waves` answer, exactly once, after
every `placed`: `waves[i]` is how many of the placed tickets sit in
wave `i + 1` (as long as the highest wave placed; `[]` when none is),
`missing` how many are `missing_blocker`, `done` how many are done. */
export interface TicketsEventWaveCounts {
    type: 'wave_counts';
    done: number;
    missing: number;
    waves: number[];
}
/** MENGER-31: a `blocked_by` cycle among what a `waves_to` target waits
on (yielded after the `placed`, before the `wave_counts`, one per
cycle): its ids in the store's order. Every ticket on it — and the
target behind it — is `missing_blocker`: no status change ends it. */
export interface TicketsEventWaveCycle {
    type: 'wave_cycle';
    ids: TicketId[];
}
/** WAVE-6: the kept placements one write moved (yielded by `watch`) —
one event per write that moved any, after that write's other events,
each change the ticket's id and its placement now: the ticket a
status change crossed `Complete` / `Superseded` with, and every
ticket downstream of it along `unlocks` whose placement changed; or,
for a birth that resolved forward references, the tickets that were
waiting on its id and theirs (the newborn's own is on its `created`).
`watch { epic }` keeps only the changes in that shortcode, and the
event only when one is left. */
export interface TicketsEventPlacementsChanged {
    type: 'placements_changed';
    changes: PlacementChange[];
}
/** COMP-3: one entry of the component registry (yielded per component by
`components`, in name order) — orphans included (`owner: null`). */
export interface TicketsEventComponent {
    type: 'component';
    component: Component;
}
/** COMP-3: a component was registered, or its source, owner or
references moved (yielded by `watch`) — a `create` or `amend` whose
body gained, changed or lost a ` ```component ` block or a `<name`
tag. Carries the whole component as it is now, so applying it is one
upsert; one per component the write moved, in name order, after that
write's other events. A component belongs to no shortcode — any
ticket may use it — so `watch { epic }` passes it on whatever the
filter. */
export interface TicketsEventComponentChanged {
    type: 'component_changed';
    component: Component;
}
/** COMP-3: a component left the registry (yielded by `watch`) — an
orphan whose last reference went, in the write that took it. After
the write's `component_changed` events; past any `epic` filter, as
they are. */
export interface TicketsEventComponentRemoved {
    type: 'component_removed';
    name: string;
}
/** INPUT-3: one input, as it is now (yielded per input by `inputs`, in
the store's order of tickets and the body's order inside one — a
removed input after the ones its body declares; and by a
`clear_answer`, `close_input` or `reopen_input` that changed nothing). */
export interface TicketsEventInput {
    type: 'input';
    input: Input;
}
/** INPUT-3: an input changed (yielded by `answer`, `clear_answer`,
`close_input` and `reopen_input`, and by `watch`) — answered, cleared,
closed, reopened; or, by a `create` or `amend` of its ticket's body,
declared, re-declared (its kind, question, options or recommendation),
removed from the body, or declared again. Carries the whole input —
its state, its answer and its history — so applying it is one upsert;
after every other event of its write. `watch { epic }` filters it by
its ticket's shortcode. */
export interface TicketsEventInputChanged {
    type: 'input_changed';
    input: Input;
}
/** One resolved blocker of the queried ticket, at its current status
(yielded per-entry by `blockers`). */
export interface TicketsEventBlocker {
    type: 'blocker';
    ticket: Ticket;
}
/** A blocker id that names no existing ticket — a dangling forward
reference (yielded per-entry by `blockers`). */
export interface TicketsEventDanglingBlocker {
    type: 'dangling_blocker';
    id: TicketName;
}
/** FILTER-15: whether a filter admits one ticket (yielded by `member`,
one per id asked, in the order asked): `id` as it was asked,
`matched` whether the filter admits the ticket it names — `false`
for an id that names no ticket. One lookup in the filter's admitted
cells, never a test of the ticket. */
export interface TicketsEventMember {
    type: 'member';
    id: TicketRef;
    matched: boolean;
}
/** Lookup miss. */
export interface TicketsEventNotFound {
    type: 'not_found';
    id: Asked;
}
/** ORG-3: `id` (an address as it was asked — a ticket or a shortcode)
names more than one thing in the reader's reach: `candidates` are
each one's full address, in the store's order. Nothing was read or
written; name one of them in full, or by a short form only it has. */
export interface TicketsEventAmbiguous {
    type: 'ambiguous';
    candidates: string[];
    id: Asked;
}
/** In-band error (bad enum value, invalid epic, blocked_by cycle, …).
A *data* event: the call succeeded and the activation said no,
which is not the same thing as a transport or remote failure. */
export interface TicketsEventError {
    type: 'error';
    message: string;
}
/** MENGER-15: a `create` or `amend` refused for its body's size — the
body was `bytes` bytes, past the server's cap, `max`
(`--max-body-bytes`, 8 MiB by default); `id` the amended ticket
(`None` for a `create`, which allocated no id). Typed, so a client can
say so without reading `message`; `message` says it in words. Nothing
was written. */
export interface TicketsEventBodyTooLarge {
    type: 'body_too_large';
    bytes: number;
    id: string | null;
    max: number;
    message: string;
}
/** FILTER-13: one row of `tree` — a parent's child, or a shortcode's top
row, that matches the filter or holds a match below it: `matched`
whether it matches itself, `below` how many matches are strictly
under it (its whole subtree, whatever the nav shows). A row that only
holds matches has `matched: false` and `below` above 0. Yielded in
the store's order, then one `page`. */
export interface TicketsEventNode {
    type: 'node';
    below: number;
    matched: boolean;
    summary: TicketSummary;
}
/** SEARCH-2: one ticket a `search` matched, best first — its row; its
title and a passage of its body's text (at most 24 words, around the
matches), each as stretches with the matched words marked; and
`corrected` when it matched only through a word's correction (those
follow every ticket matched without one). */
export interface TicketsEventHit {
    type: 'hit';
    corrected: boolean;
    snippet: Marked[];
    summary: TicketSummary;
    title: Marked[];
}
/** SEARCH-2: the last data event of a `search` — `matched`, how many
tickets match (under its filter); `next`, the `offset` of the next
page, or null when this page holds the last match; `words`, the
query's words as they were read, with their corrections. */
export interface TicketsEventSearched {
    type: 'searched';
    matched: number;
    next: number | null;
    words: SearchWord[];
}
/** MENGER-13: one transition (yielded per transition by `history`: the
ticket's own, oldest first, then those they caused in other tickets;
and by `watch`, right after the event of the write that made it — a
`created`, `status_changed` or `kind_changed` — so a cascade is its
cause's events, then each canceled ticket's `status_changed` and
`transition`, in the store's order). */
export interface TicketsEventTransition {
    type: 'transition';
    transition: Transition;
}
/** MENGER-13: the last event of `history` — when the log began
(`began_unix`: nothing before it has a history; `null` for a log no
file began), and, for a canceled ticket, `restorable`: the tickets its
latest cancel canceled and nothing has moved since, which `set_status
{ restore_children: true }` would put back, in the store's order
(empty otherwise). FILTER-27: and since when the ticket has held its
current status (`status_since_unix`) — its latest status transition,
`status_since_exact: true`; or, for a ticket the log says nothing of
(from before it began), a bound — "status since at least …": its last
update or the log's beginning, whichever is earlier — `false`. */
export interface TicketsEventHistoryEnd {
    type: 'history_end';
    beganUnix: number | null;
    /** ACT-5: the deletes that touched this ticket — each tombstone whose
  delete moved it off its parent (`reparented`) or dropped its block
  on the deleted ticket (`unblocked`), oldest first: why its parent
  or its blockers moved. Absent when none did. */
    deletions: Tombstone[];
    restorable: string[];
    statusSinceExact: boolean | null;
    statusSinceUnix: number | null;
}
/** MENGER-39: a ticket put in front of the owner, or queued again
(`action`: `queued`, `raised` — a higher urgency —, `requeued` — the
urgency kept) — `queue`'s answer, and on `watch`; the entry as the
write leaves it and its ticket's row. */
export interface TicketsEventQueued {
    type: 'queued';
    action: QueueAction;
    entry: QueueEntry;
    ticket: TicketSummary;
}
/** MENGER-39: a ticket out of the queue — `unqueue`'s answer, and on
`watch`; `action` `unqueued`, or `closed` (the input the entry named
was closed); the entry as it was, who took it out, when. */
export interface TicketsEventUnqueued {
    type: 'unqueued';
    action: QueueAction;
    atUnix: number;
    by: string;
    entry: QueueEntry;
    id: string;
}
/** MENGER-39: a queued ticket snoozed until its entry's
`snoozed_until_unix` — `snooze`'s answer, and on `watch`. */
export interface TicketsEventSnoozed {
    type: 'snoozed';
    entry: QueueEntry;
    ticket: TicketSummary;
}
/** APP-23: `request_changes`'s answer — the change request kept as a
comment on the ticket's conversation (intent `changes`), and the
ticket out of the queue (`entry` as it was, who, when). `watch`
carries the `unqueued` (action `changes_requested`); `comments.watch`
the comment's `added`. */
export interface TicketsEventChangesRequested {
    type: 'changes_requested';
    atUnix: number;
    by: string;
    comment: Comment;
    entry: QueueEntry;
    id: string;
}
/** APP-24: `await_reply`'s first event, at once — the ticket it waits
on (its id as kept) and its queue entry, when it is queued. */
export interface TicketsEventWaiting {
    type: 'waiting';
    queued: QueueEntry | null;
    ticket: string;
}
/** APP-24: `await_reply`'s answer — the first response on the ticket
since `since`; the stream ends after it. */
export interface TicketsEventReplied {
    type: 'replied';
    reply: Reply;
    ticket: string;
}
/** APP-24: `await_reply` waited `timeout_minutes` and nothing came; the
stream ends. */
export interface TicketsEventTimedOut {
    type: 'timed_out';
    ticket: string;
}
/** MENGER-39: `unqueue` of a ticket that is not queued — nothing written. */
export interface TicketsEventNotQueued {
    type: 'not_queued';
    id: string;
}
/** MENGER-39: one row of `queued` — the entry and its ticket's row, most
urgent first, then the oldest. */
export interface TicketsEventQueueRow {
    type: 'queue_row';
    entry: QueueEntry;
    ticket: TicketSummary;
}
/** MENGER-39: the last event of `queued` — how many rows it sent, how
many of them are snoozed, and the server's clock (a snooze's end is
read against it). */
export interface TicketsEventQueueEnd {
    type: 'queue_end';
    count: number;
    nowUnix: number;
    snoozed: number;
}
/** MENGER-39: a line of the ticket's queue history (yielded by `history`
after its transitions, oldest first): who queued it, why and at what
urgency; who snoozed it until when; who took it out. */
export interface TicketsEventQueueRecord {
    type: 'queue_record';
    record: QueueRecord;
}
export type TicketsEvent = TicketsEventHealth | TicketsEventCreated | TicketsEventTicket | TicketsEventSummary | TicketsEventPage | TicketsEventStatusChanged | TicketsEventKindChanged | TicketsEventAmended | TicketsEventCommitsLinked | TicketsEventCommitsUnlinked | TicketsEventLabelsChanged | TicketsEventUnlocksChanged | TicketsEventUnlocksRemoved | TicketsEventBlockersChanged | TicketsEventSeverityChanged | TicketsEventUpdated | TicketsEventUpdatedMany | TicketsEventDeleted | TicketsEventUndeleted | TicketsEventReshortcoded | TicketsEventChildrenChanged | TicketsEventParentChanged | TicketsEventChildRemoved | TicketsEventLagged | TicketsEventWatchClosed | TicketsEventGroup | TicketsEventGroupChanged | TicketsEventExtent | TicketsEventCount | TicketsEventFacet | TicketsEventEntered | TicketsEventLeft | TicketsEventRecount | TicketsEventPlaced | TicketsEventWaveCounts | TicketsEventWaveCycle | TicketsEventPlacementsChanged | TicketsEventComponent | TicketsEventComponentChanged | TicketsEventComponentRemoved | TicketsEventInput | TicketsEventInputChanged | TicketsEventBlocker | TicketsEventDanglingBlocker | TicketsEventMember | TicketsEventNotFound | TicketsEventAmbiguous | TicketsEventError | TicketsEventBodyTooLarge | TicketsEventNode | TicketsEventHit | TicketsEventSearched | TicketsEventTransition | TicketsEventHistoryEnd | TicketsEventQueued | TicketsEventUnqueued | TicketsEventSnoozed | TicketsEventChangesRequested | TicketsEventWaiting | TicketsEventReplied | TicketsEventTimedOut | TicketsEventNotQueued | TicketsEventQueueRow | TicketsEventQueueEnd | TicketsEventQueueRecord;
export declare function isTicketsEventHealth(e: TicketsEvent): e is TicketsEventHealth;
export declare function isTicketsEventCreated(e: TicketsEvent): e is TicketsEventCreated;
export declare function isTicketsEventTicket(e: TicketsEvent): e is TicketsEventTicket;
export declare function isTicketsEventSummary(e: TicketsEvent): e is TicketsEventSummary;
export declare function isTicketsEventPage(e: TicketsEvent): e is TicketsEventPage;
export declare function isTicketsEventStatusChanged(e: TicketsEvent): e is TicketsEventStatusChanged;
export declare function isTicketsEventKindChanged(e: TicketsEvent): e is TicketsEventKindChanged;
export declare function isTicketsEventAmended(e: TicketsEvent): e is TicketsEventAmended;
export declare function isTicketsEventCommitsLinked(e: TicketsEvent): e is TicketsEventCommitsLinked;
export declare function isTicketsEventCommitsUnlinked(e: TicketsEvent): e is TicketsEventCommitsUnlinked;
export declare function isTicketsEventLabelsChanged(e: TicketsEvent): e is TicketsEventLabelsChanged;
export declare function isTicketsEventUnlocksChanged(e: TicketsEvent): e is TicketsEventUnlocksChanged;
export declare function isTicketsEventUnlocksRemoved(e: TicketsEvent): e is TicketsEventUnlocksRemoved;
export declare function isTicketsEventBlockersChanged(e: TicketsEvent): e is TicketsEventBlockersChanged;
export declare function isTicketsEventSeverityChanged(e: TicketsEvent): e is TicketsEventSeverityChanged;
export declare function isTicketsEventUpdated(e: TicketsEvent): e is TicketsEventUpdated;
export declare function isTicketsEventUpdatedMany(e: TicketsEvent): e is TicketsEventUpdatedMany;
export declare function isTicketsEventDeleted(e: TicketsEvent): e is TicketsEventDeleted;
export declare function isTicketsEventUndeleted(e: TicketsEvent): e is TicketsEventUndeleted;
export declare function isTicketsEventReshortcoded(e: TicketsEvent): e is TicketsEventReshortcoded;
export declare function isTicketsEventChildrenChanged(e: TicketsEvent): e is TicketsEventChildrenChanged;
export declare function isTicketsEventParentChanged(e: TicketsEvent): e is TicketsEventParentChanged;
export declare function isTicketsEventChildRemoved(e: TicketsEvent): e is TicketsEventChildRemoved;
export declare function isTicketsEventLagged(e: TicketsEvent): e is TicketsEventLagged;
export declare function isTicketsEventWatchClosed(e: TicketsEvent): e is TicketsEventWatchClosed;
export declare function isTicketsEventGroup(e: TicketsEvent): e is TicketsEventGroup;
export declare function isTicketsEventGroupChanged(e: TicketsEvent): e is TicketsEventGroupChanged;
export declare function isTicketsEventExtent(e: TicketsEvent): e is TicketsEventExtent;
export declare function isTicketsEventCount(e: TicketsEvent): e is TicketsEventCount;
export declare function isTicketsEventFacet(e: TicketsEvent): e is TicketsEventFacet;
export declare function isTicketsEventEntered(e: TicketsEvent): e is TicketsEventEntered;
export declare function isTicketsEventLeft(e: TicketsEvent): e is TicketsEventLeft;
export declare function isTicketsEventRecount(e: TicketsEvent): e is TicketsEventRecount;
export declare function isTicketsEventPlaced(e: TicketsEvent): e is TicketsEventPlaced;
export declare function isTicketsEventWaveCounts(e: TicketsEvent): e is TicketsEventWaveCounts;
export declare function isTicketsEventWaveCycle(e: TicketsEvent): e is TicketsEventWaveCycle;
export declare function isTicketsEventPlacementsChanged(e: TicketsEvent): e is TicketsEventPlacementsChanged;
export declare function isTicketsEventComponent(e: TicketsEvent): e is TicketsEventComponent;
export declare function isTicketsEventComponentChanged(e: TicketsEvent): e is TicketsEventComponentChanged;
export declare function isTicketsEventComponentRemoved(e: TicketsEvent): e is TicketsEventComponentRemoved;
export declare function isTicketsEventInput(e: TicketsEvent): e is TicketsEventInput;
export declare function isTicketsEventInputChanged(e: TicketsEvent): e is TicketsEventInputChanged;
export declare function isTicketsEventBlocker(e: TicketsEvent): e is TicketsEventBlocker;
export declare function isTicketsEventDanglingBlocker(e: TicketsEvent): e is TicketsEventDanglingBlocker;
export declare function isTicketsEventMember(e: TicketsEvent): e is TicketsEventMember;
export declare function isTicketsEventNotFound(e: TicketsEvent): e is TicketsEventNotFound;
export declare function isTicketsEventAmbiguous(e: TicketsEvent): e is TicketsEventAmbiguous;
export declare function isTicketsEventError(e: TicketsEvent): e is TicketsEventError;
export declare function isTicketsEventBodyTooLarge(e: TicketsEvent): e is TicketsEventBodyTooLarge;
export declare function isTicketsEventNode(e: TicketsEvent): e is TicketsEventNode;
export declare function isTicketsEventHit(e: TicketsEvent): e is TicketsEventHit;
export declare function isTicketsEventSearched(e: TicketsEvent): e is TicketsEventSearched;
export declare function isTicketsEventTransition(e: TicketsEvent): e is TicketsEventTransition;
export declare function isTicketsEventHistoryEnd(e: TicketsEvent): e is TicketsEventHistoryEnd;
export declare function isTicketsEventQueued(e: TicketsEvent): e is TicketsEventQueued;
export declare function isTicketsEventUnqueued(e: TicketsEvent): e is TicketsEventUnqueued;
export declare function isTicketsEventSnoozed(e: TicketsEvent): e is TicketsEventSnoozed;
export declare function isTicketsEventChangesRequested(e: TicketsEvent): e is TicketsEventChangesRequested;
export declare function isTicketsEventWaiting(e: TicketsEvent): e is TicketsEventWaiting;
export declare function isTicketsEventReplied(e: TicketsEvent): e is TicketsEventReplied;
export declare function isTicketsEventTimedOut(e: TicketsEvent): e is TicketsEventTimedOut;
export declare function isTicketsEventNotQueued(e: TicketsEvent): e is TicketsEventNotQueued;
export declare function isTicketsEventQueueRow(e: TicketsEvent): e is TicketsEventQueueRow;
export declare function isTicketsEventQueueEnd(e: TicketsEvent): e is TicketsEventQueueEnd;
export declare function isTicketsEventQueueRecord(e: TicketsEvent): e is TicketsEventQueueRecord;
/** ACT-5: what a delete keeps of a ticket — its tombstone. The ticket is
gone (off every read, its number never reused); this says which it was
(`id`, `title`), who deleted it (`by`, the caller's principal; `name` their
display name, when they have one), when (`at_unix`) and why (`message`).
Its history stays in the log. Its body is kept until
`restorable_until_unix` — 30 days — and `undelete` brings it back until
then; after, the body is purged and the field is `null`. `parent` is
where it hung; `reparented`, the children its delete moved to that parent
(`children: reparent`); `unblocked`, the tickets whose block on it the
delete dropped (`unblock`) — what `undelete` puts back. */
export interface Tombstone {
    atUnix: number;
    by: string;
    id: TicketId;
    message?: string | null;
    name?: string | null;
    parent?: TicketId | null;
    reparented?: TicketId[];
    restorableUntilUnix?: number | null;
    title: string;
    unblocked?: TicketId[];
}
/** MENGER-13: one **transition** — a change of one ticket's status or kind,
as the store keeps it, append-only: `id` (the log's own, from 1, in the
order written), the `ticket`, the `field` it moved, `from` → `to` (the
wire spellings: `In Review`, `build`; `from` is `null` at a ticket's
birth), when (`at_unix`), who (`actor`: the caller's principal, `system`
for the store's own, `anonymous` with login off; `name` the caller's
display name, when it has one), where it came from (`source`), why
(`message`, always given for `system`) and what caused it (`cause`: the
transition a cascade carried down, or undid; `null` for a call's own).
The log starts when a file gains it (MENGER-13): a ticket written before
has no history before then. */
export interface Transition {
    actor: string;
    atUnix: number;
    cause?: number | null;
    /** The ticket `cause` is a transition of — the canceled (or restored)
  ticket a cascade carried down from; `null` without a cause. */
    causeTicket?: string | null;
    field: TransitionField;
    from?: string | null;
    id: number;
    message?: string | null;
    name?: string | null;
    source: TransitionSource;
    ticket: TicketId;
    to: string;
}
/** MENGER-13: which of a ticket's fields a transition moved — its status, or
its kind (SDLC-13's `set_kind`). */
export type TransitionField = "status" | "kind";
/** MENGER-13: where a transition came from — `app` (a viewer in the app,
which says so), `api` (synapse, an agent, a script: the default),
`pipeline` (the merge, which says so), `cascade` (the store itself: a
cancellation carried down a canceled ticket's subtree, or undone). */
export type TransitionSource = "app" | "api" | "pipeline" | "cascade";
/** How urgently the owner should look (any case) */
export type Urgency = "whenever" | "today" | "now" | "blocking";
/** MENGER-13: the time zone a history filter's days are read in — an
IANA name (`Europe/Berlin`), `UTC`, or an offset (`+02:00`). Checked
for its spelling here (wasm-safe); a name the server's tzdb does not
hold is the server's in-band refusal. */
export type Zone = string;
