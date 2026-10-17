// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../../rpc';
import { extractData, collectOne } from '../../rpc';
import type { WebhookSecret, WebhookUrl } from './types';
import type { DeploymentsEvent, EnvironmentName, Project } from '../../deployments/types';

/** Typed client interface for deployments.webhook plugin */
export interface DeploymentsWebhookClient {
  /**
   * Clear an environment's webhook (admin); yields webhook { url: null, secret_set: false }
   *
   * Clears `project`'s `environment`'s webhook: a request made after it
   * is kept and says "no CI configured"; a delivery still due fails at
   * its next attempt, saying so. Yields exactly one `webhook { webhook }`
   * (`url: null`) or an in-band `error`. Admin on the project.
   * @requires `webhook.clear` on the scope of `project`
   */
  clear(environment: EnvironmentName, project: Project): AsyncGenerator<DeploymentsEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `webhook.schema` anywhere it is held
   */
  schema(): Promise<unknown>;
  /**
   * Set an environment's webhook (admin): url (https, or http to loopback) and the secret that signs each delivery; yields webhook { url, secret_set }
   *
   * Sets (or replaces) `project`'s `environment`'s webhook: every
   * request made after it is POSTed to `url` as JSON, with
   * `X-Menger-Signature: sha256=<hex HMAC-SHA256 of the body keyed by
   * secret>` and `X-Menger-Delivery: <id>`. The URL is https, or http to
   * this machine (a local CI). Yields exactly one `webhook { webhook }` —
   * the URL and `secret_set: true`, never the secret — or an in-band
   * `error`, nothing written. Admin on the project.
   * @requires `webhook.set` on the scope of `project`
   */
  set(environment: EnvironmentName, project: Project, secret: WebhookSecret, url: WebhookUrl): AsyncGenerator<DeploymentsEvent>;
  /**
   * Show an environment's webhook (admin): url, secret_set, set_at, set_by — never the secret
   *
   * `project`'s `environment`'s webhook: its URL, whether a secret is set,
   * who set it when — never the secret. Yields exactly one `webhook
   * { webhook }` (`url: null` when none is set) or an in-band `error`.
   * Admin on the project.
   * @requires `webhook.show` on the scope of `project`
   */
  show(environment: EnvironmentName, project: Project): AsyncGenerator<DeploymentsEvent>;
}

/** Typed client implementation for deployments.webhook plugin */
class DeploymentsWebhookClientImpl implements DeploymentsWebhookClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *clear(environment: EnvironmentName, project: Project): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.webhook.clear', { environment, project });
    yield* extractData<DeploymentsEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('deployments.webhook.schema', {});
    return collectOne<unknown>(stream);
  }

  async *set(environment: EnvironmentName, project: Project, secret: WebhookSecret, url: WebhookUrl): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.webhook.set', { environment, project, secret, url });
    yield* extractData<DeploymentsEvent>(stream);
  }

  async *show(environment: EnvironmentName, project: Project): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.webhook.show', { environment, project });
    yield* extractData<DeploymentsEvent>(stream);
  }
}

/** Create a typed deployments.webhook client from an RPC client */
export function createDeploymentsWebhookClient(rpc: RpcClient): DeploymentsWebhookClient {
  return new DeploymentsWebhookClientImpl(rpc);
}

/** What each deployments.webhook method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "webhook.clear": { grant: "webhook.clear", target: "project" },
  "webhook.schema": { grant: "webhook.schema", target: null },
  "webhook.set": { grant: "webhook.set", target: "project" },
  "webhook.show": { grant: "webhook.show", target: "project" },
} as const;