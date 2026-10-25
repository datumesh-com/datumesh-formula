//! Benchmark of the formula dialect through the engine on PostgreSQL (brief test 13): numbers
//! only, no thresholds.
//!
//!     DATABASE_URL=postgres://… cargo run --release --example bench [-- quick]
//!
//! It makes a database of its own on that server (`dm_formula_bench_<pid>`, dropped at the
//! end) and measures, one call at a time:
//!
//! * parse and define cost per function, for a set of bodies: the canonical AST of a printed
//!   body without an environment (`canonical_ast`), the checked definition against an
//!   environment (`define`), and `define_function` against the store (callees read from it);
//! * evaluation cost per node: arithmetic (a body of 2,001 nodes against one of 1), through the
//!   engine (cold `get`: evaluate, sign, store) and the evaluator alone (`MemoryWorld`);
//! * lookups and aggregates over stored slices of 1,000 and 100,000 members (`last_before`,
//!   `latest`, `sum`, `mean`, `median`, a running `scan`): cold through the engine (the
//!   slice read from the store, evaluated, signed, stored), warm (memo), a fresh engine
//!   (validated), and the evaluator alone over the same members in memory (a `World` holding
//!   them, as the engine answers): the cost per member;
//! * memory per slice input: the engine's memo per member, and the evaluator's slice value.
//!
//! `quick` uses 10,000 members instead of 100,000.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use datumesh_core::json::{self, Value};
use datumesh_core::{Arg, Args, CellRef, Receipt, ReceiptBuilder, Scalar, SigningKey, Type};
use datumesh_engine::{Answer, Clock, Engine};
use datumesh_formula::definition::{define_function, publish};
use datumesh_formula::eval::{self as feval, Binding, CellKey, Fuel, SliceKey, Stop, Version, World};
use datumesh_formula::runtime::FormulaRuntime;
use datumesh_formula::Function;
use datumesh_formula::{canonical_ast, define, Env, Signature};
use datumesh_store::pg_unstable::postgres::{Config, NoTls};
use datumesh_store::{migrate, PgStore, Store};

const MESH: &str = "mesh.example.net";
const PUB: &str = "bench.example.org";
const BIND: &str = "2020-01-01T00:00:00.000000Z";
const ASOF: &str = "2026-06-01T00:00:00.000000Z";

struct Fixed;

impl Clock for Fixed {
    fn now(&self) -> String {
        "2026-07-01T00:00:00.000000Z".to_string()
    }
}

fn key(host: &str) -> SigningKey {
    SigningKey::from_seed(&datumesh_core::sha256(format!("datumesh formula test key: {host}").as_bytes()))
}

fn key_doc(host: &str) -> Vec<u8> {
    let k = key(host);
    let hex: String = k.public_key().iter().map(|b| format!("{b:02x}")).collect();
    format!(
        r#"{{"host":"{host}","keys":[{{"alg":"ed25519","id":"{}","public":"{hex}","valid_from":"2000-01-01T00:00:00.000000Z"}}],"proto":"datumesh/1"}}"#,
        k.key_id()
    )
    .into_bytes()
}

fn scalar(ty: Type, s: &str) -> Scalar {
    Scalar::from_lexical(ty, s).unwrap()
}

fn core_args(pairs: &[(&str, Scalar)]) -> Args {
    pairs.iter().map(|(k, v)| (k.to_string(), Arg::Value(v.clone()))).collect()
}

fn receipt_json(r: &Receipt) -> Vec<u8> {
    let mut m = vec![("sig".to_string(), Value::String(r.sig().to_string()))];
    m.extend(json::parse(r.payload()).unwrap().as_object().unwrap().iter().cloned());
    Value::Object(m).to_jcs()
}

/// Durations of repeated runs: median and 99th percentile, in integer nanoseconds.
struct Stats(Vec<u128>);

impl Stats {
    fn of(mut v: Vec<Duration>) -> Stats {
        v.sort();
        Stats(v.into_iter().map(|d| d.as_nanos()).collect())
    }
    fn p50(&self) -> u128 {
        self.0[self.0.len() / 2]
    }
    fn p99(&self) -> u128 {
        self.0[(self.0.len() * 99 / 100).min(self.0.len() - 1)]
    }
}

