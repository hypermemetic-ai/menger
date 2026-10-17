import type { WatchEnd } from '../chats/types';
/** DEPLOY-21: a ticket's delivery stage in an environment, short of running
its current version there (`deployed <env>`, and not `behind`) — each a
filter cell, `<stage> <env>`; a ticket is in at most one of them per
environment, the first that holds of: deploying, failed, ready for,
waiting. */
export type DeliveryStage = "waiting" | "deploying" | "ready_for" | "failed";
/** Where to read the run that made a deployment. */
export interface DeploySource {
    /** Its run, as the CI names it — what a retry of the same record repeats. */
    run?: string | null;
    /** The CI: `menger-pipeline`, `github-actions`, … */
    system?: string | null;
    url?: string | null;
}
/** What a deployment did (any case; rolled-back too) */
export type DeployState = "started" | "succeeded" | "failed" | "rolled_back";
/** A deployment as menger keeps it: the record, read, with its id, `at` as
unix seconds, and who recorded it when. */
export interface Deployment {
    /** When it happened, unix seconds — the record's `at`, else when it came. */
    at: number;
    /** As sent, each a commit reference (lower-case hash). */
    commits: string[];
    /** The environment's name, lower case. */
    environment: string;
    id: number;
    previous?: string | null;
    /** The project's shortcode, upper case. */
    project: string;
    /** When menger kept it, unix seconds. */
    recordedAt: number;
    /** Who sent it: a principal, `anonymous` with login off. */
    recordedBy: string;
    release?: string | null;
    revision: string;
    source: DeploySource;
    state: DeployState;
    /** The ids the record named, upper case — counted when they name a ticket. */
    tickets: string[];
}
/** DEPLOY-11: a deployment's number, from 1 in the order recorded. */
export type DeploymentId = number;
/** One deployment (`list`'s rows): `carried` the tickets it carried —
whose linked commits are among its commits and revision, and the ids
it named — `unlinked` its commits no ticket links, `live` whether it
still counts in its environment (not undone by a rollback). */
export interface DeploymentsEventDeployment {
    type: 'deployment';
    carried: string[];
    deployment: Deployment;
    live: boolean;
    unlinked: string[];
}
/** A deployment recorded — `record`'s answer and `watch`'s event:
as `deployment`, with the tickets that began (`entered`) and stopped
(`left`) running in its environment because of it. `duplicate`: the
same record was kept already (a CI's retry) — this is that one, and
nothing was written. */
export interface DeploymentsEventDeployed {
    type: 'deployed';
    carried: string[];
    deployment: Deployment;
    duplicate: boolean;
    entered: string[];
    left: string[];
    live: boolean;
    unlinked: string[];
}
/** One environment (`environments`' rows). */
export interface DeploymentsEventEnvironment {
    type: 'environment';
    environment: Environment;
}
/** `watch`: an environment as a recorded deployment left it — after its
`deployed`. */
export interface DeploymentsEventEnvironmentChanged {
    type: 'environment_changed';
    environment: Environment;
}
/** `for_ticket`: the ticket runs in this environment, since `since`
(unix seconds), brought by `deployment`; the environment runs
`revision` now. DEPLOY-20: `behind` the ticket's commits not carried
there, in link order, when it runs an earlier version — `version`,
its newest carried commit; empty when it runs the current one. */
export interface DeploymentsEventRunning {
    type: 'running';
    behind: string[];
    deployment: number;
    environment: string;
    project: string;
    release: string | null;
    revision: string | null;
    since: number;
    ticket: string;
    version: string | null;
}
/** `for_ticket`: an environment of the ticket's project it does not run in. */
export interface DeploymentsEventNotRunning {
    type: 'not_running';
    environment: string;
    project: string;
    ticket: string;
}
/** DEPLOY-13: `running`: an environment's kept cells — every ticket
running there (of the `ids` asked, when given), in the store's order.
One per environment asked, an empty one included, so a copy can
replace what it held for it.
DEPLOY-21: `staged` each ticket deploying, ready for or failed there
(of the `ids` asked, when given), in the store's order. */
export interface DeploymentsEventCells {
    type: 'cells';
    environment: string;
    project: string;
    running: RunningCell[];
    staged: StagedCell[];
}
/** DEPLOY-21: `promotes`' answer and `watch`'s event — the environment
`(project, environment)` promotes from `from` now (`None`: from none). */
export interface DeploymentsEventPromotes {
    type: 'promotes';
    environment: string;
    from: string | null;
    project: string;
}
/** Where a `list` stands, once, last: `next` is the id to pass as the
next call's `after`, `null` when nothing more matches. */
export interface DeploymentsEventPage {
    type: 'page';
    next: number | null;
}
/** `watch` fell behind and lost `missed` events. */
export interface DeploymentsEventLagged {
    type: 'lagged';
    missed: number;
}
/** ACCESS-12 (`watch`): the watch ended — its caller, verified again
when the grants moved or its sign-in ended, was refused. Last. */
export interface DeploymentsEventWatchClosed {
    type: 'watch_closed';
    message: string;
    reason: WatchEnd;
}
/** In-band refusal, naming the problem; nothing was written. */
export interface DeploymentsEventError {
    type: 'error';
    message: string;
}
/** What the `deployments` activation streams. Every method returns
`impl Stream<Item = DeploymentsEvent>`; a refusal is the in-band `Error`. */
export type DeploymentsEvent = DeploymentsEventDeployment | DeploymentsEventDeployed | DeploymentsEventEnvironment | DeploymentsEventEnvironmentChanged | DeploymentsEventRunning | DeploymentsEventNotRunning | DeploymentsEventCells | DeploymentsEventPromotes | DeploymentsEventPage | DeploymentsEventLagged | DeploymentsEventWatchClosed | DeploymentsEventError;
export declare function isDeploymentsEventDeployment(e: DeploymentsEvent): e is DeploymentsEventDeployment;
export declare function isDeploymentsEventDeployed(e: DeploymentsEvent): e is DeploymentsEventDeployed;
export declare function isDeploymentsEventEnvironment(e: DeploymentsEvent): e is DeploymentsEventEnvironment;
export declare function isDeploymentsEventEnvironmentChanged(e: DeploymentsEvent): e is DeploymentsEventEnvironmentChanged;
export declare function isDeploymentsEventRunning(e: DeploymentsEvent): e is DeploymentsEventRunning;
export declare function isDeploymentsEventNotRunning(e: DeploymentsEvent): e is DeploymentsEventNotRunning;
export declare function isDeploymentsEventCells(e: DeploymentsEvent): e is DeploymentsEventCells;
export declare function isDeploymentsEventPromotes(e: DeploymentsEvent): e is DeploymentsEventPromotes;
export declare function isDeploymentsEventPage(e: DeploymentsEvent): e is DeploymentsEventPage;
export declare function isDeploymentsEventLagged(e: DeploymentsEvent): e is DeploymentsEventLagged;
export declare function isDeploymentsEventWatchClosed(e: DeploymentsEvent): e is DeploymentsEventWatchClosed;
export declare function isDeploymentsEventError(e: DeploymentsEvent): e is DeploymentsEventError;
/** An environment as it stands. */
export interface Environment {
    /** The deployment that made it run `revision` (a deploy or a rollback). */
    deployment?: number | null;
    /** DEPLOY-21: the environment it promotes from, when it names one
  (`deployments promotes`, or a record's `from`). */
    from?: string | null;
    /** The latest deployment recorded to it, whatever its state. */
    last?: number | null;
    lastAt?: number | null;
    lastState?: DeployState | null;
    name: string;
    /** Its place among its project's environments ([`rank`]): 0 first. */
    order: number;
    project: string;
    release?: string | null;
    /** What it runs — `None` until a deploy to it succeeds. */
    revision?: string | null;
    /** Since when it runs `revision`, unix seconds. */
    since?: number | null;
    /** How many tickets run there. */
    tickets: number;
}
/** DEPLOY-10: an environment's name — lower case, `[a-z0-9._/-]`,
starting with a letter or a digit, at most 64 bytes (`prod`,
`pr-412`, `review/pr-412`): [`crate::deployments::environment_of`]. */
export type EnvironmentName = string;
/** DEPLOY-10: whose environment a deployment is — a shortcode's code,
any case on the way in, kept upper case (`menger` is MENGER). The
one shortcode rule, [`crate::address::Code::parse`]'s: [`crate::deployments::project_of`]. */
export type Project = string;
/** DEPLOY-10: a release — the artifact's name (`20261012T220402Z-235ff62`),
one word, at most 256 bytes. */
export type Release = string;
/** DEPLOY-13: one ticket running in an environment — a kept cell, as
`running` answers it. */
export interface RunningCell {
    /** DEPLOY-20: the ticket's commits not carried there, in link order —
  linked after the version that runs there. Empty: it runs the
  ticket's current version. */
    behind?: string[];
    /** That deploy. */
    deployment: number;
    /** Since when, unix seconds — the earliest still-counting deploy that
  carried it. */
    since: number;
    ticket: string;
    /** DEPLOY-20: when `behind`, the version that runs there — its newest
  carried commit (`None` when only a record naming the ticket carried
  it). */
    version?: string | null;
}
/** DEPLOY-21: one ticket's stage in an environment, as `running` answers it
beside the running cells — deploying, ready for or failed (waiting is
every other Complete ticket, asked with the filter cell). */
export interface StagedCell {
    stage: DeliveryStage;
    ticket: string;
}
/** DEPLOY-10: when — RFC 3339 (`2026-10-12T22:04:19Z`) or unix seconds,
by its shape; the server reads the moment. */
export type Timestamp = string;
