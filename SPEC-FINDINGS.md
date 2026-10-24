# Findings on spec-0.4.0 section 15 (datumesh-formula 0.1.0, phase A)

This crate was written from `SPEC.md` (commit `dedd132`), `units.md`, `schemas/`, `vectors/` and the
contract, without reading `tools/*.py`, `tests/*`, `crosscheck/*` (except its README) or
`DESIGN-0.4.0.md`. Where the prose left a reading open, the reading below was taken; where it says
"black box", the reference `tools/formula.py` was run on generated inputs (its outputs only, through
`tools/reference_runner.py`) and the crate follows what it does. None of these blocked the work: no
stop condition of the contract was met.

## A. Things the vectors depend on that SPEC.md does not state

A1. **The computed receipt of `formula-eval` and `excel` vectors.** SPEC 11 lists `receipt_hash`
among the outputs but not the receipt it hashes. Its members other than value, lineage, `tx` and
`asof` are conventions of the generator: `computed_by` `mesh.example.net#cd42547c321bfc08`,
`licence` `CC0-1.0`, `summary` "Computed by the Datumesh formula reference evaluator.", `publisher`
the cell's namespace, `valid` the valid-time arguments, and `issued` the later of the receipt's
`asof` and `2020-01-01T00:00:00.000000Z` (the key's window start). They were found by hashing
candidate receipts against the vectors (and the key id and the 2020 date read from the reference's
constants as outputs); `memory::Conventions` carries them. Proposal: state them in SPEC 11.

A2. **The function of an `excel` vector.** The vector gives `formula` and `signature` but no
descriptor for `cell.fn`; the receipt hash needs its function hash. The descriptor is
`{"code": {"body": <body>, "lang": "formula"}, "inputs": <derived>, "signature": <signature>}`.

A3. **Fuel details the table of 15.12 does not settle** (all from the vectors, then confirmed black
box): enumeration literals (`method`, `convention`, `roll`, `weekend`, the unit of `convert`, the
aggregate of `scan`/`window`, `along`) are not evaluated nodes and cost nothing; a lambda node costs
nothing (its body costs per member); the read inside `asof(c, T)` and `versions(c, f, t)` is not a
node (its arguments are); an aggregate over scalars (`max(d, d2)`) costs only its nodes, not "n";
`window` over n members with width w costs `(n - w + 1) * w` (each member that has w - 1
predecessors), not `n * w` as the table says; a slice read is charged after its members are
obtained, so a computed member's fault is reported before the slice's own charge; a business day
"stepped over" is every day examined while stepping or rolling (modified following that falls into
the next month counts the days of both directions).

A4. **Receipt `asof` (8.2 rule 2) of a read that failed.** `otherwise(sum(recip(*)), 0)` over a
computed slice whose member faults has no lineage entry, yet its `asof` is the binding start of
`recip`: a binding is relied on as soon as the read is attempted.

## B. Readings of the grammar (15.2-15.5) settled black box

B1. **Nesting (15.3, "deeper than 256").** Counted as the reference does: one level per
expression parsed (the body, every argument, branch, `let` value and body, lambda body and range
end), one more per parenthesis, one per `not`; a chain of unary minus signs adds nothing. So 127
nested parentheses are accepted and 128 are not. The prose suggests one level per parenthesis.

B2. **Node count (65,536).** Counted over the canonical AST's JSON objects, `open` and `range`
markers included.

B3. **Built-in parameter names.** A label for a required built-in parameter is accepted
(`abs(x: a)`), so the names are normative, but 15.7 writes several differently from the
reference: the slice of every lookup, `map`, `filter`, `scan` and `window` is `slice` (15.7 writes
`s`), the lambda of `map`/`filter` is `fn`, and the date of `year`, `month`, `day`, `weekday`,
`end_of_month` is `date` (15.7 writes `d`). Optional arguments print in parameter order
(`method:` before `bound:` in `interpolate`, vector `formula/interpolate-linear`), not ascending as
read labels do; 15.6 says only "optional ones labelled".

B4. **Error order** where 15.5 and 15.8 say only "source order" or "depth first": read callees
resolve before their arguments (`E_FORMULA_ALIAS`, `E_FORMULA_UNRESOLVED`, `E_FORMULA_POSITIONAL`,
too many positional arguments), then each argument in source order (its label's name, then its
value), then duplicates, unknown labels and missing arguments; built-in arguments bind one at a time
(too many positional, unknown label, duplicate, then the value), then missing required ones;
`and`/`or` and `asof` check their arity first. `E_FORMULA_UNKNOWN_FUNCTION`, `E_FORMULA_FORBIDDEN`,
`E_FORMULA_ARG_ORDER` and duplicate labels are parse errors (in text order, after the whole body is
lexed). In checking, a `let` name and lambda parameters are checked before their values and body,
and the left operand of `and`/`or` is checked before the right one is visited.

B5. **Lexing.** A number's run covers digits and points up to a `..` (`0.1.` is
`E_FORMULA_NUMBER`, `1..2` is `1 .. 2`), and an exponent `e5`; an instant is exactly the 27
characters of 3.3 (`...Z.` is an instant then a reserved `.`); a qualified name needs non-empty
labels (`a.b...org/f` is not one); `x.org/` followed by a non-name is `E_FORMULA_NAME`; period keys
are reserved for integers only (`1.5M` is `E_FORMULA_UNIT`).

