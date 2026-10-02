#!/bin/sh
# COMPILE-4: a stand-in for diagrams/node/mermaid-parse.js — the same stdio
# protocol, misbehaving on cue — so tests/lifecycle.rs can drive the
# service's handling of a program that hangs, dies, babbles or answers out
# of turn without needing Node (or waiting on mermaid to misbehave). The
# driver runs it as `sh fake-node.sh --bundle <scenario>`:
#
#   how it starts, by the "bundle"'s name:  *fatal*   says `fatal`, exits
#                                           *silent*  never says anything
#                                           *exits*   exits before `ready`
#                                           anything else: `ready`
#   per request, by its source:             HANG   never answers
#                                           DIE    exits mid-request
#                                           NOISE  floods stderr first (far
#                                                  past a pipe's buffer)
#                                           JUNK   writes a line that is not
#                                                  the protocol and another
#                                                  request's answer first
#                                           SLOW   takes 200 ms
#   and then answers parsed { diagram_type: <the source> }, so every caller
#   can tell its own answer from another's.
#
# Sources are plain words: the parsing below is for this file's own tests,
# not for JSON.
echo "fake-node: starting" >&2
case "$2" in
  *fatal*)
    echo '{"type":"fatal","missing":"jsdom","message":"jsdom is not installed (fake) — run `npm ci --prefix fake`"}'
    exit 1 ;;
  *silent*) exec sleep 600 ;;
  *exits*) echo "fake-node: exiting before ready" >&2; exit 9 ;;
esac
echo '{"type":"ready","node":"fake","jsdom":"fake","load_ms":0}'
while IFS= read -r line; do
  id=${line#*\"id\":}
  id=${id%%,*}
  source=${line#*\"source\":\"}
  source=${source%\"\}}
  case "$source" in
    *HANG*) exec sleep 600 ;;
    *DIE*) echo "fake-node: dying on request $id" >&2; exit 3 ;;
    *NOISE*)
      i=0
      while [ "$i" -lt 3000 ]; do
        echo "fake-node: noise $i ....................................................................." >&2
        i=$((i + 1))
      done ;;
    *JUNK*)
      echo "this line is not the protocol"
      echo '{"id":999999999,"type":"parsed","diagram_type":"not yours"}' ;;
    *SLOW*) sleep 0.2 ;;
  esac
  printf '{"id":%s,"type":"parsed","diagram_type":"%s"}\n' "$id" "$source"
done
