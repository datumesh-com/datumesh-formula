//! Digit-led words in requests (spec-0.6.0, C3; SPEC 5.8). A digit-led word is a run that begins
//! with a digit and continues with letters, digits or `_`, holds at least one letter, and is not
//! accepted whole by the number, date, instant or period lexers (`2Chr`, `1Cor`, `3John`,
//! `1_kings`, `1e3`). In a request it is readable only in a `string` position, where it is that
//! string; anywhere else it is `E_CANON_TYPE`. `2D` stays a period token (still the string "2D" in a
//! string position) and `2024-03-15x` stays `E_PARSE_URI`. Bodies are unchanged: in a body such a
//! token is an error as it was.

mod common;

use common::classes::{class_env, no_args, ONE};
use datumesh_core::json;
use datumesh_formula::call::{parse_call, CallEnv, CallError, MapCallEnv};
use datumesh_formula::define;

/// The class world of the `call` vectors (`scripture.example.org/verse`: corpus, book, chapter,
/// verse; `rates.example.org/deposit`: currency, tenor, date; `rates.example.org/fx_pair`).
fn env() -> MapCallEnv {
    MapCallEnv::from_value(common::vector("call/class-lookup.json").get("env").unwrap()).unwrap()
}

/// The environment of the digit-led vectors: one function, `datumesh.example.org/verse`.
fn verse_env() -> MapCallEnv {
    MapCallEnv::from_value(common::vector("call/digit-led-2chr.json").get("env").unwrap()).unwrap()
}

fn uri(src: &str, env: &dyn CallEnv) -> String {
    match parse_call(src, env) {
        Ok(c) => c.uri(),
        Err(e) => panic!("{src}: {e}"),
    }
}

fn code(src: &str, env: &dyn CallEnv) -> String {
    match parse_call(src, env) {
        Ok(c) => panic!("{src}: parsed to {}", c.uri()),
        Err(CallError::Code(c, _)) => c.to_string(),
        Err(e) => e.code().to_string(),
    }
}

const V: &str = "cell://datumesh.example.org/verse";

/// In a string position, positional: the digit-led word is the string, as if quoted.
#[test]
fn a_digit_led_word_is_a_string_by_position() {
    let env = verse_env();
    for (word, want) in [
        ("2Chr", "2Chr"),
        ("1Cor", "1Cor"),
        ("3John", "3John"),
        ("1_kings", "1_kings"),
        ("1e3", "1e3"),
        ("2D", "2D"),
        ("1E5", "1E5"),
        ("0x1F", "0x1F"),
        ("2chr_", "2chr_"),
    ] {
        let bare = uri(&format!("datumesh.example.org/verse(bible, {word}, 1, 1)"), &env);
        let quoted = uri(&format!("datumesh.example.org/verse(\"bible\", \"{word}\", 1, 1)"), &env);
        assert_eq!(bare, format!("{V}?book={want}&chapter=1&corpus=bible&verse=1"), "{word}");
        assert_eq!(bare, quoted, "{word}: quoted or not, the call is the same");
    }
}

/// Labelled, in any order, and in the first position too.
#[test]
fn a_digit_led_word_labelled() {
    let env = verse_env();
    let want = format!("{V}?book=1Cor&chapter=13&corpus=bible&verse=4");
    assert_eq!(uri("datumesh.example.org/verse(book: 1Cor, chapter: 13, corpus: bible, verse: 4)", &env), want);
    assert_eq!(uri("datumesh.example.org/verse(verse: 4, corpus: bible, chapter: 13, book: 1Cor)", &env), want);
    assert_eq!(uri("datumesh.example.org/verse(bible, book: 1Cor, chapter: 13, verse: 4)", &env), want);
    // The corpus (first position) is a string position as well.
    assert_eq!(
        uri("datumesh.example.org/verse(2Chr, John, 1, 1)", &env),
        format!("{V}?book=John&chapter=1&corpus=2Chr&verse=1")
    );
    assert_eq!(
        uri("datumesh.example.org/verse(corpus: 3John, book: 1_kings, chapter: 1, verse: 1)", &env),
        format!("{V}?book=1_kings&chapter=1&corpus=3John&verse=1")
    );
}

