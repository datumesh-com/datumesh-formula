//! The per-vector checks of the `formula`, `formula-eval`, `excel` and `call` runners (SPEC 11),
//! shared by the runners over the vectors of `spec-0.4.2` (`formula_vectors.rs`,
//! `eval_vectors.rs`, `call_vectors.rs`) and by the class tests of spec-0.5.0 (`classes_*.rs`).
#![allow(dead_code)]

use datumesh_core::json::Value;
use datumesh_core::Resolved;
use datumesh_formula::call::{parse_call, CallEnv, MapCallEnv};
use datumesh_formula::memory::run_vector;
use datumesh_formula::{define, Env, Signature};

use super::{array, boolean, integer, string};

/// What a runner counted: valid and invalid vectors run, and the failures it describes.
#[derive(Debug, Default)]
pub struct Tally {
    pub valid: usize,
    pub invalid: usize,
    pub failures: Vec<String>,
}

impl Tally {
    /// Print the counts and the failures (as the runners always have), and give the counts.
    pub fn report(&self, what: &str) -> (usize, usize) {
        eprintln!("{what}: {} valid, {} invalid, {} failures", self.valid, self.invalid, self.failures.len());
        for f in &self.failures {
            eprintln!("  {f}");
        }
        (self.valid, self.invalid)
    }
}

/// Run `check` over `vectors`.
pub fn tally(vectors: Vec<(String, Value)>, check: impl Fn(&str, &Value) -> Result<(), String>) -> Tally {
    let mut t = Tally::default();
    for (path, v) in vectors {
        if boolean(&v, "valid") == Some(true) {
            t.valid += 1;
        } else {
            t.invalid += 1;
        }
        if let Err(e) = check(&path, &v) {
            t.failures.push(e);
        }
    }
    t
}

/// A `formula` vector: the canonical AST and its hash, the printed body, the derived inputs, the
/// return type and unit and asof-dependence; or the error code, and the token for
/// `E_FORMULA_FORBIDDEN`.
pub fn formula_vector(path: &str, v: &Value) -> Result<(), String> {
    let input = string(v, "input").unwrap();
    let env = Env::from_value(v.get("env").unwrap()).map_err(|e| format!("{path}: env: {e}"))?;
    let got = define(input, &env);
    if boolean(v, "valid") == Some(true) {
        let c = got.map_err(|e| format!("{path}: {input:?}: unexpected {e}"))?;
        let ast = String::from_utf8(c.ast.to_jcs()).unwrap();
        let mut problems = Vec::new();
        if ast != string(v, "ast").unwrap() {
            problems.push(format!("ast {ast}"));
        }
        if c.ast.hash() != string(v, "ast_sha256").unwrap() {
            problems.push("ast_sha256".to_string());
        }
        if c.body != string(v, "body").unwrap() {
            problems.push(format!("body {:?}", c.body));
        }
        let inputs: Vec<&str> = array(v.get("inputs").unwrap()).iter().map(|x| x.as_str().unwrap()).collect();
        if c.inputs.iter().map(String::as_str).collect::<Vec<_>>() != inputs {
            problems.push(format!("inputs {:?}", c.inputs));
        }
        if c.returns.to_value() != *v.get("returns").unwrap() {
            problems.push(format!("returns {:?}", String::from_utf8(c.returns.to_value().to_jcs())));
        }
        if Some(c.asof_dependent) != boolean(v, "asof_dependent") {
            problems.push(format!("asof_dependent {}", c.asof_dependent));
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(format!("{path}: {input:?}: {}", problems.join("; ")))
        }
    } else {
        let want = string(v, "error").unwrap();
        match got {
            Ok(c) => Err(format!("{path}: {input:?}: accepted as {:?}, expected {want}", c.body)),
            Err(e) if e.code.as_str() != want => Err(format!("{path}: {input:?}: got {e}, expected {want}")),
            Err(e) if e.token.as_deref() != string(v, "token") => {
                Err(format!("{path}: token {:?}, expected {:?}", e.token, string(v, "token")))
            }
            Err(_) => Ok(()),
        }
    }
}

