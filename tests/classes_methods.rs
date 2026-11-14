//! Classes (SPEC 16, spec-0.5.0), first group: method calls as sugar for plain reads (16.3),
//! `this`, extension methods, key literals, key normalisation on the methods' functions (16.4, 7,
//! Q6), and defaults (16.7). The tests are derived from the `formula` and `formula-eval` class
//! vectors and from the text of sections 7, 15.5, 15.11, 15.12 and 16; where a case is not a
//! vector, its expected result was checked against the reference tool as a black box
//! (`IMPL-NOTES-classes.md`, "How the expectations were made").
//!
//! Contract items 1 (without `super` and dispatch), 2, 4 and 6 of `CONTRACT-formula-0.5.0-classes.md`.

mod common;

use common::classes::{class_env, class_vectors, dispatch_formula, methods_eval, methods_formula, no_args, FX, ONE};
use datumesh_core::json::{self, Value};
use datumesh_core::KeyNormalisation;
use datumesh_formula::definition::{define_function, define_method};
use datumesh_formula::memory::run_vector;
use datumesh_formula::{define, Code, Compiled, Error, Signature};

/// `src` defined in the class world as a function with no arguments returning `returns`.
fn def(src: &str, returns: &str) -> Result<Compiled, Error> {
    define(src, &class_env("calc.example.net/e", &no_args(returns), None))
}

fn body_of(src: &str, returns: &str) -> String {
    def(src, returns).unwrap_or_else(|e| panic!("{src}: {e}")).body
}

fn code_of(src: &str, returns: &str) -> String {
    match def(src, returns) {
        Ok(c) => panic!("{src}: accepted as {:?}", c.body),
        Err(e) => e.code.as_str().to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// The vectors.

/// The vendored vectors are the 1474 of `c8896ad`; this library runs the five kinds it
/// implements (656) and skips the others (818: the verifier kinds are `datumesh-core`'s).
#[test]
fn the_vectors_are_those_of_spec_0_5_0() {
    let count = |kind: &str| common::vectors_of_kind(kind).len();
    let mut all = 0;
    let mut stack = vec![common::root()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "json") {
                all += 1;
            }
        }
    }
    assert_eq!(all, 1474);
    let run = [("formula", 331), ("formula-eval", 175), ("call", 51), ("excel", 79), ("decimal", 20)];
    for (kind, n) in run {
        assert_eq!(count(kind), n, "{kind}");
    }
    assert_eq!(run.iter().map(|(_, n)| n).sum::<usize>(), 656);
    // The class vectors are the new ones of those kinds, each in one group of the three test
    // commits (`common::classes`).
    assert_eq!(class_vectors("formula").len(), 36);
    assert_eq!(methods_formula().len() + dispatch_formula().len(), 36);
    assert_eq!(class_vectors("formula-eval").len(), 11);
    assert_eq!(methods_eval().len() + common::classes::dispatch_eval().len(), 11);
    assert_eq!(class_vectors("call").len(), 24);
}

/// Every class `formula` vector of this group: 22 valid (method calls, objects, class-typed keys,
/// extension and inherited methods, defaults, each with its plain read) and 12 invalid.
#[test]
fn class_formula_vectors() {
    let t = common::check::tally(methods_formula(), common::check::formula_vector);
    let counts = t.report("class formula vectors (methods group)");
    assert!(t.failures.is_empty(), "{}", t.failures.join("\n"));
    assert_eq!(counts, (22, 12));
}

/// The class `formula-eval` vectors of this group: `this` read one way (`spot`, `forward`), the
/// `asof` default, and the computed key that is not canonical (a fault).
#[test]
fn class_formula_eval_vectors() {
    let t = common::check::tally(methods_eval(), |p, v| common::check::eval_vector("formula-eval", p, v));
    let counts = t.report("class formula-eval vectors (methods group)");
    assert!(t.failures.is_empty(), "{}", t.failures.join("\n"));
    assert_eq!(counts, (3, 1));
}

// ---------------------------------------------------------------------------------------------
// Item 1: a method call is the plain read (16.3).

