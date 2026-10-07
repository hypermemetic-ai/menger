// Auto-generated RPC client interface
// This is Layer 1: raw RPC that returns PlexusStreamItem

import type { PlexusStreamItem } from './types';
import { PlexusError, errorFromStreamItem } from './types';

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
 * Throws PlexusError (or a typed subclass — ForbiddenError for "-32003",
 * UnauthenticatedError for "-32001") on error events.
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
        throw errorFromStreamItem(item);
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
 * Throws PlexusError (or a typed subclass — ForbiddenError for "-32003",
 * UnauthenticatedError for "-32001") on error events.
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
        throw errorFromStreamItem(item);
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
