//! The classes round's worlds (CONTRACT-classes-round.md): a parent class `par`, a subclass `sub`
//! that inherits some methods and overrides others, a class with a lookup, and a callee that returns
//! a key. Built here from JSON, so that every function hash is the one the class holds.
#![allow(dead_code)]

use std::collections::BTreeMap;

use datumesh_core::json::{self, Value};
use datumesh_core::sha256_hex;
use datumesh_formula::call::MapCallEnv;
use datumesh_formula::{Env, Signature};

pub const PAR: &str = "calc.example.net/par";
pub const SUB: &str = "calc.example.net/sub";
pub const AT: &str = "2026-06-01T00:00:00.000000Z";
pub const NUM1: &str = r#"{"type": "number", "unit": "1"}"#;

pub fn v(text: &str) -> Value {
    json::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{text}: {e:?}"))
}

pub fn hash_of(d: &Value) -> String {
    sha256_hex(&d.to_jcs())
}

const KEY_K: &str = r#""k": {"type": "number", "unit": "1", "valid": false}"#;
const KEY_J: &str = r#""j": {"type": "string", "valid": false}"#;
const ARG_D: &str = r#""d": {"type": "date", "valid": true}"#;

fn sig(args: &[&str], order: &[&str], returns: &str) -> String {
    let order: Vec<String> = order.iter().map(|o| format!("\"{o}\"")).collect();
    format!(r#"{{"args": {{{}}}, "order": [{}], "returns": {returns}}}"#, args.join(", "), order.join(", "))
}

pub fn stored(signature: &str) -> Value {
    v(&format!(r#"{{"code": {{"body": "", "lang": "stored"}}, "inputs": [], "signature": {signature}}}"#))
}

pub fn computed(body: &str, inputs: &[&str], signature: &str) -> Value {
    let inputs: Vec<String> = inputs.iter().map(|i| format!("\"{i}\"")).collect();
    v(&format!(
        r#"{{"code": {{"body": {}, "lang": "formula"}}, "inputs": [{}], "signature": {signature}}}"#,
        String::from_utf8(Value::String(body.to_string()).to_jcs()).unwrap(),
        inputs.join(", ")
    ))
}

/// The functions of `par` and `sub`, by name, and the class descriptors.
///
/// `par` (key `k`): stored `st`, computed `co` (reads `st`), stored `ov`, computed `a` (reads `this.b`,
/// that is `par/b`), stored `b`. `sub` (keys `k`, `j`; parent `par`): inherits `st`, `co` and `a`;
/// overrides `ov` and `b`.
pub struct World {
    pub functions: BTreeMap<String, Value>,
    pub par: Value,
    pub sub: Value,
}

pub fn world() -> World {
    let par_sig = |returns: &str| sig(&[ARG_D, KEY_K], &["k", "d"], returns);
    let sub_sig = |returns: &str| sig(&[ARG_D, KEY_J, KEY_K], &["k", "j", "d"], returns);
    let key_sig = sig(&[KEY_K], &["k"], NUM1);
    let key2_sig = sig(&[KEY_J, KEY_K], &["k", "j"], NUM1);
    let mut f = BTreeMap::new();
    f.insert(format!("{PAR}/st"), stored(&par_sig(NUM1)));
    f.insert(
        format!("{PAR}/co"),
        computed("calc.example.net/par/st(d: d, k: k)\n", &["calc.example.net/par/st"], &par_sig(NUM1)),
    );
    f.insert(format!("{PAR}/ov"), stored(&par_sig(NUM1)));
    f.insert(format!("{PAR}/a"), computed("calc.example.net/par/b(k: k)\n", &["calc.example.net/par/b"], &key_sig));
    f.insert(format!("{PAR}/b"), stored(&key_sig));
    f.insert(format!("{SUB}/ov"), stored(&sub_sig(NUM1)));
    f.insert(format!("{SUB}/b"), stored(&key2_sig));
    let h = |n: &str| hash_of(&f[&format!("{PAR}/{n}")]);
    let steward =
        r#""steward": {"namespace": "calc.example.net", "process": "https://calc.example.net/classes/change"}"#;
    let par = v(&format!(
        r#"{{"keys": {{"k": {{"type": "number", "unit": "1"}}}}, "order": ["k"], {steward},
        "methods": {{"st": "{}", "co": "{}", "ov": "{}", "a": "{}", "b": "{}"}}}}"#,
        h("st"),
        h("co"),
        h("ov"),
        h("a"),
        h("b")
    ));
    let par_hash = datumesh_core::ClassDescriptor::from_value(&par).unwrap().hash();
    let sub = v(&format!(
        r#"{{"keys": {{"k": {{"type": "number", "unit": "1"}}, "j": {{"type": "string", "key_normalisation": "none"}}}},
        "order": ["k", "j"], "parent": "{par_hash}", {steward},
        "methods": {{"st": "{}", "co": "{}", "ov": "{}", "a": "{}", "b": "{}"}}}}"#,
        h("st"),
        h("co"),
        hash_of(&f[&format!("{SUB}/ov")]),
        h("a"),
        hash_of(&f[&format!("{SUB}/b")])
    ));
    World { functions: f, par, sub }
}

/// The inherited methods' names under `sub`, as spec-0.6.2 binds them (a flattened descriptor's
/// methods, inherited ones included). K5 stops binding them.
pub const LEGACY: &[&str] = &["st", "co", "a"];

