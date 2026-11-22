//! Classes (SPEC 5.8, 16.3, 16.7, spec-0.5.0), second group: requests. A call reads every
//! argument by the declared type of its position (Q1, D91), a class-typed position takes a key
//! encoding, bare names in function position resolve through the alias map, `N[label]` resolves
//! through the class's `lookup` method as of the call's asof (D93), a non-canonical key is
//! `E_KEY_NORMALISATION` and a malformed encoding `E_KEY_ENCODING`, and the call's result reports
//! `resolved`. The tests are derived from the `call` vectors and from the text of 5.8 and 16.7;
//! where a case is not a vector, its expectation was checked against the reference tool as a
//! black box (`IMPL-NOTES-classes.md`).
//!
//! Contract item 5, and items 1 and 6 for requests.

mod common;

use std::cell::RefCell;

use common::classes::class_vectors;
use datumesh_core::json;
use datumesh_core::{Cell, ClassDescriptor, FunctionDescriptor, ResolutionKind};
use datumesh_formula::call::{parse_call, Call, CallEnv, CallError, MapCallEnv};

const NOW: &str = "2026-07-01T00:00:00.000000Z";
const ASOF: &str = "2026-06-01T00:00:00.000000Z";

/// The class world of the `call` vectors, with `lookups` for `fx_pair`.
fn env() -> MapCallEnv {
    MapCallEnv::from_value(common::vector("call/class-lookup.json").get("env").unwrap()).unwrap()
}

fn function(json_text: &str) -> FunctionDescriptor {
    FunctionDescriptor::from_value(&json::parse(json_text.as_bytes()).unwrap()).unwrap()
}

/// The call, or its code.
fn parse(src: &str, env: &dyn CallEnv) -> Result<Call, String> {
    parse_call(src, env).map_err(|e| match e {
        CallError::Code(c, _) => c.to_string(),
        other => other.to_string(),
    })
}

fn uri(src: &str) -> String {
    parse(src, &env()).unwrap_or_else(|e| panic!("{src}: {e}")).uri()
}

fn code(src: &str) -> String {
    match parse(src, &env()) {
        Ok(c) => panic!("{src}: parsed to {}", c.uri()),
        Err(e) => e,
    }
}

