#!/bin/sh
# Brief test 12, cross-language: the engine tests with the formula runtime (every formula-eval
# and excel vector, the brief's tests 6-9, the runtime's own) run with FORMULA_CROSSCHECK_DIR
# set, so every formula receipt the engine signs and the test verifies at level recomputed
# (through Engine as the resolver) is also written, with the key documents and the manifest of
# every slice entry, as JSON lines. Then the Python library datumesh 0.4.0 (PyPI, in
# python:3.13-slim) and the JavaScript library @datumesh/verify 0.4.0 (npm, in node:22-slim)
# verify each at level signed, check its receipt hash, and check that each manifest gives its
# slice entry.
#
#   tools/crosscheck.sh [toolchain]   cargo in a rust:<toolchain> container (tools/cargo.sh)
#   tools/crosscheck.sh --native      the local cargo and DATABASE_URL (CI)
#   tools/crosscheck.sh --verify DIR  only the verifiers, over the *.jsonl of DIR
set -e
root="$(cd "$(dirname "$0")/.." && pwd)"
tests="--test engine_vectors --test brief --test runtime"
if [ "$1" = "--verify" ]; then
  dump="$(cd "$2" && pwd)"
else
  dump="$root/target-crosscheck"
  rm -rf "$dump"
  mkdir -p "$dump"
  if [ "$1" = "--native" ]; then
    (cd "$root" && FORMULA_CROSSCHECK_DIR="$dump" cargo test --locked -q $tests)
  else
    FORMULA_CROSSCHECK_DIR=/w/target-crosscheck "$root/tools/cargo.sh" "${1:-1.99}" test --locked -q $tests
  fi
fi
cat "$dump"/*.jsonl > "$dump/all.txt"
echo "$(wc -l < "$dump/all.txt") receipt lines from $(ls "$dump"/*.jsonl | wc -l) files"
docker run --rm -v "$dump:/d:ro" -v "$root/tools/crosscheck:/s:ro" python:3.13-slim \
  sh -c 'pip install -q --disable-pip-version-check --root-user-action=ignore datumesh==0.4.0 && python -I /s/verify.py /d/all.txt'
docker run --rm -v "$dump:/d:ro" -v "$root/tools/crosscheck:/s:ro" node:22-slim \
  sh -c 'mkdir -p /tmp/x && cp /s/verify.mjs /tmp/x/ && cd /tmp/x && npm install -s --no-fund --no-audit @datumesh/verify@0.4.0 && node verify.mjs /d/all.txt'
