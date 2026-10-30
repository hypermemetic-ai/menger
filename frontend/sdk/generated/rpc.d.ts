import type { PlexusStreamItem, StreamMetadata } from './types';
import { PlexusError } from './types';
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
 * Helper to extract data content from a PlexusStreamItem stream.
 * Throws a RemoteCallError on an error item (RETURNS-17) — still the
 * PlexusError subclass CA-2 / AUTH-9 name for it (Forbidden, ForbiddenError,
 * UnauthenticatedError).
 * Automatically transforms response field names from snake_case to camelCase.
 *
 * @param stream - AsyncGenerator of PlexusStreamItem
 * @returns AsyncGenerator of the unwrapped content (typed as T)
 */
export declare function extractData<T>(stream: AsyncGenerator<PlexusStreamItem>): AsyncGenerator<T>;
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
export declare function collectOne<T>(stream: AsyncGenerator<PlexusStreamItem>): Promise<T>;
export { PlexusError, ForbiddenError, UnauthenticatedError } from './types';
/** A Result item, as the wire sends it (RETURNS-1): passed through as it arrives. */
export type Result<T, E> = {
    ok: T;
} | {
    err: E;
};
/** The `ok` half of a Result item. */
export declare function isOk<T, E>(r: Result<T, E>): r is {
    ok: T;
};
/** The `err` half of a Result item. */
export declare function isErr<T, E>(r: Result<T, E>): r is {
    err: E;
};
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
export type RemoteError = (RemoteErrorBase & {
    readonly error_kind: 'invalid_params';
}) | (RemoteErrorBase & {
    readonly error_kind: 'unauthenticated';
}) | (RemoteErrorBase & {
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
}) | (RemoteErrorBase & {
    readonly error_kind: 'too_many_calls';
    /** The connection's maximum of open calls. */
    readonly max: number;
}) | (RemoteErrorBase & {
    readonly error_kind?: undefined;
});
/** Every `error_kind` the wire names. */
export type RemoteErrorKind = NonNullable<RemoteError['error_kind']>;
/** The `forbidden` refusal. */
export type ForbiddenRemoteError = Extract<RemoteError, {
    readonly error_kind: 'forbidden';
}>;
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
export declare class RemoteCallError extends PlexusError {
    /** The method's path (`tickets.watch`) for a stream; `undefined` from a one-item call's helper. */
    readonly method: string | undefined;
    /** The error item, typed. */
    readonly error: RemoteError;
    readonly error_kind: RemoteErrorKind | undefined;
    readonly grant: string | undefined;
    readonly scope: string | undefined;
    readonly roles: readonly string[] | undefined;
    readonly held_on: readonly string[] | undefined;
    constructor(method: string | undefined, error: RemoteError, metadata?: StreamMetadata);
    /** Every error a failed call throws, whichever PlexusError subclass carries it. */
    static [Symbol.hasInstance](x: unknown): x is RemoteCallError;
}
/** The error a call answered by `item` throws: CA-2 / AUTH-9's class for it, stamped a RemoteCallError. */
export declare function remoteCallError(method: string | undefined, item: {
    message: string;
    code?: string;
    recoverable: boolean;
    metadata?: StreamMetadata;
}): RemoteCallError;
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
export type WithProgress<E> = {
    readonly item: E;
} | {
    readonly progress: Progress;
};
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
export declare class MethodStream<P extends string, E> implements AsyncIterableIterator<E> {
    #private;
    /** The method's path, as a literal type. */
    readonly method: P;
    constructor(method: P, stream: AsyncGenerator<PlexusStreamItem>);
    [Symbol.asyncIterator](): MethodStream<P, E>;
    next(): Promise<IteratorResult<E, undefined>>;
    return(_value?: unknown): Promise<IteratorResult<E, undefined>>;
    throw(e?: unknown): Promise<IteratorResult<E, undefined>>;
    /** Each event tagged with this stream's method: `{ method: 'tickets.watch', event }`. */
    events(): AsyncGenerator<MethodEvent<P, E>, undefined>;
    /** Items and the wire's progress reports, in arrival order. */
    withProgress(): AsyncGenerator<WithProgress<E>, undefined>;
}
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
export declare function makeDynamicChild<T>(rpc: RpcClient, parentNamespace: string, methodName: string, config: DynamicChildConfig<T>): DynamicChild<T> & Partial<Listable & Searchable>;