fn fmt(ns: u128) -> String {
    if ns >= 10_000_000_000 {
        format!("{} s", ns / 1_000_000_000)
    } else if ns >= 1_000_000_000 {
        format!("{}.{:02} s", ns / 1_000_000_000, (ns / 10_000_000) % 100)
    } else if ns >= 10_000_000 {
        format!("{} ms", ns / 1_000_000)
    } else if ns >= 1_000_000 {
        format!("{}.{:02} ms", ns / 1_000_000, (ns / 10_000) % 100)
    } else if ns >= 10_000 {
        format!("{} µs", ns / 1_000)
    } else if ns >= 1_000 {
        format!("{}.{:02} µs", ns / 1_000, (ns / 10) % 100)
    } else {
        format!("{ns} ns")
    }
}

fn time<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let t = Instant::now();
    let r = f();
    (r, t.elapsed())
}

fn repeat(n: usize, mut f: impl FnMut()) -> Stats {
    let mut v = Vec::with_capacity(n);
    for _ in 0..n {
        let t = Instant::now();
        f();
        v.push(t.elapsed());
    }
    Stats::of(v)
}

fn main() {
    let quick = std::env::args().any(|a| a == "quick");
    let admin = std::env::var("DATABASE_URL").expect("DATABASE_URL names a PostgreSQL 16 database");
    let name = format!("dm_formula_bench_{}", std::process::id());
    let admin_config: Config = admin.parse().unwrap();
    admin_config.connect(NoTls).unwrap().batch_execute(&format!("CREATE DATABASE {name}")).unwrap();
    let (path, query) = admin.split_once('?').map(|(p, q)| (p, Some(q))).unwrap_or((admin.as_str(), None));
    let base = path.rsplit_once('/').unwrap().0;
    let url = match query {
        Some(q) => format!("{base}/{name}?{q}"),
        None => format!("{base}/{name}"),
    };
    let r = std::panic::catch_unwind(|| run(&url, quick));
    let _ = admin_config
        .connect(NoTls)
        .map(|mut c| c.batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)")));
    if let Err(e) = r {
        std::panic::resume_unwind(e);
    }
}

/// The evaluator's world for "the evaluator alone": the functions, and the stored members of
/// every slice in memory, answered as the engine would (members in key order, 1 + n fuel).
#[derive(Clone)]
struct MemWorld {
    functions: BTreeMap<String, Arc<Function>>,
    members: Vec<(feval::Args, Version)>,
    fuel: Fuel,
}

impl World for MemWorld {
    type Error = std::convert::Infallible;
    fn function(&mut self, name: &str, _at: Option<&str>) -> Result<Option<Binding>, Self::Error> {
        Ok(self.functions.get(name).map(|f| Binding { function: f.clone(), start: BIND.to_string() }))
    }
    fn read(&mut self, _cell: &CellKey, _at: Option<&str>) -> Result<Option<Version>, Stop<Self::Error>> {
        self.charge(1)?;
        Ok(None)
    }
    fn members(&mut self, slice: &SliceKey) -> Result<Vec<(feval::Args, Version)>, Stop<Self::Error>> {
        let out: Vec<_> = self.members.iter().filter(|(a, _)| slice.matches(a)).cloned().collect();
        self.charge(1 + out.len() as u64)?;
        Ok(out)
    }
    fn versions(&mut self, _c: &CellKey, _f: &str, _t: &str) -> Result<Vec<Version>, Stop<Self::Error>> {
        Ok(Vec::new())
    }
    fn charge(&mut self, n: u64) -> Result<(), datumesh_formula::Code> {
        self.fuel.charge(n)
    }
    fn asof(&mut self) -> Result<String, Stop<Self::Error>> {
        Ok(ASOF.to_string())
    }
}

struct Bench {
    store: Arc<PgStore>,
    world: MemWorld,
    env_functions: BTreeMap<String, Value>,
}

impl Bench {
    fn shared(&self) -> Arc<dyn Store + Send + Sync> {
        self.store.clone()
    }

