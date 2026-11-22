//! Calls (SPEC 5.8, spec-0.4.2): a request naming a cell as a read of the formula dialect whose
//! arguments are literals, optionally pinned to a function hash and followed by `?asof=`. A call
//! denotes the cell URI it is written as; [`parse_call`] gives that URI's reference and pin.
//!
//! ```
//! use datumesh_core::json;
//! use datumesh_formula::call::{parse_call, MapCallEnv};
//!
//! let env = MapCallEnv::from_value(&json::parse(br#"{"aliases": {"fx": "rates.example.org/fx_rate"},
//!     "descriptors": [], "functions": {"rates.example.org/fx_rate": {"code": {"body": "", "lang": "stored"},
//!     "inputs": [], "signature": {"args": {"base": {"type": "string", "valid": false},
//!     "date": {"type": "date", "valid": true}, "quote": {"type": "string", "valid": false}},
//!     "order": ["base", "quote", "date"], "returns": {"type": "number", "unit": "1"}}}}}"#).unwrap()).unwrap();
//! let c = parse_call(r#"@fx("EUR", "USD", 2024-03-15)?asof=2024-03-16T09:00:00.000000Z"#, &env).unwrap();
//! assert_eq!(c.uri(), "cell://rates.example.org/fx_rate?base=EUR&date=2024-03-15&quote=USD&asof=2024-03-16T09:00:00.000000Z");
//! assert_eq!(parse_call("rates.example.org/fx_rate(\"EUR\")", &env).unwrap_err().code(), "E_ARG_MISSING");
//! ```
//!
//! The module needs no engine: the aliases, the bindings and the descriptors known by hash come
//! from a [`CallEnv`] ([`MapCallEnv`] in memory; with feature `engine`, a store).

use std::collections::BTreeMap;
use std::fmt;

use datumesh_core::json::Value;
use datumesh_core::{
    format_object_key, format_reference, lexical, parse_object_key, Arg, CellRef, ClassDescriptor, FunctionDescriptor,
    KeyNormalisation, KeyValue, ObjectKey, Reference, Resolution, ResolutionKind, Resolved, Scalar, Type,
};

pub use datumesh_core::fold_label;

use crate::classes::{definer, DefinerError};
use crate::lex::{self, Kind, Token};
use crate::Code;

/// Where a call's callee is looked up: the alias map, the bindings of names as of a time, and
/// the descriptors known by function hash.
pub trait CallEnv {
    /// The fully qualified name an alias (without `@`) stands for.
    fn alias(&self, alias: &str) -> Result<Option<String>, String>;
    /// The descriptor `name` is bound to as of `asof` (`None`: now).
    fn bound(&self, name: &str, asof: Option<&str>) -> Result<Option<FunctionDescriptor>, String>;
    /// The descriptor whose function hash is `fn_hash`, when known.
    fn pinned(&self, fn_hash: &str) -> Result<Option<FunctionDescriptor>, String>;

    /// The flattened class descriptor the class `name` is bound to as of `asof` (`None`: now), or
    /// `None` when `name` is not a class (a function name, or unbound). Binding a class binds each
    /// method `N/m`, so the method functions come from [`CallEnv::bound`]. Since spec-0.5.0
    /// (16.3). Provided: no classes.
    fn bound_class(&self, name: &str, asof: Option<&str>) -> Result<Option<ClassDescriptor>, String> {
        let _ = (name, asof);
        Ok(None)
    }

    /// The instant of the alias snapshot [`CallEnv::alias`] answers from, recorded in the answer's
    /// `resolved.aliases_tx` when a call resolved an alias (16.7). Since spec-0.5.0. Provided:
    /// none.
    fn aliases_tx(&self) -> Result<Option<String>, String> {
        Ok(None)
    }

    /// The mesh's now, which a call without `?asof=` takes for an `asof` default and for a label's
    /// lookup (16.7). Since spec-0.5.0. Provided: none (the call then cannot default or look up).
    fn now(&self) -> Result<Option<String>, String> {
        Ok(None)
    }

    /// **The lookup hook** (16.1, 16.7): the class's `lookup` method read at `label` as of `asof`
    /// (the call's `asof`, or [`CallEnv::now`]): the cell `N/<lookup>(label: ...)`'s value, a key
    /// encoding of an object of `class` (16.5). `label` has been read as the lookup function's
    /// `label` argument already (its `key_normalisation`, if it carries one, holds). `None`: the
    /// cell has no value as of `asof`, which is `E_NAME_UNBOUND`. Since spec-0.5.0. Provided:
    /// never a value.
    fn lookup(&self, class: &str, label: &str, asof: &str) -> Result<Option<String>, String> {
        let _ = (class, label, asof);
        Ok(None)
    }

    /// The name a class hash is bound to as of `asof` (`None`: now), when the environment knows it.
    /// A method a subclass inherits is read from the class that defines it, found through the parent
    /// hashes, each named by this (16.3, K5, the classes round). Provided: none.
    fn class_name_of(&self, class_hash: &str, asof: Option<&str>) -> Result<Option<String>, String> {
        let _ = (class_hash, asof);
        Ok(None)
    }

    /// The flattened class descriptor with this class hash, whether or not a name is bound to it
    /// (K5, the classes round): whether a method is inherited is decided from it. Provided: none.
    fn class_descriptor_of(&self, class_hash: &str, asof: Option<&str>) -> Result<Option<ClassDescriptor>, String> {
        let _ = (class_hash, asof);
        Ok(None)
    }

    /// **The chain hook** (5.8, 16.6, K10, the classes round): the value of the cell `cell` as of
    /// `asof`, a key encoding of an object (`None`: the cell has no value as of `asof`). A chained
    /// call reads each link's cell through it, and the last method's on the key it holds. Provided:
    /// never a value.
    fn chain(&self, cell: &CellRef, asof: &str) -> Result<Option<String>, String> {
        let _ = (cell, asof);
        Ok(None)
    }
}

/// A [`CallEnv`] in memory: functions bound by name (at every time), an alias map and
/// descriptors known by hash (a `call` vector's `env`, SPEC 11).
#[derive(Clone, Debug, Default)]
pub struct MapCallEnv {
    /// Functions by fully qualified name.
    pub functions: BTreeMap<String, FunctionDescriptor>,
    /// Aliases (without `@`) to fully qualified names.
    pub aliases: BTreeMap<String, String>,
    /// Descriptors known by hash (besides those of `functions`).
    pub descriptors: BTreeMap<String, FunctionDescriptor>,
    /// Since spec-0.5.0: flattened class descriptors by class name (bound at every time).
    pub classes: BTreeMap<String, ClassDescriptor>,
    /// Since spec-0.5.0: the instant of the alias snapshot (`aliases_tx`).
    pub aliases_tx: Option<String>,
    /// Since spec-0.5.0: the mesh's now, for an `asof` default or a lookup in a call without
    /// `?asof=`.
    pub now: Option<String>,
    /// Since spec-0.5.0: class name to `{label: key encoding}`, the cells of the class's `lookup`
    /// method (the class's alias table), read at any `asof`.
    pub lookups: BTreeMap<String, BTreeMap<String, String>>,
    /// Since the classes round (K5): the flattened class descriptors held by hash only, with no name
    /// bound (the member `class_descriptors`).
    pub held: Vec<ClassDescriptor>,
    /// Since the classes round (K10): the values of the cells a chain reads, by their canonical call
    /// without `?asof=` (`fn(arg: literal, ...)`), each a key encoding; a cell with no value is absent.
    pub cells: BTreeMap<String, String>,
}

