// Auto-generated typed client (Layer 2)
// Wraps RPC layer and unwraps PlexusStreamItem to domain types

import type { RpcClient } from '../rpc';
import { MethodStream, collectOne } from '../rpc';
import type { EpicName, EpicsEvent, SchemaResult } from './types';
import type { OrgId } from '../orgs/types';

/** Typed client interface for epics plugin */
export interface EpicsClient {
  /**
   * Archive an epic (admin on its organization, or on *), in one write: everyone on it moves to the next newer epic (else the next older), the default too if it was this one; its tickets leave every view unless a filter names the epic or archived. The last live epic is refused. Yields archived { epic, moved, to, tickets, dry_run }.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.archive` anywhere it is held
   */
  archive(name: EpicName, dryRun?: boolean | null, org?: OrgId | null): MethodStream<'epics.archive', EpicsEvent>;
  /**
   * Make an epic in the organization, empty (admin on it, or on *): its shortcodes start at 1. Move tickets in with tickets reshortcode --to org:epic:CODE; create in it with create --epic org:epic:CODE. Yields created { epic }.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.create` anywhere it is held
   */
  create(name: EpicName, org?: OrgId | null): MethodStream<'epics.create', EpicsEvent>;
  /**
   * The epic you are in, in the organization — your choice (switch), else its default; yields current { epic, moved? } (moved: an archive moved you here, until dismiss). A filtered read answers there unless its filter names epic (epic any: every live one).
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.current` anywhere it is held
   */
  current(org?: OrgId | null): MethodStream<'epics.current', EpicsEvent>;
  /**
   * Delete an empty epic (admin on its organization, or on *) — never the organization's first; whoever was in it is on the default again. Yields deleted { org, name }.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.delete` anywhere it is held
   */
  delete(name: EpicName, org?: OrgId | null): MethodStream<'epics.delete', EpicsEvent>;
  /**
   * Dismiss the notice that an archive moved you (your own setting): current stops carrying moved. Yields dismissed { org }.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.dismiss` anywhere it is held
   */
  dismiss(org?: OrgId | null): MethodStream<'epics.dismiss', EpicsEvent>;
  /**
   * One epic of the organization by name; `not_found` when it has none of that name.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.get` anywhere it is held
   */
  get(name: EpicName, org?: OrgId | null): MethodStream<'epics.get', EpicsEvent>;
  /**
   * The organization's live epics, oldest first: one `epic { org, name, first, default, current, members_on, shortcodes, tickets, created_unix, archived? }` each (--archived true: the archived ones too).
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.list` anywhere it is held
   */
  list(archived?: boolean | null, org?: OrgId | null): MethodStream<'epics.list', EpicsEvent>;
  /**
   * Rename an epic (admin on its organization, or on *): every address naming it moves to the new name. Yields renamed { from, epic }.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.rename` anywhere it is held
   */
  rename(name: EpicName, to: EpicName, org?: OrgId | null): MethodStream<'epics.rename', EpicsEvent>;
  /**
   * Restore an archived epic (admin on its organization, or on *): back in the picker and the views; nobody moves. Yields restored { epic }.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.restore` anywhere it is held
   */
  restore(name: EpicName, org?: OrgId | null): MethodStream<'epics.restore', EpicsEvent>;
  /**
   * Get plugin or method schema. Pass {"method": "name"} for a specific method.
   * @requires `epics.schema` anywhere it is held
   */
  schema(method?: string | null): Promise<SchemaResult>;
  /**
   * Make an epic the organization's default (admin on it, or on *) — where everyone who never switched is. Yields default_set { epic }.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.set_default` anywhere it is held
   */
  setDefault(name: EpicName, org?: OrgId | null): MethodStream<'epics.set_default', EpicsEvent>;
  /**
   * Be in another epic of the organization (your own setting): short forms resolve there first, and filtered reads answer there. Yields current { epic }.
   * @server-derived auth
   * @requiresAuth — resolver: self.access.verify
   * @requires `epics.switch` anywhere it is held
   */
  switch(name: EpicName, org?: OrgId | null): MethodStream<'epics.switch', EpicsEvent>;
  /**
   * Follow your organizations' epics, open-ended: epic_changed { org, name, what } (created, renamed, deleted, archived, restored, default_moved), and moved { org, moved } when an archive moves you; lagged { missed }; watch_closed { reason, message } when your grants end it.
   * @requires `epics.watch` anywhere it is held
   */
  watch(): MethodStream<'epics.watch', EpicsEvent>;
}

