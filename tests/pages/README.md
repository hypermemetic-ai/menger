# tests/pages — a feature group's browser cases (SURFACE-1)

One file a ticket, `<group>.mjs`, `export default async (ctx) => { … }`, run
by `node tests/pages_browser.mjs --group <group>` alone or with every other
file after the board's cases (`--group all`, or no flag). `ctx` is the
runner's helpers: `test(name, body)`, `assert`, `eventually(what, fn, ms)`,
`sleep`, `typeBursts`, `openPage(route, { signedIn })` → `{ context, page,
seen }`, `marked`/`sameDocument` (no reload), `sy(activation, method,
...flags)` (synapse against the throwaway as the owner), `summaries`,
`extentsNow`, `groupsNow`, `section`/`main`/`nav`/`palette(page, B, …)`,
`rowIds`, `attrs`, `boardUp`, `BOARDS`, `server` (`origin`, `ws`), `token`,
`session`, `v1`/`v2` (the bundles pushed), `pages`, `fs`, `path`. A case
names its marks (`data-surface`, `[data-id]`, `[data-field]`,
`[data-action]`, `[data-form]`, `[data-refused]`).
