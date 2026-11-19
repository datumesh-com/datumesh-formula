//! The `formula-eval` and `excel` vectors evaluated by the engine with the formula runtime on
//! PostgreSQL (feature `engine`): the vector's world in a database of its own (its descriptors
//! bound from their `bound`, its stored receipts with their versions), the cell requested as of
//! the vector's asof from an engine signing as `mesh.example.net` with the vectors' conventions.
//! A valid vector gives the vector's value, lineage, `tx`, receipt `asof` and receipt hash, and its
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
//!
//! Shared by `engine_vectors.rs` (the vectors of spec-0.4.2) and `engine_classes.rs` (the class
//! vectors of spec-0.5.0), each of which declares `mod common; #[path = "common/engine.rs"] mod
//! engine; #[path = "common/pg.rs"] mod pg;`.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use datumesh_core::json::Value;
use datumesh_core::{Arg, CellRef, Receipt};
use datumesh_engine::{Answer, EngineError};
use datumesh_formula::definition::{define_function, publish};
use datumesh_formula::eval::CellKey;
use datumesh_formula::memory::{run_vector, DEFAULT_FUEL};
use datumesh_store::Store;

use crate::common;
use crate::pg::{self, Mesh, KEYS_FROM, MIN_INSTANT};

pub fn host_of(computed_by: &str) -> &str {
    computed_by.split_once('#').map(|(h, _)| h).unwrap_or(computed_by)
}

