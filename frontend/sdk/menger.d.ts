/**
 * menger SDK 1.6 — the types of `window.menger` and of a component script's
 * `this`, inside every framed body (COMP-4, COMP-5, COMP-10, COMP-9,
 * INPUT-4, INPUT-9). 1.5 (COMMENT-3) changes nothing here: annotation mode
 * is the host's, spoken over the frame's own messages, and a component's
 * badge and info card show in that mode only. Nor does 1.6 (COMP-12): the
 * base typography moves into a cascade layer, and a body's
 * `<meta name="menger-base" content="markdown">` confines it to markdown.
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
   * `menger.tickets.inputs(undefined, undefined, undefined, menger.input.ticket)`
   * (the typed client's parameters are in name order: `epic`, `filter`,
   * `state`, `ticket`).
   * `null` in a frame that renders no ticket's stored body (an editor's
   * preview, a component's help card), where `submit` always rejects.
   */
  readonly ticket: string | null;

  /**
   * Answer the input `id` of this frame's ticket — `tickets.answer`, made
   * by the host with source `app` — and resolve to the answer recorded
   * (the typed client's spelling: `atUnix`). The change also reaches every
   * frame as an `input_changed` on `on('change')`.
   *
   * Sent only inside a viewer's gesture — call it from a click (or key)
   * handler: the frame's transient user activation
   * (`navigator.userActivation.isActive`), or, in a browser without it, a
   * trusted click or key in the last 5 s. Otherwise it rejects and sends
   * nothing. The host then rejects it, with the reason and nothing
   * recorded, unless its own window is activated too, the input is declared
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
   * frame's ticket — `tickets.clear_answer`, made by the host with source
   * `app` — and resolve to the input as the clear left it (the typed
   * client's spelling): `open`, no answer, the `cleared` entry last in its
   * history. One that held no answer resolves as it is (nothing written).
   * The change also reaches every frame as an `input_changed` on
   * `on('change')`.
   *
   * Sent only inside a viewer's gesture, as `submit` is; otherwise it
   * rejects and sends nothing. The host then rejects it, with the reason
   * and nothing cleared, unless its own window is activated too and the
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
export interface Menger {
  /** "1.6.0". */
  readonly version: string;

  /**
   * COMP-10: the `tickets` activation, typed — every method, with the
   * activation's parameters in the generated client's order, each an
   * `AsyncGenerator<TicketsEvent>`:
   *
   * ```js
   * for await (const ev of menger.tickets.waves(['RETURNS'])) {
   *   if (ev.type === 'wave_counts') count = ev.waves[0] ?? 0;
   * }
   * const items = await Array.fromAsync(menger.tickets.components());
   * ```
   *
   * Answered by the app over its own connection, read-only: `get`, `list`,
   * `find`, `waves`, `groups`, `extents`, `blockers`, `health`,
   * `components`, `inputs` (INPUT-4 — `inputs(epic?, state?, ticket?)`,
   * one `{ type: 'input', input }` per input, its answer and history
   * included). Every other method is present and throws a `PlexusError`
   * whose message starts "read-only" when iterated — `answer` and
   * `clearAnswer` too: a frame answers through `input.submit` and clears
   * through `input.clear`.
   */
  readonly tickets: TicketsClient;

  /**
   * The untyped escape hatch: `method` by its bare wire name (`waves`) with
   * the activation's parameter names (`{ epics: ['PLEXUS'] }`) → the call's
   * items, as the wire spells them. Read-only, as `tickets` — `inputs`
   * with `{ ticket?, epic?, state? }`; a method outside the reads rejects
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
