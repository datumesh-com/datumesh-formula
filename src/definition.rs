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

use crate::classes::definer;
use crate::env::{Env, EnvError, Signature};
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
        Ok(self.class_by_hash(class_hash))
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
            fn class_name(&self, class_hash: &str) -> Result<Option<String>, String> {
                crate::call::store_class_name_of(self, class_hash, None)
            }
            fn function_descriptor(&self, fn_hash: &str) -> Result<Option<Value>, String> {
                store_function_descriptor(self, fn_hash)
            }
        }
    )*};
}

#[cfg(feature = "engine")]
store_callees!(datumesh_store::PgStore, dyn datumesh_store::Store + Send + Sync, dyn datumesh_store::Store);

/// The ancestors of a class, by name, into the environment (16.3, K5): each parent is found by its
/// hash through `callees`, which names it, up to a parent no name is bound to.
fn load_ancestors(env: &mut Env, class: &ClassDescriptor, callees: &dyn Callees) -> Result<(), DefineError> {
    let mut cur = class.parent.clone();
    while let Some(ph) = cur.take() {
        if let Some((_, d)) = env.classes.by_hash(&ph) {
            cur = d.parent.clone();
            continue;
        }
        let Some(pname) = callees.class_name(&ph).map_err(DefineError::Callees)? else { break };
        let Some(pdesc) = callees.class_descriptor(&ph).map_err(DefineError::Callees)? else { break };
        cur = pdesc.parent.clone();
        env.classes.insert(&pname, pdesc);
    }
    Ok(())
}

/// Load the class `class` into the environment: its ancestors, the class, and the functions its
/// methods are read from (16.3, K5): a method a class inherits is the function of the class that
/// defines it (`D/m`), which is the one bound. A class whose defining class is no name bound is
/// loaded without that method, so that a read of it is unresolved.
fn load_class(env: &mut Env, class: &str, callees: &dyn Callees, at: &str) -> Result<(), DefineError> {
    let Some(c) = callees.bound_class(class, at).map_err(DefineError::Callees)? else { return Ok(()) };
    load_ancestors(env, &c.descriptor, callees)?;
    let mut fqs = Vec::new();
    for m in c.descriptor.methods.keys() {
        let found = definer(
            &c.descriptor,
            m,
            |ph| Ok::<_, std::convert::Infallible>(env.class_by_hash(ph)),
            |ph| Ok::<_, std::convert::Infallible>(env.classes.by_hash(ph).map(|(n, _)| n.to_string())),
        );
        match found {
            Ok(Some(d)) => fqs.push(format!("{d}/{m}")),
            Ok(None) => fqs.push(format!("{class}/{m}")),
            Err(_) => {}
        }
    }
    for fq in fqs {
        if let Some(f) = callees.callee(&fq, at).map_err(DefineError::Callees)? {
            env.add_function(&fq, &f.descriptor).map_err(|e| DefineError::Callees(e.0))?;
        }
    }
    env.classes.insert(class, c.descriptor);
    Ok(())
}