/// A string argument of a plain function (`deposit`'s tenor and currency).
#[test]
fn a_digit_led_word_in_a_plain_function() {
    let env = env();
    assert_eq!(
        uri("rates.example.org/deposit(EUR, 2Y6M, 2026-03-16)", &env),
        "cell://rates.example.org/deposit?currency=EUR&date=2026-03-16&tenor=2Y6M"
    );
    assert_eq!(
        uri("rates.example.org/deposit(currency: 3X, tenor: 1_week, date: 2026-03-16)", &env),
        "cell://rates.example.org/deposit?currency=3X&date=2026-03-16&tenor=1_week"
    );
}

/// A class receiver's key of string type, positional and labelled; the key is then read under its
/// normalisation (`none` for the scripture book).
#[test]
fn a_digit_led_word_in_a_class_receiver_key() {
    let env = env();
    let want = "cell://scripture.example.org/verse/text?book=2Chr&chapter=1&corpus=bible&edition=uthmani&verse=1";
    assert_eq!(uri("scripture.example.org/verse(bible, 2Chr, 1, 1).text(uthmani)", &env), want);
    assert_eq!(
        uri("scripture.example.org/verse(corpus: bible, book: 2Chr, chapter: 1, verse: 1).text(uthmani)", &env),
        want
    );
    assert_eq!(
        uri("scripture.example.org/verse(bible, \"2Chr\", 1, 1).text(uthmani)", &env),
        want,
        "quoted or not, the call is the same"
    );
    // A digit-led word is also a string in the method's own arguments.
    assert_eq!(
        uri("scripture.example.org/verse(bible, John, 1, 1).text(2nd_ed)", &env),
        "cell://scripture.example.org/verse/text?book=John&chapter=1&corpus=bible&edition=2nd_ed&verse=1"
    );
    // A key under `upper_ascii` takes a digit-led word that is canonical under it, and refuses one that is not.
    assert_eq!(
        uri("rates.example.org/fx_pair(2CHR, USD).rate(2026-03-16)", &env),
        uri("rates.example.org/fx_pair(\"2CHR\", \"USD\").rate(2026-03-16)", &env)
    );
    assert_eq!(code("rates.example.org/fx_pair(2Chr, USD).rate(2026-03-16)", &env), "E_KEY_NORMALISATION");
}

/// A digit-led word in a number position is `E_CANON_TYPE`, positional or labelled, in a plain
/// function and in a class receiver's key.
#[test]
fn a_digit_led_word_in_a_number_position() {
    let v = verse_env();
    let e = env();
    for (src, env) in [
        ("datumesh.example.org/verse(bible, John, 1Cor, 1)", &v),
        ("datumesh.example.org/verse(bible, John, chapter: 2Chr, verse: 1)", &v),
        ("datumesh.example.org/verse(bible, John, 1, 3John)", &v),
        ("datumesh.example.org/verse(bible, John, 1, 1_kings)", &v),
        ("datumesh.example.org/verse(bible, John, 1e3, 1)", &v),
        ("scripture.example.org/verse(bible, John, 1Cor, 1).text(uthmani)", &e),
        ("scripture.example.org/verse(corpus: bible, book: John, chapter: 1, verse: 2Chr).text(uthmani)", &e),
    ] {
        assert_eq!(code(src, env), "E_CANON_TYPE", "{src}");
    }
}

/// A digit-led word in a date, instant or boolean-typed position is `E_CANON_TYPE`.
#[test]
fn a_digit_led_word_in_a_date_position() {
    let e = env();
    for src in [
        "rates.example.org/deposit(EUR, 1M, 2Chr)",
        "rates.example.org/deposit(EUR, 1M, date: 1_kings)",
        "rates.example.org/deposit(EUR, 1M, 2026Q1)",
        "rates.example.org/fx_pair(EUR, USD).rate(3John)",
    ] {
        assert_eq!(code(src, &e), "E_CANON_TYPE", "{src}");
    }
}

