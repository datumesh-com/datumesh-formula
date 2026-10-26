//! `define_function`: from a body, the caller's signature (with `order`), the alias map and the
//! callees' descriptors as bound in the store at define time, the canonical formula descriptor:
//! the printed body, the derived `inputs`, `order` and extents as given, every define-time
//! check (15.6: the section-7 canonical form first, then `E_FORMULA_ORDER`, the body's codes,
//! `E_FORMULA_NOT_CANONICAL`, `E_FORMULA_INPUTS`); `publish` stores it and binds the name.
#![cfg(feature = "engine")]

mod common;
#[path = "common/pg.rs"]
mod pg;

use std::collections::BTreeMap;

use datumesh_core::json;
use datumesh_core::{FunctionDescriptor, Resolver};
use datumesh_formula::definition::{define_function, publish, Callees};
use datumesh_formula::{check_descriptor, Env, Signature};
use datumesh_store::Store;
use pg::Mesh;

const T0: &str = "2026-01-01T00:00:00.000000Z";
const T1: &str = "2026-02-01T00:00:00.000000Z";

const FX_SIG: &str = r#"{"args":{"base":{"type":"string","valid":false},"date":{"type":"date","valid":true},
    "quote":{"type":"string","valid":false}},"order":["base","quote","date"],
    "returns":{"type":"number","unit":{"of":{"arg":"quote"},"per":{"arg":"base"}}}}"#;

const CONVERT_SIG: &str = r#"{"args":{"amount":{"type":"number","unit":"EUR","valid":false},
    "d":{"extent":{"hi":"2026-12-31","lo":"2026-01-01"},"type":"date","valid":true}},"order":["amount","d"],
    "returns":{"type":"number","unit":"USD"}}"#;

fn sig(text: &str) -> json::Value {
    json::parse(text.as_bytes()).unwrap()
}

fn mesh() -> Mesh {
    let m = Mesh::new(&["rates.example.org"]);
    m.stored("rates.example.org/fx", &FX_SIG.replace('\n', ""), T0);
    m
}

