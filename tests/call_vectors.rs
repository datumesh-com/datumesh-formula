//! Every `call` vector (SPEC 5.8, 11): the call parsed against the vector's environment (its
//! functions bound by name, its alias map and the descriptors known by hash) gives the vector's
//! cell URI, or its code. A pinned read in a body is `E_FORMULA_PLACEMENT` (the `formula`
//! vectors).

mod common;

use datumesh_core::json::Value;
use datumesh_formula::call::{parse_call, CallEnv, MapCallEnv};

#[test]
fn every_call_vector() {
    // The vectors of spec-0.4.2: the class vectors of spec-0.5.0 are `classes_requests.rs`'s.
    let t = common::check::tally(common::old_vectors_of_kind("call"), common::check::call_vector);
    assert!(t.failures.is_empty(), "{}", t.failures.join("\n"));
    assert_eq!((t.valid, t.invalid), (10, 17));
}

/// The unpinned URI a call gives is the one URI of its cell reference (5.2): the core's parse
/// of it gives the same reference back.
#[test]
fn an_unpinned_call_is_its_uri() {
    for (path, v) in common::old_vectors_of_kind("call") {
        if common::boolean(&v, "valid") != Some(true) || common::string(&v, "uri").unwrap().contains("fn_hash=") {
            continue;
        }
        let env = MapCallEnv::from_value(v.get("env").unwrap()).unwrap();
        let c = parse_call(common::string(&v, "input").unwrap(), &env).unwrap();
        assert!(c.fn_hash.is_none());
        let name = format!("{}/{}", c.cell.ns, c.cell.fn_name);
        let d = env.bound(&name, c.cell.asof.as_deref()).unwrap().unwrap();
        let sig: Value = d.to_value().get("signature").unwrap().clone();
        let back = datumesh_core::parse_uri(&c.uri(), &sig).unwrap();
        assert_eq!(back, c.cell, "{path}");
    }
}

/// Calls are parsed without an engine: aliases, bindings and pins come from the environment.
#[test]
fn a_pinned_call_needs_the_descriptor_not_the_binding() {
    let v = common::vector("call/pinned-unbound.json");
    let env = MapCallEnv::from_value(v.get("env").unwrap()).unwrap();
    let c = parse_call(common::string(&v, "input").unwrap(), &env).unwrap();
    assert_eq!(c.cell.fn_name, "convert");
    assert_eq!(c.fn_hash.as_deref(), Some("014fc97eb3a442ab28a96d89131ccfd11c9eec8187595578c212b392d0702c71"));
    assert!(env.bound("calc.example.net/convert", None).unwrap().is_none());
    // Unpinned, the same call is unbound.
    let unpinned = common::string(&v, "input")
        .unwrap()
        .replace("@014fc97eb3a442ab28a96d89131ccfd11c9eec8187595578c212b392d0702c71", "");
    assert_eq!(parse_call(&unpinned, &env).unwrap_err().code(), "E_NAME_UNBOUND");
}

/// Every valid call's URI, pinned or not, round-trips through the core's `parse_reference`
/// (against the signature of the descriptor it names) to the call's reference.
#[test]
fn every_call_uri_is_a_reference() {
    for (path, v) in common::old_vectors_of_kind("call") {
        if common::boolean(&v, "valid") != Some(true) {
            continue;
        }
        let env = MapCallEnv::from_value(v.get("env").unwrap()).unwrap();
        let c = parse_call(common::string(&v, "input").unwrap(), &env).unwrap();
        let d = match &c.fn_hash {
            Some(h) => env.pinned(h).unwrap().unwrap(),
            None => env.bound(&format!("{}/{}", c.cell.ns, c.cell.fn_name), None).unwrap().unwrap(),
        };
        let sig: Value = d.to_value().get("signature").unwrap().clone();
        let back = datumesh_core::parse_reference(&c.uri(), &sig).unwrap();
        assert_eq!(back, c.reference(), "{path}");
        assert_eq!(datumesh_core::format_reference(&back), c.uri(), "{path}");
    }
}
