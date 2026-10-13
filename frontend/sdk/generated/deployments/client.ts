// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { DeploySource, DeploymentsEvent } from './types';

/** Typed client interface for deployments plugin */
export interface DeploymentsClient {
  /** Each environment: what it runs, since when, its last deployment, how many tickets run there */
  environments(project?: string | null): AsyncGenerator<DeploymentsEvent>;
  /** Where a ticket's work runs: running { environment, since, deployment } per environment, not_running for the rest */
  forTicket(id: string): AsyncGenerator<DeploymentsEvent>;
  /** List deployments newest first, by project, environment, state and ticket; each with the tickets it carried */
  list(after?: number | null, environment?: string | null, limit?: number | null, project?: string | null, state?: string | null, ticket?: string | null): AsyncGenerator<DeploymentsEvent>;
  /** Record a deployment (DEPLOY-10's record): the tickets its commits carry run in the environment; yields deployed { deployment, carried, unlinked, entered, left, duplicate } */
  record(environment: string, project: string, revision: string, state: string, at?: string | null, commits?: string[] | null, previous?: string | null, release?: string | null, source?: DeploySource | null, tickets?: string[] | null): AsyncGenerator<DeploymentsEvent>;
  /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
  schema(): Promise<unknown>;
  /** Watch deployments: deployed, then environment_changed, as each is recorded */
  watch(): AsyncGenerator<DeploymentsEvent>;
}

/** Typed client implementation for deployments plugin */
class DeploymentsClientImpl implements DeploymentsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *environments(project?: string | null): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.environments', { project });
    yield* extractData<DeploymentsEvent>(stream);
  }

  async *forTicket(id: string): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.for_ticket', { id });
    yield* extractData<DeploymentsEvent>(stream);
  }

  async *list(after?: number | null, environment?: string | null, limit?: number | null, project?: string | null, state?: string | null, ticket?: string | null): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.list', { after, environment, limit, project, state, ticket });
    yield* extractData<DeploymentsEvent>(stream);
  }

  async *record(environment: string, project: string, revision: string, state: string, at?: string | null, commits?: string[] | null, previous?: string | null, release?: string | null, source?: DeploySource | null, tickets?: string[] | null): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.record', { at, commits, environment, previous, project, release, revision, source, state, tickets });
    yield* extractData<DeploymentsEvent>(stream);
  }

  async schema(): Promise<unknown> {
    const stream = this.rpc.call('deployments.schema', {});
    return collectOne<unknown>(stream);
  }

  async *watch(): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.watch', {});
    yield* extractData<DeploymentsEvent>(stream);
  }
}

/** Create a typed deployments client from an RPC client */
export function createDeploymentsClient(rpc: RpcClient): DeploymentsClient {
  return new DeploymentsClientImpl(rpc);
}