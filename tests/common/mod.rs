//! Loading the vendored spec vectors (`spec-vectors/`: every vector of datumesh-spec at a468606, spec-0.4.2).
#![allow(dead_code)]

pub mod generate;
pub mod union;

use std::path::{Path, PathBuf};

use datumesh_core::json::{self, Value};

/// The vendored vector directory.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("spec-vectors")
}

/// Every vector file under `spec-vectors/` whose `kind` is `kind`, sorted by path.
pub fn vectors_of_kind(kind: &str) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    let mut stack = vec![root()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "json") {
                let bytes = std::fs::read(&path).unwrap();
                let v = json::parse(&bytes).unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
                if v.get("kind").and_then(|k| k.as_str()) == Some(kind) {
                    let rel = path.strip_prefix(root()).unwrap().display().to_string();
                    out.push((rel, v));
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The elements of a JSON array.
pub fn array(v: &Value) -> &[Value] {
    match v {
        Value::Array(a) => a,
        _ => panic!("not an array: {v:?}"),
    }
}

/// A JSON integer.
pub fn integer(v: &Value) -> u64 {
    v.as_number_text().and_then(|t| t.parse().ok()).unwrap_or_else(|| panic!("not an integer: {v:?}"))
}

/// A JSON string member.
pub fn string<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str())
}

/// A JSON boolean member.
pub fn boolean(v: &Value, key: &str) -> Option<bool> {
    match v.get(key) {
        Some(Value::Bool(b)) => Some(*b),
        _ => None,
    }
}

/// A vector by its path under `spec-vectors/`.
pub fn vector(rel: &str) -> Value {
    json::parse(&std::fs::read(root().join(rel)).unwrap()).unwrap()
}

/// The environment of the formula vectors (`formula/add`): the function `calc.example.net/f`
/// with the shared signature, and no other functions.
pub fn formula_env() -> datumesh_formula::Env {
    datumesh_formula::Env::from_value(vector("formula/add.json").get("env").unwrap()).unwrap()
}
