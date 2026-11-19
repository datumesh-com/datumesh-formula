//! Every `formula-eval` and `excel` vector (SPEC 11): value, lineage, tx, receipt asof, fuel
//! and receipt hash, or the evaluation code; for `excel` also the printed body and that the
//! value equals Excel's result.

mod common;

use datumesh_core::json::Value;
use datumesh_formula::memory::run_vector;

fn check(kind: &str) -> common::check::Tally {
    // The vectors of spec-0.4.2: the class vectors of spec-0.5.0 are `classes_*.rs`'s.
    common::check::tally(common::old_vectors_of_kind(kind), |path, v| common::check::eval_vector(kind, path, v))
}

#[test]
fn formula_eval_vectors() {
    let t = check("formula-eval");
    let (valid, invalid) = t.report("formula-eval vectors");
    assert_eq!((valid, invalid), (128, 39));
    assert!(t.failures.is_empty());
}

#[test]
fn excel_vectors() {
    let t = check("excel");
    let (valid, invalid) = t.report("excel vectors");
    assert_eq!(valid + invalid, 79);
    assert!(t.failures.is_empty());
}

/// Brief test 6: fib(90) is the exact decimal.
#[test]
fn fib_90_exact() {
    let v = common::vector("formula-eval/fib-90.json");
    let r = run_vector(&v).unwrap().unwrap();
    assert_eq!(
        r.computed.outcome.value.to_value(),
        Value::Object(vec![
            ("type".to_string(), Value::String("number".to_string())),
            ("value".to_string(), Value::String("2880067194370816120".to_string())),
        ])
    );
}

/// asof(c, T): the cell's arguments are evaluated before its name is resolved as of T, so a
/// fault in an argument is reported before E_FORMULA_UNRESOLVED (T before the binding), as
/// the reference does.
#[test]
fn asof_arguments_before_resolution_as_of_t() {
    let mut v = common::vector("formula-eval/asof-read.json");
    let body = "asof(data.example.org/price(item: text(1 / 0)), 2025-06-01T00:00:00.000000Z)\n";
    let text = String::from_utf8(v.to_jcs())
        .unwrap()
        .replace("asof(data.example.org/price(item: s), 2026-03-15T00:00:00.000000Z)\\n", &body.replace('\n', "\\n"));
    v = datumesh_core::json::parse(text.as_bytes()).unwrap();
    assert_eq!(run_vector(&v).unwrap().unwrap_err().as_str(), "E_FORMULA_DIV_ZERO");
    let text = text.replace("text(1 / 0)", "s");
    let v = datumesh_core::json::parse(text.as_bytes()).unwrap();
    assert_eq!(run_vector(&v).unwrap().unwrap_err().as_str(), "E_FORMULA_UNRESOLVED");
}

/// The fixed fuel of sqrt, exp, ln, sin, cos, atan2 and haversine is charged at the call,
/// before its arguments are evaluated, as the reference does (15.12 does not say when).
#[test]
fn transcendental_fuel_is_charged_before_the_arguments() {
    for (body, unit, fuel, want) in [
        ("ln(1 / 0)", "1", 50, "E_FORMULA_BUDGET"),
        ("ln(1 / 0)", "1", 100, "E_FORMULA_DIV_ZERO"),
        ("sqrt(1 / 0)", "1", 8, "E_FORMULA_BUDGET"),
        ("exp(1 / 0)", "1", 20, "E_FORMULA_BUDGET"),
        ("sin(1 / 0)", "1", 20, "E_FORMULA_BUDGET"),
        ("cos(1 / 0)", "1", 20, "E_FORMULA_BUDGET"),
        ("atan2(1, 1 / 0)", "1", 30, "E_FORMULA_BUDGET"),
        ("haversine(1 / 0, 0, 0, 0)", "m", 100, "E_FORMULA_BUDGET"),
        ("haversine(1 / 0, 0, 0, 0)", "m", 500, "E_FORMULA_DIV_ZERO"),
    ] {
        let v = common::vector("formula-eval/abs.json");
        let text = String::from_utf8(v.to_jcs())
            .unwrap()
            .replace("\"body\":\"abs(a)\\n\"", &format!("\"body\":\"{body}\\n\""))
            .replace(
                "\"returns\":{\"type\":\"number\",\"unit\":\"EUR\"}",
                &format!("\"returns\":{{\"type\":\"number\",\"unit\":\"{unit}\"}}"),
            )
            .replacen("{", &format!("{{\"fuel\":{fuel},"), 1);
        let v = datumesh_core::json::parse(text.as_bytes()).unwrap();
        assert_eq!(run_vector(&v).unwrap().unwrap_err().as_str(), want, "{body} with fuel {fuel}");
    }
}
