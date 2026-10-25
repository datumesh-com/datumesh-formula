#!/bin/sh
# The library's public API (`cargo public-api -ss`, which needs a nightly toolchain and
# cargo-public-api 0.52): public-api.txt with default features (engine), public-api-no-engine.txt
# with --no-default-features; each compared with the file, or written with --write. Plain output
# whatever CARGO_TERM_COLOR says (CI sets it to always): --color=never.
#
#   tools/public_api.sh [--write]            in the datumesh-nightly-tools image (tools/cargo.sh)
#   tools/public_api.sh --native [--write]   with the local cargo (CI: nightly-2026-10-22)
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
native=false
write=false
for a in "$@"; do
  case "$a" in --native) native=true ;; --write) write=true ;; *) echo "unknown option $a" >&2; exit 2 ;; esac
done
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

api() {
  if $native; then
    (cd "$root" && cargo public-api -ss --color=never "$@")
  else
    "$root/tools/cargo.sh" nightly-tools public-api -ss --color=never "$@"
  fi
}
api > "$work/default.txt"
api --no-default-features > "$work/no-engine.txt"
# Without the engine, nothing of it; with it, only additions.
if grep -q 'datumesh_formula::runtime' "$work/no-engine.txt"; then
  echo "the runtime in the API without the engine feature" >&2
  exit 1
fi
if [ -n "$(diff "$work/no-engine.txt" "$work/default.txt" | grep '^<' || true)" ]; then
  echo "the engine feature removes items:" >&2
  diff "$work/no-engine.txt" "$work/default.txt" >&2
  exit 1
fi

if $write; then
  cp "$work/default.txt" "$root/public-api.txt"
  cp "$work/no-engine.txt" "$root/public-api-no-engine.txt"
  echo "public-api.txt and public-api-no-engine.txt written"
else
  diff -u "$root/public-api.txt" "$work/default.txt"
  diff -u "$root/public-api-no-engine.txt" "$work/no-engine.txt"
  echo "public-api.txt and public-api-no-engine.txt are current"
fi
