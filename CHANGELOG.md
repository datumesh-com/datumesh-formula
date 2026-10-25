# Changelog

All notable changes to `datumesh-formula`.

## 0.1.0 (unreleased)

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
