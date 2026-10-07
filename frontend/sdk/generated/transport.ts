// Plexus WebSocket transport
//
// Auth on the Plexus wire happens at the WebSocket UPGRADE: the server's
// CombinedAuthMiddleware accepts a `Cookie:` header (validator disambiguates,
// cookie wins) or `Authorization: Bearer <token>` (RFC-6750 prefix stripped).
// URL-query token construction is intentionally NOT supported — plexus-transport
// removed that path (SAFE-7).
//
// CA-2: attachment is SCHEMA-DIRECTED. The generated METHOD_AUTH registry
// (below) carries each gated method's requirement, including the `siteHint`
// the backend derives from its advertised auth capabilities (CA-1). The
// connection-level site resolves as:
//   config.attachmentSite (explicit override / escape hatch)
//   ?? CONNECTION_SITE_HINT (from the schema)
//   ?? 'cookie:access_token' (the ecosystem's standing convention, logged)
//
// Environment reality:
//   - Node.js: upgrade headers (Cookie / Authorization) are set via the 'ws'
//     options object — both sites work.
//   - Browsers: the WebSocket constructor accepts no headers. Programmatic
//     `credentials` therefore CANNOT be attached; connect() throws a clear
//     error if you try. Browser apps must establish a same-origin cookie
//     session (e.g. a login redirect setting the cookie named by the schema's
//     `cookie:<name>` hint) — the browser then sends it automatically on the
//     upgrade. Public-only usage needs none of this.
//
// Depends on ./types (protocol types) and ./rpc (RpcClient interface + helpers).
import type {
  PlexusStreamItem,
  PlexusStreamItemRequest,
  StandardRequest,
  StandardResponse,
} from './types';
import type { RpcClient } from './rpc';

// ─── CA-2: credential provider ─────────────────────────────────────────────

/** Pluggable credential store (e.g. a Self-store adapter). */
export interface CredentialStore {
  /** Return the credential for `backend`, or null when none is stored. */
  getToken(backend: string): string | null | Promise<string | null>;
}

/** Async (or sync) credential supplier. */
export type CredentialSupplier = () => string | Promise<string>;

/**
 * What client construction accepts as credentials (CA-2):
 * a static token, an async supplier, or a pluggable store.
 */
export type Credentials = string | CredentialSupplier | CredentialStore;

/** Per-method credential-requirement metadata (R-4). Surfacing only — enforcement is server-side. */
export interface MethodAuthMetadata {
  /** Credential the caller must hold. Absent = no scope-derived requirement. */
  readonly requiresCredential?: {
    /** Required credential kind (e.g. 'bearer', 'oauth_access'). Absent = any kind whose scopes match. */
    readonly kind?: string;
    /** Required scope set — the caller must satisfy ALL listed scopes. */
    readonly scopes: readonly string[];
    /** Preferred attach site (advisory), e.g. 'header:authorization'. */
    readonly siteHint?: string;
  };
  /** Explicitly public — exempt from the default-deny gate. */
  readonly public?: boolean;
  /** Declared auth posture of the owning activation. */
  readonly authPosture?: 'required' | 'optional' | 'mixed' | 'none';
}

/**
 * Normalized credential requirements for one method, as returned by
 * `PlexusRpcClient.requires()` (CA-2).
 */
export interface MethodRequirements {
  /** Scopes the caller must satisfy (ALL of them). Empty = no scope-derived requirement. */
  readonly scopes: readonly string[];
  /** Declared auth posture of the owning activation, when advertised. */
  readonly authPosture?: 'required' | 'optional' | 'mixed' | 'none';
  /** Explicitly public — exempt from the default-deny gate. */
  readonly public: boolean;
  /** Schema-advertised attach site, e.g. 'header:authorization'. */
  readonly siteHint?: string;
}

/**
 * Per-method credential requirements, generated from the backend schema
 * (CA-2). Keys are full method paths. Methods absent from this registry
 * advertised no credential surface.
 */
