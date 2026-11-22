//! The classes round (CONTRACT-classes-round.md), the formula crate's parts of K2, K5, K6, K7, K8,
//! K10, K11 and K13: failing tests first (one behaviour pin among them, K6).
//!
//! What cannot be written against today's API is in `classes_round_api.rs` (`define_method` over a
//! draft, `fold_label`, the chain hook), which does not compile until that API exists.
//!
//! A test that passes today is marked "PASSES TODAY" in its doc comment: it is a pin of behaviour
//! that must survive the change, or a case K5 decides by the absence of a name.

mod common;

use std::cell::RefCell;
use std::collections::BTreeMap;

use common::round::*;
use datumesh_core::json::Value;
use datumesh_core::{ClassDescriptor, FunctionDescriptor};
use datumesh_formula::call::{parse_call, Call, CallEnv, CallError, MapCallEnv};
use datumesh_formula::definition::{define_function, define_method};
use datumesh_formula::memory::{receipt_hash, run_vector};
use datumesh_formula::{define, Compiled, Env, Signature};

fn body_ok(src: &str, env: &Env) -> Compiled {
    define(src, env).unwrap_or_else(|e| panic!("{src}: {e}"))
}

fn body_code(src: &str, env: &Env) -> String {
    match define(src, env) {
        Ok(c) => panic!("{src}: accepted as {:?}", c.body),
        Err(e) => e.code.as_str().to_string(),
    }
}

fn call(src: &str, env: &dyn CallEnv) -> Result<Call, String> {
    parse_call(src, env).map_err(|e| match e {
        CallError::Code(c, _) => c.to_string(),
        other => other.to_string(),
    })
}

fn call_ok(src: &str, env: &dyn CallEnv) -> Call {
    call(src, env).unwrap_or_else(|e| panic!("{src}: {e}"))
}

fn resolved(c: &Call) -> Vec<(String, String, String)> {
    c.resolved
        .as_ref()
        .map(|r| r.resolutions.iter().map(|x| (x.kind.as_str().to_string(), x.from.clone(), x.to.clone())).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// The eval worlds.

const ZERO: &str = "2026-01-01T00:00:00.000000Z";

fn text(v: &Value) -> String {
    String::from_utf8(v.to_jcs()).unwrap()
}

/// A stored value's receipt: `args` is the JSON text of the cell's arguments, `valid` of its `valid`.
fn receipt(ns: &str, name: &str, fn_hash: &str, args: &str, valid: &str, ty: &str, value: &str) -> Value {
    v(&format!(
        r#"{{"asof": "2026-03-16T16:00:00.000000Z", "cell": {{"args": {args}, "fn": "{name}", "fn_hash": "{fn_hash}", "ns": "{ns}"}},
        "computed_by": "calc.example.net#0123456789abcdef", "inputs": [], "issued": "2026-03-16T16:00:00.000000Z",
        "licence": "CC0-1.0", "proto": "datumesh/1", "publisher": "{ns}", "summary": "A recorded value.",
        "tx": "2026-03-16T16:00:00.000000Z", "type": "{ty}", "valid": {valid}, "value": {},
        "sig": "{}"}}"#,
        text(&Value::String(value.to_string())),
        "0".repeat(128)
    ))
}

fn vector(
    functions: &BTreeMap<String, Value>,
    classes: &[(&str, &Value)],
    receipts: Vec<Value>,
    cell_fn: &str,
    cell_args: &str,
) -> Value {
    let fs: Vec<String> = functions
        .iter()
        .map(|(n, d)| format!("\"{n}\": {{\"bound\": \"{ZERO}\", \"descriptor\": {}}}", text(d)))
        .collect();
    let cs: Vec<String> =
        classes.iter().map(|(n, d)| format!("\"{n}\": {{\"bound\": \"{ZERO}\", \"descriptor\": {}}}", text(d))).collect();
    let rs: Vec<String> = receipts.iter().map(text).collect();
    v(&format!(
        r#"{{"asof": "{AT}", "kind": "formula-eval", "cell": {{"args": {cell_args}, "fn": "{cell_fn}"}},
        "world": {{"functions": {{{}}}, "classes": {{{}}}, "receipts": [{}]}}}}"#,
        fs.join(", "),
        cs.join(", "),
        rs.join(", ")
    ))
}

fn k_arg(k: &str) -> String {
    format!(r#"{{"type": "number", "value": "{k}"}}"#)
}

// ---------------------------------------------------------------------------------------------
// K2. A callee's returned classes are loaded at define.

/// The world of K2: `ref(k)` returns the key of a `par` object, `par/st` reads it. `par` is named
/// nowhere in the body `ref(k: k).st(d)`, only in `ref`'s `returns.classes`.
fn k2_callees(with_class: bool) -> Env {
    let w = world();
    let mut env = Env::new("calc.example.net/none", Signature::from_value(&v(&d_only())).unwrap());
    let key_sig = r#"{"args": {"k": {"type": "number", "unit": "1", "valid": false}}, "order": ["k"],
        "returns": {"type": "string", "classes": ["calc.example.net/par"]}}"#;
    env.add_function("calc.example.net/ref", &stored(key_sig)).unwrap();
    env.add_function("calc.example.net/par/st", &w.functions[&format!("{PAR}/st")]).unwrap();
    if with_class {
        env.classes.insert(PAR, ClassDescriptor::from_value(&w.par).unwrap());
    }
    env
}