impl MapCallEnv {
    /// Read `{"functions": {name: descriptor}, "aliases": {alias: name}, "descriptors": [...]}`
    /// (each member optional).
    pub fn from_value(v: &Value) -> Result<MapCallEnv, datumesh_core::Error> {
        let mut env = MapCallEnv::default();
        if let Some(fs) = v.get("functions").and_then(Value::as_object) {
            for (n, d) in fs {
                env.functions.insert(n.clone(), FunctionDescriptor::from_value(d)?);
            }
        }
        if let Some(al) = v.get("aliases").and_then(Value::as_object) {
            for (a, n) in al {
                if let Some(n) = n.as_str() {
                    env.aliases.insert(a.clone(), n.to_string());
                }
            }
        }
        if let Some(Value::Array(ds)) = v.get("descriptors") {
            for d in ds {
                let d = FunctionDescriptor::from_value(d)?;
                env.descriptors.insert(d.hash(), d);
            }
        }
        if let Some(cs) = v.get("classes").and_then(Value::as_object) {
            for (n, d) in cs {
                env.classes.insert(n.clone(), ClassDescriptor::from_value(d)?);
            }
        }
        env.aliases_tx = v.get("aliases_tx").and_then(Value::as_str).map(str::to_string);
        env.now = v.get("now").and_then(Value::as_str).map(str::to_string);
        if let Some(ls) = v.get("lookups").and_then(Value::as_object) {
            for (class, labels) in ls {
                let mut m = BTreeMap::new();
                for (label, key) in labels.as_object().into_iter().flatten() {
                    if let Some(k) = key.as_str() {
                        m.insert(label.clone(), k.to_string());
                    }
                }
                env.lookups.insert(class.clone(), m);
            }
        }
        if let Some(Value::Array(ds)) = v.get("class_descriptors") {
            for d in ds {
                env.held.push(ClassDescriptor::from_value(d)?);
            }
        }
        if let Some(cs) = v.get("cells").and_then(Value::as_object) {
            for (call, key) in cs {
                if let Some(k) = key.as_str() {
                    env.cells.insert(call.clone(), k.to_string());
                }
            }
        }
        Ok(env)
    }
}

impl CallEnv for MapCallEnv {
    fn alias(&self, alias: &str) -> Result<Option<String>, String> {
        Ok(self.aliases.get(alias).cloned())
    }
    fn bound(&self, name: &str, _asof: Option<&str>) -> Result<Option<FunctionDescriptor>, String> {
        Ok(self.functions.get(name).cloned())
    }
    fn pinned(&self, fn_hash: &str) -> Result<Option<FunctionDescriptor>, String> {
        if let Some(d) = self.descriptors.get(fn_hash) {
            return Ok(Some(d.clone()));
        }
        Ok(self.functions.values().find(|d| d.hash() == fn_hash).cloned())
    }
    fn bound_class(&self, name: &str, _asof: Option<&str>) -> Result<Option<ClassDescriptor>, String> {
        Ok(self.classes.get(name).cloned())
    }
    fn aliases_tx(&self) -> Result<Option<String>, String> {
        Ok(self.aliases_tx.clone())
    }
    fn now(&self) -> Result<Option<String>, String> {
        Ok(self.now.clone())
    }
    fn lookup(&self, class: &str, label: &str, _asof: &str) -> Result<Option<String>, String> {
        Ok(self.lookups.get(class).and_then(|m| m.get(label)).cloned())
    }
    fn class_name_of(&self, class_hash: &str, _asof: Option<&str>) -> Result<Option<String>, String> {
        Ok(self.classes.iter().find(|(_, d)| d.hash() == class_hash).map(|(n, _)| n.clone()))
    }
    fn class_descriptor_of(&self, class_hash: &str, _asof: Option<&str>) -> Result<Option<ClassDescriptor>, String> {
        let named = self.classes.values().find(|d| d.hash() == class_hash);
        Ok(named.or_else(|| self.held.iter().find(|d| d.hash() == class_hash)).cloned())
    }
    fn chain(&self, cell: &CellRef, _asof: &str) -> Result<Option<String>, String> {
        Ok(self.cells.get(&canonical_cell(cell)).cloned())
    }
}

/// A cell's canonical call without `?asof=`: its full name and its arguments, each labelled, in
/// ascending order of names (16.7), as the `cells` of a call environment write it.
fn canonical_cell(cell: &CellRef) -> String {
    let parts: Vec<String> = cell.args.iter().map(|(k, a)| format!("{k}: {}", literal(a))).collect();
    format!("{}/{}({})", cell.ns, cell.fn_name, parts.join(", "))
}

