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

use std::collections::{BTreeMap, BTreeSet};

use datumesh_core::json::{self, Value};
use datumesh_core::{ClassDescriptor, ClassEnv, FunctionDescriptor};

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

/// A class's binding: the class hash a class name means, when that began, and the flattened
/// descriptor. Since spec-0.5.0.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassCallee {
    /// The class hash (16.1).
    pub class_hash: String,
    /// When the binding began (an instant).
    pub start: String,
    /// The flattened class descriptor, as stored.
    pub descriptor: ClassDescriptor,
}

/// Where callees' descriptors come from: a name's binding as of a transaction time. A store
/// is one (feature `engine`); so is an [`Env`] (its functions, bound since the earliest
/// instant).
///
/// Since spec-0.5.0 it also answers for classes (16.3): the methods of a class `N` are the
/// functions `N/m`, bound by [`Callees::callee`] as any function is, and the class itself comes
/// from [`Callees::bound_class`]. All three class members are provided, answering "none".
pub trait Callees {
    /// The binding of the fully qualified `name` as of `at`, with its descriptor; `None` when
    /// the name has no binding then. An error is the source's own failure.
    fn callee(&self, name: &str, at: &str) -> Result<Option<Callee>, String>;

    /// The binding of the class name `name` as of `at`, with its flattened descriptor; `None`
    /// when `name` is not bound to a class then. Since spec-0.5.0.
    fn bound_class(&self, name: &str, at: &str) -> Result<Option<ClassCallee>, String> {
        let _ = (name, at);
        Ok(None)
    }

    /// The flattened class descriptor with this class hash, which is how a parent or an interface
    /// is found (16.2 steps 3 and 8) and what `super` reads (16.3). Since spec-0.5.0.
    fn class_descriptor(&self, class_hash: &str) -> Result<Option<ClassDescriptor>, String> {
        let _ = class_hash;
        Ok(None)
    }

    /// The name a class hash is bound to, where the source knows it: a parent's name is what a
    /// `super` read is written with (16.3). Since spec-0.5.0. Provided: none.
    fn class_name(&self, class_hash: &str) -> Result<Option<String>, String> {
        let _ = class_hash;
        Ok(None)
    }

    /// The function descriptor with this function hash, which is how a class's methods are found
    /// at define (16.2 step 6). Since spec-0.5.0.
    fn function_descriptor(&self, fn_hash: &str) -> Result<Option<Value>, String> {
        let _ = fn_hash;
        Ok(None)
    }
}

impl Callees for Env {
    fn callee(&self, name: &str, _at: &str) -> Result<Option<Callee>, String> {
        Ok(self.functions.get(name).map(|f| Callee {
            fn_hash: f.hash.clone(),
            start: crate::eval::ZERO_INSTANT.to_string(),
            descriptor: f.value.clone(),
        }))
    }

    fn bound_class(&self, name: &str, _at: &str) -> Result<Option<ClassCallee>, String> {
        Ok(self.classes.get(name).map(|d| ClassCallee {
            class_hash: d.hash(),
            start: crate::eval::ZERO_INSTANT.to_string(),
            descriptor: d.clone(),
        }))
    }

    fn class_descriptor(&self, class_hash: &str) -> Result<Option<ClassDescriptor>, String> {
        Ok(self.classes.by_hash(class_hash).map(|(_, d)| d.clone()))
    }

    fn class_name(&self, class_hash: &str) -> Result<Option<String>, String> {
        Ok(self.classes.by_hash(class_hash).map(|(n, _)| n.to_string()))
    }

    fn function_descriptor(&self, fn_hash: &str) -> Result<Option<Value>, String> {
        Ok(self.functions.values().find(|f| f.hash == fn_hash).map(|f| f.value.clone()))
    }
}

