// Auto-generated RPC client interface
// This is Layer 1: raw RPC that returns PlexusStreamItem

import type { PlexusStreamItem, StreamMetadata } from './types';
import { PlexusError, errorFromStreamItem } from './types';

/**
 * Raw RPC client interface for service communication.
 *
 * This is the low-level transport layer. All methods return AsyncGenerator<PlexusStreamItem>.
 * Use the typed client wrappers for a better developer experience.
 */
export interface RpcClient {
  /**
   * Call a method and receive a stream of PlexusStreamItem responses.
   *
   * @param method - Fully qualified method name (e.g., "echo.once", "cone.chat")
   * @param params - Method parameters as a JSON-serializable object
   * @returns AsyncGenerator yielding PlexusStreamItem events
   */
  call(method: string, params?: unknown): AsyncGenerator<PlexusStreamItem>;
}

/**
 * Convert snake_case string to camelCase.
 * Used to transform backend field names to TypeScript conventions.
 */
function toCamelCase(str: string): string {
  return str.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase());
}

/**
 * Recursively transform all object keys from snake_case to camelCase.
 * This allows TypeScript types to use idiomatic camelCase while the backend sends snake_case.
 * Similar to Rust's #[serde(rename_all = "camelCase")].
 */
function transformKeys(obj: unknown): unknown {
  if (obj === null || obj === undefined) return obj;
  if (typeof obj !== 'object') return obj;
  if (Array.isArray(obj)) return obj.map(transformKeys);

  const result: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(obj)) {
    const camelKey = toCamelCase(key);
    result[camelKey] = transformKeys(value);
  }
  return result;
}

/**
 * Helper to extract data content from a PlexusStreamItem stream.
 * Throws a RemoteCallError on an error item (RETURNS-17) — still the
 * PlexusError subclass CA-2 / AUTH-9 name for it (Forbidden, ForbiddenError,
 * UnauthenticatedError).
 * Automatically transforms response field names from snake_case to camelCase.
 *
 * @param stream - AsyncGenerator of PlexusStreamItem
 * @returns AsyncGenerator of the unwrapped content (typed as T)
 */
export async function* extractData<T>(
  stream: AsyncGenerator<PlexusStreamItem>
): AsyncGenerator<T> {
  for await (const item of stream) {
    switch (item.type) {
      case 'data':
        yield transformKeys(item.content) as T;
        break;
      case 'error':
        throw remoteCallError(undefined, item);
      case 'progress':
        // Progress events are informational, skip
        break;
      case 'done':
        // Stream completed
        return;
    }
  }
}

/**
 * Helper to collect a single result from a non-streaming method.
 * Throws a RemoteCallError on an error item (RETURNS-17) — still the
 * PlexusError subclass CA-2 / AUTH-9 name for it (Forbidden, ForbiddenError,
 * UnauthenticatedError).
 * Throws if no data is received.
 * Automatically transforms response field names from snake_case to camelCase.
 *
 * @param stream - AsyncGenerator of PlexusStreamItem
 * @returns Promise of the unwrapped content (typed as T)
 */
export async function collectOne<T>(
  stream: AsyncGenerator<PlexusStreamItem>
): Promise<T> {
  for await (const item of stream) {
    switch (item.type) {
      case 'data':
        return transformKeys(item.content) as T;
      case 'error':
        throw remoteCallError(undefined, item);
      case 'progress':
        // Progress events are informational, skip
        break;
      case 'done':
        break;
    }
  }
  throw new Error('No data received from method call');
}

// Re-export error types for convenience
export { PlexusError, ForbiddenError, UnauthenticatedError } from './types';

// ============================================================================
// RETURNS-17: the typed call — MethodStream, RemoteCallError, Result
// ============================================================================

/** A Result item, as the wire sends it (RETURNS-1): passed through as it arrives. */
export type Result<T, E> = { ok: T } | { err: E };

/** The `ok` half of a Result item. */
export function isOk<T, E>(r: Result<T, E>): r is { ok: T } {
  return 'ok' in r;
}

/** The `err` half of a Result item. */
export function isErr<T, E>(r: Result<T, E>): r is { err: E } {
  return 'err' in r;
}

/** What every error item carries (plexus-wire spec/05). */
interface RemoteErrorBase {
  /** The JSON-RPC code in decimal (`"-32003"`), or the kind for a refusal with none (`"too_many_calls"`). */
  readonly code: string;
  /** For people. Never parsed: read `error_kind` and the fields beside it. */
  readonly message: string;
  /** `true` only for `too_many_calls`: close a call and retry. */
  readonly recoverable: boolean;
}