#[test]
fn a_digit_led_word_in_a_boolean_position() {
    let sig = r#"{"code": {"body": "", "lang": "stored"}, "inputs": [], "signature": {"args": {"flag": {"type": "boolean", "valid": false},
        "name": {"type": "string", "valid": false}}, "order": ["flag", "name"], "returns": "string"}}"#;
    let text = format!(r#"{{"aliases": {{}}, "functions": {{"datumesh.example.org/flagged": {sig}}}}}"#);
    let env = MapCallEnv::from_value(&json::parse(text.as_bytes()).unwrap()).unwrap();
    assert_eq!(
        uri("datumesh.example.org/flagged(true, 2Chr)", &env),
        "cell://datumesh.example.org/flagged?flag=true&name=2Chr"
    );
    for src in ["datumesh.example.org/flagged(2Chr, x)", "datumesh.example.org/flagged(flag: 1_true, name: x)"] {
        assert_eq!(code(src, &env), "E_CANON_TYPE", "{src}");
    }
}

/// A digit-led word does not hide a malformed neighbour: a run that is not one (a hyphen, a dot, a
/// date with a suffix) is as it was.
#[test]
fn what_is_not_a_digit_led_word_is_as_before() {
    let v = verse_env();
    assert_eq!(code("datumesh.example.org/verse(bible, 2024-03-15x, 1, 1)", &v), "E_PARSE_URI");
    assert_eq!(code("datumesh.example.org/verse(bible, 2-Chr, 1, 1)", &v), "E_PARSE_URI");
    assert_eq!(code("datumesh.example.org/verse(bible, 2.Chr, 1, 1)", &v), "E_PARSE_URI");
    assert_eq!(code("datumesh.example.org/verse(bible, 2 Chr, 1, 1)", &v), "E_PARSE_URI");
    // A number the lexer accepts is a number in a number position and the string of its text in a string position.
    assert_eq!(
        uri("datumesh.example.org/verse(bible, 2, 1, 1)", &v),
        format!("{V}?book=2&chapter=1&corpus=bible&verse=1")
    );
    // `01` is a malformed number (L5), not a digit-led word: it has no letter.
    assert_eq!(code("datumesh.example.org/verse(bible, John, 01, 1)", &v), "E_CANON_NUMBER");
    // An instant or date the lexer accepts whole is not a word.
    assert_eq!(
        uri("datumesh.example.org/verse(bible, 2026-03-16, 1, 1)", &v),
        format!("{V}?book=2026-03-16&chapter=1&corpus=bible&verse=1")
    );
}

/// The unpinned URI of a call with a digit-led word is a cell reference the core parses back.
#[test]
fn a_digit_led_call_is_a_reference() {
    let env = verse_env();
    let c = parse_call("datumesh.example.org/verse(bible, 2Chr, 1, 1)", &env).unwrap();
    let d = env.bound("datumesh.example.org/verse", None).unwrap().unwrap();
    let sig = d.to_value().get("signature").unwrap().clone();
    assert_eq!(datumesh_core::parse_uri(&c.uri(), &sig).unwrap(), c.cell);
}

// ---------------------------------------------------------------------------------------------
// Bodies are unchanged.

/// In a body a digit-led token is an error, as today (pinned from the code as of 0.3.0, the
/// reference lexer's reading): `2Chr` is the number `2` with the unit `Chr`, which is not a listed
/// unit (`E_FORMULA_UNIT`); `1e3` is not a canonical number (`E_FORMULA_NUMBER`); `1_kings` stops at
/// the `_` (`E_FORMULA_SYNTAX`). None of them becomes a string or a name in a body.
#[test]
fn a_body_still_refuses_a_digit_led_word() {
    let env = class_env("calc.example.net/e", &no_args(ONE), None);
    for (src, want) in [
        ("2Chr", "E_FORMULA_UNIT"),
        ("1Cor", "E_FORMULA_UNIT"),
        ("3John", "E_FORMULA_UNIT"),
        ("1_kings", "E_FORMULA_SYNTAX"),
        ("1 + 2Chr", "E_FORMULA_UNIT"),
        ("2Chr + 1", "E_FORMULA_UNIT"),
        ("1 + 3John * 2", "E_FORMULA_UNIT"),
        ("1e3", "E_FORMULA_NUMBER"),
    ] {
        match define(src, &env) {
            Ok(c) => panic!("{src:?}: accepted as {:?}", c.body),
            Err(e) => assert_eq!(e.code.as_str(), want, "{src:?}"),
        }
    }
}
