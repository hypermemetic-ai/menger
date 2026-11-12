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

## From hyperforge: the `menger-release` pipeline

DIST-25. There is no GitHub workflow (owner, 2026-11-12: "the workflow just
runs too much. I say we run it locally as a hyperforge hook"). The archive is
built on this machine by a pipeline in `.hyperforge/pipelines.toml`, and
nothing starts it but `start`:

```sh
HF="synapse -P 44104 lforge-v5 hyperforge"
$HF ci add --path ~/dev/controlflow/hypermemetic/menger   # after the file changes
$HF ci menger-release start                               # `package` → run N
$HF ci menger-release log --run N --stage package
$HF ci menger-release trigger --run N --stage attach      # the owner's word: the upload
```

- **`package`** (`deploy/release/stage package`) builds the tag `v<version>`
  — the version the checkout's `Cargo.toml` says — with `scripts/package --at
  v<version> --container rust:1-bullseye --check`: from a throwaway worktree
  of the tag, so the archive is the tag's tree and never the checkout's tip
  or its stray files. Its outputs are `archive`, `sha256` and `tag`.
- **`attach`** (`deploy/release/stage attach`) is the only step that leaves
  the machine: hyperforge's `build release_asset` puts the archive and its
  `.sha256` on the tag's release. It refuses a tag GitHub does not have and
  an asset already there.

So the owner's steps 3 and 4 above become: push the tag, `start`, read the
log, `trigger … attach`.

## Other targets

A run makes the archive for the target of the machine it runs on: dream
gives `x86_64-unknown-linux-gnu`. `aarch64-apple-darwin` (and the other
Mac and ARM targets) need the same on a machine of that kind — hyperforge
there, or by hand: `deploy/release/stage package` (no container off Linux:
that is Linux's glibc), then `gh release upload v<version>
dist/release/menger-*`. menger has not been built on macOS.

`x86_64-unknown-linux-musl` (one static binary for every Linux; binstall
tries it second) needs a musl C toolchain for SQLite and ring, which is
not installed here.
