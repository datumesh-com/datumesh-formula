//! parse(print(ast)) = ast (15.6) on fuzzed canonical trees, without an environment.
//! `FORMULA_ROUNDTRIP_CASES` sets the count (default 5,000; the full run is 100,000).

mod common;

use common::generate::{random_ast, Rng};
use datumesh_formula::canonical_ast;

#[test]
fn print_then_parse_is_the_identity() {
    let n: u64 = std::env::var("FORMULA_ROUNDTRIP_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(5_000);
    let mut rng = Rng::new(0x5eed_0001);
    let mut nodes = 0usize;
    for i in 0..n {
        let depth = 1 + (i % 7) as u32;
        let ast = random_ast(&mut rng, depth);
        nodes += ast.node_count();
        let body = ast.body();
        let parsed = canonical_ast(&body, None).unwrap_or_else(|e| panic!("case {i}: {body:?} does not parse: {e}"));
        assert_eq!(
            String::from_utf8(parsed.to_jcs()).unwrap(),
            String::from_utf8(ast.to_jcs()).unwrap(),
            "case {i}: {body:?}"
        );
        assert_eq!(parsed.hash(), ast.hash());
        assert_eq!(parsed.body(), body);
    }
    eprintln!("round trip: {n} trees, {nodes} nodes");
}
