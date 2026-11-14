//! Classes through the engine (feature `engine`, contract item 7): the class vectors of
//! spec-0.5.0 evaluated by the engine with the formula runtime on PostgreSQL, as
//! `engine_vectors.rs` does the older ones (`common/engine.rs`).
//!
//! Group 1 (methods): `this` read one way, the `asof` default and the computed-key fault. The
//! methods' functions are plain functions in the store, as the vectors list them, so this group
//! does not need classes in the store.
//!
//! Groups 2 and 3 need datumesh-store 0.4.1's `put_class`, `bind_class` and `resolve_class`
//! (CONTRACT-store-0.4.1-classes.md), which are `todo!()` stubs on the branch
//! `step-1.5-namespace-validation` of `/root/datumesh-libs/datumesh-store` at the time of writing:
//! those tests are `#[ignore]`d "for later" (run them with `-- --ignored` once the store is
//! ready, and remove the attribute).
#![cfg(feature = "engine")]

mod common;
#[path = "common/engine.rs"]
mod engine;
#[path = "common/pg.rs"]
mod pg;

use std::collections::BTreeMap;

use common::classes::{dispatch_eval, methods_eval};
use datumesh_engine::Answer;
use datumesh_formula::call::parse_call;
use datumesh_formula::definition::{define_class, define_function, publish, publish_class};
use datumesh_store::Store;
use pg::{cell_ref, date, text, Mesh};

/// `class-spot`, `class-forward`, `class-default-asof` and the key fault: the same value, lineage,
/// `tx`, receipt `asof`, fuel and receipt hash through the engine as in memory, and the fault
/// `E_FORMULA_KEY` answered `not_covered` with it.
#[test]
fn the_methods_group_through_the_engine() {
    let n = engine::check_kind(methods_eval(), false, "class formula-eval (methods group)");
    assert_eq!(n, 4);
}

// ---------------------------------------------------------------------------------------------
// Group 2: requests against a store.

/// A store is a `CallEnv` for classes: its class bindings (`resolve_class_binding`) give a method
/// call its cell, as its function bindings give a plain call (5.8, 16.3); the store has no
/// lookup hook (reading a cell is the mesh's), so a label is `E_NAME_UNBOUND`.
#[test]
fn a_store_answers_class_calls() {
    let v = common::vector("formula-eval/class-forward.json");
    let mesh = engine::world_mesh(&v, true);
    let store = &*mesh.store;
    let c = parse_call("rates.example.org/fx_pair(EUR, USD).rate(2026-03-16)", store).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(c.uri(), "cell://rates.example.org/fx_pair/rate?base=EUR&date=2026-03-16&quote=USD");
    let plain = parse_call("rates.example.org/fx_pair/rate(EUR, USD, 2026-03-16)", store).unwrap();
    assert_eq!(c.cell, plain.cell);
    assert_eq!(
        parse_call("rates.example.org/fx_pair[cable].rate(2026-03-16)?asof=2026-06-01T00:00:00.000000Z", store)
            .unwrap_err()
            .code(),
        "E_NAME_UNBOUND"
    );
    assert_eq!(
        parse_call("rates.example.org/fx_pair(eur, USD).rate(2026-03-16)", store).unwrap_err().code(),
        "E_KEY_NORMALISATION"
    );
}

// ---------------------------------------------------------------------------------------------
// Group 3: dispatch through the engine; defining and publishing classes.

/// The dispatch-group vectors of the spec, `class-super` included, through the engine with the
/// classes put and bound in the store: the engine answers what the in-memory world does (value,
/// lineage, receipt hash under SPEC 11's licence convention, fuel), a key outside the set as
/// `not_covered` with `E_FORMULA_DISPATCH_KEY`. `class-super` needs `Inputs::read_pinned`
/// (datumesh-engine 0.4.2, SPEC-FINDINGS H8); formula's runtime answers `E_FORMULA_UNSUPPORTED`
/// for a pinned read until it uses it.
#[test]
fn the_dispatch_group_through_the_engine() {
    let vectors: Vec<_> =
        dispatch_eval().into_iter().filter(|(p, _)| !NOT_LOADABLE_THROUGH_A_STORE.contains(&p.as_str())).collect();
    let n = engine::check_kind(vectors, true, "class formula-eval (dispatch group)");
    assert_eq!(n, 6);
}

