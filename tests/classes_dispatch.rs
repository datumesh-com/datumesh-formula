//! Classes (SPEC 16.2, 16.3, 16.6, spec-0.5.0), third group: bounded dispatch, `super`, and
//! interfaces. Contract item 3 and the rest of item 1 (`super`), and the define path of item 7
//! for classes. The tests are derived from the `formula` and `formula-eval` class vectors, the
//! `class` vectors, and the text of 16.3 and 16.6; where a case is not a vector, its expectation
//! was checked against the reference tool as a black box (`IMPL-NOTES-classes.md`).

mod common;

use common::classes::{class_env, dispatch_eval, dispatch_formula, no_args, FX, ONE};
use datumesh_core::json;
use datumesh_core::ClassDescriptor;
use datumesh_formula::definition::{define_class, define_function, define_method};
use datumesh_formula::memory::run_vector;
use datumesh_formula::{check_descriptor, define, Ast, Code, Compiled, Env, Error, Node, ReadArg};

const BOND: &str = "calc.example.net/bond";
const SWAP: &str = "calc.example.net/swap";
const VERSE: &str = "scripture.example.org/verse";

/// The signature of a function with a date argument and an argument `instrument` holding the key of
/// an object of one of `classes`.
fn dispatch_signature(classes: &[&str], returns: &str) -> String {
    let set: Vec<String> = classes.iter().map(|c| format!("\"{c}\"")).collect();
    format!(
        r#"{{"args": {{"date": {{"type": "date", "valid": true}}, "instrument": {{"type": "string", "classes": [{}], "valid": false}}}},
        "order": ["instrument", "date"], "returns": {returns}}}"#,
        set.join(", ")
    )
}

fn def(src: &str, signature: &str) -> Result<Compiled, Error> {
    define(src, &class_env("calc.example.net/e2", signature, None))
}

fn ok(src: &str, signature: &str) -> Compiled {
    def(src, signature).unwrap_or_else(|e| panic!("{src}: {e}"))
}

fn code(src: &str, signature: &str) -> String {
    match def(src, signature) {
        Ok(c) => panic!("{src}: accepted as {:?}", c.body),
        Err(e) => e.code.as_str().to_string(),
    }
}

fn jcs(c: &Compiled) -> String {
    String::from_utf8(c.ast.to_jcs()).unwrap()
}

// ---------------------------------------------------------------------------------------------
// The vectors.

/// The class `formula` vectors of this group: a dispatch whose classes' methods differ in
/// signature, and `super` in a class without a parent.
#[test]
fn class_formula_vectors() {
    let t = common::check::tally(dispatch_formula(), common::check::formula_vector);
    let counts = t.report("class formula vectors (dispatch group)");
    assert!(t.failures.is_empty(), "{}", t.failures.join("\n"));
    assert_eq!(counts, (0, 2));
}

/// The class `formula-eval` vectors of this group: `price_of` dispatching `price` to a bond and to a
/// swap (the lineage names the function actually called), `super` read pinned to the parent's
/// hash, and the dispatch key fault (a key outside the set, a malformed encoding, keys that are
/// not the class's).
#[test]
fn class_formula_eval_vectors() {
    let t = common::check::tally(dispatch_eval(), |p, v| common::check::eval_vector("formula-eval", p, v));
    let counts = t.report("class formula-eval vectors (dispatch group)");
    assert!(t.failures.is_empty(), "{}", t.failures.join("\n"));
    assert_eq!(counts, (3, 3));
}

/// The local vectors over a class with one number key (`tools/gen_class_vectors.py`, expected
/// results the reference tool's): chains of dispatches over a returned key, the fault for each way
/// the returned key can fail to be the key of an object of the class, and a dispatched read without a
/// value (plain `not_covered`).
#[test]
fn local_class_vectors() {
    let vectors = common::local_vectors();
    let t = common::check::tally(vectors, |p, v| common::check::eval_vector("formula-eval", p, v));
    let counts = t.report("local class vectors");
    assert!(t.failures.is_empty(), "{}", t.failures.join("\n"));
    assert_eq!(counts, (2, 9));
}

