//! The Datumesh formula dialect (SPEC section 15, spec-0.4.0): an Excel-compatible expression
//! language over cells.
//!
//! - [`define`] lexes, parses, resolves and checks a body against an [`Env`], giving the
//!   canonical AST, its hash, the printed `code.body`, the derived `inputs`, the return type and
//!   unit, and whether the function is asof-dependent.
//! - [`decimal`] is the exact decimal arithmetic of 15.9 with the 40-digit algorithms.
//! - [`eval`] evaluates a defined function over a [`eval::World`] with fuel and lineage.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod ast;
mod bigint;
mod builtins;
pub mod calendar;
mod check;
pub mod decimal;
mod env;
mod error;
mod lex;
pub mod names;
mod parse;
mod resolve;
pub mod units;

use datumesh_core::json::Value;
use datumesh_core::Type;

pub use ast::{Ast, Node, NodeId, Op, ReadArg};
pub use check::{KeyTy, NumUnit, Ty};
pub use env::{ArgDecl, Env, EnvError, Extent, Function, Signature, UnitDecl, UnitSide};
pub use error::{Code, Error};
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
    let tree = parse::parse(&tokens)?;
    resolve::resolve(&tree, env)
}

/// Define a formula: lex, parse, resolve and check `source` against `env` (15.2-15.10), reporting
/// the first failure with its code (15.11).
pub fn define(source: &str, env: &Env) -> Result<Compiled, Error> {
    let ast = canonical_ast(source, Some(env))?;
    let checked = check::check(&ast, env)?;
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
