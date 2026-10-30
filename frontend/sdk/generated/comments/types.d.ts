import type { WatchEnd } from '../epics/types';
import type { Anchor, Comment, CommentIntent, CommentState } from '../tickets/types';
/** COMMENT-2: where on a page an anchor is — the element's path,
`[data-tag="ticket DEMAND-1"]`; `comments.list --path` matches a
prefix of it. */
export type AnchorPath = string;
/** One comment as it is now, its fields beside `type` — `add`'s,
`resolve`'s and `reopen`'s answer, and each of `list`'s rows.
[`CommentsEvent::into_comment`] gives the [`Comment`]. */
export interface CommentsEventComment {
    type: 'comment';
    anchor: Anchor;
    at: number;
    author: string;
    body: string;
    id: number;
    /** APP-23: `changes` for a change request; omitted for a comment. */
    intent: CommentIntent;
    /** IDENTITY-21: the signer's principal, when signed in. */
    principal: string | null;
    /** COMMENT-8: the thread's root this answers; omitted for a root. */
    replyTo: number | null;
    state: CommentState;
}
/** Where a `list` stands, once, after its rows: `next` is the cursor to
pass as the next call's `after` (a comment id), `null` when nothing
more matches. */
export interface CommentsEventPage {
    type: 'page';
    next: number | null;
}
/** `watch`: a comment was added. */
export interface CommentsEventAdded {
    type: 'added';
    comment: Comment;
}
/** `watch`: a comment was resolved. */
export interface CommentsEventResolved {
    type: 'resolved';
    comment: Comment;
}
/** `watch`: a resolved comment was reopened. */
export interface CommentsEventReopened {
    type: 'reopened';
    comment: Comment;
}
/** `watch`: this subscriber fell behind and `missed` changes were
dropped — `list` again to catch up. */
export interface CommentsEventLagged {
    type: 'lagged';
    missed: number;
}
/** ACCESS-12 (`watch`): the watch ended — its caller, verified again
when the grants moved or its sign-in ended, was refused. Last. */
export interface CommentsEventWatchClosed {
    type: 'watch_closed';
    message: string;
    reason: WatchEnd;
}
/** COMMENT-9: `report`'s answer — the failure as the server keeps it. */
export interface CommentsEventReported {
    type: 'reported';
    failure: Failure;
}
/** COMMENT-9: one of `failures`' rows, newest first. */
export interface CommentsEventFailure {
    type: 'failure';
    failure: Failure;
}
/** A refusal, naming the problem; nothing was kept or changed. */
export interface CommentsEventError {
    type: 'error';
    message: string;
}
/** What the `comments` activation's methods yield. In-band errors are
`Error { message }`, as every menger method's. */
export type CommentsEvent = CommentsEventComment | CommentsEventPage | CommentsEventAdded | CommentsEventResolved | CommentsEventReopened | CommentsEventLagged | CommentsEventWatchClosed | CommentsEventReported | CommentsEventFailure | CommentsEventError;
export declare function isCommentsEventComment(e: CommentsEvent): e is CommentsEventComment;
export declare function isCommentsEventPage(e: CommentsEvent): e is CommentsEventPage;
export declare function isCommentsEventAdded(e: CommentsEvent): e is CommentsEventAdded;
export declare function isCommentsEventResolved(e: CommentsEvent): e is CommentsEventResolved;
export declare function isCommentsEventReopened(e: CommentsEvent): e is CommentsEventReopened;
export declare function isCommentsEventLagged(e: CommentsEvent): e is CommentsEventLagged;
export declare function isCommentsEventWatchClosed(e: CommentsEvent): e is CommentsEventWatchClosed;
export declare function isCommentsEventReported(e: CommentsEvent): e is CommentsEventReported;
export declare function isCommentsEventFailure(e: CommentsEvent): e is CommentsEventFailure;
export declare function isCommentsEventError(e: CommentsEvent): e is CommentsEventError;
/** COMMENT-9: a comment the app could not keep, as it reported it — who,
when, where, what it said and why it was not kept — so "can you see the
failed submission?" has an answer. Kept in memory (the newest
[`FAILURES_KEPT`]; a restart forgets them) and logged; read with
`comments failures`. */
export interface Failure {
    /** The browser, as it names itself. */
    agent?: string | null;
    /** Where the comment was being left. */
    anchor: Anchor;
    /** When the server heard the report, unix seconds. */
    at: number;
    /** What it said (cut to [`BODY_MAX`]). */
    body: string;
    /** The reporter's principal. */
    by: string;
    /** When the app failed to keep it, unix seconds, as the app says (a
  report waits in the browser while the socket is down). */
    failedAt: number;
    /** The reporter's name, when it has one. */
    name?: string | null;
    /** Why it was not kept, in the app's words. */
    reason: string;
    /** 1, 2, … in the order reports arrived since the server started. */
    seq: number;
}
/** COMMENT-2: an app page, as its route — `#/ticket/DEMAND-2`, `#/` (the
board) — always beginning `#/`. */
export type Route = string;
/** A ticket or a shortcode — `comments.list`'s `under`: a ticket (it
and its descendants) or a shortcode (it and every shortcode hosted
under it), by any name [`TicketRef`] or [`ShortcodeRef`] takes. */
export type Under = string;
/** Stub type - definition missing from IR */
export type SchemaResult = unknown;
