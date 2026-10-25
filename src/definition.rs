//! Defining a formula function: from a body, the caller's signature (with `order`), the
//! resolver's alias map and the callees' descriptors as bound at define time, the canonical
//! formula descriptor (15.6) with its function hash; and, with feature `engine`, publishing
//! it to a store.
//!
//! ```
//! use datumesh_core::json;
//! use datumesh_formula::definition::define_function;
//! use datumesh_formula::{Env, Signature};
//!
//! let sig = json::parse(br#"{"args": {"a": {"type": "number", "unit": "EUR", "valid": false}},
//!     "order": ["a"], "returns": {"type": "number", "unit": "EUR"}}"#).unwrap();
//! // An environment is a callee source: here, no callees.
//! let callees = Env::new("calc.example.net/none", Signature::from_value(&sig).unwrap());
//! let d = define_function("calc.example.net/vat", "ROUND(a * 1.2, 2)", &sig, &Default::default(), &callees,
//!     "2026-01-01T00:00:00.000000Z").unwrap();
//! assert_eq!(d.descriptor.body, "round(a * 1.2, digits: 2)\n");
//! assert_eq!(d.fn_hash, datumesh_core::sha256_hex(&d.bytes));
//! ```

use std::collections::BTreeMap;

use datumesh_core::json::{self, Value};
use datumesh_core::FunctionDescriptor;

use crate::env::{Env, Signature};
use crate::{check_descriptor, define, Code, Compiled, DefineError, Error};

/// The latest instant: callees resolved "now and from now on".
pub const LATEST: &str = "9999-12-31T23:59:59.999999Z";

/// A callee's binding: the function hash a name means, when that began, and the descriptor.
#[derive(Clone, Debug, PartialEq)]
pub struct Callee {
    /// The function hash.
    pub fn_hash: String,
    /// When the binding began (an instant).
    pub start: String,
    /// The descriptor, as stored.
    pub descriptor: Value,
}

/// Where callees' descriptors come from: a name's binding as of a transaction time. A store
/// is one (feature `engine`); so is an [`Env`] (its functions, bound since the earliest
/// instant).
pub trait Callees {
    /// The binding of the fully qualified `name` as of `at`, with its descriptor; `None` when
    /// the name has no binding then. An error is the source's own failure.
    fn callee(&self, name: &str, at: &str) -> Result<Option<Callee>, String>;
}

impl Callees for Env {
    fn callee(&self, name: &str, _at: &str) -> Result<Option<Callee>, String> {
        Ok(self.functions.get(name).map(|f| Callee {
            fn_hash: f.hash.clone(),
            start: crate::eval::ZERO_INSTANT.to_string(),
            descriptor: f.value.clone(),
        }))
    }
}

#[cfg(feature = "engine")]
fn store_callee<S: datumesh_store::Store + ?Sized>(store: &S, name: &str, at: &str) -> Result<Option<Callee>, String> {
    let Some(b) = store.resolve_binding(name, at).map_err(|e| e.to_string())? else { return Ok(None) };
    let bytes = store.descriptor(&b.fn_hash).ok_or_else(|| format!("descriptor {} is not stored", b.fn_hash))?;
    let descriptor = json::parse(&bytes).map_err(|e| e.to_string())?;
    Ok(Some(Callee { fn_hash: b.fn_hash, start: b.tx, descriptor }))
}

#[cfg(feature = "engine")]
impl Callees for datumesh_store::PgStore {
    fn callee(&self, name: &str, at: &str) -> Result<Option<Callee>, String> {
        store_callee(self, name, at)
    }
}

#[cfg(feature = "engine")]
impl Callees for dyn datumesh_store::Store + Send + Sync {
    fn callee(&self, name: &str, at: &str) -> Result<Option<Callee>, String> {
        store_callee(self, name, at)
    }
}

#[cfg(feature = "engine")]
impl Callees for dyn datumesh_store::Store {
    fn callee(&self, name: &str, at: &str) -> Result<Option<Callee>, String> {
        store_callee(self, name, at)
    }
}