/// A mesh holding the vector's world.
pub fn world_mesh(v: &Value, classes: bool) -> Mesh {
    let world = v.get("world").unwrap();
    let receipts = common::array(world.get("receipts").unwrap()).to_vec();
    let hosts: Vec<String> =
        receipts.iter().map(|r| host_of(common::string(r, "computed_by").unwrap()).to_string()).collect();
    let mesh = Mesh::new(&hosts.iter().map(String::as_str).collect::<Vec<_>>());
    let mut fs: Vec<(&String, &Value)> =
        world.get("functions").unwrap().as_object().unwrap().iter().map(|(k, v)| (k, v)).collect();
    fs.sort_by(|a, b| a.0.cmp(b.0));
    // With `classes`, a class is put and bound (`bind_class` binds each method `N/m` too, so the
    // methods' functions are only stored first); without, the world's functions are bound as the
    // vectors list them and the classes are not known to the store.
    let class_world: Vec<(&String, &Value)> = match world.get("classes").and_then(Value::as_object) {
        Some(m) if classes => m.iter().map(|(k, v)| (k, v)).collect(),
        _ => Vec::new(),
    };
    let is_method = |name: &str| {
        class_world.iter().any(|(class, c)| {
            let d = c.get("descriptor").unwrap();
            name.strip_prefix(&format!("{class}/")).is_some_and(|m| d.get("methods").and_then(|ms| ms.get(m)).is_some())
        })
    };
    for (name, f) in fs {
        let d = f.get("descriptor").unwrap().to_jcs();
        if is_method(name) {
            mesh.store.put_descriptor(&d).unwrap();
        } else {
            mesh.bind(name, &d, common::string(f, "bound").unwrap());
        }
    }
    for (name, c) in class_world {
        let d = datumesh_core::ClassDescriptor::from_value(c.get("descriptor").unwrap()).unwrap();
        let hash = mesh.store.put_class(&d.canonical()).unwrap();
        assert_eq!(hash, d.hash());
        mesh.store.bind_class(name, &hash, common::string(c, "bound").unwrap()).unwrap();
    }
    // The world's blob content, in the store (the engine reads it by SHA-256).
    if let Some(bs) = world.get("blobs").and_then(Value::as_object) {
        for (sha, hex) in bs {
            let hex = hex.as_str().unwrap();
            let bytes: Vec<u8> =
                (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()).collect();
            assert_eq!(&mesh.store.put_blob(&bytes).unwrap(), sha);
        }
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

pub fn check_vector(path: &str, v: &Value, classes: bool) -> Result<(), String> {
    // Evaluator-only (the spec's README): a mesh evaluates each callee cell on its own, and since
    // engine 0.5 it splits a deep recursion itself (Build 2 E4), so no request reaches the
    // evaluator's depth limit and the engine gives the value.
    if path.ends_with("invalid/formula-eval-depth.json") {
        return Ok(());
    }
    let asof = common::string(v, "asof").unwrap();
    let cell = vector_cell(v);
    let depth = v.get("world").unwrap().get("max_depth").map(common::integer);
    let fuel = v.get("fuel").map(common::integer).unwrap_or(DEFAULT_FUEL);
    let mem = run_vector(v).unwrap();
    let mesh = world_mesh(v, classes);
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
            (Err(EngineError::BlobUnavailable { .. }), "E_FORMULA_BLOB_UNAVAILABLE") => true,
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
    let fuel_mesh = if inner.is_empty() { None } else { Some(world_mesh(v, classes)) };
    {
        let m = fuel_mesh.as_ref().unwrap_or(&mesh);
        m.clock.set(asof);
        match engine_with(m, fuel_used - 1).get(&cell, Some(asof)) {
            Err(EngineError::BudgetExceeded { .. }) => {}
            other => problems.push(format!("fuel {} gives {other:?}", fuel_used - 1)),
        }
        if fuel_mesh.is_some() {
            match engine_with(m, fuel_used).get(&cell, Some(asof)) {
                Ok(Answer::Receipt(_)) => {}
                other => problems.push(format!("fuel {fuel_used} gives {other:?}")),
            }
        }
    }
    // The receipt: inner computed cells first, each with its own issued.
    // SPEC section 11: the vectors' hashes use the licence convention `CC0-1.0`; `names` maps an
    // engine receipt's actual hash to its convention hash, `known` the actual hashes to licences.
    let mut names = std::collections::BTreeMap::new();
    let mut known = std::collections::BTreeMap::new();
    for r in common::array(v.get("world").unwrap().get("receipts").unwrap()) {
        let r = Receipt::from_json(&r.to_jcs()).unwrap();
        known.insert(r.hash().to_string(), r.licence().to_string());
    }
    let engine = engine_with(&mesh, DEFAULT_FUEL);
    let mut inner = inner;
    inner.sort_by(|a, b| (&a.2.asof, &a.1).cmp(&(&b.2.asof, &b.1)));
    let mut got = Vec::new();
    for (key, at, ver) in &inner {
        mesh.clock.set(issued_for(&ver.asof));
        match engine.get(&cell_ref_of(key), Some(at)) {
            Ok(Answer::Receipt(b)) => {
                let r = Receipt::from_json(&b).unwrap();
                known.insert(r.hash().to_string(), r.licence().to_string());
                got.push((key, ver, b, r));
            }
            other => problems.push(format!("inner {} as of {at}: {other:?}", key.name)),
        }
    }
    // An inner receipt's convention hash needs those of the inner receipts it reads: resolve in
    // dependency order.
    let mut pending: Vec<_> = got.iter().collect();
    while !pending.is_empty() {
        let before = pending.len();
        pending.retain(|(key, ver, b, r)| {
            let mut deps: Vec<String> = r.inputs().to_vec();
            for e in r.inputs().iter().filter(|e| e.starts_with("slice:")) {
                let m = datumesh_core::json::parse(&(&*mesh.store as &dyn Store).manifest(e).unwrap()).unwrap();
                deps.extend(common::array(m.get("members").unwrap()).iter().map(|h| h.as_str().unwrap().to_string()));
            }
            let ready = deps.iter().all(|h| names.contains_key(h) || !got.iter().any(|g| g.3.hash() == h));
            if ready {
                let conv = convention_hash(b, &names, &*mesh.store);
                if conv != ver.receipt_hash {
                    problems.push(format!("inner {}: receipt {conv} (convention licence CC0-1.0)", key.name));
                }
                if let Some(l) = inherited_licence(r, &known) {
                    if r.licence() != l {
                        problems.push(format!("inner {}: licence {} not the inherited {l}", key.name, r.licence()));
                    }
                }
                names.insert(r.hash().to_string(), conv);
            }
            !ready
        });
        assert!(pending.len() < before, "inner receipts read each other in a cycle");
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
            let named = convention_inputs(r.inputs(), &names, &*mesh.store);
            if named.iter().map(String::as_str).collect::<Vec<_>>() != lineage {
                problems.push(format!("lineage {:?}", r.inputs()));
            }
            if Some(r.tx()) != common::string(v, "tx") {
                problems.push(format!("tx {}", r.tx()));
            }
            if Some(r.asof()) != common::string(v, "receipt_asof") {
                problems.push(format!("receipt asof {}", r.asof()));
            }
            let conv = convention_hash(&bytes, &names, &*mesh.store);
            if conv != want_hash {
                problems.push(format!("receipt hash {conv} (convention licence CC0-1.0, SPEC section 11)"));
            }
            if let Some(l) = inherited_licence(&r, &known) {
                if r.licence() != l {
                    problems.push(format!("licence {} is not the inherited {l} (8.5)", r.licence()));
                }
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

/// The input entries of a receipt as the vectors' conventions name them (`names`: actual hash of an
/// engine receipt to convention hash): a computed receipt by its convention hash, a slice entry
/// with its manifest's members named and the members hash (SHA-256 of the sorted members array)
/// recomputed; ascending (8.1).
pub fn convention_inputs(
    inputs: &[String],
    names: &std::collections::BTreeMap<String, String>,
    store: &dyn Store,
) -> Vec<String> {
    let mut out: Vec<String> = inputs
        .iter()
        .map(|e| {
            if let Some(n) = names.get(e) {
                n.clone()
            } else if let Some(rest) = e.strip_prefix("slice:") {
                let id = rest.split(':').next().unwrap();
                let m = store.manifest(e).expect("the manifest of a slice entry is stored");
                let m = datumesh_core::json::parse(&m).unwrap();
                let mut members: Vec<String> = common::array(m.get("members").unwrap())
                    .iter()
                    .map(|h| {
                        let h = h.as_str().unwrap();
                        names.get(h).cloned().unwrap_or_else(|| h.to_string())
                    })
                    .collect();
                members.sort();
                let arr = Value::Array(members.into_iter().map(Value::String).collect());
                format!("slice:{id}:{}", datumesh_core::sha256_hex(&arr.to_jcs()))
            } else {
                e.clone()
            }
        })
        .collect();
    out.sort();
    out
}

/// The receipt hash under the convention of SPEC section 11 ("Computed receipts of `formula-eval`
/// and `excel` vectors"): the vectors' `receipt_hash` is that of a receipt with `licence`
/// `CC0-1.0`, a convention that predates licence inheritance (8.5) and is kept so that hashes stay
/// put. A mesh with inheritance (engine 0.4.1 and later) signs the inherited licence, so its
/// receipt differs in that member (and in the inputs that are computed receipts, which carry
/// theirs): rebuild the unsigned receipt with `licence` replaced and the inputs named by
/// `convention_inputs`, and hash it as 8.3 does (SHA-256 of the canonical bytes without `sig`).
pub fn convention_hash(
    receipt: &[u8],
    names: &std::collections::BTreeMap<String, String>,
    store: &dyn Store,
) -> String {
    let r = Receipt::from_json(receipt).unwrap();
    let mut v = datumesh_core::json::parse(receipt).unwrap();
    if let Value::Object(m) = &mut v {
        m.retain(|(k, _)| k != "sig");
        for (k, x) in m.iter_mut() {
            if k == "licence" {
                *x = Value::String("CC0-1.0".to_string());
            }
            if k == "inputs" {
                let ins = convention_inputs(r.inputs(), names, store);
                *x = Value::Array(ins.into_iter().map(Value::String).collect());
            }
        }
    }
    datumesh_core::sha256_hex(&v.to_jcs())
}

/// The licence 8.5 gives a computed receipt over its input receipts' licences (`known`: receipt
/// hash to licence, the world's receipts and the inner computed ones); `None` when an input is a
/// slice entry or unknown (not checked here).
fn inherited_licence(r: &Receipt, known: &std::collections::BTreeMap<String, String>) -> Option<String> {
    let mut ls = Vec::new();
    for e in r.inputs() {
        ls.push(known.get(e)?.clone());
    }
    let name = format!("{}/{}", r.cell().ns, r.cell().fn_name);
    datumesh_core::inherit_licence(ls, None, &name, r.issued(), pg::LICENCE).licence
}

/// Every vector through the engine (four threads); the failures, with `what` naming the group.
/// With `classes`, the vector's `world.classes` are put and bound in the store (datumesh-store 0.4.1).
pub fn check_kind(vectors: Vec<(String, Value)>, classes: bool, what: &str) -> usize {
    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some((path, v)) = vectors.get(i) else { break };
                if let Err(e) = check_vector(path, v, classes) {
                    failures.lock().unwrap().push(e);
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    assert!(
        failures.is_empty(),
        "{} of {} {what} vectors fail through the engine:\n{}",
        failures.len(),
        vectors.len(),
        failures.join("\n")
    );
    let _ = pg::admin_url();
    vectors.len()
}
