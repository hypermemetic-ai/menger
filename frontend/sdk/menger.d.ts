/**
 * menger SDK 2.0 — the types of `window.menger` and of a component script's
 * `this`, inside every framed body (COMP-4, COMP-5, COMP-10, COMP-9,
 * INPUT-4, INPUT-9). 1.5 (COMMENT-3) changes nothing here: annotation mode
 * is the host's, spoken over the frame's own messages, and a component's
 * badge and info card show in that mode only. Nor does 1.6 (COMP-12): the
 * base typography moves into a cascade layer, and a body's
 * `<meta name="menger-base" content="markdown">` confines it to markdown.
 * 1.7 (COMP-14) adds `input.displayOnly` and the built-in `<m-cite>`, a
 * placeholder the host draws a cited window over (no script surface).
 * 1.8 (MENGER-14) changes nothing here either: a link clicked in the body is
 * handed to the host (`menger:link`), which follows it, asks before it
 * leaves menger, or refuses it.
 * 1.12 (MENGER-34) neither: a ticket chip right-clicked in the body (an
 * `<m-ticket ref>`, or anything carrying `data-ticket-id`) opens the app's
 * ticket menu (`menger:ticket-menu`); plain text keeps the browser's.
 * 1.17 (FILTER-25) neither: Alt+click on such a chip sets the app's global
 * filter to its shortcode (`menger:filter-everywhere`).
 * 2.0 (ACCESS-20) moves the reads onto the `tickets` children's paths:
 * `menger.tickets.work.waves`, `.work.blockers`, `.nav.groups`,
 * `.nav.extents`, `.components.list`, `.inputs.list`, and `menger.call`
 * names them the same (`'work.waves'`, …); the 1.x names are refused,
 * naming the new one.
 *
 * Published by the app at /app/sdk/menger.d.ts, beside the SDK itself
 * (/app/sdk/menger-sdk.js) and the generated client's declarations
 * (/app/sdk/generated/…). Hand-written: the SDK's own surface. What it
 * says about the tickets activation is not written here — `tickets` is the
 * client hub-codegen generates from the activation's schema
 * (scripts/sdk-codegen.sh), typed by its own `.d.ts`.
 */

import type { TicketsClient } from './generated/tickets/client';
import type { Answer, Input, InputValue } from './generated/tickets/types';

export type { TicketsClient } from './generated/tickets/client';
export type { TicketsInputsClient } from './generated/tickets/inputs/client';
export type { TicketsWorkClient } from './generated/tickets/work/client';
export type { TicketsNavClient } from './generated/tickets/nav/client';
export type { TicketsComponentsClient } from './generated/tickets/components/client';
export type { TicketsCommitsClient } from './generated/tickets/commits/client';

/**
 * The typed `tickets` a frame gets: the generated client as generated, its
 * children its typed sub-clients — `work`, `nav`, `inputs`, `components`,
 * `commits` (ACCESS-14..18). SDK 2.0 (ACCESS-20) keeps no alias at the old
 * names: `menger.tickets.waves(…)` is `menger.tickets.work.waves(…)`,
 * `menger.tickets.inputs(…)` is `menger.tickets.inputs.list(…)`,
 * `menger.tickets.components()` is `menger.tickets.components.list()`.
 */
export type FrameTickets = TicketsClient;
export type * from './generated/tickets/types';

/**
 * One data event of a call or of the change stream, as the wire spells it —
 * `{ type: "placed", summary, placement, waiting_on }`, an in-band
 * `{ type: "error", message }` included. Untyped: `menger.call` and
 * `menger.on` are the escape hatch. (The typed client's events are
 * `TicketsEvent`, whose keys the generated runtime camelCases:
 * `waitingOn`.)
 */
export interface WireEvent {
  type: string;
  [field: string]: unknown;
}

/**
 * INPUT-4: `menger.input` — frozen. A frame answers its own ticket's inputs
 * (`<m-decision id>`, `<m-input id kind>`) and nothing else; `submit` and
 * (INPUT-9, 1.4) `clear` are the two writes it can make.
 */