/// A defined formula function: its canonical descriptor and what define derived.
#[derive(Clone, Debug)]
pub struct Definition {
    /// The fully qualified name it was defined as.
    pub name: String,
    /// The descriptor: `code.lang` `formula`, `code.body` the printed AST and one LF, `inputs`
    /// derived, the signature as given (with `order` and extents).
    pub descriptor: FunctionDescriptor,
    /// Its canonical bytes (JCS), what `publish` stores (feature `engine`).
    pub bytes: Vec<u8>,
    /// The function hash: SHA-256 of the bytes.
    pub fn_hash: String,
    /// The compiled body (canonical AST, return type and unit, asof-dependence).
    pub compiled: Compiled,
    /// The callees it was checked against: name to function hash.
    pub callees: BTreeMap<String, String>,
}

fn descriptor_value(body: &str, inputs: &[String], signature: &Value) -> Value {
    Value::Object(vec![
        (
            "code".to_string(),
            Value::Object(vec![
                ("body".to_string(), Value::String(body.to_string())),
                ("lang".to_string(), Value::String("formula".to_string())),
            ]),
        ),
        ("inputs".to_string(), Value::Array(inputs.iter().map(|i| Value::String(i.clone())).collect())),
        ("signature".to_string(), signature.clone()),
    ])
}

/// Define `source` as the formula function `name` with `signature` (a JSON signature object
/// with `order`): resolve its aliases with `aliases` (alias without `@` to fully qualified
/// name), its callees as bound in `callees` as of the transaction time `at` (the function
/// itself is in scope without a binding), and check it (15.2-15.10). The checks run in the
/// order of 15.6: the descriptor's canonical form of section 7 (the signature's), then
/// `E_FORMULA_ORDER`, then the body's codes; the descriptor produced then passes
/// [`check_descriptor`].
pub fn define_function(
    name: &str,
    source: &str,
    signature: &Value,
    aliases: &BTreeMap<String, String>,
    callees: &dyn Callees,
    at: &str,
) -> Result<Definition, DefineError> {
    // Section 7 first: the signature's shape and canonical form, in a descriptor.
    FunctionDescriptor::from_value(&descriptor_value("", &[], signature)).map_err(DefineError::Descriptor)?;
    let sig = Signature::from_value(signature).map_err(|e| Error::new(Code::Syntax, e.0))?;
    if sig.order.is_none() {
        return Err(Error::new(Code::Order, "a formula signature declares order").into());
    }
    let mut env = Env::new(name, sig);
    env.aliases = aliases.clone();
    let mut used = BTreeMap::new();
    for callee in crate::read_names(source, aliases)? {
        if callee == name {
            continue;
        }
        if let Some(c) = callees.callee(&callee, at).map_err(DefineError::Callees)? {
            env.add_function(&callee, &c.descriptor).map_err(|e| DefineError::Callees(e.0))?;
            used.insert(callee, c.fn_hash);
        }
    }
    let compiled = define(source, &env)?;
    let value = descriptor_value(&compiled.body, &compiled.inputs, signature);
    let descriptor = FunctionDescriptor::from_value(&value).map_err(DefineError::Descriptor)?;
    let bytes = descriptor.canonical();
    let fn_hash = descriptor.hash();
    check_descriptor(name, &json::parse(&bytes).map_err(DefineError::Descriptor)?, &env)?;
    used.retain(|n, _| compiled.inputs.contains(n));
    Ok(Definition { name: name.to_string(), descriptor, bytes, fn_hash, compiled, callees: used })
}

/// Publish a definition: store its descriptor (`put_descriptor`) and bind its name to it from
/// the transaction time `tx` (`bind_name`, which must be later than the name's latest
/// binding). Returns the function hash.
#[cfg(feature = "engine")]
pub fn publish<S: datumesh_store::Store + ?Sized>(
    store: &S,
    definition: &Definition,
    tx: &str,
) -> Result<String, datumesh_store::Error> {
    let h = store.put_descriptor(&definition.bytes)?;
    store.bind_name(&definition.name, &h, tx)?;
    Ok(h)
}
