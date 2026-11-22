//! The classes round (CONTRACT-classes-round.md K5), through the engine: `FormulaRuntime::new` over a
//! store. `StoreCallees` (src/runtime.rs) must forward `class_name`, so that an inherited method of a
//! subclass's object resolves to `D/m`, the defining class's cell, as the in-memory world does.
//!
//! This test fails today because `StoreCallees` does not forward `class_name` (the `Callees` default is
//! `Ok(None)`), so the runtime cannot name D.
#![cfg(feature = "engine")]

mod common;
#[path = "common/pg.rs"]
mod pg;

use std::collections::BTreeMap;

use common::round::*;
use datumesh_core::json::Value;
use datumesh_core::{ClassDescriptor, ReceiptBuilder};
use datumesh_engine::Answer;
use datumesh_formula::definition::{define_class, define_function, publish, publish_class};
use datumesh_store::Store;
use pg::{args, cell_ref, date, key_for, num, receipt_json, text, Mesh};

const T0: &str = "2026-01-01T00:00:00.000000Z";
const T1: &str = "2026-03-16T16:00:00.000000Z";
const NOW: &str = "2026-06-01T00:00:00.000000Z";
const SUB0: &str = "calc.example.net/sub0";

/// A subclass of `par` with no key and no method of its own, as an author writes it (`define_class`
/// flattens it): everything it has is inherited.
fn sub0(w: &World) -> Value {
    let par = ClassDescriptor::from_value(&w.par).unwrap();
    v(&format!(
        r#"{{"keys": {{}}, "order": [], "parent": "{}", "methods": {{}},
        "steward": {{"namespace": "calc.example.net", "process": "https://calc.example.net/classes/change"}}}}"#,
        par.hash()
    ))
}

#[test]
fn k5_a_runtime_over_a_store_dispatches_to_an_inherited_method_as_the_defining_class() {
    let w = world();
    let mut mesh = Mesh::new(&["calc.example.net"]);
    mesh.add_host("calc.example.net");
    let store = &*mesh.store;
    for d in w.functions.values() {
        store.put_descriptor(&d.to_jcs()).unwrap();
    }
    for (name, d) in [(PAR, w.par.clone()), (SUB0, sub0(&w))] {
        let def =
            define_class(name, &d.to_jcs(), "calc.example.net", store, T0).unwrap_or_else(|e| panic!("{name}: {e}"));
        publish_class(store, &def, T0).unwrap();
    }
    let sig = dispatch_sig(&[SUB0]);
    for (m, f) in [("st", "calc.example.net/two_st"), ("co", "calc.example.net/two_co")] {
        let d = define_function(f, &format!("i.{m}(d)"), &v(&sig), &BTreeMap::new(), store, T0)
            .unwrap_or_else(|e| panic!("{f}: {e}"));
        assert_eq!(
            d.descriptor.inputs,
            [format!("{PAR}/{m}")],
            "{m}: the dispatch reads the defining class's function"
        );
        publish(store, &d, T0).unwrap();
    }
    // The stored cell of par/st: k = 1 on 2026-03-16 is 5.
    let st_hash = store.resolve_name(&format!("{PAR}/st"), T0).unwrap().expect("par/st is bound");
    let cell = datumesh_core::Cell {
        ns: PAR.into(),
        fn_name: "st".into(),
        fn_hash: st_hash,
        args: args(&[("k", num("1")), ("d", date("2026-03-16"))]),
    };
    let valid = args(&[("d", date("2026-03-16"))]);
    let r = ReceiptBuilder::new()
        .cell(cell.clone())
        .value(num("5"))
        .valid(valid)
        .asof(T1)
        .tx(T1)
        .issued(T1)
        .publisher(PAR)
        .signer_host("calc.example.net")
        .licence("CC-BY-4.0")
        .summary("A recorded value of st.")
        .sign(&key_for("calc.example.net"))
        .unwrap();
    let hash = store.put_receipt(&receipt_json(&r)).unwrap();
    store.put_version(&cell.id(), T1, &hash).unwrap();
    mesh.clock.set(NOW);
    let (engine, _rt) = mesh.engine();
    for f in ["two_st", "two_co"] {
        let c = cell_ref(
            &format!("calc.example.net/{f}"),
            &[("i", text("calc.example.net/sub0(k:1)")), ("d", date("2026-03-16"))],
        );
        match engine.get(&c, Some(NOW)).unwrap_or_else(|e| panic!("{f}: {e:?}")) {
            Answer::Receipt(bytes) => {
                let r = datumesh_core::Receipt::from_json(&bytes).unwrap();
                assert_eq!(r.value().lexical(), "5", "{f}");
                assert_eq!(r.inputs().len(), 1, "{f}: one input, the parent's cell: {:?}", r.inputs());
            }
            other => panic!("{f}: {other:?}"),
        }
    }
}