const METHOD_AUTH: { readonly [fullPath: string]: MethodAuthMetadata } = {};

/**
 * Connection-level attachment hint derived from the schema's per-method
 * siteHints (CA-1 derives these from the backend's advertised auth
 * capabilities, so they are uniform per backend). `undefined` when the
 * schema advertises no upgrade-time (header/cookie) site.
 */
const CONNECTION_SITE_HINT: string | undefined = undefined;

/** Fallback attach convention when neither override nor schema names a site. */
const CONVENTION_SITE = 'cookie:access_token';

/** Runtime environment probe — upgrade headers are settable in Node only. */
const IS_NODE: boolean =
  typeof (globalThis as any).process !== 'undefined' &&
  (globalThis as any).process?.versions?.node != null;

/**
 * Thrown CLIENT-SIDE (preflight, no server round-trip) when a gated method
 * is called with no credentials configured (CA-2). Disable via
 * `preflight: false` in the config to send the call anyway.
 */
export class MissingCredentialError extends Error {
  readonly method: string;
  readonly scopes: readonly string[];
  readonly siteHint: string | undefined;
  constructor(method: string, scopes: readonly string[], siteHint?: string) {
    super(
      `${method} requires scope${scopes.length === 1 ? '' : 's'} [${scopes.join(', ')}] ` +
      `but no credentials are configured. Pass \`credentials\` (a token string, an async ` +
      `supplier, or a CredentialStore) when constructing the client` +
      (siteHint ? `; the backend attaches it at '${siteHint}'` : '') +
      `. (Preflight check — no request was sent. Escape hatch: \`preflight: false\`.)`
    );
    this.name = 'MissingCredentialError';
    this.method = method;
    this.scopes = scopes;
    this.siteHint = siteHint;
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

/**
 * Build the WS upgrade headers attaching `token` at `site` (CA-2).
 *
 * - `header:authorization` gets RFC-6750 `Bearer ` prefixing (the server's
 *   middleware strips exactly that prefix); other header names carry the
 *   raw token.
 * - `cookie:<name>` renders a `Cookie: <name>=<token>` header.
 * - `first_frame:` / `in_rpc_param:` are not upgrade-time sites.
 */
function upgradeHeadersFor(site: string, token: string): Record<string, string> {
  if (site.startsWith('header:')) {
    const name = site.slice('header:'.length);
    const value =
      name.toLowerCase() === 'authorization' && !token.startsWith('Bearer ')
        ? `Bearer ${token}`
        : token;
    return { [name]: value };
  }
  if (site.startsWith('cookie:')) {
    const name = site.slice('cookie:'.length);
    return { Cookie: `${name}=${token}` };
  }
  throw new Error(
    `Attachment site '${site}' is not an upgrade-time site. ` +
    `Supported here: 'header:<name>', 'cookie:<name>'.`
  );
}

// ─── WebSocket transport ───────────────────────────────────────────────────

export interface PlexusRpcConfig {
  backend: string;
  url: string;
  connectionTimeout?: number;
  debug?: boolean;
  onBidirectionalRequest?: BidirectionalRequestHandler;
  /**
   * Credentials for gated methods (CA-2): a static token, an async
   * supplier, or a pluggable store. Resolved at connect() time and attached
   * where the schema says (`CONNECTION_SITE_HINT`, override via
   * `attachmentSite`). Node-only — see the browser note in the file header.
   */
  credentials?: Credentials;
  /**
   * Escape hatch: force the attach site ('header:<name>' | 'cookie:<name>'),
   * overriding the schema-derived hint. Use when talking to a backend whose
   * schema predates site_hint emission.
   */
  attachmentSite?: string;
  /**
   * Client-side requirement preflight (CA-2, default true): calling a gated
   * method with no credentials configured throws MissingCredentialError
   * without a server round-trip. Set false to send anyway and let the
   * server decide.
   */
  preflight?: boolean;
  /**
   * JWT auth token to attach as a Cookie on the WebSocket upgrade.
   *
   * @deprecated Legacy SAFE-7 surface — prefer `credentials`. When set (and
   * `credentials` is not), it behaves exactly as before CA-2: attached as
   * `Cookie: access_token=<jwt>` regardless of schema hints.
   */
  authToken?: string;
}

export type BidirectionalRequestHandler = (
  request: StandardRequest
) => Promise<StandardResponse | undefined>;

interface JsonRpcRequest {
  jsonrpc: '2.0';
  id: number;
  method: string;
  params?: unknown;
}

interface JsonRpcSuccess { jsonrpc: '2.0'; id: number; result: unknown; }
interface JsonRpcError   { jsonrpc: '2.0'; id: number; error: { code: number; message: string; data?: unknown }; }
type JsonRpcResponse = JsonRpcSuccess | JsonRpcError;

// ─── Typed RPC errors (REQ-7) ────────────────────────────────────────────
//
// Semantic JSON-RPC error codes per plexus-core's `plexus_error_to_jsonrpc`:
//   -32001  Authentication required
//   -32602  Invalid parameters
//   -32601  Method not found
//   -32000  Server-side execution error
// Transport dispatches to the appropriate subclass so client code can
// `catch (e) { if (e instanceof AuthenticationError) … }` rather than
// string-match on error messages.
//
// NOTE (CA-2): Forbidden (-32003) normally arrives as a STREAM error item on
// the accepted subscription, not as a JSON-RPC error response — the typed
// ForbiddenError for that path lives in ./types and is thrown by the ./rpc
// helpers. The case below covers servers that reject the request itself.

export class PlexusRpcError extends Error {
  readonly code: number;
  readonly data?: unknown;
  constructor(code: number, message: string, data?: unknown) {
    super(`RPC error ${code}: ${message}`);
    this.name = 'PlexusRpcError';
    this.code = code;
    this.data = data;
    Object.setPrototypeOf(this, new.target.prototype);
  }
}

export class AuthenticationError extends PlexusRpcError {
  constructor(message: string, data?: unknown) { super(-32001, message, data); this.name = 'AuthenticationError'; }
}
export class ForbiddenRpcError extends PlexusRpcError {
  /** The unmet scope, when the server's Forbidden message names one. */
  readonly missingScope: string | undefined;
  constructor(message: string, data?: unknown) {
    super(-32003, message, data);
    this.name = 'ForbiddenRpcError';
    const m = /missing required scope '([^']+)'/.exec(message);
    this.missingScope = m ? m[1] : undefined;
  }
}
export class InvalidParamsError extends PlexusRpcError {
  constructor(message: string, data?: unknown) { super(-32602, message, data); this.name = 'InvalidParamsError'; }
}
export class MethodNotFoundError extends PlexusRpcError {
  constructor(message: string, data?: unknown) { super(-32601, message, data); this.name = 'MethodNotFoundError'; }
}
export class ExecutionError extends PlexusRpcError {
  constructor(message: string, data?: unknown) { super(-32000, message, data); this.name = 'ExecutionError'; }
}