fn resolved(c: &Call) -> Vec<(String, String, String)> {
    c.resolved
        .as_ref()
        .map(|r| r.resolutions.iter().map(|x| (x.kind.as_str().to_string(), x.from.clone(), x.to.clone())).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// The vectors.

/// Every class `call` vector (10 valid, 14 invalid): the cell URI, or the code, and `resolved`
/// when the vector carries it (and none when it does not).
#[test]
fn class_call_vectors() {
    let t = common::check::tally(class_vectors("call"), common::check::call_vector);
    let counts = t.report("class call vectors");
    assert!(t.failures.is_empty(), "{}", t.failures.join("\n"));
    assert_eq!(counts, (10, 14));
}

/// `N(keys).m(args)` is the cell of the plain read `N/m(...)`: the same cell reference, URI and,
/// with the function hash of `N/m`, the same cell id (16.3).
#[test]
fn a_method_call_is_the_cell_of_its_plain_read() {
    let env = env();
    for (method, plain, function) in [
        (
            "rates.example.org/fx_pair(EUR, USD).rate(2026-03-16)",
            "rates.example.org/fx_pair/rate(\"EUR\", \"USD\", 2026-03-16)",
            "rates.example.org/fx_pair/rate",
        ),
        (
            "scripture.example.org/verse(quran, 2, 1, 255).text(uthmani)",
            "scripture.example.org/verse/text(quran, 2, 1, 255, uthmani)",
            "scripture.example.org/verse/text",
        ),
        (
            "rates.example.org/fx_pair(EUR, USD).calc.example.net/fx_extra/carry(2026-03-16)",
            "calc.example.net/fx_extra/carry(EUR, USD, 2026-03-16)",
            "calc.example.net/fx_extra/carry",
        ),
        (
            "calc.example.net/fx_ndf(EUR, USD, F1).cross(GBP, 2026-03-16)",
            // K5 (D115, BREAKING): an inherited method is the cell of the class that defines it.
            "rates.example.org/fx_pair/cross(EUR, USD, GBP, 2026-03-16)",
            "rates.example.org/fx_pair/cross",
        ),
    ] {
        let (a, b) = (parse(method, &env).unwrap_or_else(|e| panic!("{method}: {e}")), parse(plain, &env).unwrap());
        assert_eq!(a.cell, b.cell, "{method}");
        assert_eq!(a.uri(), b.uri(), "{method}");
        let fn_hash = env.functions[function].hash();
        let id = |c: &Call| {
            Cell {
                ns: c.cell.ns.clone(),
                fn_name: c.cell.fn_name.clone(),
                fn_hash: fn_hash.clone(),
                args: c.cell.args.clone(),
            }
            .id()
        };
        assert_eq!(id(&a), id(&b), "{method}: the same cell id");
        // The plain read resolved nothing; the method call said so.
        assert!(b.resolved.is_none(), "{plain}");
        assert!(resolved(&a).iter().any(|(k, _, _)| k == "method"), "{method}");
    }
}

// ---------------------------------------------------------------------------------------------
// Values are read by their position's type (5.8, D91).

/// An unquoted value is exactly one token of the dialect, read by the type of its position: in a
/// `string` position it is that string, whatever the token kind (a word, a number, a date, `true`,
/// a period); a quoted string is only a string.
#[test]
fn values_are_read_by_the_type_of_their_position() {
    let d = "cell://rates.example.org/deposit";
    for (src, want) in [
        ("rates.example.org/deposit(EUR, 1M, 2026-03-16)", format!("{d}?currency=EUR&date=2026-03-16&tenor=1M")),
        (
            "rates.example.org/deposit(\"EUR\", \"1M\", 2026-03-16)",
            format!("{d}?currency=EUR&date=2026-03-16&tenor=1M"),
        ),
        ("rates.example.org/deposit(true, 1M, 2026-03-16)", format!("{d}?currency=true&date=2026-03-16&tenor=1M")),
        ("rates.example.org/deposit(-5, 1M, 2026-03-16)", format!("{d}?currency=-5&date=2026-03-16&tenor=1M")),
        (
            "rates.example.org/deposit(2026-03-16, 1M, 2026-03-16)",
            format!("{d}?currency=2026-03-16&date=2026-03-16&tenor=1M"),
        ),
        ("rates.example.org/deposit(EUR, 1.5, 2026-03-16)", format!("{d}?currency=EUR&date=2026-03-16&tenor=1.5")),
        ("rates.example.org/deposit(EUR, \"\", 2026-03-16)", format!("{d}?currency=EUR&date=2026-03-16&tenor=")),
        (
            "rates.example.org/deposit(\"a\"\"b\", 1M, 2026-03-16)",
            format!("{d}?currency=a%22b&date=2026-03-16&tenor=1M"),
        ),
    ] {
        assert_eq!(uri(src), want, "{src}");
    }
    // The token 2 is the string "2" in a string position and a number in a number position.
    assert_eq!(
        uri("scripture.example.org/verse/text(quran, 2, 1, 255, uthmani)"),
        "cell://scripture.example.org/verse/text?book=2&chapter=1&corpus=quran&edition=uthmani&verse=255"
    );
}

/// Anything else is quoted (`E_PARSE_URI`); a value of the wrong kind in a typed position is
/// `E_CANON_TYPE`, a lexical form the dialect refuses has its type's code.
#[test]
fn what_is_not_one_token_is_quoted() {
    for (src, want) in [
        ("rates.example.org/deposit(a.b, 1M, 2026-03-16)", "E_PARSE_URI"),
        ("rates.example.org/deposit(a b, 1M, 2026-03-16)", "E_PARSE_URI"),
        ("scripture.example.org/verse(quran, 2, 1, 255).text(ux-lc)", "E_PARSE_URI"),
        ("rates.example.org/deposit(EUR, 1M, \"2026-03-16\")", "E_CANON_TYPE"),
        ("rates.example.org/deposit(EUR, 1M, abc)", "E_CANON_TYPE"),
        ("rates.example.org/deposit(EUR, 1M, 2026-03-16T00:00:00.000000Z)", "E_CANON_TYPE"),
        ("rates.example.org/deposit(EUR, 1M, 2026-02-30)", "E_CANON_DATE"),
        ("rates.example.org/deposit(EUR, 01, 2026-03-16)", "E_CANON_NUMBER"),
        ("rates.example.org/deposit(EUR, 1M, 2026-03-16)?asof=2026-06-01", "E_CANON_INSTANT"),
        ("rates.example.org/deposit(EUR, 1M, 2026-03-16)?foo=1", "E_PARSE_URI"),
    ] {
        assert_eq!(code(src), want, "{src}");
    }
}

/// The order of checks (5.8): the text, `?asof=`, the callee, the arguments left to right (one past
/// `order` or unknown, one bound twice), then the missing ones, then each value; for a method call
/// the receiver's keys are bound and read first, each as it is written, then the method's arguments.
#[test]
fn the_order_of_checks() {
    for (src, want) in [
        ("rates.example.org/deposit(EUR, 1M)", "E_ARG_MISSING"),
        ("rates.example.org/deposit(EUR, 1M, 2026-03-16, 1)", "E_ARG_UNKNOWN"),
        ("rates.example.org/deposit(currency: EUR, 1M, 2026-03-16)", "E_PARSE_URI"),
        ("rates.example.org/deposit(currency: EUR, nope: 1M, date: 2026-03-16)", "E_ARG_UNKNOWN"),
        (
            "rates.example.org/deposit(currency: EUR, currency: GBP, date: 2026-03-16, tenor: 1M)",
            "E_CANON_ARG_DUPLICATE",
        ),
        // The receiver's keys first: a key not canonical comes before the method's arguments' faults.
        ("rates.example.org/fx_pair(EUR, usd).rate(notadate)", "E_KEY_NORMALISATION"),
        ("rates.example.org/fx_pair(eur, USD).rate(2026-03-16, 1)", "E_KEY_NORMALISATION"),
        ("rates.example.org/fx_pair(eur, USD).rate()", "E_KEY_NORMALISATION"),
        ("rates.example.org/fx_pair(EUR).rate(2026-03-16)", "E_ARG_MISSING"),
        ("rates.example.org/fx_pair(EUR, USD, GBP).rate(2026-03-16)", "E_ARG_UNKNOWN"),
        ("rates.example.org/fx_pair(EUR, USD).rate(base: GBP, date: 2026-03-16)", "E_CANON_ARG_DUPLICATE"),
        ("rates.example.org/fx_pair(EUR, USD).rate(notadate)", "E_CANON_TYPE"),
    ] {
        assert_eq!(code(src), want, "{src}");
    }
}

/// Keys bind by position or label; the method's arguments after them; a call of a class names one
/// method.
#[test]
fn objects_in_calls() {
    let want = "cell://rates.example.org/fx_pair/rate?base=EUR&date=2026-03-16&quote=USD";
    for src in [
        "rates.example.org/fx_pair(EUR, USD).rate(2026-03-16)",
        "rates.example.org/fx_pair(base: EUR, quote: USD).rate(date: 2026-03-16)",
        "rates.example.org/fx_pair(quote: USD, base: EUR).rate(date: 2026-03-16)",
        "rates.example.org/fx_pair(\"EUR\", quote: \"USD\").rate(2026-03-16)",
    ] {
        assert_eq!(uri(src), want, "{src}");
    }
    assert_eq!(
        uri("rates.example.org/fx_pair(EUR, USD).cross(GBP, 2026-03-16)"),
        "cell://rates.example.org/fx_pair/cross?base=EUR&date=2026-03-16&quote=USD&via=GBP"
    );
    // An inherited method through the subclass's name, with the parent's keys: the cell of the
    // defining class (K5, D115, BREAKING; it was fx_ndf/forward).
    assert_eq!(
        uri("calc.example.net/fx_ndf(EUR, USD, F1).forward(1M, 2026-03-16)"),
        "cell://rates.example.org/fx_pair/forward?base=EUR&date=2026-03-16&quote=USD&tenor=1M"
    );
    for (src, want) in [
        ("rates.example.org/fx_pair(EUR, USD)", "E_PARSE_URI"),
        ("rates.example.org/fx_pair(EUR, USD).rate", "E_PARSE_URI"),
        ("rates.example.org/fx_pair(EUR, USD).rate(2026-03-16).rate(2026-03-16)", "E_PARSE_URI"),
        ("rates.example.org/fx_pair(EUR, USD).points(2026-03-16)", "E_NAME_UNBOUND"),
        ("rates.example.org/fx_pair(EUR, USD).calc.example.net/nonexistent(2026-03-16)", "E_NAME_UNBOUND"),
    ] {
        assert_eq!(code(src), want, "{src}");
    }
}

/// A key not canonical under its normalisation is `E_KEY_NORMALISATION` (a `bad_request` code),
/// in an object or in the plain read of a function whose argument carries the rule; an unquoted
/// token is checked as a quoted one is.
#[test]
fn keys_are_checked_against_their_normalisation() {
    for src in [
        "rates.example.org/fx_pair(\"eur\", \"USD\").rate(2026-03-16)",
        "rates.example.org/fx_pair(eur, USD).rate(2026-03-16)",
        "rates.example.org/fx_pair/rate(\"EUR\", \"usd\", 2026-03-16)",
        "rates.example.org/fx_pair/rate(EUR, usd, 2026-03-16)",
        "calc.example.net/bond/price(xs0001, 2026-03-16)",
        "rates.example.org/fx_pair(EUR, \"\").rate(2026-03-16)",
    ] {
        assert_eq!(code(src), "E_KEY_NORMALISATION", "{src}");
    }
    assert!(uri("calc.example.net/bond/price(XS0001, 2026-03-16)").starts_with("cell://calc.example.net/bond/price?"));
}

/// Class-typed positions take a key encoding (16.5): the object, or the encoding quoted; a malformed
/// encoding is `E_KEY_ENCODING`; the object of another class, or a bare token, is a value of the
/// wrong kind (`E_CANON_TYPE`); an object's key not canonical is `E_KEY_NORMALISATION`.
#[test]
fn class_typed_positions_take_key_encodings() {
    let want = "cell://rates.example.org/ccy_pair/rate?base=iso.example.org%2Fcurrency%28code:%22EUR%22%29&date=2026-03-16&quote=iso.example.org%2Fcurrency%28code:%22USD%22%29";
    for src in [
        "rates.example.org/ccy_pair(iso.example.org/currency(EUR), iso.example.org/currency(USD)).rate(2026-03-16)",
        "rates.example.org/ccy_pair(\"iso.example.org/currency(code:\"\"EUR\"\")\", \"iso.example.org/currency(code:\"\"USD\"\")\").rate(2026-03-16)",
        "rates.example.org/ccy_pair(base: iso.example.org/currency(EUR), quote: iso.example.org/currency(USD)).rate(2026-03-16)",
        "rates.example.org/ccy_pair/rate(iso.example.org/currency(EUR), iso.example.org/currency(USD), 2026-03-16)",
    ] {
        assert_eq!(uri(src), want, "{src}");
    }
    for (src, want) in [
        ("rates.example.org/ccy_pair(\"nonsense\", iso.example.org/currency(USD)).rate(2026-03-16)", "E_KEY_ENCODING"),
        ("rates.example.org/ccy_pair/rate(\"iso.example.org/currency(code:\"\"EUR\"\")\", \"garbage\", 2026-03-16)", "E_KEY_ENCODING"),
        ("rates.example.org/ccy_pair/rate(\"iso.example.org/currency(code: \"\"EUR\"\")\", iso.example.org/currency(USD), 2026-03-16)", "E_KEY_ENCODING"),
        ("rates.example.org/ccy_pair(iso.example.org/currency(eur), iso.example.org/currency(USD)).rate(2026-03-16)", "E_KEY_NORMALISATION"),
        ("rates.example.org/ccy_pair(rates.example.org/fx_pair(EUR, USD), iso.example.org/currency(USD)).rate(2026-03-16)", "E_CANON_TYPE"),
        ("rates.example.org/ccy_pair(EUR, USD).rate(2026-03-16)", "E_CANON_TYPE"),
        ("rates.example.org/ccy_pair(iso.example.org/currency(EUR), USD).rate(2026-03-16)", "E_CANON_TYPE"),
    ] {
        assert_eq!(code(src), want, "{src}");
    }
}

// ---------------------------------------------------------------------------------------------
// Short calls, defaults and `resolved` (16.7).

fn short_env() -> MapCallEnv {
    let mut e = env();
    e.aliases.insert("fx_rate".into(), "rates.example.org/fx_rate".into());
    e.aliases.insert("EUR".into(), "rates.example.org/fx_rate".into());
    e.aliases_tx = Some("2026-05-01T00:00:00.000000Z".into());
    e
}

const AT: &str = "?asof=2026-03-16T18:00:00.000000Z";

/// A bare name in function position resolves through the alias map (`@name` as before); an alias
/// the mesh does not have is `E_NAME_UNBOUND`; a bare word in an argument position is a value,
/// never an alias; `resolved` says what was resolved, with the alias snapshot only when an alias
/// was used.
#[test]
fn bare_names_resolve_through_the_alias_map() {
    let e = short_env();
    let c = parse_call(&format!("fx_rate(EUR, GBP, 2026-03-01){AT}"), &e).unwrap();
    assert_eq!(
        c.uri(),
        "cell://rates.example.org/fx_rate?base=EUR&date=2026-03-01&quote=GBP&asof=2026-03-16T18:00:00.000000Z"
    );
    assert_eq!(resolved(&c), [("alias".into(), "fx_rate".into(), "rates.example.org/fx_rate".into())]);
    let r = c.resolved.as_ref().unwrap();
    assert_eq!(r.aliases_tx.as_deref(), Some("2026-05-01T00:00:00.000000Z"));
    assert_eq!(
        r.call,
        "rates.example.org/fx_rate(base: \"EUR\", date: 2026-03-01, quote: \"GBP\")?asof=2026-03-16T18:00:00.000000Z"
    );
    // `@name` as before, recorded as written.
    let c = parse_call("@fx_rate(EUR, GBP, 2026-03-01)", &e).unwrap();
    assert_eq!(resolved(&c), [("alias".into(), "@fx_rate".into(), "rates.example.org/fx_rate".into())]);
    // A bare name that is not an alias.
    assert_eq!(parse("nofx(EUR, GBP, 2026-03-01)", &e).unwrap_err(), "E_NAME_UNBOUND");
    // `EUR` is an alias too, and in an argument position still the string EUR.
    let c = parse_call("rates.example.org/fx_rate(EUR, GBP, 2026-03-01)", &e).unwrap();
    assert_eq!(c.uri(), "cell://rates.example.org/fx_rate?base=EUR&date=2026-03-01&quote=GBP");
    // Nothing was resolved: no `resolved` and no alias snapshot.
    assert!(c.resolved.is_none());
}

/// Every `@name` is an alias resolution (I1), with or without an alias snapshot and when pinned;
/// `aliases_tx` only when the environment has one. The pinned call is `name@<hash>(...)?asof=`.
#[test]
fn every_alias_is_recorded() {
    let v = json::parse(include_bytes!("../spec-vectors/call/pinned-alias.json")).unwrap();
    let mut e = MapCallEnv::from_value(v.get("env").unwrap()).unwrap();
    assert!(e.aliases_tx.is_none());
    let input = v.get("input").and_then(|i| i.as_str()).unwrap().to_string();
    let c = parse_call(&input, &e).unwrap();
    assert_eq!(resolved(&c), [("alias".into(), "@fx".into(), "rates.example.org/fx_rate".into())]);
    let r = c.resolved.as_ref().unwrap();
    assert_eq!(r.aliases_tx, None);
    assert_eq!(
        r.call,
        "rates.example.org/fx_rate@f041cf0f3eabaf4a611a6f57050dac8fb1f2e2cd5d6b6526913cff0847ef88df(base: \"EUR\", date: 2024-03-15, quote: \"USD\")?asof=2024-03-16T09:00:00.000000Z"
    );
    let c = parse_call("@fx(\"EUR\", \"USD\", 2024-03-15)", &e).unwrap();
    assert_eq!(
        c.resolved.as_ref().unwrap().call,
        "rates.example.org/fx_rate(base: \"EUR\", date: 2024-03-15, quote: \"USD\")"
    );
    e.aliases_tx = Some("2026-01-01T00:00:00.000000Z".into());
    let c = parse_call("@fx(\"EUR\", \"USD\", 2024-03-15)", &e).unwrap();
    assert_eq!(c.resolved.as_ref().unwrap().aliases_tx.as_deref(), Some("2026-01-01T00:00:00.000000Z"));
}

/// An unbound argument whose declaration has `"default": "asof"` binds to the call's `asof`
/// (`date_of` for a date), or to the mesh's now when the call has none; the cell URI carries the
/// value and the resolution records it. A bound argument has no resolution.
#[test]
fn defaults_take_the_calls_asof_or_now() {
    let mut e = short_env();
    e.now = Some("2026-03-17T01:02:03.000004Z".into());
    let c = parse_call(&format!("fx_rate(EUR, GBP){AT}"), &e).unwrap();
    assert_eq!(
        c.uri(),
        "cell://rates.example.org/fx_rate?base=EUR&date=2026-03-16&quote=GBP&asof=2026-03-16T18:00:00.000000Z"
    );
    assert_eq!(
        resolved(&c),
        [
            ("alias".into(), "fx_rate".into(), "rates.example.org/fx_rate".into()),
            ("default".into(), "date".into(), "2026-03-16".into())
        ]
    );
    // No asof: the mesh's now. The canonical call then has no `?asof=`.
    let c = parse_call("fx_rate(EUR, GBP)", &e).unwrap();
    assert_eq!(c.uri(), "cell://rates.example.org/fx_rate?base=EUR&date=2026-03-17&quote=GBP");
    assert_eq!(
        c.resolved.as_ref().unwrap().call,
        "rates.example.org/fx_rate(base: \"EUR\", date: 2026-03-17, quote: \"GBP\")"
    );
    assert_eq!(c.cell.asof, None);
    // Written in full, the default does not apply.
    assert!(resolved(&parse_call(&format!("fx_rate(EUR, GBP, 2026-03-01){AT}"), &e).unwrap())
        .iter()
        .all(|(k, _, _)| k != "default"));
    // A valid-time argument without a default is required.
    assert_eq!(parse("rates.example.org/fx_pair/rate(EUR, USD)", &e).unwrap_err(), "E_ARG_MISSING");
    // An instant default takes the instant itself.
    let snap = function(
        r#"{"code": {"body": "", "lang": "stored"}, "inputs": [], "signature": {"args": {"at": {"type": "instant", "valid": true, "default": "asof"},
        "k": {"type": "string", "valid": false}}, "order": ["k", "at"], "returns": {"type": "number", "unit": "1"}}}"#,
    );
    e.functions.insert("calc.example.net/snap".into(), snap);
    let c = parse_call(&format!("calc.example.net/snap(K){AT}"), &e).unwrap();
    assert_eq!(
        c.uri(),
        "cell://calc.example.net/snap?at=2026-03-16T18:00:00.000000Z&k=K&asof=2026-03-16T18:00:00.000000Z"
    );
    assert_eq!(resolved(&c), [("default".into(), "at".into(), "2026-03-16T18:00:00.000000Z".into())]);
    let c = parse_call("calc.example.net/snap(K)", &e).unwrap();
    assert!(c.uri().starts_with("cell://calc.example.net/snap?at=2026-03-17T01:02:03.000004Z&k=K"));
}

