//! Errors name the dialect form to use (datumesh-formula 0.6.2, CONTRACT-next-patch item 34).
//!
//! The usage errors of Experiment C (DIALECT-GAPS.md N12) keep their codes; only the message
//! gains a hint, as the existing message followed by "; write …". Each test asserts today's code
//! and key substrings of the hint (the text after the last "; write").

use datumesh_core::json;
use datumesh_formula::{canonical_ast, define, Code, Env, Error};

/// A body defined as `calc.example.net/f(d, lo, hi)` returning percent, reading two percent
/// functions; `par_yield` also has the alias `@py`.
const ENV: &str = r#"{
  "name": "calc.example.net/f",
  "signature": {
    "args": {
      "d": {"type": "date", "valid": true},
      "lo": {"type": "date", "valid": true},
      "hi": {"type": "date", "valid": true}
    },
    "order": ["d", "hi", "lo"],
    "returns": {"type": "number", "unit": "percent"}
  },
  "functions": {
    "treasury.example.org/estr": {
      "code": {"lang": "stored", "body": ""},
      "inputs": [],
      "signature": {
        "args": {"date": {"type": "date", "valid": true}},
        "order": ["date"],
        "returns": {"type": "number", "unit": "percent"}
      }
    },
    "treasury.example.org/par_yield": {
      "code": {"lang": "stored", "body": ""},
      "inputs": [],
      "signature": {
        "args": {"date": {"type": "date", "valid": true}, "tenor": {"type": "string", "valid": false}},
        "order": ["date", "tenor"],
        "returns": {"type": "number", "unit": "percent"}
      }
    }
  },
  "aliases": {"py": "treasury.example.org/par_yield"}
}"#;

fn env() -> Env {
    Env::from_value(&json::parse(ENV.as_bytes()).unwrap()).unwrap()
}

fn define_err(src: &str) -> Error {
    match define(src, &env()) {
        Ok(c) => panic!("{src}: accepted as {:?}", c.body),
        Err(e) => e,
    }
}

fn canonical_err(src: &str, env: Option<&Env>) -> Error {
    match canonical_ast(src, env) {
        Ok(_) => panic!("{src}: canonical_ast accepted it"),
        Err(e) => e,
    }
}

/// The hint: the text after the last "; write" of the message.
fn hint<'a>(src: &str, e: &'a Error) -> &'a str {
    match e.detail.rfind("; write") {
        Some(i) => &e.detail[i + "; write".len()..],
        None => panic!("{src}: {} without a \"; write …\" hint: {:?}", e.code, e.detail),
    }
}

const AGGREGATES: &[&str] = &["mean", "sum", "count", "min", "max"];

// ---------------------------------------------------------------------------------------------
// A list comprehension: E_FORMULA_PLACEMENT, naming a slice with a range and the aggregates.

const COMPREHENSIONS: &[&str] = &[
    "mean([treasury.example.org/estr(date: x) for x in d])",
    "mean([treasury.example.org/estr(x) for x in range(lo, hi)])",
    "[x for x in d]",
];

fn assert_comprehension_hint(src: &str, e: &Error) {
    assert_eq!(e.code, Code::Placement, "{src}: {e}");
    let h = hint(src, e);
    assert!(h.contains("[lo..hi]"), "{src}: the hint names no slice with a range [lo..hi]: {:?}", e.detail);
    assert!(
        AGGREGATES.iter().any(|a| h.contains(a)),
        "{src}: the hint names no aggregate ({AGGREGATES:?}): {:?}",
        e.detail
    );
}

#[test]
fn a_list_comprehension_names_a_range_slice_and_the_aggregates_in_define() {
    for src in COMPREHENSIONS {
        assert_comprehension_hint(src, &define_err(src));
    }
}

#[test]
fn a_list_comprehension_names_a_range_slice_and_the_aggregates_in_canonical_ast() {
    let env = env();
    for src in COMPREHENSIONS {
        assert_comprehension_hint(src, &canonical_err(src, Some(&env)));
        assert_comprehension_hint(src, &canonical_err(src, None));
    }
}

// ---------------------------------------------------------------------------------------------
// A bare number against a percent: E_FORMULA_UNIT_MISMATCH, naming the unit literal with the
// number as written.

#[test]
fn a_bare_number_against_a_percent_names_the_unit_literal() {
    for (src, literal) in [
        ("treasury.example.org/estr(d) - 1.9", "1.9 percent"),
        ("treasury.example.org/estr(date: d) - 1.9", "1.9 percent"),
        ("if(treasury.example.org/estr(d) > 1.926, 1 percent, 2 percent)", "1.926 percent"),
    ] {
        let e = define_err(src);
        assert_eq!(e.code, Code::UnitMismatch, "{src}: {e}");
        let h = hint(src, &e);
        assert!(h.contains(literal), "{src}: the hint does not name {literal:?}: {:?}", e.detail);
    }
}

// ---------------------------------------------------------------------------------------------
// A date written as quoted text where a date is expected: E_FORMULA_TYPE, naming the date
// literal unquoted.

#[test]
fn a_date_written_as_text_names_the_unquoted_date_literal() {
    for src in [
        "treasury.example.org/estr(date: \"2026-11-20\")",
        "treasury.example.org/estr(\"2026-11-20\")",
        "if(d > \"2026-11-20\", 1 percent, 2 percent)",
    ] {
        let e = define_err(src);
        assert_eq!(e.code, Code::Type, "{src}: {e}");
        let h = hint(src, &e);
        assert!(h.contains("2026-11-20"), "{src}: the hint does not name the date 2026-11-20: {:?}", e.detail);
        assert!(!h.contains("\"2026-11-20\""), "{src}: the hint names the date quoted: {:?}", e.detail);
    }
}

// ---------------------------------------------------------------------------------------------
// An unqualified user function name: E_FORMULA_UNKNOWN_FUNCTION, naming the qualified or
// @alias form.

#[test]
fn an_unqualified_name_known_to_the_env_names_its_qualified_or_alias_form() {
    let env = env();
    for (src, forms) in [
        ("par_yield(d, \"10y\")", &["treasury.example.org/par_yield", "@py"][..]),
        ("estr(date: d)", &["treasury.example.org/estr"][..]),
    ] {
        for e in [define_err(src), canonical_err(src, Some(&env))] {
            assert_eq!(e.code, Code::UnknownFunction, "{src}: {e}");
            let h = hint(src, &e);
            assert!(forms.iter().any(|f| h.contains(f)), "{src}: the hint names none of {forms:?}: {:?}", e.detail);
        }
    }
}

#[test]
fn an_unqualified_name_without_an_env_names_the_alias_and_qualified_forms() {
    let src = "par_yield(d, \"10y\")";
    let e = canonical_err(src, None);
    assert_eq!(e.code, Code::UnknownFunction, "{src}: {e}");
    let h = hint(src, &e);
    assert!(h.contains('@'), "{src}: the hint names no @alias form: {:?}", e.detail);
    assert!(h.contains("/par_yield"), "{src}: the hint names no qualified form of par_yield: {:?}", e.detail);
}