    fn engine(&self) -> (Engine, Arc<FormulaRuntime>) {
        let rt = Arc::new(FormulaRuntime::new(self.shared()));
        let e = Engine::builder(self.shared(), MESH)
            .signing_key(key(MESH))
            .clock(Arc::new(Fixed))
            .runtime(rt.clone())
            .memo_capacity(1_000_000)
            .build()
            .unwrap();
        (e, rt)
    }

    fn stored(&mut self, name: &str, signature: &str) {
        let d = format!(r#"{{"code":{{"body":"","lang":"stored"}},"inputs":[],"signature":{signature}}}"#);
        let v = json::parse(d.as_bytes()).unwrap();
        let h = self.store.put_descriptor(&v.to_jcs()).unwrap();
        self.store.bind_name(name, &h, BIND).unwrap();
        self.world.functions.insert(name.to_string(), Arc::new(Function::from_value(&v).unwrap()));
        self.env_functions.insert(name.to_string(), v);
    }

    fn formula(&mut self, name: &str, source: &str, signature: &str) {
        let sig = json::parse(signature.as_bytes()).unwrap();
        let d = define_function(name, source, &sig, &BTreeMap::new(), &*self.store, BIND).unwrap();
        publish(&*self.store, &d, BIND).unwrap();
        let v = json::parse(&d.bytes).unwrap();
        self.world.functions.insert(name.to_string(), Arc::new(Function::from_value(&v).unwrap()));
        self.env_functions.insert(name.to_string(), v);
    }

    /// `n` stored members of `name(k)`, k = 1..n, value k mod 1000 + 0.5.
    fn members(&mut self, name: &str, n: i64) {
        let fn_hash = self.store.resolve_name(name, BIND).unwrap().unwrap();
        let (ns, f) = name.rsplit_once('/').unwrap();
        for k in 1..=n {
            let tx =
                format!("2021-01-01T{:02}:{:02}:{:02}.{:06}Z", (k / 3600) % 24, (k / 60) % 60, k % 60, k % 1_000_000);
            let ka = scalar(Type::Number, &k.to_string());
            let mut fa = feval::Args::new();
            fa.insert("k".to_string(), ka.clone());
            let cell = datumesh_core::Cell {
                ns: ns.into(),
                fn_name: f.into(),
                fn_hash: fn_hash.clone(),
                args: core_args(&[("k", ka)]),
            };
            let value = scalar(Type::Number, &format!("{}.5", k % 1000));
            let r = ReceiptBuilder::new()
                .cell(cell.clone())
                .value(value.clone())
                .valid(Args::new())
                .asof(tx.as_str())
                .tx(tx.as_str())
                .issued(tx.as_str())
                .publisher(PUB)
                .signer_host(PUB)
                .licence("CC0-1.0")
                .summary("A bench value.")
                .sign(&key(PUB))
                .unwrap();
            let bytes = receipt_json(&r);
            let h = self.store.put_receipt(&bytes).unwrap();
            self.store.put_version(&cell.id(), &tx, &h).unwrap();
            self.world.members.push((fa, Version { value, receipt_hash: h, tx: tx.clone(), asof: tx }));
        }
    }

    fn env(&self, name: &str, signature: &str) -> Env {
        let mut env = Env::new(name, Signature::from_value(&json::parse(signature.as_bytes()).unwrap()).unwrap());
        for (n, d) in &self.env_functions {
            env.add_function(n, d).unwrap();
        }
        env
    }
}

fn get(engine: &Engine, name: &str, pairs: &[(&str, Scalar)]) -> Answer {
    let (ns, f) = name.rsplit_once('/').unwrap();
    let r = CellRef { ns: ns.into(), fn_name: f.into(), args: core_args(pairs), asof: None };
    let a = engine.get(&r, Some(ASOF)).unwrap();
    assert!(matches!(a, Answer::Receipt(_)), "{name}: {a:?}");
    a
}

const N1: &str = r#"{"type":"number","unit":"1","valid":false}"#;

fn sig1() -> String {
    format!(r#"{{"args":{{"x":{N1}}},"order":["x"],"returns":{{"type":"number","unit":"1"}}}}"#)
}

fn run(url: &str, quick: bool) {
    let store = Arc::new(PgStore::connect(url).unwrap());
    migrate::apply(&store).unwrap();
    for h in [MESH, PUB] {
        store.put_namespace(h, &key_doc(h), "2026-01-01T00:00:00.000000Z").unwrap();
    }
    let world = MemWorld { functions: BTreeMap::new(), members: Vec::new(), fuel: Fuel::new(u64::MAX) };
    let mut b = Bench { store, world, env_functions: BTreeMap::new() };
    let big: i64 = if quick { 10_000 } else { 100_000 };
    let ssig = format!(r#"{{"args":{{"k":{N1}}},"order":["k"],"returns":{{"type":"number","unit":"1"}}}}"#);
    b.stored("bench.example.org/s", &ssig);
    b.stored("bench.example.org/rate", &ssig);
    eprintln!("storing {big} members ...");
    let (_, t) = time(|| b.members("bench.example.org/s", big));
    println!("setup: {big} stored members signed and stored in {}", fmt(t.as_nanos()));

    // ---------------------------------------------------------------------------------------
    println!("\n## Parse and define, per function (median of 200; p99)\n");
    println!("| Body | nodes | canonical_ast | define | define_function (store) | define per node |");
    println!("|---|---:|---:|---:|---:|---:|");
    let chain: String = (0..1000).map(|i| format!("x * {}.5", i % 9 + 1)).collect::<Vec<_>>().join(" + ");
    let bodies: Vec<(&str, String)> = vec![
        ("arithmetic", "ROUND(x * 1.2 + 3 / x - 2 ^ 3, 2)".to_string()),
        ("if and let", "LET y = x * 2 IN IF(y > 10 AND x < 100, y - 10, 0)".to_string()),
        ("read", "bench.example.org/rate(x) * x".to_string()),
        ("sum of a slice", "SUM(bench.example.org/s(*))".to_string()),
        ("last_before, bound", "LAST_BEFORE(bench.example.org/s(*), x, 10)".to_string()),
        (
            "scan and window",
            "LAST(SCAN(bench.example.org/s(*), sum)) + MAX(WINDOW(bench.example.org/s(*), 7, mean))".to_string(),
        ),
        ("map and filter", "SUM(MAP(FILTER(bench.example.org/s(*), v -> v > 1), v -> v * 2))".to_string()),
        ("1,000-term chain", chain.clone()),
    ];
    for (what, src) in &bodies {
        let sig = sig1();
        let env = b.env("bench.example.org/f", &sig);
        let c = define(src, &env).unwrap();
        let printed = c.body.clone();
        let nodes = c.ast.node_count() as u128;
        let n = 200;
        let parse = repeat(n, || {
            canonical_ast(&printed, None).unwrap();
        });
        let def = repeat(n, || {
            define(src, &env).unwrap();
        });
        let sigv = json::parse(sig.as_bytes()).unwrap();
        let df = repeat(n / 4, || {
            define_function("bench.example.org/f", src, &sigv, &BTreeMap::new(), &*b.store, BIND).unwrap();
        });
        println!(
            "| {what} | {nodes} | {} ({}) | {} ({}) | {} ({}) | {} |",
            fmt(parse.p50()),
            fmt(parse.p99()),
            fmt(def.p50()),
            fmt(def.p99()),
            fmt(df.p50()),
            fmt(df.p99()),
            fmt(def.p50() / nodes)
        );
    }

    // ---------------------------------------------------------------------------------------
    println!("\n## Evaluation per node: arithmetic (cold get through the engine, median of 50)\n");
    let long: String = (0..1000).map(|i| format!("x * {}.5", i % 9 + 1)).collect::<Vec<_>>().join(" + ");
    b.formula("bench.example.org/one", "x", &sig1());
    b.formula("bench.example.org/long", &long, &sig1());
    let nodes_long = define(&long, &b.env("bench.example.org/long", &sig1())).unwrap().ast.node_count() as u128;
    let (engine, _) = b.engine();
    let mut k = 0u64;
    let mut cold = |name: &str| {
        repeat(50, || {
            k += 1;
            get(&engine, name, &[("x", scalar(Type::Number, &format!("{k}.25")))]);
        })
    };
    let one = cold("bench.example.org/one");
    let longs = cold("bench.example.org/long");
    let per = longs.p50().saturating_sub(one.p50()) / (nodes_long - 1);
    println!("| Body | nodes | cold get p50 | p99 |");
    println!("|---|---:|---:|---:|");
    println!("| `x` | 1 | {} | {} |", fmt(one.p50()), fmt(one.p99()));
    println!("| 1,000 products summed | {nodes_long} | {} | {} |", fmt(longs.p50()), fmt(longs.p99()));
    println!("\nPer arithmetic node through the engine: {} (the difference over {} nodes).", fmt(per), nodes_long - 1);
    let c = define(&long, &b.env("bench.example.org/long", &sig1())).unwrap();
    let f = Function::from_value(&b.env_functions["bench.example.org/long"]).unwrap();
    let mut world = b.world.clone();
    let mut xs = 0u64;
    let alone = repeat(2000, || {
        xs += 1;
        let mut a = feval::Args::new();
        a.insert("x".into(), scalar(Type::Number, &format!("{xs}.25")));
        world.fuel = Fuel::new(10_000_000);
        feval::evaluate_value(&c, &f, &a, &mut world).unwrap();
    });
    println!(
        "The evaluator alone (in memory): {} per body of {nodes_long} nodes, {} per node.",
        fmt(alone.p50()),
        fmt(alone.p50() / nodes_long)
    );

    // ---------------------------------------------------------------------------------------
    println!("\n## Lookups and aggregates over stored slices\n");
    let readers: &[(&str, &str)] = &[
        ("sum", "SUM(bench.example.org/s([1..x]))"),
        ("mean", "MEAN(bench.example.org/s([1..x]))"),
        ("median", "MEDIAN(bench.example.org/s([1..x]))"),
        ("last_before", "LAST_BEFORE(bench.example.org/s([1..x]), x / 2)"),
        ("latest", "LATEST(bench.example.org/s([1..x]))"),
        ("running scan", "LAST(SCAN(bench.example.org/s([1..x]), sum))"),
    ];
    for (i, (what, src)) in readers.iter().enumerate() {
        b.formula(&format!("bench.example.org/r{i}"), src, &sig1());
        let _ = what;
    }
    println!(
        "| Reader | members | cold get (first ever) | warm get (memo) | fresh engine (validated) | evaluator alone | evaluator per member | memo per member |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|");
    for n in [1000i64, big] {
        for (i, (what, src)) in readers.iter().enumerate() {
            let name = format!("bench.example.org/r{i}");
            let x = scalar(Type::Number, &n.to_string());
            let (engine, _) = b.engine();
            let before = engine.memo_stats().approx_bytes;
            let (_, cold) = time(|| get(&engine, &name, &[("x", x.clone())]));
            let mem = engine.memo_stats().approx_bytes.saturating_sub(before) as u128;
            let warm = repeat(20, || {
                get(&engine, &name, &[("x", x.clone())]);
            });
            let (fresh, _) = b.engine();
            let (_, validated) = time(|| get(&fresh, &name, &[("x", x.clone())]));
            // The evaluator alone over the same members in memory.
            let c = define(src, &b.env(&name, &sig1())).unwrap();
            let f = Function::from_value(&b.env_functions[&name]).unwrap();
            // Only the slice's members (k = 1..n, the first n), as the store answers them.
            let mut world = b.world.clone();
            world.members.truncate(n as usize);
            let runs = if n > 10_000 { 3 } else { 20 };
            let alone = repeat(runs, || {
                world.fuel = Fuel::new(100_000_000);
                let mut a = feval::Args::new();
                a.insert("x".into(), x.clone());
                feval::evaluate_value(&c, &f, &a, &mut world).unwrap();
            });
            println!(
                "| {what} | {n} | {} | {} | {} | {} | {} | {} B |",
                fmt(cold.as_nanos()),
                fmt(warm.p50()),
                fmt(validated.as_nanos()),
                fmt(alone.p50()),
                fmt(alone.p50() / n as u128),
                mem / n as u128
            );
        }
    }
    println!(
        "\nThe evaluator's slice value: {} B per member with one key (its key tuple and value, inline; \
         strings and blobs besides).",
        feval::member_bytes(1)
    );
}
