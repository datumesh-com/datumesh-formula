# API changes in datumesh-formula 0.2.0

Generated with `cargo public-api -ss --color=never diff 0.1.1` (cargo-public-api 0.52.0,
nightly-tools image) against 0.1.1 on crates.io, with the default features (`engine`) and with
`--no-default-features`, built against `datumesh-engine` 0.4.0 (a local copy of its package
until it is on crates.io); the full output is at the end. **No item removed, changed or
added.** `public-api.txt` and `public-api-no-engine.txt` are regenerated (unchanged).

The release is breaking all the same, with feature `engine`: the listing names the engine's and
the store's types without their version (`datumesh_engine::runtime::Runtime`, `Inputs`,
`Budget`, `EvalError`; `datumesh_store::Store`, `datumesh_store::pg::PgStore`,
`datumesh_store::error::Error`), and those are now `datumesh-engine` 0.4.0's and
`datumesh-store` 0.4.0's, semver-incompatible releases. Users move to the three together.
Without feature `engine` nothing changed.

Behaviour: none changed. Code: none changed. Dependencies: `datumesh-engine` 0.4.0 (was
0.3.2) and `datumesh-store` 0.4.0 (was 0.3.2), both optional (feature `engine`);
`datumesh-core` 0.3.6 as before.

## Full diff

Both feature sets give the same output:

```text
Removed items from the public API
=================================
(none)

Changed items in the public API
===============================
(none)

Added items to the public API
=============================
(none)

```
