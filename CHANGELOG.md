# Changelog

All notable changes to `datumesh-formula`.

## 0.6.0

For the classes round (`CONTRACT-classes-round.md`, the branch `classes-round`), on datumesh-core 0.3.13
(unreleased; `[patch]` to the local core until it is published) and the spec at commit `4114360`, which
is now the submodule `spec/` (a gitlink, as datumesh-core has it); `spec-vectors` is a symlink to
`spec/vectors`. Every change is additive except the first, which is **BREAKING**.

- **Withdrawn stored cells have no value** (CONTRACT-round-0.7 R1; spec-0.7.0, SPEC 8.1, 15.11-15.13;
  datumesh-core 0.3.13's `Withdrawal`). A stored receipt with `withdrawn` in place of `type` and `value` is
  a version of its cell with no value. `MemoryWorld::add_receipt` parses it with `Withdrawal::from_value`.
  As of the evaluation asof, the latest version (by `tx`, then `asof`) decides: a withdrawal makes
  `World::read` return `Ok(None)` (a read alone is `E_FORMULA_NOT_COVERED`, `at` is `E_FORMULA_ABSENT`), and
  `World::members` leaves the cell out. A later valued version ends the withdrawal. `World::versions` lists
  the values only: withdrawals are not listed. Cost: a stored receipt with `withdrawn` was refused
  (`malformed input`) before.
- **A withdrawal that left a slice member out counts for time** (SPEC 8.2, D123; `spec-0.7.0`). A stored
  cell of a slice read whose latest version as of the asof is a withdrawal, and that matches the slice,
  contributes its `tx` to the computed receipt's `tx` and its `asof` to the asof candidates, though it is
  no input. A withdrawal outside the slice's range is no candidate. New `World::withdrawn_in_slice` (default:
  none); `MemoryWorld` implements it. Free (no fuel). Engine worlds keep the default until the engine
  reads it. Cost: the `formula-eval/withdrawn-slice-*` receipts change.
- **K8 empty key**: a lookup that returns `""` is `E_KEY_NORMALISATION` (D116 as amended by D123); a label
  with no value stays `E_NAME_UNBOUND`.
- **BREAKING: an inherited method reads the cells of the class that defines it** (K5; spec 16.2, 16.3,
  16.6). For an object of class S, a method S inherits without overriding denotes `D/m`, where D is the
  nearest ancestor whose descriptor gave `m` the same function hash. This holds in a body, a request
  (`resolved` says `method` to `D/m`), a dispatch and `this`. An overridden method is still `S/m`. D is
  found through the parent hashes; an override needs only the parent's flattened descriptor, by hash (env
  member `class_descriptors`), and an inherited method needs the defining class's name. Store-backed names
  come from store 0.5.3's `class_names_bound_to`, the first in ascending order. Names come from (`CallEnv::class_name_of`,
  `World::class_name`, `Callees::class_name`); a parent with no name is `E_FORMULA_UNRESOLVED` in a body
  and `E_NAME_UNBOUND` in a request. Binding S binds only S's own methods (datumesh-store's `bind_class`
  is the store's change, not this crate's). Cost: a call or body that read `S/m` for an inherited `m` now
  reads `D/m`; the vectors `formula/class-inherited` and `formula/class-inherited-plain` change to
  `rates.example.org/fx_pair/cross` (their AST, hash, body and inputs). The plain read `S/m` of an
  inherited method is now `E_FORMULA_UNRESOLVED` (body) or `E_NAME_UNBOUND` (request). Libraries
  compiled against 0.5.x resolve the old way until they are recompiled.
