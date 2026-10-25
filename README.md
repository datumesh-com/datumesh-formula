# datumesh-formula

The formula dialect of [Datumesh](https://datumesh.com): `code.lang = "formula"`, the default
language of computed functions, an Excel-compatible expression language over cells. It implements
section 15 of [spec-0.4.0](https://github.com/datumesh-com/datumesh-spec): the lexer and PEG
grammar, resolution to the canonical AST and its hash, the printer that gives `code.body`, the type
and unit checker that derives `inputs`, exact decimal arithmetic with the 40-digit transcendental
algorithms, units, angles and haversine, an evaluator with normative fuel, faults and lineage, the
definition of formula functions (the canonical descriptor), and the
[`datumesh-engine`](https://crates.io/crates/datumesh-engine) `Runtime` for `lang = "formula"`.

## Example

```toml
[dependencies]
datumesh-formula = "0.1.0"
datumesh-engine = "0.3.0"
datumesh-store = "0.3.1"
datumesh-core = { version = "0.3.4", features = ["sign"] }
```

A publisher's stored daily price, a formula function over a week of it defined against the store
and published, the engine answering a cell of it with the formula runtime, and the receipt
verified at level `recomputed` with the engine as the resolver (`examples/engine.rs`; run it with
`DATABASE_URL` naming a PostgreSQL 16 database):

```rust
use std::collections::BTreeMap;
use std::sync::Arc;

use datumesh_core::json::{self, Value};
use datumesh_core::{verify, Arg, Args, Cell, CellRef, Keys, Level, Receipt, ReceiptBuilder, Resolver, Scalar, SigningKey, Type};
use datumesh_engine::{Answer, Engine};
use datumesh_formula::definition::{define_function, publish};
use datumesh_formula::runtime::FormulaRuntime;
use datumesh_store::{migrate, PgStore, Store};

const T0: &str = "2026-01-01T00:00:00.000000Z";

/// A key document publishing `key` for `host`, as https://<host>/.well-known/datumesh.json serves it.
fn key_doc(host: &str, key: &SigningKey) -> String {
    let public: String = key.public_key().iter().map(|b| format!("{b:02x}")).collect();
    format!(r#"{{"host":"{host}","keys":[{{"alg":"ed25519","id":"{}","public":"{public}","valid_from":"{T0}"}}],"proto":"datumesh/1"}}"#, key.key_id())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(PgStore::connect(&std::env::var("DATABASE_URL")?)?);
    migrate::apply(&store)?;
    let publisher = SigningKey::from_seed(&[1; 32]);
    let mesh = SigningKey::from_seed(&[2; 32]);
    let mut keys = Keys::new();
    for (host, key) in [("prices.example.org", &publisher), ("mesh.example.com", &mesh)] {
        if store.namespace(host)?.is_none() {
            store.put_namespace(host, key_doc(host, key).as_bytes(), T0)?;
        }
        keys.insert_response(host, 200, Some(key_doc(host, key).as_bytes()));
    }

    // A publisher's stored function: the daily price of an item, in EUR.
    let daily = r#"{"code":{"body":"","lang":"stored"},"inputs":[],"signature":{"args":{"date":{"type":"date","valid":true},
        "item":{"type":"string","valid":false}},"order":["item","date"],"returns":{"type":"number","unit":"EUR"}}}"#;
    let daily_hash = store.put_descriptor(daily.as_bytes())?;
    if store.resolve_name("prices.example.org/daily", T0)?.is_none() {
        store.bind_name("prices.example.org/daily", &daily_hash, T0)?;
    }

    // A formula function over it: the mean over the seven days to d, with VAT. Its inputs are
    // derived from the body; define checks it against the store's bindings and gives the
    // canonical descriptor, which publish stores and binds.
    let signature = json::parse(br#"{"args":{"d":{"type":"date","valid":true},"item":{"type":"string","valid":false}},
        "order":["item","d"],"returns":{"type":"number","unit":"EUR"}}"#)?;
    let source = "ROUND(MEAN(prices.example.org/daily(item, [d - 6..d])) * 1.2, 2)";
    let def = define_function("shop.example.com/week_price", source, &signature, &BTreeMap::new(), &*store, T0)?;
    assert_eq!(def.descriptor.body, "round(mean(prices.example.org/daily(date: [d - 6..d], item: item)) * 1.2, digits: 2)\n");
    assert_eq!(def.descriptor.inputs, ["prices.example.org/daily"]);
    if store.resolve_name("shop.example.com/week_price", T0)?.is_none() {
        publish(&*store, &def, T0)?;
    }

    // The publisher's prices for a week, each recorded the day after.
    let item = Scalar::from_lexical(Type::String, "bolt")?;
    for (i, price) in ["10", "11", "12", "13", "14", "15", "16"].into_iter().enumerate() {
        let date = format!("2026-03-{:02}", i + 1);
        let tx = format!("2026-03-{:02}T09:00:00.000000Z", i + 2);
        let a: Args = [
            ("date".to_string(), Arg::Value(Scalar::from_lexical(Type::Date, &date)?)),
            ("item".to_string(), Arg::Value(item.clone())),
        ]
        .into();
        let cell = Cell { ns: "prices.example.org".into(), fn_name: "daily".into(), fn_hash: daily_hash.clone(), args: a };
        if store.latest(&cell.id())?.is_some() {
            continue;
        }
        let valid: Args = [("date".to_string(), Arg::Value(Scalar::from_lexical(Type::Date, &date)?))].into();
        let receipt = ReceiptBuilder::new()
            .cell(cell.clone())
            .value(Scalar::from_lexical(Type::Number, price)?)
            .valid(valid)
            .asof(tx.as_str())
            .tx(tx.as_str())
            .issued(tx.as_str())
            .publisher("prices.example.org")
            .signer_host("prices.example.org")
            .licence("CC-BY-4.0")
            .summary("The daily price of a bolt.")
            .sign(&publisher)?;
        let mut members = vec![("sig".to_string(), Value::String(receipt.sig().to_string()))];
        members.extend(json::parse(receipt.payload())?.as_object().into_iter().flatten().cloned());
        let hash = store.put_receipt(&Value::Object(members).to_jcs())?;
        store.put_version(&cell.id(), &tx, &hash)?;
    }

    // The engine with the formula runtime; the computed cell as of 10 March.
    let shared: Arc<dyn Store + Send + Sync> = store.clone();
    let runtime = Arc::new(FormulaRuntime::new(shared.clone()));
    let engine = Engine::builder(shared, "mesh.example.com").signing_key(mesh).runtime(runtime).build()?;
    let week = CellRef {
        ns: "shop.example.com".into(),
        fn_name: "week_price".into(),
        args: [
            ("d".to_string(), Arg::Value(Scalar::from_lexical(Type::Date, "2026-03-07")?)),
            ("item".to_string(), Arg::Value(item)),
        ]
        .into(),
        asof: None,
    };
    let Answer::Receipt(bytes) = engine.get(&week, Some("2026-03-10T00:00:00.000000Z"))? else {
        return Err("not covered".into());
    };
    let receipt = Receipt::from_json(&bytes)?;
    assert_eq!(receipt.value().lexical(), "15.6"); // mean 13, times 1.2
    // One input: the slice of the seven prices (slice-as-lineage), its manifest in the store.
    assert_eq!(receipt.inputs().len(), 1);
    assert!(store.manifest(&receipt.inputs()[0]).is_some());
    let result = verify(&bytes, &keys, Some(&engine), None);
    assert_eq!(result.level, Some(Level::Recomputed), "{:?}", result.error());
    println!("week_price = {} EUR as of {}, from {}", receipt.value().lexical(), receipt.asof(), receipt.inputs()[0]);
    println!("verified {} receipt {}", result.level.map(|l| l.as_str()).unwrap_or("?"), receipt.hash());
    Ok(())
}
```

It prints `week_price = 15.6 EUR as of 2026-03-08T09:00:00.000000Z` and the slice entry it read,
then the receipt's hash. Run again, it finds everything in place and the engine validates the
stored version.

Without the engine (`default-features = false`), `define` checks a body against an `Env` and
`eval::evaluate` runs it over any `World`, such as `memory::MemoryWorld` (`examples/quickstart.rs`).

## What is in the crate

| Module | |
|---|---|
| `define`, `canonical_ast`, `Compiled` | Lex, parse, resolve and check a body (15.2-15.10); `parse(print(ast)) = ast` without an environment |
| `ast` | The canonical AST (an arena), its JCS bytes and hash, the printer |
| `decimal` | 34 digits half-even with the range rule; SQRT-T40, EXP-T48, LN-ATANH80, POW-EXPLN, TRIG-T24, ATAN2-T32, HAVERSINE at 40 digits; integers only (`u64` limbs), no float anywhere |
| `units` | The units of `units.md` and the unit algebra of 15.10 |
| `calendar` | Dates, instants, business days, rolls, year fractions |
| `text` | `upper`/`lower` with Unicode 15.1 full case mappings carried in the crate (generated by `tools/gen_casing.py` from Python 3.13), `trim`, `substr`, `find` |
| `eval` | The evaluator over a `World`, with `Fuel`, `Outcome` (value, lineage, manifests, tx, receipt asof) |
| `memory` | `MemoryWorld`: bindings, stored receipts, computed cells memoised with cycle and depth limits, and the vectors' receipt conventions |
| `definition` | `define_function`: the canonical descriptor (printed body, derived `inputs`, `order` and extents as given, every check of 15.6) from a body, a signature, an alias map and the callees as bound at define time (`Callees`: a store or an `Env`); `publish` (`put_descriptor` + `bind_name`) |
| `runtime` (feature `engine`) | `FormulaRuntime`, the engine's `Runtime` for `lang = "formula"`: the evaluator over the engine's `Inputs` |

### The world

```rust
pub trait World {
    type Error;                                                      // passed through as Stop::World
    fn function(&mut self, name: &str, at: Option<&str>) -> Result<Option<Binding>, Self::Error>;
    fn read(&mut self, cell: &CellKey, at: Option<&str>) -> Result<Option<Version>, Stop<Self::Error>>;
    fn members(&mut self, slice: &SliceKey) -> Result<Vec<(Args, Version)>, Stop<Self::Error>>;
    fn versions(&mut self, cell: &CellKey, from: &str, to: &str) -> Result<Vec<Version>, Stop<Self::Error>>;
    fn charge(&mut self, n: u64) -> Result<(), Code>;                // the request's fuel
    fn asof(&mut self) -> Result<String, Stop<Self::Error>>;         // asked only when needed
    fn rebound(&mut self, name: &str, b: &Binding)                   // a callee rebound: check again
        -> Result<Option<Arc<Compiled>>, Stop<Self::Error>> { Ok(None) }
}
```

A `Version` is a value and the receipt that states it (hash, `tx`, `asof`). The evaluator asks the
world for single cells as of the evaluation asof or as of an explicit `T`, for the members of
slices, and for version slices; a computed cell or a slice over a computed function is the world's
to evaluate (memoise, sign, refuse cycles and depth), using `eval::extent_cells` for the cells of
an extent. The evaluator charges nodes and built-ins through `charge`; the world charges reads
as 15.12's table says. From the answers the evaluator derives the lineage, the manifests, the latest
input `tx` and the receipt `asof` of 8.2 rule 2. A world's own error (an input not evaluated yet, a
store failure) comes back from `evaluate` unchanged. `evaluate_value` gives only the value and asks
the world for the asof only when the body needs it (`asof()`, a bounded `latest`, and the order of
`E_FORMULA_FUTURE` in `asof(c, T)` and `versions`): the runtime's mode, where the engine keeps the
lineage and reading the asof would make the evaluation asof-dependent.

### The runtime

`FormulaRuntime::new(store)` is the thin adapter of `World` over the engine's `Inputs`:
`read_cell`, `read_asof`, `read_slice` (members in key order), `read_versions` and `binding`,
which charge reads as 15.12 says while the evaluator charges nodes and built-ins to the `Budget`.
A fault is `EvalError::Fault` with its code, so the engine answers `NotCovered { fault }`; a
computed input's fault comes back as its formula code, which `otherwise` catches where 15.12 says;
the engine's own errors (an input not evaluated yet or without a value, a cycle, the depth, the
budget) pass through; any other code is `EvalError::Runtime` with the code first.
`asof_dependent` is the checker's flag. A descriptor is checked (15.6) against its callees as bound
now and compiled once per function hash; a read whose name resolves to another function (a callee
rebound, or a name read as of an earlier `T`) checks the body again (15.8), and a body that does not
check against the latest bindings is checked against those of the evaluation asof.
`FormulaRuntime::stats` counts attempts and completed evaluations.

## Conformance

- Every vector of kinds `formula` (293), `formula-eval` (155), `excel` (78) and `decimal` (20
  files, 6,054 cases) passes, and the evaluation of 11 of the 12 slice-lineage receipt vectors whose
  inputs have descriptors among the vectors.
- Through the engine on PostgreSQL (`tests/engine_vectors.rs`): every `formula-eval` and `excel`
  vector, in a database of its own, gives the vector's value, lineage, `tx`, receipt `asof` and
  receipt hash (signed as `mesh.example.net` with the vectors' conventions), verifies at
  `recomputed`, and answers invalid vectors as 15.12 says; the engine-measured fuel equals the
  vector's on all but one (`member-fault-caught`: datumesh-engine 0.3.0 evaluates every member of
  a computed slice past the first fault, 16 where the vector says 7).
- The brief's tests through the engine (`tests/brief.rs`): `fib(90)` exact with 91 memoised cells;
  a curve bootstrapped over ten tenors; a weighted mean over 1,000 stations with two slice entries,
  a backdated correction, the old receipt signed with `E_INPUT_SUPERSEDED`; `last_before` with a
  staleness bound and `latest` at two asofs; a running total over 3,650 days, one slice entry per
  day, a correction dirtying only the days on or after it.
- Generated bodies through two fresh engines on two databases give byte-identical receipts and
  agree with the in-memory world (`tests/engine_determinism.rs`, `FORMULA_ENGINE_CASES`; 100,000
  at full size).
- `tools/crosscheck.sh`: every receipt those tests sign verifies at level `signed` with Python
  `datumesh` 0.4.0 and JavaScript `@datumesh/verify` 0.4.0, manifests included.
- `parse(print(ast)) = ast` on fuzzed trees, and evaluating generated bodies twice is identical,
  100,000 each at full size (`FORMULA_ROUNDTRIP_CASES`, `FORMULA_FUZZ_CASES`).
- `tools/decimal_crosscheck.sh` compares 10,000 random cases per operation with Python's `decimal`
  (`python:3.13-slim`, standard library only) and, for sin, cos, atan2 and haversine, with the
  reference tool run as a black box.
- `tools/fuzz.sh` runs the `parse` and `evaluate` cargo-fuzz targets: no input panics.
- Bodies at the limits of 15.3 (65,535 nodes in one chain, 256 levels of nesting) are defined,
  printed, round-tripped and evaluated: operator chains are walked iteratively, and a body of more
  than 128 tokens is parsed and checked on a thread with a 64 MiB stack (the caller's stack where
  threads are not available), so a debug build needs no large stack either.
- `SPEC-FINDINGS.md` lists where the specification leaves a reading open and which reading the
  crate takes.

## Development

With a PostgreSQL 16 server whose role may create databases in `DATABASE_URL`, `cargo test` runs
everything (the engine tests make a database each and drop it). `tools/cargo.sh <tag> <args>` runs
cargo in a container on the `datumesh` network next to a `datumesh-pg` container:
`tools/cargo.sh 1.99 test`, `tools/cargo.sh 1.97 test` (the MSRV),
`tools/cargo.sh lint clippy --all-targets -- -D warnings`, `tools/cargo.sh lint fmt`.
`tools/package_test.sh` is the packaging test, `tools/public_api.sh` checks `public-api.txt`,
`tools/crosscheck.sh` the cross-language check, `tools/decimal_crosscheck.sh` the decimals. `tools/gen_cases.py`, `tools/reference_runner.py` and
`tools/compare.py` run generated cases through this crate (`examples/formula_runner.rs`) and through
another implementation and compare them. `spec-vectors/` is a copy of the spec's vectors at `dedd132`, unchanged at `f3d3e2c`.

Dependencies: `datumesh-core` 0.3.4 for strict JSON, JCS, SHA-256, the string rules of SPEC 3.5
(Unicode 15.1 assigned code points, NFC), `FunctionDescriptor` and `Fault`; `unicode-normalization`
(already a dependency of `datumesh-core`) to normalise computed strings to NFC; with feature
`engine` (default), `datumesh-engine` 0.3.0 for `Runtime` and `datumesh-store` 0.3.1 (already a
dependency of the engine) for the callees and `publish`. Without the feature, the language alone.


## Benchmark

`examples/bench.rs` (`cargo run --release --example bench`), against PostgreSQL 16 with default
settings on the development host (6 vCPU, 15 GB RAM), one call at a time, the engine's memo on;
the full run is in `bench/2026-10-25-0.1.0.md`.

| Parse and define (per function, p50) | nodes | `canonical_ast` | `define` | `define_function` (store) |
|---|---:|---:|---:|---:|
| `ROUND(x * 1.2 + 3 / x - 2 ^ 3, 2)` | 13 | 13 µs | 20 µs | 152 µs |
| `SUM(bench.example.org/s(*))` | 3 | 3.6 µs | 4.3 µs | 614 µs |
| `LAST_BEFORE(bench.example.org/s(*), x, 10)` | 5 | 9.1 µs | 13 µs | 668 µs |
| a 1,000-term sum of products | 3,999 | 4.9 ms | 9.2 ms | 23 ms |

Define costs 1.2 to 2.9 µs per node; `define_function` adds the store's binding and descriptor
queries per callee (about 0.5 ms).

Evaluation per node: an arithmetic node costs 2.3 µs through the engine (the difference between
cold `get`s of a 3,999-node body, 15 ms, and of `x`, 6.0 ms: evaluating, signing and storing), and
248 ns in the evaluator alone.

| Slice reader | members | cold `get` | warm `get` | fresh engine (validated) | evaluator alone, per member | memo per member |
|---|---:|---:|---:|---:|---:|---:|
| `sum` | 1,000 | 464 ms | 0.42 ms | 255 ms | 3.0 µs | 469 B |
| `median` | 1,000 | 249 ms | 0.42 ms | 263 ms | 3.0 µs | 469 B |
| `last_before` | 1,000 | 256 ms | 0.52 ms | 317 ms | 3.8 µs | 469 B |
| running `scan` | 1,000 | 234 ms | 0.35 ms | 238 ms | 4.9 µs | 469 B |
| `sum` | 100,000 | 20 s | 0.42 ms | 15 s | 8.8 µs | 467 B |
| `median` | 100,000 | 15 s | 0.36 ms | 15 s | 11 µs | 467 B |
| `last_before` | 100,000 | 15 s | 0.32 ms | 15 s | 10 µs | 467 B |
| running `scan` | 100,000 | 14 s | 0.49 ms | 15 s | 12 µs | 467 B |

A cold slice read is dominated by the store's `slice_members` (about 150 µs per member at
100,000, receipt bytes included); the evaluator's own work is under a tenth of it. Memory per
slice input: 467 B per member in the engine's memo, 104 B per member in the evaluator's slice value.

Licensed under Apache-2.0.