/** Typed client implementation for epics plugin */
class EpicsClientImpl implements EpicsClient {
  private rpc: RpcClient;
  constructor(rpc: RpcClient) { this.rpc = rpc; }

  archive(name: EpicName, dryRun?: boolean | null, org?: OrgId | null): MethodStream<'epics.archive', EpicsEvent> {
    const stream = this.rpc.call('epics.archive', { dry_run: dryRun, name, org });
    return new MethodStream<'epics.archive', EpicsEvent>('epics.archive', stream);
  }

  create(name: EpicName, org?: OrgId | null): MethodStream<'epics.create', EpicsEvent> {
    const stream = this.rpc.call('epics.create', { name, org });
    return new MethodStream<'epics.create', EpicsEvent>('epics.create', stream);
  }

  current(org?: OrgId | null): MethodStream<'epics.current', EpicsEvent> {
    const stream = this.rpc.call('epics.current', { org });
    return new MethodStream<'epics.current', EpicsEvent>('epics.current', stream);
  }

  delete(name: EpicName, org?: OrgId | null): MethodStream<'epics.delete', EpicsEvent> {
    const stream = this.rpc.call('epics.delete', { name, org });
    return new MethodStream<'epics.delete', EpicsEvent>('epics.delete', stream);
  }

  dismiss(org?: OrgId | null): MethodStream<'epics.dismiss', EpicsEvent> {
    const stream = this.rpc.call('epics.dismiss', { org });
    return new MethodStream<'epics.dismiss', EpicsEvent>('epics.dismiss', stream);
  }

  get(name: EpicName, org?: OrgId | null): MethodStream<'epics.get', EpicsEvent> {
    const stream = this.rpc.call('epics.get', { name, org });
    return new MethodStream<'epics.get', EpicsEvent>('epics.get', stream);
  }

  list(archived?: boolean | null, org?: OrgId | null): MethodStream<'epics.list', EpicsEvent> {
    const stream = this.rpc.call('epics.list', { archived, org });
    return new MethodStream<'epics.list', EpicsEvent>('epics.list', stream);
  }

  rename(name: EpicName, to: EpicName, org?: OrgId | null): MethodStream<'epics.rename', EpicsEvent> {
    const stream = this.rpc.call('epics.rename', { name, org, to });
    return new MethodStream<'epics.rename', EpicsEvent>('epics.rename', stream);
  }

  restore(name: EpicName, org?: OrgId | null): MethodStream<'epics.restore', EpicsEvent> {
    const stream = this.rpc.call('epics.restore', { name, org });
    return new MethodStream<'epics.restore', EpicsEvent>('epics.restore', stream);
  }

  async schema(method?: string | null): Promise<SchemaResult> {
    const stream = this.rpc.call('epics.schema', { method });
    return collectOne<SchemaResult>(stream);
  }

  setDefault(name: EpicName, org?: OrgId | null): MethodStream<'epics.set_default', EpicsEvent> {
    const stream = this.rpc.call('epics.set_default', { name, org });
    return new MethodStream<'epics.set_default', EpicsEvent>('epics.set_default', stream);
  }

  switch(name: EpicName, org?: OrgId | null): MethodStream<'epics.switch', EpicsEvent> {
    const stream = this.rpc.call('epics.switch', { name, org });
    return new MethodStream<'epics.switch', EpicsEvent>('epics.switch', stream);
  }

  watch(): MethodStream<'epics.watch', EpicsEvent> {
    const stream = this.rpc.call('epics.watch', {});
    return new MethodStream<'epics.watch', EpicsEvent>('epics.watch', stream);
  }
}

/** Create a typed epics client from an RPC client */
export function createEpicsClient(rpc: RpcClient): EpicsClient {
  return new EpicsClientImpl(rpc);
}

/** What each epics method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export const requires = {
  "epics.archive": { grant: "epics.archive", target: null },
  "epics.create": { grant: "epics.create", target: null },
  "epics.current": { grant: "epics.current", target: null },
  "epics.delete": { grant: "epics.delete", target: null },
  "epics.dismiss": { grant: "epics.dismiss", target: null },
  "epics.get": { grant: "epics.get", target: null },
  "epics.list": { grant: "epics.list", target: null },
  "epics.rename": { grant: "epics.rename", target: null },
  "epics.restore": { grant: "epics.restore", target: null },
  "epics.schema": { grant: "epics.schema", target: null },
  "epics.set_default": { grant: "epics.set_default", target: null },
  "epics.switch": { grant: "epics.switch", target: null },
  "epics.watch": { grant: "epics.watch", target: null },
} as const;