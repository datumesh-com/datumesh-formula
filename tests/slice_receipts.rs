//! The evaluation parts of the slice-lineage receipt vectors (SPEC 11, 15.13): where every
//! function a receipt reads has a descriptor among the vectors, evaluating the cell over its
//! offered inputs gives the recompute value, the receipt's inputs (or, for a superseded
//! input, the offered lineage) and the offered manifests.

mod common;

use std::collections::BTreeMap;

use datumesh_core::json::{self, Value};
use datumesh_formula::eval::CellKey;
use datumesh_formula::memory::MemoryWorld;

/// Every descriptor in the formula, formula-eval and excel vectors, by function hash.
fn known_descriptors() -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for kind in ["formula", "formula-eval", "excel"] {
        for (_, v) in common::vectors_of_kind(kind) {
            let fs: Vec<Value> = match kind {
                "formula" => v
                    .get("env")
                    .unwrap()
                    .get("functions")
                    .unwrap()
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(_, d)| d.clone())
                    .collect(),
                _ => v
                    .get("world")
                    .unwrap()
                    .get("functions")
                    .unwrap()
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(_, f)| f.get("descriptor").unwrap().clone())
                    .collect(),
            };
            for d in fs {
                out.insert(datumesh_core::sha256_hex(&d.to_jcs()), d);
            }
        }
    }
    out
}

fn cell_name(c: &Value) -> (String, String) {
    let ns = common::string(c, "ns").unwrap();
    let f = common::string(c, "fn").unwrap();
    (format!("{ns}/{f}"), common::string(c, "fn_hash").unwrap().to_string())
}

#[test]
fn slice_receipts_evaluate_to_their_values_and_lineage() {
    let known = known_descriptors();
    let (mut checked, mut skipped) = (0, Vec::new());
    for (path, v) in common::vectors_of_kind("receipt") {
        let Some(rc) = v.get("recompute") else { continue };
        let (Some(desc), Some(value)) = (rc.get("descriptor"), rc.get("value")) else { continue };
        if desc.get("code").and_then(|c| c.get("lang")).and_then(Value::as_str) != Some("formula") {
            continue;
        }
        if common::boolean(&v, "valid") != Some(true) || !path.contains("slice") {
            continue;
        }
        let receipt = json::parse(common::string(&v, "input").unwrap().as_bytes()).unwrap();
        let (name, _) = cell_name(receipt.get("cell").unwrap());
        let asof = common::string(&receipt, "asof").unwrap();
        let mut world = MemoryWorld::new(asof);
        world.bind(&name, desc, "0001-01-01T00:00:00.000000Z").unwrap();
        // The functions read, by the hashes their receipts and manifests pin.
        let mut pins = BTreeMap::new();
        let mut receipts = Vec::new();
        if let Some(inputs) = rc.get("inputs").and_then(Value::as_object) {
            for (_, text) in inputs {
                let r = json::parse(text.as_str().unwrap().as_bytes()).unwrap();
                let (n, h) = cell_name(r.get("cell").unwrap());
                if n != name {
                    pins.insert(n, h);
                    receipts.push(r);
                }
            }
        }
        if let Some(ms) = rc.get("manifests").and_then(Value::as_object) {
            for (_, text) in ms {
                let m = json::parse(text.as_str().unwrap().as_bytes()).unwrap();
                let s = m.get("slice").unwrap();
                let c = s.get("cell").unwrap_or(s);
                let (n, h) = cell_name(c);
                pins.insert(n, h);
            }
        }
        let missing: Vec<&String> = pins.iter().filter(|(_, h)| !known.contains_key(*h)).map(|(n, _)| n).collect();
        if !missing.is_empty() {
            skipped.push(format!("{path} (no descriptor for {missing:?})"));
            continue;
        }
        for (n, h) in &pins {
            world.bind(n, &known[h], "0001-01-01T00:00:00.000000Z").unwrap();
        }
        for r in &receipts {
            world.add_receipt(r).unwrap();
        }
        let cell = receipt.get("cell").unwrap();
        let mut args = datumesh_formula::eval::Args::new();
        for (k, a) in cell.get("args").unwrap().as_object().unwrap() {
            let t = datumesh_core::Type::from_name(common::string(a, "type").unwrap()).unwrap();
            let s = match a.get("value").unwrap() {
                Value::Bool(b) => datumesh_core::Scalar::Boolean(*b),
                x => datumesh_core::Scalar::from_lexical(t, x.as_str().unwrap()).unwrap(),
            };
            args.insert(k.clone(), s);
        }

        let c = world.compute(&CellKey { name: name.clone(), args }, asof).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(c.outcome.value.to_value(), *value, "{path}: value");
        let want: Vec<String> = match rc.get("lineage") {
            Some(l) => common::array(l).iter().map(|x| x.as_str().unwrap().to_string()).collect(),
            None => {
                common::array(receipt.get("inputs").unwrap()).iter().map(|x| x.as_str().unwrap().to_string()).collect()
            }
        };
        assert_eq!(c.outcome.lineage, want, "{path}: lineage");
        if let Some(ms) = rc.get("manifests").and_then(Value::as_object) {
            for m in &c.outcome.manifests {
                if let Some((_, text)) = ms.iter().find(|(e, _)| *e == m.entry) {
                    let offered = json::parse(text.as_str().unwrap().as_bytes()).unwrap();
                    assert_eq!(offered.to_jcs(), m.to_value().to_jcs(), "{path}: manifest {}", m.entry);
                }
            }
        }
        checked += 1;
    }
    eprintln!("slice receipts: {checked} evaluated; skipped {}: {skipped:?}", skipped.len());
    assert!(checked >= 8, "{checked}");
}
