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
use datumesh_core::{format_reference, lexical, Arg, Args, CellRef, FunctionDescriptor, Reference, Scalar, Type};

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

/// A store as a [`CallEnv`]: bindings and descriptors from it, no aliases (a mesh's alias map is
/// its own: wrap the store to add one). `None` as the asof is the latest binding.
#[cfg(feature = "engine")]
impl CallEnv for datumesh_store::PgStore {
    fn alias(&self, _alias: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn bound(&self, name: &str, asof: Option<&str>) -> Result<Option<FunctionDescriptor>, String> {
        store_bound(self, name, asof)
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
    fn bound(&self, name: &str, asof: Option<&str>) -> Result<Option<FunctionDescriptor>, String> {
        store_bound(self, name, asof)
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

/// A literal as written: its kind and text.
enum Lit {
    Number(String),
    Str(String),
    Date(String),
    Instant(String),
    Bool(bool),
    Open,
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

/// Parse a call (5.8) against `env`, reporting the first failure in the order 5.8 gives: the
/// text a read of literals then `?asof=` and nothing else (`E_PARSE_URI`, or a literal's code),
/// `asof` an instant, the callee (`E_NAME_UNBOUND`, `E_FN_UNAVAILABLE`), the arguments left to
/// right (`E_ARG_UNKNOWN`, `E_CANON_ARG_DUPLICATE`), the missing ones (`E_ARG_MISSING`), then
/// each value as written (`E_CANON_TYPE`, then its lexical form).
pub fn parse_call(text: &str, env: &dyn CallEnv) -> Result<Call, CallError> {
    let (read, query) = split_query(text);
    // 1. A read of literals.
    let tokens = lex::lex(read).or_else(|e| {
        let code = match e.code {
            Code::Number => "E_CANON_NUMBER",
            Code::String => "E_CANON_STRING",
            Code::Date => "E_CANON_DATE",
            Code::Instant => "E_CANON_INSTANT",
            _ => "E_PARSE_URI",
        };
        fail(code, e.to_string())
    })?;
    let (callee, args) = read_of_literals(&tokens)?;
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
    // 3. The callee.
    let (name_text, is_alias, pin) = callee;
    let name = if is_alias {
        match env.alias(&name_text).map_err(CallError::Env)? {
            Some(n) => n,
            None => return fail("E_NAME_UNBOUND", format!("@{name_text} is not an alias of this mesh")),
        }
    } else {
        name_text
    };
    let descriptor = match &pin {
        Some(h) => match env.pinned(h).map_err(CallError::Env)? {
            Some(d) => d,
            None => return fail("E_FN_UNAVAILABLE", format!("no descriptor of {h} is known")),
        },
        None => match env.bound(&name, asof.as_deref()).map_err(CallError::Env)? {
            Some(d) => d,
            None => return fail("E_NAME_UNBOUND", format!("{name} is not bound")),
        },
    };
    // 4. The arguments, left to right.
    let declared = &descriptor.signature.args;
    let mut bound: BTreeMap<String, Lit> = BTreeMap::new();
    let mut written: Vec<String> = Vec::new();
    for (i, (label, lit)) in args.into_iter().enumerate() {
        let arg = match label {
            None => match descriptor.order.as_ref().and_then(|o| o.get(i)) {
                Some(a) => a.clone(),
                None => return fail("E_ARG_UNKNOWN", format!("positional argument {} has no parameter", i + 1)),
            },
            Some(l) => {
                if !declared.contains_key(&l) {
                    return fail("E_ARG_UNKNOWN", format!("{name} has no argument {l}"));
                }
                l
            }
        };
        if bound.contains_key(&arg) {
            return fail("E_CANON_ARG_DUPLICATE", format!("{arg} is bound twice"));
        }
        written.push(arg.clone());
        bound.insert(arg, lit);
    }
    // 5. The missing ones.
    if let Some(missing) = declared.keys().find(|k| !bound.contains_key(*k)) {
        return fail("E_ARG_MISSING", format!("{missing} is not bound"));
    }
    // 6. Each value as written: its kind, then its lexical form.
    let mut cell_args = Args::new();
    for arg in written {
        let ty = declared[&arg].ty;
        let lit = &bound[&arg];
        let value = match (lit, ty) {
            (Lit::Open, _) => Arg::Slice,
            (Lit::Bool(b), Type::Boolean) => Arg::Value(Scalar::Boolean(*b)),
            (Lit::Number(t), Type::Number)
            | (Lit::Str(t), Type::String | Type::Blob)
            | (Lit::Date(t), Type::Date)
            | (Lit::Instant(t), Type::Instant) => match Scalar::from_lexical(ty, t) {
                Ok(s) => Arg::Value(s),
                Err(e) => return Err(CallError::Code(e.code_str(), e.to_string())),
            },
            _ => return fail("E_CANON_TYPE", format!("{arg} is a {}, not written as one", ty.as_str())),
        };
        cell_args.insert(arg, value);
    }
    let (ns, fn_name) = name.rsplit_once('/').unwrap_or(("", name.as_str()));
    Ok(Call { cell: CellRef { ns: ns.to_string(), fn_name: fn_name.to_string(), args: cell_args, asof }, fn_hash: pin })
}

type Callee = (String, bool, Option<String>);
/// The arguments as written: label (if any) and literal.
type Written = Vec<(Option<String>, Lit)>;

/// The tokens of a read of literals: the callee (name or alias, whether an alias, the pin) and
/// the arguments (label, literal) as written; anything else is `E_PARSE_URI`.
fn read_of_literals(tokens: &[Token]) -> Result<(Callee, Written), CallError> {
    let syntax = |what: &str| fail::<()>("E_PARSE_URI", format!("not a call: {what}"));
    let mut i = 0;
    let at = |i: usize| tokens.get(i).filter(|t| t.kind != Kind::End);
    let Some(head) = at(0).filter(|t| matches!(t.kind, Kind::Qualified | Kind::Alias)) else {
        return fail("E_PARSE_URI", "not a call: it starts with a qualified name or an alias");
    };
    i += 1;
    let pin = match at(i) {
        Some(t) if t.kind == Kind::Pin => {
            i += 1;
            Some(t.text.clone())
        }
        _ => None,
    };
    if !at(i).is_some_and(|t| t.is("(")) {
        syntax("the callee is followed by (")?;
    }
    i += 1;
    let mut args = Vec::new();
    let mut labelled = false;
    if at(i).is_some_and(|t| t.is(")")) {
        i += 1;
    } else {
        loop {
            let mut label = None;
            if at(i).is_some_and(|t| t.kind == Kind::Word) && at(i + 1).is_some_and(|t| t.is(":")) {
                label = at(i).map(|t| t.text.clone());
                labelled = true;
                i += 2;
            } else if labelled {
                syntax("a positional argument after a labelled one")?;
            }
            let lit = match at(i) {
                Some(t) if t.is("-") => match at(i + 1) {
                    Some(n) if n.kind == Kind::Number && n.unit.is_none() => {
                        i += 1;
                        Lit::Number(format!("-{}", n.text))
                    }
                    _ => return fail("E_PARSE_URI", "not a call: '-' before something other than a number"),
                },
                Some(t) if t.kind == Kind::Number && t.unit.is_none() => Lit::Number(t.text.clone()),
                Some(t) if t.kind == Kind::Str => Lit::Str(t.text.clone()),
                Some(t) if t.kind == Kind::Date => Lit::Date(t.text.clone()),
                Some(t) if t.kind == Kind::Instant => Lit::Instant(t.text.clone()),
                Some(t) if t.is_word("true") => Lit::Bool(true),
                Some(t) if t.is_word("false") => Lit::Bool(false),
                Some(t) if t.is("*") => Lit::Open,
                _ => return fail("E_PARSE_URI", "not a call: an argument is a literal"),
            };
            i += 1;
            args.push((label, lit));
            match at(i) {
                Some(t) if t.is(",") => i += 1,
                Some(t) if t.is(")") => {
                    i += 1;
                    break;
                }
                _ => syntax("arguments are literals separated by commas")?,
            }
        }
    }
    if at(i).is_some() {
        syntax("nothing but ?asof= follows the call")?;
    }
    Ok(((head.text.clone(), head.kind == Kind::Alias, pin), args))
}
