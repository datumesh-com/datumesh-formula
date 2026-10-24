//! The checker (SPEC 15.8, 15.10, 15.13): types, units, scope, lambdas, extents, joins,
//! lookups and transaction-time reads, depth first and left to right, first failure reported.
//! It also derives the inputs, asof-dependence and the unit of every number node for the
//! evaluator.

use std::collections::BTreeSet;

use datumesh_core::Type;

use crate::ast::{Ast, Node, NodeId, Op, ReadArg};
use crate::builtins::{AGGREGATES, LOOKUPS, METHODS, ROLLS, WEEKENDS};
use crate::calendar::{self, CONVENTIONS};
use crate::decimal::Decimal;
use crate::env::{ArgDecl, Env, Extent, Signature, UnitDecl, UnitSide};
use crate::units::{self, Unit};
use crate::{Code, Error};

/// The unit of a number in the checker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NumUnit {
    /// A unit (`1` is the empty unit).
    Known(Unit),
    /// A literal without a suffix (or arithmetic on such literals): dimensionless in `*`, `/`
    /// and `^`, its context's unit elsewhere. `zero` when it is the literal 0.
    Bare {
        /// Whether it is the literal 0.
        zero: bool,
    },
    /// Varies by slice member (a unit referring to an open argument).
    Varies,
}

impl NumUnit {
    fn known(u: Unit) -> NumUnit {
        NumUnit::Known(u)
    }

    fn atom(a: &str) -> NumUnit {
        NumUnit::Known(Unit::atom(a))
    }

    /// The text of the vectors' `returns.unit`.
    pub fn text(&self) -> String {
        match self {
            NumUnit::Known(u) => u.text(),
            NumUnit::Bare { .. } => "a bare number".to_string(),
            NumUnit::Varies => "a varying unit".to_string(),
        }
    }

    fn is_bare(&self) -> bool {
        matches!(self, NumUnit::Bare { .. })
    }

    fn single(&self) -> Option<&str> {
        match self {
            NumUnit::Known(u) => u.single(),
            _ => None,
        }
    }

    fn is_degc(&self) -> bool {
        self.single() == Some("degC")
    }
}

/// A key of a slice: an open argument's name, type, unit (numbers) and valid-time flag.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyTy {
    /// The argument name.
    pub name: String,
    /// Its type.
    pub ty: Type,
    /// Its unit, for numbers.
    pub unit: Option<NumUnit>,
    /// Whether it is a valid-time argument.
    pub valid: bool,
}

/// A type of the checker: the six scalars, and slices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ty {
    /// A number and its unit.
    Num(NumUnit),
    /// A string.
    Str,
    /// A boolean.
    Bool,
    /// A date.
    Date,
    /// An instant.
    Instant,
    /// A blob.
    Blob,
    /// A slice: member type and keys (ascending by name).
    Slice(Box<Ty>, Vec<KeyTy>),
}

impl Ty {
    fn name(&self) -> &'static str {
        match self {
            Ty::Num(_) => "number",
            Ty::Str => "string",
            Ty::Bool => "boolean",
            Ty::Date => "date",
            Ty::Instant => "instant",
            Ty::Blob => "blob",
            Ty::Slice(..) => "slice",
        }
    }

    fn scalar_type(&self) -> Option<Type> {
        Some(match self {
            Ty::Num(_) => Type::Number,
            Ty::Str => Type::String,
            Ty::Bool => Type::Boolean,
            Ty::Date => Type::Date,
            Ty::Instant => Type::Instant,
            Ty::Blob => Type::Blob,
            Ty::Slice(..) => return None,
        })
    }

    fn is_slice(&self) -> bool {
        matches!(self, Ty::Slice(..))
    }
}

/// The checked body: its type, the derived inputs, asof-dependence, and every node's type.
#[derive(Clone, Debug)]
pub(crate) struct Checked {
    pub(crate) ty: Ty,
    pub(crate) inputs: Vec<String>,
    pub(crate) asof_dependent: bool,
    pub(crate) types: Vec<Option<Ty>>,
}

fn e(code: Code, detail: impl Into<String>) -> Error {
    Error::new(code, detail)
}

/// Map a declared unit side to a checker unit with argument references replaced by `subst`.
fn side_unit(side: &UnitSide, subst: &dyn Fn(&str) -> Result<NumUnit, Error>) -> Result<NumUnit, Error> {
    match side {
        UnitSide::Literal(l) => Ok(NumUnit::atom(l)),
        UnitSide::Arg(a) => subst(a),
    }
}

/// A declared unit (7.1) with argument references replaced by `subst`.
pub(crate) fn decl_unit(d: &UnitDecl, subst: &dyn Fn(&str) -> Result<NumUnit, Error>) -> Result<NumUnit, Error> {
    match d {
        UnitDecl::Simple(s) => side_unit(s, subst),
        UnitDecl::Rate(of, per) => {
            let a = side_unit(of, subst)?;
            let b = side_unit(per, subst)?;
            match (a, b) {
                (NumUnit::Known(x), NumUnit::Known(y)) => Ok(NumUnit::Known(x.mul(&y, -1))),
                _ => Ok(NumUnit::Varies),
            }
        }
    }
}

/// A declared unit with references to the defining function's own arguments as `arg:<name>`.
pub(crate) fn own_unit(d: &UnitDecl) -> NumUnit {
    decl_unit(d, &|a| Ok(NumUnit::atom(&format!("arg:{a}")))).unwrap_or(NumUnit::Varies)
}

fn ratio_like(u: &NumUnit) -> bool {
    matches!(u.single(), Some("percent") | Some("permille") | Some("bp"))
}

/// Unify a bare literal with a unit in a context that needs equal units (15.10): the unit, or
/// `E_FORMULA_UNIT_MISMATCH` when only 0 is accepted.
fn adopt(bare_zero: bool, u: &NumUnit) -> Result<NumUnit, Error> {
    if ratio_like(u) && !bare_zero {
        return Err(e(
            Code::UnitMismatch,
            format!("a bare number cannot be {}: write the unit (e.g. 1 {})", u.text(), u.text()),
        ));
    }
    Ok(u.clone())
}

/// Equal units, a bare literal adopting the other side's.
fn same_unit(a: &NumUnit, b: &NumUnit, what: &str) -> Result<NumUnit, Error> {
    match (a, b) {
        (NumUnit::Varies, _) | (_, NumUnit::Varies) => {
            Err(e(Code::UnitUnknown, format!("{what}: the unit varies by member")))
        }
        (NumUnit::Bare { zero: z1 }, NumUnit::Bare { zero: z2 }) => Ok(NumUnit::Bare { zero: *z1 && *z2 }),
        (NumUnit::Bare { zero }, u) | (u, NumUnit::Bare { zero }) => adopt(*zero, u),
        (NumUnit::Known(x), NumUnit::Known(y)) => {
            if x == y {
                Ok(a.clone())
            } else {
                Err(e(Code::UnitMismatch, format!("{what}: {} and {}", a.text(), b.text())))
            }
        }
    }
}

/// The scope: names and their types, innermost last.
struct Scope {
    names: Vec<(String, Ty, Option<UnitSource>)>,
}

/// What a string name can stand for in a unit substitution (15.10).
#[derive(Clone, Debug, PartialEq, Eq)]
enum UnitSource {
    /// A string argument of the function being defined.
    Arg(String),
    /// A listed unit literal.
    Literal(String),
}

