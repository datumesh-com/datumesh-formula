# Changelog

All notable changes to `datumesh-formula`.

## 0.2.0 (unreleased)

On `datumesh-engine` 0.4.0 and `datumesh-store` 0.4.0 (the optional feature `engine`; were
0.3.2 and 0.3.2), so that the formula runtime runs on the store's SQLite backend as well as on
PostgreSQL. Nothing else changed: no code, no behaviour (`API-CHANGES-0.2.0.md`).

- Breaking only because, with feature `engine`, the public API names the engine's and the
  store's types (`runtime::FormulaRuntime::new` takes an `Arc<dyn Store + Send + Sync>`, the
  runtime implements the engine's `Runtime`, `CallEnv` is implemented for `PgStore` and for
  `dyn Store + Send + Sync`), which are now engine 0.4's and store 0.4's: users move to the
  three together. Without feature `engine` the API is unchanged.

## 0.1.1 (2026-11-02)

For spec-0.4.2 (the tag, `fc870ab`), on `datumesh-core` 0.3.6, `datumesh-engine` 0.3.2 and
`datumesh-store` 0.3.2. Additive (`API-CHANGES-0.1.1.md`).

- `call::parse_call`: calls (SPEC 5.8), `name@<fn_hash>(args)?asof=…`, against a `CallEnv`
  (aliases, bindings as of the asof, descriptors by hash; `MapCallEnv` in memory, a store with
  feature `engine`), with 5.8's check order and codes; `Call::uri` is the cell URI with
  `fn_hash=` before `asof` (the core's `format_reference`; `Call::reference` is the core's
  `Reference`). It needs no engine. Every `call` vector passes (10 valid, 17 invalid).
- The pin token (15.2 L4a); a pinned read in a body is `E_FORMULA_PLACEMENT`
  (`invalid/formula-pinned-read`).
- `fn_hash` is a reserved argument name: `E_CANON_ARG_NAME` in `check_descriptor` and
  `define_function` (datumesh-core 0.3.6's descriptor check).
- Built-ins `text_of(blob)` (a `text/plain` blob's content as an NFC string, from
  `World::blob` by SHA-256, checked for length and hash) and `remove_chars(text, chars)`, their
  fuel, and the evaluation code `E_FORMULA_BLOB_UNAVAILABLE` (the evaluator's failure). A
  `formula-eval` world's `blobs`. The runtime reads blob content with engine 0.3.2's
  `Inputs::read_blob`; content the store lacks is the engine's `BlobUnavailable`.
- Vectors: every vector of the spec at the `spec-0.4.2` tag (1,146) is vendored; `formula-eval` 125 valid
  and 39 invalid, `excel` 79, `formula` 172 and 123, `call` 10 and 17 pass.
- Through the engine every `formula-eval` and `excel` vector now gives the vector's value,
  lineage, receipt hash and fuel exactly (engine 0.3.1 fixed `member-fault-caught`'s fuel to 7
  and the order of `E_FORMULA_FUTURE`; 0.3.2 gives blob content).
- A `call` cargo-fuzz target.

## 0.1.0 (2026-10-25)

The formula dialect of spec-0.4.0 (section 15), on `datumesh-core` 0.3.4 and, with the default
feature `engine`, `datumesh-engine` 0.3.0 and `datumesh-store` 0.3.1.

### With the engine (step 1.3, phase B)

- `runtime::FormulaRuntime`: the engine's `Runtime` for `lang = "formula"`, the evaluator over the
  engine's `Inputs` (cell, transaction-time, slice and version reads, bindings). Fuel as 15.12
  (the engine charges reads, the evaluator nodes and built-ins); faults as `EvalError::Fault`, so
  the engine answers `not_covered` with the fault; `asof_dependent` from the checker's flag. A
  descriptor is checked against its callees as bound now, and again against a callee rebound
  (15.8); `stats()` counts evaluations.
- `definition::define_function`: the canonical formula descriptor from a body, a signature with
  `order`, an alias map and the callees as bound at define time (`Callees`: a store, or an
  `Env`); `publish` stores it and binds the name.
- `check_descriptor` checks the section-7 canonical form first (`DefineError::Descriptor`), then
  the formula codes of 15.6, in order.
- `eval::evaluate_value` and `World::asof`: the value only, the asof asked of the world only when
  the body needs it; `World::rebound`. `MemoryWorld::evaluated`.
- `scan` with a running `sum`, `mean`, `count`, `min`, `max`, `first` or `last` runs in one pass.
- Tests on PostgreSQL: every `formula-eval` and `excel` vector through the engine; the brief's
  tests 2 and 6-9; `tools/crosscheck.sh` (test 12); `examples/bench.rs` (test 13).

### The language (phase A)

- Lexer (15.2), PEG parser with the precedence table and rejected forms (15.3, 15.4, Appendix A),
  resolution to the canonical AST with aliases, `signature.order` and built-in parameters (15.5),
  the printer of `code.body` (15.6) and the canonical AST hash.
- The checker (15.8, 15.10, 15.13): scope, types, units with bare literals and offset units,
  time arithmetic, joins, lookups and their metrics, extents, lambdas, transaction-time reads,
  derived `inputs`, asof-dependence, and every define-time code of 15.11.
- Decimal arithmetic (15.9) on integers only: 34 digits half-even, the range rule, and SQRT-T40,
  EXP-T48, LN-ATANH80, POW-EXPLN, TRIG-T24, ATAN2-T32 and HAVERSINE at 40 digits.
- The evaluator (15.12, 15.13) over an abstract `World`: fuel, faults, slices, lookups that
  narrow their reads, scans, windows, joins, calendars, text with Unicode 15.1 case data, and
  lineage with slice entries and manifests. `MemoryWorld` evaluates the vectors.
- Every `formula`, `formula-eval`, `excel` and `decimal` vector passes; `SPEC-FINDINGS.md` records
  the readings taken where the specification is open.
