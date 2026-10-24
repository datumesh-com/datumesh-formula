# Changelog

All notable changes to `datumesh-formula`.

## 0.1.0 (unreleased)

The formula dialect of spec-0.4.0 (section 15), standalone: phase A of step 1.3.

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
