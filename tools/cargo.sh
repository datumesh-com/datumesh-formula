#!/bin/sh
# Run cargo in a toolchain container on the `datumesh` Docker network, where the
# development database (a postgres:16 container named datumesh-pg) answers:
#   tools/cargo.sh <image-tag> <cargo args...>, e.g. tools/cargo.sh 1.99 test --release
# Tags: a Rust version (image rust:<tag>), nightly (rustlang/rust:nightly, for cargo-fuzz),
# nightly-tools (datumesh-nightly-tools: cargo-public-api), lint (datumesh-rust-1.99-lint:
# rust:1.99 with clippy and rustfmt added).
set -e
tag="$1"; shift
case "$tag" in nightly) image=rustlang/rust:nightly ;; nightly-tools) image=datumesh-nightly-tools ;; lint) image=datumesh-rust-1.99-lint ;; *) image="rust:$tag" ;; esac
root="$(cd "$(dirname "$0")/.." && pwd)"
if [ "$tag" = lint ] && ! docker image inspect "$image" >/dev/null 2>&1; then
  printf 'FROM rust:1.99\nRUN rustup component add clippy rustfmt\n' | docker build -q -t "$image" - >&2
fi
exec docker run --rm -i --network datumesh -v "$HOME/.cache/datumesh-cargo:/usr/local/cargo/registry" -v "$root:/w" -w /w \
  -e CARGO_TARGET_DIR="/w/target-$tag" -e CARGO_TERM_COLOR=never \
  -e DATABASE_URL="${DATABASE_URL:-postgres://postgres:postgres@datumesh-pg:5432/postgres}" \
  -e RUSTDOCFLAGS -e RUST_TEST_THREADS -e FORMULA_FUZZ_CASES -e FORMULA_ROUNDTRIP_CASES -e FORMULA_ENGINE_CASES \
  "$image" cargo "$@"
