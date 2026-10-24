//! Lex, parse, resolve and check any input; whatever defines must print, re-parse to the same
//! AST without an environment, and hash. Nothing may panic.
#![no_main]

use std::sync::OnceLock;

use datumesh_formula::{canonical_ast, define, Env};
use libfuzzer_sys::fuzz_target;

fn env() -> &'static Env {
    static E: OnceLock<Env> = OnceLock::new();
    E.get_or_init(|| {
        let v = datumesh_core::json::parse(include_bytes!("../env.json")).expect("env.json");
        Env::from_value(&v).expect("env")
    })
}

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else { return };
    let _ = canonical_ast(src, None);
    if let Ok(c) = define(src, env()) {
        let again = canonical_ast(&c.body, None).expect("a printed body parses");
        assert_eq!(again.to_jcs(), c.ast.to_jcs(), "parse(print(ast)) = ast");
        let _ = c.ast.hash();
    }
});
