//! The built-ins `last_by` and `first_by` (spec-0.6.2, SPEC 15.7 and 15.13) and the unit
//! suffixes `shares` and `pure` (`units.md` 2.3): parse, check refusals, evaluation, fuel,
//! lineage and the printer. The vendored vectors are run by `formula_vectors.rs` and
//! `eval_vectors.rs`; these tests exercise the API on the same worlds and variations of them.

mod common;

use datumesh_core::json::{self, Value};
use datumesh_formula::memory::run_vector;
use datumesh_formula::{define, Env};

const LAST: &str = "last_by(data.example.org/fact(accession: *), data.example.org/filed(accession: *))";
const FIRST: &str = "first_by(data.example.org/fact(accession: *), data.example.org/filed(accession: *))";

/// The environment of `formula/last-by` plus every function of the environments of `more`.
fn env_with(more: &[&str]) -> Env {
    let mut env = Env::from_value(common::vector("formula/last-by.json").get("env").unwrap()).unwrap();
    for rel in more {
        let v = common::vector(rel);
        for (name, d) in v.get("env").unwrap().get("functions").unwrap().as_object().unwrap() {
            if env.add_function(name, d).is_err() {
                // already present (the same descriptor)
            }
        }
    }
    env
}

fn env() -> Env {
    env_with(&[
        "invalid/formula-last-by-arg-not-open.json",
        "invalid/formula-last-by-boolean.json",
        "invalid/formula-last-by-key-type.json",
        "invalid/formula-last-by-units-vary.json",
        "formula/last-by-number-by.json",
    ])
}

fn code(src: &str) -> String {
    match define(src, &env()) {
        Ok(c) => panic!("{src}: accepted as {:?}", c.body),
        Err(e) => e.code.as_str().to_string(),
    }
}

fn eval(rel: &str) -> datumesh_formula::memory::VectorResult {
    run_vector(&common::vector(rel)).unwrap().unwrap_or_else(|c| panic!("{rel}: {c}"))
}

fn value_text(r: &datumesh_formula::memory::VectorResult) -> String {
    let v = r.computed.outcome.value.to_value();
    v.get("value").unwrap().as_str().unwrap().to_string()
}

/// A vector's text with `last_by` replaced by `first_by` in the body, re-parsed.
fn first_of(rel: &str) -> Value {
    let text = String::from_utf8(common::vector(rel).to_jcs()).unwrap().replace("last_by(", "first_by(");
    json::parse(text.as_bytes()).unwrap()
}

// ---------------------------------------------------------------------------------------------
// Parse, check and print.

#[test]
fn last_by_and_first_by_parse_with_the_unit_of_the_slice() {
    for (src, name) in [(LAST, "last_by"), (FIRST, "first_by")] {
        let c = define(src, &env()).unwrap();
        assert_eq!(c.body, format!("{src}\n"));
        assert_eq!(c.inputs, vec!["data.example.org/fact", "data.example.org/filed"]);
        assert!(!c.asof_dependent);
        assert_eq!(
            c.returns.to_value(),
            Value::Object(vec![
                ("type".to_string(), Value::String("number".to_string())),
                ("unit".to_string(), Value::String("EUR".to_string())),
            ])
        );
        let ast = String::from_utf8(c.ast.to_jcs()).unwrap();
        assert!(ast.contains(&format!("\"fn\":\"{name}\"")), "{ast}");
        assert!(ast.contains("\"named\":{}"), "{ast}");
    }
}

#[test]
fn last_by_labelled_and_positional_star_print_positionally() {
    let want = format!("{LAST}\n");
    for src in [
        "last_by(slice: data.example.org/fact(accession: *), by: data.example.org/filed(accession: *))",
        "last_by(by: data.example.org/filed(accession: *), slice: data.example.org/fact(accession: *))",
        "last_by(data.example.org/fact(*), data.example.org/filed(*))",
    ] {
        assert_eq!(define(src, &env()).unwrap().body, want, "{src}");
    }
}

#[test]
fn last_by_printer_round_trip() {
    for src in [
        LAST,
        FIRST,
        &format!("{LAST} - {FIRST}"),
        "sum(last_by(data.example.org/revenue(accession: *, period: *), data.example.org/filed(accession: *)))",
    ] {
        let a = define(src, &env()).unwrap();
        let b = define(&a.body, &env()).unwrap();
        assert_eq!(a.body, b.body, "{src}");
        assert_eq!(a.ast.hash(), b.ast.hash(), "{src}");
    }
}

#[test]
fn both_built_ins_in_one_body_list_each_input_once() {
    let c = define(&format!("{LAST}-{FIRST}"), &env()).unwrap();
    assert_eq!(c.body, format!("{LAST} - {FIRST}\n"));
    assert_eq!(c.inputs, vec!["data.example.org/fact", "data.example.org/filed"]);
}

