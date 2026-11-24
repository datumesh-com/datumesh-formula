//! `read_names` (datumesh-formula 0.6.2, CONTRACT-next-patch item 16): the function names a body
//! reads, as written, with no environment, after lexing and parsing it. The signature is
//! `pub fn read_names(source: &str) -> Result<BTreeSet<String>, datumesh_formula::Error>` (the
//! crate's existing error type; the contract's "FormulaError" is that type). Aliases stay in their
//! `@` form (the caller resolves them as of its asof); built-ins (`last_before`, `if`, `abs`, ...)
//! are not names; variables and key literals are not names; a name is listed once. A body that
//! does not lex or parse is the crate's error, with the code `canonical_ast` gives it.
//!
//! Method calls (the documented choice). `N(...).m(...)` is a method on a class, and resolving it
//! needs what the resolver looks up: the class `N` (the receiver read, to dispatch on its object
//! key) and the method's function `N/m` (SPEC 16.3, `<class>/<method>`). So `read_names` yields
//! BOTH `N` and `N/m`, each as written (an aliased receiver gives `@a/N` and `@a/N/m`), as the
//! crate's internal collector does. A caller that looks up functions only skips the names it does
//! not find as functions (the class name), and finds the method as `N/m`. An extension method
//! written with its qualified name (`.ext.example.org/gloss(...)`) yields that qualified name, and
//! the receiver its own reads. The method's arguments contribute their reads like any expression.
//! A method whose receiver is not itself a read (a variable, or a chain on a method's result) has
//! a class known only after type checking, so it yields no `N/m` name; the reads inside the
//! receiver are still listed.

use std::collections::BTreeSet;

use datumesh_formula::{canonical_ast, read_names};

fn names(src: &str) -> Vec<String> {
    read_names(src).unwrap_or_else(|e| panic!("{src}: {e:?}")).into_iter().collect()
}

fn set(of: &[&str]) -> Vec<String> {
    of.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>().into_iter().collect()
}

#[test]
fn the_return_type_is_a_btreeset_of_strings() {
    let r: Result<BTreeSet<String>, datumesh_formula::Error> = read_names("data.example.org/f(1)");
    assert_eq!(r.unwrap(), BTreeSet::from(["data.example.org/f".to_string()]));
}

#[test]
fn nested_body() {
    assert_eq!(
        names("data.example.org/f(data.example.org/g(x)) + data.example.org/h(1)"),
        set(&["data.example.org/f", "data.example.org/g", "data.example.org/h"])
    );
    assert_eq!(
        names("let y = data.example.org/g(x) in (if y > 0 then data.example.org/f(y) else abs(@n/m(y)))"),
        set(&["data.example.org/f", "data.example.org/g", "@n/m"])
    );
}

#[test]
fn slice_read_inside_a_builtin() {
    assert_eq!(names("last_before(@boe/bank_rate(*), 2026-11-05)"), set(&["@boe/bank_rate"]));
    assert_eq!(names("last_before(data.example.org/rate(*), 2026-11-05)"), set(&["data.example.org/rate"]));
}

#[test]
fn alias_stays_in_at_form() {
    assert_eq!(names("@fx/usd_gbp(d)"), set(&["@fx/usd_gbp"]));
    assert_eq!(names("@fx/usd_gbp(d) * data.example.org/k(d)"), set(&["@fx/usd_gbp", "data.example.org/k"]));
    // An alias and the qualified name it might stand for are different names as written.
    assert_eq!(names("@k(d) + data.example.org/k(d)"), set(&["@k", "data.example.org/k"]));
}

#[test]
fn fully_qualified_name() {
    assert_eq!(
        names("datumesh.com/relay/treasury.gov/par_yield(d)"),
        set(&["datumesh.com/relay/treasury.gov/par_yield"])
    );
}

#[test]
fn method_call_on_a_class_yields_class_and_method_function() {
    assert_eq!(
        names("scripture.example.org/verse(tanakh, Gen, 1, 1).text(uxlc)"),
        set(&["scripture.example.org/verse", "scripture.example.org/verse/text"])
    );
}

#[test]
fn method_call_with_an_alias_receiver() {
    assert_eq!(
        names("@scripture/verse(tanakh, Gen, 1, 1).text(uxlc)"),
        set(&["@scripture/verse", "@scripture/verse/text"])
    );
}

#[test]
fn method_arguments_contribute_their_reads() {
    assert_eq!(
        names("scripture.example.org/verse(tanakh, Gen, 1, 1).text(data.example.org/pick(d))"),
        set(&["scripture.example.org/verse", "scripture.example.org/verse/text", "data.example.org/pick"])
    );
}

#[test]
fn extension_method_yields_its_qualified_name() {
    assert_eq!(
        names("scripture.example.org/verse(tanakh, Gen, 1, 1).ext.example.org/gloss(d)"),
        set(&["scripture.example.org/verse", "ext.example.org/gloss"])
    );
}

#[test]
fn method_on_a_receiver_that_is_not_a_read_yields_no_method_name() {
    // A variable receiver: its class is known only after type checking.
    assert_eq!(
        names("let v = scripture.example.org/verse(tanakh, Gen, 1, 1) in v.text(uxlc)"),
        set(&["scripture.example.org/verse"])
    );
    // A chain: `.text` is on the class `next` returns; `.next` is on the receiver read.
    assert_eq!(
        names("scripture.example.org/verse(tanakh, Gen, 1, 1).next().text(uxlc)"),
        set(&["scripture.example.org/verse", "scripture.example.org/verse/next"])
    );
}

#[test]
fn builtins_are_not_names() {
    assert!(names("abs(1) + (if true then 2 else 3)").is_empty());
    assert!(names("if(true, 1, 2)").is_empty());
    assert!(names("sum(1, 2, 3)").is_empty());
    assert_eq!(names("abs(data.example.org/f(1))"), set(&["data.example.org/f"]));
}

#[test]
fn variables_and_literals_are_not_names() {
    assert!(names("let y = 2 in y + x").is_empty());
    assert!(names("1 + 2026-11-05 + \"s\" + Gen").is_empty());
}

#[test]
fn duplicates_once() {
    assert_eq!(
        names("data.example.org/f(1) + data.example.org/f(2) + @a/b(1) + @a/b(2)"),
        set(&["data.example.org/f", "@a/b"])
    );
    assert_eq!(
        names("scripture.example.org/verse(a).text(u) + scripture.example.org/verse(b).text(u)"),
        set(&["scripture.example.org/verse", "scripture.example.org/verse/text"])
    );
}

#[test]
fn parse_error_is_the_crates_error() {
    for src in ["data.example.org/f(1) +", "data.example.org/f(", "nosuch(1)", "@Bad(1)", "a.b"] {
        let e: datumesh_formula::Error = read_names(src).unwrap_err();
        let want = canonical_ast(src, None).unwrap_err();
        assert_eq!(e.code, want.code, "{src}");
    }
    let e = read_names("nosuch(1)").unwrap_err();
    assert_eq!(e.code.as_str(), "E_FORMULA_UNKNOWN_FUNCTION");
}
