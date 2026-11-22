//! Every `formula` vector (SPEC 11): the canonical AST and its hash, the printed body, the
//! derived inputs, the return type and unit and asof-dependence; or the error code, and the
//! token for `E_FORMULA_FORBIDDEN`.

mod common;

use datumesh_core::json::Value;
use datumesh_formula::{define, Env};

#[test]
fn formula_vectors() {
    // The vectors of spec-0.4.2: the class vectors of spec-0.5.0 are `classes_*.rs`'s.
    let t = common::check::tally(common::old_vectors_of_kind("formula"), common::check::formula_vector);
    let (valid, invalid) = t.report("formula vectors");
    assert_eq!((valid, invalid), (195, 147));
    assert!(t.failures.is_empty());
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
