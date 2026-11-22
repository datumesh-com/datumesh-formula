//! Formula reads of withdrawn cells (CONTRACT-round-0.7 R1; spec-0.7.0, SPEC 8.1, 15.11-15.13).
//!
//! A stored cell's version may be a *withdrawal*: a receipt with `withdrawn` (a reason) in place of
//! `type` and `value`. From its `asof` on the cell has no value, until a later version with a value
//! ends it. In a formula:
//!
//! - a stored input read alone, whose latest version as of the evaluation asof is a withdrawal, has
//!   no value: `E_FORMULA_NOT_COVERED`, which `otherwise` does not catch (15.11, 15.12);
//! - `at(slice, d)` on a withdrawn member is `E_FORMULA_ABSENT` (not a member, so none qualifies),
//!   which `otherwise` catches too;
//! - a withdrawn cell is not a member of a slice: `count`, `sum` and the empty slice see 9 of 10.
//!
//! # Assumed API
//!
//! None is added. The evaluator's `World` already says "this cell has no value" (`read` returns
//! `Ok(None)`, `members` leaves the cell out), and the in-memory world is fed the stored receipts of
//! a vector's `world.receipts`. The one thing assumed is that `MemoryWorld::add_receipt` (and so
//! `MemoryWorld::from_value`) accepts a *withdrawal receipt*, the wire form of SPEC 8.1 (`withdrawn`
//! and no `type` or `value`), and reads the latest version as of the asof (by the receipt's `asof`),
//! a withdrawal being a version like any other. Today it answers `malformed input: type`.
//!
//! The same cases through `FormulaRuntime` over a store or the engine's `Inputs` depend on the
//! store and engine presenting a withdrawal as no value; they are tested where that is built.

mod common;

use datumesh_core::json::{self, Value};
use datumesh_core::Scalar;
use datumesh_formula::eval::{Args, CellKey};
use datumesh_formula::memory::{Computed, MemoryWorld};

const PRICE_HASH: &str = "4193e310f5c949998b3f339d2ad56567286ebd0828504b3f31a2c671e99c71a7";
const DAILY_HASH: &str = "e18ab921761089909a83eacfd7e88b71a580da2d35610769557f035fdb2b8169";
const CALC: &str = "calc.example.net/e";

fn parse(s: &str) -> Value {
    json::parse(s.as_bytes()).unwrap_or_else(|e| panic!("{s}: {e:?}"))
}

const PRICE_DESCRIPTOR: &str = r#"{"code":{"body":"","lang":"stored"},"inputs":[],"signature":{"args":{"item":{"type":"string","valid":false}},"order":["item"],"returns":{"type":"number","unit":"EUR"}}}"#;
const DAILY_DESCRIPTOR: &str = r#"{"code":{"body":"","lang":"stored"},"inputs":[],"signature":{"args":{"date":{"type":"date","valid":true}},"order":["date"],"returns":{"type":"number","unit":"EUR"}}}"#;

