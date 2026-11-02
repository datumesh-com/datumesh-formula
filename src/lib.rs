//! The Datumesh formula dialect (SPEC section 15, spec-0.4.0): an Excel-compatible expression
//! language over cells.
//!
//! - [`define`] lexes, parses, resolves and checks a body against an [`Env`], giving the
//!   canonical AST, its hash, the printed `code.body`, the derived `inputs`, the return type and
//!   unit, and whether the function is asof-dependent. [`canonical_ast`] parses without an
//!   environment: for every canonical AST, `parse(print(ast)) = ast`.
//! - [`decimal`] is the exact decimal arithmetic of 15.9 with the 40-digit algorithms, on
//!   integers only.
//! - [`eval`] evaluates a defined function over a [`eval::World`] with fuel, faults and lineage;
//!   [`memory::MemoryWorld`] is a world in memory (the vectors' world).
//!
//! ```
//! use datumesh_core::json;
//! use datumesh_formula::{define, Env, Signature};
//!
//! let signature = json::parse(br#"{"args": {"a": {"type": "number", "unit": "EUR", "valid": false}},
//!     "order": ["a"], "returns": {"type": "number", "unit": "EUR"}}"#).unwrap();
//! let env = Env::new("calc.example.net/f", Signature::from_value(&signature).unwrap());
//! let c = define("ROUND(a * 1.2, 2)", &env).unwrap();
//! assert_eq!(c.body, "round(a * 1.2, digits: 2)\n");
//! assert!(c.inputs.is_empty());
//! assert_eq!(define("a + 1 USD", &env).unwrap_err().code.as_str(), "E_FORMULA_UNIT_MISMATCH");
//! ```
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod ast;
mod bigint;
mod builtins;
pub mod calendar;
pub mod call;
mod casing_table;
mod check;
pub mod decimal;
pub mod definition;
mod env;
mod error;
pub mod eval;
mod lex;
pub mod memory;
pub mod names;
mod parse;
mod resolve;
#[cfg(feature = "engine")]
pub mod runtime;
pub mod text;
pub mod units;

use datumesh_core::json::Value;
use datumesh_core::Type;

pub use ast::{Ast, Node, NodeId, Op, ReadArg};
pub use check::{KeyTy, NumUnit, Ty};
pub use env::{ArgDecl, Env, EnvError, Extent, Function, Signature, UnitDecl, UnitSide};
pub use error::{Code, DefineError, Error};
pub use parse::{BUILTINS as BUILTIN_NAMES, FORBIDDEN as FORBIDDEN_NAMES, MAX_NESTING};
pub use resolve::MAX_NODES;

/// The type and unit of a body (the vectors' `returns`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Returns {
    /// The type.
    pub ty: Type,
    /// For numbers, the unit's text: atoms ascending with exponents (`arg:base^-1 arg:quote`),
    /// `1`, or `a bare number` for an unsuffixed literal.
    pub unit: Option<String>,
}

impl Returns {
    /// `{"type": ..., "unit"?: ...}`.
    pub fn to_value(&self) -> Value {
        let mut m = vec![("type".to_string(), Value::String(self.ty.as_str().to_string()))];
        if let Some(u) = &self.unit {
            m.push(("unit".to_string(), Value::String(u.clone())));
        }
        Value::Object(m)
    }
}

/// A defined formula: the canonical AST and what define derives from it.
#[derive(Clone, Debug)]
pub struct Compiled {
    /// The canonical AST.
    pub ast: Ast,
    /// `code.body`: the printed AST and one LF.
    pub body: String,
    /// The derived `inputs`: every fully qualified name the body reads, ascending.
    pub inputs: Vec<String>,
    /// The body's type and unit.
    pub returns: Returns,
    /// Whether the body reads `asof()` or uses `latest` with a bound (15.12).
    pub asof_dependent: bool,
    /// The checker's type of every node (indexed by node id), for the evaluator.
    pub(crate) types: Vec<Option<Ty>>,
}

impl Compiled {
    /// Whether two compilations of one body give every node the same type and unit.
    pub fn types_eq(&self, other: &Compiled) -> bool {
        self.types == other.types
    }

    /// The checker's type of a node, when it has one.
    pub fn type_of(&self, id: NodeId) -> Option<&Ty> {
        self.types.get(id as usize).and_then(Option::as_ref)
    }
}

/// Parse and resolve `source` to its canonical AST. With an environment, aliases resolve and
/// positional read arguments bind by `signature.order`; without one, every read must be
/// labelled, which a printed body always is (`parse(print(ast)) = ast`, 15.6).
pub fn canonical_ast(source: &str, env: Option<&Env>) -> Result<Ast, Error> {
    let tokens = lex::lex(source)?;
    deep(tokens.len(), || {
        let tree = parse::parse(&tokens)?;
        resolve::resolve(&tree, env)
    })
}

