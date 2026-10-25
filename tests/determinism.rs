//! Brief test 2: generated bodies, evaluated twice over the vectors' world, give identical
//! results: value or code, lineage, manifests, tx, receipt asof, fuel and receipt hash.
//! `FORMULA_FUZZ_CASES` sets the count (default 2,000; the full run is 100,000).

mod common;

use common::generate::{body, Rng, KINDS};
use common::union::{signature, union_world, VALUES};
use datumesh_core::json::Value;
use datumesh_core::Scalar;
use datumesh_formula::eval::{CellKey, Fuel};
use datumesh_formula::{define, Env, Signature};

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
