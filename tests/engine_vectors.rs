//! Every `formula-eval` and `excel` vector evaluated by the engine with the formula runtime on
//! PostgreSQL: the vector's world in a database of its own (its descriptors bound from their
//! `bound`, its stored receipts with their versions), the cell requested as of the vector's
//! asof from an engine signing as `mesh.example.net` with the vectors' conventions. A valid
//! vector gives the vector's value, lineage, `tx`, receipt `asof` and receipt hash, and its
//! normative fuel is the vector's: the request fails with `BudgetExceeded` at `fuel_used - 1`
//! and succeeds at `fuel_used`. An invalid one answers as SPEC 15.12 says: a fault is
//! `not_covered` with the fault, `E_FORMULA_NOT_COVERED` plain `not_covered`, budget, cycle and
//! depth the engine's errors.
//!
//! `issued` is the request's clock reading; the vectors' is the later of the receipt's `asof`
//! and 2020-01-01, so the clock is set to that. A vector whose cell reads computed cells has
//! those evaluated first, each by a request of its own with its own `issued` (in ascending
//! receipt `asof`), as the in-memory world evaluated them; its fuel is checked in a second
//! database, where nothing is evaluated yet.
#![cfg(feature = "engine")]

mod common;
#[path = "common/engine.rs"]
mod engine;
#[path = "common/pg.rs"]
mod pg;

#[test]
fn formula_eval_vectors_through_the_engine() {
    let n = engine::check_kind(common::old_vectors_of_kind("formula-eval"), false, "formula-eval");
    eprintln!("formula-eval: {n} vectors through the engine");
}

#[test]
fn excel_vectors_through_the_engine() {
    let n = engine::check_kind(common::old_vectors_of_kind("excel"), false, "excel");
    eprintln!("excel: {n} vectors through the engine");
}