/// The fully qualified names `source` reads (aliases resolved through `aliases`; an unknown
/// alias is left for resolution to report), after lexing and parsing it.
pub(crate) fn read_names(
    source: &str,
    aliases: &std::collections::BTreeMap<String, String>,
) -> Result<std::collections::BTreeSet<String>, Error> {
    let tokens = lex::lex(source)?;
    let tree = deep(tokens.len(), || parse::parse(&tokens))?;
    let mut names = std::collections::BTreeSet::new();
    for node in &tree.nodes {
        if let parse::PNode::Read { name, alias, .. } = node {
            if !*alias {
                names.insert(name.clone());
            } else if let Some(n) = aliases.get(name) {
                names.insert(n.clone());
            }
        }
    }
    Ok(names)
}

/// Run `f`, on a thread with a large stack when the body is long enough to nest deeply: the
/// recursive descent at 256 levels needs more than the 2 MiB of a default thread in a debug
/// build. Where threads cannot be spawned, `f` runs on the caller's stack.
fn deep<T: Send>(tokens: usize, f: impl FnOnce() -> T + Send) -> T {
    const LONG: usize = 128;
    const STACK: usize = 64 << 20;
    if tokens <= LONG {
        return f();
    }
    let mut f = Some(f);
    let spawned = std::thread::scope(|s| {
        let run = || f.take().map(|f| f());
        match std::thread::Builder::new().stack_size(STACK).spawn_scoped(s, run) {
            Ok(h) => Some(h.join()),
            Err(_) => None,
        }
    });
    match spawned {
        Some(Ok(Some(v))) => v,
        // A panic on the deep thread is the caller's panic.
        Some(Err(payload)) => std::panic::resume_unwind(payload),
        // No thread (a platform without them): the caller's stack.
        _ => match f.take() {
            Some(f) => f(),
            None => unreachable!("the closure ran on the deep thread"),
        },
    }
}

/// Check a formula descriptor (15.6), in this order: its canonical form of section 7
/// ([`DefineError::Descriptor`], with the core's code), `signature.order` (`E_FORMULA_ORDER`),
/// its body defined against `env`'s functions and aliases with the descriptor's own signature
/// (the body's codes), the body the print of its canonical AST followed by one LF
/// (`E_FORMULA_NOT_CANONICAL`), and exactly the derived inputs (`E_FORMULA_INPUTS`). A
/// descriptor of another language is `E_FORMULA_SYNTAX`.
pub fn check_descriptor(name: &str, descriptor: &Value, env: &Env) -> Result<Compiled, DefineError> {
    datumesh_core::FunctionDescriptor::from_value(descriptor).map_err(DefineError::Descriptor)?;
    let f = Function::from_value(descriptor).map_err(|e| Error::new(Code::Syntax, e.0))?;
    if f.lang != "formula" {
        return Err(Error::new(Code::Syntax, format!("code.lang is {}, not formula", f.lang)).into());
    }
    if f.signature.order.is_none() {
        return Err(Error::new(Code::Order, "a formula descriptor declares signature.order").into());
    }
    let mut env = env.clone();
    env.name = name.to_string();
    env.signature = f.signature.clone();
    let c = define(&f.body, &env)?;
    if c.body != f.body {
        return Err(
            Error::new(Code::NotCanonical, format!("the body is not the print of its AST: {:?}", c.body)).into()
        );
    }
    if c.inputs != f.inputs {
        return Err(Error::new(Code::Inputs, format!("inputs are {:?}, derived {:?}", f.inputs, c.inputs)).into());
    }
    Ok(c)
}

/// Define a formula: lex, parse, resolve and check `source` against `env` (15.2-15.10), reporting
/// the first failure with its code (15.11).
pub fn define(source: &str, env: &Env) -> Result<Compiled, Error> {
    let tokens = lex::lex(source)?;
    let (ast, checked) = deep(tokens.len(), || -> Result<(Ast, check::Checked), Error> {
        let tree = parse::parse(&tokens)?;
        let ast = resolve::resolve(&tree, Some(env))?;
        let checked = check::check(&ast, env)?;
        Ok((ast, checked))
    })?;
    let returns = Returns {
        ty: match &checked.ty {
            Ty::Num(_) => Type::Number,
            Ty::Str => Type::String,
            Ty::Bool => Type::Boolean,
            Ty::Date => Type::Date,
            Ty::Instant => Type::Instant,
            Ty::Blob | Ty::Slice(..) => Type::Blob,
        },
        unit: match &checked.ty {
            Ty::Num(u) => Some(u.text()),
            _ => None,
        },
    };
    Ok(Compiled {
        body: ast.body(),
        ast,
        inputs: checked.inputs,
        returns,
        asof_dependent: checked.asof_dependent,
        types: checked.types,
    })
}