impl World {
    /// The functions as K5 binds them (own methods only), plus, with `legacy`, the inherited ones
    /// under `sub` as before.
    pub fn bound(&self, legacy: bool) -> BTreeMap<String, Value> {
        let mut f = self.functions.clone();
        if legacy {
            for m in LEGACY {
                f.insert(format!("{SUB}/{m}"), self.functions[&format!("{PAR}/{m}")].clone());
            }
        }
        f
    }

    /// A definition environment for a body with this signature (a JSON text). `classes` are the
    /// classes bound: `par` and `sub`, or fewer.
    pub fn env(&self, signature: &str, legacy: bool, with_par: bool) -> Env {
        let mut env = Env::new("calc.example.net/e", Signature::from_value(&v(signature)).unwrap());
        for (n, d) in self.bound(legacy) {
            env.add_function(&n, &d).unwrap();
        }
        if with_par {
            env.classes.insert(PAR, datumesh_core::ClassDescriptor::from_value(&self.par).unwrap());
        }
        env.classes.insert(SUB, datumesh_core::ClassDescriptor::from_value(&self.sub).unwrap());
        env
    }

    /// A request environment.
    pub fn call_env(&self, legacy: bool, with_par: bool) -> MapCallEnv {
        let mut e = MapCallEnv::default();
        for (n, d) in self.bound(legacy) {
            e.functions.insert(n, datumesh_core::FunctionDescriptor::from_value(&d).unwrap());
        }
        if with_par {
            e.classes.insert(PAR.into(), datumesh_core::ClassDescriptor::from_value(&self.par).unwrap());
        }
        e.classes.insert(SUB.into(), datumesh_core::ClassDescriptor::from_value(&self.sub).unwrap());
        e.now = Some("2026-07-01T00:00:00.000000Z".into());
        e
    }
}

/// The signature of a body with one date argument `d`, returning a number of unit 1.
pub fn d_only() -> String {
    sig(&[ARG_D], &["d"], NUM1)
}

/// The signature with an argument `i` holding the key of an object of one of `classes`, and `d`.
pub fn dispatch_sig(classes: &[&str]) -> String {
    let set: Vec<String> = classes.iter().map(|c| format!("\"{c}\"")).collect();
    sig(
        &[ARG_D, &format!(r#""i": {{"type": "string", "classes": [{}], "valid": false}}"#, set.join(", "))],
        &["i", "d"],
        NUM1,
    )
}

/// The signature of a method of `sub` (keys `k`, `j`, then `d`), returning a number of unit 1.
pub fn sub_method_sig() -> String {
    sig(&[ARG_D, KEY_J, KEY_K], &["k", "j", "d"], NUM1)
}

/// A request environment of the SEC-like class `company` (one string key `cik`, no normalisation)
/// with a lookup, a method returning a key and a function with string and number arguments.
pub fn company_env() -> MapCallEnv {
    let company = "sec.example.org/company";
    let named = stored(&sig(
        &[r#""label": {"type": "string", "valid": false, "key_normalisation": "label_fold"}"#],
        &["label"],
        &format!(r#"{{"type": "string", "classes": ["{company}"]}}"#),
    ));
    let fact = stored(&sig(
        &[r#""accession": {"type": "string", "valid": false}"#, r#""cik": {"type": "string", "valid": false}"#],
        &["cik", "accession"],
        NUM1,
    ));
    let owner = stored(&sig(
        &[r#""cik": {"type": "string", "valid": false}"#],
        &["cik"],
        &format!(r#"{{"type": "string", "classes": ["{company}"]}}"#),
    ));
    let size = stored(&sig(&[r#""cik": {"type": "string", "valid": false}"#], &["cik"], NUM1));
    let filing = stored(&sig(
        &[
            r#""accession": {"type": "string", "valid": false}"#,
            r#""cik": {"type": "string", "valid": false}"#,
            r#""n": {"type": "number", "unit": "1", "valid": false}"#,
        ],
        &["cik", "accession", "n"],
        NUM1,
    ));
    let class = v(&format!(
        r#"{{"keys": {{"cik": {{"type": "string", "key_normalisation": "none"}}}}, "order": ["cik"], "lookup": "named",
        "steward": {{"namespace": "sec.example.org", "process": "https://sec.example.org/classes/change"}},
        "methods": {{"fact": "{}", "named": "{}", "owner": "{}", "size": "{}"}}}}"#,
        hash_of(&fact),
        hash_of(&named),
        hash_of(&owner),
        hash_of(&size)
    ));
    let mut e = MapCallEnv::default();
    for (n, d) in [("fact", &fact), ("named", &named), ("owner", &owner), ("size", &size)] {
        e.functions.insert(format!("{company}/{n}"), datumesh_core::FunctionDescriptor::from_value(d).unwrap());
    }
    e.functions
        .insert("sec.example.org/filing".into(), datumesh_core::FunctionDescriptor::from_value(&filing).unwrap());
    e.classes.insert(company.into(), datumesh_core::ClassDescriptor::from_value(&class).unwrap());
    e.now = Some("2026-07-01T00:00:00.000000Z".into());
    let enc = |cik: &str| format!("{company}(cik:\"{cik}\")");
    let mut labels = BTreeMap::new();
    labels.insert("apple".to_string(), enc("0000320193"));
    labels.insert("new york".to_string(), enc("0000789019"));
    labels.insert("strasse".to_string(), enc("0001018724"));
    e.lookups.insert(company.into(), labels);
    e
}
