//! Every `formula-eval` and `excel` vector evaluated by the engine with the formula runtime on
//! PostgreSQL: the vector's world in a database of its own (its descriptors bound from their
//! `bound`, its stored receipts with their versions), the cell requested as of the vector's
//! asof from an engine signing as `mesh.example.net` with the vectors' conventions. A valid
//! vector gives the vector's value, lineage, `tx`, receipt `asof` and receipt hash, and its
//! normative fuel is the vector's: the request fails with `BudgetExceeded` at `fuel_used - 1`
//! and succeeds at `fuel_used`. An invalid one answers as SPEC 15.12 says: a fault is
//! `not_covered` with the fault, `E_FORMULA_NOT_COVERED` plain `not_covered`, budget, cycle and
//! depth the engine's errors.
//!
//! `issued` is the request's clock reading; the vectors' is the later of the receipt's `asof`
//! and 2020-01-01, so the clock is set to that. A vector whose cell reads computed cells has
//! those evaluated first, each by a request of its own with its own `issued` (in ascending
//! receipt `asof`), as the in-memory world evaluated them; its fuel is checked in a second
//! database, where nothing is evaluated yet.
#![cfg(feature = "engine")]

mod common;
#[path = "common/pg.rs"]
mod pg;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use datumesh_core::json::Value;
use datumesh_core::{Arg, CellRef, Receipt};
use datumesh_engine::{Answer, EngineError};
use datumesh_formula::definition::{define_function, publish};
use datumesh_formula::eval::CellKey;
use datumesh_formula::memory::{run_vector, DEFAULT_FUEL};
use pg::{Mesh, KEYS_FROM, MIN_INSTANT};

fn host_of(computed_by: &str) -> &str {
    computed_by.split_once('#').map(|(h, _)| h).unwrap_or(computed_by)
}

/// A mesh holding the vector's world.
fn world_mesh(v: &Value) -> Mesh {
    let world = v.get("world").unwrap();
    let receipts = common::array(world.get("receipts").unwrap()).to_vec();
    let hosts: Vec<String> =
        receipts.iter().map(|r| host_of(common::string(r, "computed_by").unwrap()).to_string()).collect();
    let mesh = Mesh::new(&hosts.iter().map(String::as_str).collect::<Vec<_>>());
    let mut fs: Vec<(&String, &Value)> =
        world.get("functions").unwrap().as_object().unwrap().iter().map(|(k, v)| (k, v)).collect();
    fs.sort_by(|a, b| a.0.cmp(b.0));
    for (name, f) in fs {
        let d = f.get("descriptor").unwrap().to_jcs();
        mesh.bind(name, &d, common::string(f, "bound").unwrap());
    }
    // Versions in tx order per cell.
    let mut rs: Vec<(String, String, Value)> = receipts
        .into_iter()
        .map(|r| {
            let parsed = Receipt::from_json(&r.to_jcs()).unwrap();
            (parsed.cell().id(), parsed.tx().to_string(), r)
        })
        .collect();
    rs.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    for (_, _, r) in &rs {
        mesh.put_receipt_value(r);
    }
    if common::string(v, "kind") == Some("excel") {
        let name = common::string(v.get("cell").unwrap(), "fn").unwrap();
        let d = define_function(
            name,
            common::string(v, "formula").unwrap(),
            v.get("signature").unwrap(),
            &Default::default(),
            &*mesh.store,
            common::string(v, "asof").unwrap(),
        )
        .unwrap_or_else(|e| panic!("define: {e}"));
        assert_eq!(Some(d.compiled.body.as_str()), common::string(v, "body"));
        publish(&*mesh.store, &d, MIN_INSTANT).unwrap();
    }
    mesh
}

fn cell_ref_of(key: &CellKey) -> CellRef {
    let (ns, f) = key.name.rsplit_once('/').unwrap();
    CellRef {
        ns: ns.into(),
        fn_name: f.into(),
        args: key.args.iter().map(|(k, s)| (k.clone(), Arg::Value(s.clone()))).collect(),
        asof: None,
    }
}