/** Construct the appropriate typed error for a JSON-RPC error payload. */
function rpcErrorFor(code: number, message: string, data?: unknown): PlexusRpcError {
  switch (code) {
    case -32001: return new AuthenticationError(message, data);
    case -32003: return new ForbiddenRpcError(message, data);
    case -32602: return new InvalidParamsError(message, data);
    case -32601: return new MethodNotFoundError(message, data);
    case -32000: return new ExecutionError(message, data);
    default:     return new PlexusRpcError(code, message, data);
  }
}

interface JsonRpcNotification {
  jsonrpc: '2.0';
  method: 'subscription';
  params: { subscription: number; result: PlexusStreamItem };
}

interface PendingRequest {
  resolve: (subscriptionId: number) => void;
  reject: (error: Error) => void;
}

interface ActiveSubscription {
  queue: PlexusStreamItem[];
  waiting: ((item: PlexusStreamItem | null) => void) | null;
  done: boolean;
}

export class PlexusRpcClient implements RpcClient {
  private ws: WebSocket | null = null;
  private nextId = 1;
  private pendingRequests = new Map<number, PendingRequest>();
  private subscriptions = new Map<number, ActiveSubscription>();
  private pendingSubscriptionMessages = new Map<number, PlexusStreamItem[]>();
  private config: Omit<Required<PlexusRpcConfig>, 'onBidirectionalRequest' | 'credentials' | 'attachmentSite'>;
  private credentials: Credentials | undefined;
  private attachmentSiteOverride: string | undefined;
  private connectionPromise: Promise<void> | null = null;
  private onBidirectionalRequest?: BidirectionalRequestHandler;

