import type { RpcClient } from '../rpc';
import type { ChatId, ChatState, ChatsEvent, MessageId } from './types';
import type { Body, TicketRef, Title } from '../notes/types';
import type { Principal } from '../orgs/types';
/** Typed client interface for chats plugin */
export interface ChatsClient {
    /**
     * Add a participant to a chat; yields added { chat }.
  
  By a participant, to an open chat; the one added must be able to read
  the ticket, or it is `refused`. One already in it is the chat as it is.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.add` anywhere it is held
     */
    add(chat: ChatId, principal: Principal): AsyncGenerator<ChatsEvent>;
    /**
     * Wait for the next message for the caller: waiting at once, then one message { message } by someone else — in the chat named, or any of the caller's chats (on ticket, when named) — and the end; timed_out after timeout_minutes.
  
  The first message after `since` (unix seconds, at or after it; without
  it, after the call) and after the message `after` by anyone but the
  caller — so a reconnect with the `since` that `waiting` said, or the
  last message it read as `after` (exact, where a second can hold two
  messages), loses nothing. While it waits, the chats it waits on
  show the caller as `listening` on `watch`. A chat the caller is not
  in is `not_found`.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.await_message` anywhere it is held
     */
    awaitMessage(after?: MessageId | null, chat?: ChatId | null, since?: number | null, ticket?: TicketRef | null, timeoutMinutes?: number | null): AsyncGenerator<ChatsEvent>;
    /**
     * Who could be in a chat on a ticket: a candidate { principal, reads } each — every principal holding a grant in the ticket's organization (a team's members by name), and those in the caller's chats there — then the end.
  
  CHAT-4's picker: `reads` says whether the principal can read the
  ticket now (`open` and `add` refuse one that cannot). The caller is
  not among them. A ticket the caller cannot read is `refused` as one
  that is none. A participant who holds no grant there now (one kept
  before ACCESS-24, when login could be off) is listed all the same, as
  one of the chats' participants.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.candidates` anywhere it is held
     */
    candidates(ticket: TicketRef): AsyncGenerator<ChatsEvent>;
    /**
     * Close a chat; yields closed { chat }. A closed chat is read-only, kept on its ticket.
  
  By any participant. Closing a closed chat changes nothing.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.close` anywhere it is held
     */
    close(chat: ChatId): AsyncGenerator<ChatsEvent>;
    /**
     * The caller's chats, by last activity, newest first — on a ticket, with or without unread, open or closed; a chat { chat } each, then page { next }.
  
  Without `ticket`, the chats the caller is in; with it, those on that
  ticket the caller is in — every one there, for an admin of it. Each
  chat's `unread` is the caller's. `after` (a chat id) continues a page;
  `limit` is 50 unless given, at most 500.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.list` anywhere it is held
     */
    list(after?: ChatId | null, limit?: number | null, state?: ChatState | null, ticket?: TicketRef | null, unread?: boolean | null): AsyncGenerator<ChatsEvent>;
    /**
     * A page of a chat's messages, oldest first within it; a message { message } each, then page { next }.
  
  For a participant (or an admin of its ticket); anyone else is
  answered `not_found`. `after` (a message id) pages forward, `next`
  the page's last while newer remain. CHAT-8: `newest: true` is the
  newest page, and `before` (a message id) pages back from one — `next`
  the page's first while older remain, what `before` takes to go on.
  `limit` is 100 unless given, at most 1,000.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.messages` anywhere it is held
     */
    messages(chat: ChatId, after?: MessageId | null, before?: MessageId | null, limit?: number | null, newest?: boolean | null): AsyncGenerator<ChatsEvent>;
    /**
     * Open a chat on a ticket with the principals named; yields opened { chat } — the open chat they already have there, when they do (existing: true).
  
  The caller and `others` are the participants (CHAT-1's `with`, which
  TypeScript does not take as a parameter's name) — `others` empty or
  absent is a chat of one, the caller's alone, to add to later (CHAT-6);
  every one of them must be
  able to read the ticket, or the call is `refused { reasons }`, naming
  each who cannot, and nothing is made. A ticket the caller cannot read
  is refused as one that is none. `watch` carries `opened` to each
  participant.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.open` anywhere it is held
     */
    open(ticket: TicketRef, others?: Principal[] | null, title?: Title | null): AsyncGenerator<ChatsEvent>;
    /**
     * Post a message in a chat; yields posted { message }, signed by the caller.
  
  Only a participant posts; a closed chat is `refused`. The body is
  markdown, not blank, at most the ticket body cap. A chat the caller
  does not see is `not_found`. `watch` carries `posted` to every
  participant, and it answers their `await_message`.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.post` anywhere it is held
     */
    post(body: Body, chat: ChatId): AsyncGenerator<ChatsEvent>;
    /**
     * Mark a chat read up to a message; yields read { chat, upto, unread }.
  
  Moves the caller's own mark forward (never back) to the latest of
  the chat's messages at or before `upto`; `unread` counts after it.
  `watch` carries `read` to the caller's other watches.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.read` anywhere it is held
     */
    read(chat: ChatId, upto: MessageId): AsyncGenerator<ChatsEvent>;
    /**
     * Rename a chat; yields renamed { chat } — the composer's /title (CHAT-7).
  
  By any participant. No title, or a blank one, clears it (the chat is
  then named by who is in it). The same title changes nothing.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.rename` anywhere it is held
     */
    rename(chat: ChatId, title?: Title | null): AsyncGenerator<ChatsEvent>;
    /**
     * Reopen a closed chat; yields reopened { chat }.
  
  By any participant. Reopening an open chat changes nothing.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.reopen` anywhere it is held
     */
    reopen(chat: ChatId): AsyncGenerator<ChatsEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `chats.schema` anywhere it is held
     */
    schema(): Promise<unknown>;
    /**
     * Subscribe to the caller's chats: opened, posted, added, read, closed, reopened, listening, left — until disconnect.
  
  The chats the caller is in (with `ticket`, those on it — every one
  there for an admin of it), each change as the caller sees it: its own
  unread, its own `read`s. A chat whose ticket leaves the caller's
  reach is said once as `left { chat }`. ACCESS-12: the caller is
  verified again as its grants move; refused, the watch ends with
  `watch_closed { reason, message }`. `lagged { missed }` where a slow
  subscriber lost changes.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.watch` anywhere it is held
     */
    watch(ticket?: TicketRef | null): AsyncGenerator<ChatsEvent>;
}
/** Create a typed chats client from an RPC client */
export declare function createChatsClient(rpc: RpcClient): ChatsClient;
/** What each chats method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "chats.add": {
        readonly grant: "chats.add";
        readonly target: null;
    };
    readonly "chats.await_message": {
        readonly grant: "chats.await_message";
        readonly target: null;
    };
    readonly "chats.candidates": {
        readonly grant: "chats.candidates";
        readonly target: null;
    };
    readonly "chats.close": {
        readonly grant: "chats.close";
        readonly target: null;
    };
    readonly "chats.list": {
        readonly grant: "chats.list";
        readonly target: null;
    };
    readonly "chats.messages": {
        readonly grant: "chats.messages";
        readonly target: null;
    };
    readonly "chats.open": {
        readonly grant: "chats.open";
        readonly target: null;
    };
    readonly "chats.post": {
        readonly grant: "chats.post";
        readonly target: null;
    };
    readonly "chats.read": {
        readonly grant: "chats.read";
        readonly target: null;
    };
    readonly "chats.rename": {
        readonly grant: "chats.rename";
        readonly target: null;
    };
    readonly "chats.reopen": {
        readonly grant: "chats.reopen";
        readonly target: null;
    };
    readonly "chats.schema": {
        readonly grant: "chats.schema";
        readonly target: null;
    };
    readonly "chats.watch": {
        readonly grant: "chats.watch";
        readonly target: null;
    };
};
