// CHAT-66: a message as the bridge shows it to the agent — its head, and,
// for a reply, what it answers: `(replying to #N: who: "first words")`, the
// body without its `[↩](message:N)` token (the server reads the token off
// and answers `reply_to` and `quote` with the message, so this never parses
// the original). tests/reply.test.mjs holds it (`node --test`).

/** The body without its leading reply token (as tickets-api's chat_reply). */
export const withoutToken = (body) => String(body ?? "").replace(/^\s*\[[^\]\n]*\]\(message:\d+\)[ \t]*\r?\n?/, "");

/** What a reply says of its original. */
export function quoteText(q) {
  if (!q) return "a message you can't see";
  if (q.deleted) return "a deleted message";
  const who = q.name ? q.name : q.author;
  return q.snippet ? `${who}: "${q.snippet}"` : who;
}

/** One message for the agent's prompt. */
export function fmtMessage(m) {
  const who = m.name ? `${m.name} (${m.author})` : m.author;
  const at = new Date(m.at * 1000).toISOString().slice(0, 16).replace("T", " ");
  const replying = m.reply_to ? `(replying to #${m.reply_to}: ${quoteText(m.quote)})\n` : "";
  return `[message ${m.id} · ${who} · ${at}Z]\n${replying}${m.reply_to ? withoutToken(m.body) : m.body}`;
}
