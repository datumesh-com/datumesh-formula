//! spec-0.5.0 at `c8896ad`: the clarifications of this crate's SPEC-FINDINGS sections H and I
//! (`BRIEF-spec-0.5.0-clarifications-2.md`). The vectors of the revision are run by the other test
//! files; these tests pin the points whose behaviour changes for this crate (H1, H3, H4, H7, I3)
//! and the points that were text only (H5, I1, I2, I4, H10), each on cases beyond the vectors.
//! Where a case is not a vector, its expectation was checked against the reference tool of
//! `c8896ad` as a black box (`IMPL-NOTES-classes.md`, "How the expectations were made").

mod common;

use common::classes::{class_env, no_args, FX, ONE};
use datumesh_core::json::Value;
use datumesh_formula::call::{parse_call, CallError, MapCallEnv};
use datumesh_formula::memory::run_vector;
use datumesh_formula::{define, Code, Env};

// ---------------------------------------------------------------------------------------------
// Helpers.

/// Remove the member at `path` of an object tree.
fn remove(v: &mut Value, path: &[&str]) {
    let Value::Object(m) = v else { panic!("not an object at {path:?}") };
    if path.len() == 1 {
        let before = m.len();
        m.retain(|(k, _)| k != path[0]);
        assert_eq!(m.len() + 1, before, "no member {}", path[0]);
    } else {
        let (_, child) = m.iter_mut().find(|(k, _)| k == path[0]).unwrap_or_else(|| panic!("no member {}", path[0]));
        remove(child, &path[1..]);
    }
}

/// Replace the string at `path` of an object tree.
fn set_string(v: &mut Value, path: &[&str], new: &str) {
    let Value::Object(m) = v else { panic!("not an object at {path:?}") };
    let (_, child) = m.iter_mut().find(|(k, _)| k == path[0]).unwrap_or_else(|| panic!("no member {}", path[0]));
    if path.len() == 1 {
        *child = Value::String(new.to_string());
    } else {
        set_string(child, &path[1..], new);
    }
}

const PRICE: &str = "calc.example.net/bond/price";
const BOND: &str = "calc.example.net/bond";

fn body_code(src: &str) -> String {
    match define(src, &class_env("calc.example.net/e", &no_args(ONE), None)) {
        Ok(c) => panic!("{src}: accepted as {:?}", c.body),
        Err(e) => e.code.as_str().to_string(),
    }
}

fn call_env() -> MapCallEnv {
    MapCallEnv::from_value(common::vector("call/class-lookup.json").get("env").unwrap()).unwrap()
}

fn call_code(src: &str, env: &MapCallEnv) -> String {
    match parse_call(src, env) {
        Ok(c) => panic!("{src}: parsed to {}", c.uri()),
        Err(CallError::Code(c, _)) => c.to_string(),
        Err(other) => other.to_string(),
    }
}

const NOW: &str = "2026-07-01T00:00:00.000000Z";
const ASOF: &str = "?asof=2026-06-01T00:00:00.000000Z";

// ---------------------------------------------------------------------------------------------
// H1 (15.8, 15.11, 16.6): a dispatch's method rebound or unbound at evaluation.

/// `<class>/m` bound to another hash than the class's method is `E_FORMULA_DISPATCH`; `<class>/m`
/// or the class not bound as of the read is `E_FORMULA_UNRESOLVED`. Both are the mesh's failures,
/// never faults (`invalid/formula-eval-class-dispatch-rebound`).
#[test]
fn a_method_rebound_or_unbound_at_evaluation() {
    let rebound = common::vector("invalid/formula-eval-class-dispatch-rebound.json");
    assert_eq!(run_vector(&rebound).unwrap().unwrap_err(), Code::Dispatch);
    for c in [Code::Dispatch, Code::Unresolved] {
        assert!(!c.is_fault() && !c.is_catchable() && c.is_mesh_failure(), "{c}");
    }

    // The same world with the method's binding gone: unresolved, not the define-time code.
    let mut unbound = rebound.clone();
    remove(&mut unbound, &["world", "functions", PRICE]);
    assert_eq!(run_vector(&unbound).unwrap().unwrap_err(), Code::Unresolved);

    // The class not bound (the method bound to another hash, as in the vector).
    let mut no_class = rebound.clone();
    remove(&mut no_class, &["world", "classes", BOND]);
    assert_eq!(run_vector(&no_class).unwrap().unwrap_err(), Code::Unresolved);
    let mut neither = no_class.clone();
    remove(&mut neither, &["world", "functions", PRICE]);
    assert_eq!(run_vector(&neither).unwrap().unwrap_err(), Code::Unresolved);

    // The valid dispatch with the method (or the class) bound only after the asof: unresolved.
    let valid = common::vector("formula-eval/class-dispatch-bond.json");
    assert!(run_vector(&valid).unwrap().is_ok());
    let mut later = valid.clone();
    set_string(&mut later, &["world", "functions", PRICE, "bound"], "2026-12-01T00:00:00.000000Z");
    assert_eq!(run_vector(&later).unwrap().unwrap_err(), Code::Unresolved);
    let mut class_later = valid.clone();
    set_string(&mut class_later, &["world", "classes", BOND, "bound"], "2026-12-01T00:00:00.000000Z");
    assert_eq!(run_vector(&class_later).unwrap().unwrap_err(), Code::Unresolved);
}