#[cfg(feature = "engine")]
fn store_descriptor<S: datumesh_store::Store + ?Sized>(
    store: &S,
    fn_hash: &str,
) -> Result<Option<FunctionDescriptor>, String> {
    match store.descriptor(fn_hash) {
        Some(b) => FunctionDescriptor::from_json(&b).map(Some).map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

#[cfg(feature = "engine")]
fn store_bound<S: datumesh_store::Store + ?Sized>(
    store: &S,
    name: &str,
    asof: Option<&str>,
) -> Result<Option<FunctionDescriptor>, String> {
    let at = asof.unwrap_or(crate::definition::LATEST);
    match store.resolve_name(name, at).map_err(|e| e.to_string())? {
        Some(h) => store_descriptor(store, &h),
        None => Ok(None),
    }
}

/// The flattened class descriptor a store binds the class name to as of `asof` (16.1, 9.7).
#[cfg(feature = "engine")]
fn store_class<S: datumesh_store::Store + ?Sized>(
    store: &S,
    name: &str,
    asof: Option<&str>,
) -> Result<Option<ClassDescriptor>, String> {
    let at = asof.unwrap_or(crate::definition::LATEST);
    let Some(b) = store.resolve_class_binding(name, at).map_err(|e| e.to_string())? else { return Ok(None) };
    match store.class(&b.class_hash).map_err(|e| e.to_string())? {
        Some(bytes) => ClassDescriptor::from_json(&bytes).map(Some).map_err(|e| e.to_string()),
        None => Err(format!("class {} is bound but not stored", b.class_hash)),
    }
}

/// The first class name a store binds the class hash to as of `asof` (K5, ascending; 16.3).
#[cfg(feature = "engine")]
pub(crate) fn store_class_name_of<S: datumesh_store::Store + ?Sized>(
    store: &S,
    class_hash: &str,
    asof: Option<&str>,
) -> Result<Option<String>, String> {
    let at = asof.unwrap_or(crate::definition::LATEST);
    let names = store.class_names_bound_to(class_hash, at).map_err(|e| e.to_string())?;
    Ok(names.into_iter().next())
}

/// The flattened class descriptor a store holds under this class hash, bound or not (K5).
#[cfg(feature = "engine")]
pub(crate) fn store_class_by_hash<S: datumesh_store::Store + ?Sized>(
    store: &S,
    class_hash: &str,
) -> Result<Option<ClassDescriptor>, String> {
    match store.class(class_hash).map_err(|e| e.to_string())? {
        Some(bytes) => ClassDescriptor::from_json(&bytes).map(Some).map_err(|e| e.to_string()),
        None => Ok(None),
    }
}

/// A store as a [`CallEnv`]: bindings and descriptors from it, no aliases (a mesh's alias map is
/// its own: wrap the store to add one). `None` as the asof is the latest binding.
#[cfg(feature = "engine")]
impl CallEnv for datumesh_store::PgStore {
    fn alias(&self, _alias: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn class_name_of(&self, class_hash: &str, asof: Option<&str>) -> Result<Option<String>, String> {
        store_class_name_of(self, class_hash, asof)
    }
    fn class_descriptor_of(&self, class_hash: &str, _asof: Option<&str>) -> Result<Option<ClassDescriptor>, String> {
        store_class_by_hash(self, class_hash)
    }
    fn bound(&self, name: &str, asof: Option<&str>) -> Result<Option<FunctionDescriptor>, String> {
        store_bound(self, name, asof)
    }
    fn bound_class(&self, name: &str, asof: Option<&str>) -> Result<Option<ClassDescriptor>, String> {
        store_class(self, name, asof)
    }
    fn pinned(&self, fn_hash: &str) -> Result<Option<FunctionDescriptor>, String> {
        store_descriptor(self, fn_hash)
    }
}

#[cfg(feature = "engine")]
impl CallEnv for dyn datumesh_store::Store + Send + Sync {
    fn alias(&self, _alias: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn class_name_of(&self, class_hash: &str, asof: Option<&str>) -> Result<Option<String>, String> {
        store_class_name_of(self, class_hash, asof)
    }
    fn class_descriptor_of(&self, class_hash: &str, _asof: Option<&str>) -> Result<Option<ClassDescriptor>, String> {
        store_class_by_hash(self, class_hash)
    }
    fn bound(&self, name: &str, asof: Option<&str>) -> Result<Option<FunctionDescriptor>, String> {
        store_bound(self, name, asof)
    }
    fn bound_class(&self, name: &str, asof: Option<&str>) -> Result<Option<ClassDescriptor>, String> {
        store_class(self, name, asof)
    }
    fn pinned(&self, fn_hash: &str) -> Result<Option<FunctionDescriptor>, String> {
        store_descriptor(self, fn_hash)
    }
}

/// Why a call is refused: a code of 5.8, or the environment's own failure.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CallError {
    /// A code: `E_PARSE_URI`, an `E_CANON_*` code, `E_NAME_UNBOUND`, `E_FN_UNAVAILABLE`,
    /// `E_ARG_UNKNOWN` or `E_ARG_MISSING`; and a detail.
    Code(&'static str, String),
    /// The [`CallEnv`] failed (a store error).
    Env(String),
}

impl CallError {
    /// The code; `env` for the environment's failure.
    pub fn code(&self) -> &str {
        match self {
            CallError::Code(c, _) => c,
            CallError::Env(_) => "env",
        }
    }
}

impl fmt::Display for CallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CallError::Code(c, d) => write!(f, "{c}: {d}"),
            CallError::Env(m) => write!(f, "the call's environment failed: {m}"),
        }
    }
}

impl std::error::Error for CallError {}

fn fail<T>(code: &'static str, detail: impl Into<String>) -> Result<T, CallError> {
    Err(CallError::Code(code, detail.into()))
}

/// A parsed call: the cell reference it denotes (its arguments typed by the callee's
/// signature, its asof) and its pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    /// The cell reference.
    pub cell: CellRef,
    /// The pin: the function hash the call names, if any.
    pub fn_hash: Option<String>,
    /// Since spec-0.5.0 (16.7, 5.5): what the call resolved, for the answer's envelope member
    /// `resolved`: the full canonical call and each alias, method, default and lookup resolution,
    /// with `aliases_tx` when an alias was used. `None` when nothing was resolved: a call
    /// written in full (`call/class-method-plain`).
    pub resolved: Option<Resolved>,
}

impl Call {
    /// The core's reference (spec-0.4.2): the cell reference with its pin.
    pub fn reference(&self) -> Reference {
        let mut r = Reference::from(self.cell.clone());
        r.fn_hash = self.fn_hash.clone();
        r
    }

    /// The cell URI the call is written as (5.1, 5.2; the core's `format_reference`): the
    /// arguments in order, then `fn_hash=` when pinned, then `asof=`.
    pub fn uri(&self) -> String {
        format_reference(&self.reference())
    }
}

/// The read's text and its query, split at the first `?` outside a string.
fn split_query(text: &str) -> (&str, Option<&str>) {
    let mut in_string = false;
    for (i, c) in text.char_indices() {
        match c {
            '"' => in_string = !in_string,
            '?' if !in_string => return (&text[..i], Some(&text[i + 1..])),
            _ => {}
        }
    }
    (text, None)
}

/// An unquoted token's kind (5.8): exactly one token of the dialect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bare {
    Number,
    Word,
    /// A digit-led word (spec-0.6.0, C3): a value only in a string position.
    Digit,
    Date,
    Instant,
    Period,
    /// Since the classes round (K11): a run of digits and hyphens, a string in a string position.
    DigitRun,
}

/// A value as written in a request (5.8): `*`, a quoted string, an unquoted token, or an object
/// (a class-typed position's key, written as the object).
#[derive(Clone, Debug)]
enum Val {
    Open,
    Quoted(String),
    Bare(Bare, String),
    Object(String, Vec<RArg>),
}

/// An argument as written: its label, if any, and its value.
#[derive(Clone, Debug)]
struct RArg {
    label: Option<String>,
    val: Val,
}

/// The receiver of a call: the parenthesised arguments of a function, an object's keys, or a
/// label in brackets (before a method, 16.7).
#[derive(Clone, Debug)]
enum Recv {
    Args(Vec<RArg>),
    Label(Val),
}

/// A method call `.name(args)` after the receiver.
#[derive(Clone, Debug)]
struct Method {
    name: String,
    qualified: bool,
    args: Vec<RArg>,
}

/// The callee as written.
#[derive(Clone, Debug)]
struct Head {
    /// As written: `fx_rate`, `@fx_rate` or the qualified name.
    written: String,
    /// The name (for an alias or a bare name, without `@`).
    text: String,
    alias: bool,
    qualified: bool,
}

/// A request's read as written (5.8), before any lookup.
#[derive(Clone, Debug)]
struct Request {
    head: Head,
    pin: Option<String>,
    recv: Recv,
    /// The methods called, in order: one, or a chain of them (K10, the classes round).
    methods: Vec<Method>,
}

/// A recursive-descent reader of a request's tokens (the grammar of Appendix A, `call`).
struct Reader<'a> {
    t: &'a [Token],
    i: usize,
}

fn syntax<T>(what: &str) -> Result<T, CallError> {
    fail("E_PARSE_URI", format!("not a call: {what}"))
}