impl Scope {
    fn get(&self, n: &str) -> Option<&(String, Ty, Option<UnitSource>)> {
        self.names.iter().rev().find(|(m, _, _)| m == n)
    }
    fn has(&self, n: &str) -> bool {
        self.get(n).is_some()
    }
}

struct Checker<'a> {
    ast: &'a Ast,
    env: &'a Env,
    scope: Scope,
    inputs: BTreeSet<String>,
    asof_dependent: bool,
    in_lambda: usize,
    types: Vec<Option<Ty>>,
}

/// Check a resolved body against its environment.
pub(crate) fn check(ast: &Ast, env: &Env) -> Result<Checked, Error> {
    // The function's own arguments: none may be a keyword or reserved word (15.2.1).
    for name in env.signature.args.keys() {
        if crate::lex::is_keyword(name) || name == "this" || name == "super" {
            return Err(e(Code::Reserved, format!("{name} cannot be an argument name of a formula function")));
        }
        if name == "asof" {
            return Err(e(Code::Reserved, "asof cannot be an argument name"));
        }
    }
    let mut names = Vec::new();
    for (name, d) in &env.signature.args {
        let ty = arg_ty(d, &|a| Ok(NumUnit::atom(&format!("arg:{a}"))))?;
        let src = if d.ty == Type::String { Some(UnitSource::Arg(name.clone())) } else { None };
        names.push((name.clone(), ty, src));
    }
    let mut c = Checker {
        ast,
        env,
        scope: Scope { names },
        inputs: BTreeSet::new(),
        asof_dependent: false,
        in_lambda: 0,
        types: vec![None; ast.nodes.len()],
    };
    let ty = c.node(ast.root)?;
    c.returns(&ty, &env.signature)?;
    Ok(Checked { ty, inputs: c.inputs.into_iter().collect(), asof_dependent: c.asof_dependent, types: c.types })
}

/// The checker type of an argument declaration.
fn arg_ty(d: &ArgDecl, subst: &dyn Fn(&str) -> Result<NumUnit, Error>) -> Result<Ty, Error> {
    Ok(match d.ty {
        Type::Number => Ty::Num(match &d.unit {
            Some(u) => decl_unit(u, subst)?,
            None => NumUnit::Known(Unit::one()),
        }),
        t => scalar(t),
    })
}

fn scalar(t: Type) -> Ty {
    match t {
        Type::Number => Ty::Num(NumUnit::Known(Unit::one())),
        Type::String => Ty::Str,
        Type::Boolean => Ty::Bool,
        Type::Date => Ty::Date,
        Type::Instant => Ty::Instant,
        Type::Blob => Ty::Blob,
    }
}

/// The literal value of a numeric node, if it is a number literal or a minus applied to one.
pub(crate) fn literal_number(ast: &Ast, id: NodeId) -> Option<Decimal> {
    match ast.node(id) {
        Node::Num { value, .. } => Decimal::parse(value),
        Node::Op { op: Op::Neg, args } => literal_number(ast, args[0]).map(|d| d.neg()),
        _ => None,
    }
}

fn str_literal(ast: &Ast, id: NodeId) -> Option<&str> {
    match ast.node(id) {
        Node::Str(s) => Some(s),
        _ => None,
    }
}

