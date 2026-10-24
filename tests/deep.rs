//! Bodies at the limits of 15.3 are defined, printed, hashed, round-tripped and evaluated on a
//! test thread's default stack: a chain of 65,535 nodes, 65,000 minus signs, and nesting just
//! under 256 at every precedence level.

mod common;

use datumesh_core::{Scalar, Type};
use datumesh_formula::eval::{evaluate, Args, Fuel};
use datumesh_formula::memory::MemoryWorld;
use datumesh_formula::{canonical_ast, define, Code, Env};

fn env() -> Env {
    let mut env = common::formula_env();
    env.signature.returns = Type::Number;
    env
}

fn eval_x(src: &str, x: &str, ret_unit_one: bool) -> Result<String, Code> {
    let mut env = env();
    if ret_unit_one {
        env.signature.returns_unit =
            Some(datumesh_formula::UnitDecl::Simple(datumesh_formula::UnitSide::Literal("1".to_string())));
    }
    let c = define(src, &env).map_err(|e| e.code)?;
    // Round trip without an environment.
    let again = canonical_ast(&c.body, None).map_err(|e| e.code)?;
    assert_eq!(again.to_jcs(), c.ast.to_jcs());
    let f = datumesh_formula::Function {
        lang: "formula".to_string(),
        body: c.body.clone(),
        inputs: vec![],
        signature: env.signature.clone(),
        hash: String::new(),
        value: datumesh_core::json::Value::Null,
    };
    let mut args = Args::new();
    for (name, d) in &env.signature.args {
        let v = match d.ty {
            Type::Number => Scalar::from_lexical(Type::Number, x).unwrap(),
            Type::String => Scalar::from_lexical(Type::String, "s").unwrap(),
            Type::Boolean => Scalar::Boolean(true),
            Type::Date => Scalar::from_lexical(Type::Date, "2026-01-01").unwrap(),
            Type::Instant => Scalar::from_lexical(Type::Instant, "2026-01-01T00:00:00.000000Z").unwrap(),
            Type::Blob => unreachable!("no blob arguments"),
        };
        args.insert(name.clone(), v);
    }
    let mut world = MemoryWorld::new("2026-06-01T00:00:00.000000Z");
    let mut fuel = Fuel::new(10_000_000);
    let o = evaluate(&c, &f, &args, "2026-06-01T00:00:00.000000Z", &mut world, &mut fuel)?;
    Ok(o.value.lexical().to_string())
}

#[test]
fn chain_of_65535_nodes() {
    let src = format!("x{}", " + x".repeat(32767));
    assert_eq!(eval_x(&src, "1", true).unwrap(), "32768");
    let too_many = format!("x{}", " + x".repeat(32768));
    assert_eq!(define(&too_many, &env()).unwrap_err().code, Code::Limit);
}

#[test]
fn many_minus_signs() {
    let src = format!("{}x", "-".repeat(65_000));
    assert_eq!(eval_x(&src, "2", true).unwrap(), "2");
    let src = format!("{}x", "-".repeat(64_999));
    assert_eq!(eval_x(&src, "2", true).unwrap(), "-2");
}

#[test]
fn nesting_at_every_level() {
    // 125 parentheses, each around every precedence level, and inside the innermost a
    // parenthesised if with a not: 256 levels of nesting.
    let mut src = "x".to_string();
    for _ in 0..125 {
        src = format!("(-x ^ 2 * x / 1 + x - (if 1 < 2 and not false or true then x else x) + {src})");
    }
    let src = src.replace("-x ^ 2", "(-x) ^ 2");
    assert_eq!(eval_x(&src, "1", true), Ok("126".to_string()));
    let deeper = format!("({src})");
    assert_eq!(define(&deeper, &env()).unwrap_err().code, Code::Limit);
    // 255 nested calls.
    let calls = format!("{}x{}", "abs(".repeat(255), ")".repeat(255));
    assert_eq!(eval_x(&calls, "-3", true).unwrap(), "3");
    let calls = format!("{}x{}", "abs(".repeat(256), ")".repeat(256));
    assert_eq!(define(&calls, &env()).unwrap_err().code, Code::Limit);
}
