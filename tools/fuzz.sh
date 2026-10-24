#!/bin/sh
# Fuzz a target with cargo-fuzz in a nightly container (image datumesh-fuzz: rustlang/rust:nightly
# with cargo-fuzz installed), seeded from fuzz/seeds:
#   tools/fuzz.sh <parse|evaluate> [seconds=120]
set -e
target="$1"; secs="${2:-120}"
root="$(cd "$(dirname "$0")/.." && pwd)"
if ! docker image inspect datumesh-fuzz >/dev/null 2>&1; then
  printf 'FROM rustlang/rust:nightly\nRUN cargo install cargo-fuzz --locked\n' | docker build -q -t datumesh-fuzz - >&2
fi
mkdir -p "$root/fuzz/corpus/$target"
cp "$root"/fuzz/seeds/* "$root/fuzz/corpus/$target/"
exec docker run --rm -v "$HOME/.cache/datumesh-cargo:/usr/local/cargo/registry" -v "$root:/w" -w /w/fuzz \
  -e CARGO_TARGET_DIR=/w/fuzz/target datumesh-fuzz \
  cargo fuzz run "$target" "corpus/$target" -- -max_total_time="$secs" -max_len=4096 -print_final_stats=1
