//! The formula runtime's own rules through the engine on PostgreSQL: a callee rebound since
//! define time is checked again (15.8), and evaluated with its own types; `E_FORMULA_FUTURE` is
//! reported before a failure of the cell's arguments in `asof(c, T)` (15.13) without making
//! the function asof-dependent; codes that are neither faults nor the engine's errors.
#![cfg(feature = "engine")]

mod common;
#[path = "common/pg.rs"]
mod pg;

use datumesh_core::{Fault, Level};
use datumesh_engine::{Answer, EngineError};
use pg::{cell_ref, instant, num, receipt, text, Mesh};

const T0: &str = "2026-01-01T00:00:00.000000Z";
const T1: &str = "2026-03-01T00:00:00.000000Z";
const A: &str = "2026-02-01T00:00:00.000000Z";
const A2: &str = "2026-04-01T00:00:00.000000Z";

fn lag_sig(unit: &str) -> String {
    format!(r#"{{"args":{{"k":{{"type":"string","valid":false}}}},"order":["k"],"returns":{{"type":"number","unit":"{unit}"}}}}"#)
}

#[test]
fn a_rebound_callee_is_checked_again_and_evaluated_with_its_types() {
    let m = Mesh::new(&["rates.example.org"]);
    m.stored("rates.example.org/lag", &lag_sig("min"), T0);
    m.record("rates.example.org/lag", &[("k", text("x"))], num("30"), T0, T0);
    let sig = r#"{"args":{"t":{"type":"instant","valid":true}},"order":["t"],"returns":"instant"}"#;
    m.define("calc.example.net/later", r#"t + rates.example.org/lag("x")"#, sig, T0);
    // From T1 the name means a function in hours.
    m.stored("rates.example.org/lag", &lag_sig("h"), T1);
    m.record("rates.example.org/lag", &[("k", text("x"))], num("2"), T1, T1);
    let t = [("t", instant("2026-01-10T00:00:00.000000Z"))];
    for _ in 0..2 {
        let (engine, _) = m.engine();
        let (b, r) = receipt(engine.get(&cell_ref("calc.example.net/later", &t), Some(A)));
        assert_eq!(r.value().lexical(), "2026-01-10T00:30:00.000000Z");
        assert_eq!(datumesh_core::verify(&b, &m.keys, Some(&engine), None).level, Some(Level::Recomputed));
        let (b, r) = receipt(engine.get(&cell_ref("calc.example.net/later", &t), Some(A2)));
        assert_eq!(r.value().lexical(), "2026-01-10T02:00:00.000000Z");
        assert_eq!(datumesh_core::verify(&b, &m.keys, Some(&engine), None).level, Some(Level::Recomputed));
    }
}

#[test]
fn a_rebinding_that_breaks_the_body_fails_only_where_it_holds() {
    let m = Mesh::new(&["rates.example.org"]);
    m.stored("rates.example.org/lag", &lag_sig("1"), T0);
    m.record("rates.example.org/lag", &[("k", text("x"))], num("3"), T0, T0);
    let sig = r#"{"args":{"k":{"type":"string","valid":false}},"order":["k"],"returns":{"type":"number","unit":"1"}}"#;
    m.define("calc.example.net/twice", "rates.example.org/lag(k) * 2", sig, T0);
    // From T1 the name means a function in EUR: the body's return unit no longer checks.
    m.stored("rates.example.org/lag", &lag_sig("EUR"), T1);
    m.record("rates.example.org/lag", &[("k", text("x"))], num("4"), T1, T1);
    let k = [("k", text("x"))];
    let (engine, _) = m.engine();
    let (_, r) = receipt(engine.get(&cell_ref("calc.example.net/twice", &k), Some(A)));
    assert_eq!(r.value().lexical(), "6");
    match engine.get(&cell_ref("calc.example.net/twice", &k), Some(A2)) {
        Err(EngineError::Runtime { message, .. }) => assert!(message.contains("E_FORMULA_RETURN_UNIT"), "{message}"),
        other => panic!("{other:?}"),
    }
    // A fresh engine, the later asof first.
    let (engine, _) = m.engine();
    assert!(matches!(engine.get(&cell_ref("calc.example.net/twice", &k), Some(A2)), Err(EngineError::Runtime { .. })));
    assert_eq!(receipt(engine.get(&cell_ref("calc.example.net/twice", &k), Some(A))).1.value().lexical(), "6");
}

#[test]
fn future_before_the_arguments_without_asof_dependence() {
    let m = Mesh::new(&["rates.example.org"]);
    let rate = r#"{"args":{"ccy":{"type":"string","valid":false}},"order":["ccy"],"returns":{"type":"number","unit":"1"}}"#;
    m.stored("rates.example.org/rate", rate, T0);
    m.record("rates.example.org/rate", &[("ccy", text("E1"))], num("1.25"), "2026-01-05T00:00:00.000000Z", "2026-01-05T00:00:00.000000Z");
    let sig = r#"{"args":{"n":{"type":"number","unit":"1","valid":false},"t":{"type":"instant","valid":false}},"order":["n","t"],"returns":{"type":"number","unit":"1"}}"#;
    let d = m.define("calc.example.net/then", r#"asof(rates.example.org/rate(concat("E", text(1 / n))), t)"#, sig, T0);
    assert!(!d.compiled.asof_dependent);
    let (engine, _) = m.engine();
    let get = |n: &str, t: &str| engine.get(&cell_ref("calc.example.net/then", &[("n", num(n)), ("t", instant(t))]), Some(A));
    // T after the asof: E_FORMULA_FUTURE, though the argument divides by zero.
    assert_eq!(get("0", A2).unwrap(), Answer::NotCovered { fault: Some(Fault::Future) });
    assert_eq!(get("1", A2).unwrap(), Answer::NotCovered { fault: Some(Fault::Future) });
    // T at or before it: the argument's fault.
    assert_eq!(get("0", "2026-01-20T00:00:00.000000Z").unwrap(), Answer::NotCovered { fault: Some(Fault::DivZero) });
    // A value: the receipt's asof is T's, not the request's (the function is not asof-dependent).
    let (_, r) = receipt(get("1", "2026-01-20T00:00:00.000000Z"));
    assert_eq!(r.value().lexical(), "1.25");
    assert_eq!(r.asof(), "2026-01-20T00:00:00.000000Z");
    // T before the name's binding: E_FORMULA_UNRESOLVED, an evaluation code that is no fault.
    match get("1", "2025-12-01T00:00:00.000000Z") {
        Err(EngineError::Runtime { message, .. }) => assert!(message.contains("E_FORMULA_UNRESOLVED"), "{message}"),
        other => panic!("{other:?}"),
    }
}
