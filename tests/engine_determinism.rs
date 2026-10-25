//! Brief test 2 through the engine: generated bodies (the generator of `tests/determinism.rs`)
//! over the union of the vectors' worlds, each defined and published as a function of its own
//! in two databases holding the same world, and evaluated by a fresh engine on each. The two
//! answers are identical (receipts byte for byte), and agree with the in-memory world: the
//! same value, lineage, `tx`, receipt `asof` and receipt hash (when no computed input is
//! involved, whose `issued` differs), the same code otherwise, and the same fuel.
//!
//! The engines' work limit is out of the way: a single charge past the fuel (`ln` costs 80) is
//! past the default work limit too (16 times the fuel), which the engine reports first.
//!
//! `FORMULA_ENGINE_CASES` sets the count (default 200).
#![cfg(feature = "engine")]

mod common;
#[path = "common/pg.rs"]
mod pg;

use std::collections::BTreeMap;

use common::generate::{body, Rng, KINDS};
use common::union::{signature, union_parts, VALUES};
use datumesh_core::json::Value;
use datumesh_core::{Arg, CellRef, Receipt, Scalar};
use datumesh_engine::{Answer, EngineError};
use datumesh_formula::definition::{define_function, publish};
use datumesh_formula::eval::{Args, CellKey, Fuel};
use datumesh_formula::{define, Code, Env, Signature};
use pg::{Mesh, KEYS_FROM};

const BIND: &str = "2026-01-01T00:00:00.000000Z";

fn load(m: &Mesh, functions: &[(String, Value, String)], receipts: &[Value]) {
    for (name, d, bound) in functions {
        m.bind(name, &d.to_jcs(), bound);
    }
    for r in receipts {
        m.put_receipt_value(r);
    }
}

/// The answer as text to compare: receipt bytes, or the outcome without volatile details.
fn answer_text(a: &Result<Answer, EngineError>) -> String {
    match a {
        Ok(Answer::Receipt(b)) => String::from_utf8(b.clone()).unwrap(),
        Ok(other) => format!("{other:?}"),
        Err(EngineError::Runtime { message, .. }) => format!("runtime {message}"),
        Err(e) => format!("{e:?}"),
    }
}

fn code_of(a: &Result<Answer, EngineError>) -> Option<String> {
    match a {
        Ok(Answer::NotCovered { fault: Some(f) }) => Some(f.as_str().to_string()),
        Ok(Answer::NotCovered { fault: None }) => Some("E_FORMULA_NOT_COVERED".to_string()),
        Err(EngineError::BudgetExceeded { .. }) => Some("E_FORMULA_BUDGET".to_string()),
        Err(EngineError::Cycle { .. }) => Some("E_FORMULA_CYCLE".to_string()),
        Err(EngineError::DepthExceeded { .. }) => Some("E_FORMULA_DEPTH".to_string()),
        Err(EngineError::Runtime { message, .. }) => message
            .split(|c: char| !(c.is_ascii_uppercase() || c == '_'))
            .find(|w| w.starts_with("E_FORMULA_"))
            .map(str::to_string),
        _ => None,
    }
}

struct Case {
    i: u64,
    src: String,
    kind: &'static str,
    args: Args,
    asof: &'static str,
    limit: u64,
}