export interface MengerInput {
  /**
   * The ticket whose stored body this frame renders — what `submit`
   * answers against, and what to read this ticket's inputs with:
   * `menger.tickets.inputs.list(undefined, undefined, undefined, menger.input.ticket)`
   * (the typed client's parameters are in name order: `epic`, `filter`,
   * `state`, `ticket`).
   * `null` in a frame that renders no ticket's stored body (an editor's
   * preview, a component's help card), where `submit` always rejects.
   */
  readonly ticket: string | null;

  /**
   * COMP-14 (1.7): `true` in a cited window's frame (an `<m-cite>`'s
   * window, drawn by the host), where `ticket` is the cited ticket and
   * `submit` and `clear` reject at once — a cited section's inputs are
   * shown, and answered only in their own ticket. A component may grey its
   * controls out on it.
   */
  readonly displayOnly: boolean;

  /**
   * Answer the input `id` of this frame's ticket — `tickets.inputs.answer`,
   * made by the host (ACCESS-14: the server records its source from the
   * signed-in viewer) — and resolve to the answer recorded
   * (the typed client's spelling: `atUnix`). The change also reaches every
   * frame as an `input_changed` on `on('change')`.
   *
   * Sent only inside a viewer's gesture — call it from a click (or key)
   * handler: the frame's transient user activation
   * (`navigator.userActivation.isActive`), or, in a browser without it, a
   * trusted click or key in the last 5 s. Otherwise it rejects and sends
   * nothing. The host then rejects it, with the reason and nothing
   * recorded, unless it sees the gesture on this very frame too (IDENTITY-36: not just any click in the app; a browser without `navigator.userActivation` is refused), the input is declared
   * in the ticket's body and neither closed nor removed, and `value`
   * answers it — `{ type: 'choice', option }` one of its options,
   * `{ type: 'multi', options }` any of them, `{ type: 'text', text }` not
   * blank.
   *
   * ```js
   * button.onclick = () =>
   *   menger.input.submit('landing', { type: 'choice', option: 'archive' }, 'holds are real')
   *     .then((answer) => show(answer.value), (e) => complain(e.message));
   * ```
   */
  submit(id: string, value: InputValue, rationale?: string | null): Promise<Answer>;

  /**
   * INPUT-9 (1.4): clear the answer recorded on the input `id` of this
   * frame's ticket — `tickets.inputs.clear`, made by the host (its source
   * the server's, as for `submit`) — and resolve to the input as the clear left it (the typed
   * client's spelling): `open`, no answer, the `cleared` entry last in its
   * history. One that held no answer resolves as it is (nothing written).
   * The change also reaches every frame as an `input_changed` on
   * `on('change')`.
   *
   * Sent only inside a viewer's gesture, as `submit` is; otherwise it
   * rejects and sends nothing. The host then rejects it, with the reason
   * and nothing cleared, unless it sees the gesture on this very frame too and the
   * input is declared in the ticket's body and neither closed nor removed.
   * An app before 1.4 has no `clear`: test for it
   * (`typeof menger.input.clear === 'function'`).
   *
   * ```js
   * clearButton.onclick = () =>
   *   menger.input.clear('landing').then((input) => show(input.history), (e) => complain(e.message));
   * ```
   */
  clear(id: string): Promise<Input>;
}

/** `window.menger` — frozen; also `this.menger` in a component script. */
/** THEME-7: what a frame is painted with — `on('theme')`'s argument, and `menger.theme`. */
export interface MengerTheme {
  /** The painted mode. */
  scheme: 'light' | 'dark';
  /** The theme the frame's region wears (`default`: none). */
  theme: string;
  /** Every token of the contract, as painted (`--accent` → `#d4572a`). */
  tokens: Record<string, string>;
}

export interface Menger {
  /** "2.4.0". */
  readonly version: string;

  /**
   * COMP-10: the `tickets` activation, typed — every method, with the
   * activation's parameters in the generated client's order, each an
   * `AsyncGenerator<TicketsEvent>`:
   *
   * ```js
   * for await (const ev of menger.tickets.work.waves(['RETURNS'])) {
   *   if (ev.type === 'wave_counts') count = ev.waves[0] ?? 0;
   * }
   * const items = await Array.fromAsync(menger.tickets.components.list());
   * ```
   *
   * A child's methods are its sub-client's (`tickets.nav.groups` on the
   * wire is `menger.tickets.nav.groups()`).
   *
   * Answered by the app over its own connection, read-only: `get`, `list`,
   * `find`, `health`, `work.waves`, `work.blockers`, `nav.groups`,
   * `nav.extents`, `components.list`, `inputs.list` (INPUT-4 —
   * `inputs.list(epic?, filter?, state?, ticket?)`, one
   * `{ type: 'input', input }` per input, its answer and history
   * included). Every other method is present and
   * throws a `PlexusError` whose message starts "read-only" when iterated
   * — `inputs.answer` and `inputs.clear` too: a frame answers through
   * `input.submit` and clears through `input.clear`.
   */
  readonly tickets: FrameTickets;

