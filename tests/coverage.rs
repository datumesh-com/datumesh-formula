//! Build 2 Part 3, E5 "Branch coverage": the additive API of datumesh-formula 0.4.1 with which
//! datumesh-http decides `E_TESTS_MISSING`. Tests first; these do not compile until the API
//! exists.
//!
//! The API under test (all additions; nothing existing changes):
//!
//! * `datumesh_formula::coverage::branches(&Ast) -> Vec<BranchPoint>`: the branch points of a
//!   body, ascending by `id` (the arena index of the node, which a parse and a re-parse of the
//!   canonical body give alike). `BranchPoint { id: NodeId, kind: BranchKind, arms: usize }`;
//!   `BranchKind::{If, Otherwise}`. The language has no `choose` form: a chain of `if`s is what
//!   a choose is, each `if` is a point of two arms. Arms: `if` 0 = then, 1 = else; `otherwise`
//!   0 = the first argument gave a value, 1 = it faulted `E_FORMULA_EMPTY`/`E_FORMULA_ABSENT`
//!   and the fallback was taken.
//! * `trait EvalObserver { fn branch_taken(&mut self, id: NodeId, arm: usize); }` and
//!   `Coverage` (an observer collecting a set of `(id, arm)`): `new`/`default`, `taken()`
//!   (sorted), `contains`, `union`, `covered(&[BranchPoint])` (arms taken, of those points),
//!   `total(&[BranchPoint])` (all arms), `missing(&[BranchPoint])` (sorted `(id, arm)`).
//! * Evaluating with an observer: `eval::evaluate_observed(.., observer)` and
//!   `MemoryWorld::compute_observed(&cell, asof, &mut dyn EvalObserver)`. Only the arms of the
//!   evaluated function's own body are reported (not those of callees, whose ids belong to
//!   other bodies). An arm is reported when taken, also if the evaluation then faults.
//! * `coverage::edge_facts(&Compiled, &Signature) -> EdgeFacts` with `divisors`, `cell_inputs`,
//!   `slice_inputs`, `number_args`, `date_args`, `instant_args`, `extent_args` (see below).

mod common;

use std::collections::BTreeMap;

use datumesh_core::json::{self, Value};
use datumesh_core::{Scalar, Type};
use datumesh_formula::coverage::{branches, edge_facts, BranchKind, BranchPoint, Coverage, EvalObserver};
use datumesh_formula::eval::CellKey;
use datumesh_formula::memory::MemoryWorld;
use datumesh_formula::{define, Compiled, Env, Extent, NodeId, Signature};

const ASOF: &str = "2026-06-01T00:00:00.000000Z";
const NAME: &str = "calc.example.net/f";