#[test]
fn a_number_by_and_a_group_key_are_accepted() {
    let c =
        define("last_by(data.example.org/fact(accession: *), data.example.org/rank(accession: *))", &env()).unwrap();
    assert_eq!(c.inputs, vec!["data.example.org/fact", "data.example.org/rank"]);
    // `period` is open in the slice and not in `by`: a slice over `period`, summed.
    define(
        "sum(last_by(data.example.org/revenue(accession: *, period: *), data.example.org/filed(accession: *)))",
        &env(),
    )
    .unwrap();
}

#[test]
fn last_by_arity_is_two() {
    assert_eq!(code("last_by(data.example.org/fact(accession: *))"), "E_FORMULA_ARITY");
    assert_eq!(code("first_by(data.example.org/fact(accession: *))"), "E_FORMULA_ARITY");
    assert_eq!(
        code("last_by(data.example.org/fact(accession: *), data.example.org/filed(accession: *), accession)"),
        "E_FORMULA_ARITY"
    );
}

#[test]
fn join_refusals_are_e_formula_join() {
    for f in ["last_by", "first_by"] {
        // `by` has an open argument that `slice` does not have.
        assert_eq!(
            code(&format!(
                "{f}(data.example.org/fact(accession: *), data.example.org/revenue(accession: *, period: *))"
            )),
            "E_FORMULA_JOIN",
            "{f}"
        );
        // `slice` binds `period`, `by` leaves it open.
        assert_eq!(
            code(&format!(
                "{f}(data.example.org/revenue(accession: *, period: 2025-12-31), data.example.org/revenue(accession: *, period: *))"
            )),
            "E_FORMULA_JOIN",
            "{f}"
        );
        // The same open name, a number key in `slice` and a string key in `by`.
        assert_eq!(
            code(&format!("{f}(data.example.org/seq(accession: *), data.example.org/filed(accession: *))")),
            "E_FORMULA_JOIN",
            "{f}"
        );
    }
}

#[test]
fn type_refusals_are_e_formula_type() {
    for f in ["last_by", "first_by"] {
        // `by` of booleans cannot be ordered.
        assert_eq!(
            code(&format!("{f}(data.example.org/fact(accession: *), data.example.org/active(accession: *))")),
            "E_FORMULA_TYPE",
            "{f}"
        );
        // `by` is a scalar.
        assert_eq!(code(&format!("{f}(data.example.org/fact(accession: *), 1 EUR)")), "E_FORMULA_TYPE", "{f}");
        // `slice` is a single-cell read.
        assert_eq!(
            code(&format!("{f}(data.example.org/fact(accession: \"a1\"), data.example.org/filed(accession: *))")),
            "E_FORMULA_TYPE",
            "{f}"
        );
    }
}

#[test]
fn by_units_that_depend_on_an_open_argument_are_unit_unknown() {
    for f in ["last_by", "first_by"] {
        assert_eq!(
            code(&format!(
                "{f}(data.example.org/held(accession: *, unit: *), data.example.org/size(accession: *, unit: *))"
            )),
            "E_FORMULA_UNIT_UNKNOWN",
            "{f}"
        );
    }
}

#[test]
fn a_group_key_leaves_a_slice_and_is_not_a_scalar_body() {
    assert_eq!(
        code("last_by(data.example.org/revenue(accession: *, period: *), data.example.org/filed(accession: *))"),
        "E_FORMULA_SLICE_VALUE"
    );
}

// ---------------------------------------------------------------------------------------------
// Evaluation.

#[test]
fn last_by_evaluates_to_the_member_with_the_greatest_by() {
    let r = eval("formula-eval/last-by.json");
    assert_eq!(value_text(&r), "110");
    // 3 nodes + (1 + 3) + (1 + 3).
    assert_eq!(r.fuel_used, 11);
    // Both slices are read: two slice entries in the lineage.
    let l = &r.computed.outcome.lineage;
    assert_eq!(l.len(), 2, "{l:?}");
    assert!(l.iter().all(|e| e.starts_with("slice:")), "{l:?}");
    assert_ne!(l[0], l[1]);
}

#[test]
fn first_by_evaluates_to_the_member_with_the_least_by() {
    let r = eval("formula-eval/first-by.json");
    assert_eq!(value_text(&r), "120");
    assert_eq!(r.computed.outcome.lineage.len(), 2);
    // The world of `last-by` read through `first_by`.
    let r = run_vector(&first_of("formula-eval/last-by.json")).unwrap().unwrap();
    assert_eq!(value_text(&r), "120");
    assert_eq!(r.fuel_used, 11);
}

#[test]
fn ties_go_to_the_later_key_for_last_by_and_the_earlier_for_first_by() {
    assert_eq!(value_text(&eval("formula-eval/last-by-tie.json")), "110"); // a2 of a1, a2
    assert_eq!(value_text(&eval("formula-eval/first-by-tie.json")), "100"); // a1 of a1, a2
}

