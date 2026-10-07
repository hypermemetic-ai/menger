/**
 * menger SDK 1.1 — the types of `window.menger` and of a component script's
 * `this`, inside every framed body (COMP-4, COMP-5, COMP-10).
 *
 * Published by the app at /app/sdk/menger.d.ts, beside the SDK itself
 * (/app/sdk/menger-sdk.js) and the generated client's declarations
 * (/app/sdk/generated/…). Hand-written: the SDK's own surface. What it
 * says about the tickets activation is not written here — `tickets` is the
 * client hub-codegen generates from the activation's schema
 * (scripts/sdk-codegen.sh), typed by its own `.d.ts`.
 */

import type { TicketsClient } from './generated/tickets/client';

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

/** `window.menger` — frozen; also `this.menger` in a component script. */
export interface Menger {
  /** "1.1.0". */
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
   * `components`. Every other method is present and throws a
   * `PlexusError` whose message starts "read-only" when iterated.
   */
  readonly tickets: TicketsClient;

  /**
   * The untyped escape hatch: `method` by its bare wire name (`waves`) with
   * the activation's parameter names (`{ epics: ['PLEXUS'] }`) → the call's
   * items, as the wire spells them. Read-only, as `tickets`; a method
   * outside the reads rejects with "read-only: …", a parameter the method
   * does not take rejects naming it.
   */
  call(method: string, params?: Record<string, unknown> | null): Promise<WireEvent[]>;

  /**
   * Every `watch` event the app hears (`amended`, `status_changed`,
   * `component_changed`, …), in the wire's spelling. Returns the function
   * that stops it; one made while a component script runs also stops
   * before that instance's next render.
   */
  on(kind: 'change', listener: (event: WireEvent) => void): () => void;
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