#[test]
fn generated_bodies_through_two_fresh_engines() {
    let n: u64 = std::env::var("FORMULA_ENGINE_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    let (functions, receipts, base) = union_parts();
    let hosts: Vec<String> = {
        let mut h: Vec<String> = receipts
            .iter()
            .map(|r| r.get("computed_by").unwrap().as_str().unwrap().split('#').next().unwrap().to_string())
            .collect();
        h.sort();
        h.dedup();
        h
    };
    let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
    let (ma, mb) = (Mesh::new(&hosts), Mesh::new(&hosts));
    load(&ma, &functions, &receipts);
    load(&mb, &functions, &receipts);
    let mut env_functions = BTreeMap::new();
    for (name, d, _) in &functions {
        env_functions.insert(name.clone(), d.clone());
    }
    let mut rng = Rng::new(0x5eed_0b02);
    let asofs = ["2026-06-01T00:00:00.000000Z", "2026-03-01T10:09:00.000000Z", "2026-03-01T11:00:00.000000Z"];
    let mut cases = Vec::new();
    for i in 0..n {
        let kind = *rng.pick(KINDS);
        let src = body(&mut rng, kind, 1 + (i % 3) as u32);
        let mut args = Args::new();
        for (name, ty, vals) in VALUES {
            args.insert(name.to_string(), Scalar::from_lexical(*ty, rng.pick(vals)).unwrap());
        }
        args.insert("flag".to_string(), Scalar::Boolean(rng.chance(500)));
        let asof = *rng.pick(&asofs);
        let limit = if rng.chance(100) { 1 + rng.below(200) as u64 } else { 10_000_000 };
        cases.push(Case { i, src, kind, args, asof, limit });
    }
    let (mut defined, mut receipts_n, mut answers_n, mut fuel_checked, mut hashes) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut failures = Vec::new();
    for c in &cases {
        let name = format!("calc.example.net/e{}", c.i);
        let sig = signature(c.kind);
        let mut env = Env::new(&name, Signature::from_value(&sig).unwrap());
        for (n, d) in &env_functions {
            env.add_function(n, d).unwrap();
        }
        let phase_a = define(&c.src, &env);
        let da = define_function(&name, &c.src, &sig, &BTreeMap::new(), &*ma.store, BIND);
        let db = define_function(&name, &c.src, &sig, &BTreeMap::new(), &*mb.store, BIND);
        let (da, db) = match (phase_a, da, db) {
            (Err(e), Err(x), Err(y)) => {
                if x.code() != e.code.as_str() || y.code() != e.code.as_str() {
                    failures.push(format!("{}: define {} / {} / {}", c.src, e.code, x.code(), y.code()));
                }
                continue;
            }
            (Ok(_), Ok(x), Ok(y)) => (x, y),
            (a, x, y) => {
                failures.push(format!("{}: define differs: {:?} {:?} {:?}", c.src, a.err(), x.err(), y.err()));
                continue;
            }
        };
        defined += 1;
        assert_eq!(da.bytes, db.bytes, "{}", c.src);
        publish(&*ma.store, &da, BIND).unwrap();
        publish(&*mb.store, &db, BIND).unwrap();
        // The in-memory world, first: it gives the receipt's asof, hence the vectors' issued.
        let descriptor = datumesh_core::json::parse(&da.bytes).unwrap();
        let mut world = base.clone();
        world.bind(&name, &descriptor, BIND).unwrap();
        world.fuel = Fuel::new(c.limit);
        let key = CellKey { name: name.clone(), args: c.args.clone() };
        let mem = world.compute(&key, c.asof);
        let mem_fuel = world.fuel.used;
        let inner = world.evaluated().iter().any(|(k, _, _)| *k != key);
        let issued = match &mem {
            Ok(x) if x.outcome.receipt_asof.as_str() > KEYS_FROM => x.outcome.receipt_asof.clone(),
            _ => KEYS_FROM.to_string(),
        };
        ma.clock.set(&issued);
        mb.clock.set(&issued);
        let (ns, f) = name.rsplit_once('/').unwrap();
        let cell = CellRef {
            ns: ns.into(),
            fn_name: f.into(),
            args: c.args.iter().map(|(k, s)| (k.clone(), Arg::Value(s.clone()))).collect(),
            asof: None,
        };
        let (ra, rb) = std::thread::scope(|s| {
            let ha =
                s.spawn(|| ma.builder().0.fuel(c.limit).work_limit(u64::MAX).build().unwrap().get(&cell, Some(c.asof)));
            let hb =
                s.spawn(|| mb.builder().0.fuel(c.limit).work_limit(u64::MAX).build().unwrap().get(&cell, Some(c.asof)));
            (ha.join().unwrap(), hb.join().unwrap())
        });
        if answer_text(&ra) != answer_text(&rb) {
            failures.push(format!(
                "{}: the two engines differ:\n  {}\n  {}",
                c.src,
                answer_text(&ra),
                answer_text(&rb)
            ));
            continue;
        }
        match (&mem, &ra) {
            (Ok(x), Ok(Answer::Receipt(bytes))) => {
                receipts_n += 1;
                let r = Receipt::from_json(bytes).unwrap();
                let o = &x.outcome;
                let mut p = Vec::new();
                if *r.value() != o.value {
                    p.push(format!("value {:?} / {:?}", r.value(), o.value));
                }
                if r.tx() != o.tx.as_deref().unwrap_or("0001-01-01T00:00:00.000000Z") {
                    p.push(format!("tx {} / {:?}", r.tx(), o.tx));
                }
                if r.asof() != o.receipt_asof {
                    p.push(format!("asof {} / {}", r.asof(), o.receipt_asof));
                }
                if !inner {
                    if r.inputs() != &o.lineage[..] {
                        p.push(format!("lineage {:?} / {:?}", r.inputs(), o.lineage));
                    }
                    if r.hash() != x.receipt_hash {
                        p.push("receipt hash".to_string());
                    } else {
                        hashes += 1;
                    }
                }
                if !p.is_empty() {
                    failures.push(format!("{}: {}", c.src, p.join("; ")));
                }
            }
            (Err(code), a) => {
                answers_n += 1;
                // Fuel and depth are the request's: with a computed input already stored, the
                // engine validates it instead of evaluating it.
                let comparable = !inner || !matches!(code, Code::Budget);
                if comparable && code_of(a).as_deref() != Some(code.as_str()) {
                    failures.push(format!("{}: {} / {}", c.src, code, answer_text(a)));
                }
            }
            (Ok(_), a) => failures.push(format!("{}: in memory a value, the engine {}", c.src, answer_text(a))),
        }
        // Fuel: the same body under another name, cold, at the in-memory world's fuel less one.
        if mem.is_ok() && !inner && c.i % 10 == 0 {
            fuel_checked += 1;
            let name2 = format!("calc.example.net/f{}", c.i);
            let d2 = define_function(&name2, &c.src, &sig, &BTreeMap::new(), &*ma.store, BIND).unwrap();
            publish(&*ma.store, &d2, BIND).unwrap();
            let cell2 = CellRef { fn_name: format!("f{}", c.i), ..cell.clone() };
            let short =
                ma.builder().0.fuel(mem_fuel - 1).work_limit(u64::MAX).build().unwrap().get(&cell2, Some(c.asof));
            if !matches!(short, Err(EngineError::BudgetExceeded { .. })) {
                failures.push(format!("{}: fuel {} gives {}", c.src, mem_fuel - 1, answer_text(&short)));
            }
            let exact = ma.builder().0.fuel(mem_fuel).work_limit(u64::MAX).build().unwrap().get(&cell2, Some(c.asof));
            if !matches!(exact, Ok(Answer::Receipt(_))) {
                failures.push(format!("{}: fuel {mem_fuel} gives {}", c.src, answer_text(&exact)));
            }
        }
    }
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
    eprintln!(
        "engine determinism: {n} bodies, {defined} defined, {receipts_n} receipts and {answers_n} other answers, \
         each from two fresh engines, identical; {hashes} receipt hashes equal to the in-memory world's; \
         fuel exact on {fuel_checked}"
    );
}