/// A `formula-eval` or `excel` vector: value, lineage, tx, receipt asof, fuel and receipt hash, or
/// the evaluation code; for `excel` also the printed body and that the value equals Excel's.
pub fn eval_vector(kind: &str, path: &str, v: &Value) -> Result<(), String> {
    let got = run_vector(v).map_err(|e| format!("{path}: {e}"))?;
    if boolean(v, "valid") == Some(true) {
        let r = got.map_err(|c| format!("{path}: unexpected {c}"))?;
        let o = &r.computed.outcome;
        let mut problems = Vec::new();
        let value = o.value.to_value();
        if value != *v.get("value").unwrap() {
            problems.push(format!("value {}", String::from_utf8(value.to_jcs()).unwrap()));
        }
        let lineage: Vec<&str> = array(v.get("lineage").unwrap()).iter().map(|x| x.as_str().unwrap()).collect();
        if o.lineage.iter().map(String::as_str).collect::<Vec<_>>() != lineage {
            problems.push(format!("lineage {:?}", o.lineage));
        }
        let tx = o.tx.clone().unwrap_or_else(|| "0001-01-01T00:00:00.000000Z".to_string());
        if Some(tx.as_str()) != string(v, "tx") {
            problems.push(format!("tx {tx}"));
        }
        if Some(o.receipt_asof.as_str()) != string(v, "receipt_asof") {
            problems.push(format!("receipt_asof {}", o.receipt_asof));
        }
        if r.fuel_used != integer(v.get("fuel_used").unwrap()) {
            problems.push(format!("fuel_used {}", r.fuel_used));
        }
        if Some(r.computed.receipt_hash.as_str()) != string(v, "receipt_hash") {
            problems.push("receipt_hash".to_string());
        }
        if kind == "excel" {
            if Some(&value) != v.get("excel_result") {
                problems.push("value differs from excel_result".to_string());
            }
            let sig = Signature::from_value(v.get("signature").unwrap()).unwrap();
            let name = string(v.get("cell").unwrap(), "fn").unwrap();
            let mut env = Env::new(name, sig);
            for (n, f) in v.get("world").unwrap().get("functions").unwrap().as_object().unwrap() {
                env.add_function(n, f.get("descriptor").unwrap()).unwrap();
            }
            match define(string(v, "formula").unwrap(), &env) {
                Ok(c) if Some(c.body.as_str()) == string(v, "body") => {}
                Ok(c) => problems.push(format!("body {:?}", c.body)),
                Err(e) => problems.push(format!("define: {e}")),
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(format!("{path}: {}", problems.join("; ")))
        }
    } else {
        let want = string(v, "error").unwrap();
        match got {
            Ok(r) => Err(format!(
                "{path}: evaluated to {}, expected {want}",
                String::from_utf8(r.computed.outcome.value.to_value().to_jcs()).unwrap()
            )),
            Err(c) if c.as_str() == want => Ok(()),
            Err(c) => Err(format!("{path}: got {c}, expected {want}")),
        }
    }
}

/// A `Resolved` as the vectors' `resolved` member: `{"call", "resolutions": [{"asof"?, "from",
/// "kind", "to"}], "aliases_tx"?}`.
pub fn resolved_value(r: &Resolved) -> Value {
    let s = |x: &str| Value::String(x.to_string());
    let resolutions = r
        .resolutions
        .iter()
        .map(|x| {
            let mut m = vec![
                ("from".to_string(), s(&x.from)),
                ("kind".to_string(), s(x.kind.as_str())),
                ("to".to_string(), s(&x.to)),
            ];
            if let Some(a) = &x.asof {
                m.push(("asof".to_string(), s(a)));
            }
            Value::Object(m)
        })
        .collect();
    let mut m = vec![("call".to_string(), s(&r.call)), ("resolutions".to_string(), Value::Array(resolutions))];
    if let Some(t) = &r.aliases_tx {
        m.push(("aliases_tx".to_string(), s(t)));
    }
    Value::Object(m)
}

/// A `call` vector: the call parsed against the vector's environment gives the cell URI it is
/// written as, or its code; since spec-0.5.0 also the `resolved` member when the vector has one
/// (and none when it has none).
pub fn call_vector(path: &str, v: &Value) -> Result<(), String> {
    let env = MapCallEnv::from_value(v.get("env").unwrap()).map_err(|e| format!("{path}: env: {e}"))?;
    let input = string(v, "input").unwrap();
    let got = parse_call(input, &env as &dyn CallEnv);
    if boolean(v, "valid") == Some(true) {
        let want = string(v, "uri").unwrap();
        match got {
            Ok(c) => {
                let mut problems = Vec::new();
                if c.uri() != want {
                    problems.push(format!("uri {}", c.uri()));
                }
                match (v.get("resolved"), &c.resolved) {
                    // 11: `resolved` is compared when the vector carries it and ignored otherwise.
                    (None, _) => {}
                    (Some(w), Some(r)) => {
                        let (a, b) = (resolved_value(r).to_jcs(), w.to_jcs());
                        if a != b {
                            problems.push(format!(
                                "resolved {} expected {}",
                                String::from_utf8(a).unwrap(),
                                String::from_utf8(b).unwrap()
                            ));
                        }
                    }
                    (Some(_), None) => problems.push("no resolved".to_string()),
                }
                if problems.is_empty() {
                    Ok(())
                } else {
                    Err(format!("{path}: {input:?}: {}", problems.join("; ")))
                }
            }
            Err(e) => Err(format!("{path}: {input:?}: {e}")),
        }
    } else {
        let want = string(v, "error").unwrap();
        match got {
            Err(e) if e.code() == want => Ok(()),
            Err(e) => Err(format!("{path}: {input:?}: {e}, expected {want}")),
            Ok(c) => Err(format!("{path}: {input:?}: parsed to {}, expected {want}", c.uri())),
        }
    }
}
