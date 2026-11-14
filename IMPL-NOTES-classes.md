# datumesh-formula: classes for spec-0.5.0, notes for the implementer

For `/root/datumesh-libs/CONTRACT-formula-0.5.0-classes.md`. The tests commits (three) are written;
yours are the three implementation commits, one per tests commit, in the contract's order. This file
is committed with the first tests commit and covers all three.

**Independence rule.** Do not read `spec/tools/*.py` (`formula.py`, `canon.py`, `class_world.py`,
`gen_*.py`). The tests and `IMPL-NOTES` were written the same way: from `SPEC.md` at `6d5633e`, the
vectors, and the reference tools run as black boxes (see "How the expectations were made"). Implement
from `SPEC.md`: 5.5, 5.8, 7, 9.7, 14 (the spec-0.5.0 entry), 15.2, 15.3, 15.5, 15.11, 15.12, 15.15, 16 (all of it),
Appendix A.

## 0. How to build

- Cargo runs in docker. The dependencies of this work are not published: use a **local, uncommitted**
  `.cargo/config.toml` (already in the working tree, excluded by `.git/info/exclude`):
  ```toml
  [patch.crates-io]
  datumesh-core = { path = "/core/crates/datumesh-core" }     # core 0.3.8, commit 0d53fa8
  datumesh-store = { path = "/store/crates/datumesh-store" }  # store 0.4.1, see below
  ```
  The paths are the ones inside the container: mount `/root/datumesh-libs/datumesh-core` at `/core` and a
  checkout of the store at `/store` (`docker run ... -v /root/datumesh-libs/datumesh-core:/core:ro -v <store>:/store:ro`;
  `tools/cargo.sh` mounts only the repo, so use a copy of it with the extra mounts). The tests commits were built
  against a `git archive` of `datumesh-store` at `9cecd60` (the store's own tests commit: `put_class`, `class`,
  `bind_class`, `resolve_class`, `resolve_class_binding` are `todo!()` there). When the store's implementation is
  ready, patch to it instead and un-ignore the three store tests (section 4).
- The committed `Cargo.toml` requires `datumesh-core = "0.3.8"` (both entries) and `datumesh-store = "0.4.1"`
  (the minima the new API needs); the engine stays `0.4.0`. **`Cargo.lock` is not committed with these:** a build
  rewrites it for the patches; `git checkout Cargo.lock` before committing (locks are fixed at release time).
- A full `cargo test` needs PostgreSQL (`DATABASE_URL`, the `datumesh-pg` container as before).
  Clippy `-D warnings`, `fmt` and `RUSTDOCFLAGS='-D warnings' cargo doc` pass on the tests commits.
- Remove `target-*` when done (a debug build of the test suite is about 3 GB).

## 1. The commits and what fails

| Tests commit | Files | Contract items | Failing now |
|---|---|---|---|
| 1. equivalence, normalisation, defaults | `tests/classes_methods.rs`, `tests/engine_classes.rs` (the first test) | 1 (no `super`, no dispatch), 2, 4, 6, 7 (define, engine for methods) | `classes_methods`: 25 tests, 23 fail (two are guards); `engine_classes`: 1 fails |
| 2. requests | `tests/classes_requests.rs`, `tests/engine_classes.rs` (+ one ignored test) | 5, 6, 1 for requests | `classes_requests`: 15 tests, 13 fail (two are guards); 1 ignored store test fails when run |
| 3. dispatch, interfaces, `super` | `tests/classes_dispatch.rs`, `tests/vectors-local/`, `tools/gen_class_vectors.py`, `tests/engine_classes.rs` (+ two ignored tests) | 3, 1 (`super`), 7 (class define), interfaces | `classes_dispatch`: 17 tests, 17 fail; 2 ignored store tests fail when run |

