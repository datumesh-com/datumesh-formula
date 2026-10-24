#!/bin/sh
# Run cargo in a toolchain container:
#   tools/cargo.sh <image-tag> <cargo args...>, e.g. tools/cargo.sh 1.99 test --release
# Tags: a Rust version (image rust:<tag>), nightly (rustlang/rust:nightly, for cargo-fuzz),
# lint (datumesh-rust-1.99-lint: rust:1.99 with clippy and rustfmt added).
set -e
tag="$1"; shift
case "$tag" in nightly) image=rustlang/rust:nightly ;; lint) image=datumesh-rust-1.99-lint ;; *) image="rust:$tag" ;; esac
root="$(cd "$(dirname "$0")/.." && pwd)"
exec docker run --rm -i -v "$HOME/.cache/datumesh-cargo:/usr/local/cargo/registry" -v "$root:/w" -w /w \
  -e CARGO_TARGET_DIR="/w/target-$tag" -e CARGO_TERM_COLOR=never \
  -e RUSTDOCFLAGS -e RUST_TEST_THREADS -e FORMULA_FUZZ_CASES -e FORMULA_ROUNDTRIP_CASES "$image" cargo "$@"
