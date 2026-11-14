//! Function descriptors and signatures as the formula dialect reads them (SPEC 7, 7.1), and the
//! environment of a definition: the name being defined, its signature, the functions it may
//! read and the resolver's alias map.
//!
//! Descriptors are taken as given: they come from a store or a vector already checked under
//! section 7. Only what the dialect needs is read; a shape it cannot read is an [`EnvError`].

use std::collections::BTreeMap;
use std::fmt;

use datumesh_core::json::Value;
use datumesh_core::{ArgExtras, Type};

use crate::classes::Classes;

/// A malformed environment, descriptor or world: the caller's error, not a formula code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvError(pub String);

impl fmt::Display for EnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "malformed input: {}", self.0)
    }
}

impl std::error::Error for EnvError {}

fn bad(s: impl Into<String>) -> EnvError {
    EnvError(s.into())
}

/// One side of a unit declaration (7.1): a literal or an argument reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitSide {
    /// A literal unit.
    Literal(String),
    /// `{"arg": name}`.
    Arg(String),
}

/// A declared unit (7.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitDecl {
    /// A literal or an argument reference.
    Simple(UnitSide),
    /// `{"of": X, "per": Y}`.
    Rate(UnitSide, UnitSide),
}

/// An argument's extent (7): listed values or an integer/date range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Extent {
    /// Lexical values, ascending.
    Values(Vec<String>),
    /// `{"hi", "lo"}`, inclusive.
    Range(String, String),
}

/// An argument declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArgDecl {
    /// Its type.
    pub ty: Type,
    /// Its unit, for numbers.
    pub unit: Option<UnitDecl>,
    /// Whether it is a valid-time argument.
    pub valid: bool,
    /// Its extent, if declared.
    pub extent: Option<Extent>,
    /// Since spec-0.5.0 (7): `classes` (a string argument holding the key of an object of one of
    /// them, 16.6), `default: "asof"` (16.7) and `key_normalisation` (16.4), core's
    /// [`ArgExtras`]; absent members are the defaults.
    pub extras: ArgExtras,
}

/// A signature (7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    /// The arguments by name.
    pub args: BTreeMap<String, ArgDecl>,
    /// `order`, if present.
    pub order: Option<Vec<String>>,
    /// The return type.
    pub returns: Type,
    /// The return unit, for numbers.
    pub returns_unit: Option<UnitDecl>,
    /// Since spec-0.5.0 (7): `returns` is `{"type": "string", "classes": [...]}`, a returned
    /// object key (16.6): the classes.
    pub returns_classes: Option<Vec<String>>,
}

/// A function descriptor: language, body, inputs, signature, and its function hash.
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    /// `code.lang`.
    pub lang: String,
    /// `code.body`.
    pub body: String,
    /// `inputs`.
    pub inputs: Vec<String>,
    /// The signature.
    pub signature: Signature,
    /// SHA-256 of the descriptor's canonical bytes.
    pub hash: String,
    /// The descriptor as given.
    pub value: Value,
}

fn type_of(v: &Value, what: &str) -> Result<Type, EnvError> {
    let s = v.as_str().ok_or_else(|| bad(format!("{what}: type is not a string")))?;
    Type::from_name(s).ok_or_else(|| bad(format!("{what}: unknown type {s}")))
}

fn side(v: &Value) -> Result<UnitSide, EnvError> {
    match v {
        Value::String(s) => Ok(UnitSide::Literal(s.clone())),
        Value::Object(_) => {
            let a = v.get("arg").and_then(Value::as_str).ok_or_else(|| bad("unit reference without arg"))?;
            Ok(UnitSide::Arg(a.to_string()))
        }
        _ => Err(bad("unit is not a string or object")),
    }
}

fn unit_decl(v: &Value) -> Result<UnitDecl, EnvError> {
    if let (Some(of), Some(per)) = (v.get("of"), v.get("per")) {
        return Ok(UnitDecl::Rate(side(of)?, side(per)?));
    }
    Ok(UnitDecl::Simple(side(v)?))
}

