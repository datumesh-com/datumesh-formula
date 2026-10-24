//! Run vectors of kinds `formula`, `formula-eval`, `excel` and `decimal`, one JSON object per
//! line on stdin, and print each result as one JSON line, in the reference tool's output form:
//!
//! - `formula`: `{"ast", "ast_sha256", "body", "inputs", "returns", "asof_dependent"}` or
//!   `{"error", "token"?}`;
//! - `formula-eval`, `excel`: `{"value", "lineage", "tx", "receipt_asof", "fuel_used",
//!   "receipt_hash"}` or `{"error"}`;
//! - `decimal`: `{"results": [...]}`.
//!
//! `cargo run --release --example formula_runner < cases.jsonl`

use std::io::{BufRead, Write};

use datumesh_core::json::{self, Value};
use datumesh_formula::memory::run_vector;
use datumesh_formula::{decimal, define, Env};

fn s(x: &str) -> Value {
    Value::String(x.to_string())
}

fn obj(m: Vec<(&str, Value)>) -> Value {
    Value::Object(m.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn run(v: &Value) -> Value {
    match v.get("kind").and_then(Value::as_str) {
        Some("formula") => {
            let env = match v.get("env").map(Env::from_value) {
                Some(Ok(e)) => e,
                _ => return obj(vec![("error", s("malformed env"))]),
            };
            match define(v.get("input").and_then(Value::as_str).unwrap_or(""), &env) {
                Ok(c) => obj(vec![
                    ("ast", s(&String::from_utf8_lossy(&c.ast.to_jcs()))),
                    ("ast_sha256", s(&c.ast.hash())),
                    ("body", s(&c.body)),
                    ("inputs", Value::Array(c.inputs.iter().map(|i| s(i)).collect())),
                    ("returns", c.returns.to_value()),
                    ("asof_dependent", Value::Bool(c.asof_dependent)),
                ]),
                Err(e) => {
                    let mut m = vec![("error", s(e.code.as_str()))];
                    if let Some(t) = &e.token {
                        m.push(("token", s(t)));
                    }
                    obj(m)
                }
            }
        }
        Some("formula-eval") | Some("excel") => match run_vector(v) {
            Err(e) => obj(vec![("error", s(&format!("malformed: {e}")))]),
            Ok(Err(c)) => obj(vec![("error", s(c.as_str()))]),
            Ok(Ok(r)) => {
                let o = &r.computed.outcome;
                obj(vec![
                    ("value", o.value.to_value()),
                    ("lineage", Value::Array(o.lineage.iter().map(|i| s(i)).collect())),
                    ("tx", s(o.tx.as_deref().unwrap_or(datumesh_formula::eval::ZERO_INSTANT))),
                    ("receipt_asof", s(&o.receipt_asof)),
                    ("fuel_used", json::parse(r.fuel_used.to_string().as_bytes()).unwrap_or(Value::Null)),
                    ("receipt_hash", s(&r.computed.receipt_hash)),
                ])
            }
        },
        Some("decimal") => {
            let op = v.get("op").and_then(Value::as_str).unwrap_or("");
            let mut results = Vec::new();
            if let Some(Value::Array(cases)) = v.get("cases") {
                for c in cases {
                    let args: Vec<&str> = match c {
                        Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
                        _ => Vec::new(),
                    };
                    results.push(match decimal::apply(op, &args) {
                        Ok(r) => s(&r),
                        Err(code) => s(code.as_str()),
                    });
                }
            }
            obj(vec![("results", Value::Array(results))])
        }
        _ => obj(vec![("error", s("unknown kind"))]),
    }
}

fn main() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let r = match json::parse(line.as_bytes()) {
            Ok(v) => run(&v),
            Err(e) => obj(vec![("error", s(&format!("malformed line: {e:?}")))]),
        };
        let _ = out.write_all(&r.to_jcs());
        let _ = out.write_all(b"\n");
    }
}