/// The alias snapshot's instant is the one the alias map gave, and an older snapshot gives the
/// resolution it gave then (`call/class-short-old-snapshot`): `aliases_tx` is the environment's.
#[test]
fn resolved_names_the_alias_snapshot() {
    let mut e = short_env();
    e.aliases.insert("fx_rate".into(), "rates.example.org/fx_pair/rate".into());
    e.aliases_tx = Some("2026-01-01T00:00:00.000000Z".into());
    let c = parse_call("fx_rate(EUR, GBP, 2026-03-16)", &e).unwrap();
    assert_eq!(c.uri(), "cell://rates.example.org/fx_pair/rate?base=EUR&date=2026-03-16&quote=GBP");
    assert_eq!(c.resolved.as_ref().unwrap().aliases_tx.as_deref(), Some("2026-01-01T00:00:00.000000Z"));
    // An environment without a snapshot instant records none.
    e.aliases_tx = None;
    let c = parse_call("fx_rate(EUR, GBP, 2026-03-16)", &e).unwrap();
    assert_eq!(c.resolved.as_ref().unwrap().aliases_tx, None);
}

// ---------------------------------------------------------------------------------------------
// Labels and the lookup hook (D93).

/// An environment recording the lookups the hook is asked.
struct Recording {
    inner: MapCallEnv,
    asked: RefCell<Vec<(String, String, String)>>,
}

