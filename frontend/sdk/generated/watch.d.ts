/**
 * A stream that knows its own path — `readonly method` is the method's path
 * as a literal type (`'chats.watch'`): a generated streaming method's
 * `MethodStream` (./rpc, RETURNS-17), or any `AsyncIterable` that says it.
 * A source of this shape is merged with no path written: `watch(a, b, c)`.
 */
export interface MethodSource<P extends string, E> extends AsyncIterable<E> {
    readonly method: P;
}
/**
 * One source of a merge: a {@link MethodSource}, or a `[path, stream]` pair
 * for a stream that does not carry its path.
 */
export type WatchSource<P extends string = string, E = unknown> = MethodSource<P, E> | readonly [P, AsyncIterable<E>];
/** One item of a merge: which method spoke, and its event, exactly. */
export interface Tagged<P extends string, E> {
    readonly source: P;
    readonly event: E;
}
/**
 * A source that failed: the error it threw (a `PlexusError` for an error
 * item from the server, a transport `Error` otherwise) and its path. The
 * source has ended; the others go on unless the merge stops on errors.
 */
export interface WatchFailure<P extends string> {
    readonly source: P;
    readonly error: Error;
}
/** The tagged item of one source. Distributes over a union of sources. */
export type TaggedOf<S> = S extends readonly [infer P extends string, AsyncIterable<infer E>] ? Tagged<P, E> : S extends MethodSource<infer P, infer E> ? Tagged<P, E> : never;
/** What a merge of `T` yields: an item of one of its sources, or a failure naming it. */
export type WatchItem<T extends Tagged<string, unknown>> = T | WatchFailure<T['source']>;
/** A merged stream: `for await` it, or `close()` it. */
export interface Merged<T extends Tagged<string, unknown>> extends AsyncIterableIterator<WatchItem<T>> {
    /** Each source's path, in the order given. */
    readonly sources: readonly T['source'][];
    /** End the merge now: every subscription still open is unsubscribed, once. */
    close(): Promise<void>;
}
/** How a merge treats a failing source, and what may end it from outside. */
export interface WatchOptions {
    /** `'continue'` (the default): a failure is an item and the others go on. `'stop'`: it is the last item. */
    readonly errors?: 'continue' | 'stop';
    /** Aborting it closes the merge. */
    readonly signal?: AbortSignal;
}
type Sources = readonly [WatchSource<string, unknown>, ...WatchSource<string, unknown>[]];
/**
 * Merge several method streams into one whose item is exactly the union of
 * theirs, tagged by source. At least one; items in the order they arrive;
 * it ends when every source has ended; a failing source is an item and the
 * others go on. Breaking out of `for await`, `close()`, or an abort
 * unsubscribes every subscription still open.
 */
export declare function watch<const S extends Sources>(...sources: S): Merged<TaggedOf<S[number]>>;
/** {@link watch}, with options: `watchWith({ errors: 'stop' }, a, b)`. */
export declare function watchWith<const S extends Sources>(options: WatchOptions, ...sources: S): Merged<TaggedOf<S[number]>>;
/** What a transport registers its cancel with. */
export interface CallScope {
    onCancel(cancel: () => void): void;
}
/** The scope of the call being started, if a merge is starting it. Read synchronously, first, by a transport's `call()`. */
export declare function currentCallScope(): CallScope | undefined;
export {};