/// Lineage records the receipt, and so the function hash, actually called: the receipt a bond's
/// dispatch lists is of the bond's `price`, a swap's is of the swap's (16.6).
#[test]
fn lineage_names_the_function_actually_called() {
    let class_price = |class: &str| {
        let world = common::vector("formula-eval/class-dispatch-bond.json");
        let c =
            world.get("world").unwrap().get("classes").unwrap().get(class).unwrap().get("descriptor").unwrap().clone();
        ClassDescriptor::from_value(&c).unwrap().methods["price"].clone()
    };
    for (vector, class) in
        [("formula-eval/class-dispatch-bond.json", BOND), ("formula-eval/class-dispatch-swap.json", SWAP)]
    {
        let v = common::vector(vector);
        let r = run_vector(&v).unwrap().unwrap();
        assert_eq!(r.computed.outcome.lineage.len(), 1);
        let receipt = common::array(v.get("world").unwrap().get("receipts").unwrap())
            .iter()
            .find(|x| datumesh_formula::memory::receipt_hash(x) == r.computed.outcome.lineage[0])
            .expect("the receipt the dispatch read is in the world");
        assert_eq!(common::string(receipt.get("cell").unwrap(), "fn_hash").unwrap(), class_price(class), "{vector}");
        assert_eq!(common::string(receipt.get("cell").unwrap(), "ns"), Some(class));
    }
    // The two calls of `price_of` are two different functions read through one body.
    let a = run_vector(&common::vector("formula-eval/class-dispatch-bond.json")).unwrap().unwrap();
    let b = run_vector(&common::vector("formula-eval/class-dispatch-swap.json")).unwrap().unwrap();
    assert_ne!(a.computed.outcome.lineage, b.computed.outcome.lineage);
}

/// A key of the class whose rule it breaks: the key is first decoded and checked as the class's
/// own, then read as the method function's argument, whose `key_normalisation` faults
/// `E_FORMULA_KEY` (the reference tool's reading; 16.6 says nothing of a well-formed key not
/// canonical under the class's rule, see SPEC-FINDINGS.md).
#[test]
fn a_dispatch_key_not_canonical_for_its_class() {
    let v = common::vector("formula-eval/class-dispatch-bond.json");
    let text = String::from_utf8(v.to_jcs()).unwrap().replace("bond(isin:\\\"XS0001\\\")", "bond(isin:\\\"xs0001\\\")");
    assert_ne!(text, String::from_utf8(v.to_jcs()).unwrap());
    let bad = json::parse(text.as_bytes()).unwrap();
    assert_eq!(run_vector(&bad).unwrap().unwrap_err(), Code::Key);
}

// ---------------------------------------------------------------------------------------------
// The AST nodes.

/// The `dispatch` node: `{"args", "classes", "method", "node": "dispatch", "on"}`, printed
/// `<on>.m(name: value, ...)`, and a chain on a returned key.
#[test]
fn the_dispatch_node() {
    let ast = Ast::from_parts(
        vec![
            Node::Var("instrument".into()),
            Node::Var("date".into()),
            Node::Dispatch {
                on: 0,
                method: "price".into(),
                classes: vec![BOND.into(), SWAP.into()],
                args: vec![("date".into(), 1)],
            },
        ],
        2,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(ast.to_jcs()).unwrap(),
        r#"{"args":{"date":{"name":"date","node":"var"}},"classes":["calc.example.net/bond","calc.example.net/swap"],"method":"price","node":"dispatch","on":{"name":"instrument","node":"var"}}"#
    );
    assert_eq!(ast.print(), "instrument.price(date: date)");
    assert_eq!(ast.body(), "instrument.price(date: date)\n");
    // A chain: `v.next().text(edition: "uxlc")`, a dispatch over the returned class.
    let chain = Ast::from_parts(
        vec![
            Node::Var("v".into()),
            Node::Dispatch { on: 0, method: "next".into(), classes: vec![VERSE.into()], args: vec![] },
            Node::Str("uxlc".into()),
            Node::Dispatch {
                on: 1,
                method: "text".into(),
                classes: vec![VERSE.into()],
                args: vec![("edition".into(), 2)],
            },
        ],
        3,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(chain.to_jcs()).unwrap(),
        r#"{"args":{"edition":{"node":"str","value":"uxlc"}},"classes":["scripture.example.org/verse"],"method":"text","node":"dispatch","on":{"args":{},"classes":["scripture.example.org/verse"],"method":"next","node":"dispatch","on":{"name":"v","node":"var"}}}"#
    );
    assert_eq!(chain.print(), "v.next().text(edition: \"uxlc\")");
    assert_eq!(chain.depth(), 3); // text -> next -> v: three nodes on the longest path (Ast::depth)
}

