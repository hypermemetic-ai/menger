# Vendored: mermaid 11.17.2 (MIT)

The ticket page renders ```mermaid fences as SVG (AGS-14). mermaid has no
Rust renderer, and the app must make no request to anything but menger
itself (MFE-2), so the JS library is vendored here and served by the shell
like every other dist asset — never a CDN.

| | |
|---|---|
| package | `mermaid` 11.17.2 — https://github.com/mermaid-js/mermaid |
| file | `dist/mermaid.min.js` from the npm tarball (IIFE build; defines `globalThis.mermaid`; no dynamic `import()`, no runtime fetches) |
| tarball | https://registry.npmjs.org/mermaid/-/mermaid-11.17.2.tgz (registry shasum `e3caf3717582c0e44e5d04cff043e025caec7cba`) |
| sha256 of `mermaid-11.17.2.min.js` | `581ed7d74bd9048d0e3a91363927d72ef22942d7722546b27f7cc29e35390eb8` |
| size | 3,572,661 bytes |
| license | MIT — `LICENSE` alongside is the package's own file, verbatim |

How it is wired: `frontend/index.html` has a trunk `copy-file` link that
places the bundle at `dist/vendor/mermaid-11.17.2.min.js`, so the server
serves it at `/app/vendor/mermaid-11.17.2.min.js`. Nothing references it at
page load — `frontend/src/mermaid.rs` injects a `<script>` for that URL the
first time a rendered body actually contains a mermaid fence, then drives
`mermaid.initialize({ securityLevel: 'strict', … })` / `mermaid.render`
through js-sys. Pages without diagrams never fetch it.

To upgrade: download the new tarball, copy `package/dist/mermaid.min.js`
to `mermaid-<version>.min.js` here (delete the old one), copy `LICENSE`,
update the version in `frontend/index.html` (the copy-file href) and
`frontend/src/mermaid.rs` (`ASSET_URL`), then update this table — the
native test `vendored_asset_named_by_asset_url_exists` fails until the
three agree.