impl CallEnv for Recording {
    fn alias(&self, alias: &str) -> Result<Option<String>, String> {
        self.inner.alias(alias)
    }
    fn bound(&self, name: &str, asof: Option<&str>) -> Result<Option<FunctionDescriptor>, String> {
        self.inner.bound(name, asof)
    }
    fn pinned(&self, fn_hash: &str) -> Result<Option<FunctionDescriptor>, String> {
        self.inner.pinned(fn_hash)
    }
    fn bound_class(&self, name: &str, asof: Option<&str>) -> Result<Option<ClassDescriptor>, String> {
        self.inner.bound_class(name, asof)
    }
    fn aliases_tx(&self) -> Result<Option<String>, String> {
        self.inner.aliases_tx()
    }
    fn now(&self) -> Result<Option<String>, String> {
        Ok(Some(NOW.to_string()))
    }
    fn lookup(&self, class: &str, label: &str, asof: &str) -> Result<Option<String>, String> {
        self.asked.borrow_mut().push((class.to_string(), label.to_string(), asof.to_string()));
        self.inner.lookup(class, label, asof)
    }
}

fn recording() -> Recording {
    Recording { inner: env(), asked: RefCell::new(Vec::new()) }
}

/// `N[label]` before a method: the class's lookup method read at the label as of the call's
/// `asof` (the mesh's now when the call has none); its value is the key, and the call is the
/// method call on that object. `resolved` records the label, the key encoding and that instant,
/// then the method.
#[test]
fn a_label_resolves_through_the_lookup_hook() {
    let rec = recording();
    let c = parse_call(&format!("rates.example.org/fx_pair[cable].rate(2026-03-16)?asof={ASOF}"), &rec).unwrap();
    assert_eq!(
        c.uri(),
        format!("cell://rates.example.org/fx_pair/rate?base=GBP&date=2026-03-16&quote=USD&asof={ASOF}")
    );
    assert_eq!(*rec.asked.borrow(), [("rates.example.org/fx_pair".to_string(), "cable".to_string(), ASOF.to_string())]);
    let r = c.resolved.as_ref().unwrap();
    assert_eq!(r.resolutions[0].kind, ResolutionKind::Lookup);
    assert_eq!(r.resolutions[0].asof.as_deref(), Some(ASOF));
    assert_eq!(r.resolutions[0].from, "cable");
    assert_eq!(r.resolutions[0].to, "rates.example.org/fx_pair(base:\"GBP\",quote:\"USD\")");
    assert_eq!(r.resolutions[1].kind, ResolutionKind::Method);
    assert_eq!(r.aliases_tx, None);
    // Without an asof, the mesh's now; a quoted label is the same label.
    let rec = recording();
    let c = parse_call("rates.example.org/fx_pair[\"fiber\"].cross(GBP, 2026-03-16)", &rec).unwrap();
    assert_eq!(c.uri(), "cell://rates.example.org/fx_pair/cross?base=EUR&date=2026-03-16&quote=USD&via=GBP");
    assert_eq!(rec.asked.borrow()[0].2, NOW);
    assert_eq!(c.resolved.as_ref().unwrap().resolutions[0].asof.as_deref(), Some(NOW));
}

