//! The class tests' shared parts (spec-0.5.0): the groups of class vectors by test commit, the
//! vectors' shared class world, and helpers to build small worlds.
#![allow(dead_code)]

use datumesh_core::json::Value;

use super::{is_class_vector, vector, vectors_of_kind};

/// The class vectors of `kind` (`formula`, `formula-eval` or `call`).
pub fn class_vectors(kind: &str) -> Vec<(String, Value)> {
    vectors_of_kind(kind).into_iter().filter(|(p, _)| is_class_vector(p)).collect()
}

/// The `formula` vectors of the dispatch, `super` and interface group (third test commit); every
/// other class `formula` vector is in the methods group (first commit).
pub const DISPATCH_FORMULA: &[&str] =
    &["invalid/formula-class-dispatch-signatures.json", "invalid/formula-class-super-without-parent.json"];

/// The `formula-eval` vectors of the methods group (first commit): `this`, defaults and the
/// computed-key fault; the others (dispatch and `super`) are in the third.
pub const METHODS_EVAL: &[&str] = &[
    "formula-eval/class-default-asof.json",
    "formula-eval/class-forward.json",
    "formula-eval/class-spot.json",
    "invalid/formula-eval-class-key-computed.json",
];

/// The class `formula` vectors of the methods group (first commit).
pub fn methods_formula() -> Vec<(String, Value)> {
    class_vectors("formula").into_iter().filter(|(p, _)| !DISPATCH_FORMULA.contains(&p.as_str())).collect()
}

/// The class `formula` vectors of the dispatch group (third commit).
pub fn dispatch_formula() -> Vec<(String, Value)> {
    class_vectors("formula").into_iter().filter(|(p, _)| DISPATCH_FORMULA.contains(&p.as_str())).collect()
}

/// The class `formula-eval` vectors of the methods group (first commit).
pub fn methods_eval() -> Vec<(String, Value)> {
    class_vectors("formula-eval").into_iter().filter(|(p, _)| METHODS_EVAL.contains(&p.as_str())).collect()
}

/// The class `formula-eval` vectors of the dispatch and `super` group (third commit).
pub fn dispatch_eval() -> Vec<(String, Value)> {
    class_vectors("formula-eval").into_iter().filter(|(p, _)| !METHODS_EVAL.contains(&p.as_str())).collect()
}

/// The environment of a definition in the vectors' class world (`formula/class-fx-rate`'s `env`:
/// the classes `fx_pair`, `fx_ndf`, `ccy_pair`, `verse`, `bond`, `swap` and `currency`, and the
/// functions of their methods and a few others), with `name`, the signature (a JSON text) and,
/// for a method, `method_of`.
pub fn class_env(name: &str, signature: &str, method_of: Option<&str>) -> datumesh_formula::Env {
    let mut v = vector("formula/class-fx-rate.json").get("env").unwrap().clone();
    if let Value::Object(m) = &mut v {
        m.retain(|(k, _)| k != "name" && k != "signature" && k != "method_of");
        m.push(("name".to_string(), Value::String(name.to_string())));
        m.push(("signature".to_string(), datumesh_core::json::parse(signature.as_bytes()).unwrap()));
        if let Some(c) = method_of {
            m.push(("method_of".to_string(), Value::String(c.to_string())));
        }
    }
    datumesh_formula::Env::from_value(&v).unwrap_or_else(|e| panic!("class_env: {e}"))
}

/// The signature of a body with no arguments that returns `returns` (a JSON text).
pub fn no_args(returns: &str) -> String {
    format!(r#"{{"args": {{}}, "order": [], "returns": {returns}}}"#)
}

/// `returns` of a read of `fx_pair`: USD per EUR.
pub const FX: &str = r#"{"type": "number", "unit": {"of": "USD", "per": "EUR"}}"#;
/// `returns` of a number of unit 1.
pub const ONE: &str = r#"{"type": "number", "unit": "1"}"#;