/// The classes a function's descriptor names: its returned key's and its arguments' (7, 16.6). A
/// read of such a function needs them loaded, as a dispatch on its returned key does (K2).
fn descriptor_classes(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let Some(sig) = v.get("signature") else { return out };
    let mut sets = vec![sig.get("returns").and_then(|r| r.get("classes"))];
    if let Some(Value::Object(args)) = sig.get("args") {
        sets.extend(args.iter().map(|(_, a)| a.get("classes")));
    }
    for set in sets.into_iter().flatten() {
        if let Value::Array(names) = set {
            out.extend(names.iter().filter_map(|n| n.as_str().map(str::to_string)));
        }
    }
    out
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
    let names = crate::read_names_resolved(source, aliases, method_of)?;
    let mut used = BTreeMap::new();
    if let Some(class) = method_of {
        load_class(&mut env, class, callees, at)?;
    }
    for class in &sig_classes {
        load_class(&mut env, class, callees, at)?;
    }
    for callee in names {
        if callee == name {
            continue;
        }
        if callees.bound_class(&callee, at).map_err(DefineError::Callees)?.is_some() {
            load_class(&mut env, &callee, callees, at)?;
        }
        if let Some(c) = callees.callee(&callee, at).map_err(DefineError::Callees)? {
            for class in descriptor_classes(&c.descriptor) {
                load_class(&mut env, &class, callees, at)?;
            }
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
    // The callees the body reads, by name (the methods it reads through a class's inheritance included).
    for input in &compiled.inputs {
        if let (false, Some(f)) = (used.contains_key(input), env.functions.get(input)) {
            used.insert(input.clone(), f.hash.clone());
        }
    }
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

/// A class's draft (K7, the classes round): its keys, its order, its parent (a class hash) and its
/// methods' signatures by name, before the class is defined. `this` in a method of a class that is
/// not yet bound is checked against the draft.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassDraft {
    keys: Value,
    order: Vec<String>,
    parent: Option<String>,
    methods: BTreeMap<String, Value>,
}

impl ClassDraft {
    /// Read `{"keys": {...}, "order": [...], "parent": "<class hash>"?, "methods": {name: signature}}`.
    pub fn from_value(v: &Value) -> Result<ClassDraft, EnvError> {
        let bad = |why: &str| EnvError(format!("class draft: {why}"));
        let keys = v.get("keys").filter(|k| matches!(k, Value::Object(_))).ok_or_else(|| bad("keys is an object"))?;
        let Some(Value::Array(order)) = v.get("order") else { return Err(bad("order is an array")) };
        let order = order
            .iter()
            .map(|o| o.as_str().map(str::to_string).ok_or_else(|| bad("order names")))
            .collect::<Result<_, _>>()?;
        let parent = match v.get("parent") {
            None => None,
            Some(p) => Some(p.as_str().ok_or_else(|| bad("parent is a class hash"))?.to_string()),
        };
        let mut methods = BTreeMap::new();
        for (name, sig) in v.get("methods").and_then(Value::as_object).ok_or_else(|| bad("methods is an object"))? {
            methods.insert(name.clone(), sig.clone());
        }
        Ok(ClassDraft { keys: keys.clone(), order, parent, methods })
    }

    /// The flattened descriptor of the draft as class `class`, with a stored stub function for each of
    /// its methods (a signature and no body), by name, so that the methods' hashes are the stubs'.
    fn describe(
        &self,
        class: &str,
        inherited: &BTreeMap<String, String>,
    ) -> Result<(ClassDescriptor, BTreeMap<String, Callee>), EnvError> {
        let ns = class.rsplit_once('/').map_or(class, |(n, _)| n);
        let mut stubs = BTreeMap::new();
        let mut hashes = Vec::new();
        for (m, sig) in &self.methods {
            let stub = Value::Object(vec![
                (
                    "code".to_string(),
                    Value::Object(vec![
                        ("body".to_string(), Value::String(String::new())),
                        ("lang".to_string(), Value::String("stored".to_string())),
                    ]),
                ),
                ("inputs".to_string(), Value::Array(Vec::new())),
                ("signature".to_string(), sig.clone()),
            ]);
            let fd = FunctionDescriptor::from_value(&stub).map_err(|e| EnvError(e.to_string()))?;
            let canonical = json::parse(&fd.canonical()).map_err(|e| EnvError(e.to_string()))?;
            hashes.push((m.clone(), Value::String(fd.hash())));
            stubs.insert(
                m.clone(),
                Callee { fn_hash: fd.hash(), start: crate::eval::ZERO_INSTANT.to_string(), descriptor: canonical },
            );
        }
        // A flattened class has its parent's methods too, by the parent's hashes (16.2).
        for (m, h) in inherited {
            if !self.methods.contains_key(m) {
                hashes.push((m.clone(), Value::String(h.clone())));
            }
        }
        let mut members = vec![
            ("keys".to_string(), self.keys.clone()),
            ("order".to_string(), Value::Array(self.order.iter().map(|o| Value::String(o.clone())).collect())),
            ("methods".to_string(), Value::Object(hashes)),
            (
                "steward".to_string(),
                Value::Object(vec![
                    ("namespace".to_string(), Value::String(ns.to_string())),
                    ("process".to_string(), Value::String(format!("https://{ns}/classes/change"))),
                ]),
            ),
        ];
        if let Some(p) = &self.parent {
            members.push(("parent".to_string(), Value::String(p.clone())));
        }
        let desc = ClassDescriptor::from_value(&Value::Object(members)).map_err(|e| EnvError(e.to_string()))?;
        Ok((desc, stubs))
    }
}

/// The callees of a draft's class: the draft's class, and its stub methods, with the rest as the
/// callees give them.
struct DraftCallees<'a> {
    class: &'a str,
    /// The method being defined: it is not bound in the draft's class (a read of it is unresolved).
    own: &'a str,
    desc: ClassDescriptor,
    hash: String,
    stubs: BTreeMap<String, Callee>,
    inner: &'a dyn Callees,
}

impl Callees for DraftCallees<'_> {
    fn callee(&self, name: &str, at: &str) -> Result<Option<Callee>, String> {
        if name == self.own {
            return Ok(None);
        }
        if let Some(m) = name.strip_prefix(self.class).and_then(|r| r.strip_prefix('/')) {
            if let Some(c) = self.stubs.get(m) {
                return Ok(Some(c.clone()));
            }
        }
        self.inner.callee(name, at)
    }
    fn bound_class(&self, name: &str, at: &str) -> Result<Option<ClassCallee>, String> {
        if name == self.class {
            let start = crate::eval::ZERO_INSTANT.to_string();
            return Ok(Some(ClassCallee { class_hash: self.hash.clone(), start, descriptor: self.desc.clone() }));
        }
        self.inner.bound_class(name, at)
    }
    fn class_descriptor(&self, class_hash: &str) -> Result<Option<ClassDescriptor>, String> {
        if class_hash == self.hash {
            return Ok(Some(self.desc.clone()));
        }
        self.inner.class_descriptor(class_hash)
    }
    fn class_name(&self, class_hash: &str) -> Result<Option<String>, String> {
        if class_hash == self.hash {
            return Ok(Some(self.class.to_string()));
        }
        self.inner.class_name(class_hash)
    }
    fn function_descriptor(&self, fn_hash: &str) -> Result<Option<Value>, String> {
        if let Some(c) = self.stubs.values().find(|c| c.fn_hash == fn_hash) {
            return Ok(Some(c.descriptor.clone()));
        }
        self.inner.function_descriptor(fn_hash)
    }
}

/// Define `source` as the method `name` of the class `class`, whose draft gives the class before it
/// is defined (K7, the classes round): as [`define_method`], with `this` checked against the draft's
/// keys and signatures (`this.m` reads `class/m` by name, or the defining class's, 16.3, K5). The
/// draft's methods are the stub functions of its class; the class itself is not bound.
pub fn define_method_in_draft(
    class: &str,
    draft: &ClassDraft,
    name: &str,
    source: &str,
    signature: &Value,
    callees: &dyn Callees,
    at: &str,
) -> Result<Definition, DefineError> {
    // The parent's methods, by the parent's hashes, which the draft's flattened descriptor carries (K5).
    let mut inherited = BTreeMap::new();
    if let Some(p) = &draft.parent {
        if let Some(pd) = callees.class_descriptor(p).map_err(DefineError::Callees)? {
            inherited = pd.methods.clone();
        }
    }
    let (desc, stubs) =
        draft.describe(class, &inherited).map_err(|e| DefineError::Formula(Error::new(Code::Syntax, e.0)))?;
    let hash = desc.hash();
    let wrapped = DraftCallees { class, own: name, desc, hash, stubs, inner: callees };
    define_in(name, source, signature, &BTreeMap::new(), &wrapped, at, Some(class))
}