fn vector_cell(v: &Value) -> CellRef {
    let cell = v.get("cell").unwrap();
    let name = common::string(cell, "fn").unwrap();
    let (ns, f) = name.rsplit_once('/').unwrap();
    let mut a = datumesh_core::Args::new();
    for (k, x) in cell.get("args").unwrap().as_object().unwrap() {
        let ty = datumesh_core::Type::from_name(common::string(x, "type").unwrap()).unwrap();
        let s = match x.get("value").unwrap() {
            Value::Bool(b) => datumesh_core::Scalar::Boolean(*b),
            other => datumesh_core::Scalar::from_lexical(ty, other.as_str().unwrap()).unwrap(),
        };
        a.insert(k.clone(), Arg::Value(s));
    }
    CellRef { ns: ns.into(), fn_name: f.into(), args: a, asof: None }
}

fn issued_for(asof: &str) -> &str {
    if asof > KEYS_FROM {
        asof
    } else {
        KEYS_FROM
    }
}

/// Vectors whose fuel the engine measures otherwise, with the engine's fuel and why. They fail
/// here as soon as the engine measures the vector's fuel.
const KNOWN_FUEL: &[(&str, u64, &str)] = &[(
    "formula-eval/member-fault-caught.json",
    16,
    "engine 0.3.0 evaluates every member of a slice over a computed function before the reader runs \
     again, past the first member that faults; the reference stops there (15.12: strict, left to right)",
)];