/// A vector whose world cannot be loaded through a store: the class `bond` names the method
/// hash `cfd19943...` for `price`, but the world lists no descriptor with that hash (the function
/// `bond/price` is bound to another descriptor, which is the vector's point), and `bind_class`
/// needs the descriptor of every method it binds (`NotFound { what: "descriptor" }`). Reported as a
/// spec finding; the in-memory runner runs it (`classes_dispatch`).
const NOT_LOADABLE_THROUGH_A_STORE: &[&str] = &["invalid/formula-eval-class-dispatch-rebound.json"];

/// The local class vectors (`tests/vectors-local`) through the engine.
#[test]
#[ignore = "the local vectors' receipts are signed by a key id the mesh does not publish (SPEC-FINDINGS I5)"]
fn the_local_class_vectors_through_the_engine() {
    let n = engine::check_kind(common::local_vectors(), true, "local class vectors");
    assert_eq!(n, 11);
}

/// `formula-eval/class-super` through the engine, one vector alone: `fx_ndf/rate` reads
/// `fx_pair/rate` pinned to the parent's function hash (`read_pinned`). The value `1.0892`, the
/// lineage (the pinned receipt, `5a98aec5...`) and the receipt hash `a28b2591...` (under SPEC 11's
/// convention) are the vector's, not an `E_FORMULA_UNSUPPORTED` fault.
#[test]
fn super_through_the_engine() {
    let v = common::vector("formula-eval/class-super.json");
    let mesh = engine::world_mesh(&v, true);
    mesh.clock.set("2026-06-01T00:00:00.000000Z");
    let (engine_, _rt) = mesh.engine();
    let cell = cell_ref(
        "calc.example.net/fx_ndf/rate",
        &[("base", text("EUR")), ("quote", text("USD")), ("date", date("2026-03-16"))],
    );
    match engine_.get(&cell, Some("2026-06-01T00:00:00.000000Z")).unwrap() {
        Answer::Receipt(bytes) => {
            let r = datumesh_core::Receipt::from_json(&bytes).unwrap();
            assert_eq!(r.value().lexical(), "1.0892");
            assert_eq!(r.inputs(), ["5a98aec504eb80a27ef6bcc82775b3b58a41aaa7ea090828912100f93ed745ca"]);
            assert_eq!(r.tx(), "2026-03-16T16:00:00.000000Z");
            assert_eq!(r.asof(), "2026-03-16T16:00:00.000000Z");
            assert_eq!(
                engine::convention_hash(&bytes, &BTreeMap::new(), &*mesh.store),
                "a28b25916e8c7ae34cb12d3bfc37613f4e0658cb80f0827a705dab8232b36a4d"
            );
        }
        other => panic!("{other:?}"),
    }
}

/// A pinned read relies on no binding (16.3, 8.2 rule 2): with `rates.example.org/fx_pair/rate`
/// rebound to another function before the asof, `fx_ndf/rate` answers the same value, lineage and
/// receipt hash, since `super` reads the function stored under the parent's hash, not what `P/m`
/// is bound to.
#[test]
fn super_relies_on_no_binding_through_the_engine() {
    let v = common::vector("formula-eval/class-super.json");
    let mesh = engine::world_mesh(&v, true);
    let other = v
        .get("world")
        .unwrap()
        .get("functions")
        .unwrap()
        .get("rates.example.org/fx_pair/spot")
        .unwrap()
        .get("descriptor")
        .unwrap();
    mesh.bind("rates.example.org/fx_pair/rate", &other.to_jcs(), "2026-05-01T00:00:00.000000Z");
    mesh.clock.set("2026-06-01T00:00:00.000000Z");
    let (engine_, _rt) = mesh.engine();
    let cell = cell_ref(
        "calc.example.net/fx_ndf/rate",
        &[("base", text("EUR")), ("quote", text("USD")), ("date", date("2026-03-16"))],
    );
    match engine_.get(&cell, Some("2026-06-01T00:00:00.000000Z")).unwrap() {
        Answer::Receipt(bytes) => {
            let r = datumesh_core::Receipt::from_json(&bytes).unwrap();
            assert_eq!(r.value().lexical(), "1.0892");
            assert_eq!(r.inputs(), ["5a98aec504eb80a27ef6bcc82775b3b58a41aaa7ea090828912100f93ed745ca"]);
            assert_eq!(r.asof(), "2026-03-16T16:00:00.000000Z");
            assert_eq!(
                engine::convention_hash(&bytes, &BTreeMap::new(), &*mesh.store),
                "a28b25916e8c7ae34cb12d3bfc37613f4e0658cb80f0827a705dab8232b36a4d"
            );
        }
        other => panic!("{other:?}"),
    }
}