- **`this` and a dispatch in a draft** (K7): `definition::ClassDraft` and `define_method_in_draft`
  define a method whose `this` is a class not yet bound; `this.m` is checked against the draft's keys
  and signatures and reads `N/m` (or `D/m`, for an inherited method of the draft's parent). A method
  reads the leading keys its signature has, so an override lacking a trailing key is accepted. Cost: a
  method that does not take its class's keys first is refused as a method read (`E_FORMULA_METHOD`).
- **Variable-key receivers** (K7, 16.3): `N(b, USD).m(d)` in a body is the plain read
  `N/m(b, USD, d)`, with the same canonical AST. The literal rule stays in class-typed positions.
  Cost: `E_FORMULA_PLACEMENT` for a non-literal key is kept only there.
- **Returned classes are loaded at define** (K2): a dispatch on a callee's returned key defines
  without the class being named in the body. Cost: one more class loaded per such callee.
- **Request chains** (K10, 5.8, 16.6): `N(k).m(a).n(b)` calls each method on the key the one before
  returns, read as of the call's asof (or the mesh's now) through the new `CallEnv::chain` hook;
  `resolved` gains one `chain` entry per link. A link after a non-key method is `E_PARSE_URI`; a
  link with no value is `E_NAME_UNBOUND`; an undeclared key is `E_FORMULA_DISPATCH_KEY`. Cost: a
  request with more than one method was `E_PARSE_URI`; `MapCallEnv` reads links from a new `cells`
  member.
- **Folded labels** (K8, 16.4): `fold_label` (re-exported from datumesh-core) folds a label before its
  lookup when the lookup's `label` argument is `label_fold`; the resolution records the label as
  written; a withdrawn label (`""`) is `E_NAME_UNBOUND`. Cost: one folding per labelled call.
- **Digit and hyphen runs are strings in string positions** (K11, 5.8): an unquoted run such as
  `0000320193` or `0000320193-24-000123` is a string where a string is expected; elsewhere the codes are
  today's (`E_CANON_NUMBER` or `E_PARSE_URI`). Cost: a call that was refused for such a run in a
  string position is now accepted; runs with letters are unchanged.
- **Literal defaults** (K13, 7, 16.7): an argument may declare `"default": <lexical value>`, bound in a
  request (`resolved` records a `default`) and in a body (the canonical AST holds the literal). Cost:
  a hashed descriptor member, for new functions only; a default of the wrong type is `E_SCHEMA` at
  define.
- Inherited methods are loaded with their ancestors at define (K5), and a method of the class being
  defined is no longer bound by its own name when it is a draft (K7). Cost: a define loads the
  ancestors of each class it reads.
- A dispatch to an inherited method passes only the defining class's keys, the leading keys, as the
  request path does (K5). A subclass object's extra keys are not arguments of the parent's method.
  Cost: a dispatch that passed every key of the object now passes the defining class's keys only.
- `FormulaRuntime::new` over a store names the defining class of a dispatch: `StoreCallees` forwards
  `Callees::class_name` to the store's `class_names_bound_to` (K5), so `E_FORMULA_UNRESOLVED` is no
  longer returned for a store with a bound parent.

## 0.5.2 (unreleased)