  constructor(config: PlexusRpcConfig) {
    this.config = {
      backend: config.backend,
      url: config.url,
      connectionTimeout: config.connectionTimeout ?? 5000,
      debug: config.debug ?? false,
      preflight: config.preflight ?? true,
      authToken: config.authToken ?? '',
    };
    this.credentials = config.credentials;
    this.attachmentSiteOverride = config.attachmentSite;
    this.onBidirectionalRequest = config.onBidirectionalRequest;
  }

  setBidirectionalHandler(handler: BidirectionalRequestHandler | undefined): void {
    this.onBidirectionalRequest = handler;
  }

  private log(...args: unknown[]): void {
    if (this.config.debug) console.log('[PlexusRpcClient]', ...args);
  }

  /**
   * The credential requirements the backend schema advertises for a method
   * (CA-2). Methods with no advertised surface return the empty requirement
   * (`scopes: []`, `public: false`).
   */
  requires(method: string): MethodRequirements {
    const meta = METHOD_AUTH[method];
    return {
      scopes: meta?.requiresCredential?.scopes ?? [],
      authPosture: meta?.authPosture,
      public: meta?.public ?? false,
      siteHint: meta?.requiresCredential?.siteHint,
    };
  }

  /** Whether any credential source is configured. */
  private hasCredentials(): boolean {
    return this.credentials !== undefined || this.config.authToken !== '';
  }

  /** Resolve the configured credentials to a token, or null when none. */
  private async resolveCredential(): Promise<string | null> {
    const c = this.credentials;
    if (c === undefined) {
      return this.config.authToken !== '' ? this.config.authToken : null;
    }
    if (typeof c === 'string') return c;
    if (typeof c === 'function') return await c();
    return await c.getToken(this.config.backend);
  }

  /**
   * The attach site used at connect time: explicit override, else the
   * schema-derived hint, else the standing convention (logged — never
   * silently). Legacy `authToken` (without `credentials`) pins the SAFE-7
   * cookie convention for byte-compatible behavior.
   */
  private connectAttachmentSite(): string {
    if (this.credentials === undefined && this.config.authToken !== '') {
      return CONVENTION_SITE;
    }
    if (this.attachmentSiteOverride !== undefined) return this.attachmentSiteOverride;
    if (CONNECTION_SITE_HINT !== undefined) return CONNECTION_SITE_HINT;
    this.log(
      `No schema-advertised attachment site and no attachmentSite override — ` +
      `falling back to convention '${CONVENTION_SITE}'`
    );
    return CONVENTION_SITE;
  }

  async connect(): Promise<void> {
    if (this.ws?.readyState === WebSocket.OPEN) return;
    if (this.connectionPromise) return this.connectionPromise;
    this.connectionPromise = this.doConnect();
    try { await this.connectionPromise; } finally { this.connectionPromise = null; }
  }

