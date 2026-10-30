import type { RpcClient } from '../rpc';
import { MethodStream } from '../rpc';
import type { ChatId, MediaData, MediaEvent, MediaHash, MediaType, SchemaResult } from './types';
import type { TicketRef } from '../notes/types';
/** Typed client interface for media plugin */
export interface MediaClient {
    /**
     * An image's bytes by its hash; yields media { hash, mime, bytes, width, height, data } (data base64), or not_found.
  
  Yields exactly one event: `media` — the image as kept, its bytes in
  standard base64 — or `not_found { hash }` for a hash that names no
  image, and alike for one put only for tickets the caller cannot see
  (ACCESS-10: not found ≡ not yours). COMMENT-10: with `ticket`, held
  to that ticket's body — the image only when it was put for that
  ticket (one kept for another ticket, a chat or the app is
  `not_found` there). The bytes are read from the database, never the
  cache.
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `media.get` anywhere it is held
     */
    get(hash: MediaHash, ticket?: TicketRef | null): MethodStream<'media.get', MediaEvent>;
    /**
     * Keep an image for a ticket (PNG, JPEG, GIF or WebP, at most 7 MiB, base64); yields stored { hash, mime, bytes, width, height, duplicate }.
  
  The image is named by its SHA-256 (`hash`, 64 hex digits); a body
  shows it as `![alt](media:<hash>)`. Its type is read from its own
  magic bytes — PNG, JPEG, GIF or WebP, never SVG — and must be the
  `mime` given; its width and height from its header. At most 7 MiB
  (an image travels as base64 in one WebSocket message, whose limit is
  10 MiB). Yields exactly one event: `stored` — `duplicate: true` when
  the same bytes were kept already (one row: nothing new is stored but,
  for a second ticket, that it is that one's too) — or `refused {
  reason, message, bytes?, max? }` (`too_large`, `not_an_image`,
  `type_mismatch`, `truncated`, `bad_data`), naming why, with nothing
  kept; `not_found { ticket }` for a ticket that is none or out of
  reach. The image belongs to `ticket`: `get` answers it to whoever can
  see that ticket (an editor's: a viewer is answered `not_found`).
  CHAT-14: with `chat` instead, it is that chat's — `get` answers it to
  its participants alone; `not_found { hash: null }` for a chat the
  caller is not in. Without `ticket` (a comment on the app) it is the
  putter's, and the admins' of `*`. Through synapse the data is a file
  (`base64 -w0 shot.png > shot.b64`, then `--data @shot.b64`) or stdin
  (`--data -`), never `"$(base64 …)"`, which Linux caps at 128 KB
  (README "Comments").
     * @server-derived auth
     * @requiresAuth — resolver: self.access.verify
     * @requires `media.put` on the scope of `ticket`
     */
    put(data: MediaData, mime: MediaType, chat?: ChatId | null, ticket?: TicketRef | null): MethodStream<'media.put', MediaEvent>;
    /**
     * Get plugin or method schema. Pass {"method": "name"} for a specific method.
     * @requires `media.schema` anywhere it is held
     */
    schema(method?: string | null): Promise<SchemaResult>;
}
/** Create a typed media client from an RPC client */
export declare function createMediaClient(rpc: RpcClient): MediaClient;
/** What each media method needs (AUTH-9): its grant, and the param that locates what it acts on (`null`: allowed wherever the grant is held). The gate decides, server-side. */
export declare const requires: {
    readonly "media.get": {
        readonly grant: "media.get";
        readonly target: null;
    };
    readonly "media.put": {
        readonly grant: "media.put";
        readonly target: "ticket";
    };
    readonly "media.schema": {
        readonly grant: "media.schema";
        readonly target: null;
    };
};