/// A class without `lookup`, or a label the lookup has no value for as of that instant, is
/// `E_NAME_UNBOUND` (it may be bound later, as an alias may); the hook is asked only for a class
/// with a `lookup` member.
#[test]
fn an_unknown_label_is_unbound() {
    let rec = recording();
    for (src, asked) in [
        (format!("scripture.example.org/verse[kursi].text(uthmani)?asof={ASOF}"), 0),
        (format!("rates.example.org/fx_pair[sterling].rate(2026-03-16)?asof={ASOF}"), 1),
    ] {
        assert_eq!(parse(&src, &rec).unwrap_err(), "E_NAME_UNBOUND", "{src}");
        assert_eq!(rec.asked.borrow().len(), asked, "{src}");
        rec.asked.borrow_mut().clear();
    }
    // The default environment has no lookups at all.
    let mut bare = env();
    bare.lookups.clear();
    assert_eq!(
        parse(&format!("rates.example.org/fx_pair[cable].rate(2026-03-16)?asof={ASOF}"), &bare).unwrap_err(),
        "E_NAME_UNBOUND"
    );
}

/// A label stands where `N(keys)` would, as the receiver of a method call, in a request only:
/// anything else is `E_PARSE_URI`.
#[test]
fn a_label_is_a_receiver() {
    let rec = recording();
    for src in [
        "rates.example.org/fx_pair[cable]",
        "rates.example.org/fx_pair[cable].rate",
        "rates.example.org/fx_pair[cable, fiber].rate(2026-03-16)",
        "rates.example.org/fx_pair[cable](EUR).rate(2026-03-16)",
        "rates.example.org/fx_pair/rate[cable](2026-03-16)",
        "rates.example.org/fx_pair/named[cable]",
        "rates.example.org/fx_pair[].rate(2026-03-16)",
    ] {
        assert_eq!(parse(&format!("{src}?asof={ASOF}"), &rec).unwrap_err(), "E_PARSE_URI", "{src}");
    }
}