impl Reader<'_> {
    fn at(&self, k: usize) -> Option<&Token> {
        self.t.get(self.i + k).filter(|t| t.kind != Kind::End)
    }

    fn is(&self, k: usize, p: &str) -> bool {
        self.at(k).is_some_and(|t| t.is(p))
    }

    /// `"(" [ arg *( "," arg ) ] ")"`, with the cursor at `(`.
    fn args(&mut self) -> Result<Vec<RArg>, CallError> {
        if !self.is(0, "(") {
            return syntax("arguments in parentheses");
        }
        self.i += 1;
        let mut out = Vec::new();
        if self.is(0, ")") {
            self.i += 1;
            return Ok(out);
        }
        loop {
            let mut label = None;
            if self.at(0).is_some_and(|t| t.kind == Kind::Word) && self.is(1, ":") {
                label = self.at(0).map(|t| t.text.clone());
                self.i += 2;
            } else if out.iter().any(|a: &RArg| a.label.is_some()) {
                return syntax("a positional argument after a labelled one");
            }
            let val = self.value()?;
            out.push(RArg { label, val });
            if self.is(0, ",") {
                self.i += 1;
            } else if self.is(0, ")") {
                self.i += 1;
                return Ok(out);
            } else {
                return syntax("arguments are separated by commas and closed by )");
            }
        }
    }

    /// One value (5.8): `*` where an argument ends, a quoted string, an unquoted token, or an
    /// object (a qualified name and its arguments).
    fn value(&mut self) -> Result<Val, CallError> {
        let Some(t) = self.at(0) else { return syntax("an argument is missing") };
        let next_ends = self.is(1, ",") || self.is(1, ")");
        let val = match t.kind {
            Kind::Punct if t.text == "*" && next_ends => {
                self.i += 1;
                return Ok(Val::Open);
            }
            Kind::Punct if t.text == "-" => match self.at(1) {
                Some(n) if n.kind == Kind::Number && n.unit.is_none() => {
                    let v = Val::Bare(Bare::Number, format!("-{}", n.text));
                    self.i += 2;
                    return Ok(v);
                }
                _ => return syntax("'-' before something other than a number"),
            },
            Kind::Str => Val::Quoted(t.text.clone()),
            Kind::Number if t.unit.is_none() => Val::Bare(Bare::Number, t.text.clone()),
            Kind::Date => Val::Bare(Bare::Date, t.text.clone()),
            Kind::Instant => Val::Bare(Bare::Instant, t.text.clone()),
            Kind::Period => Val::Bare(Bare::Period, t.text.clone()),
            Kind::Word => Val::Bare(Bare::Word, t.text.clone()),
            Kind::DigitWord => Val::Bare(Bare::Digit, t.text.clone()),
            Kind::DigitRun => Val::Bare(Bare::DigitRun, t.text.clone()),
            Kind::Qualified if self.is(1, "(") => {
                let name = t.text.clone();
                self.i += 1;
                let args = self.args()?;
                return Ok(Val::Object(name, args));
            }
            _ => return syntax("an argument is a literal, an object or *"),
        };
        self.i += 1;
        Ok(val)
    }

    /// The whole request: callee, pin, receiver, optional method, and nothing else.
    fn request(&mut self) -> Result<Request, CallError> {
        let Some(t) = self.at(0) else { return syntax("empty") };
        let head = match t.kind {
            Kind::Qualified => Head { written: t.text.clone(), text: t.text.clone(), alias: false, qualified: true },
            Kind::Alias => {
                Head { written: format!("@{}", t.text), text: t.text.clone(), alias: true, qualified: false }
            }
            Kind::Word => Head { written: t.text.clone(), text: t.text.clone(), alias: false, qualified: false },
            _ => return fail("E_PARSE_URI", "not a call: it starts with a name or an alias"),
        };
        self.i += 1;
        let pin = match self.at(0).cloned() {
            Some(p) if p.kind == Kind::Pin && (head.qualified || head.alias) => {
                self.i += 1;
                Some(p.text)
            }
            _ => None,
        };
        let recv = if self.is(0, "[") {
            self.i += 1;
            let v = self.value()?;
            if matches!(v, Val::Open | Val::Object(..)) || !self.is(0, "]") {
                return syntax("a label is one value in brackets");
            }
            self.i += 1;
            if !self.is(0, ".") {
                return syntax("a label names an object before a method");
            }
            Recv::Label(v)
        } else {
            Recv::Args(self.args()?)
        };
        let mut methods = Vec::new();
        while self.is(0, ".") {
            let named = self.at(1).is_some_and(|t| matches!(t.kind, Kind::Word | Kind::Qualified)) && self.is(2, "(");
            if !named {
                return syntax("a method is .name(arguments)");
            }
            let t = self.at(1).cloned().unwrap_or_else(|| self.t[self.i].clone());
            self.i += 2;
            let args = self.args()?;
            methods.push(Method { name: t.text.clone(), qualified: t.kind == Kind::Qualified, args });
        }
        if self.at(0).is_some() {
            return syntax("nothing but ?asof= follows the call");
        }
        if matches!(recv, Recv::Label(_)) && methods.is_empty() {
            return syntax("a label is a receiver of a method");
        }
        Ok(Request { head, pin, recv, methods })
    }
}

/// The request's read as written (5.8), by the grammar of Appendix A; anything else is
/// `E_PARSE_URI`.
fn parse_request(tokens: &[Token]) -> Result<Request, CallError> {
    let mut r = Reader { t: tokens, i: 0 };
    r.request()
}

/// The lexical form of a token of `ty`, for the type's code on failure.
fn lexical_of(ty: Type, text: &str) -> Result<Scalar, CallError> {
    Scalar::from_lexical(ty, text).map_err(|e| CallError::Code(e.code_str(), e.to_string()))
}

/// A key's final value and its key value (16.5): the cell argument and the object's key.
type Key = (Arg, KeyValue);

/// A string value under its key normalisation (7, 16.4): canonical, or `E_KEY_NORMALISATION`
/// (a `bad_request` code). Other values have none.
fn check_key(rule: Option<KeyNormalisation>, scalar: &Scalar) -> Result<(), CallError> {
    match (rule, scalar) {
        (Some(rule), Scalar::String(t)) if rule.check(t).is_err() => {
            fail("E_KEY_NORMALISATION", format!("{t:?} is not canonical under {rule}"))
        }
        _ => Ok(()),
    }
}

/// The key encoding of an object's keys (16.5).
fn encode(class: &str, keys: &BTreeMap<String, Key>) -> Result<String, CallError> {
    let pairs = keys.iter().map(|(k, (_, v))| (k.clone(), v.clone())).collect::<Vec<_>>();
    let obj = ObjectKey::new(class, pairs).map_err(|e| CallError::Code(e.code_str(), e.to_string()))?;
    Ok(format_object_key(&obj))
}