/// `N(k1, k2).m(a)` resolves to the same canonical AST as the plain read `N/m(k1: ..., a: ...)`,
/// so the same hash, body and inputs (so the same cell and cell id): the 11 pairs of vectors.
#[test]
fn a_method_call_is_the_plain_read() {
    let all = class_vectors("formula");
    let mut pairs = 0;
    for (path, plain) in all.iter().filter(|(p, _)| p.ends_with("-plain.json")) {
        let method_path = path.replace("-plain.json", ".json");
        let method = &all.iter().find(|(p, _)| *p == method_path).unwrap_or_else(|| panic!("{method_path}")).1;
        let compile = |v: &Value| {
            let env = datumesh_formula::Env::from_value(v.get("env").unwrap()).unwrap();
            define(common::string(v, "input").unwrap(), &env).unwrap_or_else(|e| panic!("{path}: {e}"))
        };
        let (a, b) = (compile(method), compile(plain));
        assert_eq!(a.ast, b.ast, "{path}");
        assert_eq!(a.ast.hash(), b.ast.hash(), "{path}");
        assert_eq!(a.body, b.body, "{path}");
        assert_eq!(a.inputs, b.inputs, "{path}");
        assert_eq!(a.asof_dependent, b.asof_dependent, "{path}");
        assert_eq!(a.returns, b.returns, "{path}");
        pairs += 1;
    }
    assert_eq!(pairs, 11);
}

/// Objects bind their keys positionally by the class `order` or labelled, every key once; the
/// method's own arguments then bind after the keys, positionally or labelled.
#[test]
fn keys_bind_by_position_or_by_label() {
    let want = "rates.example.org/fx_pair/rate(base: \"EUR\", date: 2026-03-16, quote: \"USD\")\n";
    for src in [
        "rates.example.org/fx_pair(EUR, USD).rate(2026-03-16)",
        "rates.example.org/fx_pair(quote: USD, base: EUR).rate(date: 2026-03-16)",
        "rates.example.org/fx_pair(EUR, quote: USD).rate(2026-03-16)",
        "rates.example.org/fx_pair(base: \"EUR\", quote: \"USD\").rate(date: 2026-03-16)",
        "rates.example.org/fx_pair(\"EUR\", \"USD\").rate(2026-03-16)",
        "rates.example.org/fx_pair/rate(\"EUR\", \"USD\", 2026-03-16)",
    ] {
        assert_eq!(body_of(src, FX), want, "{src}");
    }
}

/// The codes of binding keys and arguments (15.11, 16.3): checked against the reference tool.
#[test]
fn key_and_argument_errors() {
    for (src, code) in [
        ("rates.example.org/fx_pair(EUR).rate(2026-03-16)", "E_FORMULA_ARG_MISSING"),
        ("rates.example.org/fx_pair(EUR, USD).rate()", "E_FORMULA_ARG_MISSING"),
        ("rates.example.org/fx_pair(EUR, foo: USD).rate(2026-03-16)", "E_FORMULA_ARG_UNKNOWN"),
        ("rates.example.org/fx_pair(EUR, USD).rate(foo: 1, date: 2026-03-16)", "E_FORMULA_ARG_UNKNOWN"),
        ("rates.example.org/fx_pair(EUR, EUR, quote: USD).rate(2026-03-16)", "E_FORMULA_ARG_DUPLICATE"),
        // A label naming a key, in the method's arguments, is a duplicate.
        ("rates.example.org/fx_pair(EUR, USD).rate(base: \"EUR\", date: 2026-03-16)", "E_FORMULA_ARG_DUPLICATE"),
        ("rates.example.org/fx_pair(EUR, USD).rate(date: 2026-03-16, date: 2026-03-17)", "E_FORMULA_ARG_DUPLICATE"),
        // More positional keys, or arguments, than the order lists.
        ("rates.example.org/fx_pair(EUR, USD, GBP).rate(2026-03-16)", "E_FORMULA_ARITY"),
        ("rates.example.org/fx_pair(EUR, USD).rate(2026-03-16, 1)", "E_FORMULA_ARITY"),
        // No `*` or range in an object.
        ("rates.example.org/fx_pair(*, USD).rate(2026-03-16)", "E_FORMULA_PLACEMENT"),
    ] {
        assert_eq!(code_of(src, FX), code, "{src}");
    }
}