/// `MapCallEnv` reads the environment of the vectors: classes, the alias snapshot, now and lookups.
#[test]
fn the_map_environment_reads_the_new_members() {
    let v = common::vector("call/class-short.json");
    let e = MapCallEnv::from_value(v.get("env").unwrap()).unwrap();
    assert_eq!(e.aliases_tx.as_deref(), Some("2026-05-01T00:00:00.000000Z"));
    assert!(e.classes.contains_key("rates.example.org/fx_pair"));
    assert!(e.bound_class("rates.example.org/fx_pair", None).unwrap().is_some());
    assert!(e.bound_class("rates.example.org/fx_pair/rate", None).unwrap().is_none());
    let l = env();
    assert_eq!(
        l.lookup("rates.example.org/fx_pair", "cable", ASOF).unwrap().as_deref(),
        Some("rates.example.org/fx_pair(base:\"GBP\",quote:\"USD\")")
    );
    assert_eq!(l.lookup("rates.example.org/fx_pair", "sterling", ASOF).unwrap(), None);
    let with_now =
        String::from_utf8(v.get("env").unwrap().to_jcs()).unwrap().replacen("{", &format!("{{\"now\":\"{NOW}\","), 1);
    assert_eq!(MapCallEnv::from_value(&json::parse(with_now.as_bytes()).unwrap()).unwrap().now.as_deref(), Some(NOW));
}