#[cfg(feature = "engine")]
fn store_callee<S: datumesh_store::Store + ?Sized>(store: &S, name: &str, at: &str) -> Result<Option<Callee>, String> {
    let Some(b) = store.resolve_binding(name, at).map_err(|e| e.to_string())? else { return Ok(None) };
    let bytes = store.descriptor(&b.fn_hash).ok_or_else(|| format!("descriptor {} is not stored", b.fn_hash))?;
    let descriptor = json::parse(&bytes).map_err(|e| e.to_string())?;
    Ok(Some(Callee { fn_hash: b.fn_hash, start: b.tx, descriptor }))
}

/// The binding of a class name in a store as of `at`, with its stored flattened descriptor (9.7).
#[cfg(feature = "engine")]
fn store_bound_class<S: datumesh_store::Store + ?Sized>(
    store: &S,
    name: &str,
    at: &str,
) -> Result<Option<ClassCallee>, String> {
    let Some(b) = store.resolve_class_binding(name, at).map_err(|e| e.to_string())? else { return Ok(None) };
    let descriptor = store_class_descriptor(store, &b.class_hash)?
        .ok_or_else(|| format!("class {} is bound but not stored", b.class_hash))?;
    Ok(Some(ClassCallee { class_hash: b.class_hash, start: b.tx, descriptor }))
}