/**
 * The wire's error item, without its `type` and `metadata`: a call refused or
 * failed (spec/05), discriminated by `error_kind`. An item with no
 * `error_kind` is read by its `code` — `"-32601"` (no such activation or
 * method), `"-32000"` (the method failed while executing).
 */
export type RemoteError =
  | (RemoteErrorBase & { readonly error_kind: 'invalid_params' })
  | (RemoteErrorBase & { readonly error_kind: 'unauthenticated' })
  | (RemoteErrorBase & {
      readonly error_kind: 'forbidden';
      /** The grant the call needed (`tickets.create`); absent on a scope-gate denial. */
      readonly grant?: string;
      /** The target's innermost scope (`*` for no target), or the scope a scope gate wanted. */
      readonly scope?: string;
      /** The roles that list the grant, `admin` last. */
      readonly roles?: readonly string[];
      /** Where the caller holds the grant (none: nowhere). */
      readonly held_on?: readonly string[];
      /** A scope gate's denial. */
      readonly reason?: 'missing_scope' | 'tenant_boundary' | 'not_accepted';
    })
  | (RemoteErrorBase & {
      readonly error_kind: 'too_many_calls';
      /** The connection's maximum of open calls. */
      readonly max: number;
    })
  | (RemoteErrorBase & { readonly error_kind?: undefined });

/** Every `error_kind` the wire names. */
export type RemoteErrorKind = NonNullable<RemoteError['error_kind']>;

/** The `forbidden` refusal. */
export type ForbiddenRemoteError = Extract<RemoteError, { readonly error_kind: 'forbidden' }>;

const REMOTE_CALL_ERROR = Symbol.for('plexus.RemoteCallError');

/**
 * What a failed call throws: the server answered its call with an error
 * item. `error` is that item, typed; `error_kind`, `grant`, `scope`, `roles`
 * and `held_on` are its fields, as the wire names them.
 *
 * ```ts
 * try { await client.create(…); }
 * catch (e) {
 *   if (e instanceof RemoteCallError && e.error.error_kind === 'forbidden') ask(e.error.grant, e.error.held_on);
 * }
 * ```
 *
 * It is still the PlexusError subclass CA-2 and AUTH-9 throw for the same
 * item — `Forbidden` (when the IR carries grants), `ForbiddenError`,
 * `UnauthenticatedError`, else `PlexusError` — so a handler written for
 * those still catches it; `instanceof RemoteCallError` holds for each.
 * A failure that never crossed the wire (a socket lost, refused, timed out)
 * is the transport's `TransportError`, never this.
 */
export class RemoteCallError extends PlexusError {
  /** The method's path (`tickets.watch`) for a stream; `undefined` from a one-item call's helper. */
  declare readonly method: string | undefined;
  /** The error item, typed. */
  declare readonly error: RemoteError;
  declare readonly error_kind: RemoteErrorKind | undefined;
  declare readonly grant: string | undefined;
  declare readonly scope: string | undefined;
  declare readonly roles: readonly string[] | undefined;
  declare readonly held_on: readonly string[] | undefined;

  constructor(method: string | undefined, error: RemoteError, metadata?: StreamMetadata) {
    super(error.message, error.code, error.recoverable, metadata);
    this.name = 'RemoteCallError';
    stampRemote(this, method, error);
    Object.setPrototypeOf(this, new.target.prototype);
  }

  /** Every error a failed call throws, whichever PlexusError subclass carries it. */
  static [Symbol.hasInstance](x: unknown): x is RemoteCallError {
    return typeof x === 'object' && x !== null && (x as Record<symbol, unknown>)[REMOTE_CALL_ERROR] === true;
  }
}

function stampRemote(target: object, method: string | undefined, error: RemoteError): void {
  const e = error as RemoteErrorBase & Partial<Record<'error_kind' | 'grant' | 'scope' | 'roles' | 'held_on', unknown>>;
  const fields: Record<string, unknown> = {
    method, error,
    error_kind: e.error_kind, grant: e.grant, scope: e.scope, roles: e.roles, held_on: e.held_on,
  };
  for (const [k, v] of Object.entries(fields)) {
    // a field the AUTH-9 class already holds (its `scope`) keeps its value
    if (!Object.prototype.hasOwnProperty.call(target, k)) {
      Object.defineProperty(target, k, { value: v, enumerable: true });
    }
  }
  Object.defineProperty(target, REMOTE_CALL_ERROR, { value: true });
}