#[test]
fn a_member_without_a_by_value_is_not_a_candidate() {
    assert_eq!(value_text(&eval("formula-eval/last-by-join-miss.json")), "100");
    assert_eq!(value_text(&eval("formula-eval/first-by-join-miss.json")), "100");
}

#[test]
fn a_number_by_is_ordered_numerically() {
    assert_eq!(value_text(&eval("formula-eval/last-by-number-by.json")), "110");
}

#[test]
fn groups_give_one_member_per_group_key() {
    assert_eq!(value_text(&eval("formula-eval/last-by-group-key.json")), "31");
    assert_eq!(value_text(&eval("formula-eval/first-by-group-key.json")), "61");
    // at(last_by(...), d): the result with a group key is a slice over `period`.
    assert_eq!(value_text(&eval("formula-eval/last-by-group-at.json")), "11");
}

#[test]
fn last_by_and_first_by_over_the_same_slices_have_two_lineage_entries() {
    let r = eval("formula-eval/last-by-both.json");
    assert_eq!(value_text(&r), "-10");
    assert_eq!(r.computed.outcome.lineage.len(), 2);
}

#[test]
fn no_candidate_is_e_formula_absent() {
    for rel in [
        "invalid/formula-eval-last-by-all-missing.json",
        "invalid/formula-eval-first-by-all-missing.json",
        "invalid/formula-eval-last-by-empty-by.json",
    ] {
        let c = run_vector(&common::vector(rel)).unwrap().unwrap_err();
        assert_eq!(c.as_str(), "E_FORMULA_ABSENT", "{rel}");
    }
}

#[test]
fn fuel_of_last_by_is_charged_and_exhausts() {
    let with_fuel = |n: u64| {
        let text = String::from_utf8(common::vector("formula-eval/last-by.json").to_jcs()).unwrap().replacen(
            '{',
            &format!("{{\"fuel\":{n},"),
            1,
        );
        json::parse(text.as_bytes()).unwrap()
    };
    assert_eq!(run_vector(&with_fuel(11)).unwrap().unwrap().fuel_used, 11);
    assert_eq!(run_vector(&with_fuel(10)).unwrap().unwrap_err().as_str(), "E_FORMULA_BUDGET");
}

// ---------------------------------------------------------------------------------------------
// Unit suffixes (`units.md` 2.3).

/// The environment of `formula/add` with a bare `number` result: the add vector declares EUR, and a
/// body in another unit is then `E_FORMULA_RETURN_UNIT` (15.8), which is not what these tests probe.
fn bare_env() -> datumesh_formula::Env {
    let Value::Object(members) = common::vector("formula/add.json").get("env").unwrap().clone() else {
        panic!("env is an object")
    };
    let members = members
        .into_iter()
        .map(|(k, v)| {
            if k != "signature" {
                return (k, v);
            }
            let Value::Object(sig) = v else { panic!("signature is an object") };
            let sig = sig
                .into_iter()
                .map(|(k, v)| {
                    if k == "returns" {
                        (k, Value::Object(vec![("type".to_string(), Value::String("number".to_string()))]))
                    } else {
                        (k, v)
                    }
                })
                .collect();
            (k, Value::Object(sig))
        })
        .collect();
    datumesh_formula::Env::from_value(&Value::Object(members)).unwrap()
}

fn returns_unit(src: &str) -> Value {
    define(src, &bare_env()).unwrap_or_else(|e| panic!("{src}: {e}")).returns.to_value()
}

#[test]
fn shares_and_pure_are_unit_suffixes() {
    for (src, unit) in [
        ("5 shares", "shares"),
        ("5 pure", "pure"),
        ("1 shares + 2 shares", "shares"),
        ("2 pure * 3 pure", "pure pure"),
    ] {
        let u = returns_unit(src);
        assert_eq!(u.get("type").and_then(|t| t.as_str()), Some("number"), "{src}");
        if !src.contains('*') {
            assert_eq!(u.get("unit").and_then(|t| t.as_str()), Some(unit), "{src}");
        }
    }
    let c = define("5 shares", &bare_env()).unwrap();
    assert_eq!(c.body, "5 shares\n");
    let c = define("5 pure", &bare_env()).unwrap();
    assert_eq!(c.body, "5 pure\n");
}

#[test]
fn shares_and_pure_are_distinct_units() {
    let env = bare_env();
    for src in ["5 shares + 5 pure", "5 pure + 5 shares", "5 shares + 1 USD", "5 pure + 1 EUR"] {
        assert_eq!(define(src, &env).unwrap_err().code.as_str(), "E_FORMULA_UNIT_MISMATCH", "{src}");
    }
    // The spelling is exact: `Shares` and `PURE` are not units.
    for src in ["5 Shares", "5 PURE"] {
        assert!(define(src, &env).is_err(), "{src}");
    }
}
