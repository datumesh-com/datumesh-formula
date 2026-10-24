//! The README's example: define a weekly mean over a stored daily price and evaluate it.

use datumesh_core::json;
use datumesh_core::{Scalar, Type};
use datumesh_formula::eval::{Args, CellKey, Fuel};
use datumesh_formula::memory::MemoryWorld;
use datumesh_formula::{define, Env, Signature};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A stored function: a daily price in EUR.
    let daily = json::parse(
        br#"{"code": {"body": "", "lang": "stored"}, "inputs": [],
             "signature": {"args": {"date": {"type": "date", "valid": true}}, "order": ["date"],
                           "returns": {"type": "number", "unit": "EUR"}}}"#,
    )?;
    // The function being defined: the mean over the seven days to d.
    let signature = json::parse(
        br#"{"args": {"d": {"type": "date", "valid": true}}, "order": ["d"],
             "returns": {"type": "number", "unit": "EUR"}}"#,
    )?;
    let mut env = Env::new("calc.example.net/week_mean", Signature::from_value(&signature)?);
    env.add_function("data.example.org/daily", &daily)?;
    let c = define("MEAN(data.example.org/daily([d - 6..d]))", &env)?;
    assert_eq!(c.body, "mean(data.example.org/daily(date: [d - 6..d]))\n");
    assert_eq!(c.inputs, ["data.example.org/daily"]);
    println!("body {:?}, AST hash {}", c.body, c.ast.hash());

    // A world: the two functions and a week of stored receipts.
    let mut world = MemoryWorld::new("2026-03-08T00:00:00.000000Z");
    world.bind("data.example.org/daily", &daily, "2026-01-01T00:00:00.000000Z")?;
    let fn_hash = datumesh_core::sha256_hex(&daily.to_jcs());
    for day in 1..=7 {
        let receipt = format!(
            r#"{{"asof": "2026-03-0{day}T18:00:00.000000Z", "tx": "2026-03-0{day}T18:00:00.000000Z", "type": "number",
                "value": "{}", "cell": {{"ns": "data.example.org", "fn": "daily", "fn_hash": "{fn_hash}",
                "args": {{"date": {{"type": "date", "value": "2026-03-0{day}"}}}}}}}}"#,
            10 + day
        );
        world.add_receipt(&json::parse(receipt.as_bytes())?)?;
    }
    let descriptor = format!(
        r#"{{"code": {{"body": {}, "lang": "formula"}}, "inputs": ["data.example.org/daily"], "signature": {}}}"#,
        String::from_utf8(json::Value::String(c.body.clone()).to_jcs())?,
        String::from_utf8(signature.to_jcs())?
    );
    world.bind("calc.example.net/week_mean", &json::parse(descriptor.as_bytes())?, "2026-03-01T00:00:00.000000Z")?;

    let mut args = Args::new();
    args.insert("d".to_string(), Scalar::from_lexical(Type::Date, "2026-03-07").map_err(|e| format!("{e:?}"))?);
    let mut fuel = Fuel::new(10_000);
    let cell = CellKey { name: "calc.example.net/week_mean".to_string(), args };
    let r = world.compute(&cell, "2026-03-08T00:00:00.000000Z", &mut fuel).map_err(|c| c.to_string())?;
    assert_eq!(r.outcome.value.lexical(), "14");
    assert_eq!(r.outcome.lineage.len(), 1); // one slice entry for seven member receipts
    println!("value {}, inputs {:?}, fuel {}", r.outcome.value.lexical(), r.outcome.lineage, fuel.used);
    Ok(())
}