/// A read pinned to a function hash (what `super` compiles to): the `read` node has `fn_hash`, and
/// prints `<fn>@<fn_hash>(...)`.
#[test]
fn a_pinned_read_node() {
    let hash = "530bf86365ff4afb5a70196542d45f293fad5909026e015eddcfd866290f85b8";
    let ast = Ast::from_parts(
        vec![
            Node::Var("base".into()),
            Node::Var("date".into()),
            Node::Var("quote".into()),
            Node::Read {
                fn_name: "rates.example.org/fx_pair/rate".into(),
                fn_hash: Some(hash.into()),
                args: vec![
                    ("base".into(), ReadArg::Expr(0)),
                    ("date".into(), ReadArg::Expr(1)),
                    ("quote".into(), ReadArg::Expr(2)),
                ],
            },
        ],
        3,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(ast.to_jcs()).unwrap(),
        format!(
            r#"{{"args":{{"base":{{"name":"base","node":"var"}},"date":{{"name":"date","node":"var"}},"quote":{{"name":"quote","node":"var"}}}},"fn":"rates.example.org/fx_pair/rate","fn_hash":"{hash}","node":"read"}}"#
        )
    );
    assert_eq!(ast.print(), format!("rates.example.org/fx_pair/rate@{hash}(base: base, date: date, quote: quote)"));
}

// ---------------------------------------------------------------------------------------------
// Dispatch at define (16.6).

const PRICE: &str = r#"{"type": "number", "unit": "1"}"#;

