#!/bin/sh
# The decimal cross-check of the contract (brief test 2): N random cases per operation, the
# crate against Python's decimal (python:3.13-slim) for + - * / integer powers, sqrt, exp, ln
# and non-integer powers; sin, cos, atan2 and haversine against the reference tool run as a black
# box, when a datumesh-spec checkout is given.
#   tools/decimal_crosscheck.sh [N=10000] [SEED=1] [SPEC_DIR]
# Writes tools/out/. REF_IMAGE (default python:3.13-slim, PyNaCl installed on the fly) runs
# the reference.
set -e
N="${1:-10000}"; SEED="${2:-1}"; SPEC="$3"
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/tools/out"; mkdir -p "$out"
py() { docker run --rm -i -v "$root/tools:/t:ro" -v "$out:/o" python:3.13-slim python3 -I /t/decimal_crosscheck.py "$@"; }
py generate "$N" "$SEED" > "$out/decimal-cases.jsonl"
"$root/tools/cargo.sh" 1.99 build --release --example formula_runner >&2
runner() { docker run --rm -i -v "$root:/w:ro" -w /w rust:1.99 /w/target-1.99/release/examples/formula_runner; }
runner < "$out/decimal-cases.jsonl" > "$out/decimal-crate.jsonl"
py python /o/decimal-cases.jsonl > "$out/decimal-python.jsonl"
py compare /o/decimal-cases.jsonl /o/decimal-crate.jsonl /o/decimal-python.jsonl
if [ -n "$SPEC" ]; then
  py trig "$N" "$SEED" > "$out/trig-cases.jsonl"
  runner < "$out/trig-cases.jsonl" > "$out/trig-crate.jsonl"
  image="${REF_IMAGE:-python:3.13-slim}"
  install=""; [ "$image" = "python:3.13-slim" ] && install="pip install -q pynacl >/dev/null 2>&1 &&"
  docker run --rm -i -v "$SPEC:/s:ro" -v "$root/tools:/t:ro" "$image" sh -c "$install python3 /t/reference_runner.py /s" \
    < "$out/trig-cases.jsonl" > "$out/trig-reference.jsonl"
  python3 "$root/tools/compare.py" "$out/trig-cases.jsonl" "$out/trig-crate.jsonl" "$out/trig-reference.jsonl" 10
fi