/// A canonical literal of a scalar, as the `call` member of `resolved` writes it (16.7).
fn literal(a: &Arg) -> String {
    match a {
        Arg::Slice => "*".to_string(),
        Arg::Value(Scalar::String(t)) => format!("\"{}\"", t.replace('"', "\"\"")),
        Arg::Value(Scalar::Blob(t)) => format!("\"{}\"", t.replace('"', "\"\"")),
        Arg::Value(Scalar::Boolean(b)) => b.to_string(),
        Arg::Value(s) => s.lexical().to_string(),
    }
}

/// The code of a digit-and-hyphen run that is no string here (K11, the classes round): the number
/// lexer's code for a leading part that is no canonical number, else `E_PARSE_URI` (the run is no
/// single token).
fn digit_run_code(text: &str) -> &'static str {
    let first = text.split('-').next().unwrap_or(text);
    if lexical_of(Type::Number, first).is_err() {
        "E_CANON_NUMBER"
    } else {
        "E_PARSE_URI"
    }
}

/// The state of one call's resolution: its environment, the call's asof, what it resolved.
struct Resolving<'a> {
    env: &'a dyn CallEnv,
    asof: Option<String>,
    resolutions: Vec<Resolution>,
    alias_used: bool,
}

fn resolution(f: Result<Resolution, datumesh_core::Error>) -> Result<Resolution, CallError> {
    f.map_err(|e| CallError::Code(e.code_str(), e.to_string()))
}

impl Resolving<'_> {
    /// The instant of a lookup or a default: the call's asof, else the mesh's now (16.7).
    fn instant(&self) -> Result<Option<String>, CallError> {
        match &self.asof {
            Some(a) => Ok(Some(a.clone())),
            None => self.env.now().map_err(CallError::Env),
        }
    }

    fn bound_class(&self, name: &str) -> Result<Option<ClassDescriptor>, CallError> {
        self.env.bound_class(name, self.asof.as_deref()).map_err(CallError::Env)
    }

    fn bound(&self, name: &str) -> Result<Option<FunctionDescriptor>, CallError> {
        self.env.bound(name, self.asof.as_deref()).map_err(CallError::Env)
    }

    /// A value of a scalar type, read by its position's type (5.8, D91).
    fn scalar(ty: Type, val: &Val) -> Result<Arg, CallError> {
        let v = match (val, ty) {
            (Val::Open, _) => return Ok(Arg::Slice),
            (Val::Bare(Bare::Digit, t), Type::String) => lexical_of(ty, t)?,
            (Val::Bare(Bare::Digit, _), _) => {
                return fail("E_CANON_TYPE", format!("a digit-led word in a {} position", ty.as_str()))
            }
            (Val::Bare(Bare::DigitRun, t), Type::String | Type::Blob) => lexical_of(ty, t)?,
            (Val::Bare(Bare::DigitRun, t), _) => {
                return fail(digit_run_code(t), format!("{t:?} is not a {}", ty.as_str()))
            }
            (Val::Quoted(t) | Val::Bare(_, t), Type::String | Type::Blob) => lexical_of(ty, t)?,
            (Val::Bare(Bare::Number, t), Type::Number) => lexical_of(ty, t)?,
            (Val::Bare(Bare::Date, t), Type::Date) => lexical_of(ty, t)?,
            (Val::Bare(Bare::Instant, t), Type::Instant) => lexical_of(ty, t)?,
            (Val::Bare(Bare::Word, t), Type::Boolean) if t == "true" || t == "false" => Scalar::Boolean(t == "true"),
            _ => return fail("E_CANON_TYPE", format!("a value of the wrong kind for a {}", ty.as_str())),
        };
        Ok(Arg::Value(v))
    }

    /// A key-typed value of an argument declaring `classes` (16.5): an object of one of them, or
    /// its encoding quoted; `*` is a slice.
    fn class_value(&mut self, classes: &[String], val: &Val) -> Result<Arg, CallError> {
        match val {
            Val::Open => Ok(Arg::Slice),
            Val::Object(name, args) => {
                if !classes.contains(name) {
                    return fail("E_CANON_TYPE", format!("{name} is not one of the classes of this position"));
                }
                let keys = self.object_keys(name, args)?;
                Ok(Arg::Value(Scalar::String(encode(name, &keys)?)))
            }
            Val::Quoted(text) => Ok(Arg::Value(Scalar::String(self.encoding(text, classes)?))),
            Val::Bare(..) => fail("E_CANON_TYPE", "a bare token in a class-typed position"),
        }
    }

    /// A quoted key encoding (16.5): decoded, canonical, of one of `classes`.
    fn encoding(&self, text: &str, classes: &[String]) -> Result<String, CallError> {
        let key = parse_object_key(text).map_err(|e| CallError::Code("E_KEY_ENCODING", e.to_string()))?;
        if format_object_key(&key) != text {
            return fail("E_KEY_ENCODING", format!("not the canonical encoding: {text}"));
        }
        if !classes.contains(&key.class) {
            return fail("E_CANON_TYPE", format!("{} is not one of the classes of this position", key.class));
        }
        self.check_encoding(&key)?;
        Ok(text.to_string())
    }

    /// A decoded key encoding checked against its class, as an object written there is (16.5, 5.8):
    /// its key names are the class's and each value has its key's type (`E_CANON_TYPE`), and each
    /// string value is canonical under its key's normalisation (`E_KEY_NORMALISATION`). A class not
    /// bound here is not checked.
    fn check_encoding(&self, obj: &ObjectKey) -> Result<(), CallError> {
        let Some(desc) = self.bound_class(&obj.class)? else { return Ok(()) };
        if !obj.keys.keys().eq(desc.keys.keys()) {
            return fail("E_CANON_TYPE", format!("the keys of {} are not its class's", obj.class));
        }
        for (k, v) in &obj.keys {
            match (desc.keys.get(k), v) {
                (Some(datumesh_core::ClassKey::Scalar { ty, key_normalisation, .. }), KeyValue::Scalar(sc))
                    if sc.ty() == *ty =>
                {
                    check_key(*key_normalisation, sc)?;
                }
                (Some(datumesh_core::ClassKey::Class(c)), KeyValue::Object(o)) if o.class == *c => {
                    self.check_encoding(o)?;
                }
                _ => return fail("E_CANON_TYPE", format!("the key {k} of {} is not of its type", obj.class)),
            }
        }
        Ok(())
    }

    /// The keys of an object of `class` (16.5), bound by the class's `order` or by label, every key
    /// once, then each read by its declaration in ascending order of names.
    fn object_keys(&mut self, class: &str, args: &[RArg]) -> Result<BTreeMap<String, Key>, CallError> {
        let desc = self
            .bound_class(class)?
            .ok_or_else(|| CallError::Code("E_NAME_UNBOUND", format!("{class} is not a class")))?;
        let mut bound: BTreeMap<String, &Val> = BTreeMap::new();
        let mut i = 0;
        for a in args {
            let key = match &a.label {
                Some(l) => {
                    if !desc.keys.contains_key(l) {
                        return fail("E_ARG_UNKNOWN", format!("{class} has no key {l}"));
                    }
                    l.clone()
                }
                None => match desc.order.get(i) {
                    Some(k) => {
                        i += 1;
                        k.clone()
                    }
                    None => return fail("E_ARG_UNKNOWN", format!("positional key {} has no parameter", i + 1)),
                },
            };
            if bound.contains_key(&key) {
                return fail("E_CANON_ARG_DUPLICATE", format!("{key} is bound twice"));
            }
            bound.insert(key, &a.val);
        }
        if let Some(k) = desc.keys.keys().find(|k| !bound.contains_key(*k)) {
            return fail("E_ARG_MISSING", format!("{k} is not bound"));
        }
        let mut keys = BTreeMap::new();
        for (k, ck) in &desc.keys {
            let val = bound[k];
            let key = match ck {
                datumesh_core::ClassKey::Scalar { ty, key_normalisation, .. } => {
                    if matches!(val, Val::Open) {
                        return syntax("* is not a key");
                    }
                    let Arg::Value(scalar) = Self::scalar(*ty, val)? else { return syntax("* is not a key") };
                    check_key(*key_normalisation, &scalar)?;
                    (Arg::Value(scalar.clone()), KeyValue::Scalar(scalar))
                }
                datumesh_core::ClassKey::Class(c) => {
                    let text = match val {
                        Val::Object(name, oargs) => {
                            if name != c {
                                return fail("E_CANON_TYPE", format!("{name} is not {c}"));
                            }
                            encode(name, &self.object_keys(name, oargs)?)?
                        }
                        Val::Quoted(t) => self.encoding(t, std::slice::from_ref(c))?,
                        Val::Open => return syntax("* is not a key"),
                        Val::Bare(..) => return fail("E_CANON_TYPE", "a bare token in a class-typed position"),
                    };
                    let obj = parse_object_key(&text).map_err(|e| CallError::Code("E_KEY_ENCODING", e.to_string()))?;
                    (Arg::Value(Scalar::String(text)), KeyValue::Object(obj))
                }
                _ => return syntax("a key of an unknown kind"),
            };
            keys.insert(k.clone(), key);
        }
        Ok(keys)
    }

    /// Bind the arguments of a read of `name` (5.8): the pre-bound keys, then the positional ones
    /// after them, then the labelled; the missing ones (a defaulted one takes the asof, 16.7); then
    /// each value by its type, in ascending order of names.
    fn bind(
        &mut self,
        name: &str,
        desc: &FunctionDescriptor,
        pre: Vec<(String, Arg)>,
        args: &[RArg],
    ) -> Result<BTreeMap<String, Arg>, CallError> {
        enum Src<'v> {
            Pre(Arg),
            Written(&'v Val),
            Default,
            /// A literal default (K13, the classes round): its lexical value.
            Literal(String),
        }
        let declared = &desc.signature.args;
        let extras = &desc.arg_extras;
        let order = desc.order.clone().unwrap_or_default();
        let skip = pre.len();
        let mut bound: BTreeMap<String, Src> = BTreeMap::new();
        for (k, a) in pre {
            bound.insert(k, Src::Pre(a));
        }
        let mut i = 0;
        for a in args {
            let arg = match &a.label {
                None => match order.get(skip + i) {
                    Some(p) => {
                        i += 1;
                        p.clone()
                    }
                    None => return fail("E_ARG_UNKNOWN", format!("positional argument {} has no parameter", i + 1)),
                },
                Some(l) => {
                    if !declared.contains_key(l) {
                        return fail("E_ARG_UNKNOWN", format!("{name} has no argument {l}"));
                    }
                    l.clone()
                }
            };
            if bound.contains_key(&arg) {
                return fail("E_CANON_ARG_DUPLICATE", format!("{arg} is bound twice"));
            }
            bound.insert(arg, Src::Written(&a.val));
        }
        for k in declared.keys() {
            if bound.contains_key(k) {
                continue;
            }
            if extras.get(k).is_some_and(|x| x.default_asof) {
                bound.insert(k.clone(), Src::Default);
            } else if let Some(text) = extras.get(k).and_then(|x| x.default_literal.clone()) {
                bound.insert(k.clone(), Src::Literal(text));
            } else {
                return fail("E_ARG_MISSING", format!("{k} is not bound"));
            }
        }
        let mut out = BTreeMap::new();
        for (k, src) in bound {
            let decl = &declared[&k];
            let x = extras.get(&k).cloned().unwrap_or_default();
            let arg = match src {
                Src::Pre(a) => a,
                Src::Written(val) => match &x.classes {
                    Some(classes) => self.class_value(classes, val)?,
                    None => {
                        let arg = Self::scalar(decl.ty, val)?;
                        if let Arg::Value(scalar) = &arg {
                            check_key(x.key_normalisation, scalar)?;
                        }
                        arg
                    }
                },
                Src::Literal(text) => {
                    self.resolutions.push(resolution(Resolution::new(&k, ResolutionKind::Default, &text))?);
                    Arg::Value(lexical_of(decl.ty, &text)?)
                }
                Src::Default => {
                    let Some(at) = self.instant()? else {
                        return fail("E_ARG_MISSING", format!("{k} defaults to the asof, and the mesh has no now"));
                    };
                    let text = if decl.ty == Type::Date { at[..10].to_string() } else { at };
                    self.resolutions.push(resolution(Resolution::new(&k, ResolutionKind::Default, &text))?);
                    Arg::Value(lexical_of(decl.ty, &text)?)
                }
            };
            out.insert(k, arg);
        }
        Ok(out)
    }
}

