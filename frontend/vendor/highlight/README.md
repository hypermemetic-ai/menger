# Vendored: highlight.js 11.11.1 (BSD-3-Clause)

Every code block menger shows is highlighted (MENGER-22): in a plain body,
a framed body and `m-code`, a comment, a board note, a component's docs and
its source. The app must make no request to anything but menger itself
(MFE-2), so the library is vendored here and served by the shell like every
other dist asset — never a CDN, exactly as mermaid is (`../mermaid/`).

| | |
|---|---|
| package | `@highlightjs/cdn-assets` 11.11.1 (2024-12-25) — https://github.com/highlightjs/highlight.js |
| tarball | https://registry.npmjs.org/@highlightjs/cdn-assets/-/cdn-assets-11.11.1.tgz (registry shasum `136984ae467865e22080b3a4b65398a086e1ae7b`) |
| file | `highlight-11.11.1.min.js` = the tarball's `highlight.min.js` (the IIFE "common" build: defines the global `hljs`, with 37 languages registered) followed by `languages/haskell.min.js` (registers `haskell` on that global), concatenated byte for byte |
| sha256 of `highlight.min.js` | `c4a399dd6f488bc97a3546e3476747b3e714c99c57b9473154c6fb8d259b9381` |
| sha256 of `languages/haskell.min.js` | `58f159d73c815d95c1aac41abdc480c92d547c1b483ecf5e479f5591d8408c5e` |
| sha256 of `highlight-11.11.1.min.js` | `7dfd79dfe7dd4c04381955db3cae0dfe234bbba82a932407c5777262e4c464ef` |
| size | 129,761 bytes |
| license | BSD-3-Clause — `LICENSE` alongside is the package's own file, verbatim |

The house's languages (`frontend/src/highlight.rs`, `LANGUAGES`): rust,
typescript, javascript, json, toml (highlight.js's `ini`), yaml, bash, sql,
html/xml, css, markdown, haskell, python and diff — every one in this
bundle. A fence with no language is guessed from those alone; `text` or
`plain` stays plain.

No vendor stylesheet: the colours are menger's `--code-*` tokens
(index.html, and THEME-8's themes), mapped from the `hljs-*` classes by the
app's own CSS and the frame SDK's — so a theme recolours code with no
parse again.

How it is wired: `frontend/index.html` has a trunk `copy-file` link that
places the bundle at `dist/vendor/highlight-11.11.1.min.js`, so the server
serves it at `/app/vendor/highlight-11.11.1.min.js`. Nothing references it
at page load — `frontend/src/highlight.rs` injects a `<script>` for that
URL the first time a page holds a code block, and a frame's SDK does the
same with the URL its init block carries (`highlight.url`, beside
`mermaid.url`).

To upgrade: download the new tarball, concatenate `package/highlight.min.js`
and `package/languages/haskell.min.js` into `highlight-<version>.min.js`
here (delete the old one), copy `LICENSE`, update the version in
`frontend/index.html` (the copy-file href) and `frontend/src/highlight.rs`
(`ASSET_URL`), then update this table — the native test
`vendored_bundle_named_by_asset_url_exists` fails until the three agree.
