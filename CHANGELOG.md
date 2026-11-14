# Changelog

All notable changes to `datumesh-formula`.

## 0.3.0 (unreleased)

For spec-0.5.0 (the commit `6d5633e` of [datumesh-spec](https://github.com/datumesh-com/datumesh-spec),
on the branch `spec-0.5.0`, untagged): classes (SPEC 16) and the requests of 5.8 by the types of their
positions. On `datumesh-core` 0.3.8 (was 0.3.6) and
`datumesh-store` 0.4.1 (was 0.4.0); `datumesh-engine` 0.4.2 (was 0.4.0; the minimum, for
`Inputs::read_pinned`). Breaking: a new variant of
`Node`, new members of public structs, and the minimum versions of the dependencies
(`API-CHANGES-0.3.0.md`).

- Method calls (16.3): `N(keys).m(args)` is the plain read of `N/m`, the same canonical AST, cell and
  cell id; objects bind their keys by the class's order or by label; extension methods by qualified
  name; `this` in a method's body; `super` as a read pinned to the parent's method hash
  (`Node::Read::fn_hash`, printed `P/m@<hash>(...)`).
- Dispatch (16.6): a method call on a key declared with `classes` (or returned as one) is a
  `Node::Dispatch` over the declared set; the lineage holds the receipt of the function actually
  called; a key outside the set is the fault `E_FORMULA_DISPATCH_KEY`.
- Key normalisation (7, 16.4): a function's `key_normalisation` is checked for literals at define
  (`E_FORMULA_KEY`) and for every argument value at evaluation (the fault `E_FORMULA_KEY`).
- Defaults (16.7): `"default": "asof"` binds `asof()` (an instant) or `date_of(asof())` (a date)
  in the AST.
- Requests (5.8): each argument is read by the declared type of its position; a class-typed
  position takes a key encoding; bare names resolve through the alias map (a short call, recorded
  in `resolved`); `N[label]` resolves through the class's `lookup` as of the call's asof; the
  result reports `resolved`; `E_KEY_NORMALISATION` and `E_KEY_ENCODING` are `bad_request` codes.
- Define of classes (16.2): `define_class` over `datumesh-core`'s `define_class` and
  `publish_class` (store 0.4.1's `put_class` and `bind_class`); `define_method`; the environment
  reads `classes` and `method_of`.
- The formula runtime (feature `engine`) answers dispatch and classes from the store, and `super`
  works through the engine with datumesh-engine 0.4.2: a pinned read is the engine's
  `Inputs::read_pinned` of the function stored under the parent's hash (found through the callees,
  whatever its name is bound to now), which relies on no binding (16.3, 8.2 rule 2).
- Clarifications of spec-0.5.0 at `c8896ad` (SPEC-FINDINGS H and I; the tests of
  `tests/classes_clarifications.rs`, the vectors re-pinned to 1474, 656 run here): a dispatch whose
  `<class>/m` is bound to another hash is `E_FORMULA_DISPATCH` at evaluation, and an unbound method
  or class `E_FORMULA_UNRESOLVED`, both mesh failures (`is_mesh_failure`); a dispatch relies on the
  method's binding only (no class start is an `asof` candidate); a quoted key encoding in a
  class-typed position is decoded and checked against its class (`E_FORMULA_KEY` and
  `E_FORMULA_TYPE` in a body, `E_KEY_NORMALISATION` and `E_CANON_TYPE` in a call); a call with no
  asof and no now that needs one, for a default or a label, is `E_ARG_MISSING`; an extension method
  not taking the class keys is `E_ARG_UNKNOWN`; `super` with no class name bound to the parent's
  hash is `E_CLASS_PARENT` (new `Code::ClassParent`), and the printed name is the first ascending
  name bound to it; a printed dispatch needs its signature to parse back (text only).

## 0.2.0 (unreleased)

On `datumesh-engine` 0.4.0 and `datumesh-store` 0.4.0 (the optional feature `engine`; were
0.3.2 and 0.3.2), so that the formula runtime runs on the store's SQLite backend (feature
`sqlite`) as well as on PostgreSQL. Nothing else changed: no code, no behaviour
(`API-CHANGES-0.2.0.md`).

- Breaking only because, with feature `engine`, the public API names the engine's and the
  store's types (`runtime::FormulaRuntime::new` takes an `Arc<dyn Store + Send + Sync>`, the
  runtime implements the engine's `Runtime`, `CallEnv` is implemented for `PgStore` and for
  `dyn Store + Send + Sync`), which are now engine 0.4's and store 0.4's: users move to the
  three together. Without feature `engine` the API is unchanged.
- `datumesh-store` with `default-features = false`, and a new feature `sqlite` (off: `engine`
  with `datumesh-engine/sqlite` and `datumesh-store/sqlite`) for the store's SQLite backend.
  `PgStore` is always built, so PostgreSQL users need no new feature.

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
