import type { RpcClient } from '../rpc';
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
    /** The kept cells: cells { project, environment, running: [{ ticket, since, deployment }] } per environment */
    running(environment?: string | null, ids?: string[] | null, project?: string | null): AsyncGenerator<DeploymentsEvent>;
    /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
    schema(): Promise<unknown>;
    /** Watch deployments: deployed, then environment_changed, as each is recorded */
    watch(): AsyncGenerator<DeploymentsEvent>;
}
/** Create a typed deployments client from an RPC client */
export declare function createDeploymentsClient(rpc: RpcClient): DeploymentsClient;