  private async doConnect(): Promise<void> {
    // CA-2: resolve the credential (possibly async) BEFORE the upgrade, and
    // attach it where the schema says.
    const token = await this.resolveCredential();
    let wsOpts: { headers: Record<string, string> } | undefined;
    if (token !== null && token !== '') {
      if (!IS_NODE) {
        throw new Error(
          'Programmatic credentials cannot be attached in a browser: the WebSocket ' +
          'constructor accepts no upgrade headers, and the server authenticates the ' +
          'UPGRADE (Cookie or Authorization: Bearer). Establish a same-origin cookie ' +
          'session instead (the schema names the cookie via its siteHint), or proxy ' +
          'through a server that can set headers.'
        );
      }
      const site = this.connectAttachmentSite();
      wsOpts = { headers: upgradeHeadersFor(site, token) };
      this.log(`Attaching credential at '${site}' on the WS upgrade`);
    }

    return new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(new Error(`Connection timeout after ${this.config.connectionTimeout}ms`));
      }, this.config.connectionTimeout);

      // In browsers, wsOpts is always undefined here (guarded above) — the
      // native WebSocket constructor only accepts (url, protocols).
      this.ws = wsOpts ? new WebSocket(this.config.url, wsOpts as any) : new WebSocket(this.config.url);
      this.ws.onopen  = () => { clearTimeout(timeout); this.log('Connected to', this.config.url); resolve(); };
      this.ws.onerror = (event) => { clearTimeout(timeout); this.log('WebSocket error:', event); reject(new Error('WebSocket connection failed')); };
      this.ws.onclose = (event) => { this.log('WebSocket closed:', event.code, event.reason); this.handleDisconnect(); };
      this.ws.onmessage = (event) => { this.handleMessage(event.data.toString()); };
    });
  }

  disconnect(): void {
    if (this.ws) { this.ws.close(1000, 'Client disconnect'); this.ws = null; }
    this.handleDisconnect();
  }

  private handleDisconnect(): void {
    for (const [id, pending] of this.pendingRequests) { pending.reject(new Error('Connection closed')); this.pendingRequests.delete(id); }
    for (const [id, sub] of this.subscriptions) { sub.done = true; if (sub.waiting) { sub.waiting(null); sub.waiting = null; } this.subscriptions.delete(id); }
  }

  private handleMessage(data: string): void {
    this.log('Received:', data);
    let msg: unknown;
    try { msg = JSON.parse(data); } catch { this.log('Failed to parse message:', data); return; }
    const obj = msg as Record<string, unknown>;
    if ('method' in obj && !('id' in obj) && obj.params && typeof (obj.params as any).subscription !== 'undefined') {
      this.handleNotification(msg as JsonRpcNotification); return;
    }
    if ('id' in obj) { this.handleResponse(msg as JsonRpcResponse); return; }
    this.log('Unknown message format:', msg);
  }

  private handleResponse(resp: JsonRpcResponse): void {
    const pending = this.pendingRequests.get(resp.id);
    if (!pending) { this.log('Unknown request id:', resp.id); return; }
    this.pendingRequests.delete(resp.id);
    if ('error' in resp) { pending.reject(rpcErrorFor(resp.error.code, resp.error.message, resp.error.data)); }
    else { pending.resolve(resp.result as number); }
  }

  private handleNotification(notif: JsonRpcNotification): void {
    const subscriptionId = notif.params.subscription;
    const item = notif.params.result;
    let sub = this.subscriptions.get(subscriptionId);
    if (!sub) {
      if (!this.pendingSubscriptionMessages.has(subscriptionId)) this.pendingSubscriptionMessages.set(subscriptionId, []);
      this.pendingSubscriptionMessages.get(subscriptionId)!.push(item);
      return;
    }
    if (item.type === 'request') { this.handleBidirectionalRequest(item as PlexusStreamItemRequest); return; }
    if (item.type === 'done' || item.type === 'error') sub.done = true;
    if (sub.waiting) { const w = sub.waiting; sub.waiting = null; w(item); }
    else { sub.queue.push(item); }
    if (sub.done && sub.queue.length === 0) this.subscriptions.delete(subscriptionId);
  }

  private async handleBidirectionalRequest(requestItem: PlexusStreamItemRequest): Promise<void> {
    const { requestId, requestData, timeoutMs } = requestItem;
    if (!this.onBidirectionalRequest) {
      this.log('No bidirectional handler, auto-cancelling:', requestId);
      await this.sendBidirectionalResponse(requestId, { type: 'cancelled' }); return;
    }
    const timeoutPromise = new Promise<undefined>(resolve => setTimeout(() => resolve(undefined), timeoutMs));
    try {
      const response = await Promise.race([this.onBidirectionalRequest(requestData), timeoutPromise]);
      await this.sendBidirectionalResponse(requestId, response ?? { type: 'cancelled' });
    } catch (err) {
      this.log('Bidirectional handler error:', err);
      await this.sendBidirectionalResponse(requestId, { type: 'cancelled' });
    }
  }

  private async sendBidirectionalResponse(requestId: string, response: StandardResponse): Promise<void> {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) { this.log('Cannot send response, not connected'); return; }
    const id = this.nextId++;
    this.ws.send(JSON.stringify({ jsonrpc: '2.0', id, method: `${this.config.backend}.respond`, params: { request_id: requestId, response_data: response } }));
  }

  async *call(method: string, params?: unknown): AsyncGenerator<PlexusStreamItem> {
    // CA-2 preflight: a gated, non-public method with no credentials
    // configured fails fast CLIENT-SIDE, naming the requirement — no server
    // round-trip. Escape hatch: `preflight: false`.
    const meta = METHOD_AUTH[method];
    if (
      this.config.preflight &&
      meta?.requiresCredential &&
      !meta.public &&
      !this.hasCredentials()
    ) {
      throw new MissingCredentialError(
        method,
        meta.requiresCredential.scopes,
        meta.requiresCredential.siteHint
      );
    }

    // CA-2: in_rpc_param sites carry the credential as a named parameter on
    // each call (backends without HTTP cookie/header support).
    let callParams = params ?? {};
    const siteHint = meta?.requiresCredential?.siteHint;
    if (siteHint?.startsWith('in_rpc_param:') && this.hasCredentials()) {
      const paramName = siteHint.slice('in_rpc_param:'.length);
      const token = await this.resolveCredential();
      if (token !== null && typeof callParams === 'object' && !Array.isArray(callParams)) {
        callParams = { ...(callParams as Record<string, unknown>), [paramName]: token };
      }
    }

    await this.connect();
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) throw new Error('Not connected');

    const sub: ActiveSubscription = { queue: [], waiting: null, done: false };
    const id = this.nextId++;
    const request: JsonRpcRequest = {
      jsonrpc: '2.0', id,
      method: `${this.config.backend}.call`,
      params: { method, params: callParams },
    };
    this.log('Sending:', JSON.stringify(request));

    const subscriptionIdPromise = new Promise<number>((resolve, reject) => {
      this.pendingRequests.set(id, { resolve, reject });
    });
    this.ws.send(JSON.stringify(request));

    const subscriptionId = await subscriptionIdPromise;
    this.log('Got subscription ID:', subscriptionId);
    this.subscriptions.set(subscriptionId, sub);

    const pendingMessages = this.pendingSubscriptionMessages.get(subscriptionId);
    if (pendingMessages) {
      this.pendingSubscriptionMessages.delete(subscriptionId);
      for (const msg of pendingMessages) {
        if (msg.type === 'done' || msg.type === 'error') sub.done = true;
        sub.queue.push(msg);
      }
    }

    try {
      while (true) {
        if (sub.queue.length > 0) {
          const item = sub.queue.shift()!;
          yield item;
          if (item.type === 'done' || item.type === 'error') return;
          continue;
        }
        if (sub.done) return;
        const item = await new Promise<PlexusStreamItem | null>(resolve => { sub.waiting = resolve; });
        if (item === null) return;
        yield item;
        if (item.type === 'done' || item.type === 'error') return;
      }
    } finally {
      this.subscriptions.delete(subscriptionId);
    }
  }
}

export function createClient(config: PlexusRpcConfig): PlexusRpcClient {
  return new PlexusRpcClient(config);
}