import type { RpcClient } from '../../rpc';
import type { ChatsEvent, InputId, InputKind, InputState, InputValue, MessageId } from '../../chats/types';
import type { ChatId } from '../../media/types';
import type { TicketRef } from '../../notes/types';
import type { Message } from '../../tickets/types';
/** Typed client interface for chats.inputs plugin */
export interface ChatsInputsClient {
    /**
     * Answer a message's input — a value checked against its kind and options, with a rationale; who, the source and the time said by the server; yields input_changed { input }.
  
  By a participant of the chat, while the chat is open and the message
  kept. The value is typed as `tickets inputs answer`'s:
  `{"type": "choice", "option": …}`, `{"type": "multi", "options": […]}`,
  `{"type": "text", "text": …}`. Every answer is a history entry. A
  closed input, a value that does not answer it, or one the caller may
  not give is `refused { reasons }`, and nothing is written. `watch`
  carries the `input_changed` to every participant; it answers an
  `await_message` waiting on it.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `message_inputs.answer` anywhere it is held
     */
    answer(chat: ChatId, input: InputId, message: MessageId, value: InputValue, rationale?: Message | null): AsyncGenerator<ChatsEvent>;
    /**
     * Clear a message's input's answer (open again; kept in its history); yields input_changed { input }, or input { input } when it held none.
  
  By the one who gave the answer (any participant, for an answer kept
  before who answered was), while the chat is open. A closed input is
  `refused`.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `message_inputs.clear` anywhere it is held
     */
    clear(chat: ChatId, input: InputId, message: MessageId): AsyncGenerator<ChatsEvent>;
    /**
     * Close a message's input: freeze its answer (DECIDED, for a decision); yields input_changed { input }, or input { input } when it was closed.
  
  By the message's author — who asked — or an admin of the chat's
  ticket. An answer or a clear is refused until it is reopened.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `message_inputs.close` anywhere it is held
     */
    close(chat: ChatId, input: InputId, message: MessageId): AsyncGenerator<ChatsEvent>;
    /**
     * The inputs of the messages in the caller's chats (an <m-decision id> / <m-input id kind> in a message's body), oldest message first; an input { input } each, then page { next }.
  
  Each with its home (chat, message, the chat's ticket, who asked),
  kind, question, options, recommendation, state, answer (`by`, `name`,
  `source`, `at_unix`) and whole history. `chat` one chat (one the
  caller does not see is `not_found`), else every chat the caller is
  in; `ticket` only chats on it. `after` (a message id) continues a
  page; `limit` 100 unless given, at most 1,000 — a message's inputs
  are never split. A deleted message's are not listed.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `message_inputs.list` anywhere it is held
     */
    list(after?: MessageId | null, chat?: ChatId | null, kind?: InputKind | null, limit?: number | null, state?: InputState | null, ticket?: TicketRef | null): AsyncGenerator<ChatsEvent>;
    /**
     * Reopen a closed message's input; yields input_changed { input }, or input { input } when it was not closed.
  
  By the message's author or an admin of the chat's ticket: `answered`
  again when it holds an answer, else `open`.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `message_inputs.reopen` anywhere it is held
     */
    reopen(chat: ChatId, input: InputId, message: MessageId): AsyncGenerator<ChatsEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `message_inputs.schema` anywhere it is held
     */
    schema(): Promise<unknown>;
}
/** Create a typed chats.inputs client from an RPC client */
export declare function createChatsInputsClient(rpc: RpcClient): ChatsInputsClient;
/** What each chats.inputs method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "message_inputs.answer": {
        readonly grant: "message_inputs.answer";
        readonly target: null;
    };
    readonly "message_inputs.clear": {
        readonly grant: "message_inputs.clear";
        readonly target: null;
    };
    readonly "message_inputs.close": {
        readonly grant: "message_inputs.close";
        readonly target: null;
    };
    readonly "message_inputs.list": {
        readonly grant: "message_inputs.list";
        readonly target: null;
    };
    readonly "message_inputs.reopen": {
        readonly grant: "message_inputs.reopen";
        readonly target: null;
    };
    readonly "message_inputs.schema": {
        readonly grant: "message_inputs.schema";
        readonly target: null;
    };
};
/** The chats.inputs activation's default roles, each the grants it lists (AUTH-3). Defaults only: a server's roles are data an admin may change, and `admin` holds every grant. */
export declare const defaultRoles: {
    readonly chatter: readonly ["message_inputs.list", "message_inputs.answer", "message_inputs.clear", "message_inputs.close", "message_inputs.reopen"];
    readonly editor: readonly ["message_inputs.list", "message_inputs.answer", "message_inputs.clear", "message_inputs.close", "message_inputs.reopen"];
    readonly viewer: readonly ["message_inputs.list", "message_inputs.answer", "message_inputs.clear", "message_inputs.close", "message_inputs.reopen"];
};
