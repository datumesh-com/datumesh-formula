//! The define-time descriptor checks of 15.6: `E_FORMULA_ORDER` (no `signature.order`), then the
//! body's own codes, then `E_FORMULA_NOT_CANONICAL` (the body is not the print of its AST, LF
//! included), then `E_FORMULA_INPUTS` (the declared inputs differ from the derived set).

mod common;

use datumesh_core::json::{self, Value};
use datumesh_formula::check_descriptor;

fn descriptor(body: &str, inputs: &[&str], order: bool) -> Value {
    let env = common::vector("formula/add.json");
    let mut sig = env.get("env").unwrap().get("signature").unwrap().clone();
    if !order {
        if let Value::Object(m) = &mut sig {
            m.retain(|(k, _)| k != "order");
        }
    }
    let text = format!(
        r#"{{"code":{{"body":{},"lang":"formula"}},"inputs":[{}],"signature":{}}}"#,
        String::from_utf8(Value::String(body.to_string()).to_jcs()).unwrap(),
        inputs.iter().map(|i| format!("\"{i}\"")).collect::<Vec<_>>().join(","),
        String::from_utf8(sig.to_jcs()).unwrap()
    );
    json::parse(text.as_bytes()).unwrap()
}

fn code(body: &str, inputs: &[&str], order: bool) -> Option<String> {
    let env = common::formula_env();
    let mut env = env;
    env.add_function(
        "rates.example.org/fx_rate",
        common::vector("formula/read-positional.json").get("env").unwrap().get("functions").unwrap().get("rates.example.org/fx_rate").unwrap(),
    )
    .unwrap();
    match check_descriptor("calc.example.net/f", &descriptor(body, inputs, order), &env) {
        Ok(_) => None,
        Err(e) => Some(e.code.as_str().to_string()),
    }
}

#[test]
fn descriptor_checks_in_order() {
    assert_eq!(code("a + b\n", &[], true), None);
    assert_eq!(code("a + b", &[], true).as_deref(), Some("E_FORMULA_NOT_CANONICAL"));
    assert_eq!(code("a+b\n", &[], true).as_deref(), Some("E_FORMULA_NOT_CANONICAL"));
    assert_eq!(code("SUM(a, b)\n", &[], true).as_deref(), Some("E_FORMULA_NOT_CANONICAL"));
    assert_eq!(code("a + b\n", &["rates.example.org/fx_rate"], true).as_deref(), Some("E_FORMULA_INPUTS"));
    assert_eq!(code("a+b\n", &["rates.example.org/fx_rate"], true).as_deref(), Some("E_FORMULA_NOT_CANONICAL"));
    assert_eq!(code("a + b\n", &[], false).as_deref(), Some("E_FORMULA_ORDER"));
    assert_eq!(code("a +\n", &[], false).as_deref(), Some("E_FORMULA_ORDER"));
    assert_eq!(code("a +\n", &[], true).as_deref(), Some("E_FORMULA_SYNTAX"));
    assert_eq!(code("a + c\n", &[], true).as_deref(), Some("E_FORMULA_UNIT_MISMATCH"));
    assert_eq!(code("flag\n", &[], true).as_deref(), Some("E_FORMULA_RETURN_TYPE"));
    let read = "a * rates.example.org/fx_rate(base: base, date: d, quote: quote)\n";
    assert_eq!(code(read, &[], true).as_deref(), Some("E_FORMULA_RETURN_UNIT"));
}
