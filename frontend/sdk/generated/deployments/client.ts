// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { extractData, collectOne } from '../rpc';
import type { DeploySource, DeployState, DeploymentId, DeploymentsEvent, EnvironmentName, Project, Release, Timestamp } from './types';
import type { TicketRef } from '../notes/types';
import type { CommitRef, Message } from '../tickets/types';
import type { DeploymentsWebhookClient } from '../deployments/webhook/client';
import { createDeploymentsWebhookClient } from '../deployments/webhook/client';

/** Typed client interface for deployments plugin */
export interface DeploymentsClient {
  /** DEPLOY-25: each environment's webhook — where a request is POSTed,
signed: `deployments.webhook.set|clear|show`. Admin's alone. */
  readonly webhook: DeploymentsWebhookClient;
  /**
   * Cancel a pending request (one no CI has picked up); yields request { deployment, carried }
   *
   * DEPLOY-25: withdraws a pending request — `canceled`, by the caller;
   * its webhook sends no more. One a CI has picked up (a record names
   * it), or one canceled already, is refused in band, naming why. Yields
   * exactly one `request { deployment, carried }` or an in-band `error`.
   * The deployer's (or admin's) on the request's project.
   * @requires `deployments.cancel_request` anywhere it is held
   */
  cancelRequest(id: DeploymentId): AsyncGenerator<DeploymentsEvent>;
  /**
   * Each environment: what it runs, since when, its last deployment, how many tickets run there
   *
   * Every environment — of `project` when given — one `environment`
   * event each, by project and then in display order (test → staging →
   * prod): what it runs, since when, its last deployment, how many
   * tickets run there, and (DEPLOY-21) `from`, where it promotes from.
   * @requires `deployments.environments` anywhere it is held
   */
  environments(project?: Project | null): AsyncGenerator<DeploymentsEvent>;
  /**
   * Where a ticket's work runs: running { environment, since, deployment, behind, version } per environment, not_running for the rest
   *
   * Where a ticket's work runs: one `running` event for each environment
   * it runs in — since when, brought by which deployment, what the
   * environment runs now — and one `not_running` for each other
   * environment of its projects (its shortcode's, its hosts', and any it
   * has run in). DEPLOY-20: a `running` whose `behind` is not empty runs
   * an earlier version there — `behind` the commits not carried, in link
   * order, `version` the newest carried one. An `error` for an id that
   * names no ticket.
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
   * Set where an environment promotes from (admin): prod from test; without from, from none; yields promotes { project, environment, from }
   *
   * DEPLOY-21: where an environment promotes from — `from` (`test`, for
   * menger's `prod`), or, without it, from none (its own track, as the
   * landing page's `edge`). Admin on the project's shortcode. A ticket's
   * `ready for <environment>` is its current version running on `from`
   * and not here; the environment's track (it and every environment it
   * promotes from or to) is where `waiting` looks. Yields exactly one
   * `promotes { project, environment, from }` — the same when nothing
   * moved — or an in-band `error` (itself, a cycle), nothing written.
   * @requires `deployments.promotes` on the scope of `project`
   */
  promotes(environment: EnvironmentName, project: Project, from?: EnvironmentName | null): AsyncGenerator<DeploymentsEvent>;
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
   * problem, with nothing kept. DEPLOY-21: `from` names the environment
   * this one promotes from (`test`, for `prod`), set (or moved) with the
   * record — `promotes` follows it on `watch`. DEPLOY-25: `request` names
   * the request it answers (`requested` is not a state a CI records); the
   * request then says this record's state, and `request` follows on
   * `watch`.
   * @requires `deployments.record` on the scope of `project`
   */
  record(environment: EnvironmentName, project: Project, revision: CommitRef, state: DeployState, at?: Timestamp | null, commits?: CommitRef[] | null, from?: EnvironmentName | null, previous?: CommitRef | null, release?: Release | null, request?: DeploymentId | null, source?: DeploySource | null, tickets?: TicketRef[] | null): AsyncGenerator<DeploymentsEvent>;
  /**
   * Queue a deploy: kept as a requested deployment, POSTed to the environment's webhook (signed); yields request { deployment, carried }
   *
   * DEPLOY-25: queues a deploy from menger — kept as a deployment in the
   * state `requested`: who asked, when, `project`'s `environment`, what
   * to deploy (`revision`; else the latest recorded deploy of `release`;
   * else what the environment it promotes from runs, DEPLOY-21's `from`)
   * and `note`. It runs nothing and completes nothing. When the
   * environment has a webhook (`webhook set`), menger POSTs the request
   * to it, signed, retrying with backoff; without one it is kept and
   * says `webhook: none` ("no CI configured"). A CI's `record` names it
   * as `request` when it picks it up. Yields exactly one `request
   * { deployment, carried }` — the request as kept and the tickets it
   * would carry — or an in-band `error`, nothing kept. The deployer's
   * (or admin's) on the project.
   * @requires `deployments.request` on the scope of `project`
   */
  request(environment: EnvironmentName, project: Project, note?: Message | null, release?: Release | null, revision?: CommitRef | null): AsyncGenerator<DeploymentsEvent>;
  /**
   * The kept cells: cells { project, environment, running: [{ ticket, since, deployment, behind, version }], staged: [{ ticket, stage }] } per environment
   *
   * DEPLOY-13: the kept cells — one `cells { project, environment,
   * running }` per environment (of `project`, named `environment`, when
   * given), `running` every ticket there (only `ids`' when given, at most
   * 1,000), each with since when and the deploy that brought it, in the
   * store's order — an empty one too, so a copy can replace what it held.
   * DEPLOY-20: a cell's `behind` (and `version`) when the ticket runs an
   * earlier version there. DEPLOY-21: `staged`, each ticket deploying,
   * ready for or failed there.
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
  readonly webhook: DeploymentsWebhookClient;
  constructor(rpc: RpcClient) {
    this.rpc = rpc;
    this.webhook = createDeploymentsWebhookClient(rpc);
  }

  async *cancelRequest(id: DeploymentId): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.cancel_request', { id });
    yield* extractData<DeploymentsEvent>(stream);
  }

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

  async *promotes(environment: EnvironmentName, project: Project, from?: EnvironmentName | null): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.promotes', { environment, from, project });
    yield* extractData<DeploymentsEvent>(stream);
  }

  async *record(environment: EnvironmentName, project: Project, revision: CommitRef, state: DeployState, at?: Timestamp | null, commits?: CommitRef[] | null, from?: EnvironmentName | null, previous?: CommitRef | null, release?: Release | null, request?: DeploymentId | null, source?: DeploySource | null, tickets?: TicketRef[] | null): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.record', { at, commits, environment, from, previous, project, release, request, revision, source, state, tickets });
    yield* extractData<DeploymentsEvent>(stream);
  }

  async *request(environment: EnvironmentName, project: Project, note?: Message | null, release?: Release | null, revision?: CommitRef | null): AsyncGenerator<DeploymentsEvent> {
    const stream = this.rpc.call('deployments.request', { environment, note, project, release, revision });
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
  "deployments.cancel_request": { grant: "deployments.cancel_request", target: null },
  "deployments.environments": { grant: "deployments.environments", target: null },
  "deployments.for_ticket": { grant: "deployments.for_ticket", target: "id" },
  "deployments.list": { grant: "deployments.list", target: null },
  "deployments.promotes": { grant: "deployments.promotes", target: "project" },
  "deployments.record": { grant: "deployments.record", target: "project" },
  "deployments.request": { grant: "deployments.request", target: "project" },
  "deployments.running": { grant: "deployments.running", target: null },
  "deployments.schema": { grant: "deployments.schema", target: null },
  "deployments.watch": { grant: "deployments.watch", target: null },
} as const;