impl Signature {
    /// Read a signature object.
    pub fn from_value(v: &Value) -> Result<Signature, EnvError> {
        let args_v = v.get("args").and_then(Value::as_object).ok_or_else(|| bad("signature without args"))?;
        let mut args = BTreeMap::new();
        for (name, d) in args_v {
            let ty = type_of(d.get("type").ok_or_else(|| bad(format!("argument {name} without type")))?, name)?;
            let unit = match d.get("unit") {
                Some(u) => Some(unit_decl(u)?),
                None => None,
            };
            let valid = matches!(d.get("valid"), Some(Value::Bool(true)));
            let extent = match d.get("extent") {
                None => None,
                Some(Value::Array(a)) => Some(Extent::Values(
                    a.iter()
                        .map(|x| x.as_str().map(str::to_string).ok_or_else(|| bad("extent value")))
                        .collect::<Result<_, _>>()?,
                )),
                Some(o @ Value::Object(_)) => {
                    let lo = o.get("lo").and_then(Value::as_str).ok_or_else(|| bad("extent lo"))?;
                    let hi = o.get("hi").and_then(Value::as_str).ok_or_else(|| bad("extent hi"))?;
                    Some(Extent::Range(lo.to_string(), hi.to_string()))
                }
                Some(_) => return Err(bad("extent shape")),
            };
            args.insert(name.clone(), ArgDecl { ty, unit, valid, extent, extras: ArgExtras::default() });
        }
        let order = match v.get("order") {
            None => None,
            Some(Value::Array(a)) => Some(
                a.iter()
                    .map(|x| x.as_str().map(str::to_string).ok_or_else(|| bad("order entry")))
                    .collect::<Result<_, _>>()?,
            ),
            Some(_) => return Err(bad("order is not an array")),
        };
        let r = v.get("returns").ok_or_else(|| bad("signature without returns"))?;
        let (returns, returns_unit) = match r {
            Value::String(_) => (type_of(r, "returns")?, None),
            Value::Object(_) => {
                let t = type_of(r.get("type").ok_or_else(|| bad("returns without type"))?, "returns")?;
                let u = match r.get("unit") {
                    Some(u) => Some(unit_decl(u)?),
                    None => None,
                };
                (t, u)
            }
            _ => return Err(bad("returns shape")),
        };
        // The members of spec-0.5.0 (`classes`, `default`, `key_normalisation`, a returned key's
        // `classes`) are read by core's function descriptor, over the signature alone.
        let wrapped = Value::Object(vec![
            (
                "code".to_string(),
                Value::Object(vec![
                    ("body".to_string(), Value::String(String::new())),
                    ("lang".to_string(), Value::String("stored".to_string())),
                ]),
            ),
            ("inputs".to_string(), Value::Array(Vec::new())),
            ("signature".to_string(), v.clone()),
        ]);
        // An environment's signature is trusted as the caller gives it: one core would refuse
        // (a descriptor's own checks are `define`'s) has the members absent.
        let (extras, returns_classes) = match datumesh_core::FunctionDescriptor::from_value(&wrapped) {
            Ok(fd) => (fd.arg_extras.clone(), fd.returns_classes.clone()),
            Err(_) => (Default::default(), None),
        };
        for (name, decl) in args.iter_mut() {
            decl.extras = extras.get(name).cloned().unwrap_or_default();
        }
        Ok(Signature { args, order, returns, returns_unit, returns_classes })
    }
}

impl Function {
    /// Read a descriptor `{"code", "inputs", "signature"}`.
    pub fn from_value(v: &Value) -> Result<Function, EnvError> {
        let code = v.get("code").ok_or_else(|| bad("descriptor without code"))?;
        let lang = code.get("lang").and_then(Value::as_str).ok_or_else(|| bad("code without lang"))?;
        let body = code.get("body").and_then(Value::as_str).ok_or_else(|| bad("code without body"))?;
        let inputs = match v.get("inputs") {
            Some(Value::Array(a)) => a
                .iter()
                .map(|x| x.as_str().map(str::to_string).ok_or_else(|| bad("inputs entry")))
                .collect::<Result<_, _>>()?,
            _ => return Err(bad("descriptor without inputs")),
        };
        let signature = Signature::from_value(v.get("signature").ok_or_else(|| bad("descriptor without signature"))?)?;
        Ok(Function {
            lang: lang.to_string(),
            body: body.to_string(),
            inputs,
            signature,
            hash: datumesh_core::sha256_hex(&v.to_jcs()),
            value: v.clone(),
        })
    }
}

