//! Define any input over the world of the formula-eval vectors and evaluate it with a small
//! budget. Nothing may panic.
#![no_main]

use std::sync::OnceLock;

use datumesh_core::json::Value;
use datumesh_core::{Scalar, Type};
use datumesh_formula::eval::{Args, CellKey, Fuel};
use datumesh_formula::memory::MemoryWorld;
use datumesh_formula::{define, Env, Signature};
use libfuzzer_sys::fuzz_target;

const SIG: &str = r#"{"args":{"a":{"type":"number","unit":"EUR","valid":false},"x":{"type":"number","unit":"1","valid":false},
"d":{"type":"date","valid":true},"t":{"type":"instant","valid":true},"s":{"type":"string","valid":false},
"flag":{"type":"boolean","valid":false},"t1":{"type":"number","unit":"degC","valid":false}},
"order":["a","d","flag","s","t","t1","x"],"returns":{"type":"number","unit":"EUR"}}"#;

struct Fixture {
    world: MemoryWorld,
    env: Env,
}

fn fixture() -> &'static Fixture {
    static F: OnceLock<Fixture> = OnceLock::new();
    F.get_or_init(|| {
        let v = datumesh_core::json::parse(include_bytes!("../world.json")).expect("world.json");
        let world = MemoryWorld::from_value(&v, "2026-06-01T00:00:00.000000Z").expect("world");
        let sig = Signature::from_value(&datumesh_core::json::parse(SIG.as_bytes()).expect("sig")).expect("sig");
        let mut env = Env::new("calc.example.net/e", sig);
        for (n, f) in v.get("functions").and_then(Value::as_object).expect("functions") {
            env.add_function(n, f.get("descriptor").expect("descriptor")).expect("descriptor");
        }
        Fixture { world, env }
    })
}

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else { return };
    let fx = fixture();
    // Any return type: try the body's own.
    let Ok(probe) = define(src, &fx.env) else {
        for ret in [r#""boolean""#, r#""string""#, r#""date""#, r#"{"type":"number","unit":"1"}"#] {
            let mut env = fx.env.clone();
            let text = SIG.replace(r#"{"type":"number","unit":"EUR"}}"#, &format!("{ret}}}"));
            env.signature = Signature::from_value(&datumesh_core::json::parse(text.as_bytes()).expect("sig")).expect("sig");
            if let Ok(c) = define(src, &env) {
                run(&c.body, &c.inputs, &text);
                return;
            }
        }
        return;
    };
    run(&probe.body, &probe.inputs, SIG);
});

fn run(body: &str, inputs: &[String], sig: &str) {
    let fx = fixture();
    let desc = format!(
        r#"{{"code":{{"body":{},"lang":"formula"}},"inputs":[{}],"signature":{}}}"#,
        String::from_utf8(Value::String(body.to_string()).to_jcs()).expect("utf-8"),
        inputs.iter().map(|i| format!("\"{i}\"")).collect::<Vec<_>>().join(","),
        sig
    );
    let desc = datumesh_core::json::parse(desc.as_bytes()).expect("descriptor");
    let mut world = fx.world.clone();
    world.bind("calc.example.net/e", &desc, "2026-01-01T00:00:00.000000Z").expect("bind");
    let mut args = Args::new();
    for (k, t, v) in [
        ("a", Type::Number, "12.5"),
        ("x", Type::Number, "3"),
        ("d", Type::Date, "2026-03-03"),
        ("t", Type::Instant, "2026-03-01T10:05:00.000000Z"),
        ("s", Type::String, "kew"),
        ("t1", Type::Number, "20"),
    ] {
        args.insert(k.to_string(), Scalar::from_lexical(t, v).expect("scalar"));
    }
    args.insert("flag".to_string(), Scalar::Boolean(true));
    world.fuel = Fuel::new(20_000);
    let _ = world.compute(&CellKey { name: "calc.example.net/e".to_string(), args }, "2026-06-01T00:00:00.000000Z");
}