The first commit also holds the infrastructure all three share: `spec-vectors/` (the 1466 vectors of `6d5633e` and
`units.md`; the README says so), the API stubs of section 2, `tests/common/{check,classes,engine}.rs`, and the
changes to the old runners. **Old tests keep passing** (checked at each commit): the runners
`formula_vectors.rs`, `eval_vectors.rs`, `call_vectors.rs` and `engine_vectors.rs` now run the vectors of spec-0.4.2
only (`common::old_vectors_of_kind`, which leaves out the 63 class vectors by file name) with their old counts
(172+123, 125+39, 10+17; `excel` 79); the class vectors are the new tests'. Together: formula 330 (194 valid, 136
invalid), formula-eval 174 (131, 43), call 45 (20, 25), excel 79, decimal 20 = **648 run, 818 skipped** of 1466 (the
skipped are the verifier's kinds: `datumesh-core` runs them). `classes_methods::the_vectors_are_those_of_spec_0_5_0`
asserts the counts.

Your pass for each commit: its tests pass, the earlier ones still do, `cargo clippy --all-targets -- -D warnings`,
`cargo fmt --check` and the docs stay clean. The tests marked "guard" pass today and must keep passing.

## 2. The API (stubs are in the tree; they compile; the behaviour is `todo!()` or absent)

Semver: `Node` gains a variant and a field on `Node::Read`, `ArgDecl`, `Signature` and `Env` gain fields, and `Ty` will
gain a variant: **breaking, so 0.3.0** (run `cargo public-api` against 0.2.0 and write `API-CHANGES-0.3.0.md`; the
contract says so if the diff is not additions only). Everything else is additive. `Code` is `#[non_exhaustive]`.

Names avoid clashing with `datumesh_store::Store` (`class`, `descriptor` are its methods): the new `Callees` and
`CallEnv` members are `bound_class`, `class_descriptor`, `function_descriptor`.

```rust
// error.rs: four codes, in the order of 15.11 (after NotCanonical: Method, Dispatch, Key; after Future: DispatchKey).
Code::{Method, Dispatch, Key, DispatchKey}      // present. STUB: is_fault() is not yet true for Key and DispatchKey.

// classes.rs (new, `pub use classes::Classes`): flattened class descriptors by class name. Complete.
pub struct Classes { .. }                       // new, insert(&str, ClassDescriptor), get, by_hash(&str) -> Option<(&str, &ClassDescriptor)>,
                                                // names(), is_empty(), from_value(&Value)

// env.rs
ArgDecl { .., extras: datumesh_core::ArgExtras }       // classes, default_asof, key_normalisation. STUB: Signature::from_value leaves the default.
Signature { .., returns_classes: Option<Vec<String>> } // STUB: None
Env { .., classes: Classes, method_of: Option<String> } // STUB: Env::from_value does not read `classes` or `method_of`

// ast.rs
Node::Read { fn_name, fn_hash: Option<String>, args }  // fn_hash: a read pinned by `super`; STUB: ignored by the JSON and the printer
Node::Dispatch { on: NodeId, method: String, classes: Vec<String>, args: Vec<(String, NodeId)> }
                                                       // STUB: `children` is real; to_jcs and print are todo!(); resolve/check/eval never reach it yet

// eval.rs (World): provided methods, free
fn class(&mut self, name: &str, at: Option<&str>) -> Result<Option<ClassBinding>, Self::Error>   // default None
pub struct ClassBinding { class: Arc<ClassDescriptor>, class_hash: String, start: String }
fn function_by_hash(&mut self, fn_hash: &str) -> Result<Option<Arc<Function>>, Self::Error>       // default None
fn read_pinned(&mut self, fn_hash: &str, cell: &CellKey) -> Result<Option<Version>, Stop<Self::Error>>  // default Err(Unsupported)

// memory.rs
MemoryWorld::bind_class(&mut self, name, &Value descriptor, start) -> Result<(), EnvError>   // real
// STUB: MemoryWorld::from_value does not read world.classes ({name: {"descriptor", "bound"}}).
// MemoryWorld implements World::class (real); function_by_hash / read_pinned: yours.

// call.rs
CallEnv::{bound_class(&self, name, asof) -> Result<Option<ClassDescriptor>, String>,   // default None
          aliases_tx(&self) -> Result<Option<String>, String>,                          // default None
          now(&self) -> Result<Option<String>, String>,                                 // default None
          lookup(&self, class, label, asof: &str) -> Result<Option<String>, String>}    // THE LOOKUP HOOK; default None
MapCallEnv { .., classes, aliases_tx, now, lookups }        // read by from_value; implements the four members (real)
Call { .., resolved: Option<datumesh_core::Resolved> }       // STUB: always None; parse_call ignores classes

// definition.rs
Callees::{bound_class(name, at) -> Result<Option<ClassCallee>, String>, class_descriptor(class_hash), function_descriptor(fn_hash)}  // defaults None
pub struct ClassCallee { class_hash, start, descriptor }
impl Callees for Env                                         // real for the three (classes by name and hash, functions by hash)
pub fn define_method(class, name, source, signature, aliases, callees, at) -> Result<Definition, DefineError>   // STUB todo!()
pub struct ClassDefinition { name, descriptor: ClassDescriptor, bytes, class_hash }
pub fn define_class(name, source: &[u8], definer, callees, at) -> Result<ClassDefinition, DefineError>          // STUB todo!()
#[cfg(feature = "engine")] pub fn publish_class(store, &ClassDefinition, tx) -> Result<String, datumesh_store::Error>   // STUB todo!()
// To do for the stores (feature engine): impl Callees / CallEnv for PgStore and `dyn Store` with
// bound_class (resolve_class_binding + class), class_descriptor (class), function_descriptor (descriptor); define_function
// reads classes from the callees.
```

`Env.method_of` is the class **name** whose method the function is. `Classes::by_hash` is how `super` finds the parent (the
flattened descriptor names its parent by hash) and how interfaces are found.

## 3. Where each test is, and what it pins (by contract item)

Spec sections in brackets. "V" = a vector runner over spec vectors; "R" = expectation checked against the reference.

1. **Method calls** [16.3, 15.5, 15.15, 15.11]
   - `class_formula_vectors` (V, 22 valid + 11 invalid, methods group) and `a_method_call_is_the_plain_read` (the 11
     `X`/`X-plain` pairs: same AST, hash, body, inputs).
   - `keys_bind_by_position_or_by_label`, `key_and_argument_errors`, `method_errors`,
     `the_method_must_be_bound_to_the_classs_hash`, `extension_methods` (incl. `this.calc.example.net/f(1)` by longest
     match), `inherited_methods_take_the_parents_keys` (R).
   - `key_literals_are_strings`, `literal_keys_are_checked_at_define`, `class_typed_positions_take_key_encodings` (R).
   - `this_in_a_method` (R); `define_method_reproduces_the_vectors_function_hashes`: `define_method` of `this.rate(date)` and
     of the `forward` body gives the function hashes the class world holds for `spot` and `forward`.
   - `define_function_accepts_method_calls`, `the_environment_reads_classes`, `signatures_read_the_new_members`.
   - `super` (commit 3): `super_is_pinned_to_the_parents_hash`, `a_pinned_read_is_for_methods_and_stored_bodies`,
     `define_method_reproduces_the_overriding_methods_hash` (hash of `fx_ndf`'s `rate`), `super_relies_on_no_binding`,
     `a_pinned_read_node` (the node's JSON and print), vector `class-super` and `invalid/formula-class-super-without-parent`.
   - Requests (commit 2): `a_method_call_is_the_cell_of_its_plain_read` (same cell reference, URI and **cell id**),
     `objects_in_calls`.
2. **Key normalisation** [7, 16.4, 15.11, 15.12]: `the_new_codes` (order in `Code::ALL`, `is_fault`, not catchable, not a mesh
   failure, core's `Fault::from_code`), `a_computed_key_is_checked_at_evaluation` (vector + `upper` variant),
   `a_cells_own_argument_is_checked_where_it_is_read`, `an_empty_computed_key_is_a_fault`, the engine test of the faulting
   vector (`NotCovered { fault: Some(Key) }`), `keys_are_checked_against_their_normalisation` (calls: `E_KEY_NORMALISATION`).
3. **Dispatch** [16.6] (commit 3): `class_formula_eval_vectors` (V: bond, swap, `super`, the three key-fault vectors),
   `local_class_vectors` (V, `tests/vectors-local/`: chains over a returned key, every way a returned key fails to be an object
   key, a dispatched read without value), `lineage_names_the_function_actually_called`, `the_dispatch_node` (JSON, print, chain),
   `a_method_call_on_a_key_dispatches`, `dispatch_errors_at_define`, `a_chain_on_returned_keys`, `define_function_reproduces_price_of`,
   `a_dispatch_key_not_canonical_for_its_class` (R, see H2 of SPEC-FINDINGS.md).
4. **Defaults** [16.7, 15.12] (commit 1): `defaults_compile_to_asof`, `an_instant_default_is_asof`, `a_default_follows_the_evaluation_asof`
   (receipt `asof` is the request's, one receipt per requested asof; the date follows the asof's date; the next day has no value).
   Requests: `defaults_take_the_calls_asof_or_now`.
5. **Requests** [5.8, 16.7, 16.5] (commit 2): `class_call_vectors` (V, 10 + 8, also `resolved`), `values_are_read_by_the_type_of_their_position`,
   `what_is_not_one_token_is_quoted`, `the_order_of_checks`, `class_typed_positions_take_key_encodings`,
   `bare_names_resolve_through_the_alias_map`, `resolved_names_the_alias_snapshot`, `a_label_resolves_through_the_lookup_hook` (the hook is
   called once with the class, the label and the asof of the call, or the mesh's now), `an_unknown_label_is_unbound` (the hook is not asked
   for a class without `lookup`), `a_label_is_a_receiver`, `the_map_environment_reads_the_new_members`.
6. **Vectors**: the runners above; `the_vectors_are_those_of_spec_0_5_0` counts. Run/skipped as in section 1.
7. **Engine / define** (`tests/engine_classes.rs`, feature `engine`): `the_methods_group_through_the_engine` (commit 1; no classes in the store: the
   methods' functions are plain functions); and **ignored "for later"**, because the store's `put_class`/`bind_class` are stubs:
   `a_store_answers_class_calls` (commit 2), `the_dispatch_group_through_the_engine` and `define_publish_and_dispatch_over_a_store` (commit 3).
   Run them with `-- --ignored` once the store is ready and remove the attributes. `define_class_over_the_vectors` (commit 3, no store):
   every `class` vector (45) through `define_class` over an `Env` as callee source: the flattened bytes and class hash, or core's code.
   **Interfaces** have nothing formula-specific in the spec beyond core's define (16.2 step 8): `a_class_with_interfaces_is_an_ordinary_class`
   (a class that `implements` an interface is read like any class) and the `class` vectors that have `implements`.

## 4. Pitfalls (each was found with the reference; the spec sentence is the authority, the reference the check)

- **Receiver keys first in a call.** `fx_pair(eur, USD).rate()` is `E_KEY_NORMALISATION`, not `E_ARG_MISSING`: bind and check the receiver's keys (type,
  lexical form, normalisation) before the method's own arguments. A missing key (`fx_pair(EUR).rate(..)`) is `E_ARG_MISSING`, more
  keys than the order `E_ARG_UNKNOWN`. In a body the same cases are `E_FORMULA_ARG_MISSING`, `E_FORMULA_ARITY`.
- **Core does the key work.** `parse_object_key`/`format_object_key` for encodings (anything that is not exactly the encoding of what it decodes to is
  `E_KEY_ENCODING`), `KeyNormalisation::check` for the rules, `define_class` for 16.2. Do not reimplement them.
- **Positions.** In a call a class-typed position takes the object (`iso.example.org/currency(EUR)`) or a quoted encoding; a bare token there is
  `E_CANON_TYPE`, an object of another class `E_CANON_TYPE`, a malformed quoted encoding `E_KEY_ENCODING`. Unquoted tokens are strings in `string`
  positions whatever their kind (`-5`, `true`, a date, `1M`) and `E_PARSE_URI` when they hold anything else. `01` in a string position is `E_CANON_NUMBER`.
- **A body is different from a request.** In a body `eur` (lowercase bare) is a variable; key literals are uppercase words and periods in a `string` position
  only; `N[label]` is `E_FORMULA_SYNTAX`; a literal key not canonical is `E_FORMULA_KEY`, a string literal that is a malformed encoding in a class-typed
  position `E_FORMULA_KEY`, of another class `E_FORMULA_TYPE`.
- **`this` binds through the receiver's class name.** In `fx_ndf`'s method, `this.rate(date)` reads `calc.example.net/fx_ndf/rate` (the subclass's name, inherited
  or overridden), with the method's own key arguments as keys; `this` alone is `E_FORMULA_RESERVED`, `this.named(..)` `E_FORMULA_METHOD`.
- **`super`.** Parent found by hash in `Classes`; the pinned hash is the parent's descriptor entry for `m`; the read node carries `fn_hash` and prints
  `P/m@<hash>(...)`. `check_descriptor` (the evaluator's path, no `method_of`) must accept the pin; `define` of a non-method body keeps
  `E_FORMULA_PLACEMENT`. Evaluating it relies on no binding: no binding-start `asof` candidate, works when `P/m` is unbound at the asof
  (`super_relies_on_no_binding`; the receipt hash `a28b2591...` and fuel 7 are the reference's). That needs the function by hash: `World::function_by_hash` and
  `read_pinned` (stored receipts match on the pinned function's hash). **The engine 0.4 cannot read by hash**: through the runtime a pinned read is
  `E_FORMULA_UNSUPPORTED` (a mesh failure) until it can (H8 of SPEC-FINDINGS.md); the contract's item 7 does not list `super`. Report it.
- **Dispatch.** Define: the class set comes from the argument's `classes` (or a returned key's), every class known, has the method, `N/m` bound to
  *its* hash (`E_FORMULA_METHOD`), one signature once the keys are taken away (`E_FORMULA_DISPATCH`); the AST keeps the classes in the order the
  declaration gives; `inputs` list `<class>/m` for every class; the printed form always labels the arguments (`v.next().text(edition: "uxlc")`).
  `.` after a number, a plain string variable or a non-key expression stays `E_FORMULA_RESERVED`. Evaluate: decode the key (core), a malformed
  encoding / key of a class outside the set / keys not the class's / a string-typed number key etc. is the **fault** `E_FORMULA_DISPATCH_KEY`; a
  well-formed key whose value breaks the class's normalisation falls through to the function argument's check, the fault `E_FORMULA_KEY` (H2); a
  dispatched read with no value is plain `E_FORMULA_NOT_COVERED`. **Fuel** is normative: a dispatch costs its node (1), its `on` and arguments' nodes, and
  1 for the read of the called function (`class-dispatch-bond` 4; `class-seq-chain` 5; `class-seq-chain-twice` 7); decoding is free. Lineage is the
  receipts read, so the hash called follows from the read; do not add anything to it.
- **Fault classification.** `Code::Key` and `Code::DispatchKey` are faults (`is_fault`), not catchable by `otherwise`, not mesh failures. The memo of
  `MemoryWorld::version` already keeps any code that is not a mesh failure, so they memoize; the engine recomputes faults as it always did (H9).
- **Defaults.** Only on valid-time `date`/`instant` arguments. In a body, an unbound defaulted argument binds `date_of(asof())` (date) or `asof()` (instant) in
  the AST, so the function is asof-dependent (`Compiled::asof_dependent`, and `FormulaRuntime::asof_dependent`, which today reads the AST for `asof()`: it
  still works, the default is in the AST). In a call the default is the call's `asof` (date part for a date), or `CallEnv::now`; the cell URI carries the value.
- **`resolved`.** Absent when nothing was resolved; otherwise `call` (the full name, every argument labelled ascending, `name: <literal>`, `?asof=` when the call
  had one), the resolutions (kinds `alias`, `lookup`, `method`, `default`; for an alias `from` is the name as written, `@name` for an `@alias`; for a method `from` is
  `N(...).m` and `to` the function name; a lookup is `{asof, from: label, to: key encoding}` and comes before the method; a default is `{from: argument,
  to: value}`), and `aliases_tx` only when an alias was resolved and the environment has a snapshot. Compare with the vectors by JCS (`common::check::resolved_value`).
- **The lookup hook.** Called once per `[label]`, with the class name, the label and the effective asof (call's `?asof=`, else `now()`); `None` or an unknown label is
  `E_NAME_UNBOUND`; a class without a `lookup` member is `E_NAME_UNBOUND` and the hook is not called. `N[label]` anywhere but before a method is `E_PARSE_URI`.
- **Callee collection for `define_function`.** `read_names` reads the parse tree: a method call `N(...).m(...)` needs `N`'s class and the function `N/m`; a dispatch
  needs, for each class of the set, `<class>/m`; a chain needs the returned class of the inner read. A workable approach: load every class a body or a loaded
  signature names, with all its methods' functions (`bound_class` + `callee` of each `N/m`). `Callees for Env` already answers all three.
- **Parsing a printed body without an environment.** `canonical_ast(source, None)` (15.6: "parse(print(ast)) = ast, without an environment") is used by
  `FormulaRuntime::asof_dependent` and the round-trip tests. A printed dispatch (`v.next().text(edition: "uxlc")`) has lost its class set, which only the
  signature knows, so it cannot give the same AST without an environment (H10 of SPEC-FINDINGS.md). The tests do not need it to (the round-trip
  test generates no dispatch); but `canonical_ast(None)` must not fail on method syntax, or `asof_dependent` breaks for every dispatching function: parse it
  (a `dispatch` with an empty `classes`, say) or derive asof-dependence another way.
- **`FormulaRuntime`** compiles stored bodies with `check_descriptor`; it must put the classes (`bound_class` at the asof) and the pinned parents into the `Env` it
  builds, and implement `World::class` over the store (`resolve_class_binding(name, asof)` + `class(hash)`; the binding `start` is its `tx`).
- **`sqlite` feature, `examples/`, `fuzz/`, `bench/`** use `Env`, `Signature`, `Node`: they compile today; check them after your changes
  (`cargo build --features sqlite --all-targets` and the fuzz targets in the nightly image).

## 5. How the expectations were made

- Vectors: `spec-vectors/` at `6d5633e`; runners in `tests/common/check.rs`.
- The reference tools at `6d5633e` were extracted with `git archive` into a scratch directory and run in the `datumesh-test` image (Python 3.13, PyNaCl) as
  black boxes: `tools/reference_runner.py <spec dir>` (the `formula` and `formula-eval` kinds) and `canon.run_vector` for `call`. For each focused test a vector
  with the vectors' shared class world (`formula/class-fx-rate`'s `env`, `call/class-lookup`'s `env`) was built, with the source and the signature the test uses, and
  the reference's answer (AST, body, inputs, returns, asof-dependence, or the code) became the expectation. Nothing of `spec/tools/` was read.
- `tests/vectors-local/class-seq-*.json` were generated by `tools/gen_class_vectors.py` (the world is built there; the expected results are the reference's;
  the script needs the spec checkout's tools, like `tools/reference_runner.py`). The world: class `calc.example.net/seq` with one number key `n` (unit 1), methods
  `next` (returns a `seq` key) and `val` (a number), a formula function `two_ahead` reading `seq(n).next().val()`.
- The points where the reference and the text differ or the text is silent are in `SPEC-FINDINGS.md`, section H (H1 to H10). Tests that follow the reference
  where the text is silent: `a_dispatch_key_not_canonical_for_its_class` (H2), `the_order_of_checks` (H6), the `resolved` details (H5). If you find the text says
  otherwise, change the test and say so in the report.