/// The environment of a definition (the `env` of a `formula` vector, SPEC 11).
#[derive(Clone, Debug, PartialEq)]
pub struct Env {
    /// The fully qualified name of the function being defined.
    pub name: String,
    /// Its signature.
    pub signature: Signature,
    /// The functions it may read, by fully qualified name.
    pub functions: BTreeMap<String, Function>,
    /// The resolver's alias map: alias (without `@`) to fully qualified name.
    pub aliases: BTreeMap<String, String>,
    /// Since spec-0.5.0 (16): the classes a body may read as objects, by class name. The
    /// methods' functions are in `functions` as `<class>/<method>`.
    pub classes: Classes,
    /// Since spec-0.5.0 (16.3): the class whose method the function being defined is, which
    /// gives `this` and `super` their meaning; `None` outside a method's body.
    pub method_of: Option<String>,
    /// Whether the body is a stored one being evaluated (`check_descriptor`): a pinned read is
    /// accepted there, as the printed form of `super` (16.3).
    pub(crate) stored_body: bool,
    /// Whether the body is checked again at evaluation, against the bindings an evaluation reads
    /// (15.8). There a dispatch's method bound to another hash is not a define-time failure: the
    /// dispatch's read gives `E_FORMULA_DISPATCH` (16.6).
    pub(crate) evaluating: bool,
    /// Since spec-0.5.0 (16.3): the functions a body pins its reads to (`super`), by function
    /// hash, whatever name they are bound to now. The runtime adds the ones a body names.
    pub(crate) pinned: BTreeMap<String, Function>,
}

impl Env {
    /// An environment with no functions and no aliases.
    pub fn new(name: &str, signature: Signature) -> Env {
        Env {
            name: name.to_string(),
            signature,
            functions: BTreeMap::new(),
            aliases: BTreeMap::new(),
            classes: Classes::new(),
            method_of: None,
            stored_body: false,
            evaluating: false,
            pinned: BTreeMap::new(),
        }
    }

    /// Read `{"name", "signature", "functions": {name: descriptor}, "aliases": {alias: name}}` and,
    /// since spec-0.5.0, `classes` (class name to flattened descriptor) and `method_of`.
    pub fn from_value(v: &Value) -> Result<Env, EnvError> {
        let name = v.get("name").and_then(Value::as_str).ok_or_else(|| bad("env without name"))?;
        let signature = Signature::from_value(v.get("signature").ok_or_else(|| bad("env without signature"))?)?;
        let mut env = Env::new(name, signature);
        if let Some(fs) = v.get("functions").and_then(Value::as_object) {
            for (n, d) in fs {
                env.add_function(n, d)?;
            }
        }
        if let Some(cs) = v.get("classes") {
            env.classes = Classes::from_value(cs).map_err(|e| bad(e.0))?;
        }
        if let Some(m) = v.get("method_of") {
            env.method_of = Some(m.as_str().ok_or_else(|| bad("method_of is not a string"))?.to_string());
        }
        if let Some(al) = v.get("aliases").and_then(Value::as_object) {
            for (a, n) in al {
                let n = n.as_str().ok_or_else(|| bad("alias target"))?;
                env.aliases.insert(a.clone(), n.to_string());
            }
        }
        Ok(env)
    }

    /// Add (or replace) a function the body may read.
    pub fn add_function(&mut self, name: &str, descriptor: &Value) -> Result<(), EnvError> {
        self.functions.insert(name.to_string(), Function::from_value(descriptor)?);
        Ok(())
    }

    /// The signature a read of `name` binds against: the function being defined, or one of
    /// the environment's functions.
    pub fn signature_of(&self, name: &str) -> Option<&Signature> {
        if name == self.name {
            return Some(&self.signature);
        }
        self.functions.get(name).map(|f| &f.signature)
    }

    /// The language of `name` (`formula` for the function being defined).
    pub fn lang_of(&self, name: &str) -> Option<&str> {
        if name == self.name {
            return Some("formula");
        }
        self.functions.get(name).map(|f| f.lang.as_str())
    }
}