/// A calc function `calc.example.net/e(s: string | d: date)` with this body, over the stored
/// `price(item)` and `daily(date)`, evaluating as of `asof`.
fn world(body: &str, arg: &str, asof: &str) -> MemoryWorld {
    let (name, ty, valid) = if arg == "s" { ("s", "string", "false") } else { ("d", "date", "true") };
    let inputs = if body.contains("price") { r#""data.example.org/price""# } else { r#""data.example.org/daily""# };
    let unit = if body.starts_with("count") { "count" } else { "EUR" };
    let calc = format!(
        r#"{{"code":{{"body":{},"lang":"formula"}},"inputs":[{inputs}],"signature":{{"args":{{"{name}":{{"type":"{ty}","valid":{valid}}}}},"order":["{name}"],"returns":{{"type":"number","unit":"{unit}"}}}}}}"#,
        String::from_utf8(Value::String(format!("{body}\n")).to_jcs()).unwrap()
    );
    let mut w = MemoryWorld::new(asof);
    let z = "2026-01-01T00:00:00.000000Z";
    w.bind("data.example.org/price", &parse(PRICE_DESCRIPTOR), z).unwrap();
    w.bind("data.example.org/daily", &parse(DAILY_DESCRIPTOR), z).unwrap();
    w.bind(CALC, &parse(&calc), z).unwrap();
    w
}

/// A stored receipt (unsigned: the world does not verify) of `price(item)` with a value.
fn price(w: &mut MemoryWorld, item: &str, value: &str, at: &str) {
    stored(w, "price", PRICE_HASH, "item", "string", item, "{}", Some(value), None, at);
}

/// A withdrawal receipt of `price(item)`.
fn price_withdrawn(w: &mut MemoryWorld, item: &str, reason: &str, at: &str) {
    stored(w, "price", PRICE_HASH, "item", "string", item, "{}", None, Some(reason), at);
}

fn daily(w: &mut MemoryWorld, date: &str, value: &str, at: &str) {
    let valid = format!(r#"{{"date":{{"type":"date","value":"{date}"}}}}"#);
    stored(w, "daily", DAILY_HASH, "date", "date", date, &valid, Some(value), None, at);
}

fn daily_withdrawn(w: &mut MemoryWorld, date: &str, reason: &str, at: &str) {
    let valid = format!(r#"{{"date":{{"type":"date","value":"{date}"}}}}"#);
    stored(w, "daily", DAILY_HASH, "date", "date", date, &valid, None, Some(reason), at);
}

#[allow(clippy::too_many_arguments)]
fn stored(
    w: &mut MemoryWorld,
    f: &str,
    fn_hash: &str,
    arg: &str,
    ty: &str,
    key: &str,
    valid: &str,
    value: Option<&str>,
    withdrawn: Option<&str>,
    at: &str,
) {
    let outcome = match (value, withdrawn) {
        (Some(v), None) => format!(r#""type":"number","value":"{v}""#),
        (None, Some(r)) => {
            format!(r#""withdrawn":{}"#, String::from_utf8(Value::String(r.to_string()).to_jcs()).unwrap())
        }
        _ => unreachable!(),
    };
    let r = format!(
        r#"{{"asof":"{at}","cell":{{"args":{{"{arg}":{{"type":"{ty}","value":"{key}"}}}},"fn":"{f}","fn_hash":"{fn_hash}","ns":"data.example.org"}},"computed_by":"data.example.org#efb20f82a7e15e96","inputs":[],"issued":"{at}","licence":"CC-BY-4.0","proto":"datumesh/1","publisher":"data.example.org","summary":"A version of {f}.","tx":"{at}",{outcome},"valid":{valid},"sig":"abababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababab"}}"#
    );
    w.add_receipt(&parse(&r)).unwrap_or_else(|e| panic!("add_receipt: {e}: {r}"));
}

fn eval(w: &mut MemoryWorld, arg: &str, value: Scalar, asof: &str) -> Result<Computed, String> {
    let mut args = Args::new();
    args.insert(arg.to_string(), value);
    w.compute(&CellKey { name: CALC.to_string(), args }, asof).map_err(|c| c.as_str().to_string())
}

fn number(c: &Computed) -> String {
    match c.outcome.value.to_value().get("value") {
        Some(Value::String(s)) => s.clone(),
        other => panic!("not a number: {other:?}"),
    }
}

fn s(x: &str) -> Scalar {
    Scalar::String(x.to_string())
}

fn d(x: &str) -> Scalar {
    Scalar::Date(x.to_string())
}

const ASOF: &str = "2026-06-01T00:00:00.000000Z";

/// bolt: 2.5, then 2.75 (Apr), then withdrawn (1 May).
fn bolt(body: &str) -> MemoryWorld {
    let mut w = world(body, "s", ASOF);
    price(&mut w, "bolt", "2.5", "2026-02-01T09:00:00.000000Z");
    price(&mut w, "bolt", "2.75", "2026-04-01T09:00:00.000000Z");
    price_withdrawn(
        &mut w,
        "bolt",
        "The price was withdrawn: the item is no longer sold.",
        "2026-05-01T09:00:00.000000Z",
    );
    w
}

/// daily readings 1..=10 March (10, 12, 9, 15, 11, 13, 14, 8, 16, 10 = 118); 4 March (15) is
/// withdrawn on 20 March.
fn readings(body: &str, arg: &str, asof: &str) -> MemoryWorld {
    let mut w = world(body, arg, asof);
    for (i, v) in [10, 12, 9, 15, 11, 13, 14, 8, 16, 10].iter().enumerate() {
        daily(
            &mut w,
            &format!("2026-03-{:02}", i + 1),
            &v.to_string(),
            &format!("2026-03-{:02}T18:00:00.000000Z", i + 1),
        );
    }
    daily_withdrawn(
        &mut w,
        "2026-03-04",
        "The reading of 4 March was withdrawn as a sensor fault.",
        "2026-03-20T18:00:00.000000Z",
    );
    w
}

// ---- a withdrawn input read alone ------------------------------------------------------------

#[test]
fn withdrawn_input_read_alone_is_not_covered() {
    let mut w = bolt("data.example.org/price(item: s)");
    assert_eq!(eval(&mut w, "s", s("bolt"), ASOF).err().as_deref(), Some("E_FORMULA_NOT_COVERED"));
}

#[test]
fn withdrawn_input_read_alone_is_not_caught_by_otherwise() {
    // A single-cell read with no value cannot be caught (15.12: a receipt cannot list an absence);
    // `otherwise` catches only E_FORMULA_EMPTY and E_FORMULA_ABSENT.
    let mut w = bolt("otherwise(data.example.org/price(item: s), -1)");
    assert_eq!(eval(&mut w, "s", s("bolt"), ASOF).err().as_deref(), Some("E_FORMULA_NOT_COVERED"));
}

#[test]
fn a_withdrawal_as_the_only_version_is_not_covered() {
    let mut w = world("data.example.org/price(item: s)", "s", ASOF);
    price_withdrawn(&mut w, "nut", "never sold", "2026-02-01T09:00:00.000000Z");
    assert_eq!(eval(&mut w, "s", s("nut"), ASOF).err().as_deref(), Some("E_FORMULA_NOT_COVERED"));
}

#[test]
fn a_withdrawal_after_the_asof_is_not_yet_the_latest_version() {
    // 2026-04-15: the withdrawal of 1 May is not known yet; the value 2.75 stands (15.12).
    let mut w = bolt("data.example.org/price(item: s)");
    let c = eval(&mut w, "s", s("bolt"), "2026-04-15T00:00:00.000000Z").unwrap();
    assert_eq!(number(&c), "2.75");
    assert_eq!(c.outcome.tx.as_deref(), Some("2026-04-01T09:00:00.000000Z"));
}

#[test]
fn a_later_valued_version_ends_the_withdrawal() {
    let mut w = bolt("data.example.org/price(item: s)");
    price(&mut w, "bolt", "3", "2026-05-20T09:00:00.000000Z");
    let c = eval(&mut w, "s", s("bolt"), ASOF).unwrap();
    assert_eq!(number(&c), "3");
    assert_eq!(c.outcome.tx.as_deref(), Some("2026-05-20T09:00:00.000000Z"));
    assert_eq!(c.outcome.receipt_asof, "2026-05-20T09:00:00.000000Z");
    // Between the withdrawal and the new value the cell has none.
    let mut w2 = bolt("data.example.org/price(item: s)");
    price(&mut w2, "bolt", "3", "2026-05-20T09:00:00.000000Z");
    assert_eq!(
        eval(&mut w2, "s", s("bolt"), "2026-05-10T00:00:00.000000Z").err().as_deref(),
        Some("E_FORMULA_NOT_COVERED")
    );
}

#[test]
fn a_withdrawal_after_the_ending_version_is_latest_again() {
    let mut w = bolt("data.example.org/price(item: s)");
    price(&mut w, "bolt", "3", "2026-05-20T09:00:00.000000Z");
    price_withdrawn(&mut w, "bolt", "withdrawn again", "2026-05-25T09:00:00.000000Z");
    assert_eq!(eval(&mut w, "s", s("bolt"), ASOF).err().as_deref(), Some("E_FORMULA_NOT_COVERED"));
}

#[test]
fn a_withdrawal_leaves_other_cells_of_the_function() {
    let mut w = bolt("data.example.org/price(item: s)");
    price(&mut w, "nut", "0.25", "2026-02-01T09:00:00.000000Z");
    let c = eval(&mut w, "s", s("nut"), ASOF).unwrap();
    assert_eq!(number(&c), "0.25");
}

// ---- at(slice, d) ----------------------------------------------------------------------------

#[test]
fn at_on_a_withdrawn_member_is_absent() {
    let mut w = readings("at(data.example.org/daily(date: *), d)", "d", ASOF);
    assert_eq!(eval(&mut w, "d", d("2026-03-04"), ASOF).err().as_deref(), Some("E_FORMULA_ABSENT"));
}

#[test]
fn at_on_a_withdrawn_member_is_caught_by_otherwise() {
    let mut w = readings("otherwise(at(data.example.org/daily(date: *), d), -1)", "d", ASOF);
    let c = eval(&mut w, "d", d("2026-03-04"), ASOF).unwrap();
    assert_eq!(number(&c), "-1");
}

#[test]
fn at_on_another_member_is_unchanged_by_the_withdrawal() {
    let mut w = readings("at(data.example.org/daily(date: *), d)", "d", ASOF);
    let c = eval(&mut w, "d", d("2026-03-05"), ASOF).unwrap();
    assert_eq!(number(&c), "11");
}

#[test]
fn at_on_the_reading_before_its_withdrawal_is_the_value() {
    let asof = "2026-03-10T18:00:00.000000Z";
    let mut w = readings("at(data.example.org/daily(date: *), d)", "d", asof);
    let c = eval(&mut w, "d", d("2026-03-04"), asof).unwrap();
    assert_eq!(number(&c), "15");
}

// ---- a withdrawn cell is not a slice member --------------------------------------------------

#[test]
fn count_leaves_out_a_withdrawn_cell() {
    let mut w = readings("count(data.example.org/daily(date: *))", "d", ASOF);
    assert_eq!(number(&eval(&mut w, "d", d("2026-03-04"), ASOF).unwrap()), "9");
}

#[test]
fn count_before_the_withdrawal_counts_it() {
    let asof = "2026-03-10T18:00:00.000000Z";
    let mut w = readings("count(data.example.org/daily(date: *))", "d", asof);
    assert_eq!(number(&eval(&mut w, "d", d("2026-03-04"), asof).unwrap()), "10");
}

#[test]
fn sum_leaves_out_a_withdrawn_cell() {
    // 118 - 15.
    let mut w = readings("sum(data.example.org/daily(date: *))", "d", ASOF);
    assert_eq!(number(&eval(&mut w, "d", d("2026-03-04"), ASOF).unwrap()), "103");
}

#[test]
fn the_slice_lineage_names_the_members_without_the_withdrawn_cell() {
    // The slice entry's members hash is of the 9 members: it differs from the 10 of an earlier asof.
    let mut w = readings("count(data.example.org/daily(date: *))", "d", ASOF);
    let after = eval(&mut w, "d", d("2026-03-04"), ASOF).unwrap().outcome.lineage;
    let asof = "2026-03-10T18:00:00.000000Z";
    let mut w = readings("count(data.example.org/daily(date: *))", "d", asof);
    let before = eval(&mut w, "d", d("2026-03-04"), asof).unwrap().outcome.lineage;
    assert_eq!(after.len(), 1);
    assert_eq!(before.len(), 1);
    // Same slice id, different members hash.
    let (a, b) = (after[0].split(':').collect::<Vec<_>>(), before[0].split(':').collect::<Vec<_>>());
    assert_eq!(a[1], b[1], "the slice is the same");
    assert_ne!(a[2], b[2], "the members are not");
}

#[test]
fn a_slice_whose_only_cell_is_withdrawn_is_empty() {
    let mut w = world("count(data.example.org/daily(date: *))", "d", ASOF);
    daily(&mut w, "2026-03-01", "10", "2026-03-01T18:00:00.000000Z");
    daily_withdrawn(&mut w, "2026-03-01", "gone", "2026-03-02T18:00:00.000000Z");
    assert_eq!(number(&eval(&mut w, "d", d("2026-03-01"), ASOF).unwrap()), "0");

    let mut w = world("sum(data.example.org/daily(date: *))", "d", ASOF);
    daily(&mut w, "2026-03-01", "10", "2026-03-01T18:00:00.000000Z");
    daily_withdrawn(&mut w, "2026-03-01", "gone", "2026-03-02T18:00:00.000000Z");
    assert_eq!(number(&eval(&mut w, "d", d("2026-03-01"), ASOF).unwrap()), "0");

    // Where an aggregate needs a member, the slice is empty: E_FORMULA_EMPTY, catchable.
    let mut w = world("mean(data.example.org/daily(date: *))", "d", ASOF);
    daily(&mut w, "2026-03-01", "10", "2026-03-01T18:00:00.000000Z");
    daily_withdrawn(&mut w, "2026-03-01", "gone", "2026-03-02T18:00:00.000000Z");
    assert_eq!(eval(&mut w, "d", d("2026-03-01"), ASOF).err().as_deref(), Some("E_FORMULA_EMPTY"));

    let mut w = world("otherwise(mean(data.example.org/daily(date: *)), 0)", "d", ASOF);
    daily(&mut w, "2026-03-01", "10", "2026-03-01T18:00:00.000000Z");
    daily_withdrawn(&mut w, "2026-03-01", "gone", "2026-03-02T18:00:00.000000Z");
    assert_eq!(number(&eval(&mut w, "d", d("2026-03-01"), ASOF).unwrap()), "0");
}

#[test]
fn a_withdrawn_cell_ended_by_a_later_version_is_a_member_again() {
    let mut w = readings("count(data.example.org/daily(date: *))", "d", ASOF);
    daily(&mut w, "2026-03-04", "16", "2026-04-01T18:00:00.000000Z");
    assert_eq!(number(&eval(&mut w, "d", d("2026-03-04"), ASOF).unwrap()), "10");

    let mut w = readings("sum(data.example.org/daily(date: *))", "d", ASOF);
    daily(&mut w, "2026-03-04", "16", "2026-04-01T18:00:00.000000Z");
    assert_eq!(number(&eval(&mut w, "d", d("2026-03-04"), ASOF).unwrap()), "119");
}

// ---- the spec's vectors ----------------------------------------------------------------------

/// Every `withdrawn-*` formula-eval vector of spec-0.7.0 (value, lineage, tx, receipt asof, fuel and
/// receipt hash; or the code), named, so that a failure says which.
#[test]
fn the_withdrawn_formula_eval_vectors() {
    let mut seen = 0;
    let mut failures = Vec::new();
    for (path, v) in common::vectors_of_kind("formula-eval") {
        let name = path.rsplit('/').next().unwrap_or(&path);
        if !(name.starts_with("withdrawn-") || name.starts_with("formula-eval-withdrawn-")) {
            continue;
        }
        seen += 1;
        if let Err(e) = common::check::eval_vector("formula-eval", &path, &v) {
            failures.push(e);
        }
    }
    assert_eq!(seen, 9, "7 valid and 2 invalid withdrawn vectors");
    assert!(failures.is_empty(), "{failures:#?}");
}