/// Parse a call (5.8) against `env`, reporting the first failure in the order 5.8 gives: the
/// text a read of literals then `?asof=` and nothing else (`E_PARSE_URI`, or a literal's code),
/// `asof` an instant, the callee (`E_NAME_UNBOUND`, `E_FN_UNAVAILABLE`), the arguments left to
/// right (`E_ARG_UNKNOWN`, `E_CANON_ARG_DUPLICATE`), the missing ones (`E_ARG_MISSING`), then
/// each value as written (`E_CANON_TYPE`, then its lexical form, then its key normalisation).
/// Since spec-0.5.0, a method call is read through its object (16.3), a label through the
/// class's lookup (16.7), and the result says what was resolved (`resolved`, 5.5).
pub fn parse_call(text: &str, env: &dyn CallEnv) -> Result<Call, CallError> {
    let (read, query) = split_query(text);
    // 1. A read of literals.
    let tokens = lex::lex_request(read).or_else(|e| {
        let code = match e.code {
            Code::Number => "E_CANON_NUMBER",
            Code::String => "E_CANON_STRING",
            Code::Date => "E_CANON_DATE",
            Code::Instant => "E_CANON_INSTANT",
            _ => "E_PARSE_URI",
        };
        fail(code, e.to_string())
    })?;
    let req = parse_request(&tokens)?;
    // 2. Then `?asof=` and nothing else; the asof an instant.
    let asof = match query {
        None => None,
        Some(q) => {
            let Some(a) = q.strip_prefix("asof=") else { return fail("E_PARSE_URI", "only ?asof= may follow a call") };
            if a.contains(['&', '?', '#', '=']) {
                return fail("E_PARSE_URI", "nothing may follow ?asof=<instant>");
            }
            if let Err(e) = lexical::check_instant(a, "call.asof") {
                return fail("E_CANON_INSTANT", e.to_string());
            }
            Some(a.to_string())
        }
    };
    let mut r = Resolving { env, asof: asof.clone(), resolutions: Vec::new(), alias_used: false };
    // 3. The callee: a bare name or an alias through the alias map; a qualified name as it is.
    let fq = if req.head.qualified {
        req.head.text.clone()
    } else {
        match env.alias(&req.head.text).map_err(CallError::Env)? {
            Some(n) => {
                // Every name through the alias map is recorded as an alias resolution (16.7): a bare
                // name or an `@alias`, as written, with or without an alias snapshot and pinned or not.
                r.alias_used = true;
                r.resolutions.push(resolution(Resolution::new(&req.head.written, ResolutionKind::Alias, &n))?);
                n
            }
            None if req.head.alias => {
                return fail("E_NAME_UNBOUND", format!("{} is not an alias of this mesh", req.head.written))
            }
            None => return fail("E_NAME_UNBOUND", format!("{} is not an alias of this mesh", req.head.text)),
        }
    };
    let pinned = match &req.pin {
        Some(h) => match env.pinned(h).map_err(CallError::Env)? {
            Some(d) => Some(d),
            None => return fail("E_FN_UNAVAILABLE", format!("no descriptor of {h} is known")),
        },
        None => None,
    };
    let (call_name, cell_args) = match (pinned, r.bound_class(&fq)?) {
        (None, Some(cd)) => {
            let (name, _, args) = match req.methods.as_slice() {
                [] => return syntax("a call of a class names one method"),
                [m] => class_call(&mut r, &fq, &cd, &req.recv, m)?,
                _ => chain_call(&mut r, &fq, &cd, &req.recv, &req.methods)?,
            };
            (name, args)
        }
        (pinned, _) => {
            let desc = match pinned {
                Some(d) => d,
                None => match r.bound(&fq)? {
                    Some(d) => d,
                    None => return fail("E_NAME_UNBOUND", format!("{fq} is not bound")),
                },
            };
            let Recv::Args(args) = &req.recv else { return syntax("a label is a receiver of a method") };
            if !req.methods.is_empty() {
                return syntax("a method is called on an object");
            }
            let cell_args = r.bind(&fq, &desc, Vec::new(), args)?;
            (fq.clone(), cell_args)
        }
    };
    // What was resolved, for the envelope (16.7): none when nothing was.
    let resolved = if r.resolutions.is_empty() {
        None
    } else {
        let mut parts: Vec<String> = cell_args.iter().map(|(k, a)| format!("{k}: {}", literal(a))).collect();
        parts.sort();
        let pin = req.pin.as_ref().map(|h| format!("@{h}")).unwrap_or_default();
        let mut call = format!("{call_name}{pin}({})", parts.join(", "));
        if let Some(a) = &asof {
            call.push_str(&format!("?asof={a}"));
        }
        let mut res = resolved_of(&call, r.resolutions)?;
        if r.alias_used {
            if let Some(t) = env.aliases_tx().map_err(CallError::Env)? {
                res = res.with_aliases_tx(&t).map_err(|e| CallError::Code(e.code_str(), e.to_string()))?;
            }
        }
        Some(res)
    };
    let (ns, fn_name) = call_name.rsplit_once('/').unwrap_or(("", call_name.as_str()));
    Ok(Call {
        cell: CellRef { ns: ns.to_string(), fn_name: fn_name.to_string(), args: cell_args, asof },
        fn_hash: req.pin,
        resolved,
    })
}