fn k2_signature() -> Value {
    v(r#"{"args": {"d": {"type": "date", "valid": true}, "k": {"type": "number", "unit": "1", "valid": false}},
        "order": ["k", "d"], "returns": {"type": "number", "unit": "1"}}"#)
}

/// A dispatch on a callee's returned key defines, though its class is named nowhere else: `define_in`
/// loads the callee's `returns.classes` when it resolves the callee.
#[test]
fn k2_a_dispatch_on_a_returned_key_defines() {
    let d = define_function(
        "calc.example.net/two",
        "calc.example.net/ref(k: k).st(d)",
        &k2_signature(),
        &BTreeMap::new(),
        &k2_callees(true),
        AT,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d.compiled.inputs, ["calc.example.net/par/st", "calc.example.net/ref"]);
    assert_eq!(d.callees.keys().collect::<Vec<_>>(), ["calc.example.net/par/st", "calc.example.net/ref"]);
}

/// The same definition evaluates: the descriptor it produced, bound in a world with the callee's
/// receipt (a key) and the dispatched method's receipt, reads the method's value.
#[test]
fn k2_the_defined_dispatch_evaluates() {
    let w = world();
    let d = define_function(
        "calc.example.net/two",
        "calc.example.net/ref(k: k).st(d)",
        &k2_signature(),
        &BTreeMap::new(),
        &k2_callees(true),
        AT,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let mut fs = BTreeMap::new();
    let key_sig = r#"{"args": {"k": {"type": "number", "unit": "1", "valid": false}}, "order": ["k"],
        "returns": {"type": "string", "classes": ["calc.example.net/par"]}}"#;
    let refd = stored(key_sig);
    fs.insert("calc.example.net/ref".to_string(), refd.clone());
    fs.insert(format!("{PAR}/st"), w.functions[&format!("{PAR}/st")].clone());
    fs.insert("calc.example.net/two".to_string(), datumesh_core::json::parse(&d.bytes).unwrap());
    let st_hash = hash_of(&w.functions[&format!("{PAR}/st")]);
    let receipts = vec![
        receipt("calc.example.net", "ref", &hash_of(&refd), &format!(r#"{{"k": {}}}"#, k_arg("1")), "{}", "string", "calc.example.net/par(k:1)"),
        receipt(
            PAR,
            "st",
            &st_hash,
            &format!(r#"{{"d": {{"type": "date", "value": "2026-03-16"}}, "k": {}}}"#, k_arg("1")),
            r#"{"d": {"type": "date", "value": "2026-03-16"}}"#,
            "number",
            "5",
        ),
    ];
    let vec = vector(
        &fs,
        &[(PAR, &w.par)],
        receipts,
        "calc.example.net/two",
        &format!(r#"{{"d": {{"type": "date", "value": "2026-03-16"}}, "k": {}}}"#, k_arg("1")),
    );
    let r = run_vector(&vec).unwrap().unwrap_or_else(|c| panic!("{c}"));
    assert_eq!(r.computed.outcome.value.lexical(), "5");
}

/// A returned class that is bound nowhere is still `E_FORMULA_UNRESOLVED`. (PASSES TODAY: the class is
/// not loaded at all today, which also gives that code; it must stay the code once the class is loaded.)
#[test]
fn k2_an_unbound_returned_class_is_unresolved() {
    let e = define_function(
        "calc.example.net/two",
        "calc.example.net/ref(k: k).st(d)",
        &k2_signature(),
        &BTreeMap::new(),
        &k2_callees(false),
        AT,
    )
    .unwrap_err();
    assert_eq!(e.code(), "E_FORMULA_UNRESOLVED", "{e}");
}

// ---------------------------------------------------------------------------------------------
// K5. An inherited method reads the cells of the class that defines it.

const SUB_OBJ: &str = "calc.example.net/sub(1, \"x\")";
const PAR_OBJ: &str = "calc.example.net/par(1)";

/// In a body, an inherited method of a subclass, stored or computed, is the read of `D/m`, the class
/// that defines it. The binding of spec-0.6.2 had `sub/st` and `sub/co` as well; K5 binds `sub`'s own
/// methods only, so the world here has no `sub/st`.
#[test]
fn k5_body_an_inherited_method_reads_the_defining_class() {
    let env = world().env(&d_only(), false, true);
    for (m, d) in [("st", "calc.example.net/par/st"), ("co", "calc.example.net/par/co")] {
        let c = body_ok(&format!("{SUB_OBJ}.{m}(d)"), &env);
        assert_eq!(c.inputs, [d], "{m}");
        assert!(c.body.starts_with(&format!("{d}(")), "{m}: {}", c.body);
        // The parent's object reads the same function.
        let p = body_ok(&format!("{PAR_OBJ}.{m}(d)"), &env);
        assert_eq!(p.inputs, [d], "{m}");
    }
}

/// Binding is not what decides: with the spec-0.6.2 binding (`sub/st` bound too) the read is `par/st`.
#[test]
fn k5_body_the_old_binding_of_the_inherited_name_is_not_read() {
    let env = world().env(&d_only(), true, true);
    for (m, d) in [("st", "calc.example.net/par/st"), ("co", "calc.example.net/par/co"), ("a", "calc.example.net/par/a")] {
        let src = if m == "a" { format!("{SUB_OBJ}.a()") } else { format!("{SUB_OBJ}.{m}(d)") };
        assert_eq!(body_ok(&src, &env).inputs, [d], "{m}");
    }
}

/// An override stays the subclass's: `sub/ov`, and the parent's object reads `par/ov`. (PASSES TODAY.)
#[test]
fn k5_body_an_override_stays_the_subclass_method() {
    let env = world().env(&d_only(), false, true);
    assert_eq!(body_ok(&format!("{SUB_OBJ}.ov(d)"), &env).inputs, ["calc.example.net/sub/ov"]);
    assert_eq!(body_ok(&format!("{PAR_OBJ}.ov(d)"), &env).inputs, ["calc.example.net/par/ov"]);
    let legacy = world().env(&d_only(), true, true);
    assert_eq!(body_ok(&format!("{SUB_OBJ}.ov(d)"), &legacy).inputs, ["calc.example.net/sub/ov"]);
}

/// The plain read of `sub/st`, an inherited method, is `E_FORMULA_UNRESOLVED`: its name is not bound.
/// (PASSES TODAY: the world has no `sub/st`, and an unbound name is unresolved. It pins the code.)
#[test]
fn k5_body_the_plain_read_of_an_inherited_method_is_unresolved() {
    let env = world().env(&d_only(), false, true);
    for m in ["st", "co"] {
        let src = format!("calc.example.net/sub/{m}(1, \"x\", d)");
        assert_eq!(body_code(&src, &env), "E_FORMULA_UNRESOLVED", "{m}");
    }
}

/// D's hash is bound to no name: the read is `E_FORMULA_UNRESOLVED`, even where `sub/st` is bound (as
/// spec-0.6.2 bound it), since the rule names D by the class name bound to the parent's hash.
#[test]
fn k5_body_a_defining_class_with_no_name_is_unresolved() {
    let env = world().env(&d_only(), true, false);
    for m in ["st", "co"] {
        assert_eq!(body_code(&format!("{SUB_OBJ}.{m}(d)"), &env), "E_FORMULA_UNRESOLVED", "{m}");
    }
    // An override needs no D.
    assert_eq!(body_ok(&format!("{SUB_OBJ}.ov(d)"), &env).inputs, ["calc.example.net/sub/ov"]);
}

/// Through `this`, in a method of `sub`: inherited methods are `par/m`, an override is `sub/m`.
#[test]
fn k5_this_an_inherited_method_reads_the_defining_class() {
    let w = world();
    let env = w.env(&d_only(), false, true);
    let sig = v(&sub_method_sig());
    let def = |src: &str| define_method(SUB, "calc.example.net/sub/m", src, &sig, &BTreeMap::new(), &env, AT);
    let d = def("this.st(d) + this.co(d)").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d.compiled.inputs, ["calc.example.net/par/co", "calc.example.net/par/st"]);
    let d = def("this.ov(d)").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d.compiled.inputs, ["calc.example.net/sub/ov"]);
    // D unbound.
    let env = w.env(&d_only(), true, false);
    let e = define_method(SUB, "calc.example.net/sub/m", "this.st(d)", &sig, &BTreeMap::new(), &env, AT).unwrap_err();
    assert_eq!(e.code(), "E_FORMULA_UNRESOLVED", "{e}");
}

/// In a dispatch, each class's method is the function of the class that defines it.
#[test]
fn k5_dispatch_an_inherited_method_reads_the_defining_class() {
    let env = world().env(&dispatch_sig(&[SUB]), false, true);
    assert_eq!(body_ok("i.st(d)", &env).inputs, ["calc.example.net/par/st"]);
    assert_eq!(body_ok("i.co(d)", &env).inputs, ["calc.example.net/par/co"]);
    // An override in the set is its own.
    assert_eq!(body_ok("i.ov(d)", &env).inputs, ["calc.example.net/sub/ov"]);
    // Over both classes: the inherited method is one function, an overridden one is two.
    let both = world().env(&dispatch_sig(&[PAR, SUB]), false, true);
    assert_eq!(body_ok("i.st(d)", &both).inputs, ["calc.example.net/par/st"]);
    assert_eq!(body_ok("i.ov(d)", &both).inputs, ["calc.example.net/par/ov", "calc.example.net/sub/ov"]);
    // D unbound.
    let none = world().env(&dispatch_sig(&[SUB]), true, false);
    assert_eq!(body_code("i.st(d)", &none), "E_FORMULA_UNRESOLVED");
}

/// In a request, a method call on a subclass's object is the cell of `D/m`, and `resolved` says
/// `method` to `D/m`; an override is `S/m`.
#[test]
fn k5_request_an_inherited_method_is_the_defining_classes_cell() {
    let env = world().call_env(false, true);
    let c = call_ok("calc.example.net/sub(1, x).st(2026-03-16)", &env);
    assert_eq!(c.uri(), "cell://calc.example.net/par/st?d=2026-03-16&k=1");
    assert_eq!(
        resolved(&c).iter().find(|r| r.0 == "method").unwrap(),
        &("method".to_string(), "calc.example.net/sub(...).st".to_string(), "calc.example.net/par/st".to_string())
    );
    let c = call_ok("calc.example.net/sub(1, x).co(2026-03-16)", &env);
    assert_eq!(c.uri(), "cell://calc.example.net/par/co?d=2026-03-16&k=1");
    assert_eq!(resolved(&c).last().unwrap().2, "calc.example.net/par/co");
    // Only the keys D takes are passed on.
    assert_eq!(call_ok("calc.example.net/sub(1, x).a()", &env).uri(), "cell://calc.example.net/par/a?k=1");
    // An override is S's own.
    let c = call_ok("calc.example.net/sub(1, x).ov(2026-03-16)", &env);
    assert_eq!(c.uri(), "cell://calc.example.net/sub/ov?d=2026-03-16&j=x&k=1");
    assert_eq!(resolved(&c).last().unwrap().2, "calc.example.net/sub/ov");
}

/// Under the spec-0.6.2 binding (`sub/st` bound too) the request still reads `par/st`.
#[test]
fn k5_request_the_old_binding_is_not_read() {
    let env = world().call_env(true, true);
    assert_eq!(call_ok("calc.example.net/sub(1, x).st(2026-03-16)", &env).uri(), "cell://calc.example.net/par/st?d=2026-03-16&k=1");
}

/// D's hash bound to no name as of the request's asof is `E_NAME_UNBOUND`.
#[test]
fn k5_request_a_defining_class_with_no_name_is_unbound() {
    let env = world().call_env(true, false);
    assert_eq!(call("calc.example.net/sub(1, x).st(2026-03-16)", &env).unwrap_err(), "E_NAME_UNBOUND");
    assert_eq!(call("calc.example.net/sub(1, x).co(2026-03-16)", &env).unwrap_err(), "E_NAME_UNBOUND");
    // An override needs no D.
    assert!(call("calc.example.net/sub(1, x).ov(2026-03-16)", &env).is_ok());
}

/// The plain read `sub/st` of an inherited method is `E_NAME_UNBOUND` in a request. (PASSES TODAY: not
/// bound in this world. It pins the code.)
#[test]
fn k5_request_the_plain_read_of_an_inherited_method_is_unbound() {
    let env = world().call_env(false, true);
    for m in ["st", "co"] {
        assert_eq!(call(&format!("calc.example.net/sub/{m}(1, x, 2026-03-16)"), &env).unwrap_err(), "E_NAME_UNBOUND", "{m}");
    }
}

// ---------------------------------------------------------------------------------------------
// K6. A parent method's `this.b()` is static, not virtual. A behaviour pin.

/// `par/a` has the body `this.b()`, that is `par/b(k: k)`: read through `sub`, which overrides `b`,
/// it reads `par/b`. BEHAVIOUR PIN: PASSES TODAY, and K5 must keep it. It is checked as defined (the body
/// names `par/b`, defined as a method of `par`) and as evaluated (`par/a` and `sub/a`, both bound in
/// this world, read the receipt of `par/b`: 10, not `sub/b`'s 20).
#[test]
fn k6_this_in_a_parent_method_is_not_virtual() {
    let w = world();
    let env = w.env(&d_only(), false, true);
    // Defined: `this.b()` in a method of par is par/b.
    let key_sig = v(r#"{"args": {"k": {"type": "number", "unit": "1", "valid": false}}, "order": ["k"],
        "returns": {"type": "number", "unit": "1"}}"#);
    let d = define_method(PAR, "calc.example.net/par/a", "this.b()", &key_sig, &BTreeMap::new(), &env, AT)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d.descriptor.body, "calc.example.net/par/b(k: k)\n");
    assert_eq!(d.compiled.inputs, ["calc.example.net/par/b"]);
    assert_eq!(hash_of(&datumesh_core::json::parse(&d.bytes).unwrap()), d.fn_hash);
    // Evaluated.
    let mut fs = w.bound(true);
    let b_par = hash_of(&fs[&format!("{PAR}/b")]);
    let b_sub = hash_of(&fs[&format!("{SUB}/b")]);
    fs.insert("calc.example.net/par/a".into(), fs["calc.example.net/par/a"].clone());
    let r_par = receipt(PAR, "b", &b_par, &format!(r#"{{"k": {}}}"#, k_arg("1")), "{}", "number", "10");
    let r_sub = receipt(
        SUB,
        "b",
        &b_sub,
        &format!(r#"{{"j": {{"type": "string", "value": "x"}}, "k": {}}}"#, k_arg("1")),
        "{}",
        "number",
        "20",
    );
    for cell in ["calc.example.net/par/a", "calc.example.net/sub/a"] {
        let vec = vector(&fs, &[(PAR, &w.par), (SUB, &w.sub)], vec![r_par.clone(), r_sub.clone()], cell, &format!(r#"{{"k": {}}}"#, k_arg("1")));
        let r = run_vector(&vec).unwrap().unwrap_or_else(|c| panic!("{cell}: {c}"));
        assert_eq!(r.computed.outcome.value.lexical(), "10", "{cell}");
        assert_eq!(r.computed.outcome.lineage, [receipt_hash(&r_par)], "{cell}");
    }
}

// ---------------------------------------------------------------------------------------------
// K7. Variable-key receivers.

fn fx_env(signature: &str) -> Env {
    common::classes::class_env("calc.example.net/e", signature, None)
}

const FX_SIG: &str = r#"{"args": {"b": {"type": "string", "valid": false}, "d": {"type": "date", "valid": true}},
    "order": ["b", "d"], "returns": {"type": "number", "unit": {"of": "USD", "per": "EUR"}}}"#;

/// `N(b, "USD").m(d)` with a variable key defines, and is the plain read with those expressions: the
/// same AST, hash, body and inputs.
#[test]
fn k7_a_variable_key_receiver_is_the_plain_read() {
    let env = fx_env(FX_SIG);
    for (method, plain) in [
        ("rates.example.org/fx_pair(b, USD).rate(d)", "rates.example.org/fx_pair/rate(b, USD, d)"),
        ("rates.example.org/fx_pair(b, \"USD\").rate(d)", "rates.example.org/fx_pair/rate(b, \"USD\", d)"),
        ("rates.example.org/fx_pair(quote: \"USD\", base: b).rate(date: d)", "rates.example.org/fx_pair/rate(base: b, quote: \"USD\", date: d)"),
    ] {
        let (m, p) = (body_ok(method, &env), body_ok(plain, &env));
        assert_eq!(m.ast, p.ast, "{method}");
        assert_eq!(m.ast.hash(), p.ast.hash(), "{method}");
        assert_eq!(m.body, p.body, "{method}");
        assert_eq!(m.inputs, ["rates.example.org/fx_pair/rate"], "{method}");
    }
}

/// A computed key (an expression) is also a key expression, as the plain read takes it.
#[test]
fn k7_a_key_may_be_any_expression() {
    let env = fx_env(FX_SIG);
    let (m, p) = (
        body_ok("rates.example.org/fx_pair(lower(b), USD).rate(d)", &env),
        body_ok("rates.example.org/fx_pair/rate(lower(b), USD, d)", &env),
    );
    assert_eq!(m.ast, p.ast);
}

/// The literal rule stays in class-typed positions (16.3): an object that is the key of an object
/// takes literals only. (PASSES TODAY.)
#[test]
fn k7_class_typed_positions_keep_the_literal_rule() {
    let sig = r#"{"args": {"c": {"type": "string", "valid": false}, "d": {"type": "date", "valid": true}},
        "order": ["c", "d"], "returns": {"type": "number", "unit": "1"}}"#;
    let env = fx_env(sig);
    let ok = "rates.example.org/ccy_pair(iso.example.org/currency(EUR), iso.example.org/currency(USD)).rate(d)";
    assert!(define(ok, &env).is_ok(), "{ok}: {:?}", define(ok, &env).err());
    for src in [
        "rates.example.org/ccy_pair(iso.example.org/currency(c), iso.example.org/currency(USD)).rate(d)",
        "rates.example.org/ccy_pair(iso.example.org/currency(EUR), iso.example.org/currency(lower(c))).rate(d)",
    ] {
        assert_eq!(body_code(src, &env), "E_FORMULA_PLACEMENT", "{src}");
    }
    // A bare variable where a key of a class is.
    assert_eq!(body_code("rates.example.org/ccy_pair(c, iso.example.org/currency(USD)).rate(d)", &env), "E_FORMULA_PLACEMENT");
}

// ---------------------------------------------------------------------------------------------
// K8. Request labels are folded before the lookup.

struct Recording {
    inner: MapCallEnv,
    asked: RefCell<Vec<String>>,
}

impl CallEnv for Recording {
    fn alias(&self, alias: &str) -> Result<Option<String>, String> {
        self.inner.alias(alias)
    }
    fn bound(&self, name: &str, asof: Option<&str>) -> Result<Option<FunctionDescriptor>, String> {
        self.inner.bound(name, asof)
    }
    fn pinned(&self, fn_hash: &str) -> Result<Option<FunctionDescriptor>, String> {
        self.inner.pinned(fn_hash)
    }
    fn bound_class(&self, name: &str, asof: Option<&str>) -> Result<Option<ClassDescriptor>, String> {
        self.inner.bound_class(name, asof)
    }
    fn now(&self) -> Result<Option<String>, String> {
        self.inner.now()
    }
    fn lookup(&self, class: &str, label: &str, asof: &str) -> Result<Option<String>, String> {
        self.asked.borrow_mut().push(label.to_string());
        self.inner.lookup(class, label, asof)
    }
}

fn rec() -> Recording {
    Recording { inner: company_env(), asked: RefCell::new(Vec::new()) }
}

const ACC: &str = "0000320193-24-000123";

/// The lookup is given the folded label: `[apple]`, `[  Apple   ]` and `[APPLE]` resolve to one key.
#[test]
fn k8_labels_are_folded_before_the_lookup() {
    for label in ["apple", "  Apple   ", "APPLE", "aPPle", "\"  Apple \""] {
        let r = rec();
        let src = format!("sec.example.org/company[{label}].size()");
        let c = call_ok(&src, &r);
        assert_eq!(c.uri(), "cell://sec.example.org/company/size?cik=0000320193", "{src}");
        assert_eq!(*r.asked.borrow(), ["apple"], "{src}");
    }
}

/// A quoted label with spaces, case and a letter that folds to two: folded, runs of white space made
/// one space, ends trimmed (and NFC).
#[test]
fn k8_quoted_labels_with_spaces_are_folded() {
    for (label, folded, cik) in [
        ("\"New   York\"", "new york", "0000789019"),
        ("\"  NEW YORK  \"", "new york", "0000789019"),
        ("\"new\tyork\"", "new york", "0000789019"),
        ("\"New\u{a0} York\"", "new york", "0000789019"),
        ("\"Stra\u{df}e\"", "strasse", "0001018724"),
        ("\"STRASSE\"", "strasse", "0001018724"),
    ] {
        let r = rec();
        let src = format!("sec.example.org/company[{label}].size()");
        let c = call_ok(&src, &r);
        assert_eq!(c.uri(), format!("cell://sec.example.org/company/size?cik={cik}"), "{src:?}");
        assert_eq!(*r.asked.borrow(), [folded], "{src:?}");
    }
}

/// An unknown folded label is still unbound, and the lookup was asked the folded label.
#[test]
fn k8_an_unknown_label_is_unbound_under_its_folded_form() {
    let r = rec();
    assert_eq!(call("sec.example.org/company[Banana].size()", &r).unwrap_err(), "E_NAME_UNBOUND");
    assert_eq!(*r.asked.borrow(), ["banana"]);
}

/// Keys are not folded: a key keeps its case and spaces, and a key's rule still rejects. (PASSES
/// TODAY.)
#[test]
fn k8_keys_are_not_folded() {
    let env = company_env();
    assert_eq!(call_ok("sec.example.org/company(\"  Ab C \").size()", &env).uri(), "cell://sec.example.org/company/size?cik=%20%20Ab%20C%20");
    // Case is kept in an upper_ascii key's rejection: it is not repaired.
    let fx = MapCallEnv::from_value(common::vector("call/class-lookup.json").get("env").unwrap()).unwrap();
    assert_eq!(call("rates.example.org/fx_pair(eur, USD).rate(2026-03-16)", &fx).unwrap_err(), "E_KEY_NORMALISATION");
}

// ---------------------------------------------------------------------------------------------
// K10. Chains: a link after a non-key method.

/// A link after a method that does not return a key is `E_PARSE_URI`. (PASSES TODAY: every chain is
/// `E_PARSE_URI` today. After K10 it holds for this case only, so it is a pin; the chains that parse are
/// in `classes_round_api.rs`.)
#[test]
fn k10_a_link_after_a_non_key_method_is_a_parse_error() {
    let env = company_env();
    for src in [
        "sec.example.org/company(abc).size().fact(x)",
        "sec.example.org/company(abc).fact(x).size()",
        "sec.example.org/company(abc).size().size().size()",
    ] {
        assert_eq!(call(src, &env).unwrap_err(), "E_PARSE_URI", "{src}");
    }
    // A chain needs a method on each link, with arguments in parentheses.
    for src in [
        "sec.example.org/company(abc).owner().",
        "sec.example.org/company(abc).owner().fact",
        "sec.example.org/company(abc).owner()fact(x)",
    ] {
        assert_eq!(call(src, &env).unwrap_err(), "E_PARSE_URI", "{src}");
    }
}

// ---------------------------------------------------------------------------------------------
// K11. Unquoted runs of digits and hyphens are strings in string positions.

const CIK: &str = "0000320193";

/// In a string key and a string argument, an unquoted `0000320193` or `0000320193-24-000123` is that
/// string, positional or labelled, in an object's keys, a method's arguments and a plain read.
#[test]
fn k11_runs_of_digits_and_hyphens_are_strings_in_string_positions() {
    let env = company_env();
    let fact = format!("cell://sec.example.org/company/fact?accession={ACC}&cik={CIK}");
    for src in [
        format!("sec.example.org/company({CIK}).fact({ACC})"),
        format!("sec.example.org/company(cik: {CIK}).fact(accession: {ACC})"),
        format!("sec.example.org/company/fact({CIK}, {ACC})"),
        format!("sec.example.org/company/fact(accession: {ACC}, cik: {CIK})"),
    ] {
        assert_eq!(call_ok(&src, &env).uri(), fact, "{src}");
    }
    let filing = format!("cell://sec.example.org/filing?accession={ACC}&cik={CIK}&n=5");
    assert_eq!(call_ok(&format!("sec.example.org/filing({CIK}, {ACC}, 5)"), &env).uri(), filing);
    assert_eq!(call_ok(&format!("sec.example.org/filing(n: 5, accession: {ACC}, cik: {CIK})"), &env).uri(), filing);
    // The same strings, quoted, are the same cell.
    assert_eq!(call_ok(&format!("sec.example.org/filing(\"{CIK}\", \"{ACC}\", 5)"), &env).uri(), filing);
}

/// More shapes of the run, in a string position: one digit, several hyphens, a trailing hyphen.
#[test]
fn k11_the_run_is_a_string_whatever_its_shape() {
    let env = company_env();
    for run in ["0", "00", "1-2", "0000320193-24-000123-5", "007-007", "123456789012345678901234567890"] {
        let c = call_ok(&format!("sec.example.org/company({run}).size()"), &env);
        assert_eq!(c.uri(), format!("cell://sec.example.org/company/size?cik={run}"), "{run}");
    }
}

/// In a number position the run keeps today's codes (the number lexer's, or a parse error).
/// (PASSES TODAY.)
#[test]
fn k11_in_a_number_position_the_codes_are_today_s() {
    let env = company_env();
    for (src, code) in [
        ("sec.example.org/filing(a, b, 0000320193)", "E_CANON_NUMBER"),
        ("sec.example.org/filing(a, b, 0000320193-24-000123)", "TODAY"),
        ("sec.example.org/filing(a, b, 1-2)", "TODAY"),
    ] {
        let got = call(src, &env).unwrap_err();
        if code == "TODAY" {
            assert!(got == "E_PARSE_URI" || got == "E_CANON_NUMBER" || got == "E_CANON_TYPE", "{src}: {got}");
        } else {
            assert_eq!(got, code, "{src}");
        }
    }
}

/// Runs with letters keep the digit-led-word rule: `ux-lc` and the like stay `E_PARSE_URI` in a string
/// position, and a run starting with a hyphen is not a string. (`ux-lc` PASSES TODAY.)
#[test]
fn k11_runs_with_letters_or_a_leading_hyphen_are_refused() {
    let env = company_env();
    // The digit-led-word rule: E_PARSE_URI, as the vector `invalid/call-class-unquoted-needs-quotes`.
    assert_eq!(call("sec.example.org/company(ux-lc).size()", &env).unwrap_err(), "E_PARSE_URI");
    assert_eq!(call("scripture.example.org/verse(quran, 2, 1, 255).text(ux-lc)", &env).unwrap_err(), "E_PARSE_URI");
    // A digit run with letters, and a run that does not start with a digit, are not strings.
    for src in [
        "sec.example.org/company(0000320193-ab).size()",
        "sec.example.org/company(1a-2).size()",
        "sec.example.org/company(-0000320193-24).size()",
        "sec.example.org/company(--1).size()",
        "sec.example.org/company(1--).size(",
    ] {
        assert!(call(src, &env).is_err(), "{src}");
    }
}

/// What the date lexer accepts as a whole stays a date token, which is a string in a string position
/// (as today).  (PASSES TODAY.)
#[test]
fn k11_a_date_is_still_a_date() {
    let env = company_env();
    assert_eq!(call_ok("sec.example.org/company(2024-03-15).size()", &env).uri(), "cell://sec.example.org/company/size?cik=2024-03-15");
}

// ---------------------------------------------------------------------------------------------
// K13. Literal defaults.

/// A function whose argument `rank` is a number of unit 1 defaulting to `"1"`.
fn default_function(default: &str) -> Value {
    stored(&format!(
        r#"{{"args": {{"name": {{"type": "string", "valid": false}},
        "rank": {{"type": "number", "unit": "1", "valid": false, "default": {default}}}}},
        "order": ["name", "rank"], "returns": {{"type": "number", "unit": "1"}}}}"#
    ))
}