// ---------------------------------------------------------------------------------------------
// I4 (16.6, 8.2): what a dispatch relies on.

/// The class is relied on through its methods' bindings: the receipt's `asof` candidates are those
/// of the `<class>/m` bindings read, so a later start of `bond/price` moves the receipt's `asof`,
/// and a later start of the class binding (still before the evaluation asof) moves nothing.
#[test]
fn a_dispatch_relies_on_the_methods_bindings_only() {
    let valid = common::vector("formula-eval/class-dispatch-bond.json");
    let base = run_vector(&valid).unwrap().unwrap();
    assert_eq!(base.computed.outcome.receipt_asof, "2026-03-16T16:00:00.000000Z");
    assert_eq!(base.computed.receipt_hash, "fe8f3955276c1e5a6adaf1a1c93c2f1130e0804455988a50000fe35b067c65e9");

    let mut method_later = valid.clone();
    set_string(&mut method_later, &["world", "functions", PRICE, "bound"], "2026-05-01T00:00:00.000000Z");
    let r = run_vector(&method_later).unwrap().unwrap();
    assert_eq!(r.computed.outcome.receipt_asof, "2026-05-01T00:00:00.000000Z");
    assert_eq!(r.computed.receipt_hash, "6b81d9c2e20c0b4007b7810148acb1fc2e9d9ea866304b01ff7c337c81c6d063");
    assert_eq!(r.computed.outcome.lineage, base.computed.outcome.lineage);

    let mut class_later = valid.clone();
    set_string(&mut class_later, &["world", "classes", BOND, "bound"], "2026-05-01T00:00:00.000000Z");
    let r = run_vector(&class_later).unwrap().unwrap();
    assert_eq!(r.computed.outcome.receipt_asof, base.computed.outcome.receipt_asof);
    assert_eq!(r.computed.receipt_hash, base.computed.receipt_hash);
}

// ---------------------------------------------------------------------------------------------
// H3 (16.3, 16.5, 5.8): a quoted key encoding in a class-typed position is decoded against its class.

fn enc(code: &str) -> String {
    format!("\"iso.example.org/currency(code:{})\"", code.replace('"', "\"\""))
}

fn pair(base: &str, quote: &str) -> String {
    format!("rates.example.org/ccy_pair({base}, {quote}).rate(2026-03-16)")
}