B6. **Operands that cannot start an expression.** `^` is `E_FORMULA_AMBIGUOUS`, a comparison
operator is `E_FORMULA_CHAIN`, a lone `.` is `E_FORMULA_RESERVED`, `->` is
`E_FORMULA_PLACEMENT`, `*` and `[` are `E_FORMULA_PLACEMENT`; `not(` as an operand is
`E_FORMULA_PLACEMENT` but `not x` as an operand is `E_FORMULA_SYNTAX`; `true(`/`false(` and the
other keywords followed by `(` are `E_FORMULA_SYNTAX`; `let` not followed by a name reports what
an operand there would (`let = 1` is `E_FORMULA_CHAIN`).

B7. **Lambdas.** Tried for every built-in call argument (any word or parenthesised words before
`->`), never in a read argument (there `->` is `E_FORMULA_PLACEMENT`); their placement (only the
second argument of `map`/`filter`, positional or labelled `fn:`) is checked at resolution, after the
body has parsed. `if(` with a label after the parenthesis is the keyword form.

B8. **Key-set arguments (`E_FORMULA_UNSUPPORTED`).** A read argument that is a `within(...)` call.

## C. Readings of types and units (15.8, 15.10)

C1. **degC.** In `+`/`-`: degC - degC is K; degC +- K and degC +- a bare literal are degC; K + degC
and bare + degC are degC; every other combination with degC is `E_FORMULA_UNIT_OFFSET` (also
degC + EUR, which one might read as a mismatch). `mod`, `atan2`, `sqrt`, `*`, `/` and `^` of
degC are offset errors; comparisons, `if`, `otherwise` and variadic aggregates mixing degC with
another unit are `E_FORMULA_UNIT_MISMATCH`. Negation keeps degC.

C2. **Bare literals.** In `convert` a bare number is `1` (`convert(100, "deg")` is accepted,
`convert(5, "m")` is a mismatch). `add_days` counts a bare number or `d` (a number in `1` is a
mismatch); `add_months` counts a dimensionless number. `count(a, plat)` does not unify units.

C3. **Return unit text.** The vectors' `returns.unit` for an unsuffixed literal body is the text
`a bare number`; other units are atoms ascending with `^exponent` (`arg:base^-1 arg:quote`).

## D. Evaluation readings (15.9, 15.12, 15.13)

D1. **15.9.3, `n = x / LN10` and `k = x / HALF_PI` "rounded half-even to an integer".** Read as the
40-digit quotient (every step is at 40 digits) rounded half-even; `z = (m - 1) / (m + 1)` with
`m - 1` and `m + 1` each at 40 digits; in HAVERSINE `dphi = phi2 - phi1` and `dlambda` at 40 digits
and `R * 2` before the product with the angle. All 6,054 `decimal` cases agree with these readings.

D2. **Stored versions.** "The version answering as of A" is the receipt of the cell (with the
function hash the name resolves to) with the latest `tx` among those whose `asof` is at or before A.

D3. **`interpolate(..., "step")`** takes the member at or before `x` (within the bound); it needs
no member after `x`, unlike linear interpolation.

D4. **Order of `asof(c, T)` and `versions(c, from, to)`.** The times are evaluated, and
`E_FORMULA_FUTURE` reported, before the cell's arguments.

D5. **`find`** with `start` past the end gives 0, not a fault; `start < 1` is `E_FORMULA_DOMAIN`.
**`round`** with a huge `digits` is exact (no fault); with a huge negative one it is 0.

## E. Defects of the reference found on the way (black box)

E1. `if(flag, a, b, x)`: the reference raises `IndexError`; this crate reports `E_FORMULA_ARITY`.

E2. A non-integer where an integer is required (`find(..., start: 1.5)`, `substr(s, 1.5, 1)`,
`round(x, 1.5)`, `fixed(x, 1.5)`, `date(2024, 2.5, 1)`, `add_days(d, 1.5)`, `add_months(d, 1.5)`):
the reference raises an uncaught `ArithmeticFault`; this crate reports `E_FORMULA_DOMAIN` (as
`d + 1.5` already is in both).

E3. `fixed(1, 2000)`: the reference returns a 2,002-byte string; 15.7 limits every string result to
1,024 bytes (`E_FORMULA_RANGE`), which this crate reports.

E4. A left-associated chain of more than about 10,000 nodes (`a + a + ...`) makes the reference fail
with `RecursionError`; the node limit is 65,536. This crate accepts and evaluates such chains (its
passes walk operator chains iteratively).