For spec-0.6.2 (the branch `spec-0.6.2` of [datumesh-spec](https://github.com/datumesh-com/datumesh-spec),
commit `39d6849`; `spec-vectors/` is its vectors, the 159 new ones included). Additive: every vector
of the previous revision keeps its result.

- **`last_by(slice, by)` and `first_by(slice, by)`** (15.7, 15.11, 15.13). Per group of the keys of
  `slice` that `by` does not have, the member whose `by` value is the greatest (`last_by`) or least
  (`first_by`). The open arguments of `by` are the ordering dimension and join `slice` as an inner
  join: a member with no `by` member is no candidate, and a tie goes to the later (`last_by`) or
  earlier (`first_by`) member in key order. A group with no candidate is left out of a slice result;
  a scalar result with no candidate is `E_FORMULA_ABSENT`. Both slices are read, so each is an
  `inputs` entry (the same slice twice is one entry), and fuel is that of the two reads. The printer
  writes both arguments positionally. A `by` open argument that `slice` lacks, or has with another
  key type, is `E_FORMULA_JOIN`; a `by` of booleans or blobs is `E_FORMULA_TYPE`; a unit that varies
  by member is `E_FORMULA_UNIT_UNKNOWN`, as `max` gives.
- **Unit suffixes `shares` and `pure`** (`units.md` 2.3): `5 shares` and `5 pure` are numbers in
  those units, and the two do not mix (`E_FORMULA_UNIT_MISMATCH`).

## 0.5.1

For spec-0.6.1 (the branch `spec-0.6.1` of [datumesh-spec](https://github.com/datumesh-com/datumesh-spec),
unpushed at the time of writing; `spec-vectors/` is its vectors, commit `f78773a`). The formula
rules of its section 15 and the names of section 2.3:

- **DNS names in later segments** (15.2 L3, 15.3, 15.4, D109). A segment after the host may be a DNS
  name (2.1), so `datumesh.com/relay/ecb.europa.eu/rate(d: d)` is a read. In a body, a run after a
  `/` that contains `.` or `-` is a DNS segment only when a `/` follows it: `ecb.europa.eu/rate(d: d)-1`
  is a read minus one, `…/rate(d: d)/…/yield(d: d)` divides two reads, and `.status()` after a call
  is still a method call. The last segment is a name, as before, so a `.` or `-` after it ends the
  name. A run followed by `/` that is neither a name nor a DNS name is `E_FORMULA_NAME`.
- `names::is_function_name` takes a DNS name in a middle segment (at most 253 characters, the
  segment rules of 2.1); the last segment is a name of at most 64 characters, and the whole name
  keeps its 512.
- Every vector of the previous revision keeps its result. The new vectors are `formula/dns-segment-*`
  (8), `formula-eval/dns-segment-*` (3) and `invalid/formula-dns-segment-*` (9).
- The host alone decides a name's authority and rootedness (2.5): nothing here changes that. A
  DNS name in a later segment is a label only.

Libraries before 0.5.1 refuse a dotted later segment with `E_FORMULA_NAME`. This release requires
`datumesh-core` 0.3.10 (the DNS segments in its name checks).

## 0.5.0 (2026-11-19)

- On datumesh-engine 0.5 and datumesh-store 0.5 (Build 2): the engine splits a deep recursion
  itself, so `invalid/formula-eval-depth`, which the spec lists as evaluator-only, is no longer
  run through the engine in the tests. The public API is unchanged by the move.

Additions (Build 2 Part 3, E5: branch coverage, for the mesh's `E_TESTS_MISSING`):

- `coverage::branches(&Ast)`: the branch points of a body, ascending by id (the arena index of the
  node): each `if` (two arms: then, else) and each `otherwise` (two arms: the first argument gave a
  value, or it faulted with a catchable code and the fallback was taken). Unreachable nodes are not
  points.
- `coverage::EvalObserver` (`branch_taken(id, arm)`), `coverage::Coverage` (a set of taken
  `(id, arm)`: `new`, `taken`, `contains`, `union`, `covered`, `total`, `missing`), and
  `coverage::BranchPoint` / `BranchKind`. An arm is reported when taken, before it is evaluated, so
  an arm that then faults is still recorded; the arms of callees are not reported.
- `eval::evaluate_observed` and `MemoryWorld::compute_observed`: the evaluation with an observer.
  `eval::evaluate` is `evaluate_observed` with a no-op observer; its results are unchanged.
- `coverage::edge_facts(&Compiled, &Signature) -> EdgeFacts`: the derived edge-case facts of a body:
  the `number` arguments in a divisor (the right operand of `/`, the second argument of `mod`), the
  cell reads and the slice reads (by function name), the declared `number`, `date` and `instant`
  arguments, and the `date`/`instant` arguments with a declared extent. Static: a divisor in an arm
  no input reaches counts.

## 0.4.0

- 0.4.0: spec-0.6.0 request grammar — digit-led words such as 2Chr in string positions; previously E_PARSE_URI.
  A digit-led word (`2Chr`, `1Cor`, `1_kings`) in a call reads as the string it spells, labelled or positional,
  in a class receiver's key or a method argument; anywhere else `E_CANON_TYPE`. `2D` stays a period token,
  `2024-03-15x` is `E_PARSE_URI` (was `E_CANON_DATE`), and bodies are unchanged.

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
- A bare `this` or `super` (not followed by `.`) is `E_FORMULA_RESERVED` at its token, whatever follows,
  inside and outside a method body; `examples/formula_runner` echoes the input's `id` first in each line.

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
