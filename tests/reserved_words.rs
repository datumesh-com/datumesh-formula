//! `this` and `super` outside a method body (SPEC 15 lexical word rules, item 2: "Reserved:
//! `this`, `super` (classes, 1.3b): `E_FORMULA_RESERVED` anywhere"; 16.3: "anywhere else they
//! are `E_FORMULA_RESERVED`"). A bare `this` or `super` is the error at its token, whatever
//! follows it. Found by the spec-0.5.0 cross-check (`generate_formula.py`, seed 7).
//!
//! Also: `examples/formula_runner.rs` echoes the input vector's `id` first in each output line
//! (`crosscheck/run.sh` compares the runners' outputs by `id`).

mod common;

use common::classes::{class_env, no_args, ONE};
use datumesh_formula::{define, Env};
use std::io::Write;
use std::process::{Command, Stdio};

const FX_SIGNATURE: &str = r#"{"args": {"base": {"type": "string", "valid": false, "key_normalisation": "upper_ascii"},
    "date": {"type": "date", "valid": true}, "quote": {"type": "string", "valid": false, "key_normalisation": "upper_ascii"}},
    "order": ["base", "quote", "date"], "returns": {"type": "number", "unit": {"of": {"arg": "quote"}, "per": {"arg": "base"}}}}"#;

fn plain_env() -> Env {
    // An environment that defines no method (`method_of` absent).
    class_env("calc.example.net/e", &no_args(ONE), None)
}

fn code(src: &str, env: &Env) -> String {
    match define(src, env) {
        Ok(c) => panic!("{src:?}: accepted as {:?}", c.body),
        Err(e) => e.code.as_str().to_string(),
    }
}

// The expected codes are the reference tool's at spec c8896ad. One left-to-right parse reports the
// first failure: a bare `this` or `super` (not followed by `.`) cannot start an operand and reports
// what it is, E_FORMULA_RESERVED, at its token, whatever follows; `f(` is rule 4's unknown function
// before its arguments are parsed; `this.x(` is a method read, checked at resolution (16.3), so the
// unterminated call is a syntax error first.
const CASES: [(&str, &str); 10] = [
    ("this this", "E_FORMULA_RESERVED"),
    ("super +", "E_FORMULA_RESERVED"),
    ("this", "E_FORMULA_RESERVED"),
    ("super", "E_FORMULA_RESERVED"),
    ("this +", "E_FORMULA_RESERVED"),
    ("1 + this", "E_FORMULA_RESERVED"),
    ("super super", "E_FORMULA_RESERVED"),
    ("f(this)", "E_FORMULA_UNKNOWN_FUNCTION"),
    ("this.x(", "E_FORMULA_SYNTAX"),
    ("this.rate(date) this", "E_FORMULA_SYNTAX"),
];

fn check(env: &Env, cases: &[(&str, &str)]) {
    let bad: Vec<String> = cases
        .iter()
        .filter_map(|(src, want)| {
            let got = code(src, env);
            (got != *want).then(|| format!("{src:?}: {got}, expected {want}"))
        })
        .collect();
    assert!(bad.is_empty(), "\n{}", bad.join("\n"));
}

#[test]
fn this_and_super_are_reserved_anywhere_outside_a_method_body() {
    check(&plain_env(), &CASES);
}

#[test]
fn a_bare_this_or_super_is_reserved_inside_a_method_body_too() {
    // A method of rates.example.org/fx_pair (base, quote, date): only `this.m(...)` and
    // `super.m(...)` are forms; a bare word is reserved there as well.
    let env = class_env("rates.example.org/fx_pair/spot", FX_SIGNATURE, Some("rates.example.org/fx_pair"));
    check(&env, &CASES);
}

#[test]
fn the_runner_echoes_the_id_first() {
    // The example is built by `cargo test`; the test executable is in `target/<profile>/deps`.
    let mut exe = std::env::current_exe().unwrap();
    exe.pop();
    exe.pop();
    exe.push("examples");
    exe.push("formula_runner");
    assert!(exe.exists(), "{} not built", exe.display());
    let env = common::vector("formula/class-fx-rate.json").get("env").unwrap().clone();
    let env = String::from_utf8(env.to_jcs()).unwrap();
    let input = format!(
        "{{\"id\":\"a-1\",\"kind\":\"formula\",\"env\":{env},\"input\":\"this this\"}}\n\
         {{\"id\":\"b-2\",\"kind\":\"decimal\",\"op\":\"add\",\"cases\":[[\"1\",\"2\"]]}}\n"
    );
    let mut child = Command::new(&exe).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    assert!(lines[0].starts_with("{\"id\":\"a-1\""), "{}", lines[0]);
    assert!(lines[1].starts_with("{\"id\":\"b-2\""), "{}", lines[1]);
}