/// In a body: a string inside the quoted encoding not canonical under its key's rule is
/// `E_FORMULA_KEY` (as an object literal's is), key names or types not the class's are
/// `E_FORMULA_TYPE`, a bare lowercase word as an object's key is a variable, so the object has a
/// non-literal key: `E_FORMULA_PLACEMENT`.
#[test]
fn a_quoted_key_encoding_is_decoded_against_its_class_in_a_body() {
    let (eur, usd) = (enc("\"EUR\""), enc("\"USD\""));
    let ok = define(&pair(&eur, &usd), &class_env("calc.example.net/e", &no_args(ONE), None)).unwrap();
    assert!(ok.body.contains("code:\"\"EUR\"\""), "{}", ok.body);
    for (src, want) in [
        (pair(&enc("\"eur\""), &usd), "E_FORMULA_KEY"),
        (pair(&eur, &enc("\"usd\"")), "E_FORMULA_KEY"),
        (pair(&enc("\"EUR \""), &usd), "E_FORMULA_KEY"),
        (format!("rates.example.org/ccy_pair/rate({}, {usd}, 2026-03-16)", enc("\"eur\"")), "E_FORMULA_KEY"),
        (pair(&enc("\"EUR\"").replace("code", "cod"), &usd), "E_FORMULA_TYPE"),
        (pair(&enc("1"), &usd), "E_FORMULA_TYPE"),
        (
            "rates.example.org/ccy_pair(\"rates.example.org/fx_pair(base:\"\"EUR\"\",quote:\"\"USD\"\")\", "
                .to_string()
                + &usd
                + ").rate(2026-03-16)",
            "E_FORMULA_TYPE",
        ),
        (pair("iso.example.org/currency(eur)", &usd), "E_FORMULA_PLACEMENT"),
        (pair(&eur, "iso.example.org/currency(usd)"), "E_FORMULA_PLACEMENT"),
    ] {
        assert_eq!(body_code(&src), want, "{src}");
    }
}

/// In a call: not canonical is `E_KEY_NORMALISATION`, key names or types not the class's
/// `E_CANON_TYPE`, a malformed encoding `E_KEY_ENCODING` as before.
#[test]
fn a_quoted_key_encoding_is_decoded_against_its_class_in_a_call() {
    let e = call_env();
    let (eur, usd) = (enc("\"EUR\""), enc("\"USD\""));
    assert!(parse_call(&pair(&eur, &usd), &e).is_ok());
    for (src, want) in [
        (pair(&enc("\"eur\""), &usd), "E_KEY_NORMALISATION"),
        (pair(&eur, &enc("\"usd\"")), "E_KEY_NORMALISATION"),
        (pair(&enc("\"EUR \""), &usd), "E_KEY_NORMALISATION"),
        (format!("rates.example.org/ccy_pair/rate({}, {usd}, 2026-03-16)", enc("\"eur\"")), "E_KEY_NORMALISATION"),
        (pair(&enc("\"EUR\"").replace("code", "cod"), &usd), "E_CANON_TYPE"),
        (pair(&enc("1"), &usd), "E_CANON_TYPE"),
        (
            "rates.example.org/ccy_pair(\"rates.example.org/fx_pair(base:\"\"EUR\"\",quote:\"\"USD\"\")\", "
                .to_string()
                + &usd
                + ").rate(2026-03-16)",
            "E_CANON_TYPE",
        ),
        (pair("\"nonsense\"", &usd), "E_KEY_ENCODING"),
        (pair("iso.example.org/currency(eur)", &usd), "E_KEY_NORMALISATION"),
    ] {
        assert_eq!(call_code(&src, &e), want, "{src}");
    }
}

// ---------------------------------------------------------------------------------------------
// H4 (16.7): a call that needs a now.

/// A call with no `asof` and no now that needs one (an `asof` default, a label's lookup) is
/// `E_ARG_MISSING`, a label with no value as well (the lookup is not reached); with an `asof`, with
/// a now, or when it needs none, it is answered.
#[test]
fn a_call_that_needs_a_now_and_has_none() {
    let mut e = call_env();
    e.now = None;
    for src in [
        "rates.example.org/fx_rate(EUR, GBP)",
        "rates.example.org/fx_pair[cable].rate(2026-03-16)",
        "rates.example.org/fx_pair[nope].rate(2026-03-16)",
        "rates.example.org/fx_pair[cable].rate()",
    ] {
        assert_eq!(call_code(src, &e), "E_ARG_MISSING", "{src}");
    }
    // With an asof, the label is looked up (and a missing one is a name that is not bound).
    assert!(parse_call(&format!("rates.example.org/fx_rate(EUR, GBP){ASOF}"), &e).is_ok());
    assert!(parse_call(&format!("rates.example.org/fx_pair[cable].rate(2026-03-16){ASOF}"), &e).is_ok());
    assert_eq!(call_code(&format!("rates.example.org/fx_pair[nope].rate(2026-03-16){ASOF}"), &e), "E_NAME_UNBOUND");
    // Needing no now: every argument given.
    assert!(parse_call("rates.example.org/fx_rate(EUR, GBP, 2026-03-16)", &e).is_ok());
    // With the mesh's now.
    e.now = Some(NOW.to_string());
    assert!(parse_call("rates.example.org/fx_rate(EUR, GBP)", &e).is_ok());
    assert!(parse_call("rates.example.org/fx_pair[cable].rate(2026-03-16)", &e).is_ok());
}