/** The error a call answered by `item` throws: CA-2 / AUTH-9's class for it, stamped a RemoteCallError. */
export function remoteCallError(
  method: string | undefined,
  item: { message: string; code?: string; recoverable: boolean; metadata?: StreamMetadata },
): RemoteCallError {
  const thrown = errorFromStreamItem(item);
  const { type: _type, metadata: _metadata, ...rest } = item as typeof item & { type?: unknown };
  stampRemote(thrown, method, { ...rest, code: item.code ?? '' } as RemoteError);
  return thrown as RemoteCallError;
}

/** The wire's progress item as main's IR carries it: a message and a percentage (RETURNS-15 types it per method). */
export interface Progress {
  readonly message: string;
  readonly percentage?: number;
}

/** One event of a {@link MethodStream}, tagged with its method: what `.events()` yields. */
export interface MethodEvent<P extends string, E> {
  readonly method: P;
  readonly event: E;
}

/** What `.withProgress()` yields: an item, or a progress report, in arrival order. */
export type WithProgress<E> = { readonly item: E } | { readonly progress: Progress };

/**
 * What a generated streaming method returns: its events, decoded, from a
 * stream that knows its method's path (`readonly method: 'tickets.watch'`).
 *
 * - `for await (const ev of client.watch())` yields each event; progress is dropped.
 * - `.events()` yields `{ method, event }`; `.withProgress()` yields `{ item }` or `{ progress }`.
 * - An error item throws {@link RemoteCallError} and ends it; a lost socket throws the transport's `TransportError`.
 * - `watch(a, b, c)` merges several with no path written: each brings its own.
 *
 * It is read once: by `for await` / `next()`, or by one of `.events()` /
 * `.withProgress()`. Breaking out (or `return()`) unsubscribes the call.
 */
export class MethodStream<P extends string, E> implements AsyncIterableIterator<E> {
  /** The method's path, as a literal type. */
  readonly method: P;
  #raw: AsyncGenerator<PlexusStreamItem> | undefined;
  #items: AsyncGenerator<E, undefined> | undefined;

  constructor(method: P, stream: AsyncGenerator<PlexusStreamItem>) {
    this.method = method;
    this.#raw = stream;
  }