#[test]
fn the_canonical_descriptor() {
    let m = mesh();
    let aliases: BTreeMap<String, String> = [("fx".to_string(), "rates.example.org/fx".to_string())].into();
    let src = r#"ROUND(amount * @fx("EUR", "USD", d), 2)"#;
    let d = define_function("calc.example.net/convert", src, &sig(CONVERT_SIG), &aliases, &*m.store, T0).unwrap();
    let body = "round(amount * rates.example.org/fx(base: \"EUR\", date: d, quote: \"USD\"), digits: 2)\n";
    assert_eq!(d.compiled.body, body);
    assert_eq!(d.descriptor.body, body);
    assert_eq!(d.descriptor.lang, "formula");
    assert_eq!(d.descriptor.inputs, ["rates.example.org/fx"]);
    assert_eq!(d.descriptor.order.as_deref(), Some(&["amount".to_string(), "d".to_string()][..]));
    assert!(d.descriptor.extents.contains_key("d"), "extent as given");
    // Canonical: the bytes are the descriptor's JCS, the hash their SHA-256, and the descriptor
    // passes every check of 15.6.
    assert_eq!(d.bytes, d.descriptor.canonical());
    assert_eq!(d.fn_hash, datumesh_core::sha256_hex(&d.bytes));
    assert_eq!(FunctionDescriptor::from_json(&d.bytes).unwrap(), d.descriptor);
    let mut env = Env::new("calc.example.net/convert", Signature::from_value(&sig(CONVERT_SIG)).unwrap());
    env.add_function(
        "rates.example.org/fx",
        &sig(&format!(r#"{{"code":{{"body":"","lang":"stored"}},"inputs":[],"signature":{FX_SIG}}}"#)),
    )
    .unwrap();
    check_descriptor("calc.example.net/convert", &json::parse(&d.bytes).unwrap(), &env).unwrap();
    // The callees it was checked against.
    assert_eq!(
        d.callees.get("rates.example.org/fx").map(String::as_str),
        m.store.resolve_name("rates.example.org/fx", T0).unwrap().as_deref()
    );
    // Defining again gives the same bytes.
    let again = define_function("calc.example.net/convert", src, &sig(CONVERT_SIG), &aliases, &*m.store, T0).unwrap();
    assert_eq!(again.bytes, d.bytes);
}

#[test]
fn publish_stores_and_binds() {
    let m = mesh();
    let src = r#"amount * rates.example.org/fx("EUR", "USD", d)"#;
    let d =
        define_function("calc.example.net/convert", src, &sig(CONVERT_SIG), &BTreeMap::new(), &*m.store, T1).unwrap();
    let h = publish(&*m.store, &d, T1).unwrap();
    assert_eq!(h, d.fn_hash);
    assert_eq!(m.store.resolve_name("calc.example.net/convert", T1).unwrap().as_deref(), Some(h.as_str()));
    assert_eq!(m.store.resolve_name("calc.example.net/convert", T0).unwrap(), None);
    assert_eq!(&*m.store.descriptor(&h).unwrap(), &d.bytes[..]);
}

#[test]
fn callees_resolve_as_of_define_time() {
    let m = mesh();
    let src = r#"amount * rates.example.org/fx("EUR", "USD", d)"#;
    let before = "2025-12-31T00:00:00.000000Z";
    let e = define_function("calc.example.net/convert", src, &sig(CONVERT_SIG), &BTreeMap::new(), &*m.store, before)
        .unwrap_err();
    assert_eq!(e.code(), "E_FORMULA_UNRESOLVED");
    // The store as a callee source: the binding as of a time, its start and its descriptor.
    let c = (&*m.store as &dyn Callees).callee("rates.example.org/fx", T0).unwrap().unwrap();
    assert_eq!(c.start, T0);
    assert_eq!(FunctionDescriptor::from_value(&c.descriptor).unwrap().lang, "stored");
    assert!((&*m.store as &dyn Callees).callee("rates.example.org/none", T0).unwrap().is_none());
    // A self-reference needs no binding: the function being defined is in scope.
    let fib = r#"{"args":{"n":{"type":"number","unit":"1","valid":false}},"order":["n"],"returns":{"type":"number","unit":"1"}}"#;
    let d = define_function(
        "calc.example.net/fib",
        "IF(n < 2, n, calc.example.net/fib(n - 1) + calc.example.net/fib(n - 2))",
        &sig(fib),
        &BTreeMap::new(),
        &*m.store,
        T0,
    )
    .unwrap();
    assert_eq!(
        d.descriptor.body,
        "if n < 2 then n else calc.example.net/fib(n: n - 1) + calc.example.net/fib(n: n - 2)\n"
    );
    assert_eq!(d.descriptor.inputs, ["calc.example.net/fib"]);
}

#[test]
fn define_time_checks_in_order() {
    let m = mesh();
    let no_order = CONVERT_SIG.replace(r#""order":["amount","d"],"#, "");
    let code = |src: &str, signature: &str| {
        define_function("calc.example.net/convert", src, &sig(signature), &BTreeMap::new(), &*m.store, T0)
            .err()
            .map(|e| e.code().to_string())
    };
    // The body's codes.
    assert_eq!(code("amount + 1 USD", CONVERT_SIG).as_deref(), Some("E_FORMULA_UNIT_MISMATCH"));
    assert_eq!(code("amount +", CONVERT_SIG).as_deref(), Some("E_FORMULA_SYNTAX"));
    assert_eq!(code("@fx(\"EUR\", \"USD\", d)", CONVERT_SIG).as_deref(), Some("E_FORMULA_ALIAS"));
    assert_eq!(code("NOW()", CONVERT_SIG).as_deref(), Some("E_FORMULA_FORBIDDEN"));
    assert_eq!(code("amount", CONVERT_SIG).as_deref(), Some("E_FORMULA_RETURN_UNIT"));
    // E_FORMULA_ORDER before the body's codes.
    assert_eq!(code("amount +", &no_order).as_deref(), Some("E_FORMULA_ORDER"));
    // The section-7 canonical form first (here an unknown unit): a core code.
    let bad_unit = CONVERT_SIG.replace(r#""unit":"USD""#, r#""unit":"XYZ""#);
    let bad_unit_no_order = bad_unit.replace(r#""order":["amount","d"],"#, "");
    let c = code("amount +", &bad_unit_no_order).unwrap();
    assert!(!c.starts_with("E_FORMULA_"), "{c}");
    assert_eq!(code("amount +", &bad_unit).as_deref(), Some(c.as_str()));
}

#[test]
fn check_descriptor_puts_the_canonical_form_first() {
    // Inputs not ascending is a section-7 failure (E_CANON_SET), reported before E_FORMULA_ORDER.
    let env = Env::new("calc.example.net/f", Signature::from_value(&sig(CONVERT_SIG)).unwrap());
    let d = json::parse(
        br#"{"code":{"body":"amount\n","lang":"formula"},"inputs":["z.example.org/b","a.example.org/a"],
        "signature":{"args":{"amount":{"type":"number","unit":"EUR","valid":false}},"returns":{"type":"number","unit":"EUR"}}}"#,
    )
    .unwrap();
    let e = check_descriptor("calc.example.net/f", &d, &env).unwrap_err();
    assert_eq!(e.code(), "E_CANON_SET");
    let d = json::parse(
        br#"{"code":{"body":"amount\n","lang":"formula"},"inputs":[],
        "signature":{"args":{"amount":{"type":"number","unit":"EUR","valid":false}},"returns":{"type":"number","unit":"EUR"}}}"#,
    )
    .unwrap();
    assert_eq!(check_descriptor("calc.example.net/f", &d, &env).unwrap_err().code(), "E_FORMULA_ORDER");
}

#[test]
fn fn_hash_is_a_reserved_argument_name() {
    // spec-0.4.2: `fn_hash` is the URI's pin (5.1), reserved as an argument name (2.4).
    let m = mesh();
    let sig = r#"{"args":{"fn_hash":{"type":"string","valid":false}},"order":["fn_hash"],"returns":"string"}"#;
    let e = define_function(
        "calc.example.net/h",
        "fn_hash",
        &json::parse(sig.as_bytes()).unwrap(),
        &BTreeMap::new(),
        &*m.store,
        T0,
    )
    .unwrap_err();
    assert_eq!(e.code(), "E_CANON_ARG_NAME");
    let env = Env::new("calc.example.net/h", Signature::from_value(&json::parse(sig.as_bytes()).unwrap()).unwrap());
    let d = json::parse(
        br#"{"code":{"body":"fn_hash\n","lang":"formula"},"inputs":[],"signature":{"args":{"fn_hash":{"type":"string","valid":false}},"order":["fn_hash"],"returns":"string"}}"#,
    )
    .unwrap();
    assert_eq!(check_descriptor("calc.example.net/h", &d, &env).unwrap_err().code(), "E_CANON_ARG_NAME");
}
