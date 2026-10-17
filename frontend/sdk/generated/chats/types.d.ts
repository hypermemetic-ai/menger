/** One chat, as one caller sees it: `unread` is the caller's own count —
the messages after its read mark (zero for an admin who is not in it). */
export interface Chat {
    /** CHAT-8: how many messages it holds — the app's spacer for what it
  has not fetched. */
    count?: number;
    /** Its id: 1, 2, … in the order chats were opened, never reused. */
    id: ChatId;
    /** The time of its last message — its opening, until one is posted. */
    lastAt: number;
    /** Its last message's id, when it has one. */
    lastMessage?: MessageId | null;
    /** CHAT-8: the caller's read mark — the last message it has read
  (none: nothing read, or not a participant). The app opens a chat
  with more unread than a page around it. */
    mark?: MessageId | null;
    openedAt: number;
    /** Who opened it, and when (unix seconds). */
    openedBy: string;
    /** Every participant, by principal (`anonymous` on one kept before ACCESS-24, when login could be off), in
  the order they joined — the opener first. */
    participants: string[];
    state: ChatState;
    /** The ticket it was opened on, by its key as it stands now. */
    ticket: string;
    title?: string | null;
    /** The caller's unread messages in it. */
    unread: number;
    /** The participants waiting on `await_message` for it now (CHAT-1's
  "waiting for a message"): who is listening, as the pane shows it. */
    waiting?: string[];
}
/** CHAT-3: a chat's number, from 1 in the order opened. */
export type ChatId = number;
/** Where a chat stands: open, or closed (read-only) (any case) */
export type ChatState = "open" | "closed";
/** `open`'s answer, and on `watch` a chat opened with the caller in it.
`existing`: the same participants already had this chat open on the
ticket, and nothing was made. */
export interface ChatsEventOpened {
    type: 'opened';
    chat: Chat;
    existing: boolean;
}
/** One of `list`'s rows. */
export interface ChatsEventChat {
    type: 'chat';
    chat: Chat;
}
/** CHAT-4: one of `candidates`' rows — a principal who could be named in
a chat on the ticket, and whether it can read the ticket now (one
that cannot is shown in the picker and cannot be picked). */
export interface ChatsEventCandidate {
    type: 'candidate';
    principal: string;
    reads: boolean;
}
/** `post`'s answer, and on every participant's `watch`. */
export interface ChatsEventPosted {
    type: 'posted';
    message: Message;
}
/** One of `messages`' rows (oldest first), and `await_message`'s answer. */
export interface ChatsEventMessage {
    type: 'message';
    message: Message;
}
/** `add`'s answer, and on `watch`: the chat with its participants now. */
export interface ChatsEventAdded {
    type: 'added';
    chat: Chat;
}
/** `read`'s answer, and on the reader's own other `watch`es: its mark
in `chat` is `upto`, and `unread` are after it. */
export interface ChatsEventRead {
    type: 'read';
    chat: ChatId;
    unread: number;
    upto: MessageId;
}
/** `close`'s answer, and on `watch`. */
export interface ChatsEventClosed {
    type: 'closed';
    chat: ChatId;
}
/** `reopen`'s answer, and on `watch`. */
export interface ChatsEventReopened {
    type: 'reopened';
    chat: ChatId;
}
/** CHAT-7: `rename`'s answer, and on `watch`: the chat with its title now. */
export interface ChatsEventRenamed {
    type: 'renamed';
    chat: Chat;
}
/** On `watch`: `principal` began (`waiting: true`) or stopped waiting on
`await_message` for `chat` — the pane's "waiting for a message". */
export interface ChatsEventListening {
    type: 'listening';
    chat: ChatId;
    principal: string;
    waiting: boolean;
}
/** AGENT-3: a participating agent's claim began or ended — a line in the
chat. Declared for AGENT-3's claims; nothing emits it yet. */
export interface ChatsEventClaimLine {
    type: 'claim_line';
    agent: string;
    at: number;
    began: boolean;
    chat: ChatId;
    holder: string;
}
/** `await_message`'s first event: waiting for a message in `chat` (or
any of the caller's chats, on `ticket` when one is named), from
`since` (unix seconds) — what to pass again after a dropped
connection. */
export interface ChatsEventWaiting {
    type: 'waiting';
    chat: ChatId | null;
    since: number;
    ticket: string | null;
}
/** `await_message`: nothing came within `timeout_minutes`. Last. */
export interface ChatsEventTimedOut {
    type: 'timed_out';
}
/** Where `list` or `messages` stands, once, after its rows: `next` is
the cursor to pass as the next call's `after`, `null` when nothing
more matches. */
export interface ChatsEventPage {
    type: 'page';
    next: number | null;
}
/** On `watch`: the caller can no longer read `chat` (its reach on the
ticket went); it is not found for it from now. */
export interface ChatsEventLeft {
    type: 'left';
    chat: ChatId;
}
/** The chat is not one the caller sees — not there, or not its. */
export interface ChatsEventNotFound {
    type: 'not_found';
    chat: ChatId;
}
/** Refused in band, every reason named; nothing was written. */
export interface ChatsEventRefused {
    type: 'refused';
    reasons: string[];
}
/** `watch`: this subscriber fell behind and `missed` changes were
dropped — `list` again to catch up. */
export interface ChatsEventLagged {
    type: 'lagged';
    missed: number;
}
/** ACCESS-12 (`watch`, `await_message`): the stream ended — its caller,
verified again when the grants moved or its sign-in ended, was
refused. Last. */
export interface ChatsEventWatchClosed {
    type: 'watch_closed';
    message: string;
    reason: WatchEnd;
}
export interface ChatsEventError {
    type: 'error';
    message: string;
}
/** What the `chats` activation's methods yield. In-band errors are
`Error { message }`, as every menger method's; a chat the caller does
not see is `not_found`, whether it is there or not (ACCESS-9). */
export type ChatsEvent = ChatsEventOpened | ChatsEventChat | ChatsEventCandidate | ChatsEventPosted | ChatsEventMessage | ChatsEventAdded | ChatsEventRead | ChatsEventClosed | ChatsEventReopened | ChatsEventRenamed | ChatsEventListening | ChatsEventClaimLine | ChatsEventWaiting | ChatsEventTimedOut | ChatsEventPage | ChatsEventLeft | ChatsEventNotFound | ChatsEventRefused | ChatsEventLagged | ChatsEventWatchClosed | ChatsEventError;
export declare function isChatsEventOpened(e: ChatsEvent): e is ChatsEventOpened;
export declare function isChatsEventChat(e: ChatsEvent): e is ChatsEventChat;
export declare function isChatsEventCandidate(e: ChatsEvent): e is ChatsEventCandidate;
export declare function isChatsEventPosted(e: ChatsEvent): e is ChatsEventPosted;
export declare function isChatsEventMessage(e: ChatsEvent): e is ChatsEventMessage;
export declare function isChatsEventAdded(e: ChatsEvent): e is ChatsEventAdded;
export declare function isChatsEventRead(e: ChatsEvent): e is ChatsEventRead;
export declare function isChatsEventClosed(e: ChatsEvent): e is ChatsEventClosed;
export declare function isChatsEventReopened(e: ChatsEvent): e is ChatsEventReopened;
export declare function isChatsEventRenamed(e: ChatsEvent): e is ChatsEventRenamed;
export declare function isChatsEventListening(e: ChatsEvent): e is ChatsEventListening;
export declare function isChatsEventClaimLine(e: ChatsEvent): e is ChatsEventClaimLine;
export declare function isChatsEventWaiting(e: ChatsEvent): e is ChatsEventWaiting;
export declare function isChatsEventTimedOut(e: ChatsEvent): e is ChatsEventTimedOut;
export declare function isChatsEventPage(e: ChatsEvent): e is ChatsEventPage;
export declare function isChatsEventLeft(e: ChatsEvent): e is ChatsEventLeft;
export declare function isChatsEventNotFound(e: ChatsEvent): e is ChatsEventNotFound;
export declare function isChatsEventRefused(e: ChatsEvent): e is ChatsEventRefused;
export declare function isChatsEventLagged(e: ChatsEvent): e is ChatsEventLagged;
export declare function isChatsEventWatchClosed(e: ChatsEvent): e is ChatsEventWatchClosed;
export declare function isChatsEventError(e: ChatsEvent): e is ChatsEventError;
/** One message, as kept. */
export interface Message {
    /** When, unix seconds. */
    at: number;
    /** Who wrote it: the caller's principal (`anonymous` on one kept before ACCESS-24, when login could be off). */
    author: string;
    /** What it says: markdown, rendered by the app's `md.rs`, never framed. */
    body: string;
    chat: ChatId;
    /** Its id: 1, 2, … across every chat, in the order posted, never reused
  — so a later message always has a greater id. */
    id: MessageId;
    /** The name the author went by when it wrote it, when signed in with one. */
    name?: string | null;
    /** AGENT-3: the process that held the agent who wrote it (its claimant).
  Kept, never set by a call; `None` until agents claim. */
    via?: string | null;
}
/** CHAT-3: a chat message's number, from 1 across every chat in the
order posted — a later message's is always greater. */
export type MessageId = number;
/** ACCESS-12: why an open watch ended — its caller verified again when the
grants moved (or its sign-in ended), and refused. Every watch (`tickets`,
`comments`, `themes`, `deployments`) ends with one `watch_closed { reason,
message }` and nothing after it. */
export type WatchEnd = "signed_out" | "expired" | "revoked" | "refused";