  #take(how: string): AsyncGenerator<PlexusStreamItem> {
    const raw = this.#raw;
    if (!raw) {
      throw new TypeError(`${this.method}: ${how} on a stream already being read — a MethodStream is read once`);
    }
    this.#raw = undefined;
    return raw;
  }

  #iterator(): AsyncGenerator<E, undefined> {
    this.#items ??= decodeItems<E>(this.method, this.#take('for await'));
    return this.#items;
  }

  [Symbol.asyncIterator](): MethodStream<P, E> {
    return this;
  }

  next(): Promise<IteratorResult<E, undefined>> {
    return this.#iterator().next();
  }

  return(_value?: unknown): Promise<IteratorResult<E, undefined>> {
    if (this.#items) return this.#items.return(undefined);
    const raw = this.#raw;
    this.#raw = undefined;
    // never started: no call was made, so there is nothing to unsubscribe
    return (raw ? raw.return(undefined) : Promise.resolve()).then(() => ({ value: undefined, done: true as const }));
  }

  throw(e?: unknown): Promise<IteratorResult<E, undefined>> {
    return this.#iterator().throw(e);
  }

  /** Each event tagged with this stream's method: `{ method: 'tickets.watch', event }`. */
  events(): AsyncGenerator<MethodEvent<P, E>, undefined> {
    return tagEvents<P, E>(this.method, decodeItems<E>(this.method, this.#take('.events()')));
  }

  /** Items and the wire's progress reports, in arrival order. */
  withProgress(): AsyncGenerator<WithProgress<E>, undefined> {
    return decodeWithProgress<E>(this.method, this.#take('.withProgress()'));
  }
}

async function* decodeItems<E>(method: string, stream: AsyncGenerator<PlexusStreamItem>): AsyncGenerator<E, undefined> {
  for await (const item of stream) {
    switch (item.type) {
      case 'data':
        yield transformKeys(item.content) as E;
        break;
      case 'error':
        throw remoteCallError(method, item);
      case 'done':
        return undefined;
      default:
        // progress: dropped (.withProgress() keeps it); request: the transport answers it
        break;
    }
  }
  return undefined;
}

async function* tagEvents<P extends string, E>(method: P, items: AsyncGenerator<E, undefined>): AsyncGenerator<MethodEvent<P, E>, undefined> {
  for await (const event of items) yield { method, event };
  return undefined;
}

async function* decodeWithProgress<E>(method: string, stream: AsyncGenerator<PlexusStreamItem>): AsyncGenerator<WithProgress<E>, undefined> {
  for await (const item of stream) {
    switch (item.type) {
      case 'data':
        yield { item: transformKeys(item.content) as E };
        break;
      case 'progress':
        yield { progress: item.percentage === undefined ? { message: item.message } : { message: item.message, percentage: item.percentage } };
        break;
      case 'error':
        throw remoteCallError(method, item);
      case 'done':
        return undefined;
      default:
        break;
    }
  }
  return undefined;
}

// ============================================================================
// IR-9: Typed-handle runtime primitives for DynamicChild methods
// ============================================================================
//
// Methods tagged MethodRole::DynamicChild generate a typed handle on the
// parent client that exposes .get(name) plus the opt-in Listable / Searchable
// capabilities. The typed-handle form gives compile-time errors when callers
// invoke a capability (e.g., .search) that wasn't opted in via the IR's
// list_method / search_method fields — instead of runtime errors.

/**
 * A handle to a dynamic child activation: the child's TYPE is known at codegen
 * time, but the specific child INSTANCE's name is runtime data.
 *
 * @typeParam T - The child's generated client class.
 */
export interface DynamicChild<T> {
  /** Resolve a child activation by name. Returns null if the child doesn't exist. */
  get(name: string): Promise<T | null>;
}

/**
 * Capability interface: the dynamic-child gate can enumerate available names.
 * Mixed in via intersection when the IR's `list_method` is `Some`.
 */
export interface Listable {
  list(): AsyncIterable<string>;
}

/**
 * Capability interface: the dynamic-child gate can search available names.
 * Mixed in via intersection when the IR's `search_method` is `Some`.
 */
export interface Searchable {
  search(query: string): AsyncIterable<string>;
}

/**
 * Configuration for makeDynamicChild.
 *
 * @internal This shape is stable within a generated artifact but is not
 * intended for hand-written consumer code — the generator emits the call.
 */
export interface DynamicChildConfig<T> {
  listMethod: string | null;
  searchMethod: string | null;
  childClient: new (rpc: RpcClient) => T;
}

/**
 * Build a typed-handle instance for a DynamicChild method.
 *
 * The returned object always satisfies DynamicChild<T>. When `listMethod`
 * is non-null, it also exposes `.list()`; when `searchMethod` is non-null,
 * it also exposes `.search(query)`. The intersection type at the call site
 * (e.g., `DynamicChild<T> & Listable`) ensures callers can only invoke
 * capabilities that were opted in at codegen time.
 */
export function makeDynamicChild<T>(
  rpc: RpcClient,
  parentNamespace: string,
  methodName: string,
  config: DynamicChildConfig<T>,
): DynamicChild<T> & Partial<Listable & Searchable> {
  const handle: DynamicChild<T> & Partial<Listable & Searchable> = {
    async get(name: string): Promise<T | null> {
      const fullPath = `${parentNamespace}.${methodName}`;
      try {
        // Dynamic children pass `name` as the sole positional argument.
        const stream = rpc.call(fullPath, { name });
        const resolved = await collectOne<unknown>(stream);
        if (resolved === null || resolved === undefined) return null;
        // The child activation is addressable via the same RPC client; the
        // child class is constructed over the same transport.
        return new config.childClient(rpc);
      } catch (e) {
        if (e instanceof PlexusError && e.recoverable === false) throw e;
        return null;
      }
    },
  };

  if (config.listMethod !== null) {
    const listPath = `${parentNamespace}.${config.listMethod}`;
    handle.list = async function* (): AsyncIterable<string> {
      const stream = rpc.call(listPath, {});
      yield* extractData<string>(stream);
    };
  }

  if (config.searchMethod !== null) {
    const searchPath = `${parentNamespace}.${config.searchMethod}`;
    handle.search = async function* (query: string): AsyncIterable<string> {
      const stream = rpc.call(searchPath, { query });
      yield* extractData<string>(stream);
    };
  }

  return handle;
}