// ---------------------------------------------------------------------------------------------
// H7 (5.8, 16.3): the lookup method on an object, an extension method whose arguments don't match.

/// The class's `lookup` method called on an object is a method the class does not have:
/// `E_NAME_UNBOUND` (and not `E_PARSE_URI`), as a method it has not.
#[test]
fn the_lookup_method_on_an_object() {
    let e = call_env();
    for src in [
        "rates.example.org/fx_pair(EUR, USD).named(cable)",
        "rates.example.org/fx_pair(EUR, USD).named(\"cable\")",
        "rates.example.org/fx_pair(EUR, USD).named()",
        "rates.example.org/fx_pair(EUR, USD).points(2026-03-16)",
    ] {
        assert_eq!(call_code(src, &e), "E_NAME_UNBOUND", "{src}");
    }
    // Through the label it is still the lookup, and the method after it is read as a method.
    assert!(parse_call(&format!("rates.example.org/fx_pair[cable].rate(2026-03-16){ASOF}"), &e).is_ok());
}

/// An extension method that does not take the class keys first is `E_ARG_UNKNOWN` (the object's
/// keys are arguments it does not take); its own arguments bind as any call's: `E_ARG_UNKNOWN` for
/// one it does not have, `E_ARG_MISSING` for one not bound.
#[test]
fn an_extension_method_whose_arguments_do_not_match() {
    let e = call_env();
    let fx = "rates.example.org/fx_pair(EUR, USD)";
    for (src, want) in [
        (format!("{fx}.calc.example.net/price_of(2026-03-16)"), "E_ARG_UNKNOWN"),
        (format!("{fx}.calc.example.net/price_of(instrument: 1)"), "E_ARG_UNKNOWN"),
        (format!("{fx}.calc.example.net/fx_extra/carry(2026-03-16, foo: 1)"), "E_ARG_UNKNOWN"),
        (format!("{fx}.calc.example.net/fx_extra/carry()"), "E_ARG_MISSING"),
        (format!("{fx}.calc.example.net/fx_ndf/named(cable)"), "E_ARG_UNKNOWN"),
    ] {
        assert_eq!(call_code(&src, &e), want, "{src}");
    }
    let ok = parse_call(&format!("{fx}.calc.example.net/fx_extra/carry(2026-03-16)"), &e).unwrap();
    assert_eq!(ok.uri(), "cell://calc.example.net/fx_extra/carry?base=EUR&date=2026-03-16&quote=USD");
}

// ---------------------------------------------------------------------------------------------
// I3 (16.1, 16.3, 10.2): the parent's name for `super`.

const NDF_SIGNATURE: &str = r#"{"args": {"base": {"type": "string", "valid": false, "key_normalisation": "upper_ascii"},
    "date": {"type": "date", "valid": true}, "quote": {"type": "string", "valid": false, "key_normalisation": "upper_ascii"}},
    "order": ["base", "quote", "date"], "returns": {"type": "number", "unit": {"of": {"arg": "quote"}, "per": {"arg": "base"}}}}"#;
const PARENT_RATE: &str = "530bf86365ff4afb5a70196542d45f293fad5909026e015eddcfd866290f85b8";
const PARENT: &str = "rates.example.org/fx_pair";
const METHODS: [&str; 4] = ["rate", "cross", "spot", "forward"];

fn ndf_env() -> Env {
    class_env("calc.example.net/fx_ndf/rate", NDF_SIGNATURE, Some("calc.example.net/fx_ndf"))
}

/// The environment with the parent class (and its methods' functions) bound under `names` only.
fn with_parent_names(names: &[&str]) -> Env {
    let mut env = ndf_env();
    let descriptor = env.classes.get(PARENT).unwrap().clone();
    let functions: Vec<(String, Value)> = METHODS
        .iter()
        .filter_map(|m| env.functions.get(&format!("{PARENT}/{m}")).map(|f| (m.to_string(), f.value.clone())))
        .collect();
    let mut classes = datumesh_formula::Classes::new();
    for n in env.classes.names().filter(|n| *n != PARENT) {
        classes.insert(n, env.classes.get(n).unwrap().clone());
    }
    for n in names {
        classes.insert(n, descriptor.clone());
        for (m, f) in &functions {
            env.add_function(&format!("{n}/{m}"), f).unwrap();
        }
    }
    env.classes = classes;
    env
}