impl Checker<'_> {
    fn node(&mut self, id: NodeId) -> Result<Ty, Error> {
        // Binary operator spines are checked iteratively (a chain of 65,536 nodes is legal).
        if let Node::Op { op, args } = self.ast.node(id) {
            if args.len() == 2 && *op != Op::Pow {
                let mut spine = Vec::new();
                let mut cur = id;
                while let Node::Op { op, args } = self.ast.node(cur) {
                    if args.len() != 2 || *op == Op::Pow {
                        break;
                    }
                    spine.push((cur, *op, args[1]));
                    cur = args[0];
                }
                let mut acc = self.node(cur)?;
                for (nid, op, r) in spine.into_iter().rev() {
                    if matches!(op, Op::And | Op::Or) {
                        if acc.is_slice() {
                            return Err(e(Code::SliceValue, "a slice where a scalar is required"));
                        }
                        if acc != Ty::Bool {
                            return Err(e(
                                Code::Type,
                                format!("{} {} ...: and and or take booleans", acc.name(), op.text()),
                            ));
                        }
                    }
                    let rt = self.node(r)?;
                    acc = self.binary(op, &acc, &rt)?;
                    self.types[nid as usize] = Some(acc.clone());
                }
                return Ok(acc);
            }
        }
        let t = self.node_inner(id)?;
        self.types[id as usize] = Some(t.clone());
        Ok(t)
    }

    fn scalar_of(&mut self, id: NodeId) -> Result<Ty, Error> {
        let t = self.node(id)?;
        if t.is_slice() {
            return Err(e(Code::SliceValue, "a slice where a scalar is required"));
        }
        Ok(t)
    }

    fn node_inner(&mut self, id: NodeId) -> Result<Ty, Error> {
        let ast = self.ast;
        match ast.node(id) {
            Node::Num { value, unit } => Ok(Ty::Num(match unit {
                Some(u) => NumUnit::atom(u),
                None => NumUnit::Bare { zero: value == "0" },
            })),
            Node::Str(_) => Ok(Ty::Str),
            Node::Bool(_) => Ok(Ty::Bool),
            Node::Date(_) => Ok(Ty::Date),
            Node::Instant(_) => Ok(Ty::Instant),
            Node::Var(n) => match self.scope.get(n) {
                Some((_, t, _)) => Ok(t.clone()),
                None => Err(e(Code::UnknownName, format!("{n} is not an argument, let or lambda name"))),
            },
            Node::Op { op, args } => match op {
                Op::Neg => {
                    // A chain of minus signs, iteratively.
                    let mut chain = vec![id];
                    let mut cur = args[0];
                    while let Node::Op { op: Op::Neg, args } = ast.node(cur) {
                        chain.push(cur);
                        cur = args[0];
                    }
                    let t = self.node(cur)?;
                    match t {
                        Ty::Num(_) => {}
                        Ty::Slice(ref elem, _) if matches!(**elem, Ty::Num(_)) => {}
                        _ => return Err(e(Code::Type, format!("-{}", t.name()))),
                    }
                    for n in chain {
                        self.types[n as usize] = Some(t.clone());
                    }
                    Ok(t)
                }
                Op::Not => {
                    let t = self.scalar_of(args[0])?;
                    if t != Ty::Bool {
                        return Err(e(Code::Type, format!("not {}", t.name())));
                    }
                    Ok(Ty::Bool)
                }
                Op::Pow => {
                    let b = self.node(args[0])?;
                    let x = self.node(args[1])?;
                    self.power(&b, &x, args[1])
                }
                _ => {
                    let l = self.node(args[0])?;
                    let r = self.node(args[1])?;
                    self.binary(*op, &l, &r)
                }
            },
            Node::If { cond, then, els } => {
                let c = self.scalar_of(*cond)?;
                if c != Ty::Bool {
                    return Err(e(Code::Type, format!("the condition of if is {}, not boolean", c.name())));
                }
                let a = self.scalar_of(*then)?;
                let b = self.scalar_of(*els)?;
                self.unify(&a, &b, "the branches of if")
            }
            Node::Let { name, value, body } => {
                if self.scope.has(name) {
                    return Err(e(Code::Shadow, format!("{name} is already a name here")));
                }
                let v = self.node(*value)?;
                let src = match ast.node(*value) {
                    Node::Str(s) if units::is_unit(s) => Some(UnitSource::Literal(s.clone())),
                    Node::Var(n) => self.scope.get(n).and_then(|(_, _, s)| s.clone()),
                    _ => None,
                };
                self.scope.names.push((name.clone(), v, src));
                let r = self.node(*body);
                self.scope.names.pop();
                r
            }
            Node::Lambda { .. } => Err(e(Code::Placement, "a lambda is only an argument of map or filter")),
            Node::Agg(n) => Err(e(Code::Enum, format!("{n} is an aggregate of scan or window only"))),
            Node::Key(n) => Err(e(Code::Along, format!("{n} names a dimension only in along"))),
            Node::Read { .. } => self.read(id),
            Node::Call { fn_name, args, named } => self.call(id, fn_name, args, named),
        }
    }

    fn unify(&self, a: &Ty, b: &Ty, what: &str) -> Result<Ty, Error> {
        match (a, b) {
            (Ty::Num(x), Ty::Num(y)) => Ok(Ty::Num(same_unit(x, y, what)?)),
            _ if a == b => Ok(a.clone()),
            _ => Err(e(Code::Type, format!("{what}: {} and {}", a.name(), b.name()))),
        }
    }

    // ------------------------------------------------------------------------------------
    // Operators.

    fn binary(&mut self, op: Op, l: &Ty, r: &Ty) -> Result<Ty, Error> {
        match op {
            Op::And | Op::Or => {
                if l.is_slice() || r.is_slice() {
                    return Err(e(Code::SliceValue, "a slice where a scalar is required"));
                }
                if *l != Ty::Bool || *r != Ty::Bool {
                    return Err(e(Code::Type, format!("{} {} {}", l.name(), op.text(), r.name())));
                }
                Ok(Ty::Bool)
            }
            Op::Cat => {
                if l.is_slice() || r.is_slice() {
                    return Err(e(Code::SliceValue, "a slice where a scalar is required"));
                }
                if *l != Ty::Str || *r != Ty::Str {
                    return Err(e(Code::Type, format!("{} & {}: & joins strings", l.name(), r.name())));
                }
                Ok(Ty::Str)
            }
            o if o.is_comparison() => {
                if l.is_slice() || r.is_slice() {
                    return Err(e(Code::SliceValue, "a slice where a scalar is required"));
                }
                self.compare(o, l, r)
            }
            _ => {
                // + - * / : joins over slices.
                match (l, r) {
                    (Ty::Slice(le, lk), Ty::Slice(re, rk)) => {
                        let same =
                            lk.len() == rk.len() && lk.iter().zip(rk).all(|(a, b)| a.name == b.name && a.ty == b.ty);
                        if !same {
                            return Err(e(Code::Join, "an operator over two slices with different keys"));
                        }
                        let t = self.arith(op, le, re)?;
                        Ok(Ty::Slice(Box::new(t), lk.clone()))
                    }
                    (Ty::Slice(le, lk), s) => {
                        let t = self.arith(op, le, s)?;
                        Ok(Ty::Slice(Box::new(t), lk.clone()))
                    }
                    (s, Ty::Slice(re, rk)) => {
                        let t = self.arith(op, s, re)?;
                        Ok(Ty::Slice(Box::new(t), rk.clone()))
                    }
                    _ => self.arith(op, l, r),
                }
            }
        }
    }

    fn compare(&self, op: Op, l: &Ty, r: &Ty) -> Result<Ty, Error> {
        let ordered = !matches!(op, Op::Eq | Op::Ne);
        match (l, r) {
            (Ty::Num(a), Ty::Num(b)) => {
                same_unit(a, b, &format!("comparison {}", op.text()))?;
                Ok(Ty::Bool)
            }
            (Ty::Bool, Ty::Bool) | (Ty::Blob, Ty::Blob) if !ordered => Ok(Ty::Bool),
            (Ty::Str, Ty::Str) | (Ty::Date, Ty::Date) | (Ty::Instant, Ty::Instant) => Ok(Ty::Bool),
            _ => Err(e(Code::Type, format!("{} {} {}", l.name(), op.text(), r.name()))),
        }
    }

    fn arith(&self, op: Op, l: &Ty, r: &Ty) -> Result<Ty, Error> {
        let what = format!("{} {} {}", l.name(), op.text(), r.name());
        match (op, l, r) {
            (Op::Add | Op::Sub, Ty::Num(a), Ty::Num(b)) => Ok(Ty::Num(add_units(op, a, b)?)),
            (Op::Mul | Op::Div, Ty::Num(a), Ty::Num(b)) => Ok(Ty::Num(mul_units(op, a, b)?)),
            (Op::Add | Op::Sub, Ty::Instant, Ty::Num(u)) => {
                time_unit(u, &["s", "min", "h", "d"], "an instant moves by s, min, h or d")?;
                Ok(Ty::Instant)
            }
            (Op::Add, Ty::Num(u), Ty::Instant) => {
                time_unit(u, &["s", "min", "h", "d"], "an instant moves by s, min, h or d")?;
                Ok(Ty::Instant)
            }
            (Op::Sub, Ty::Instant, Ty::Instant) => Ok(Ty::Num(NumUnit::atom("s"))),
            (Op::Add | Op::Sub, Ty::Date, Ty::Num(u)) => {
                time_unit(u, &["d"], &format!("a date moves by days (d), not {}", u.text()))?;
                Ok(Ty::Date)
            }
            (Op::Add, Ty::Num(u), Ty::Date) => {
                time_unit(u, &["d"], &format!("a date moves by days (d), not {}", u.text()))?;
                Ok(Ty::Date)
            }
            (Op::Sub, Ty::Date, Ty::Date) => Ok(Ty::Num(NumUnit::atom("d"))),
            _ => Err(e(Code::Type, what)),
        }
    }

    fn power(&self, b: &Ty, x: &Ty, exp_id: NodeId) -> Result<Ty, Error> {
        let (Ty::Num(bu), Ty::Num(xu)) = (b, x) else {
            if b.is_slice() || x.is_slice() {
                return Err(e(Code::SliceValue, "a slice where a scalar is required"));
            }
            return Err(e(Code::Type, format!("{} ^ {}", b.name(), x.name())));
        };
        match xu {
            NumUnit::Bare { .. } => {}
            NumUnit::Known(u) if u.is_one() => {}
            _ => return Err(e(Code::Exponent, "an exponent is dimensionless")),
        }
        if bu.is_degc() {
            return Err(e(Code::UnitOffset, "degC is a temperature"));
        }
        match bu {
            NumUnit::Bare { .. } => Ok(Ty::Num(NumUnit::Bare { zero: false })),
            NumUnit::Varies => Err(e(Code::UnitUnknown, "the unit varies by member")),
            NumUnit::Known(u) if u.is_one() => Ok(Ty::Num(bu.clone())),
            NumUnit::Known(u) => match literal_number(self.ast, exp_id) {
                Some(n) if n.is_integer() => match n.to_i64() {
                    Some(k) => Ok(Ty::Num(NumUnit::known(u.pow(k)))),
                    None => Err(e(Code::Exponent, "a number with a unit takes an integer literal exponent")),
                },
                _ => Err(e(Code::Exponent, "a number with a unit takes an integer literal exponent")),
            },
        }
    }

    // ------------------------------------------------------------------------------------
    // Reads.

    fn read(&mut self, id: NodeId) -> Result<Ty, Error> {
        let ast = self.ast;
        let Node::Read { fn_name, args } = ast.node(id) else { unreachable!("read") };
        if self.in_lambda > 0 {
            return Err(e(Code::LambdaRead, "no cell reads inside a lambda: join instead"));
        }
        self.inputs.insert(fn_name.clone());
        let env = self.env;
        let sig = env.signature_of(fn_name).ok_or_else(|| e(Code::Unresolved, format!("{fn_name} has no binding")))?;
        let lang = env.lang_of(fn_name).unwrap_or("stored").to_string();
        // Argument expressions, ascending by label.
        let mut bound: Vec<(String, Option<Ty>)> = Vec::new();
        for (name, a) in args {
            let decl =
                sig.args.get(name).ok_or_else(|| e(Code::ArgUnknown, format!("{fn_name} has no argument {name}")))?;
            match a {
                ReadArg::Open => bound.push((name.clone(), None)),
                ReadArg::Expr(x) => {
                    if let Node::Call { fn_name: f, .. } = ast.node(*x) {
                        if f == "within" {
                            return Err(e(
                                Code::Unsupported,
                                "a key-set argument (a read over the keys of a slice) is 1.3b",
                            ));
                        }
                    }
                    let t = self.node(*x)?;
                    if t.is_slice() {
                        return Err(e(Code::SliceValue, format!("{fn_name}({name}) takes a scalar, not a slice")));
                    }
                    bound.push((name.clone(), Some(t)));
                }
                ReadArg::Range(lo, hi) => {
                    if matches!(decl.ty, Type::Boolean | Type::Blob) {
                        return Err(e(
                            Code::SliceBound,
                            format!("a range over the {} argument {name}", decl.ty.as_str()),
                        ));
                    }
                    let a = self.scalar_of(*lo)?;
                    let b = self.scalar_of(*hi)?;
                    let t = self.unify(&a, &b, "the ends of a range")?;
                    bound.push((name.clone(), Some(t)));
                }
            }
        }
        // Substitution of the callee's unit references by the units of the bound expressions.
        let subst = |a: &str| -> Result<NumUnit, Error> {
            let Some(pos) = args.iter().position(|(n, _)| n == a) else {
                return Err(e(Code::UnitUnknown, format!("{fn_name}: the unit argument {a} is not bound")));
            };
            match &args[pos].1 {
                ReadArg::Open | ReadArg::Range(..) => Ok(NumUnit::Varies),
                ReadArg::Expr(x) => match ast.node(*x) {
                    Node::Str(s) if units::is_unit(s) => Ok(NumUnit::atom(s)),
                    Node::Var(n) => match self.scope.get(n).and_then(|(_, _, s)| s.clone()) {
                        Some(UnitSource::Arg(q)) => Ok(NumUnit::atom(&format!("arg:{q}"))),
                        Some(UnitSource::Literal(l)) => Ok(NumUnit::atom(&l)),
                        None => {
                            Err(e(Code::UnitUnknown, format!("the unit of {fn_name} cannot be determined statically")))
                        }
                    },
                    _ => Err(e(Code::UnitUnknown, format!("the unit of {fn_name} cannot be determined statically"))),
                },
            }
        };
        // Types and units of bound arguments against the callee's signature.
        let mut keys = Vec::new();
        for (name, t) in &bound {
            let decl = &sig.args[name];
            match t {
                None => {
                    let unit = match decl.ty {
                        Type::Number => Some(match &decl.unit {
                            Some(u) => decl_unit(u, &subst).unwrap_or(NumUnit::Varies),
                            None => NumUnit::Known(Unit::one()),
                        }),
                        _ => None,
                    };
                    keys.push(KeyTy { name: name.clone(), ty: decl.ty, unit, valid: decl.valid });
                }
                Some(t) => {
                    if t.scalar_type() != Some(decl.ty) {
                        return Err(e(
                            Code::Type,
                            format!("{fn_name}({name}) is a {}, not a {}", decl.ty.as_str(), t.name()),
                        ));
                    }
                    if let (Ty::Num(got), Some(u)) = (t, &decl.unit) {
                        let want = decl_unit(u, &subst)?;
                        if want != NumUnit::Varies {
                            same_unit(got, &want, &format!("{fn_name}({name})"))?;
                        }
                    }
                    let is_range =
                        matches!(args.iter().find(|(n, _)| n == name).map(|(_, a)| a), Some(ReadArg::Range(..)));
                    if is_range {
                        let unit = match decl.ty {
                            Type::Number => Some(match &decl.unit {
                                Some(u) => decl_unit(u, &subst).unwrap_or(NumUnit::Varies),
                                None => NumUnit::Known(Unit::one()),
                            }),
                            _ => None,
                        };
                        keys.push(KeyTy { name: name.clone(), ty: decl.ty, unit, valid: decl.valid });
                    }
                }
            }
        }
        // Extents (15.13).
        if !keys.is_empty() && lang != "stored" {
            self.extents(fn_name, sig, args)?;
        }
        let elem = match sig.returns {
            Type::Number => Ty::Num(match &sig.returns_unit {
                Some(u) => decl_unit(u, &subst)?,
                None => NumUnit::Known(Unit::one()),
            }),
            t => scalar(t),
        };
        if keys.is_empty() {
            Ok(elem)
        } else {
            Ok(Ty::Slice(Box::new(elem), keys))
        }
    }

    /// Open and ranged arguments over a function that is not `stored` (15.13).
    fn extents(&self, fn_name: &str, sig: &Signature, args: &[(String, ReadArg)]) -> Result<(), Error> {
        let mut cells: u128 = 1;
        for (name, a) in args {
            let decl = &sig.args[name];
            let count: u128 = match a {
                ReadArg::Expr(_) => continue,
                ReadArg::Open => match &decl.extent {
                    Some(x) => extent_size(decl.ty, x),
                    None => {
                        return Err(e(Code::Unbounded, format!("{fn_name}({name}: *) has no extent")));
                    }
                },
                ReadArg::Range(lo, hi) => match &decl.extent {
                    Some(x) => extent_size(decl.ty, x),
                    None => {
                        if !matches!(decl.ty, Type::Date | Type::Number) {
                            return Err(e(
                                Code::Unbounded,
                                format!("a range over {fn_name}({name}) cannot be enumerated"),
                            ));
                        }
                        match range_size(self.ast, decl.ty, *lo, *hi) {
                            Some(n) => n,
                            None => {
                                if decl.ty == Type::Number && !self.integer_valued(*lo, *hi) {
                                    return Err(e(
                                        Code::Unbounded,
                                        format!("a range over {fn_name}({name}) cannot be enumerated"),
                                    ));
                                }
                                1
                            }
                        }
                    }
                },
            };
            cells = cells.saturating_mul(count);
        }
        if cells > 1_048_576 {
            return Err(e(Code::Unbounded, format!("a slice of {fn_name} of more than 1,048,576 cells")));
        }
        Ok(())
    }

    fn integer_valued(&self, lo: NodeId, hi: NodeId) -> bool {
        let ok = |id: NodeId| match literal_number(self.ast, id) {
            Some(d) => d.is_integer(),
            None => true,
        };
        ok(lo) && ok(hi)
    }

    // ------------------------------------------------------------------------------------
    // Built-ins.

    fn call(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        if AGGREGATES.contains(&f) {
            return self.aggregate(f, args);
        }
        if LOOKUPS.contains(&f) {
            return self.lookup(id, f, args, named);
        }
        match f {
            "map" | "filter" => self.map_filter(f, args),
            "scan" | "window" => self.t_scan(id, f, args, named),
            "otherwise" => self.t_otherwise(id, f, args, named),
            "asof" => self.t_asof(id, f, args, named),
            "versions" => self.t_versions(id, f, args, named),
            "abs" | "floor" | "ceiling" => {
                let x = self.number(args[0], f)?;
                Ok(Ty::Num(x))
            }
            "round" | "round_up" | "round_down" => self.t_round(id, f, args, named),
            "mod" => self.t_mod(id, f, args, named),
            "sqrt" => self.t_sqrt(id, f, args, named),
            "exp" | "ln" => self.t_exp(id, f, args, named),
            "sin" | "cos" => self.t_sin(id, f, args, named),
            "atan2" => self.t_atan2(id, f, args, named),
            "pi" => Ok(Ty::Num(NumUnit::Known(Unit::one()))),
            "haversine" => self.t_haversine(id, f, args, named),
            "convert" => self.t_convert(id, f, args, named),
            "date" => self.t_date(id, f, args, named),
            "year" | "month" | "day" | "weekday" => self.t_year(id, f, args, named),
            "end_of_month" => {
                self.expect(args[0], &Ty::Date, f)?;
                Ok(Ty::Date)
            }
            "add_days" | "add_months" => self.t_add_days(id, f, args, named),
            "business_days" => self.t_business_days(id, f, args, named),
            "year_fraction" => self.t_year_fraction(id, f, args, named),
            "is_holiday" => self.t_is_holiday(id, f, args, named),
            "date_of" => self.t_date_of(id, f, args, named),
            "instant" => self.t_instant(id, f, args, named),
            "len" => self.t_len(id, f, args, named),
            "upper" | "lower" | "trim" => {
                self.expect(args[0], &Ty::Str, f)?;
                Ok(Ty::Str)
            }
            "concat" => self.t_concat(id, f, args, named),
            "substr" => self.t_substr(id, f, args, named),
            "find" => self.t_find(id, f, args, named),
            "text" => {
                self.scalar_of(args[0])?;
                Ok(Ty::Str)
            }
            "fixed" => self.t_fixed(id, f, args, named),
            _ => Err(e(Code::UnknownFunction, format!("{f} is not a built-in"))),
        }
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_scan(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        let named_of = |n: &str| named.iter().find(|(k, _)| k == n).map(|(_, v)| *v);
        let s = self.node(args[0])?;
        let Ty::Slice(elem, keys) = &s else {
            return Err(e(Code::Type, format!("{f}() takes a slice")));
        };
        let agg_pos = if f == "scan" { 1 } else { 2 };
        if f == "window" {
            let n = self.scalar_of(args[1])?;
            self.dimensionless(&n, "the width of window()")?;
        }
        let Node::Agg(agg) = self.ast.node(args[agg_pos]) else {
            return Err(e(Code::Enum, format!("the aggregate of {f}() is an aggregate name")));
        };
        let along = match named_of("along") {
            Some(a) => Some(self.along_key(a)?),
            None => None,
        };
        self.dimension(keys, along.as_deref())?;
        let t = self.aggregate_type(agg, elem)?;
        Ok(Ty::Slice(Box::new(t), keys.clone()))
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_otherwise(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        let a = self.node(args[0])?;
        let b = self.node(args[1])?;
        if a.is_slice() || b.is_slice() {
            return Err(e(Code::SliceValue, "otherwise() takes scalars"));
        }
        self.unify(&a, &b, "otherwise()")
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_asof(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        if args.is_empty() {
            self.asof_dependent = true;
            return Ok(Ty::Instant);
        }
        let t = self.tx_cell(args[0], "asof(cell, T)")?;
        let at = self.scalar_of(args[1])?;
        if at != Ty::Instant {
            return Err(e(Code::Type, format!("T of asof(cell, T) is an instant, not {}", at.name())));
        }
        Ok(t)
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_versions(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        let t = self.tx_cell(args[0], "versions(cell, from, to)")?;
        for a in &args[1..] {
            let x = self.scalar_of(*a)?;
            if x != Ty::Instant {
                return Err(e(Code::Type, format!("versions() takes instants, not {}", x.name())));
            }
        }
        let key = KeyTy { name: "tx".to_string(), ty: Type::Instant, unit: None, valid: true };
        Ok(Ty::Slice(Box::new(t), vec![key]))
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_round(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        let named_of = |n: &str| named.iter().find(|(k, _)| k == n).map(|(_, v)| *v);
        let x = self.number(args[0], f)?;
        if let Some(d) = named_of("digits") {
            let t = self.scalar_of(d)?;
            self.dimensionless(&t, "digits")?;
        }
        Ok(Ty::Num(x))
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_mod(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        let a = self.number(args[0], f)?;
        let b = self.number(args[1], f)?;
        if a.is_degc() || b.is_degc() {
            return Err(e(Code::UnitOffset, "mod() of degC"));
        }
        Ok(Ty::Num(same_unit(&a, &b, "mod()")?))
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_sqrt(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        let x = self.number(args[0], f)?;
        if x.is_degc() {
            return Err(e(Code::UnitOffset, "sqrt() of degC"));
        }
        match &x {
            NumUnit::Bare { .. } => Ok(Ty::Num(NumUnit::Bare { zero: false })),
            NumUnit::Varies => Err(e(Code::UnitUnknown, "the unit varies by member")),
            NumUnit::Known(u) => match u.sqrt() {
                Some(r) => Ok(Ty::Num(NumUnit::Known(r))),
                None => Err(e(Code::Exponent, format!("sqrt() of {} has no unit", x.text()))),
            },
        }
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_exp(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        let x = self.number(args[0], f)?;
        if !(x.is_bare() || x == NumUnit::Known(Unit::one())) {
            return Err(e(Code::UnitMismatch, format!("the argument of {f}() is dimensionless")));
        }
        Ok(Ty::Num(NumUnit::Known(Unit::one())))
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_sin(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        let x = self.number(args[0], f)?;
        if !(x.is_bare() || x == NumUnit::Known(Unit::one()) || x.single() == Some("deg")) {
            return Err(e(Code::UnitMismatch, format!("{f}() takes radians (1) or deg, not {}", x.text())));
        }
        Ok(Ty::Num(NumUnit::Known(Unit::one())))
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_atan2(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        let y = self.number(args[0], f)?;
        let x = self.number(args[1], f)?;
        if y.is_degc() || x.is_degc() {
            return Err(e(Code::UnitOffset, "atan2() of degC"));
        }
        same_unit(&y, &x, "atan2()")?;
        Ok(Ty::Num(NumUnit::Known(Unit::one())))
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_haversine(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        for a in args {
            let x = self.number(*a, f)?;
            if !(x.is_bare() || x.single() == Some("deg")) {
                return Err(e(Code::UnitMismatch, format!("haversine() takes degrees, not {}", x.text())));
            }
        }
        Ok(Ty::Num(NumUnit::atom("m")))
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_convert(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        let x = self.number(args[0], f)?;
        let Some(target) = str_literal(self.ast, args[1]) else {
            return Err(e(Code::Enum, "the unit of convert() is a string literal"));
        };
        if !units::is_unit(target) {
            return Err(e(Code::Unit, format!("not a listed unit: {target}")));
        }
        if convert_factor(&x, target).is_none() {
            return Err(e(Code::UnitMismatch, format!("convert() cannot convert {} to {target}", x.text())));
        }
        Ok(Ty::Num(NumUnit::atom(target)))
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_date(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        for a in args {
            let t = self.scalar_of(*a)?;
            self.dimensionless(&t, "date()")?;
        }
        Ok(Ty::Date)
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_year(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        self.expect(args[0], &Ty::Date, f)?;
        Ok(Ty::Num(NumUnit::Known(Unit::one())))
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_add_days(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        self.expect(args[0], &Ty::Date, f)?;
        let n = self.scalar_of(args[1])?;
        match &n {
            Ty::Num(u) if f == "add_days" => {
                if !(u.is_bare() || u.single() == Some("d")) {
                    return Err(e(Code::UnitMismatch, format!("add_days() counts days, not {}", u.text())));
                }
            }
            Ty::Num(_) => self.dimensionless(&n, "the months of add_months()")?,
            _ => return Err(e(Code::Type, format!("{f}() counts with a number, not {}", n.name()))),
        }
        self.business_options(named, f == "add_months")?;
        Ok(Ty::Date)
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_business_days(
        &mut self,
        id: NodeId,
        f: &str,
        args: &[NodeId],
        named: &[(String, NodeId)],
    ) -> Result<Ty, Error> {
        self.expect(args[0], &Ty::Date, f)?;
        self.expect(args[1], &Ty::Date, f)?;
        self.business_options(named, false)?;
        Ok(Ty::Num(NumUnit::atom("d")))
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_year_fraction(
        &mut self,
        id: NodeId,
        f: &str,
        args: &[NodeId],
        named: &[(String, NodeId)],
    ) -> Result<Ty, Error> {
        self.expect(args[0], &Ty::Date, f)?;
        self.expect(args[1], &Ty::Date, f)?;
        match str_literal(self.ast, args[2]) {
            Some(c) if CONVENTIONS.contains(&c) => {}
            _ => return Err(e(
                Code::Enum,
                "convention is a string literal, one of act/360, act/365f, act/act_isda, 30/360, 30e/360, 30/360_us",
            )),
        }
        Ok(Ty::Num(NumUnit::Known(Unit::one())))
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_is_holiday(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        self.expect(args[0], &Ty::Date, f)?;
        self.calendar(args[1])?;
        Ok(Ty::Bool)
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_date_of(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        self.expect(args[0], &Ty::Instant, f)?;
        Ok(Ty::Date)
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_instant(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        self.expect(args[0], &Ty::Date, f)?;
        Ok(Ty::Instant)
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_len(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        self.expect(args[0], &Ty::Str, f)?;
        Ok(Ty::Num(NumUnit::Known(Unit::one())))
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_concat(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        for a in args {
            self.expect(*a, &Ty::Str, f)?;
        }
        Ok(Ty::Str)
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_substr(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        self.expect(args[0], &Ty::Str, f)?;
        for a in &args[1..] {
            let t = self.scalar_of(*a)?;
            self.dimensionless(&t, "substr()")?;
        }
        Ok(Ty::Str)
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_find(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        let named_of = |n: &str| named.iter().find(|(k, _)| k == n).map(|(_, v)| *v);
        self.expect(args[0], &Ty::Str, f)?;
        self.expect(args[1], &Ty::Str, f)?;
        if let Some(s) = named_of("start") {
            let t = self.scalar_of(s)?;
            self.dimensionless(&t, "start")?;
        }
        Ok(Ty::Num(NumUnit::Known(Unit::one())))
    }

    #[inline(never)]
    #[allow(unused_variables)]
    fn t_fixed(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        self.number(args[0], f)?;
        let t = self.scalar_of(args[1])?;
        self.dimensionless(&t, "digits")?;
        Ok(Ty::Str)
    }

    fn expect(&mut self, id: NodeId, want: &Ty, f: &str) -> Result<(), Error> {
        let t = self.scalar_of(id)?;
        if std::mem::discriminant(&t) != std::mem::discriminant(want) {
            return Err(e(Code::Type, format!("{f}() takes a {}, not a {}", want.name(), t.name())));
        }
        Ok(())
    }

    fn number(&mut self, id: NodeId, f: &str) -> Result<NumUnit, Error> {
        match self.scalar_of(id)? {
            Ty::Num(u) => Ok(u),
            t => Err(e(Code::Type, format!("{f}() takes a number, not a {}", t.name()))),
        }
    }

    fn dimensionless(&self, t: &Ty, what: &str) -> Result<(), Error> {
        match t {
            Ty::Num(NumUnit::Bare { .. }) => Ok(()),
            Ty::Num(NumUnit::Known(u)) if u.is_one() => Ok(()),
            Ty::Num(_) => Err(e(Code::UnitMismatch, format!("{what} is dimensionless"))),
            other => Err(e(Code::Type, format!("{what} is a number, not a {}", other.name()))),
        }
    }

    fn business_options(&mut self, named: &[(String, NodeId)], rolls: bool) -> Result<(), Error> {
        for (k, v) in named {
            match k.as_str() {
                "calendar" => self.calendar(*v)?,
                "weekend" => match str_literal(self.ast, *v) {
                    Some(w) if WEEKENDS.contains(&w) => {}
                    _ => return Err(e(Code::Enum, "weekend is a string literal, one of sat-sun, fri-sat, sun, none")),
                },
                "roll" if rolls => match str_literal(self.ast, *v) {
                    Some(r) if ROLLS.contains(&r) => {}
                    _ => {
                        return Err(e(
                            Code::Enum,
                            "roll is a string literal, one of following, modified_following, preceding",
                        ))
                    }
                },
                _ => {}
            }
        }
        Ok(())
    }

    /// A calendar: a slice with one open date argument.
    fn calendar(&mut self, id: NodeId) -> Result<(), Error> {
        let t = self.node(id)?;
        match &t {
            Ty::Slice(_, keys) if keys.len() == 1 && keys[0].ty == Type::Date => Ok(()),
            _ => Err(e(Code::Type, "a calendar is a slice with one open date argument")),
        }
    }

    fn tx_cell(&mut self, id: NodeId, what: &str) -> Result<Ty, Error> {
        if !matches!(self.ast.node(id), Node::Read { .. }) {
            return Err(e(Code::TxArg, format!("the first argument of {what} is a single-cell read")));
        }
        let t = self.node(id)?;
        if t.is_slice() {
            return Err(e(Code::TxArg, format!("the first argument of {what} is a single-cell read, not a slice")));
        }
        Ok(t)
    }

    fn aggregate(&mut self, f: &str, args: &[NodeId]) -> Result<Ty, Error> {
        let mut tys = Vec::new();
        for a in args {
            tys.push(self.node(*a)?);
        }
        let elem = if tys.len() == 1 && tys[0].is_slice() {
            let Ty::Slice(elem, _) = &tys[0] else { unreachable!("slice") };
            (**elem).clone()
        } else {
            if tys.iter().any(Ty::is_slice) {
                return Err(e(Code::Type, format!("{f}() takes one slice or scalars")));
            }
            let mut acc = tys[0].clone();
            for t in &tys[1..] {
                acc = match (&acc, t) {
                    (Ty::Num(_), Ty::Num(_)) if f == "count" => acc,
                    (Ty::Num(a), Ty::Num(b)) => Ty::Num(same_unit(a, b, &format!("{f}()"))?),
                    _ if std::mem::discriminant(&acc) == std::mem::discriminant(t) => acc,
                    _ => return Err(e(Code::Type, format!("{f}() of {} and {}", acc.name(), t.name()))),
                };
            }
            acc
        };
        self.aggregate_type(f, &elem)
    }

    fn aggregate_type(&self, f: &str, elem: &Ty) -> Result<Ty, Error> {
        if f == "count" {
            return Ok(Ty::Num(NumUnit::atom("count")));
        }
        match elem {
            Ty::Num(u) => {
                if *u == NumUnit::Varies {
                    return Err(e(Code::UnitUnknown, format!("{f}(): the unit varies by member")));
                }
                match f {
                    "sum" if u.is_degc() => Err(e(Code::UnitOffset, "sum() of degC: go through kelvin")),
                    "stdev" | "stdev_p" if u.is_degc() => Ok(Ty::Num(NumUnit::atom("K"))),
                    _ => Ok(Ty::Num(u.clone())),
                }
            }
            Ty::Str | Ty::Date | Ty::Instant if matches!(f, "min" | "max" | "first" | "last") => Ok(elem.clone()),
            Ty::Bool | Ty::Blob if matches!(f, "first" | "last") => Ok(elem.clone()),
            _ => Err(e(Code::Type, format!("{f}() needs numbers"))),
        }
    }

    fn along_key(&mut self, id: NodeId) -> Result<String, Error> {
        match self.ast.node(id) {
            Node::Key(k) => Ok(k.clone()),
            _ => Err(e(Code::Along, "along names an open argument of the slice")),
        }
    }

    /// The dimension of a lookup or scan (15.13): `along`, else the only key, else the only
    /// valid-time key.
    fn dimension(&self, keys: &[KeyTy], along: Option<&str>) -> Result<usize, Error> {
        dimension(keys, along)
    }

    fn lookup(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<Ty, Error> {
        let _ = id;
        let s = self.node(args[0])?;
        let Ty::Slice(elem, keys) = &s else {
            return Err(e(Code::Type, format!("{f}() takes a slice")));
        };
        let x = if args.len() > 1 { Some(self.scalar_of(args[1])?) } else { None };
        let mut along = None;
        let mut bound = None;
        let mut method_id = None;
        for (k, v) in named {
            match k.as_str() {
                "along" => along = Some(self.along_key(*v)?),
                "bound" => bound = Some(self.scalar_of(*v)?),
                "method" => method_id = Some(*v),
                _ => {}
            }
        }
        if f == "within" {
            bound = Some(self.scalar_of(args[2])?);
        }
        let d = self.dimension(keys, along.as_deref())?;
        let key = &keys[d];
        if let Some(x) = &x {
            match (x, key.ty) {
                (Ty::Num(u), Type::Number) => {
                    if let Some(ku) = &key.unit {
                        same_unit(u, ku, &format!("{f}()"))?;
                    }
                }
                _ if x.scalar_type() == Some(key.ty) => {}
                _ => {
                    return Err(e(Code::Type, format!("{f}(): the key is a {}, not a {}", key.ty.as_str(), x.name())));
                }
            }
        }
        let needs_metric = matches!(f, "nearest" | "within" | "interpolate") || bound.is_some();
        if needs_metric {
            match key.ty {
                Type::Instant | Type::Date | Type::Number => {}
                _ => return Err(e(Code::Metric, format!("{f}(): {} keys have no metric", key.ty.as_str()))),
            }
            if f == "latest" && key.ty != Type::Instant {
                return Err(e(Code::Metric, "latest() with a bound needs an instant dimension"));
            }
        }
        if let Some(b) = &bound {
            let ok = match (b, key.ty) {
                (Ty::Num(u), Type::Instant) => matches!(u.single(), Some("s" | "min" | "h" | "d")) || u.is_bare(),
                (Ty::Num(u), Type::Date) => u.single() == Some("d") || u.is_bare(),
                (Ty::Num(u), Type::Number) => match &key.unit {
                    Some(ku) => same_unit(u, ku, "bound").is_ok(),
                    None => true,
                },
                _ => false,
            };
            if !ok {
                return Err(e(Code::Metric, format!("{f}(): a bound in the wrong unit")));
            }
            if f == "latest" {
                self.asof_dependent = true;
            }
        }
        if f == "interpolate" {
            if let Some(m) = method_id {
                match str_literal(self.ast, m) {
                    Some("spline") => return Err(e(Code::Unsupported, "spline interpolation is not in this revision")),
                    Some(s) if METHODS.contains(&s) => {}
                    _ => return Err(e(Code::Enum, "method is a string literal, one of linear, step")),
                }
            }
            if !matches!(**elem, Ty::Num(_)) {
                return Err(e(Code::Type, "interpolate() needs numbers"));
            }
        }
        if let Ty::Num(NumUnit::Varies) = **elem {
            return Err(e(Code::UnitUnknown, format!("{f}(): the unit varies by member")));
        }
        if f == "within" {
            return Ok(s.clone());
        }
        if keys.len() == 1 {
            Ok((**elem).clone())
        } else {
            let mut rest = keys.clone();
            rest.remove(d);
            Ok(Ty::Slice(elem.clone(), rest))
        }
    }

    fn map_filter(&mut self, f: &str, args: &[NodeId]) -> Result<Ty, Error> {
        let s = self.node(args[0])?;
        let Ty::Slice(elem, keys) = &s else {
            return Err(e(Code::Type, format!("{f}() takes a slice")));
        };
        let Node::Lambda { params, body } = self.ast.node(args[1]) else {
            return Err(e(Code::Type, format!("the second argument of {f}() is a lambda")));
        };
        let mut pushed = 0;
        let mut result = Ok(());
        for (i, p) in params.iter().enumerate() {
            if self.scope.has(p) || params[..i].contains(p) {
                result = Err(e(Code::Shadow, format!("{p} is already a name here")));
                break;
            }
            let t = if i == 0 {
                (**elem).clone()
            } else {
                match keys.iter().find(|k| &k.name == p) {
                    Some(k) => match k.ty {
                        Type::Number => Ty::Num(k.unit.clone().unwrap_or(NumUnit::Known(Unit::one()))),
                        t => scalar(t),
                    },
                    None => {
                        result = Err(e(Code::LambdaParam, format!("{p} is not an open argument of the slice")));
                        break;
                    }
                }
            };
            self.scope.names.push((p.clone(), t, None));
            pushed += 1;
        }
        let r = result.and_then(|_| {
            self.in_lambda += 1;
            let r = self.node(*body);
            self.in_lambda -= 1;
            r
        });
        for _ in 0..pushed {
            self.scope.names.pop();
        }
        let bt = r?;
        if bt.is_slice() {
            return Err(e(Code::SliceValue, "a lambda returns a scalar"));
        }
        if f == "filter" {
            if bt != Ty::Bool {
                return Err(e(Code::Type, format!("the predicate of filter() is {}, not boolean", bt.name())));
            }
            return Ok(s.clone());
        }
        Ok(Ty::Slice(Box::new(bt), keys.clone()))
    }

    fn returns(&self, ty: &Ty, sig: &Signature) -> Result<(), Error> {
        if ty.is_slice() {
            return Err(e(Code::SliceValue, "a function returns a scalar, not a slice"));
        }
        if ty.scalar_type() != Some(sig.returns) {
            return Err(e(
                Code::ReturnType,
                format!("the body is {}; the function returns {}", ty.name(), sig.returns.as_str()),
            ));
        }
        if let (Ty::Num(got), Some(u)) = (ty, &sig.returns_unit) {
            let want = own_unit(u);
            let ok = match got {
                NumUnit::Bare { zero } => adopt(*zero, &want).is_ok(),
                g => *g == want,
            };
            if !ok {
                return Err(e(
                    Code::ReturnUnit,
                    format!("the body is in {}; the function returns {}", got.text(), want.text()),
                ));
            }
        }
        Ok(())
    }
}

/// The dimension of a lookup or scan over `keys` (15.13).
pub(crate) fn dimension(keys: &[KeyTy], along: Option<&str>) -> Result<usize, Error> {
    if let Some(a) = along {
        return keys
            .iter()
            .position(|k| k.name == a)
            .ok_or_else(|| e(Code::Along, format!("{a} is not an open argument of the slice")));
    }
    if keys.len() == 1 {
        return Ok(0);
    }
    let valid: Vec<usize> = (0..keys.len()).filter(|&i| keys[i].valid).collect();
    if valid.len() == 1 {
        return Ok(valid[0]);
    }
    Err(e(Code::Along, "the dimension cannot be chosen: name it with along"))
}

fn add_units(op: Op, a: &NumUnit, b: &NumUnit) -> Result<NumUnit, Error> {
    let what = format!("{}: {} and {}", op.text(), a.text(), b.text());
    let k = NumUnit::atom("K");
    let degc = NumUnit::atom("degC");
    // Offset units (15.10): degC - degC is K; degC +- K and degC +- a bare literal are degC;
    // K + degC and a bare literal + degC are degC; anything else with degC is an offset error.
    if a.is_degc() || b.is_degc() {
        return match (op, a, b) {
            (Op::Sub, x, y) if x.is_degc() && y.is_degc() => Ok(k),
            (_, x, y) if x.is_degc() && (*y == k || y.is_bare()) => Ok(degc),
            (Op::Add, x, y) if (*x == k || x.is_bare()) && y.is_degc() => Ok(degc),
            _ => Err(e(Code::UnitOffset, format!("{what}: add a difference (K) to a temperature"))),
        };
    }
    same_unit(a, b, op.text())
}

fn mul_units(op: Op, a: &NumUnit, b: &NumUnit) -> Result<NumUnit, Error> {
    if a.is_degc() || b.is_degc() {
        return Err(e(Code::UnitOffset, format!("degC {} x: go through kelvin", op.text())));
    }
    let sign = if op == Op::Mul { 1 } else { -1 };
    match (a, b) {
        (NumUnit::Varies, _) | (_, NumUnit::Varies) => Err(e(Code::UnitUnknown, "the unit varies by member")),
        (NumUnit::Bare { .. }, NumUnit::Bare { .. }) => Ok(NumUnit::Bare { zero: false }),
        (NumUnit::Bare { .. }, NumUnit::Known(u)) => Ok(NumUnit::Known(Unit::one().mul(u, sign))),
        (NumUnit::Known(u), NumUnit::Bare { .. }) => Ok(NumUnit::Known(u.clone())),
        (NumUnit::Known(x), NumUnit::Known(y)) => Ok(NumUnit::Known(x.mul(y, sign))),
    }
}

fn time_unit(u: &NumUnit, allowed: &[&str], msg: &str) -> Result<(), Error> {
    match u {
        NumUnit::Bare { .. } => Ok(()),
        NumUnit::Known(_) if u.single().is_some_and(|s| allowed.contains(&s)) => Ok(()),
        _ => Err(e(Code::UnitMismatch, msg.to_string())),
    }
}

/// The groups of `convert` (15.10) and each unit's factor to the group's base, as a rational
/// `(numerator, denominator)`; degrees are special-cased by the evaluator.
pub(crate) fn convert_factor(from: &NumUnit, to: &str) -> Option<ConvertKind> {
    let groups: [&[(&str, i64)]; 3] = [
        &[("1", 10000), ("percent", 100), ("permille", 10), ("bp", 1)],
        &[("mm", 1), ("cm", 10), ("m", 1000), ("km", 1_000_000)],
        &[("s", 1), ("min", 60), ("h", 3600), ("d", 86400)],
    ];
    let f = match from {
        NumUnit::Bare { .. } => "1",
        NumUnit::Known(u) if u.is_one() => "1",
        NumUnit::Known(_) => from.single()?,
        NumUnit::Varies => return None,
    };
    for g in groups {
        let a = g.iter().find(|(n, _)| *n == f);
        let b = g.iter().find(|(n, _)| *n == to);
        if let (Some(a), Some(b)) = (a, b) {
            return Some(ConvertKind::Ratio(a.1, b.1));
        }
    }
    match (f, to) {
        ("degC", "K") => Some(ConvertKind::CelsiusToKelvin),
        ("K", "degC") => Some(ConvertKind::KelvinToCelsius),
        ("degC", "degC") | ("K", "K") | ("deg", "deg") => Some(ConvertKind::Ratio(1, 1)),
        ("deg", "1") => Some(ConvertKind::DegToRad),
        ("1", "deg") => Some(ConvertKind::RadToDeg),
        _ => None,
    }
}

/// How `convert` maps a value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConvertKind {
    /// Multiply by `from / to` (both factors to the group's smallest unit).
    Ratio(i64, i64),
    /// `+ 273.15`.
    CelsiusToKelvin,
    /// `- 273.15`.
    KelvinToCelsius,
    /// `* RAD_PER_DEG` at 40 digits, then 34.
    DegToRad,
    /// `/ RAD_PER_DEG` at 40 digits, then 34.
    RadToDeg,
}

/// The number of values in an extent (7).
pub(crate) fn extent_size(ty: Type, x: &Extent) -> u128 {
    match x {
        Extent::Values(v) => v.len() as u128,
        Extent::Range(lo, hi) => match ty {
            Type::Date => match (calendar::Date::parse(lo), calendar::Date::parse(hi)) {
                (Some(a), Some(b)) => (b.ordinal() - a.ordinal() + 1).max(0) as u128,
                _ => 0,
            },
            _ => match (Decimal::parse(lo).and_then(|d| d.to_i64()), Decimal::parse(hi).and_then(|d| d.to_i64())) {
                (Some(a), Some(b)) => (i128::from(b) - i128::from(a) + 1).max(0) as u128,
                _ => 0,
            },
        },
    }
}

/// The size of a written range with literal ends, if both ends are literals.
fn range_size(ast: &Ast, ty: Type, lo: NodeId, hi: NodeId) -> Option<u128> {
    match ty {
        Type::Date => match (ast.node(lo), ast.node(hi)) {
            (Node::Date(a), Node::Date(b)) => {
                let (a, b) = (calendar::Date::parse(a)?, calendar::Date::parse(b)?);
                Some((b.ordinal() - a.ordinal() + 1).max(0) as u128)
            }
            _ => None,
        },
        _ => {
            let a = literal_number(ast, lo)?;
            let b = literal_number(ast, hi)?;
            let (a, b) = (a.floor().to_i64()?, b.floor().to_i64()?);
            Some((i128::from(b) - i128::from(a) + 1).max(0) as u128)
        }
    }
}
