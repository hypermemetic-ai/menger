// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { DeploySource, DeployState, DeploymentId, DeploymentsEvent, EnvironmentName, Project, Release, Timestamp } from './types';
import type { TicketRef } from '../notes/types';
import type { CommitRef } from '../tickets/types';

/** Typed client interface for deployments plugin */
export interface DeploymentsClient {
  /**
   * Each environment: what it runs, since when, its last deployment, how many tickets run there
   *
   * Every environment — of `project` when given — one `environment`
   * event each, by project and then in display order (test → staging →
   * prod): what it runs, since when, its last deployment, and how many
   * tickets run there.
   * @requires `deployments.environments` anywhere it is held
   */
  environments(project?: Project | null): AsyncGenerator<DeploymentsEvent>;
  /**
   * Where a ticket's work runs: running { environment, since, deployment } per environment, not_running for the rest
   *
   * Where a ticket's work runs: one `running` event for each environment
   * it runs in — since when, brought by which deployment, what the
   * environment runs now — and one `not_running` for each other
   * environment of its projects (its shortcode's, its hosts', and any it
   * has run in). An `error` for an id that names no ticket.
   * @requires `deployments.for_ticket` on the scope of `id`
   */
  forTicket(id: TicketRef): AsyncGenerator<DeploymentsEvent>;
  /**
   * List deployments newest first, by project, environment, state and ticket; each with the tickets it carried
   *
   * The deployments matching every filter given, newest first — one
   * `deployment` event each (with the tickets it carried, its unlinked
   * commits, and `live`: whether it still counts in its environment) —
   * then exactly one `page { next }`. `after` starts strictly after
   * (older than) that deployment; `limit` caps the page. From the cache.
   * @requires `deployments.list` anywhere it is held
   */
  list(after?: DeploymentId | null, environment?: EnvironmentName | null, limit?: number | null, project?: Project | null, state?: DeployState | null, ticket?: TicketRef | null): AsyncGenerator<DeploymentsEvent>;
  /**
   * Record a deployment (DEPLOY-10's record): the tickets its commits carry run in the environment; yields deployed { deployment, carried, unlinked, entered, left, duplicate }
   *
   * Records one deployment — DEPLOY-10's record, its fields as the
   * parameters (`deploy/record --send` passes the record whole as the
   * params object). `project` is a shortcode the store holds, any case;
   * `environment` a name (kept lower case); `state` one of `started`,
   * `succeeded`, `failed`, `rolled_back`; `revision` the commit now
   * running (for `failed`, the one tried; for `rolled_back`, the one
   * returned to — `previous` the one taken out); `commits` the range it
   * carries, when the CI can list them; `tickets` ids it names itself.
   * The tickets whose linked commits are among `commits` and `revision`,
   * plus `tickets`, run in the environment from `at` once it succeeds; a
   * rollback, or a later deploy to an older revision, takes out what only
   * the undone deploys carried; `started` and `failed` count for nothing.
   * A deployment never changes a status. Yields exactly one event:
   * `deployed` — the deployment as kept, the tickets it carried, its
   * commits no ticket links, and the tickets that `entered` and `left`
   * the environment; `duplicate: true` and nothing written when the same
   * record (project, environment, state, revision, and `source.run`,
   * else `at`) is kept already — or an in-band `error` naming every
   * problem, with nothing kept.
   * @requires `deployments.record` on the scope of `project`
   */
  record(environment: EnvironmentName, project: Project, revision: CommitRef, state: DeployState, at?: Timestamp | null, commits?: CommitRef[] | null, previous?: CommitRef | null, release?: Release | null, source?: DeploySource | null, tickets?: TicketRef[] | null): AsyncGenerator<DeploymentsEvent>;
  /**
   * The kept cells: cells { project, environment, running: [{ ticket, since, deployment }] } per environment
   *
   * DEPLOY-13: the kept cells — one `cells { project, environment,
   * running }` per environment (of `project`, named `environment`, when
   * given), `running` every ticket there (only `ids`' when given, at most
   * 1,000), each with since when and the deploy that brought it, in the
   * store's order — an empty one too, so a copy can replace what it held.
   * What the app's board marks are drawn from, one read for every row.
   * @requires `deployments.running` anywhere it is held
   */
  running(environment?: EnvironmentName | null, ids?: TicketRef[] | null, project?: Project | null): AsyncGenerator<DeploymentsEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `deployments.schema` anywhere it is held
   */
  schema(): Promise<unknown>;
  /**
   * Watch deployments: deployed, then environment_changed, as each is recorded
   *
   * Open-ended: every deployment recorded from now on — `deployed`, then
   * `environment_changed` with the environment as it left it — of the
   * projects the caller holds this on; `lagged` when it fell behind. A
   * ticket entering or leaving a `deployed` filter is said on
   * `tickets.watch { filter }`.
   * @requires `deployments.watch` anywhere it is held
   */
  watch(): AsyncGenerator<DeploymentsEvent>;
}

/** Typed client implementation for deployments plugin */
class DeploymentsClientImpl implements DeploymentsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  async *environments(project?: Project | null): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.environments', { project });
    yield* extractData<DeploymentsEvent>(stream);
  }

  async *forTicket(id: TicketRef): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.for_ticket', { id });
    yield* extractData<DeploymentsEvent>(stream);
  }

  async *list(after?: DeploymentId | null, environment?: EnvironmentName | null, limit?: number | null, project?: Project | null, state?: DeployState | null, ticket?: TicketRef | null): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.list', { after, environment, limit, project, state, ticket });
    yield* extractData<DeploymentsEvent>(stream);
  }

  async *record(environment: EnvironmentName, project: Project, revision: CommitRef, state: DeployState, at?: Timestamp | null, commits?: CommitRef[] | null, previous?: CommitRef | null, release?: Release | null, source?: DeploySource | null, tickets?: TicketRef[] | null): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.record', { at, commits, environment, previous, project, release, revision, source, state, tickets });
    yield* extractData<DeploymentsEvent>(stream);
  }

  async *running(environment?: EnvironmentName | null, ids?: TicketRef[] | null, project?: Project | null): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.running', { environment, ids, project });
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

/** What each deployments method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "deployments.environments": { grant: "deployments.environments", target: null },
  "deployments.for_ticket": { grant: "deployments.for_ticket", target: "id" },
  "deployments.list": { grant: "deployments.list", target: null },
  "deployments.record": { grant: "deployments.record", target: "project" },
  "deployments.running": { grant: "deployments.running", target: null },
  "deployments.schema": { grant: "deployments.schema", target: null },
  "deployments.watch": { grant: "deployments.watch", target: null },
} as const;

/** The deployments activation's default roles, each the grants it lists (AUTH-3). Defaults only: a server's roles are data an admin may change, and `admin` holds every grant. */
export const defaultRoles = {
  "deployer": ["deployments.list", "deployments.environments", "deployments.for_ticket", "deployments.running", "deployments.watch", "deployments.record"],
  "editor": ["deployments.list", "deployments.environments", "deployments.for_ticket", "deployments.running", "deployments.watch"],
  "viewer": ["deployments.list", "deployments.environments", "deployments.for_ticket", "deployments.running", "deployments.watch"],
} as const;