import type { PlexusStreamItem } from './types';
/**
 * Raw RPC client interface for hub communication.
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
 * Throws PlexusError (or a typed subclass — ForbiddenError for "-32003",
 * UnauthenticatedError for "-32001") on error events.
 * Automatically transforms response field names from snake_case to camelCase.
 *
 * @param stream - AsyncGenerator of PlexusStreamItem
 * @returns AsyncGenerator of the unwrapped content (typed as T)
 */
export declare function extractData<T>(stream: AsyncGenerator<PlexusStreamItem>): AsyncGenerator<T>;
/**
 * Helper to collect a single result from a non-streaming method.
 * Throws PlexusError (or a typed subclass — ForbiddenError for "-32003",
 * UnauthenticatedError for "-32001") on error events.
 * Throws if no data is received.
 * Automatically transforms response field names from snake_case to camelCase.
 *
 * @param stream - AsyncGenerator of PlexusStreamItem
 * @returns Promise of the unwrapped content (typed as T)
 */
export declare function collectOne<T>(stream: AsyncGenerator<PlexusStreamItem>): Promise<T>;
export { PlexusError, ForbiddenError, UnauthenticatedError } from './types';
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