  /**
   * The untyped escape hatch: `method` by its path under `tickets`
   * (`get`, `work.waves`; SDK 2.0, ACCESS-20 — a 1.x name such as `waves`
   * is refused naming its path) with the activation's parameter names
   * (`{ epics: ['PLEXUS'] }`) → the call's items, as the wire spells them.
   * Read-only, as `tickets` — `inputs.list` with `{ ticket?, epic?, state? }`; a method outside the reads rejects
   * with "read-only: …" (naming `input.submit` and `input.clear` as the
   * two writes), a
   * parameter the method does not take rejects naming it.
   */
  call(method: string, params?: Record<string, unknown> | null): Promise<WireEvent[]>;

  /**
   * Every `watch` event the app hears (`amended`, `status_changed`,
   * `component_changed`, `input_changed`, …), in the wire's spelling. Returns the function
   * that stops it; one made while a component script runs also stops
   * before that instance's next render.
   */
  on(kind: 'change', listener: (event: WireEvent) => void): () => void;

  /**
   * THEME-7 (1.13): whenever what this frame is painted with moves — the
   * light/dark mode, the theme its region wears (a reader's pick, an
   * amended ```theme block, a project entered or left) or any token's
   * value. Called once the new tokens are live on `:root`, never for a
   * repaint that moved nothing: a component that draws with colours in JS
   * redraws here. Returns the function that stops it; one made while a
   * component script runs also stops before that instance's next render.
   */
  on(kind: 'theme', listener: (theme: MengerTheme) => void): () => void;

  /** THEME-7 (1.13): what this frame wears now — a fresh copy each read. */
  readonly theme: MengerTheme;

  /**
   * DESIGN-40 (1.18): what moves now — the app's Motion setting, worn as
   * `html[data-motion]` (followed live), or `none` while the reader's
   * system asks for reduced motion. `calm` drops the decorative moves; a
   * re-rendered body also wears `html[data-entered]` (its entrance moves
   * have played).
   */
  readonly motion: 'full' | 'calm' | 'none';

  /**
   * DESIGN-43 (1.19): how fast code types itself in now — the app's Typed
   * text setting, worn as `html[data-typing]` (followed live; absent is
   * `normal`), or `off` while `motion` is `none`. ART-9's `m-code` types at
   * slow 45 ms, normal 22 ms, fast 8 ms a character; `off` shows it at once.
   * Motion `calm` does not stop typing: this setting alone decides.
   */
  readonly typing: 'off' | 'slow' | 'normal' | 'fast';

  /**
   * COMP-9: move the app to an app route — `#/ticket/<id>` (with
   * `?component=<tag>` to scroll to and flash that docs card),
   * `#/components`, `#/components/<tag>` (a component's help page),
   * `#/waves`, `#/waves/<SHORTCODE>`. The host ignores any other route.
   * (A click on a link to `#/…` in a body does the same.)
   */
  navigate(route: string): void;

  /** INPUT-4, INPUT-9: the two writes — this frame's ticket's inputs, answered and cleared. */
  readonly input: MengerInput;
}

/**
 * `this` in a component's `<script>` (the scripts of a source run as one
 * function, once per render, with the element as `this`). Returning a
 * function registers it to run before the next render and when the
 * element leaves the document.
 */
export interface MengerElement extends HTMLElement {
  /** The instance's open shadow root: the source's styles, then its template. */
  readonly root: ShadowRoot;
  /** The attribute, or null — read this way, a change re-renders the instance. */
  attr(name: string): string | null;
  /** `window.menger` itself. */
  readonly menger: Menger;
}

declare global {
  interface Window {
    readonly menger: Menger;
  }
  const menger: Menger;
}
