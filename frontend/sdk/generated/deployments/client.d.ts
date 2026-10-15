import type { RpcClient } from '../rpc';
import type { DeploySource, DeployState, DeploymentId, DeploymentsEvent, EnvironmentName, Project, Release, Timestamp } from './types';
import type { TicketRef } from '../notes/types';
import type { CommitRef } from '../tickets/types';
/** Typed client interface for deployments plugin */
export interface DeploymentsClient {
    /** Each environment: what it runs, since when, its last deployment, how many tickets run there */
    environments(project?: Project | null): AsyncGenerator<DeploymentsEvent>;
    /** Where a ticket's work runs: running { environment, since, deployment } per environment, not_running for the rest */
    forTicket(id: TicketRef): AsyncGenerator<DeploymentsEvent>;
    /** List deployments newest first, by project, environment, state and ticket; each with the tickets it carried */
    list(after?: DeploymentId | null, environment?: EnvironmentName | null, limit?: number | null, project?: Project | null, state?: DeployState | null, ticket?: TicketRef | null): AsyncGenerator<DeploymentsEvent>;
    /** Record a deployment (DEPLOY-10's record): the tickets its commits carry run in the environment; yields deployed { deployment, carried, unlinked, entered, left, duplicate } */
    record(environment: EnvironmentName, project: Project, revision: CommitRef, state: DeployState, at?: Timestamp | null, commits?: CommitRef[] | null, previous?: CommitRef | null, release?: Release | null, source?: DeploySource | null, tickets?: TicketRef[] | null): AsyncGenerator<DeploymentsEvent>;
    /** The kept cells: cells { project, environment, running: [{ ticket, since, deployment }] } per environment */
    running(environment?: EnvironmentName | null, ids?: TicketRef[] | null, project?: Project | null): AsyncGenerator<DeploymentsEvent>;
    /** Get plugin or method schema. Pass {"method": "name"} for a specific method. */
    schema(): Promise<unknown>;
    /** Watch deployments: deployed, then environment_changed, as each is recorded */
    watch(): AsyncGenerator<DeploymentsEvent>;
}
/** Create a typed deployments client from an RPC client */
export declare function createDeploymentsClient(rpc: RpcClient): DeploymentsClient;
