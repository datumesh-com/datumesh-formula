//! The README's example: a formula function defined and published to a PostgreSQL store,
//! evaluated by the engine with the formula runtime, and its receipt verified at recomputed.
//!
//!     DATABASE_URL=postgres://… cargo run --example engine

use std::collections::BTreeMap;
use std::sync::Arc;

use datumesh_core::json::{self, Value};
use datumesh_core::{
    verify, Arg, Args, Cell, CellRef, Keys, Level, Receipt, ReceiptBuilder, Resolver, Scalar, SigningKey, Type,
};
use datumesh_engine::{Answer, Engine};
use datumesh_formula::definition::{define_function, publish};
use datumesh_formula::runtime::FormulaRuntime;
use datumesh_store::{migrate, PgStore, Store};

const T0: &str = "2026-01-01T00:00:00.000000Z";

/// A key document publishing `key` for `host`, as https://<host>/.well-known/datumesh.json serves it.
fn key_doc(host: &str, key: &SigningKey) -> String {
    let public: String = key.public_key().iter().map(|b| format!("{b:02x}")).collect();
    format!(
        r#"{{"host":"{host}","keys":[{{"alg":"ed25519","id":"{}","public":"{public}","valid_from":"{T0}"}}],"proto":"datumesh/1"}}"#,
        key.key_id()
    )
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(PgStore::connect(&std::env::var("DATABASE_URL")?)?);
    migrate::apply(&store)?;
    let publisher = SigningKey::from_seed(&[1; 32]);
    let mesh = SigningKey::from_seed(&[2; 32]);
    let mut keys = Keys::new();
    for (host, key) in [("prices.example.org", &publisher), ("mesh.example.com", &mesh)] {
        if store.namespace(host)?.is_none() {
            store.put_namespace(host, key_doc(host, key).as_bytes(), T0)?;
        }
        keys.insert_response(host, 200, Some(key_doc(host, key).as_bytes()));
    }

    // A publisher's stored function: the daily price of an item, in EUR.
    let daily = r#"{"code":{"body":"","lang":"stored"},"inputs":[],"signature":{"args":{"date":{"type":"date","valid":true},
        "item":{"type":"string","valid":false}},"order":["item","date"],"returns":{"type":"number","unit":"EUR"}}}"#;
    let daily_hash = store.put_descriptor(daily.as_bytes())?;
    if store.resolve_name("prices.example.org/daily", T0)?.is_none() {
        store.bind_name("prices.example.org/daily", &daily_hash, T0)?;
    }

    // A formula function over it: the mean over the seven days to d, with VAT. Its inputs are
    // derived from the body; define checks it against the store's bindings and gives the
    // canonical descriptor, which publish stores and binds.
    let signature = json::parse(
        br#"{"args":{"d":{"type":"date","valid":true},"item":{"type":"string","valid":false}},
        "order":["item","d"],"returns":{"type":"number","unit":"EUR"}}"#,
    )?;
    let source = "ROUND(MEAN(prices.example.org/daily(item, [d - 6..d])) * 1.2, 2)";
    let def = define_function("shop.example.com/week_price", source, &signature, &BTreeMap::new(), &*store, T0)?;
    assert_eq!(
        def.descriptor.body,
        "round(mean(prices.example.org/daily(date: [d - 6..d], item: item)) * 1.2, digits: 2)\n"
    );
    assert_eq!(def.descriptor.inputs, ["prices.example.org/daily"]);
    if store.resolve_name("shop.example.com/week_price", T0)?.is_none() {
        publish(&*store, &def, T0)?;
    }

    // The publisher's prices for a week, each recorded the day after.
    let item = Scalar::from_lexical(Type::String, "bolt")?;
    for (i, price) in ["10", "11", "12", "13", "14", "15", "16"].into_iter().enumerate() {
        let date = format!("2026-03-{:02}", i + 1);
        let tx = format!("2026-03-{:02}T09:00:00.000000Z", i + 2);
        let a: Args = [
            ("date".to_string(), Arg::Value(Scalar::from_lexical(Type::Date, &date)?)),
            ("item".to_string(), Arg::Value(item.clone())),
        ]
        .into();
        let cell =
            Cell { ns: "prices.example.org".into(), fn_name: "daily".into(), fn_hash: daily_hash.clone(), args: a };
        if store.latest(&cell.id())?.is_some() {
            continue;
        }
        let valid: Args = [("date".to_string(), Arg::Value(Scalar::from_lexical(Type::Date, &date)?))].into();
        let receipt = ReceiptBuilder::new()
            .cell(cell.clone())
            .value(Scalar::from_lexical(Type::Number, price)?)
            .valid(valid)
            .asof(tx.as_str())
            .tx(tx.as_str())
            .issued(tx.as_str())
            .publisher("prices.example.org")
            .signer_host("prices.example.org")
            .licence("CC-BY-4.0")
            .summary("The daily price of a bolt.")
            .sign(&publisher)?;
        let mut members = vec![("sig".to_string(), Value::String(receipt.sig().to_string()))];
        members.extend(json::parse(receipt.payload())?.as_object().into_iter().flatten().cloned());
        let hash = store.put_receipt(&Value::Object(members).to_jcs())?;
        store.put_version(&cell.id(), &tx, &hash)?;
    }

    // The engine with the formula runtime; the computed cell as of 10 March.
    let shared: Arc<dyn Store + Send + Sync> = store.clone();
    let runtime = Arc::new(FormulaRuntime::new(shared.clone()));
    let engine = Engine::builder(shared, "mesh.example.com").signing_key(mesh).runtime(runtime).build()?;
    let week = CellRef {
        ns: "shop.example.com".into(),
        fn_name: "week_price".into(),
        args: [
            ("d".to_string(), Arg::Value(Scalar::from_lexical(Type::Date, "2026-03-07")?)),
            ("item".to_string(), Arg::Value(item)),
        ]
        .into(),
        asof: None,
    };
    let Answer::Receipt(bytes) = engine.get(&week, Some("2026-03-10T00:00:00.000000Z"))? else {
        return Err("not covered".into());
    };
    let receipt = Receipt::from_json(&bytes)?;
    assert_eq!(receipt.value().lexical(), "15.6"); // mean 13, times 1.2
                                                   // One input: the slice of the seven prices (slice-as-lineage), its manifest in the store.
    assert_eq!(receipt.inputs().len(), 1);
    assert!(store.manifest(&receipt.inputs()[0]).is_some());
    let result = verify(&bytes, &keys, Some(&engine), None);
    assert_eq!(result.level, Some(Level::Recomputed), "{:?}", result.error());
    println!("week_price = {} EUR as of {}, from {}", receipt.value().lexical(), receipt.asof(), receipt.inputs()[0]);
    println!("verified {} receipt {}", result.level.map(|l| l.as_str()).unwrap_or("?"), receipt.hash());
    Ok(())
}
