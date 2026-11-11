# A release people install: `cargo binstall menger`

DIST-22. `scripts/package` makes the archive; everything after it reaches
outward and is **the owner's, each step on its own word** — nothing here
tags, pushes, releases or publishes, and `scripts/tests/package` holds the
script to that.

## What binstall asks for

`Cargo.toml`'s `[package.metadata.binstall]` (read from the crate on
crates.io, so the publish comes before a release is of use):

    <repository>/releases/download/v<version>/menger-<target>-v<version>.tar.gz
    └ menger-<target>-v<version>/menger        (and LICENSE, README.md)

One archive per target, each an asset of the GitHub release tagged
`v<version>`. A target with no archive falls back to compiling (ten
minutes, and the UI only if the crate was published after
`scripts/embed-ui`).

## Make the archive

```sh
scripts/package --container rust:1-bullseye --check --binstall
```

- **`--container`**: built inside that image, the binary needs glibc 2.30
  or newer (Debian 11, Ubuntu 20.04, RHEL 9 and everything after). Built
  on this machine it needs this machine's glibc (2.39 on 2026-11-11), and
  is refused by Ubuntu 22.04 and Debian 12 — never ship that one.
- The UI is built from `ui/` (`scripts/embed-ui`), or `--ui TAR` takes a
  bundle already built (a pipeline release's `dist/ui.tar`, stamped against
  the menger it was built with). `embed/` is left as it was.
- The profile is `dist`: release, thin LTO, stripped — 85 MB, 31 MB packed
  (the pipeline's perf build is 260 MB, most of it line tables).
- **`--check`** runs the packed binary on a HOME that has never seen
  menger: `--version`, a start on free ports, its sign-in link, and `/app/`
  serving the UI it carries rather than the placeholder.
- **`--binstall`** serves the archive on loopback at the release's own
  path and has `cargo-binstall` install it by the manifest's URL template
  into a scratch directory — the names proven, with nothing published.
- A tree with uncommitted changes is refused; `--allow-dirty` is a
  rehearsal. The same tree packs to the same bytes.

## The owner's steps, in order

1. **The version.** 0.7.1 is on crates.io for all five crates, from before
   the binary carried the UI; the tree has moved since. The first version
   that carries the UI and `menger start` is a bump of all five
   (`menger-quota`, `menger-tickets-api`, `menger-docs`, `menger-media`,
   `menger`) and of each `version = "…"` they name each other by.
2. **crates.io**, in dependency order — `menger-quota`,
   `menger-tickets-api`, `menger-docs`, `menger-media`, then, after
   `scripts/embed-ui`, `cargo publish -p menger --allow-dirty` (the bundle
   is ignored by git, which cargo calls dirty). `cargo install menger`
   then serves the app.
3. **The tag**, `v<version>`, on the commit that was published, pushed.
4. **The GitHub release** for that tag, with the archives and their
   `.sha256` files as assets:
   `gh release create v<version> dist/release/menger-*-v<version>.tar.gz*`.
5. **Check it from nothing**: on a machine (or container) with only
   cargo-binstall, `cargo binstall menger && menger start`.

## Other targets

`x86_64-unknown-linux-gnu` is built here. `aarch64-unknown-linux-gnu`,
`aarch64-apple-darwin` and `x86_64-apple-darwin` need a machine of that
kind or a cross toolchain, which this one has neither of.
`github-release.yml` beside this file is a workflow that builds each on a
runner of its own when a `v*` tag is pushed and attaches the archives to
the release. **It has never run** and is not in `.github/workflows/`:
putting it there is the owner's decision (the repository has no workflows
today), and menger has not been built on macOS.

`x86_64-unknown-linux-musl` (one static binary for every Linux; binstall
tries it second) needs a musl C toolchain for SQLite and ring, which is
not installed here.
