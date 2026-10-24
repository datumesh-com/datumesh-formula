//! Brief test 2: generated bodies, evaluated twice over the vectors' world, give identical
//! results: value or code, lineage, manifests, tx, receipt asof, fuel and receipt hash.
//! `FORMULA_FUZZ_CASES` sets the count (default 2,000; the full run is 100,000).

mod common;

use std::collections::BTreeSet;

use common::generate::{body, Rng, KINDS};
use datumesh_core::json::Value;
use datumesh_core::{Scalar, Type};
use datumesh_formula::eval::{CellKey, Fuel};
use datumesh_formula::memory::{receipt_hash, MemoryWorld};
use datumesh_formula::{define, Env, Signature};

/// The union of the worlds of the formula-eval vectors, without their `calc.example.net/e`.
fn union_world() -> (MemoryWorld, Vec<(String, Value)>) {
    let mut w = MemoryWorld::new("2026-06-01T00:00:00.000000Z");
    let mut functions = Vec::new();
    let mut seen = BTreeSet::new();
    let mut names = BTreeSet::new();
    for (_, v) in common::vectors_of_kind("formula-eval") {
        let world = v.get("world").unwrap();
        for (n, f) in world.get("functions").unwrap().as_object().unwrap() {
            if n == "calc.example.net/e" || !names.insert(n.clone()) {
                continue;
            }
            let d = f.get("descriptor").unwrap();
            w.bind(n, d, f.get("bound").unwrap().as_str().unwrap()).unwrap();
            functions.push((n.clone(), d.clone()));
        }
        for r in common::array(world.get("receipts").unwrap()) {
            if seen.insert(receipt_hash(r)) {
                w.add_receipt(r).unwrap();
            }
        }
    }
    (w, functions)
}

fn signature(kind: &str) -> Value {
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

const VALUES: &[(&str, Type, &[&str])] = &[
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

#[test]
fn evaluating_twice_gives_identical_results() {
    let n: u64 = std::env::var("FORMULA_FUZZ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(2_000);
    let (base, functions) = union_world();
    let mut rng = Rng::new(0x5eed_0002);
    let (mut defined, mut values, mut faults) = (0u64, 0u64, 0u64);
    let asofs = ["2026-06-01T00:00:00.000000Z", "2026-03-01T10:09:00.000000Z", "2026-03-01T11:00:00.000000Z"];
    for i in 0..n {
        let kind = *rng.pick(KINDS);
        let src = body(&mut rng, kind, 1 + (i % 3) as u32);
        let sig = signature(kind);
        let mut env = Env::new("calc.example.net/e", Signature::from_value(&sig).unwrap());
        for (name, d) in &functions {
            env.add_function(name, d).unwrap();
        }
        let first = define(&src, &env);
        let second = define(&src, &env);
        let c = match (&first, &second) {
            (Ok(a), Ok(b)) => {
                assert_eq!(a.ast.to_jcs(), b.ast.to_jcs(), "{src}");
                assert_eq!(
                    (&a.body, &a.inputs, &a.returns, a.asof_dependent),
                    (&b.body, &b.inputs, &b.returns, b.asof_dependent)
                );
                a
            }
            (Err(a), Err(b)) => {
                assert_eq!(a, b, "{src}");
                continue;
            }
            _ => panic!("{src}: define differs between runs"),
        };
        defined += 1;
        let descriptor = datumesh_core::json::parse(
            format!(
                r#"{{"code":{{"body":{},"lang":"formula"}},"inputs":[{}],"signature":{}}}"#,
                String::from_utf8(Value::String(c.body.clone()).to_jcs()).unwrap(),
                c.inputs.iter().map(|s| format!("\"{s}\"")).collect::<Vec<_>>().join(","),
                String::from_utf8(sig.to_jcs()).unwrap()
            )
            .as_bytes(),
        )
        .unwrap();
        let mut args = datumesh_formula::eval::Args::new();
        for (name, ty, vals) in VALUES {
            args.insert(name.to_string(), Scalar::from_lexical(*ty, rng.pick(vals)).unwrap());
        }
        args.insert("flag".to_string(), Scalar::Boolean(rng.chance(500)));
        let asof = *rng.pick(&asofs);
        let limit = if rng.chance(100) { 1 + rng.below(200) as u64 } else { 10_000_000 };
        let run = || {
            let mut world = base.clone();
            world.bind("calc.example.net/e", &descriptor, "2026-01-01T00:00:00.000000Z").unwrap();
            world.fuel = Fuel::new(limit);
            let cell = CellKey { name: "calc.example.net/e".to_string(), args: args.clone() };
            let r = world.compute(&cell, asof);
            (r.map(|c| (c.outcome, c.receipt_hash)), world.fuel.used)
        };
        let (a, fa) = run();
        let (b, fb) = run();
        assert_eq!(a, b, "{src}");
        assert_eq!(fa, fb, "{src}");
        match a {
            Ok(_) => values += 1,
            Err(_) => faults += 1,
        }
    }
    eprintln!("determinism: {n} bodies, {defined} defined, {values} values and {faults} faults, each twice, identical");
}
