//! `this` and `super` outside a method body (SPEC 15 lexical word rules, item 2: "Reserved:
//! `this`, `super` (classes, 1.3b): `E_FORMULA_RESERVED` anywhere"; 16.3: "anywhere else they
//! are `E_FORMULA_RESERVED`"). Outside a method body the word itself is the error, whatever
//! follows it. Found by the spec-0.5.0 cross-check (`generate_formula.py`, seed 7).
//!
//! Also: `examples/formula_runner.rs` echoes the input vector's `id` first in each output line
//! (`crosscheck/run.sh` compares the runners' outputs by `id`).

mod common;

use common::classes::{class_env, no_args, ONE};
use datumesh_formula::{define, Env};
use std::io::Write;
use std::process::{Command, Stdio};

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

#[test]
fn this_and_super_are_reserved_anywhere_outside_a_method_body() {
    let env = plain_env();
    let mut bad = Vec::new();
    for src in ["this this", "super +", "this", "super", "this +", "1 + this", "f(this)", "this.x(", "super super"] {
        let got = code(src, &env);
        if got != "E_FORMULA_RESERVED" {
            bad.push(format!("{src:?}: {got}"));
        }
    }
    assert!(bad.is_empty(), "expected E_FORMULA_RESERVED:\n{}", bad.join("\n"));
}

// Inside a method body `this this` is a syntax error (`this` is the receiver, and a second
// operand cannot follow an operand). The spec text states no separate rule for this input, so it
// is not asserted here; the crate's answer is reported in the commit notes instead.

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