/// A method the class does not have, `lookup`'s method read on an object, an object as a value and
/// a method on a value (the vectors, 16.1, 16.3).
#[test]
fn method_errors() {
    assert_eq!(code_of("rates.example.org/fx_pair(EUR, USD).points(2026-03-16)", FX), "E_FORMULA_METHOD");
    // The lookup method is read by its plain name, never on an object (16.1).
    assert_eq!(code_of("rates.example.org/fx_pair(EUR, USD).named(\"cable\")", FX), "E_FORMULA_METHOD");
    assert_eq!(
        body_of("rates.example.org/fx_pair/named(\"cable\")", r#"{"type": "string"}"#),
        "rates.example.org/fx_pair/named(label: \"cable\")\n"
    );
}

/// `N/m` must be bound to the class's method hash: a function bound to another hash under the
/// method's name is `E_FORMULA_METHOD` (16.3).
#[test]
fn the_method_must_be_bound_to_the_classs_hash() {
    let mut env = class_env("calc.example.net/e", &no_args(FX), None);
    // `rates.example.org/fx_pair/rate` now means the descriptor of `spot`.
    let spot = env.functions.get("rates.example.org/fx_pair/spot").unwrap().value.clone();
    env.add_function("rates.example.org/fx_pair/rate", &spot).unwrap();
    let e = define("rates.example.org/fx_pair(EUR, USD).rate(2026-03-16)", &env).unwrap_err();
    assert_eq!(e.code.as_str(), "E_FORMULA_METHOD", "{e}");
    // The plain read of the same name is a read of whatever it is bound to.
    assert!(define("rates.example.org/fx_pair/rate(base: \"EUR\", quote: \"USD\", date: 2026-03-16)", &env).is_ok());
}

/// Extension methods: by qualified name, the function must take the class keys first; `rates...(EUR,
/// USD).calc.example.net/f(...)` against the longest match of 15.2 (`this.calc.example.net/f` is
/// still a read of a function under the host `this.calc.example.net`).
#[test]
fn extension_methods() {
    assert_eq!(
        body_of("rates.example.org/fx_pair(EUR, USD).calc.example.net/fx_extra/carry(2026-03-16)", ONE),
        "calc.example.net/fx_extra/carry(base: \"EUR\", date: 2026-03-16, quote: \"USD\")\n"
    );
    // A qualified name that is not bound.
    assert_eq!(
        code_of("rates.example.org/fx_pair(EUR, USD).calc.example.net/nonexistent(2026-03-16)", FX),
        "E_FORMULA_UNRESOLVED"
    );
    // `this.calc.example.net/f(1)` is the 0.4 reading: a read of a function of the host `this.calc.example.net`.
    let f = r#"{"code": {"body": "", "lang": "stored"}, "inputs": [], "signature": {"args": {"x": {"type": "number", "unit": "1", "valid": false}}, "order": ["x"], "returns": {"type": "number", "unit": "1"}}}"#;
    let mut env = class_env("calc.example.net/e", &no_args(ONE), None);
    env.add_function("this.calc.example.net/f", &json::parse(f.as_bytes()).unwrap()).unwrap();
    assert_eq!(define("this.calc.example.net/f(1)", &env).unwrap().body, "this.calc.example.net/f(x: 1)\n");
}

/// An inherited method is read through the subclass's name, with the keys the parent had (the
/// parent's, a prefix of the subclass's order): `fx_ndf(EUR, USD, "F1").cross("GBP", date)`.
#[test]
fn inherited_methods_take_the_parents_keys() {
    assert_eq!(
        body_of("calc.example.net/fx_ndf(EUR, USD, \"F1\").cross(\"GBP\", 2026-03-16)", FX),
        "calc.example.net/fx_ndf/cross(base: \"EUR\", date: 2026-03-16, quote: \"USD\", via: \"GBP\")\n"
    );
    // The subclass's own method takes the keys the subclass declared.
    assert_eq!(code_of("calc.example.net/fx_ndf(EUR, USD).rate(2026-03-16)", FX), "E_FORMULA_ARG_MISSING");
}

// ---------------------------------------------------------------------------------------------
// Key literals and class-typed positions (16.3, 16.4, 16.5).

/// A key literal is a string in a `string` position: an uppercase word, or digits and `D`, `M` or
/// `Y` (a period). `1M` is a string here and stays one watt elsewhere (15.2).
#[test]
fn key_literals_are_strings() {
    let want = "rates.example.org/deposit(currency: \"EUR\", date: 2026-03-16, tenor: \"1M\")\n";
    assert_eq!(body_of("rates.example.org/deposit(EUR, 1M, 2026-03-16)", ONE), want);
    assert_eq!(body_of("rates.example.org/deposit(\"EUR\", \"1M\", 2026-03-16)", ONE), want);
    // In a plain read of any function, not only of a class's methods.
    assert_eq!(
        body_of("rates.example.org/deposit(1M, 1M, 2026-03-16)", ONE),
        "rates.example.org/deposit(currency: \"1M\", date: 2026-03-16, tenor: \"1M\")\n"
    );
    // In a position of another type a key literal is a type error; elsewhere it stays reserved.
    assert_eq!(code_of("rates.example.org/deposit(EUR, 1, 2026-03-16)", ONE), "E_FORMULA_TYPE");
    assert_eq!(code_of("EUR = \"EUR\"", r#"{"type": "boolean"}"#), "E_FORMULA_RESERVED");
    assert_eq!(code_of("1M = 1", r#"{"type": "boolean"}"#), "E_FORMULA_RESERVED");
    assert_eq!(code_of("EUR", r#"{"type": "string"}"#), "E_FORMULA_RESERVED");
}

/// A literal key not canonical under its normalisation is `E_FORMULA_KEY` at define, for a string
/// or a key literal, in an object or in the plain read (the normalisation is the method
/// function's, 7, D92); every rule but `none` also refuses the empty string (16.4).
#[test]
fn literal_keys_are_checked_at_define() {
    for src in [
        "rates.example.org/fx_pair(EUR, \"usd\").rate(2026-03-16)",
        "rates.example.org/fx_pair(\"eur\", USD).rate(2026-03-16)",
        "rates.example.org/fx_pair(EUR, \"\").rate(2026-03-16)",
        "rates.example.org/fx_pair(EUR, \"US D\").rate(2026-03-16)",
        "rates.example.org/fx_pair/rate(\"EUR\", \"usd\", 2026-03-16)",
        "rates.example.org/fx_pair/rate(base: \"eur\", quote: \"USD\", date: 2026-03-16)",
        "calc.example.net/bond/price(\"xs0001\", 2026-03-16)",
    ] {
        assert_eq!(code_of(src, FX), "E_FORMULA_KEY", "{src}");
    }
    // `none` accepts any string valid by 3.5, `Gen` and `quran` included.
    assert!(def("scripture.example.org/verse(\"quran\", \"2\", 1, 255).text(\"uthmani\")", r#""blob""#).is_ok());
}

/// Class-typed positions take a key encoding (16.5): an object with literal keys, or a string
/// literal that is the encoding; a malformed string is `E_FORMULA_KEY`, an object or encoding of
/// another class `E_FORMULA_TYPE`, a non-canonical key in an object literal `E_FORMULA_KEY`.
#[test]
fn class_typed_positions_take_key_encodings() {
    let want = "rates.example.org/ccy_pair/rate(base: \"iso.example.org/currency(code:\"\"EUR\"\")\", date: 2026-03-16, quote: \"iso.example.org/currency(code:\"\"USD\"\")\")\n";
    for src in [
        "rates.example.org/ccy_pair(iso.example.org/currency(EUR), iso.example.org/currency(USD)).rate(2026-03-16)",
        "rates.example.org/ccy_pair(iso.example.org/currency(code: EUR), iso.example.org/currency(USD)).rate(2026-03-16)",
        "rates.example.org/ccy_pair(base: iso.example.org/currency(EUR), quote: iso.example.org/currency(USD)).rate(2026-03-16)",
        "rates.example.org/ccy_pair/rate(iso.example.org/currency(EUR), iso.example.org/currency(USD), 2026-03-16)",
        "rates.example.org/ccy_pair(\"iso.example.org/currency(code:\"\"EUR\"\")\", \"iso.example.org/currency(code:\"\"USD\"\")\").rate(2026-03-16)",
    ] {
        assert_eq!(body_of(src, ONE), want, "{src}");
    }
    for (src, code) in [
        ("rates.example.org/ccy_pair(\"nonsense\", iso.example.org/currency(USD)).rate(2026-03-16)", "E_FORMULA_KEY"),
        ("rates.example.org/ccy_pair(iso.example.org/currency(\"eur\"), iso.example.org/currency(USD)).rate(2026-03-16)", "E_FORMULA_KEY"),
        ("rates.example.org/ccy_pair(rates.example.org/fx_pair(EUR, USD), iso.example.org/currency(USD)).rate(2026-03-16)", "E_FORMULA_TYPE"),
        (
            "rates.example.org/ccy_pair(\"rates.example.org/fx_pair(base:\"\"EUR\"\",quote:\"\"USD\"\")\", iso.example.org/currency(USD)).rate(2026-03-16)",
            "E_FORMULA_TYPE",
        ),
        ("rates.example.org/ccy_pair(iso.example.org/currency(EUR, USD), iso.example.org/currency(USD)).rate(2026-03-16)", "E_FORMULA_ARITY"),
    ] {
        assert_eq!(code_of(src, ONE), code, "{src}");
    }
}

// ---------------------------------------------------------------------------------------------
// `this` (16.3).

const SPOT_SIGNATURE: &str = r#"{"args": {"base": {"type": "string", "valid": false, "key_normalisation": "upper_ascii"},
    "date": {"type": "date", "valid": true}, "quote": {"type": "string", "valid": false, "key_normalisation": "upper_ascii"}},
    "order": ["base", "quote", "date"], "returns": {"type": "number", "unit": {"of": {"arg": "quote"}, "per": {"arg": "base"}}}}"#;

/// In a method's body, `this.m(args)` reads `N/m` with the method's own key arguments as keys
/// (`E_FORMULA_RESERVED` anywhere else); a method of the class by name, one way.
#[test]
fn this_in_a_method() {
    let env = || class_env("rates.example.org/fx_pair/spot", SPOT_SIGNATURE, Some("rates.example.org/fx_pair"));
    let ok = |src: &str| define(src, &env()).unwrap_or_else(|e| panic!("{src}: {e}"));
    let c = ok("this.rate(date)");
    assert_eq!(c.body, "rates.example.org/fx_pair/rate(base: base, date: date, quote: quote)\n");
    assert_eq!(c.inputs, ["rates.example.org/fx_pair/rate"]);
    assert_eq!(
        String::from_utf8(c.ast.to_jcs()).unwrap(),
        r#"{"args":{"base":{"name":"base","node":"var"},"date":{"name":"date","node":"var"},"quote":{"name":"quote","node":"var"}},"fn":"rates.example.org/fx_pair/rate","node":"read"}"#
    );
    assert_eq!(
        ok("this.cross(via: \"GBP\", date: date)").body,
        "rates.example.org/fx_pair/cross(base: base, date: date, quote: quote, via: \"GBP\")\n"
    );
    assert_eq!(ok("this.forward(\"1M\", date)").inputs, ["rates.example.org/fx_pair/forward"]);
    assert_eq!(
        ok("this.rate(date) * 2").body,
        "rates.example.org/fx_pair/rate(base: base, date: date, quote: quote) * 2\n"
    );
    for (src, code) in [
        ("this.points(date)", "E_FORMULA_METHOD"),
        ("this.named(\"x\")", "E_FORMULA_METHOD"),
        ("this.rate(base: base, date: date)", "E_FORMULA_ARG_DUPLICATE"),
        ("this", "E_FORMULA_RESERVED"),
    ] {
        let e = define(src, &env()).unwrap_err();
        assert_eq!(e.code.as_str(), code, "{src}: {e}");
    }
    // Outside a method's body: reserved (`invalid/formula-class-this-outside-method`).
    assert_eq!(code_of("this.rate(2026-03-16)", FX), "E_FORMULA_RESERVED");
    // Under a subclass's method `this` is read through the subclass's name.
    let sub = class_env("calc.example.net/fx_ndf/spot", SPOT_SIGNATURE, Some("calc.example.net/fx_ndf"));
    assert_eq!(
        define("this.rate(date)", &sub).unwrap().body,
        "calc.example.net/fx_ndf/rate(base: base, date: date, quote: quote)\n"
    );
}

/// The vectors' class world is reproduced by define: the bodies of the methods that read their own
/// class, written with `this`, give the very function hashes the class descriptor holds, through
/// `define_method` (the define path of item 7, over an environment as callee source).
#[test]
fn define_method_reproduces_the_vectors_function_hashes() {
    let world = common::vector("formula/class-fx-rate.json");
    let env = world.get("env").unwrap();
    let functions = env.get("functions").unwrap();
    let class = datumesh_formula::Env::from_value(env).unwrap();
    let fx_pair = class.classes.get("rates.example.org/fx_pair").expect("env.classes is read");
    for (name, method, source) in [
        ("rates.example.org/fx_pair/spot", "spot", "this.rate(date)"),
        (
            "rates.example.org/fx_pair/forward",
            "forward",
            "(this.spot(date) + this.rate(date)) / 2 * (1 + rates.example.org/deposit(quote, tenor, date)) / (1 + rates.example.org/deposit(base, tenor, date))",
        ),
    ] {
        let signature = functions.get(name).unwrap().get("signature").unwrap();
        let d = define_method(
            "rates.example.org/fx_pair",
            name,
            source,
            signature,
            &Default::default(),
            &class,
            "2026-06-01T00:00:00.000000Z",
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(Some(&d.fn_hash), fx_pair.methods.get(method), "{name}");
        assert_eq!(d.descriptor.body, functions.get(name).unwrap().get("code").unwrap().get("body").unwrap().as_str().unwrap());
    }
}

/// `define_function` accepts method calls too: the descriptor is the plain read's.
#[test]
fn define_function_accepts_method_calls() {
    let world = common::vector("formula/class-fx-rate.json");
    let class = datumesh_formula::Env::from_value(world.get("env").unwrap()).unwrap();
    let sig = json::parse(no_args(FX).as_bytes()).unwrap();
    let at = "2026-06-01T00:00:00.000000Z";
    let a = define_function(
        "calc.example.net/e1",
        "rates.example.org/fx_pair(EUR, USD).rate(2026-03-16)",
        &sig,
        &Default::default(),
        &class,
        at,
    )
    .unwrap();
    let b = define_function(
        "calc.example.net/e1",
        "rates.example.org/fx_pair/rate(base: \"EUR\", quote: \"USD\", date: 2026-03-16)",
        &sig,
        &Default::default(),
        &class,
        at,
    )
    .unwrap();
    assert_eq!(a.fn_hash, b.fn_hash);
    assert_eq!(a.descriptor.inputs, ["rates.example.org/fx_pair/rate"]);
    assert_eq!(a.callees.keys().collect::<Vec<_>>(), ["rates.example.org/fx_pair/rate"]);
}

// ---------------------------------------------------------------------------------------------
// The environment's and the descriptors' new members.

/// `Env::from_value` reads `classes` and `method_of`; `Classes` finds a parent by hash.
#[test]
fn the_environment_reads_classes() {
    let v = common::vector("invalid/formula-class-super-without-parent.json");
    let env = datumesh_formula::Env::from_value(v.get("env").unwrap()).unwrap();
    assert_eq!(env.method_of.as_deref(), Some("rates.example.org/fx_pair"));
    assert!(env.classes.get("rates.example.org/fx_pair").is_some());
    let ndf = env.classes.get("calc.example.net/fx_ndf").unwrap();
    let parent = ndf.parent.as_deref().expect("fx_ndf's flattened descriptor names its parent by hash");
    let (name, _) = env.classes.by_hash(parent).expect("the parent is in the environment");
    assert_eq!(name, "rates.example.org/fx_pair");
    assert!(env.classes.names().any(|n| n == "scripture.example.org/verse"));
    // A class has no `method_of` unless the vector says so.
    let plain =
        datumesh_formula::Env::from_value(common::vector("formula/class-fx-rate.json").get("env").unwrap()).unwrap();
    assert_eq!(plain.method_of, None);
    assert!(!plain.classes.is_empty());
}

/// A function's `classes`, `default` and `key_normalisation` (7) and a returned key's classes.
#[test]
fn signatures_read_the_new_members() {
    let world = common::vector("formula/class-fx-rate.json");
    let fns = world.get("env").unwrap().get("functions").unwrap();
    let sig = |name: &str| Signature::from_value(fns.get(name).unwrap().get("signature").unwrap()).unwrap();
    let rate = sig("rates.example.org/fx_pair/rate");
    assert_eq!(rate.args["base"].extras.key_normalisation, Some(KeyNormalisation::UpperAscii));
    assert_eq!(rate.args["date"].extras.key_normalisation, None);
    assert!(sig("rates.example.org/fx_rate").args["date"].extras.default_asof);
    assert!(!rate.args["date"].extras.default_asof);
    let ccy = sig("rates.example.org/ccy_pair/rate");
    assert_eq!(ccy.args["base"].extras.classes.as_deref(), Some(&["iso.example.org/currency".to_string()][..]));
    let price_of = sig("calc.example.net/price_of");
    assert_eq!(price_of.args["instrument"].extras.classes.as_ref().map(Vec::len), Some(2));
    let next = sig("scripture.example.org/verse/next");
    assert_eq!(next.returns_classes.as_deref(), Some(&["scripture.example.org/verse".to_string()][..]));
    assert_eq!(rate.returns_classes, None);
}

// ---------------------------------------------------------------------------------------------
// Item 2: key normalisation on function arguments (Q6, 7, 16.4).

/// The codes of spec-0.5.0 (15.11): their order in `Code::ALL` is the table's, and the two fault
/// codes are faults, memoized as faults are (a property of the cell), not the mesh's failures; and
/// `otherwise` catches neither (15.12).
#[test]
fn the_new_codes() {
    let all: Vec<&str> = Code::ALL.iter().map(|c| c.as_str()).collect();
    let at = |s: &str| all.iter().position(|c| *c == s).unwrap_or_else(|| panic!("{s}"));
    assert_eq!(at("E_FORMULA_METHOD"), at("E_FORMULA_NOT_CANONICAL") + 1);
    assert_eq!(at("E_FORMULA_DISPATCH"), at("E_FORMULA_METHOD") + 1);
    assert_eq!(at("E_FORMULA_KEY"), at("E_FORMULA_DISPATCH") + 1);
    assert_eq!(at("E_FORMULA_DISPATCH_KEY"), at("E_FORMULA_FUTURE") + 1);
    assert_eq!(at("E_FORMULA_NOT_COVERED"), at("E_FORMULA_DISPATCH_KEY") + 1);
    for c in [Code::Key, Code::DispatchKey] {
        assert!(c.is_fault(), "{c}");
        assert!(!c.is_catchable(), "{c}");
        assert!(!c.is_mesh_failure(), "{c}");
        assert_eq!(Code::from_name(c.as_str()), Some(c));
        assert!(datumesh_core::Fault::from_code(c.as_str()).is_some(), "core's Fault has {c}");
    }
    // The define-time codes are not faults: E_FORMULA_DISPATCH at evaluation is the mesh's failure (16.6).
    for c in [Code::Method, Code::Dispatch] {
        assert!(!c.is_fault() && !c.is_catchable(), "{c}");
    }
}

fn bond_price_vector() -> Value {
    common::vector("invalid/formula-eval-class-key-computed.json")
}

/// `bond/price(isin: lower(isin))`: the computed value is not canonical under `upper_ascii`, the rule
/// the function's argument carries: a fault `E_FORMULA_KEY`, no value (the vector); with `upper` the
/// same function evaluates and reads the stored price.
#[test]
fn a_computed_key_is_checked_at_evaluation() {
    let v = bond_price_vector();
    assert_eq!(run_vector(&v).unwrap().unwrap_err(), Code::Key);
    let text = String::from_utf8(v.to_jcs()).unwrap().replace("lower(isin)", "upper(isin)");
    let v = json::parse(text.as_bytes()).unwrap();
    let r = run_vector(&v).unwrap().unwrap();
    assert_eq!(r.computed.outcome.value.lexical(), "101.25");
    // One input: the receipt of the bond's price (`formula-eval/class-dispatch-bond`).
    assert_eq!(r.computed.outcome.lineage, ["4f51923f187671d24057047af29bc066cab3ea0f96f422bc2e066cf11ac06a23"]);
}

/// `calc.example.net/price_lower` as `bond_price_vector` has it, its body reading `isin` as it is
/// and the cell asked with the `isin` argument `value`.
fn price_with_isin(value: &str) -> Value {
    let text = String::from_utf8(bond_price_vector().to_jcs()).unwrap().replace("lower(isin)", "isin").replace(
        "\"isin\":{\"type\":\"string\",\"value\":\"XS0001\"}},\"fn\":\"calc.example.net/price_lower\"",
        &format!("\"isin\":{{\"type\":\"string\",\"value\":\"{value}\"}}}},\"fn\":\"calc.example.net/price_lower\""),
    );
    json::parse(text.as_bytes()).unwrap()
}

/// The argument of the cell being evaluated itself may not be canonical: passed to the read as it
/// is, it is checked there (`xs0001` fails where `XS0001` reads).
#[test]
fn a_cells_own_argument_is_checked_where_it_is_read() {
    let ok = run_vector(&price_with_isin("XS0001")).unwrap().unwrap();
    assert_eq!(ok.computed.outcome.value.lexical(), "101.25");
    assert_eq!(run_vector(&price_with_isin("xs0001")).unwrap().unwrap_err(), Code::Key);
}

/// The empty string is a key value valid by 3.5 and refused by every rule but `none` (16.4).
#[test]
fn an_empty_computed_key_is_a_fault() {
    assert_eq!(run_vector(&price_with_isin("")).unwrap().unwrap_err(), Code::Key);
}

// ---------------------------------------------------------------------------------------------
// Item 4: defaults (16.7).

const RATE_GBP: &str = r#"{"type": "number", "unit": {"of": "GBP", "per": "EUR"}}"#;

/// `"default": "asof"` compiles to `date_of(asof())` for a date and `asof()` for an instant in the
/// AST, which makes the function asof-dependent; a bound argument does not.
#[test]
fn defaults_compile_to_asof() {
    let c = def("rates.example.org/fx_rate(EUR, GBP)", RATE_GBP).unwrap();
    assert_eq!(c.body, "rates.example.org/fx_rate(base: \"EUR\", date: date_of(asof()), quote: \"GBP\")\n");
    assert!(c.asof_dependent);
    assert_eq!(c.inputs, ["rates.example.org/fx_rate"]);
    let explicit =
        def("rates.example.org/fx_rate(base: \"EUR\", quote: \"GBP\", date: date_of(asof()))", RATE_GBP).unwrap();
    assert_eq!(c.ast, explicit.ast);
    // Inside an expression, the whole function is asof-dependent.
    let c = def("rates.example.org/fx_rate(EUR, GBP) * 2", RATE_GBP).unwrap();
    assert!(c.asof_dependent);
    assert_eq!(c.body, "rates.example.org/fx_rate(base: \"EUR\", date: date_of(asof()), quote: \"GBP\") * 2\n");
    // Bound explicitly, the default does not apply.
    let c = def("rates.example.org/fx_rate(EUR, GBP, 2026-03-16)", RATE_GBP).unwrap();
    assert!(!c.asof_dependent);
    assert_eq!(c.body, "rates.example.org/fx_rate(base: \"EUR\", date: 2026-03-16, quote: \"GBP\")\n");
    // A valid-time argument without a default is still required.
    assert_eq!(code_of("rates.example.org/fx_pair/rate(EUR, USD)", FX), "E_FORMULA_ARG_MISSING");
}

/// An instant default is `asof()` itself.
#[test]
fn an_instant_default_is_asof() {
    let snap = r#"{"code": {"body": "", "lang": "stored"}, "inputs": [], "signature": {"args": {"at": {"type": "instant", "valid": true, "default": "asof"},
        "k": {"type": "string", "valid": false}}, "order": ["k", "at"], "returns": {"type": "number", "unit": "1"}}}"#;
    let mut env = class_env("calc.example.net/e", &no_args(ONE), None);
    env.add_function("calc.example.net/snap", &json::parse(snap.as_bytes()).unwrap()).unwrap();
    let c = define("calc.example.net/snap(K)", &env).unwrap();
    assert_eq!(c.body, "calc.example.net/snap(at: asof(), k: \"K\")\n");
    assert!(c.asof_dependent);
    assert_eq!(
        String::from_utf8(c.ast.to_jcs()).unwrap(),
        r#"{"args":{"at":{"args":[],"fn":"asof","named":{},"node":"call"},"k":{"node":"str","value":"K"}},"fn":"calc.example.net/snap","node":"read"}"#
    );
    let c = define("calc.example.net/snap(K, 2026-03-16T00:00:00.000000Z)", &env).unwrap();
    assert!(!c.asof_dependent);
}

/// Evaluating a default: the date is the evaluation asof's date, the receipt's `asof` is the
/// request's asof (such receipts are one per requested asof, 15.12), whatever the input's asof was;
/// a request on another day reads another cell.
#[test]
fn a_default_follows_the_evaluation_asof() {
    let base = common::vector("formula-eval/class-default-asof.json");
    let with_asof = |asof: &str| {
        let text = String::from_utf8(base.to_jcs())
            .unwrap()
            .replace("\"asof\":\"2026-03-16T18:00:00.000000Z\",\"cell\"", &format!("\"asof\":\"{asof}\",\"cell\""));
        assert!(text.contains(asof), "the vector's asof is replaced");
        json::parse(text.as_bytes()).unwrap()
    };
    for asof in ["2026-03-16T18:00:00.000000Z", "2026-03-16T23:59:59.999999Z", "2026-03-16T16:00:00.000000Z"] {
        let r = run_vector(&with_asof(asof)).unwrap().unwrap();
        assert_eq!(r.computed.outcome.receipt_asof, asof);
        assert_eq!(r.computed.outcome.value.lexical(), "0.8556");
        assert_eq!(r.computed.outcome.tx.as_deref(), Some("2026-03-16T16:00:00.000000Z"));
    }
    // The next day's date: the rate of 2026-03-17 has no value.
    assert_eq!(run_vector(&with_asof("2026-03-17T00:00:00.000000Z")).unwrap().unwrap_err(), Code::NotCovered);
}