fn resolved_of(call: &str, resolutions: Vec<Resolution>) -> Result<Resolved, CallError> {
    Resolved::new(call, resolutions).map_err(|e| CallError::Code(e.code_str(), e.to_string()))
}

/// A method's target: its name, its descriptor and the key arguments it takes first.
type MethodTarget = (String, FunctionDescriptor, Vec<(String, Arg)>);

/// The key of an object's key value, as a receiver's key: the cell argument and the key value.
fn key_of(v: KeyValue) -> Key {
    let arg = match &v {
        KeyValue::Object(o) => Arg::Value(Scalar::String(format_object_key(o))),
        KeyValue::Scalar(s) => Arg::Value(s.clone()),
        _ => Arg::Value(Scalar::Boolean(false)),
    };
    (arg, v)
}

/// The keys of a receiver: the object's keys as written, or those of the object its label names
/// (16.7, through the class's lookup).
fn receiver_keys(
    r: &mut Resolving,
    class: &str,
    cd: &ClassDescriptor,
    recv: &Recv,
) -> Result<BTreeMap<String, Key>, CallError> {
    match recv {
        Recv::Args(args) => r.object_keys(class, args),
        Recv::Label(val) => r.label_keys(class, cd, val),
    }
}

impl Resolving<'_> {
    /// The object a label names, through the class's lookup (16.7). A label is folded before the
    /// lookup is read when the lookup's `label` argument is `label_fold` (K8, 16.4); the resolution
    /// records the label as written.
    fn label_keys(&mut self, class: &str, cd: &ClassDescriptor, val: &Val) -> Result<BTreeMap<String, Key>, CallError> {
        let Some(lookup) = cd.lookup.clone() else {
            return fail("E_NAME_UNBOUND", format!("{class} has no lookup: the label is unbound"));
        };
        let written = match val {
            Val::Quoted(t) | Val::Bare(_, t) => t.clone(),
            _ => return syntax("a label is a string"),
        };
        let mut label = written.clone();
        if let Some(f) = self.bound(&format!("{class}/{lookup}"))? {
            match f.arg_extras.get("label").and_then(|x| x.key_normalisation) {
                Some(KeyNormalisation::LabelFold) => label = fold_label(&written),
                Some(rule) if rule.check(&label).is_err() => {
                    return fail("E_KEY_NORMALISATION", format!("{label:?} is not canonical under {rule}"));
                }
                _ => {}
            }
        }
        // A label is read at the call's asof or the mesh's now (16.7): with neither, the lookup is not
        // reached.
        let Some(at) = self.instant()? else {
            return fail("E_ARG_MISSING", format!("the label {written} needs an asof: the mesh has no now"));
        };
        let enc = match self.env.lookup(class, &label, &at).map_err(CallError::Env)? {
            Some(enc) if !enc.is_empty() => enc,
            // A withdrawn label (K8) is unknown, as is one with no value.
            _ => return fail("E_NAME_UNBOUND", format!("the label {written} has no value as of {at}")),
        };
        self.resolutions.push(resolution(Resolution::lookup(&written, &enc, &at))?);
        let obj = parse_object_key(&enc).map_err(|e| CallError::Code("E_KEY_ENCODING", e.to_string()))?;
        Ok(obj.keys.into_iter().map(|(k, v)| (k, key_of(v))).collect())
    }

    /// The name of the class that defines method `m` of an object of `class` (16.3, K5): `D/m` when
    /// it is inherited from D, else `class/m`. Its parent hashes are named as of the call's asof.
    fn method_fq(&self, class: &str, cd: &ClassDescriptor, m: &str) -> Result<String, CallError> {
        let at = self.asof.as_deref();
        let found = definer(
            cd,
            m,
            |ph| self.env.class_descriptor_of(ph, at).map_err(CallError::Env),
            |ph| self.env.class_name_of(ph, at).map_err(CallError::Env),
        );
        match found {
            Ok(Some(d)) => Ok(format!("{d}/{m}")),
            Ok(None) => Ok(format!("{class}/{m}")),
            Err(DefinerError::Unnamed(h)) => fail(
                "E_NAME_UNBOUND",
                format!("no class name is bound to the parent hash {h} of {class}: its method {m} is not known"),
            ),
            Err(DefinerError::Failed(e)) => Err(e),
        }
    }

    /// The function a method call of an object of `class`, with `keys`, reads (16.3, 16.7): its
    /// name, its descriptor, and the key arguments it takes first (bound to the object's keys). Pushes
    /// no resolution.
    fn method_target(
        &self,
        class: &str,
        cd: &ClassDescriptor,
        keys: &BTreeMap<String, Key>,
        m: &Method,
    ) -> Result<MethodTarget, CallError> {
        let name = m.name.clone();
        let (fq, desc, k) = if m.qualified {
            // An extension method: a function whose first arguments are the class's keys, in order.
            let desc =
                self.bound(&name)?.ok_or_else(|| CallError::Code("E_NAME_UNBOUND", format!("{name} is not bound")))?;
            let order = desc.order.clone().unwrap_or_default();
            let k = cd.order.len();
            if order.len() < k || order[..k] != cd.order[..] {
                return fail("E_ARG_UNKNOWN", format!("{name} does not take the keys of {class} first"));
            }
            (name.clone(), desc, k)
        } else {
            if cd.lookup.as_deref() == Some(name.as_str()) || !cd.methods.contains_key(&name) {
                return fail("E_NAME_UNBOUND", format!("{class} has no method {name}"));
            }
            let fq = self.method_fq(class, cd, &name)?;
            let desc =
                self.bound(&fq)?.ok_or_else(|| CallError::Code("E_NAME_UNBOUND", format!("{fq} is not bound")))?;
            if desc.hash() != cd.methods[&name] {
                return fail("E_NAME_UNBOUND", format!("{fq} is not bound to the method's hash"));
            }
            let order = desc.order.clone().unwrap_or_default();
            let k = order.iter().zip(cd.order.iter()).take_while(|(a, b)| a == b).count();
            (fq, desc, k)
        };
        let order = desc.order.clone().unwrap_or_default();
        let mut pre = Vec::with_capacity(k);
        for n in &order[..k] {
            let (arg, _) =
                keys.get(n).ok_or_else(|| CallError::Code("E_ARG_MISSING", format!("the object has no key {n}")))?;
            pre.push((n.clone(), arg.clone()));
        }
        Ok((fq, desc, pre))
    }

    /// The object a chain's cell holds (K10, 16.6): a key encoding of one of the `declared` classes,
    /// with that class's keys, each of its type; else the fault `E_FORMULA_DISPATCH_KEY`.
    fn chained_object(&self, value: &str, declared: &[String]) -> Result<(String, BTreeMap<String, Key>), CallError> {
        let fault = |why: String| CallError::Code("E_FORMULA_DISPATCH_KEY", why);
        let obj = parse_object_key(value).map_err(|e| fault(e.to_string()))?;
        if !declared.contains(&obj.class) {
            return Err(fault(format!("a key of {}, where {declared:?} is declared", obj.class)));
        }
        let Some(cd) = self.bound_class(&obj.class)? else {
            return fail("E_NAME_UNBOUND", format!("{} is not a class of this mesh", obj.class));
        };
        if !obj.keys.keys().eq(cd.keys.keys()) {
            return Err(fault(format!("the keys of {} are not its class's", obj.class)));
        }
        self.check_encoding(&obj).map_err(|e| fault(e.to_string()))?;
        Ok((obj.class.clone(), obj.keys.into_iter().map(|(k, v)| (k, key_of(v))).collect()))
    }
}

