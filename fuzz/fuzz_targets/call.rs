//! Parse any input as a call (SPEC 5.8) against the call vectors' environment; whatever parses
//! gives a URI the core parses back (unpinned) to the same reference. Nothing may panic.
#![no_main]

use std::sync::OnceLock;

use datumesh_formula::call::{parse_call, CallEnv, MapCallEnv};
use libfuzzer_sys::fuzz_target;

fn env() -> &'static MapCallEnv {
    static E: OnceLock<MapCallEnv> = OnceLock::new();
    E.get_or_init(|| {
        let v = datumesh_core::json::parse(include_bytes!("../../spec-vectors/call/positional.json")).expect("vector");
        MapCallEnv::from_value(v.get("env").expect("env")).expect("env")
    })
}

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else { return };
    if let Ok(c) = parse_call(src, env()) {
        let uri = c.uri();
        if c.fn_hash.is_none() {
            let name = format!("{}/{}", c.cell.ns, c.cell.fn_name);
            let d = env().bound(&name, None).expect("env").expect("bound");
            let sig = d.to_value().get("signature").expect("signature").clone();
            let back = datumesh_core::parse_uri(&uri, &sig).expect("the URI of a call parses");
            assert_eq!(back, c.cell);
        }
    }
});
