//! Every `formula-eval` and `excel` vector (SPEC 11): value, lineage, tx, receipt asof, fuel
//! and receipt hash, or the evaluation code; for `excel` also the printed body and that the
//! value equals Excel's result.

mod common;

use datumesh_core::json::Value;
use datumesh_formula::memory::run_vector;
use datumesh_formula::{define, Env, Signature};

fn check(kind: &str) -> (usize, usize, Vec<String>) {
    let mut valid = 0;
    let mut invalid = 0;
    let mut failures = Vec::new();
    for (path, v) in common::vectors_of_kind(kind) {
        let got = run_vector(&v).unwrap_or_else(|e| panic!("{path}: {e}"));
        if common::boolean(&v, "valid") == Some(true) {
            valid += 1;
            let r = match got {
                Ok(r) => r,
                Err(c) => {
                    failures.push(format!("{path}: unexpected {c}"));
                    continue;
                }
            };
            let o = &r.computed.outcome;
            let mut problems = Vec::new();
            let value = o.value.to_value();
            if value != *v.get("value").unwrap() {
                problems.push(format!("value {}", String::from_utf8(value.to_jcs()).unwrap()));
            }
            let lineage: Vec<&str> =
                common::array(v.get("lineage").unwrap()).iter().map(|x| x.as_str().unwrap()).collect();
            if o.lineage.iter().map(String::as_str).collect::<Vec<_>>() != lineage {
                problems.push(format!("lineage {:?}", o.lineage));
            }
            let tx = o.tx.clone().unwrap_or_else(|| "0001-01-01T00:00:00.000000Z".to_string());
            if Some(tx.as_str()) != common::string(&v, "tx") {
                problems.push(format!("tx {tx}"));
            }
            if Some(o.receipt_asof.as_str()) != common::string(&v, "receipt_asof") {
                problems.push(format!("receipt_asof {}", o.receipt_asof));
            }
            if r.fuel_used != common::integer(v.get("fuel_used").unwrap()) {
                problems.push(format!("fuel_used {}", r.fuel_used));
            }
            if Some(r.computed.receipt_hash.as_str()) != common::string(&v, "receipt_hash") {
                problems.push("receipt_hash".to_string());
            }
            if kind == "excel" {
                if Some(&value) != v.get("excel_result") {
                    problems.push("value differs from excel_result".to_string());
                }
                let sig = Signature::from_value(v.get("signature").unwrap()).unwrap();
                let name = common::string(v.get("cell").unwrap(), "fn").unwrap();
                let mut env = Env::new(name, sig);
                for (n, f) in v.get("world").unwrap().get("functions").unwrap().as_object().unwrap() {
                    env.add_function(n, f.get("descriptor").unwrap()).unwrap();
                }
                match define(common::string(&v, "formula").unwrap(), &env) {
                    Ok(c) if Some(c.body.as_str()) == common::string(&v, "body") => {}
                    Ok(c) => problems.push(format!("body {:?}", c.body)),
                    Err(e) => problems.push(format!("define: {e}")),
                }
            }
            if !problems.is_empty() {
                failures.push(format!("{path}: {}", problems.join("; ")));
            }
        } else {
            invalid += 1;
            let want = common::string(&v, "error").unwrap();
            match got {
                Ok(r) => failures.push(format!(
                    "{path}: evaluated to {}, expected {want}",
                    String::from_utf8(r.computed.outcome.value.to_value().to_jcs()).unwrap()
                )),
                Err(c) if c.as_str() == want => {}
                Err(c) => failures.push(format!("{path}: got {c}, expected {want}")),
            }
        }
    }
    (valid, invalid, failures)
}

#[test]
fn formula_eval_vectors() {
    let (valid, invalid, failures) = check("formula-eval");
    eprintln!("formula-eval vectors: {valid} valid, {invalid} invalid, {} failures", failures.len());
    for f in &failures {
        eprintln!("  {f}");
    }
    assert_eq!((valid, invalid), (121, 34));
    assert!(failures.is_empty());
}

#[test]
fn excel_vectors() {
    let (valid, invalid, failures) = check("excel");
    eprintln!("excel vectors: {valid} valid, {invalid} invalid, {} failures", failures.len());
    for f in &failures {
        eprintln!("  {f}");
    }
    assert_eq!(valid + invalid, 78);
    assert!(failures.is_empty());
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
            .replace("\"returns\":{\"type\":\"number\",\"unit\":\"EUR\"}", &format!("\"returns\":{{\"type\":\"number\",\"unit\":\"{unit}\"}}"))
            .replacen("{", &format!("{{\"fuel\":{fuel},"), 1);
        let v = datumesh_core::json::parse(text.as_bytes()).unwrap();
        assert_eq!(run_vector(&v).unwrap().unwrap_err().as_str(), want, "{body} with fuel {fuel}");
    }
}
