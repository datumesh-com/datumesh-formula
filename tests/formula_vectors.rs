//! Every `formula` vector (SPEC 11): the canonical AST and its hash, the printed body, the
//! derived inputs, the return type and unit and asof-dependence; or the error code, and the
//! token for `E_FORMULA_FORBIDDEN`.

mod common;

use datumesh_core::json::Value;
use datumesh_formula::{define, Env};

#[test]
fn formula_vectors() {
    let mut valid = 0;
    let mut invalid = 0;
    let mut failures = Vec::new();
    for (path, v) in common::vectors_of_kind("formula") {
        let input = common::string(&v, "input").unwrap();
        let env = Env::from_value(v.get("env").unwrap()).unwrap_or_else(|e| panic!("{path}: env: {e}"));
        let got = define(input, &env);
        if common::boolean(&v, "valid") == Some(true) {
            valid += 1;
            let c = match got {
                Ok(c) => c,
                Err(e) => {
                    failures.push(format!("{path}: {input:?}: unexpected {e}"));
                    continue;
                }
            };
            let ast = String::from_utf8(c.ast.to_jcs()).unwrap();
            let mut problems = Vec::new();
            if ast != common::string(&v, "ast").unwrap() {
                problems.push(format!("ast {ast}"));
            }
            if c.ast.hash() != common::string(&v, "ast_sha256").unwrap() {
                problems.push("ast_sha256".to_string());
            }
            if c.body != common::string(&v, "body").unwrap() {
                problems.push(format!("body {:?}", c.body));
            }
            let inputs: Vec<&str> =
                common::array(v.get("inputs").unwrap()).iter().map(|x| x.as_str().unwrap()).collect();
            if c.inputs.iter().map(String::as_str).collect::<Vec<_>>() != inputs {
                problems.push(format!("inputs {:?}", c.inputs));
            }
            if c.returns.to_value() != *v.get("returns").unwrap() {
                problems.push(format!("returns {:?}", String::from_utf8(c.returns.to_value().to_jcs())));
            }
            if Some(c.asof_dependent) != common::boolean(&v, "asof_dependent") {
                problems.push(format!("asof_dependent {}", c.asof_dependent));
            }
            if !problems.is_empty() {
                failures.push(format!("{path}: {input:?}: {}", problems.join("; ")));
            }
        } else {
            invalid += 1;
            let want = common::string(&v, "error").unwrap();
            match got {
                Ok(c) => failures.push(format!("{path}: {input:?}: accepted as {:?}, expected {want}", c.body)),
                Err(e) => {
                    if e.code.as_str() != want {
                        failures.push(format!("{path}: {input:?}: got {e}, expected {want}"));
                    } else if e.token.as_deref() != common::string(&v, "token") {
                        failures.push(format!(
                            "{path}: token {:?}, expected {:?}",
                            e.token,
                            common::string(&v, "token")
                        ));
                    }
                }
            }
        }
    }
    eprintln!("formula vectors: {valid} valid, {invalid} invalid, {} failures", failures.len());
    for f in &failures {
        eprintln!("  {f}");
    }
    assert_eq!((valid, invalid), (172, 123));
    assert!(failures.is_empty());
}

/// Brief test 3: a body whose callee would have to be computed fails at parse.
#[test]
fn dynamic_names_fail_at_parse() {
    let env = common::formula_env();
    for src in [
        "(\"rates.example.org/\" & \"rate\")(quote, tenor)",
        "(a)(1)",
        "\"a.org/f\"(x)",
        "rates.example.org/rate(currency: quote, tenor: tenor)(1)",
        "abs(a)(1)",
    ] {
        let e = define(src, &env).unwrap_err();
        assert_eq!(e.code.as_str(), "E_FORMULA_DYNAMIC_NAME", "{src}: {e}");
    }
}

/// Brief test 4: every forbidden function name, in any case, fails with E_FORMULA_FORBIDDEN
/// naming the token as written; without `(` it is an ordinary (here unknown) name.
#[test]
fn purity_forbidden_tokens() {
    let env = common::formula_env();
    let names = [
        "now",
        "today",
        "rand",
        "randbetween",
        "randarray",
        "indirect",
        "offset",
        "info",
        "cell",
        "clock",
        "time",
        "random",
        "sleep",
        "fetch",
        "http",
        "eval",
    ];
    for name in names {
        for written in [name.to_string(), name.to_uppercase(), capitalised(name)] {
            for src in [format!("{written}()"), format!("a + {written}(a)"), format!("sum(a, {written} (b))")] {
                let e = define(&src, &env).unwrap_err();
                assert_eq!(e.code.as_str(), "E_FORMULA_FORBIDDEN", "{src}: {e}");
                assert_eq!(e.token.as_deref(), Some(written.as_str()), "{src}");
            }
        }
        let e = define(&format!("{name} + 1"), &env).unwrap_err();
        assert_ne!(e.code.as_str(), "E_FORMULA_FORBIDDEN", "{name} without (");
    }
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Brief test 5: ten unit mismatches fail at define; an FX forward's unit resolves to
/// quote per base.
#[test]
fn units() {
    let env = common::formula_env();
    for (src, code) in [
        ("a + c", "E_FORMULA_UNIT_MISMATCH"),
        ("a > c", "E_FORMULA_UNIT_MISMATCH"),
        ("convert(a, \"m\")", "E_FORMULA_UNIT_MISMATCH"),
        ("1 + r", "E_FORMULA_UNIT_MISMATCH"),
        ("sin(dist)", "E_FORMULA_UNIT_MISMATCH"),
        ("a + 1 USD", "E_FORMULA_UNIT_MISMATCH"),
        ("if flag then a else c", "E_FORMULA_UNIT_MISMATCH"),
        ("mean(a, c)", "E_FORMULA_UNIT_MISMATCH"),
        ("a - secs * 1 EUR", "E_FORMULA_UNIT_MISMATCH"),
        ("otherwise(a, c)", "E_FORMULA_UNIT_MISMATCH"),
        ("t1 + t2", "E_FORMULA_UNIT_OFFSET"),
        ("t1 * 2", "E_FORMULA_UNIT_OFFSET"),
        ("a ^ n", "E_FORMULA_EXPONENT"),
        ("sqrt(dist)", "E_FORMULA_EXPONENT"),
    ] {
        let e = define(src, &env).unwrap_err();
        assert_eq!(e.code.as_str(), code, "{src}: {e}");
    }
    // FX forward: spot times the ratio of one-month growth factors; its unit is quote per base.
    let fx = common::vector("formula-eval/fx-forward.json");
    let world = fx.get("world").unwrap().get("functions").unwrap();
    let mut env = Env::from_value(common::vector("formula/read-positional.json").get("env").unwrap()).unwrap();
    for name in ["rates.example.org/fx_rate", "rates.example.org/rate"] {
        env.add_function(name, world.get(name).unwrap().get("descriptor").unwrap()).unwrap();
    }
    let src = "rates.example.org/fx_rate(base, quote, d) * (1 + rates.example.org/rate(quote, \"1M\")) / (1 + rates.example.org/rate(base, \"1M\"))";
    let c = define(src, &env).unwrap();
    assert_eq!(
        c.returns.to_value(),
        Value::Object(vec![
            ("type".to_string(), Value::String("number".to_string())),
            ("unit".to_string(), Value::String("arg:base^-1 arg:quote".to_string())),
        ])
    );
}
