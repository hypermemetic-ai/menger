import type { PlexusStreamItem, StandardRequest, StandardResponse } from './types';
import type { RpcClient } from './rpc';
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
 * Thrown CLIENT-SIDE (preflight, no server round-trip) when a gated method
 * is called with no credentials configured (CA-2). Disable via
 * `preflight: false` in the config to send the call anyway.
 */
export declare class MissingCredentialError extends Error {
    readonly method: string;
    readonly scopes: readonly string[];
    readonly siteHint: string | undefined;
    constructor(method: string, scopes: readonly string[], siteHint?: string);
}
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
export type BidirectionalRequestHandler = (request: StandardRequest) => Promise<StandardResponse | undefined>;
export declare class PlexusRpcError extends Error {
    readonly code: number;
    readonly data?: unknown;
    constructor(code: number, message: string, data?: unknown);
}
export declare class AuthenticationError extends PlexusRpcError {
    constructor(message: string, data?: unknown);
}
export declare class ForbiddenRpcError extends PlexusRpcError {
    /** The unmet scope, when the server's Forbidden message names one. */
    readonly missingScope: string | undefined;
    constructor(message: string, data?: unknown);
}
export declare class InvalidParamsError extends PlexusRpcError {
    constructor(message: string, data?: unknown);
}
export declare class MethodNotFoundError extends PlexusRpcError {
    constructor(message: string, data?: unknown);
}
export declare class ExecutionError extends PlexusRpcError {
    constructor(message: string, data?: unknown);
}
export declare class PlexusRpcClient implements RpcClient {
    private ws;
    private nextId;
    private pendingRequests;
    private subscriptions;
    private pendingSubscriptionMessages;
    private config;
    private credentials;
    private attachmentSiteOverride;
    private connectionPromise;
    private onBidirectionalRequest?;
    constructor(config: PlexusRpcConfig);
    setBidirectionalHandler(handler: BidirectionalRequestHandler | undefined): void;
    private log;
    /**
     * The credential requirements the backend schema advertises for a method
     * (CA-2). Methods with no advertised surface return the empty requirement
     * (`scopes: []`, `public: false`).
     */
    requires(method: string): MethodRequirements;
    /** Whether any credential source is configured. */
    private hasCredentials;
    /** Resolve the configured credentials to a token, or null when none. */
    private resolveCredential;
    /**
     * The attach site used at connect time: explicit override, else the
     * schema-derived hint, else the standing convention (logged — never
     * silently). Legacy `authToken` (without `credentials`) pins the SAFE-7
     * cookie convention for byte-compatible behavior.
     */
    private connectAttachmentSite;
    connect(): Promise<void>;
    private doConnect;
    disconnect(): void;
    private handleDisconnect;
    private handleMessage;
    private handleResponse;
    private handleNotification;
    private handleBidirectionalRequest;
    private sendBidirectionalResponse;
    call(method: string, params?: unknown): AsyncGenerator<PlexusStreamItem>;
}
export declare function createClient(config: PlexusRpcConfig): PlexusRpcClient;