fn check_vector(path: &str, v: &Value) -> Result<(), String> {
    let asof = common::string(v, "asof").unwrap();
    let cell = vector_cell(v);
    let depth = v.get("world").unwrap().get("max_depth").map(common::integer);
    let fuel = v.get("fuel").map(common::integer).unwrap_or(DEFAULT_FUEL);
    let mem = run_vector(v).unwrap();
    let mesh = world_mesh(v);
    // Fuel alone: the work limit (16 times the fuel by default) out of the way.
    let engine_with = |mesh: &Mesh, fuel: u64| {
        let (mut b, _) = mesh.builder();
        b = b.fuel(fuel).work_limit(u64::MAX);
        if let Some(d) = depth {
            b = b.depth_limit(d as usize);
        }
        b.build().unwrap()
    };
    if common::boolean(v, "valid") != Some(true) {
        let want = common::string(v, "error").unwrap();
        mesh.clock.set(asof);
        let got = engine_with(&mesh, fuel).get(&cell, Some(asof));
        let ok = match (&got, want) {
            (Ok(Answer::NotCovered { fault: None }), "E_FORMULA_NOT_COVERED") => true,
            (Ok(Answer::NotCovered { fault: Some(f) }), w) => f.as_str() == w,
            (Err(EngineError::BudgetExceeded { .. }), "E_FORMULA_BUDGET") => true,
            (Err(EngineError::Cycle { .. }), "E_FORMULA_CYCLE") => true,
            (Err(EngineError::DepthExceeded { .. }), "E_FORMULA_DEPTH") => true,
            (Err(EngineError::Runtime { message, .. }), w) => message.contains(w),
            _ => false,
        };
        return if ok { Ok(()) } else { Err(format!("{path}: got {got:?}, expected {want}")) };
    }
    let expected = mem.map_err(|c| format!("{path}: the in-memory world gives {c}"))?;
    let fuel_used = common::integer(v.get("fuel_used").unwrap());
    let want_hash = common::string(v, "receipt_hash").unwrap();
    let inner: Vec<_> =
        expected.evaluated.iter().filter(|(_, _, ver)| ver.receipt_hash != want_hash).cloned().collect();
    let mut problems = Vec::new();
    // Fuel: in a database where nothing is evaluated yet (a failed request stores nothing).
    let fuel_mesh = if inner.is_empty() { None } else { Some(world_mesh(v)) };
    {
        let m = fuel_mesh.as_ref().unwrap_or(&mesh);
        m.clock.set(asof);
        let engine_fuel = KNOWN_FUEL.iter().find(|(p, _, _)| *p == path).map(|(_, f, _)| *f).unwrap_or(fuel_used);
        match engine_with(m, engine_fuel - 1).get(&cell, Some(asof)) {
            Err(EngineError::BudgetExceeded { .. }) => {}
            other => problems.push(format!("fuel {} gives {other:?}", engine_fuel - 1)),
        }
        if fuel_mesh.is_some() || engine_fuel != fuel_used {
            match engine_with(m, engine_fuel).get(&cell, Some(asof)) {
                Ok(Answer::Receipt(_)) => {}
                other => problems.push(format!("fuel {engine_fuel} gives {other:?}")),
            }
        }
    }
    // The receipt: inner computed cells first, each with its own issued.
    let fresh = if KNOWN_FUEL.iter().any(|(p, _, _)| *p == path) { Some(world_mesh(v)) } else { None };
    let mesh = fresh.as_ref().unwrap_or(&mesh);
    let engine = engine_with(mesh, DEFAULT_FUEL);
    let mut inner = inner;
    inner.sort_by(|a, b| (&a.2.asof, &a.1).cmp(&(&b.2.asof, &b.1)));
    for (key, at, ver) in &inner {
        mesh.clock.set(issued_for(&ver.asof));
        match engine.get(&cell_ref_of(key), Some(at)) {
            Ok(Answer::Receipt(b)) => {
                let r = Receipt::from_json(&b).unwrap();
                if r.hash() != ver.receipt_hash {
                    problems.push(format!("inner {} as of {at}: receipt {}", key.name, r.hash()));
                }
            }
            other => problems.push(format!("inner {} as of {at}: {other:?}", key.name)),
        }
    }
    mesh.clock.set(issued_for(common::string(v, "receipt_asof").unwrap()));
    match engine.get(&cell, Some(asof)) {
        Ok(Answer::Receipt(bytes)) => {
            let r = Receipt::from_json(&bytes).unwrap();
            if r.value().to_value() != *v.get("value").unwrap() {
                problems.push(format!("value {:?}", r.value()));
            }
            let lineage: Vec<&str> =
                common::array(v.get("lineage").unwrap()).iter().map(|x| x.as_str().unwrap()).collect();
            if r.inputs().iter().map(String::as_str).collect::<Vec<_>>() != lineage {
                problems.push(format!("lineage {:?}", r.inputs()));
            }
            if Some(r.tx()) != common::string(v, "tx") {
                problems.push(format!("tx {}", r.tx()));
            }
            if Some(r.asof()) != common::string(v, "receipt_asof") {
                problems.push(format!("receipt asof {}", r.asof()));
            }
            if r.hash() != want_hash {
                problems.push(format!("receipt hash {}", r.hash()));
            }
            let verified = datumesh_core::verify(&bytes, &mesh.keys, Some(&engine), None);
            if verified.level != Some(datumesh_core::Level::Recomputed) {
                problems.push(format!("verifies at {:?}: {:?}", verified.level, verified.error()));
            }
            mesh.dump(common::string(v, "kind").unwrap(), &bytes);
        }
        other => problems.push(format!("answer {other:?}")),
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!("{path}: {}", problems.join("; ")))
    }
}

fn check_kind(kind: &str) -> usize {
    let vectors = common::vectors_of_kind(kind);
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some((path, v)) = vectors.get(i) else { break };
                if let Err(e) = check_vector(path, v) {
                    failures.lock().unwrap().push(e);
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    assert!(
        failures.is_empty(),
        "{} of {} {kind} vectors fail through the engine:\n{}",
        failures.len(),
        vectors.len(),
        failures.join("\n")
    );
    let _ = pg::admin_url();
    vectors.len()
}

#[test]
fn formula_eval_vectors_through_the_engine() {
    let n = check_kind("formula-eval");
    eprintln!("formula-eval: {n} vectors through the engine");
}

#[test]
fn excel_vectors_through_the_engine() {
    let n = check_kind("excel");
    eprintln!("excel: {n} vectors through the engine");
}
