import type { RpcClient } from '../rpc';
import type { ActivityLine, ActivityState, ChatGrant, ChatState, ChatsEvent, MessageId } from './types';
import type { ChatId } from '../media/types';
import type { Body, TicketRef, Title } from '../notes/types';
import type { Principal } from '../orgs/types';
/** Typed client interface for chats plugin */
export interface ChatsClient {
    /**
     * Say what the caller is doing in a chat, live; yields activity { activity } — never a message (CHAT-18).
  
  By a participant: `started` (it has the message), `working` (the
  default: a line of what it does now, or a heartbeat with none),
  `ended` (the turn is over). Every participant's `watch` hears
  `activity`; nothing is stored — the server keeps each chat's last
  200 in memory for `activity_log`, lost on a restart. A line is cut
  at 500 characters. A chat the caller is not in is `not_found`.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.activity` anywhere it is held
     */
    activity(chat: ChatId, line?: ActivityLine | null, state?: ActivityState | null): AsyncGenerator<ChatsEvent>;
    /**
     * A chat's live work kept in memory — its last 200 activity items, oldest first; an activity { activity } each, then the end (CHAT-18).
  
  For a participant, so a viewer opening a chat mid-turn sees what has
  been done; anyone else is answered `not_found`. Empty after a restart.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.activity_log` anywhere it is held
     */
    activityLog(chat: ChatId): AsyncGenerator<ChatsEvent>;
    /**
     * Add a participant to a chat; yields added { chat }.
  
  By a participant, to an open chat; the one added must be able to read
  the ticket, or it is `refused`. One already in it is the chat as it is.
  CHAT-14: to a group, one holding a grant in its organization, by a
  participant or an admin of the organization; a direct message is
  refused (it stays between two: start a group). CHAT-16: an admin of
  the chat's scope adds anyone, in it or not; one who can't read it is
  added only with `grant` — `ticket` (viewer on the ticket alone) or
  `chat` (this chat only) — the add and the grant one write, answered
  `added { chat, granted }`; without it, `needs_grant { choices }` and
  nothing written.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.add` anywhere it is held
     */
    add(chat: ChatId, principal: Principal, grant?: ChatGrant | null): AsyncGenerator<ChatsEvent>;
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
     * Who could be in a chat — on a ticket, or (without one) outside tickets: a candidate { principal, reads, name?, agent } each — every principal holding a grant in the organization (a team's members by name), and those in the caller's chats there — then the end.
  
  CHAT-4's picker: `reads` says whether the principal can read the
  ticket now (`open` and `add` refuse one that cannot). The caller is
  not among them. A ticket the caller cannot read is `refused` as one
  that is none. A participant who holds no grant there now (one kept
  before ACCESS-24, when login could be off) is listed all the same, as
  one of the chats' participants. CHAT-14: without a ticket, those
  holding a grant in the caller's organizations (`reads`: may be in a
  direct message or a group); each by the `name` the server knows it
  by (signed in with — a synapse key's name too), and `agent` for an
  agent (an `agent:` or `key:` principal, or a standing chat member).
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.candidates` anywhere it is held
     */
    candidates(ticket?: TicketRef | null): AsyncGenerator<ChatsEvent>;
    /**
     * Close a chat; yields closed { chat }. A closed chat is read-only, kept on its ticket.
  
  By any participant. Closing a closed chat changes nothing.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.close` anywhere it is held
     */
    close(chat: ChatId): AsyncGenerator<ChatsEvent>;
    /**
     * Delete messages in a chat — kept and marked deleted { at, by }, never gone; yields messages_deleted { chat, ids, by, at, unread } (CHAT-44).
  
  A message's author deletes it, and an admin of the chat's ticket
  (outside tickets, of its organization) deletes any. Name them by
  `ids`, or every message of the chat from `from` to `to`. All or
  none: one the caller may not delete, or not the chat's, is
  `refused`, naming each; one already deleted is not named again.
  `messages` still answers a deleted message, marked; it is no one's
  unread and no `await_message` reply. `watch` carries
  `messages_deleted` to every participant.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.delete` anywhere it is held
     */
    delete(chat: ChatId, from?: MessageId | null, ids?: MessageId[] | null, to?: MessageId | null): AsyncGenerator<ChatsEvent>;
    /**
     * Link tickets to a chat, as a builder's work from it; yields linked { chat, tickets, how } — those new to it (CHAT-55).
  
  By a participant; each ticket one the caller reads, or the call is
  `refused`, naming each, and nothing is linked. `watch` carries
  `linked` to every participant.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.link` anywhere it is held
     */
    link(chat: ChatId, tickets: TicketRef[]): AsyncGenerator<ChatsEvent>;
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
     * Open a chat on a ticket, or outside tickets — a direct message or a group — with the principals named; yields opened { chat } — the open chat they already have, when they do (existing: true).
  
  The caller and `others` are the participants (CHAT-1's `with`, which
  TypeScript does not take as a parameter's name). On a ticket,
  `others` empty or absent is a chat of one, the caller's alone, to
  add to later (CHAT-6); every one of them must be able to read the
  ticket, or the call is `refused { reasons }`, naming each who cannot,
  and nothing is made. A ticket the caller cannot read is refused as
  one that is none. CHAT-14: without a ticket, one other is a direct
  message (the one the two have, when they have one) and several a
  group (`title` names it); each must hold a grant in the caller's
  organization, and only its participants read it. `watch` carries
  `opened` to each participant.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.open` anywhere it is held
     */
    open(others?: Principal[] | null, ticket?: TicketRef | null, title?: Title | null): AsyncGenerator<ChatsEvent>;
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
     * Take a participant out of a chat; yields removed { chat, principal, revoked } (CHAT-14; CHAT-16: revoke takes back the grant their add made).
  
  A group's or a ticket's chat, by a participant (itself too: it
  leaves) or, outside tickets, an admin of the chat's organization; a
  direct message is refused (close it instead). One not in it is the
  chat as it is. The one removed hears `left` on its `watch`.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.remove` anywhere it is held
     */
    remove(chat: ChatId, principal: Principal, revoke?: boolean | null): AsyncGenerator<ChatsEvent>;
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
     * Put deleted messages back in a chat; yields messages_restored { chat, ids, by, at, unread } (CHAT-44).
  
  `delete`'s rule: a message's author, or an admin of the chat's
  ticket (outside tickets, its organization); by `ids`, or `from` …
  `to`; all or none. One not deleted is not named again. `watch`
  carries `messages_restored` to every participant.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.restore` anywhere it is held
     */
    restore(chat: ChatId, from?: MessageId | null, ids?: MessageId[] | null, to?: MessageId | null): AsyncGenerator<ChatsEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `chats.schema` anywhere it is held
     */
    schema(): Promise<unknown>;
    /**
     * The tickets a chat names — in its messages, filed from it or linked to it — each a ticket { ticket } with its stage, newest first (CHAT-55).
  
  A message's codes count from when it is posted (a deleted message's
  drop out); `tickets create --chat` and `link` add theirs. Only the
  tickets the caller reads; a chat it does not see is `not_found`.
  The stage: waits_on_you, ready, in_progress, in_review, on_test,
  shipped (closed, other: not work in hand).
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `chats.tickets` anywhere it is held
     */
    tickets(chat: ChatId): AsyncGenerator<ChatsEvent>;
    /**
     * Subscribe to the caller's chats: opened, posted, added, read, closed, reopened, messages_deleted, messages_restored, listening, left — until disconnect.
  
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
    readonly "chats.activity": {
        readonly grant: "chats.activity";
        readonly target: null;
    };
    readonly "chats.activity_log": {
        readonly grant: "chats.activity_log";
        readonly target: null;
    };
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
    readonly "chats.delete": {
        readonly grant: "chats.delete";
        readonly target: null;
    };
    readonly "chats.link": {
        readonly grant: "chats.link";
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
    readonly "chats.remove": {
        readonly grant: "chats.remove";
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
    readonly "chats.restore": {
        readonly grant: "chats.restore";
        readonly target: null;
    };
    readonly "chats.schema": {
        readonly grant: "chats.schema";
        readonly target: null;
    };
    readonly "chats.tickets": {
        readonly grant: "chats.tickets";
        readonly target: null;
    };
    readonly "chats.watch": {
        readonly grant: "chats.watch";
        readonly target: null;
    };
};