/// The name printed for `super` is the class name `parent`'s hash is bound to in the definer's
/// environment: the first in ascending order of name, should several be (the hash stays the
/// parent's).
#[test]
fn the_parents_name_is_the_name_its_hash_is_bound_to() {
    let read = |env: &Env| define("super.rate(date)", env).unwrap_or_else(|e| panic!("{e}"));
    let c = read(&ndf_env());
    assert_eq!(c.inputs, [format!("{PARENT}/rate")]);
    assert!(c.body.starts_with(&format!("{PARENT}/rate@{PARENT_RATE}(")), "{}", c.body);
    // Bound under another name only.
    let c = read(&with_parent_names(&["a.example.org/first"]));
    assert_eq!(c.inputs, ["a.example.org/first/rate"]);
    assert!(c.body.starts_with(&format!("a.example.org/first/rate@{PARENT_RATE}(")), "{}", c.body);
    // Under several: the first in ascending order of name, whichever the others.
    let c = read(&with_parent_names(&[PARENT, "a.example.org/first"]));
    assert!(c.body.starts_with(&format!("a.example.org/first/rate@{PARENT_RATE}(")), "{}", c.body);
    let c = read(&with_parent_names(&[PARENT, "z.example.org/last"]));
    assert!(c.body.starts_with(&format!("{PARENT}/rate@{PARENT_RATE}(")), "{}", c.body);
}

/// No name bound to `parent`'s hash in the definer's environment: the method's define is
/// `E_CLASS_PARENT` (10.2), whatever `super` reads; `E_FORMULA_METHOD` stays for a class without
/// `parent` (`invalid/formula-class-super-without-parent`).
#[test]
fn no_name_for_the_parent_is_a_class_parent_error() {
    let none = with_parent_names(&[]);
    for src in ["super.rate(date)", "super.points(date)"] {
        assert_eq!(define(src, &none).unwrap_err().code.as_str(), "E_CLASS_PARENT", "{src}");
    }
    let fx = class_env("rates.example.org/fx_pair/spot", NDF_SIGNATURE, Some(PARENT));
    assert_eq!(define("super.rate(date)", &fx).unwrap_err().code.as_str(), "E_FORMULA_METHOD");
}

// ---------------------------------------------------------------------------------------------
// H10 (15.6): a printed dispatch needs the function's signature to parse back.

/// parse(print(ast)) = ast for a dispatch when the signature is the environment; the other nodes
/// of a method call (the plain read it is sugar for) round-trip without one.
#[test]
fn a_printed_dispatch_parses_back_with_its_signature() {
    let sig = r#"{"args": {"date": {"type": "date", "valid": true}, "instrument": {"type": "string", "classes": ["calc.example.net/bond", "calc.example.net/swap"], "valid": false}},
        "order": ["instrument", "date"], "returns": {"type": "number", "unit": "1"}}"#;
    let env = class_env("calc.example.net/e2", sig, None);
    let c = define("instrument.price(date: date)", &env).unwrap();
    assert_eq!(c.body, "instrument.price(date: date)\n");
    let back = define(&c.body, &env).unwrap();
    assert_eq!(back.ast, c.ast);
    assert_eq!(back.ast.hash(), c.ast.hash());
    let plain = define(
        "rates.example.org/fx_pair(EUR, USD).rate(2026-03-16)",
        &class_env("calc.example.net/e", &no_args(FX), None),
    )
    .unwrap();
    let again = define(&plain.body, &class_env("calc.example.net/e", &no_args(FX), None)).unwrap();
    assert_eq!(again.ast, plain.ast);
}

// H5, I1 and I2 (`resolved`) are pinned in `classes_requests.rs`: `bare_names_resolve_through_the_alias_map`
// and `every_alias_is_recorded` (alias `from`, `aliases_tx`, the pinned call), and the method
// resolution's `from` there and in the `call` vectors; H6 in `the_order_of_checks`.
