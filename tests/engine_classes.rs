//! Classes through the engine (feature `engine`, contract item 7): the class vectors of
//! spec-0.5.0 evaluated by the engine with the formula runtime on PostgreSQL, as
//! `engine_vectors.rs` does the older ones (`common/engine.rs`).
//!
//! Group 1 (methods): `this` read one way, the `asof` default and the computed-key fault. The
//! methods' functions are plain functions in the store, as the vectors list them, so this group
//! does not need classes in the store.
//!
//! Groups 2 and 3 (the next tests commits) need datumesh-store 0.4.1's `put_class`, `bind_class` and
//! `resolve_class` (CONTRACT-store-0.4.1-classes.md), which are `todo!()` stubs on the branch
//! `step-1.5-namespace-validation` of `/root/datumesh-libs/datumesh-store` at the time of writing:
//! their tests are `#[ignore]`d "for later" (run them with `-- --ignored` once the store is ready, and
//! remove the attribute).
#![cfg(feature = "engine")]

mod common;
#[path = "common/engine.rs"]
mod engine;
#[path = "common/pg.rs"]
mod pg;

use common::classes::methods_eval;
use datumesh_formula::call::parse_call;

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
#[ignore = "needs datumesh-store 0.4.1 (put_class, bind_class): todo!() stubs on the branch at the time of writing"]
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