/// A method call on an argument that declares `classes` dispatches among them: the node, the body
/// `instrument.price(date: date)`, and inputs holding `<class>/price` for every class of the set.
#[test]
fn a_method_call_on_a_key_dispatches() {
    let sig = dispatch_signature(&[BOND, SWAP], PRICE);
    let c = ok("instrument.price(date: date)", &sig);
    assert_eq!(c.body, "instrument.price(date: date)\n");
    assert_eq!(c.inputs, ["calc.example.net/bond/price", "calc.example.net/swap/price"]);
    assert_eq!(
        jcs(&c),
        r#"{"args":{"date":{"name":"date","node":"var"}},"classes":["calc.example.net/bond","calc.example.net/swap"],"method":"price","node":"dispatch","on":{"name":"instrument","node":"var"}}"#
    );
    assert!(!c.asof_dependent);
    assert_eq!(c.returns.to_value().to_jcs(), br#"{"type":"number","unit":"1"}"#.to_vec());
    // Positional or labelled, a literal or a variable: one node shape.
    assert_eq!(ok("instrument.price(date)", &sig).ast, c.ast);
    assert_eq!(ok("instrument.price(date: 2026-03-16)", &sig).body, "instrument.price(date: 2026-03-16)\n");
    assert_eq!(ok("instrument.price(date: date) + 1", &sig).inputs, c.inputs);
    // A set of one class is a dispatch too.
    let one = ok("instrument.price(date: date)", &dispatch_signature(&[BOND], PRICE));
    assert_eq!(one.inputs, ["calc.example.net/bond/price"]);
    assert!(jcs(&one).contains(r#""classes":["calc.example.net/bond"]"#));
}

/// The codes of a dispatch at define: the arguments as for a method (without the keys), a method
/// a class of the set does not have, a class that is not known, a method on a value that is no
/// object key, and signatures that differ once the keys are taken away.
#[test]
fn dispatch_errors_at_define() {
    let sig = dispatch_signature(&[BOND, SWAP], PRICE);
    for (src, want) in [
        ("instrument.price(date: date, extra: 1)", "E_FORMULA_ARG_UNKNOWN"),
        ("instrument.price()", "E_FORMULA_ARG_MISSING"),
        ("instrument.price(2026-03-16, 2026-03-17)", "E_FORMULA_ARITY"),
        ("instrument.points(date: date)", "E_FORMULA_METHOD"),
        // The result of a method is a number, not an object.
        ("instrument.price(date: date).foo()", "E_FORMULA_RESERVED"),
    ] {
        assert_eq!(code(src, &sig), want, "{src}");
    }
    // A class of the set without the method, or not known.
    let no_method = dispatch_signature(&[BOND, "rates.example.org/fx_pair"], PRICE);
    assert_eq!(code("instrument.price(date: date)", &no_method), "E_FORMULA_METHOD");
    let unknown = dispatch_signature(&[BOND, "calc.example.net/nonexistent"], PRICE);
    assert_eq!(code("instrument.price(date: date)", &unknown), "E_FORMULA_UNRESOLVED");
    // A string argument that holds no object key, an expression that is no object.
    let plain = r#"{"args": {"date": {"type": "date", "valid": true}, "isin": {"type": "string", "valid": false}},
        "order": ["isin", "date"], "returns": {"type": "number", "unit": "1"}}"#;
    assert_eq!(code("isin.price(date: date)", plain), "E_FORMULA_RESERVED");
    assert_eq!(code("(1 + 2).price(date: date)", &sig), "E_FORMULA_RESERVED");
    // `N/m` bound to another hash than the class's: the method is not the class's.
    let mut env = class_env("calc.example.net/e2", &sig, None);
    let swap_price = env.functions.get("calc.example.net/swap/price").unwrap().value.clone();
    env.add_function("calc.example.net/bond/price", &swap_price).unwrap();
    assert_eq!(define("instrument.price(date: date)", &env).unwrap_err().code.as_str(), "E_FORMULA_METHOD");
}

/// A chain on a returned key, `verse(...).next().text("uxlc")`, is a dispatch over the returned
/// classes: the inner read is the plain read of `next`, the outer node dispatches `text`; the inputs
/// hold both methods.
#[test]
fn a_chain_on_returned_keys() {
    let blob = no_args(r#""blob""#);
    let c = ok("scripture.example.org/verse(\"tanakh\", \"Gen\", 1, 1).next().text(\"uxlc\")", &blob);
    assert_eq!(c.body, "scripture.example.org/verse/next(book: \"Gen\", chapter: 1, corpus: \"tanakh\", verse: 1).text(edition: \"uxlc\")\n");
    assert_eq!(c.inputs, ["scripture.example.org/verse/next", "scripture.example.org/verse/text"]);
    assert_eq!(
        jcs(&c),
        r#"{"args":{"edition":{"node":"str","value":"uxlc"}},"classes":["scripture.example.org/verse"],"method":"text","node":"dispatch","on":{"args":{"book":{"node":"str","value":"Gen"},"chapter":{"node":"num","value":"1"},"corpus":{"node":"str","value":"tanakh"},"verse":{"node":"num","value":"1"}},"fn":"scripture.example.org/verse/next","node":"read"}}"#
    );
    // The plain read of `next` returns the same, and `.text(...)` on it is the same dispatch.
    assert_eq!(
        ok("scripture.example.org/verse/next(\"tanakh\", \"Gen\", 1, 1).text(edition: \"uxlc\")", &blob).ast,
        c.ast
    );
    // Two steps; on an argument that declares the class.
    let two = ok("scripture.example.org/verse(\"tanakh\", \"Gen\", 1, 1).next().next().text(\"uxlc\")", &blob);
    assert_eq!(two.body, "scripture.example.org/verse/next(book: \"Gen\", chapter: 1, corpus: \"tanakh\", verse: 1).next().text(edition: \"uxlc\")\n");
    let arg = r#"{"args": {"v": {"type": "string", "classes": ["scripture.example.org/verse"], "valid": false}}, "order": ["v"], "returns": "blob"}"#;
    assert_eq!(ok("v.next().text(\"uxlc\")", arg).body, "v.next().text(edition: \"uxlc\")\n");
    assert_eq!(ok("v.text(\"uxlc\")", arg).inputs, ["scripture.example.org/verse/text"]);
}

/// `define_function` of a body that dispatches reproduces the function the class world has for
/// `calc.example.net/price_of`: its hash, and the inputs that list both classes' methods.
#[test]
fn define_function_reproduces_price_of() {
    let world = common::vector("formula/class-fx-rate.json");
    let class = Env::from_value(world.get("env").unwrap()).unwrap();
    let price_of = &class.functions["calc.example.net/price_of"];
    let sig = price_of.value.get("signature").unwrap();
    let d = define_function(
        "calc.example.net/price_of",
        "instrument.price(date)",
        sig,
        &Default::default(),
        &class,
        "2026-06-01T00:00:00.000000Z",
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d.fn_hash, price_of.hash);
    assert_eq!(d.descriptor.inputs, ["calc.example.net/bond/price", "calc.example.net/swap/price"]);
}

// ---------------------------------------------------------------------------------------------
// `super` (16.3).

const NDF_SIGNATURE: &str = r#"{"args": {"base": {"type": "string", "valid": false, "key_normalisation": "upper_ascii"},
    "date": {"type": "date", "valid": true}, "quote": {"type": "string", "valid": false, "key_normalisation": "upper_ascii"}},
    "order": ["base", "quote", "date"], "returns": {"type": "number", "unit": {"of": {"arg": "quote"}, "per": {"arg": "base"}}}}"#;
const PARENT_RATE: &str = "530bf86365ff4afb5a70196542d45f293fad5909026e015eddcfd866290f85b8";
const PARENT_CROSS: &str = "9979eda4fbd1644dae480ae76d5514bb3c96e7645e887400adf1cc182bb0c911";

fn ndf(src: &str) -> Result<Compiled, Error> {
    define(src, &class_env("calc.example.net/fx_ndf/rate", NDF_SIGNATURE, Some("calc.example.net/fx_ndf")))
}

/// `super.m(args)` in a method of a subclass reads the parent's method `m` by its function hash: a
/// read of `P/m` whose node has `fn_hash`, printed `P/m@<hash>(...)`.
#[test]
fn super_is_pinned_to_the_parents_hash() {
    let c = ndf("super.rate(date)").unwrap();
    assert_eq!(c.body, format!("rates.example.org/fx_pair/rate@{PARENT_RATE}(base: base, date: date, quote: quote)\n"));
    assert_eq!(c.inputs, ["rates.example.org/fx_pair/rate"]);
    assert_eq!(
        jcs(&c),
        format!(
            r#"{{"args":{{"base":{{"name":"base","node":"var"}},"date":{{"name":"date","node":"var"}},"quote":{{"name":"quote","node":"var"}}}},"fn":"rates.example.org/fx_pair/rate","fn_hash":"{PARENT_RATE}","node":"read"}}"#
        )
    );
    let c = ndf("super.rate(date) * 1").unwrap();
    assert!(c.body.starts_with(&format!("rates.example.org/fx_pair/rate@{PARENT_RATE}(")), "{}", c.body);
    assert!(c.body.ends_with(" * 1\n"));
    let c = ndf("super.cross(\"GBP\", date)").unwrap();
    assert_eq!(
        c.body,
        format!("rates.example.org/fx_pair/cross@{PARENT_CROSS}(base: base, date: date, quote: quote, via: \"GBP\")\n")
    );
    // `this` in the subclass is the subclass's own (inherited or overridden) method, by name.
    assert_eq!(
        ndf("this.rate(date)").unwrap().body,
        "calc.example.net/fx_ndf/rate(base: base, date: date, quote: quote)\n"
    );
    for (src, want) in [
        ("super.points(date)", "E_FORMULA_METHOD"),
        ("super.rate(base: base, quote: quote, date: date)", "E_FORMULA_ARG_DUPLICATE"),
        ("super", "E_FORMULA_RESERVED"),
    ] {
        assert_eq!(ndf(src).unwrap_err().code.as_str(), want, "{src}");
    }
    // `super` in a class without a parent (the vector), and outside a method's body.
    let fx = class_env("rates.example.org/fx_pair/spot", NDF_SIGNATURE, Some("rates.example.org/fx_pair"));
    assert_eq!(define("super.rate(date)", &fx).unwrap_err().code.as_str(), "E_FORMULA_METHOD");
    assert_eq!(
        define("super.rate(date)", &class_env("calc.example.net/e", &no_args(FX), None)).unwrap_err().code.as_str(),
        "E_FORMULA_RESERVED"
    );
}

/// The pinned read is accepted in a method's body, and by an evaluator reading a stored body (here
/// `check_descriptor`, with no `method_of`); it stays `E_FORMULA_PLACEMENT` in any other body
/// (`invalid/formula-class-pin-in-body`).
#[test]
fn a_pinned_read_is_for_methods_and_stored_bodies() {
    let env = class_env("calc.example.net/fx_ndf/rate", NDF_SIGNATURE, None);
    let descriptor = env.functions["calc.example.net/fx_ndf/rate"].value.clone();
    let c = check_descriptor("calc.example.net/fx_ndf/rate", &descriptor, &env).unwrap_or_else(|e| panic!("{e}"));
    assert!(c.body.contains(&format!("@{PARENT_RATE}(")));
    assert_eq!(c.inputs, ["rates.example.org/fx_pair/rate"]);
    let pinned =
        format!("rates.example.org/fx_pair/rate@{PARENT_RATE}(base: \"EUR\", quote: \"USD\", date: 2026-03-16)");
    assert_eq!(code(&pinned, &no_args(FX)), "E_FORMULA_PLACEMENT");
    // In a method's body, written out, it is accepted too (it is what `super` prints).
    let method = class_env("calc.example.net/fx_ndf/rate", NDF_SIGNATURE, Some("calc.example.net/fx_ndf"));
    let written = format!("rates.example.org/fx_pair/rate@{PARENT_RATE}(base: base, date: date, quote: quote)");
    assert_eq!(define(&written, &method).unwrap().ast, ndf("super.rate(date)").unwrap().ast);
}

/// The class world is reproduced by define: the override written with `super` has the function
/// hash the class `fx_ndf` holds for `rate`.
#[test]
fn define_method_reproduces_the_overriding_methods_hash() {
    let world = common::vector("formula/class-fx-rate.json");
    let class = Env::from_value(world.get("env").unwrap()).unwrap();
    let name = "calc.example.net/fx_ndf/rate";
    let sig = class.functions[name].value.get("signature").unwrap();
    let d = define_method(
        "calc.example.net/fx_ndf",
        name,
        "super.rate(date) * 1",
        sig,
        &Default::default(),
        &class,
        "2026-06-01T00:00:00.000000Z",
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let fx_ndf = class.classes.get("calc.example.net/fx_ndf").unwrap();
    assert_eq!(Some(&d.fn_hash), fx_ndf.methods.get("rate"));
}

/// Evaluating the pinned read relies on no binding (16.3): with the parent's `rate` not bound at
/// the asof (or bound later than the receipts), `fx_ndf/rate` evaluates all the same, with the same
/// receipt `asof`; the plain read through `this` does rely on it (the binding's start is an
/// `asof` candidate, 8.2, and an unbound name is unresolved).
#[test]
fn super_relies_on_no_binding() {
    let rebind = |vector: &str, start: &str| {
        let text = String::from_utf8(common::vector(vector).to_jcs()).unwrap();
        let marker = "\"rates.example.org/fx_pair/rate\":{\"bound\":\"2026-01-01T00:00:00.000000Z\"";
        assert!(text.contains(marker), "{vector}");
        json::parse(
            text.replace(marker, &format!("\"rates.example.org/fx_pair/rate\":{{\"bound\":\"{start}\"")).as_bytes(),
        )
        .unwrap()
    };
    for start in ["2026-12-01T00:00:00.000000Z", "2026-05-01T00:00:00.000000Z"] {
        let r = run_vector(&rebind("formula-eval/class-super.json", start))
            .unwrap()
            .unwrap_or_else(|c| panic!("{start}: {c}"));
        assert_eq!(r.computed.outcome.value.lexical(), "1.0892");
        assert_eq!(r.computed.outcome.receipt_asof, "2026-03-16T16:00:00.000000Z");
        assert_eq!(r.computed.receipt_hash, "a28b25916e8c7ae34cb12d3bfc37613f4e0658cb80f0827a705dab8232b36a4d");
        assert_eq!(r.fuel_used, 7);
    }
    // The plain read does rely on it.
    let r = run_vector(&rebind("formula-eval/class-spot.json", "2026-05-01T00:00:00.000000Z")).unwrap().unwrap();
    assert_eq!(r.computed.outcome.receipt_asof, "2026-05-01T00:00:00.000000Z");
    assert_eq!(
        run_vector(&rebind("formula-eval/class-spot.json", "2026-12-01T00:00:00.000000Z")).unwrap().unwrap_err(),
        Code::Unresolved
    );
}

// ---------------------------------------------------------------------------------------------
// Interfaces, and the define path for classes (16.2).

/// A class implementing an interface is an ordinary class in a body: its methods, those of the
/// interface included, are plain functions `N/m` (an interface is a keyless method set, 16.2
/// step 8; nothing in 16.3 or 16.6 gives it more).
#[test]
fn a_class_with_interfaces_is_an_ordinary_class() {
    let v = common::vector("class/implements-interface.json");
    let flattened = ClassDescriptor::from_json(common::string(&v, "flattened").unwrap().as_bytes()).unwrap();
    assert_eq!(flattened.implements.len(), 1);
    let mut env = class_env("calc.example.net/e", &no_args(ONE), None);
    env.classes.insert("rates.example.org/fx_pair", flattened.clone());
    env.classes.insert(
        "rates.example.org/priced",
        ClassDescriptor::from_value(&common::array(v.get("env").unwrap().get("classes").unwrap())[0]).unwrap(),
    );
    for (method, hash) in &flattened.methods {
        let d = common::array(v.get("env").unwrap().get("functions").unwrap())
            .iter()
            .find(|d| datumesh_core::sha256_hex(&d.to_jcs()) == *hash)
            .unwrap_or_else(|| panic!("{method}"));
        env.add_function(&format!("rates.example.org/fx_pair/{method}"), d).unwrap();
    }
    let c = define("rates.example.org/fx_pair(EUR, USD).price(2026-03-16)", &env).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(c.body, "rates.example.org/fx_pair/price(base: \"EUR\", date: 2026-03-16, quote: \"USD\")\n");
    // And a member of a dispatch set.
    let sig = r#"{"args": {"date": {"type": "date", "valid": true}, "i": {"type": "string", "classes": ["rates.example.org/fx_pair"], "valid": false}},
        "order": ["i", "date"], "returns": {"type": "number", "unit": "1"}}"#;
    let mut env2 = env.clone();
    env2.signature = datumesh_formula::Signature::from_value(&json::parse(sig.as_bytes()).unwrap()).unwrap();
    assert_eq!(define("i.price(date: date)", &env2).unwrap().inputs, ["rates.example.org/fx_pair/price"]);
}

/// Every `class` vector through `define_class` over an environment holding what the vector's
/// `env` lists (the parents and interfaces by class hash, the method functions by function hash,
/// the definer): the flattened descriptor and class hash, or `datumesh-core`'s code. The define
/// itself is the core's (`define_class`, 16.2); this checks that the formula crate's define path
/// gives it what it needs.
#[test]
fn define_class_over_the_vectors() {
    let vectors = common::vectors_of_kind("class");
    assert_eq!(vectors.len(), 45);
    let mut failures = Vec::new();
    let (mut valid, mut invalid) = (0, 0);
    for (path, v) in &vectors {
        let env = v.get("env").unwrap();
        let mut callees = Env::from_value(
            &json::parse(
                br#"{"name": "x.example.org/f", "signature": {"args": {}, "order": [], "returns": "boolean"}}"#,
            )
            .unwrap(),
        )
        .unwrap();
        for (i, d) in common::array(env.get("functions").unwrap()).iter().enumerate() {
            callees.add_function(&format!("x.example.org/f{i}"), d).unwrap();
        }
        for (i, d) in common::array(env.get("classes").unwrap()).iter().enumerate() {
            callees.classes.insert(&format!("x.example.org/c{i}"), ClassDescriptor::from_value(d).unwrap());
        }
        let got = define_class(
            common::string(v, "name").unwrap(),
            common::string(v, "input").unwrap().as_bytes(),
            common::string(env, "definer").unwrap(),
            &callees,
            "2026-06-01T00:00:00.000000Z",
        );
        if common::boolean(v, "valid") == Some(true) {
            valid += 1;
            match got {
                Ok(d) => {
                    if String::from_utf8(d.bytes.clone()).unwrap() != common::string(v, "flattened").unwrap() {
                        failures.push(format!("{path}: flattened {}", String::from_utf8_lossy(&d.bytes)));
                    } else if Some(d.class_hash.as_str()) != common::string(v, "class_hash") {
                        failures.push(format!("{path}: class_hash {}", d.class_hash));
                    }
                }
                Err(e) => failures.push(format!("{path}: unexpected {e}")),
            }
        } else {
            invalid += 1;
            match got {
                Ok(d) => failures.push(format!("{path}: accepted as {}", d.class_hash)),
                Err(e) if e.code() != common::string(v, "error").unwrap() => {
                    failures.push(format!("{path}: got {e}, expected {}", common::string(v, "error").unwrap()))
                }
                Err(_) => {}
            }
        }
    }
    eprintln!("class vectors through define_class: {valid} valid, {invalid} invalid, {} failures", failures.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!((valid, invalid), (12, 33));
}
