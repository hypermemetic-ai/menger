# Vendored: the Public Suffix List, 2026-01-16 (MPL-2.0)

MENGER-14's leave dialog emphasises a link's **registrable domain** — the
part of the host a single owner controls (`example.co.uk`, `alice.github.io`)
— and "Don't ask again" trusts exactly that. Which part that is cannot be
guessed from the dots; it is the Public Suffix List's to say. The app makes
no request to anything but menger itself, so the list rides in the wasm.

| | |
|---|---|
| list | the Public Suffix List — https://publicsuffix.org/ (ICANN and private sections) |
| file | `public_suffix_list.dafsa` — libpsl's compiled form (`psl-make-dafsa`, header `.DAFSA@PSL_0`: ASCII/punycode keys), as Fedora ships it in `publicsuffix-list-dafsa-20260116-1.fc44` (`/usr/share/publicsuffix/public_suffix_list.dafsa`) |
| sha256 | `dc930790e147430ceb20bdf83e1492e697da9a401b564deb14b45bbd3ffe6cee` |
| size | 55,336 bytes |
| license | MPL-2.0 — https://mozilla.org/MPL/2.0/ (the list's own license; the file is the list, unmodified, in another encoding) |

How it is read: `frontend/src/links.rs` `include_bytes!`s it, checks the
header, and walks the graph with Chromium's `LookupStringInFixedSet`
(net/base/lookup_string_in_fixed_set.cc), whose format libpsl's
`psl-make-dafsa` writes. Each key's value is libpsl's flags: `1` exception
rule, `2` wildcard rule, `4` ICANN section, `8` private section. The
registrable domain is then the PSL algorithm over those flags
(`links::registrable`), pinned by native tests against the list's own test
vectors.

To upgrade: copy a newer `public_suffix_list.dafsa` (or build one from
`public_suffix_list.dat` with libpsl's `psl-make-dafsa --output-format=binary`)
over this file and update the table; `cargo test` runs the vectors.