fn sig(args: &[(&str, &str)], returns: &str) -> String {
    let decls: Vec<String> = args
        .iter()
        .map(|(n, t)| match *t {
            "number" => format!(r#""{n}":{{"type":"number","valid":false,"unit":"1"}}"#),
            "date" | "instant" => format!(r#""{n}":{{"type":"{t}","valid":true}}"#),
            "date-range" => format!(r#""{n}":{{"type":"date","valid":true,"extent":{{"lo":"2026-01-01","hi":"2026-12-31"}}}}"#),
            "number-extent" => {
                format!(r#""{n}":{{"type":"number","valid":false,"unit":"1","extent":{{"lo":"0","hi":"9"}}}}"#)
            }
            other => format!(r#""{n}":{{"type":"{other}","valid":false}}"#),
        })
        .collect();
    let order: Vec<String> = args.iter().map(|(n, _)| format!("\"{n}\"")).collect();
    let ret = if returns == "number" { r#"{"type":"number","unit":"1"}"#.to_string() } else { format!("\"{returns}\"") };
    format!(r#"{{"args":{{{}}},"order":[{}],"returns":{ret}}}"#, decls.join(","), order.join(","))
}

/// The environment of the vendored `formula-eval/at` world (stored `data.example.org/daily(date)`
/// in EUR, recorded for 2026-03-01 .. 2026-03-0x), with functions added by name.
fn base() -> Value {
    common::vector("formula-eval/at.json")
}

fn env_for(name: &str, signature: &str, callees: &[(&str, Value)]) -> Env {
    let b = base();
    let s = Signature::from_value(&json::parse(signature.as_bytes()).unwrap()).unwrap();
    let mut env = Env::new(name, s);
    env.add_function("data.example.org/daily", b.get("world").unwrap().get("functions").unwrap().get("data.example.org/daily").unwrap().get("descriptor").unwrap()).unwrap();
    for (n, d) in callees {
        env.add_function(n, d).unwrap();
    }
    env
}

fn descriptor(c: &Compiled, signature: &str) -> Value {
    let text = format!(
        r#"{{"code":{{"body":{},"lang":"formula"}},"inputs":[{}],"signature":{}}}"#,
        String::from_utf8(Value::String(c.body.clone()).to_jcs()).unwrap(),
        c.inputs.iter().map(|i| format!("\"{i}\"")).collect::<Vec<_>>().join(","),
        signature
    );
    json::parse(text.as_bytes()).unwrap()
}

/// A compiled body alone (for the static facts).
fn compile(body: &str, signature: &str) -> (Compiled, Signature) {
    let env = env_for(NAME, signature, &[]);
    let c = define(body, &env).unwrap_or_else(|e| panic!("{body}: {e}"));
    (c, Signature::from_value(&json::parse(signature.as_bytes()).unwrap()).unwrap())
}

struct Fixture {
    world: MemoryWorld,
    compiled: Compiled,
}

/// A world with the vendored stored cells, the `callees` (name, body, signature: computed
/// functions defined in order) and `calc.example.net/f` = `body`.
fn fixture(body: &str, signature: &str, callees: &[(&str, &str, &str)]) -> Fixture {
    let b = base();
    let mut world = MemoryWorld::from_value(b.get("world").unwrap(), ASOF).unwrap();
    let mut defined: Vec<(&str, Value)> = Vec::new();
    for (n, cb, cs) in callees {
        let env = env_for(n, cs, &defined);
        let c = define(cb, &env).unwrap_or_else(|e| panic!("{cb}: {e}"));
        let d = descriptor(&c, cs);
        world.bind(n, &d, "2026-01-01T00:00:00.000000Z").unwrap();
        defined.push((n, d));
    }
    let env = env_for(NAME, signature, &defined);
    let compiled = define(body, &env).unwrap_or_else(|e| panic!("{body}: {e}"));
    world.bind(NAME, &descriptor(&compiled, signature), "2026-01-01T00:00:00.000000Z").unwrap();
    Fixture { world, compiled }
}

fn args(pairs: &[(&str, Type, &str)]) -> BTreeMap<String, Scalar> {
    pairs.iter().map(|(n, t, v)| (n.to_string(), Scalar::from_lexical(*t, v).unwrap())).collect()
}

fn cell(a: &[(&str, Type, &str)]) -> CellKey {
    CellKey { name: NAME.to_string(), args: args(a) }
}

/// The value as a lexical string, or the fault code.
fn run(f: &mut Fixture, a: &[(&str, Type, &str)]) -> Result<String, &'static str> {
    f.world
        .compute(&cell(a), ASOF)
        .map(|c| c.outcome.value.to_value().get("value").unwrap().as_str().unwrap().to_string())
        .map_err(|c| c.as_str())
}

fn run_cov(f: &mut Fixture, a: &[(&str, Type, &str)], cov: &mut Coverage) -> Result<String, &'static str> {
    f.world
        .compute_observed(&cell(a), ASOF, cov)
        .map(|c| c.outcome.value.to_value().get("value").unwrap().as_str().unwrap().to_string())
        .map_err(|c| c.as_str())
}

const X: &str = "number";

// ---- The fixtures themselves, with today's API ----

#[test]
fn the_fixtures_evaluate_without_an_observer() {
    let s = sig(&[("x", X)], "number");
    let mut f = fixture("if x > 0 then 1 else 2", &s, &[]);
    assert_eq!(run(&mut f, &[("x", Type::Number, "5")]).as_deref(), Ok("1"));
    assert_eq!(run(&mut f, &[("x", Type::Number, "-5")]).as_deref(), Ok("2"));
}

// ---- 1. Branch ids ----

#[test]
fn a_body_without_branches_has_none() {
    let (c, _) = compile("x + 1", &sig(&[("x", X)], "number"));
    assert!(branches(&c.ast).is_empty());
    let cov = Coverage::new();
    assert_eq!(cov.covered(&[]), 0);
    assert_eq!(Coverage::total(&[]), 0);
}

#[test]
fn if_is_one_point_of_two_arms() {
    let (c, _) = compile("if x > 0 then 1 else 2", &sig(&[("x", X)], "number"));
    let b = branches(&c.ast);
    assert_eq!(b.len(), 1);
    assert_eq!(b[0].kind, BranchKind::If);
    assert_eq!(b[0].arms, 2);
    // The id is the arena index of the `if` node.
    assert!(matches!(c.ast.node(b[0].id), datumesh_formula::Node::If { .. }));
}

#[test]
fn nested_ifs_are_one_point_each_with_distinct_ascending_ids() {
    let body = "if x > 0 then (if x > 10 then 1 else 2) else (if x < -10 then 3 else 4)";
    let (c, _) = compile(body, &sig(&[("x", X)], "number"));
    let b = branches(&c.ast);
    assert_eq!(b.len(), 3);
    assert!(b.iter().all(|p| p.kind == BranchKind::If && p.arms == 2));
    assert!(b.windows(2).all(|w| w[0].id < w[1].id));
    assert_eq!(Coverage::total(&b), 6);
    for p in &b {
        assert!(matches!(c.ast.node(p.id), datumesh_formula::Node::If { .. }));
    }
}

#[test]
fn ids_are_stable_across_compiles_and_reparse_of_the_canonical_body() {
    let body = "if x > 0 then (if x > 10 then 1 else 2) else 3";
    let s = sig(&[("x", X)], "number");
    let (c1, _) = compile(body, &s);
    let (c2, _) = compile(body, &s);
    assert_eq!(branches(&c1.ast), branches(&c2.ast));
    let (c3, _) = compile(&c1.body, &s);
    assert_eq!(branches(&c1.ast), branches(&c3.ast));
}

// ---- 2. Coverage recording ----

#[test]
fn if_arms_are_recorded_and_unioned() {
    let s = sig(&[("x", X)], "number");
    let mut f = fixture("if x > 0 then 1 else 2", &s, &[]);
    let b = branches(&f.compiled.ast);
    let id = b[0].id;

    let mut first = Coverage::new();
    assert_eq!(run_cov(&mut f, &[("x", Type::Number, "5")], &mut first).as_deref(), Ok("1"));
    assert_eq!(first.taken(), vec![(id, 0)]);
    assert!(first.contains(id, 0) && !first.contains(id, 1));
    assert_eq!(first.covered(&b), 1);
    assert_eq!(first.missing(&b), vec![(id, 1)]);

    let mut second = Coverage::new();
    assert_eq!(run_cov(&mut f, &[("x", Type::Number, "-5")], &mut second).as_deref(), Ok("2"));
    assert_eq!(second.taken(), vec![(id, 1)]);

    let mut all = first.clone();
    all.union(&second);
    assert_eq!(all.covered(&b), 2);
    assert_eq!(Coverage::total(&b), 2);
    assert!(all.missing(&b).is_empty());
    // Taking an arm again changes nothing (a set).
    let mut again = all.clone();
    assert!(run_cov(&mut f, &[("x", Type::Number, "9")], &mut again).is_ok());
    assert_eq!(again, all);
}

#[test]
fn one_coverage_passed_to_several_evaluations_accumulates() {
    let s = sig(&[("x", X)], "number");
    let mut f = fixture("if x > 0 then 1 else 2", &s, &[]);
    let b = branches(&f.compiled.ast);
    let mut cov = Coverage::new();
    run_cov(&mut f, &[("x", Type::Number, "5")], &mut cov).unwrap();
    assert_eq!(cov.covered(&b), 1);
    run_cov(&mut f, &[("x", Type::Number, "-5")], &mut cov).unwrap();
    assert_eq!(cov.covered(&b), 2);
}

#[test]
fn nested_ifs_only_the_taken_path_is_recorded() {
    let body = "if x > 0 then (if x > 10 then 1 else 2) else (if x < -10 then 3 else 4)";
    let s = sig(&[("x", X)], "number");
    let mut f = fixture(body, &s, &[]);
    let b = branches(&f.compiled.ast);
    let (outer, then_if, else_if) = (b[2].id, b[0].id, b[1].id);
    // Whatever the numbering, the outer `if` is the one with the others below it: the root.
    assert_eq!(outer, f.compiled.ast.root());
    let mut cov = Coverage::new();
    assert_eq!(run_cov(&mut f, &[("x", Type::Number, "5")], &mut cov).as_deref(), Ok("2"));
    assert_eq!(cov.taken(), sorted(vec![(outer, 0), (then_if, 1)]));
    assert!(!cov.contains(else_if, 0) && !cov.contains(else_if, 1));
    assert_eq!(cov.covered(&b), 2);
    assert_eq!(Coverage::total(&b), 6);
    for (x, want) in [("20", "1"), ("-20", "3"), ("-5", "4")] {
        assert_eq!(run_cov(&mut f, &[("x", Type::Number, x)], &mut cov).as_deref(), Ok(want));
    }
    assert_eq!(cov.covered(&b), 6);
    assert!(cov.missing(&b).is_empty());
}

fn sorted(mut v: Vec<(NodeId, usize)>) -> Vec<(NodeId, usize)> {
    v.sort();
    v
}

#[test]
fn otherwise_records_the_arm_taken() {
    // `d` is a date: 2026-03-02 has a recorded value of the stored cell, 2026-01-01 none
    // (E_FORMULA_ABSENT, which otherwise catches). The stored cell is in EUR; the function's
    // result is the cell's amount or the fallback `x`, both EUR.
    let s = r#"{"args":{"d":{"type":"date","valid":true},"x":{"type":"number","valid":false,"unit":"EUR"}},"order":["d","x"],"returns":{"type":"number","unit":"EUR"}}"#;
    let mut f = fixture("otherwise(at(data.example.org/daily(date: *), d), x)", s, &[]);
    let b = branches(&f.compiled.ast);
    assert_eq!(b.len(), 1);
    assert_eq!((b[0].kind, b[0].arms), (BranchKind::Otherwise, 2));
    let id = b[0].id;
    let present = [("d", Type::Date, "2026-03-02"), ("x", Type::Number, "7")];
    let absent = [("d", Type::Date, "2026-01-01"), ("x", Type::Number, "7")];
    assert_eq!(run(&mut f, &present).as_deref(), Ok("12"));
    assert_eq!(run(&mut f, &absent).as_deref(), Ok("7"));

    let mut one = Coverage::new();
    assert_eq!(run_cov(&mut f, &present, &mut one).as_deref(), Ok("12"));
    assert_eq!(one.taken(), vec![(id, 0)]);
    let mut two = Coverage::new();
    assert_eq!(run_cov(&mut f, &absent, &mut two).as_deref(), Ok("7"));
    assert_eq!(two.taken(), vec![(id, 1)]);
    two.union(&one);
    assert_eq!(two.covered(&b), 2);
}

#[test]
fn a_fault_after_the_arm_keeps_the_arm_and_an_uncaught_fault_in_otherwise_records_no_arm() {
    let s = sig(&[("x", X), ("d", X)], "number");
    let mut f = fixture("if x > 0 then x / d else 0", &s, &[]);
    let b = branches(&f.compiled.ast);
    let mut cov = Coverage::new();
    let r = run_cov(&mut f, &[("x", Type::Number, "5"), ("d", Type::Number, "0")], &mut cov);
    assert_eq!(r, Err("E_FORMULA_DIV_ZERO"));
    assert_eq!(cov.taken(), vec![(b[0].id, 0)]);

    // E_FORMULA_DIV_ZERO is not catchable: otherwise takes neither arm.
    let mut g = fixture("otherwise(x / d, 0)", &s, &[]);
    let mut cov = Coverage::new();
    let r = run_cov(&mut g, &[("x", Type::Number, "5"), ("d", Type::Number, "0")], &mut cov);
    assert_eq!(r, Err("E_FORMULA_DIV_ZERO"));
    assert!(cov.taken().is_empty());
}

#[test]
fn only_the_function_under_evaluation_reports_not_its_callees() {
    let inner_sig = sig(&[("x", X)], "number");
    let callee = ("calc.example.net/inner", "if x > 0 then 1 else 2", inner_sig.as_str());
    let mut f = fixture("calc.example.net/inner(x: x) + 1", &sig(&[("x", X)], "number"), &[callee]);
    assert!(branches(&f.compiled.ast).is_empty());
    let mut cov = Coverage::new();
    assert_eq!(run_cov(&mut f, &[("x", Type::Number, "5")], &mut cov).as_deref(), Ok("2"));
    assert!(cov.taken().is_empty());
}

#[derive(Default)]
struct Log(Vec<(NodeId, usize)>);

impl EvalObserver for Log {
    fn branch_taken(&mut self, id: NodeId, arm: usize) {
        self.0.push((id, arm));
    }
}

#[test]
fn a_custom_observer_is_called_once_per_branch_taken_in_evaluation_order() {
    let body = "if x > 0 then (if x > 10 then 1 else 2) else 3";
    let s = sig(&[("x", X)], "number");
    let mut f = fixture(body, &s, &[]);
    let b = branches(&f.compiled.ast);
    let (inner, outer) = (b[0].id, b[1].id);
    let mut log = Log::default();
    f.world.compute_observed(&cell(&[("x", Type::Number, "5")]), ASOF, &mut log).unwrap();
    // The outer `if` decides first, then the inner one.
    assert_eq!(log.0, vec![(outer, 0), (inner, 1)]);
}

#[test]
fn evaluate_observed_is_the_same_hook_below_the_world() {
    use datumesh_formula::eval::{evaluate_observed, Stop};
    let s = sig(&[("x", X)], "number");
    let f = fixture("if x > 0 then 1 else 2", &s, &[]);
    let function = datumesh_formula::Function::from_value(&descriptor(&f.compiled, &s)).unwrap();
    let mut world = f.world.clone();
    let mut cov = Coverage::new();
    let out = evaluate_observed(&f.compiled, &function, &args(&[("x", Type::Number, "-1")]), ASOF, &mut world, &mut cov);
    match out {
        Ok(o) => assert_eq!(o.value, Scalar::Number("2".to_string())),
        Err(Stop::Code(c)) => panic!("{}", c.as_str()),
        Err(Stop::World(_)) => panic!("world"),
    }
    assert_eq!(cov.taken(), vec![(branches(&f.compiled.ast)[0].id, 1)]);
}

// ---- 3. Edge facts ----

fn names(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

#[test]
fn divisors_are_the_number_arguments_in_a_divisor() {
    let s = sig(&[("x", X), ("y", X), ("z", X)], "number");
    // `/`: the right operand; arguments anywhere inside it count.
    let (c, sg) = compile("x / y", &s);
    assert_eq!(names(&edge_facts(&c, &sg).divisors), ["y"]);
    let (c, sg) = compile("x / (y - 1)", &s);
    assert_eq!(names(&edge_facts(&c, &sg).divisors), ["y"]);
    let (c, sg) = compile("x / (y * z)", &s);
    assert_eq!(names(&edge_facts(&c, &sg).divisors), ["y", "z"]);
    // `mod`'s divisor is its second argument.
    let (c, sg) = compile("mod(x, y)", &s);
    assert_eq!(names(&edge_facts(&c, &sg).divisors), ["y"]);
    // Not a divisor: the dividend, a literal divisor, no division at all.
    let (c, sg) = compile("x / 2", &s);
    assert!(edge_facts(&c, &sg).divisors.is_empty());
    let (c, sg) = compile("x * y + z", &s);
    assert!(edge_facts(&c, &sg).divisors.is_empty());
    // Static: a divisor in an arm no input reaches still counts.
    let (c, sg) = compile("if x > 0 then 1 else x / z", &s);
    assert_eq!(names(&edge_facts(&c, &sg).divisors), ["z"]);
    // Ascending, once each.
    let (c, sg) = compile("(x / z) + (y / z) + (x / y)", &s);
    assert_eq!(names(&edge_facts(&c, &sg).divisors), ["y", "z"]);
}

#[test]
fn argument_types_are_listed_by_name() {
    let s = sig(&[("n", X), ("m", X), ("d", "date"), ("t", "instant"), ("s", "string")], "number");
    let (c, sg) = compile("n + m", &s);
    let f = edge_facts(&c, &sg);
    assert_eq!(names(&f.number_args), ["m", "n"]);
    assert_eq!(names(&f.date_args), ["d"]);
    assert_eq!(names(&f.instant_args), ["t"]);
}

#[test]
fn extents_of_date_and_instant_arguments_are_given_with_their_declaration() {
    let s = sig(&[("n", "number-extent"), ("d", "date-range"), ("e", "date"), ("t", "instant")], "number");
    let (c, sg) = compile("n", &s);
    let f = edge_facts(&c, &sg);
    // Only date/instant arguments with a declared extent; numbers' extents are not edge cases.
    assert_eq!(f.extent_args.len(), 1);
    assert_eq!(f.extent_args[0].0, "d");
    assert_eq!(f.extent_args[0].1, Extent::Range("2026-01-01".to_string(), "2026-12-31".to_string()));
}

#[test]
fn cell_reads_are_inputs_that_can_be_absent_and_slices_are_apart() {
    let eur = r#"{"args":{"d":{"type":"date","valid":true}},"order":["d"],"returns":{"type":"number","unit":"EUR"}}"#;
    // A cell read.
    let (c, sg) = compile("data.example.org/daily(date: d)", eur);
    let f = edge_facts(&c, &sg);
    assert_eq!(names(&f.cell_inputs), ["data.example.org/daily"]);
    assert!(f.slice_inputs.is_empty());
    // A slice read (an aggregate over an open range): a slice input, not a cell input.
    let (c, sg) = compile("sum(data.example.org/daily(date: *))", eur);
    let f = edge_facts(&c, &sg);
    assert_eq!(names(&f.slice_inputs), ["data.example.org/daily"]);
    assert!(f.cell_inputs.is_empty());
    // A bounded range is a slice too.
    let (c, sg) = compile("sum(data.example.org/daily(date: [2026-03-01..2026-03-03]))", eur);
    let f = edge_facts(&c, &sg);
    assert_eq!(names(&f.slice_inputs), ["data.example.org/daily"]);
    // Both kinds of the same function, and several reads of one function, once each.
    let (c, sg) = compile("data.example.org/daily(date: d) + sum(data.example.org/daily(date: *)) + data.example.org/daily(date: d)", eur);
    let f = edge_facts(&c, &sg);
    assert_eq!(names(&f.cell_inputs), ["data.example.org/daily"]);
    assert_eq!(names(&f.slice_inputs), ["data.example.org/daily"]);
    // No reads, no inputs.
    let (c, sg) = compile("x", &sig(&[("x", X)], "number"));
    let f = edge_facts(&c, &sg);
    assert!(f.cell_inputs.is_empty() && f.slice_inputs.is_empty());
}

#[test]
fn branch_points_are_a_plain_value() {
    let p = BranchPoint { id: 3, kind: BranchKind::If, arms: 2 };
    assert_eq!(p.clone(), p);
    assert_eq!(format!("{p:?}").contains("arms"), true);
}