fn default_call_env(default: &str) -> Result<MapCallEnv, datumesh_core::Error> {
    let mut e = MapCallEnv::default();
    e.functions.insert("calc.example.net/source".into(), FunctionDescriptor::from_value(&default_function(default))?);
    Ok(e)
}

/// A request that leaves the argument out binds the default, and `resolved` records it.
#[test]
fn k13_a_request_omitting_a_defaulted_argument_binds_it() {
    let env = default_call_env("\"1\"").unwrap_or_else(|e| panic!("a default \"1\": {e}"));
    let c = call_ok("calc.example.net/source(x)", &env);
    assert_eq!(c.uri(), "cell://calc.example.net/source?name=x&rank=1");
    assert_eq!(resolved(&c), [("default".to_string(), "rank".to_string(), "1".to_string())]);
    assert_eq!(c.resolved.as_ref().unwrap().call, "calc.example.net/source(name: \"x\", rank: 1)");
    // Written, it is that value and nothing was resolved.
    let c = call_ok("calc.example.net/source(x, 2)", &env);
    assert_eq!(c.uri(), "cell://calc.example.net/source?name=x&rank=2");
    assert!(c.resolved.is_none());
    // Labelled, left out.
    assert_eq!(call_ok("calc.example.net/source(name: x)", &env).uri(), "cell://calc.example.net/source?name=x&rank=1");
    // An argument without a default is still missing.
    assert_eq!(call("calc.example.net/source()", &env).unwrap_err(), "E_ARG_MISSING");
}

