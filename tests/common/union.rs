//! The union of the worlds of the `formula-eval` vectors, the signature of generated bodies
//! and the argument values they are evaluated at (brief test 2).

use std::collections::BTreeSet;

use datumesh_core::json::Value;
use datumesh_core::Type;
use datumesh_formula::memory::{receipt_hash, MemoryWorld};

/// The union of the worlds of the formula-eval vectors, without their `calc.example.net/e`.
pub fn union_world() -> (MemoryWorld, Vec<(String, Value)>) {
    let mut w = MemoryWorld::new("2026-06-01T00:00:00.000000Z");
    let mut functions = Vec::new();
    let mut seen = BTreeSet::new();
    let mut names = BTreeSet::new();
    for (_, v) in super::vectors_of_kind("formula-eval") {
        let world = v.get("world").unwrap();
        for (n, f) in world.get("functions").unwrap().as_object().unwrap() {
            if n == "calc.example.net/e" || !names.insert(n.clone()) {
                continue;
            }
            let d = f.get("descriptor").unwrap();
            w.bind(n, d, f.get("bound").unwrap().as_str().unwrap()).unwrap();
            functions.push((n.clone(), d.clone()));
        }
        for r in super::array(world.get("receipts").unwrap()) {
            if seen.insert(receipt_hash(r)) {
                w.add_receipt(r).unwrap();
            }
        }
    }
    (w, functions)
}

pub fn signature(kind: &str) -> Value {
    let returns = match kind {
        "EUR" => r#"{"type":"number","unit":"EUR"}"#,
        "N1" => r#"{"type":"number","unit":"1"}"#,
        "DEGC" => r#"{"type":"number","unit":"degC"}"#,
        "B" => r#""boolean""#,
        "S" => r#""string""#,
        "D" => r#""date""#,
        _ => r#""instant""#,
    };
    let text = format!(
        r#"{{"args":{{"a":{{"type":"number","unit":"EUR","valid":false}},"b":{{"type":"number","unit":"EUR","valid":false}},
        "x":{{"type":"number","unit":"1","valid":false}},"y":{{"type":"number","unit":"1","valid":false}},
        "n":{{"type":"number","unit":"1","valid":false}},"d":{{"type":"date","valid":true}},"d2":{{"type":"date","valid":true}},
        "t":{{"type":"instant","valid":true}},"s":{{"type":"string","valid":false}},"flag":{{"type":"boolean","valid":false}},
        "t1":{{"type":"number","unit":"degC","valid":false}},"plat":{{"type":"number","unit":"deg","valid":false}}}},
        "order":["a","b","d","d2","flag","n","plat","s","t","t1","x","y"],"returns":{returns}}}"#
    );
    datumesh_core::json::parse(text.as_bytes()).unwrap()
}

pub const VALUES: &[(&str, Type, &[&str])] = &[
    ("a", Type::Number, &["-4.2", "10", "0.5", "1234.5678", "0"]),
    ("b", Type::Number, &["3", "-1", "0", "11.5"]),
    ("x", Type::Number, &["0", "1", "-1", "2", "0.5", "1000", "0.0001"]),
    ("y", Type::Number, &["0", "1", "2", "-0.25"]),
    ("n", Type::Number, &["0", "1", "2", "5", "10", "-1"]),
    ("d", Type::Date, &["2026-03-01", "2026-03-03", "2026-03-10", "2024-01-31", "2027-01-01", "2026-04-30"]),
    ("d2", Type::Date, &["2026-03-04", "2026-04-10", "2025-12-31"]),
    (
        "t",
        Type::Instant,
        &["2026-03-01T10:00:00.000000Z", "2026-03-01T10:05:00.000000Z", "2026-03-01T11:00:00.000000Z"],
    ),
    ("s", Type::String, &["kew", "ely", "bolt", "acme", "Zürich", "  a  b "]),
    ("t1", Type::Number, &["20", "-3.5"]),
    ("plat", Type::Number, &["51.5", "0"]),
];

/// The union world as functions (name, descriptor, bound) and stored receipts, one receipt per
/// cell and `tx` (the first in vector order: a store keeps one version per cell and `tx`), in
/// `tx` order per cell; and the in-memory world of exactly those.
#[allow(clippy::type_complexity)]
pub fn union_parts() -> (Vec<(String, Value, String)>, Vec<Value>, MemoryWorld) {
    let mut w = MemoryWorld::new("2026-06-01T00:00:00.000000Z");
    let mut functions = Vec::new();
    let mut names = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut receipts = Vec::new();
    for (_, v) in super::vectors_of_kind("formula-eval") {
        let world = v.get("world").unwrap();
        for (n, f) in world.get("functions").unwrap().as_object().unwrap() {
            if n == "calc.example.net/e" || !names.insert(n.clone()) {
                continue;
            }
            let d = f.get("descriptor").unwrap();
            let bound = f.get("bound").unwrap().as_str().unwrap().to_string();
            w.bind(n, d, &bound).unwrap();
            functions.push((n.clone(), d.clone(), bound));
        }
        for r in super::array(world.get("receipts").unwrap()) {
            let (id, tx) = super::cell_id_and_tx(r);
            if seen.insert((id.clone(), tx.clone())) {
                receipts.push((id, tx, r.clone()));
            }
        }
    }
    receipts.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    let receipts: Vec<Value> = receipts.into_iter().map(|(_, _, r)| r).collect();
    for r in &receipts {
        w.add_receipt(r).unwrap();
    }
    (functions, receipts, w)
}