/// A call of a method of an object (16.3, 16.7): the object from its keys, or from its label's
/// lookup; then the method's function read with the object's keys first. Returns the function's
/// name, its descriptor and the cell's arguments.
fn class_call(
    r: &mut Resolving,
    class: &str,
    cd: &ClassDescriptor,
    recv: &Recv,
    m: &Method,
) -> Result<(String, FunctionDescriptor, BTreeMap<String, Arg>), CallError> {
    let keys = receiver_keys(r, class, cd, recv)?;
    let (fq, desc, pre) = r.method_target(class, cd, &keys, m)?;
    r.resolutions.push(resolution(Resolution::new(&format!("{class}(...).{}", m.name), ResolutionKind::Method, &fq))?);
    let args = r.bind(&fq, &desc, pre, &m.args)?;
    Ok((fq, desc, args))
}

/// A chain of methods (K10, 5.8): each link is a method called on the key the one before it returns,
/// read as of the call's asof (or the mesh's now); the last method is the call. Returns the last
/// method's name, descriptor and the cell's arguments.
fn chain_call(
    r: &mut Resolving,
    class: &str,
    cd: &ClassDescriptor,
    recv: &Recv,
    methods: &[Method],
) -> Result<(String, FunctionDescriptor, BTreeMap<String, Arg>), CallError> {
    let Some((last, links)) = methods.split_last() else { return syntax("a call of a class names one method") };
    let mut here = class.to_string();
    let mut here_desc = cd.clone();
    let mut keys = receiver_keys(r, class, cd, recv)?;
    for m in links {
        let (fq, desc, pre) = r.method_target(&here, &here_desc, &keys, m)?;
        let Some(declared) = desc.returns_classes.clone() else {
            return fail("E_PARSE_URI", format!("{fq} returns no key, so no method can follow it in a chain (K10)"));
        };
        let args = r.bind(&fq, &desc, pre, &m.args)?;
        let (ns, fn_name) = fq.rsplit_once('/').unwrap_or(("", fq.as_str()));
        let cell = CellRef { ns: ns.to_string(), fn_name: fn_name.to_string(), args, asof: None };
        let Some(at) = r.instant()? else {
            return fail(
                "E_ARG_MISSING",
                format!("a chained call of {fq} is read as of the call's asof, or the mesh's now"),
            );
        };
        // A link whose cell has no value is that cell's outcome (not_covered, E_NAME_UNBOUND).
        let Some(value) = r.env.chain(&cell, &at).map_err(CallError::Env)? else {
            return fail("E_NAME_UNBOUND", format!("{fq} has no value as of {at}"));
        };
        let (next, next_keys) = r.chained_object(&value, &declared)?;
        r.resolutions.push(resolution(Resolution::chain(&m.name, &value, &at))?);
        here_desc =
            r.bound_class(&next)?.ok_or_else(|| CallError::Code("E_NAME_UNBOUND", format!("{next} is not bound")))?;
        here = next;
        keys = next_keys;
    }
    let (fq, desc, pre) = r.method_target(&here, &here_desc, &keys, last)?;
    r.resolutions.push(resolution(Resolution::new(
        &format!("{here}(...).{}", last.name),
        ResolutionKind::Method,
        &fq,
    ))?);
    let args = r.bind(&fq, &desc, pre, &last.args)?;
    Ok((fq, desc, args))
}