fn default_env(default: &str) -> Env {
    let mut env = Env::new("calc.example.net/e", Signature::from_value(&v(&format!(r#"{{"args": {{}}, "order": [], "returns": {NUM1}}}"#))).unwrap());
    env.add_function("calc.example.net/source", &default_function(default)).unwrap_or_else(|e| panic!("{e}"));
    env
}

/// A body that leaves the argument out has the canonical AST of the body that writes the literal.
#[test]
fn k13_a_body_omitting_a_defaulted_argument_has_the_literal_s_ast() {
    let env = default_env("\"1\"");
    let omitted = body_ok("calc.example.net/source(\"x\")", &env);
    let written = body_ok("calc.example.net/source(\"x\", 1)", &env);
    assert_eq!(omitted.ast, written.ast);
    assert_eq!(omitted.ast.hash(), written.ast.hash());
    assert_eq!(omitted.body, written.body);
    assert_eq!(omitted.inputs, written.inputs);
    assert!(omitted.body.contains("rank: 1"), "{}", omitted.body);
    // Labelled, and with another value written.
    assert_eq!(body_ok("calc.example.net/source(name: \"x\")", &env).ast, written.ast);
    assert_ne!(body_ok("calc.example.net/source(\"x\", 2)", &env).ast, written.ast);
    // An argument without a default is still missing.
    assert_eq!(body_code("calc.example.net/source()", &env), "E_FORMULA_ARG_MISSING");
}

fn k13_signature(default: &str) -> Value {
    v(&format!(
        r#"{{"args": {{"rank": {{"type": "number", "unit": "1", "valid": false, "default": {default}}}}},
        "order": ["rank"], "returns": {NUM1}}}"#
    ))
}

fn define_with_default(default: &str) -> Result<(), String> {
    let sig = k13_signature(default);
    let callees = Env::new("calc.example.net/none", Signature::from_value(&sig).map_err(|e| e.to_string())?);
    define_function("calc.example.net/f", "rank + 1", &sig, &BTreeMap::new(), &callees, AT)
        .map(|_| ())
        .map_err(|e| e.code().to_string())
}

/// A default that is a valid value of the type defines; a default that is not is `E_SCHEMA`. The
/// default is part of the descriptor, so it changes the function hash.
#[test]
fn k13_a_default_of_the_wrong_type_is_refused_at_define() {
    define_with_default("\"1\"").unwrap_or_else(|c| panic!("a default \"1\" is refused: {c}"));
    define_with_default("\"2.50\"").unwrap_or_else(|c| panic!("a default \"2.50\" is refused: {c}"));
    for bad in ["\"abc\"", "\"1,5\"", "\"\"", "1", "true", "null", "\"01\"", "\"1.\""] {
        assert_eq!(define_with_default(bad).unwrap_err(), "E_SCHEMA", "default {bad}");
    }
    // A date argument's default is a date, an instant argument's an instant.
    let sig = v(r#"{"args": {"d": {"type": "date", "valid": true, "default": "2026-13-01"}}, "order": ["d"], "returns": {"type": "number", "unit": "1"}}"#);
    let callees = Env::new("calc.example.net/none", Signature::from_value(&sig).unwrap());
    let e = define_function("calc.example.net/f", "1", &sig, &BTreeMap::new(), &callees, AT).unwrap_err();
    assert_eq!(e.code(), "E_SCHEMA");
    let sig = v(r#"{"args": {"d": {"type": "date", "valid": true, "default": "2026-03-16"}}, "order": ["d"], "returns": {"type": "number", "unit": "1"}}"#);
    let callees = Env::new("calc.example.net/none", Signature::from_value(&sig).unwrap());
    define_function("calc.example.net/f", "1", &sig, &BTreeMap::new(), &callees, AT).unwrap_or_else(|e| panic!("{e}"));
    // Part of the hash.
    let hash = |default: &str| {
        let sig = k13_signature(default);
        let callees = Env::new("calc.example.net/none", Signature::from_value(&sig).unwrap());
        define_function("calc.example.net/f", "rank + 1", &sig, &BTreeMap::new(), &callees, AT).unwrap().fn_hash
    };
    assert_ne!(hash("\"1\""), hash("\"2\""));
}

/// The `asof` default of 16.7 is a different default, and keeps working beside it. (PASSES TODAY.)
#[test]
fn k13_the_asof_default_is_unchanged() {
    let sig = v(r#"{"args": {"d": {"type": "date", "valid": true, "default": "asof"}}, "order": ["d"], "returns": {"type": "number", "unit": "1"}}"#);
    let callees = Env::new("calc.example.net/none", Signature::from_value(&sig).unwrap());
    define_function("calc.example.net/f", "1", &sig, &BTreeMap::new(), &callees, AT).unwrap_or_else(|e| panic!("{e}"));
}