/// Define and publish over a store: two classes defined from their descriptors (`define_class`
/// over the store as callee source, then `publish_class`: `put_class` and `bind_class`, which binds
/// each method `N/m` too), a function defined that dispatches on them (`define_function` reads the
/// classes from the store), and the engine answering it: the same function hash as the vectors'
/// `price_of`, the same lineage.
#[test]
fn define_publish_and_dispatch_over_a_store() {
    const T0: &str = "2026-01-01T00:00:00.000000Z";
    let v = common::vector("formula-eval/class-dispatch-bond.json");
    let world = v.get("world").unwrap();
    let mesh = Mesh::new(&["calc.example.net", "rates.example.org"]); // the world's stored receipts are signed by rates.example.org
    for name in ["calc.example.net/bond/price", "calc.example.net/swap/price"] {
        let d = world.get("functions").unwrap().get(name).unwrap().get("descriptor").unwrap();
        mesh.store.put_descriptor(&d.to_jcs()).unwrap();
    }
    for name in ["calc.example.net/bond", "calc.example.net/swap"] {
        let d = world.get("classes").unwrap().get(name).unwrap().get("descriptor").unwrap();
        let def = define_class(name, &d.to_jcs(), "calc.example.net", &*mesh.store, T0)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(def.bytes, datumesh_core::ClassDescriptor::from_value(d).unwrap().canonical());
        let hash = publish_class(&*mesh.store, &def, T0).unwrap();
        assert_eq!(mesh.store.resolve_class(name, T0).unwrap().as_deref(), Some(hash.as_str()));
        // Binding the class bound its method.
        assert!(mesh.store.resolve_name(&format!("{name}/price"), T0).unwrap().is_some());
    }
    let price_of = world.get("functions").unwrap().get("calc.example.net/price_of").unwrap().get("descriptor").unwrap();
    let sig = price_of.get("signature").unwrap();
    let d =
        define_function("calc.example.net/price_of", "instrument.price(date)", sig, &BTreeMap::new(), &*mesh.store, T0)
            .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d.descriptor.inputs, ["calc.example.net/bond/price", "calc.example.net/swap/price"]);
    assert_eq!(d.fn_hash, datumesh_core::sha256_hex(&price_of.to_jcs()));
    publish(&*mesh.store, &d, T0).unwrap();
    for r in common::array(world.get("receipts").unwrap()) {
        mesh.put_receipt_value(r);
    }
    mesh.clock.set("2026-06-01T00:00:00.000000Z");
    let (engine, _rt) = mesh.engine();
    let cell = cell_ref(
        "calc.example.net/price_of",
        &[("instrument", text("calc.example.net/bond(isin:\"XS0001\")")), ("date", date("2026-03-16"))],
    );
    match engine.get(&cell, Some("2026-06-01T00:00:00.000000Z")).unwrap() {
        Answer::Receipt(bytes) => {
            let r = datumesh_core::Receipt::from_json(&bytes).unwrap();
            assert_eq!(r.value().lexical(), "101.25");
            let want: Vec<&str> =
                common::array(v.get("lineage").unwrap()).iter().map(|x| x.as_str().unwrap()).collect();
            assert_eq!(r.inputs().iter().map(String::as_str).collect::<Vec<_>>(), want);
        }
        other => panic!("{other:?}"),
    }
}
