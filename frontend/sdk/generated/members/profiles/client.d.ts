import type { RpcClient } from '../../rpc';
import { MethodStream } from '../../rpc';
import type { SchemaResult } from './types';
import type { MembersEvent } from '../../members/types';
import type { Principal } from '../../orgs/types';
/** Typed client interface for members.profiles plugin */
export interface MembersProfilesClient {
    /**
     * One principal's profile. Admin of * only.
  
  One `profile { profile }`, or an in-band `error` when the server
  has never seen it sign in.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `profiles.get` anywhere it is held
     */
    get(principal: Principal): MethodStream<'members.profiles.get', MembersEvent>;
    /**
     * Everyone who signed in, newest sign-in first; one profile each, with holds_grant (false: waiting). Admin of * only.
  
  One `profile { profile }` per principal the server has seen sign
  in: `provider`, `method` (`githuboauth`, `email_code`, `key`, …),
  `email`, `name`, `picture`, `signed_in_at`, `first_seen`, and
  `holds_grant` — whether it holds a grant of its own. Anyone but an
  admin of `*` is refused in band.
     * @requires `profiles.list` anywhere it is held
     */
    list(): MethodStream<'members.profiles.list', MembersEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `profiles.schema` anywhere it is held
     */
    schema(method?: string | null): Promise<SchemaResult>;
}
/** Create a typed members.profiles client from an RPC client */
export declare function createMembersProfilesClient(rpc: RpcClient): MembersProfilesClient;
/** What each members.profiles method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "profiles.get": {
        readonly grant: "profiles.get";
        readonly target: null;
    };
    readonly "profiles.list": {
        readonly grant: "profiles.list";
        readonly target: null;
    };
    readonly "profiles.schema": {
        readonly grant: "profiles.schema";
        readonly target: null;
    };
};