#[cfg(feature = "engine")]
fn store_class_descriptor<S: datumesh_store::Store + ?Sized>(
    store: &S,
    class_hash: &str,
) -> Result<Option<ClassDescriptor>, String> {
    match store.class(class_hash).map_err(|e| e.to_string())? {
        Some(bytes) => ClassDescriptor::from_json(&bytes).map(Some).map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

#[cfg(feature = "engine")]
fn store_function_descriptor<S: datumesh_store::Store + ?Sized>(
    store: &S,
    fn_hash: &str,
) -> Result<Option<Value>, String> {
    match store.descriptor(fn_hash) {
        Some(bytes) => json::parse(&bytes).map(Some).map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

/// The class and function answers of a store, the same for each of its handles.
#[cfg(feature = "engine")]
macro_rules! store_callees {
    ($($ty:ty),*) => {$(
        impl Callees for $ty {
            fn callee(&self, name: &str, at: &str) -> Result<Option<Callee>, String> {
                store_callee(self, name, at)
            }
            fn bound_class(&self, name: &str, at: &str) -> Result<Option<ClassCallee>, String> {
                store_bound_class(self, name, at)
            }
            fn class_descriptor(&self, class_hash: &str) -> Result<Option<ClassDescriptor>, String> {
                store_class_descriptor(self, class_hash)
            }
            fn function_descriptor(&self, fn_hash: &str) -> Result<Option<Value>, String> {
                store_function_descriptor(self, fn_hash)
            }
        }
    )*};
}

#[cfg(feature = "engine")]
store_callees!(datumesh_store::PgStore, dyn datumesh_store::Store + Send + Sync, dyn datumesh_store::Store);

/// Load the class `class` into the environment with the functions of its methods, by name (16.3).
fn load_class(env: &mut Env, class: &str, callees: &dyn Callees, at: &str) -> Result<(), DefineError> {
    let Some(c) = callees.bound_class(class, at).map_err(DefineError::Callees)? else { return Ok(()) };
    for m in c.descriptor.methods.keys() {
        let fq = format!("{class}/{m}");
        if let Some(f) = callees.callee(&fq, at).map_err(DefineError::Callees)? {
            env.add_function(&fq, &f.descriptor).map_err(|e| DefineError::Callees(e.0))?;
        }
    }
    env.classes.insert(class, c.descriptor);
    Ok(())
}

/// The parent of a method's class, for `super` (16.3): the parent class, by its name (from the
/// callees), with its methods' functions by their hashes, each under a name made of its name and
/// hash, so that a pinned read finds it by hash (the binding of `P/m` may be another function).
fn load_parent_methods(env: &mut Env, callees: &dyn Callees) -> Result<(), DefineError> {
    let Some(class) = env.method_of.clone() else { return Ok(()) };
    let Some(parent) = env.classes.get(&class).and_then(|d| d.parent.clone()) else { return Ok(()) };
    if env.classes.by_hash(&parent).is_none() {
        if let (Some(pname), Some(pdesc)) = (
            callees.class_name(&parent).map_err(DefineError::Callees)?,
            callees.class_descriptor(&parent).map_err(DefineError::Callees)?,
        ) {
            env.classes.insert(&pname, pdesc);
        }
    }
    let Some((pname, pdesc)) = env.classes.by_hash(&parent).map(|(n, d)| (n.to_string(), d.clone())) else {
        return Ok(());
    };
    for (m, h) in &pdesc.methods {
        let found = env.functions.values().any(|f| f.hash == *h);
        if found {
            continue;
        }
        if let Some(v) = callees.function_descriptor(h).map_err(DefineError::Callees)? {
            env.add_function(&format!("{pname}/{m}@{h}"), &v).map_err(|e| DefineError::Callees(e.0))?;
        }
    }
    Ok(())
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
    define_in(name, source, signature, aliases, callees, at, None)
}

/// Define `source` as the formula function `name` of the class `class`'s method (16.3): as
/// [`define_function`], with `this` the receiver of the class's methods (the class's keys are the
/// method's own key arguments) and `super` as the environment's `method_of` gives it. `callees`
/// answers for the class and for the methods' functions by name. Since spec-0.5.0.
pub fn define_method(
    class: &str,
    name: &str,
    source: &str,
    signature: &Value,
    aliases: &BTreeMap<String, String>,
    callees: &dyn Callees,
    at: &str,
) -> Result<Definition, DefineError> {
    define_in(name, source, signature, aliases, callees, at, Some(class))
}

fn define_in(
    name: &str,
    source: &str,
    signature: &Value,
    aliases: &BTreeMap<String, String>,
    callees: &dyn Callees,
    at: &str,
    method_of: Option<&str>,
) -> Result<Definition, DefineError> {
    // Section 7 first: the signature's shape and canonical form, in a descriptor.
    FunctionDescriptor::from_value(&descriptor_value("", &[], signature)).map_err(DefineError::Descriptor)?;
    let sig = Signature::from_value(signature).map_err(|e| Error::new(Code::Syntax, e.0))?;
    if sig.order.is_none() {
        return Err(Error::new(Code::Order, "a formula signature declares order").into());
    }
    // The classes the signature names (a dispatch's set, a returned key's classes; 7, 16.6).
    let mut sig_classes: BTreeSet<String> =
        sig.args.values().filter_map(|d| d.extras.classes.clone()).flatten().collect();
    sig_classes.extend(sig.returns_classes.iter().flatten().cloned());
    let mut env = Env::new(name, sig);
    env.aliases = aliases.clone();
    env.method_of = method_of.map(str::to_string);
    let names = crate::read_names(source, aliases, method_of)?;
    if let Some(class) = method_of {
        if let Some(c) = callees.bound_class(class, at).map_err(DefineError::Callees)? {
            env.classes.insert(class, c.descriptor);
        }
    }
    let mut used = BTreeMap::new();
    for class in &sig_classes {
        load_class(&mut env, class, callees, at)?;
    }
    for callee in names {
        if callee == name {
            continue;
        }
        if let Some(c) = callees.bound_class(&callee, at).map_err(DefineError::Callees)? {
            env.classes.insert(&callee, c.descriptor);
        }
        if let Some(c) = callees.callee(&callee, at).map_err(DefineError::Callees)? {
            env.add_function(&callee, &c.descriptor).map_err(|e| DefineError::Callees(e.0))?;
            used.insert(callee, c.fn_hash);
        }
    }
    load_parent_methods(&mut env, callees)?;
    let compiled = define(source, &env)?;
    let value = descriptor_value(&compiled.body, &compiled.inputs, signature);
    let descriptor = FunctionDescriptor::from_value(&value).map_err(DefineError::Descriptor)?;
    let bytes = descriptor.canonical();
    let fn_hash = descriptor.hash();
    check_descriptor(name, &json::parse(&bytes).map_err(DefineError::Descriptor)?, &env)?;
    used.retain(|n, _| compiled.inputs.contains(n));
    Ok(Definition { name: name.to_string(), descriptor, bytes, fn_hash, compiled, callees: used })
}

/// A defined class: its flattened descriptor (16.2) and class hash. Since spec-0.5.0.
#[derive(Clone, Debug)]
pub struct ClassDefinition {
    /// The class name it was defined under.
    pub name: String,
    /// The flattened descriptor, which is the class descriptor.
    pub descriptor: ClassDescriptor,
    /// Its canonical bytes (what `publish_class` stores).
    pub bytes: Vec<u8>,
    /// The class hash: SHA-256 of the bytes.
    pub class_hash: String,
}

/// Define a class: check the descriptor `source` as its author wrote it, flatten it onto its
/// parent and check the result, with `datumesh-core`'s `define_class` (16.2) over a `ClassEnv`
/// that `callees` fills: the parent and the interfaces by class hash
/// ([`Callees::class_descriptor`]), with their methods, and the class's own methods' and extent
/// functions by function hash ([`Callees::function_descriptor`]). `definer` is the namespace
/// defining (`E_CLASS_OWNER`). The core's codes come back as [`DefineError::Descriptor`]. Since
/// spec-0.5.0.
pub fn define_class(
    name: &str,
    source: &[u8],
    definer: &str,
    callees: &dyn Callees,
    at: &str,
) -> Result<ClassDefinition, DefineError> {
    let _ = at;
    let mut env = ClassEnv::new(definer);
    // What the author names: the parent and the interfaces (classes), and the methods and extent
    // functions (functions). A text that is not JSON is the core's to refuse.
    let mut classes: Vec<String> = Vec::new();
    let mut functions: Vec<String> = Vec::new();
    if let Ok(v) = json::parse(source) {
        if let Some(p) = v.get("parent").and_then(Value::as_str) {
            classes.push(p.to_string());
        }
        if let Some(Value::Array(is)) = v.get("implements") {
            classes.extend(is.iter().filter_map(|i| i.as_str().map(str::to_string)));
        }
        for m in v.get("methods").and_then(Value::as_object).into_iter().flatten() {
            functions.extend(m.1.as_str().map(str::to_string));
        }
        for k in ["exists", "keys"] {
            functions.extend(v.get("extent").and_then(|x| x.get(k)).and_then(Value::as_str).map(str::to_string));
        }
    }
    for h in classes {
        if let Some(d) = callees.class_descriptor(&h).map_err(DefineError::Callees)? {
            functions.extend(d.methods.values().cloned());
            env.insert_class(d);
        }
    }
    for h in functions {
        if let Some(v) = callees.function_descriptor(&h).map_err(DefineError::Callees)? {
            let f = FunctionDescriptor::from_value(&v).map_err(DefineError::Descriptor)?;
            env.insert_function(f);
        }
    }
    let descriptor = datumesh_core::define_class(name, source, &env).map_err(DefineError::Descriptor)?;
    let bytes = descriptor.canonical();
    let class_hash = descriptor.hash();
    Ok(ClassDefinition { name: name.to_string(), descriptor, bytes, class_hash })
}

/// Publish a class definition: `put_class` its flattened descriptor and `bind_class` its name
/// from the transaction time `tx`, which binds each method `N/m` as well (SPEC 16.3, 9.7;
/// datumesh-store 0.4.1). Every method's function descriptor must be stored. Returns the class
/// hash. Since spec-0.5.0.
#[cfg(feature = "engine")]
pub fn publish_class<S: datumesh_store::Store + ?Sized>(
    store: &S,
    definition: &ClassDefinition,
    tx: &str,
) -> Result<String, datumesh_store::Error> {
    let hash = store.put_class(&definition.bytes)?;
    store.bind_class(&definition.name, &hash, tx)?;
    Ok(hash)
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
