//! The evaluator (SPEC 15.12, 15.13): strict, left to right, with normative fuel, faults and
//! lineage, over an abstract [`World`].
//!
//! The world answers what a mesh knows: which function a name is bound to (and since when),
//! the version of a cell answering as of the evaluation asof or an explicit transaction time,
//! the members of a stored slice, and the versions of a stored cell. A computed cell read by a
//! body is evaluated by the world (memoised, with cycle and depth limits): the evaluator only
//! asks for its version. The evaluator derives the lineage (receipt hashes and slice entries),
//! the latest input `tx` and the receipt `asof` of 8.2 rule 2.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use datumesh_core::json::Value;
use datumesh_core::{Scalar, Type};

use crate::ast::{Node, NodeId, Op, ReadArg};
use crate::builtins::AGGREGATES;
use crate::calendar::{self, BusinessDays, Date, Micros, Roll, Weekend, DAY_MICROS};
use crate::check::{self, ConvertKind, KeyTy, NumUnit, Ty};
use crate::decimal::{self, Decimal, Mode, RoundMode};
use crate::env::{Extent, Function};
use crate::{text, Code, Compiled};

/// Arguments of a cell: name to typed scalar.
pub type Args = BTreeMap<String, Scalar>;

/// The binding of a function name: the function and the start of the binding (an instant).
#[derive(Clone, Debug)]
pub struct Binding {
    /// The function the name resolves to.
    pub function: Rc<Function>,
    /// When the binding began.
    pub start: String,
}

/// A version of a cell: its value and the receipt that states it.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
    /// The value.
    pub value: Scalar,
    /// The receipt hash (8.3).
    pub receipt_hash: String,
    /// The receipt's `tx`.
    pub tx: String,
    /// The receipt's `asof`.
    pub asof: String,
}

/// One cell: a function name and a complete argument map.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellKey {
    /// The fully qualified function name.
    pub name: String,
    /// The arguments.
    pub args: Args,
}

/// One argument of a slice pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatternArg {
    /// Bound to a value.
    Value(Scalar),
    /// Open (`*`).
    Open,
    /// Bounded, both ends inclusive.
    Range(Scalar, Scalar),
}

/// A slice of a function: its name and the pattern of its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SliceKey {
    /// The fully qualified function name.
    pub name: String,
    /// The pattern, by argument name.
    pub args: BTreeMap<String, PatternArg>,
}

impl SliceKey {
    /// Whether a cell's arguments match the pattern (bound arguments equal, ranges containing).
    pub fn matches(&self, args: &Args) -> bool {
        if args.len() != self.args.len() {
            return false;
        }
        for (k, p) in &self.args {
            let Some(v) = args.get(k) else { return false };
            match p {
                PatternArg::Open => {}
                PatternArg::Value(x) => {
                    if x != v {
                        return false;
                    }
                }
                PatternArg::Range(lo, hi) => {
                    if v.ty() != lo.ty() || cmp_scalar(v, lo) == std::cmp::Ordering::Less || cmp_scalar(v, hi) == std::cmp::Ordering::Greater
                    {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// Compare two scalars of one type in the type's order (15.13): numbers numerically, dates and
/// instants chronologically, strings by code point, `false` before `true`.
pub fn cmp_scalar(a: &Scalar, b: &Scalar) -> std::cmp::Ordering {
    match (a, b) {
        (Scalar::Number(x), Scalar::Number(y)) => match (Decimal::parse(x), Decimal::parse(y)) {
            (Some(x), Some(y)) => x.cmp(&y),
            _ => x.cmp(y),
        },
        (Scalar::Boolean(x), Scalar::Boolean(y)) => x.cmp(y),
        _ => a.lexical().cmp(b.lexical()),
    }
}

/// The budget of a request (15.12): fuel used and its limit, shared by the computed inputs a
/// request evaluates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fuel {
    /// The limit.
    pub limit: u64,
    /// Fuel used so far.
    pub used: u64,
}

impl Fuel {
    /// A budget of `limit`.
    pub fn new(limit: u64) -> Fuel {
        Fuel { limit, used: 0 }
    }

    /// Charge `n`; `E_FORMULA_BUDGET` when the limit is exceeded.
    pub fn charge(&mut self, n: u64) -> Result<(), Code> {
        self.used = self.used.saturating_add(n);
        if self.used > self.limit {
            return Err(Code::Budget);
        }
        Ok(())
    }
}

/// What the evaluator needs from the mesh. Phase B implements it over the engine.
pub trait World {
    /// The function `name` is bound to as of the evaluation asof, and when that binding began.
    fn function(&mut self, name: &str) -> Option<Binding>;

    /// The version of `cell` answering as of the evaluation asof, or as of the transaction time
    /// `at` when given (`asof(cell, T)`). A computed cell is evaluated by the world, charging
    /// `fuel`; its faults are returned as errors. `None` when the cell has no value.
    fn read(&mut self, cell: &CellKey, at: Option<&str>, fuel: &mut Fuel) -> Result<Option<Version>, Code>;

    /// The members of a slice of a stored function as of the evaluation asof: each cell's
    /// arguments and its version answering as of the asof.
    fn members(&mut self, slice: &SliceKey, fuel: &mut Fuel) -> Result<Vec<(Args, Version)>, Code>;

    /// The versions of a stored cell with `tx` in `[from, to]`, as known as of the evaluation
    /// asof, ascending by `tx`.
    fn versions(&mut self, cell: &CellKey, from: &str, to: &str, fuel: &mut Fuel) -> Result<Vec<Version>, Code>;
}

/// A slice manifest (15.13): the slice object and its members' receipt hashes, ascending.
#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    /// The `inputs` entry `slice:<slice id>:<members hash>`.
    pub entry: String,
    /// The slice object (4.4).
    pub slice: Value,
    /// The member receipt hashes, ascending.
    pub members: Vec<String>,
}

impl Manifest {
    /// `{"members": [...], "slice": {...}}`.
    pub fn to_value(&self) -> Value {
        Value::Object(vec![
            ("members".to_string(), Value::Array(self.members.iter().map(|m| Value::String(m.clone())).collect())),
            ("slice".to_string(), self.slice.clone()),
        ])
    }
}

/// The result of evaluating a cell.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    /// The value.
    pub value: Scalar,
    /// The receipt's `inputs`: the receipt hashes and slice entries read, ascending.
    pub lineage: Vec<String>,
    /// The manifest of every slice entry.
    pub manifests: Vec<Manifest>,
    /// The latest `tx` among the input receipts (slice members included), if any.
    pub tx: Option<String>,
    /// The receipt `asof` of 8.2 rule 2.
    pub receipt_asof: String,
}

/// The smallest instant, the `tx` and `asof` of a receipt that read nothing.
pub const ZERO_INSTANT: &str = "0001-01-01T00:00:00.000000Z";

/// Evaluate a defined function at `args` as of `asof`.
pub fn evaluate(
    compiled: &Compiled,
    function: &Function,
    args: &Args,
    asof: &str,
    world: &mut dyn World,
    fuel: &mut Fuel,
) -> Result<Outcome, Code> {
    let a = calendar::parse_instant(asof).ok_or(Code::Domain)?;
    let mut scope = Vec::new();
    for (name, decl) in &function.signature.args {
        let s = args.get(name).ok_or(Code::Domain)?;
        if s.ty() != decl.ty {
            return Err(Code::Type);
        }
        scope.push((name.clone(), from_scalar(s)?));
    }
    let mut ev = Eval {
        c: compiled,
        world,
        fuel,
        asof: a,
        scope,
        lineage: BTreeSet::new(),
        manifests: BTreeMap::new(),
        max_tx: None,
        max_asof: None,
        bindings: BTreeMap::new(),
    };
    let v = ev.node(compiled.ast.root)?;
    let value = to_scalar(&v)?;
    let mut receipt_asof = ev.max_asof.clone();
    for start in ev.bindings.values() {
        if receipt_asof.as_deref().is_none_or(|r| start.as_str() > r) {
            receipt_asof = Some(start.clone());
        }
    }
    let receipt_asof = if compiled.asof_dependent {
        asof.to_string()
    } else {
        receipt_asof.unwrap_or_else(|| ZERO_INSTANT.to_string())
    };
    Ok(Outcome {
        value,
        lineage: ev.lineage.into_iter().collect(),
        manifests: ev.manifests.into_values().collect(),
        tx: ev.max_tx,
        receipt_asof,
    })
}

// -------------------------------------------------------------------------------------------
// Values.

/// A value inside a body: a scalar or a slice.
#[derive(Clone, Debug)]
enum V {
    Num(Decimal),
    Str(String),
    Bool(bool),
    Date(Date),
    Instant(Micros),
    Blob(String),
    Slice(Rc<SliceV>),
}

/// A slice value: its keys and its members, ascending by key tuple.
#[derive(Debug)]
struct SliceV {
    keys: Vec<KeyTy>,
    members: Vec<(Vec<V>, V)>,
}

fn from_scalar(s: &Scalar) -> Result<V, Code> {
    Ok(match s {
        Scalar::Number(n) => V::Num(Decimal::parse(n).ok_or(Code::Domain)?),
        Scalar::String(t) => V::Str(t.clone()),
        Scalar::Boolean(b) => V::Bool(*b),
        Scalar::Date(d) => V::Date(Date::parse(d).ok_or(Code::Domain)?),
        Scalar::Instant(i) => V::Instant(calendar::parse_instant(i).ok_or(Code::Domain)?),
        Scalar::Blob(b) => V::Blob(b.clone()),
    })
}

fn to_scalar(v: &V) -> Result<Scalar, Code> {
    let (t, s) = match v {
        V::Num(d) => (Type::Number, d.to_canonical()),
        V::Str(s) => (Type::String, s.clone()),
        V::Bool(b) => return Ok(Scalar::Boolean(*b)),
        V::Date(d) => (Type::Date, d.text()),
        V::Instant(i) => (Type::Instant, calendar::instant_text(*i).ok_or(Code::Range)?),
        V::Blob(b) => (Type::Blob, b.clone()),
        V::Slice(_) => return Err(Code::Type),
    };
    Scalar::from_lexical(t, &s).map_err(|_| Code::Range)
}

fn cmp_v(a: &V, b: &V) -> std::cmp::Ordering {
    use std::cmp::Ordering::Equal;
    match (a, b) {
        (V::Num(x), V::Num(y)) => x.cmp(y),
        (V::Str(x), V::Str(y)) => x.cmp(y),
        (V::Bool(x), V::Bool(y)) => x.cmp(y),
        (V::Date(x), V::Date(y)) => x.cmp(y),
        (V::Instant(x), V::Instant(y)) => x.cmp(y),
        (V::Blob(x), V::Blob(y)) => x.cmp(y),
        _ => Equal,
    }
}

fn eq_v(a: &V, b: &V) -> bool {
    cmp_v(a, b) == std::cmp::Ordering::Equal
}

fn cmp_keys(a: &[V], b: &[V]) -> std::cmp::Ordering {
    for (x, y) in a.iter().zip(b) {
        match cmp_v(x, y) {
            std::cmp::Ordering::Equal => {}
            o => return o,
        }
    }
    a.len().cmp(&b.len())
}

fn num(v: &V) -> Result<&Decimal, Code> {
    match v {
        V::Num(d) => Ok(d),
        _ => Err(Code::Type),
    }
}

fn int_of(v: &V) -> Result<i64, Code> {
    let d = num(v)?;
    if !d.is_integer() {
        return Err(Code::Domain);
    }
    d.to_i64().ok_or(Code::Domain)
}

fn int(n: i64) -> Decimal {
    Decimal::from_i64(n)
}

/// Microseconds per unit of a time unit (bare: seconds).
fn micros_per(u: Option<&NumUnit>) -> i64 {
    match u.and_then(|u| match u {
        NumUnit::Known(x) => x.single().map(str::to_string),
        _ => None,
    }) {
        Some(s) if s == "min" => 60_000_000,
        Some(s) if s == "h" => 3_600_000_000,
        Some(s) if s == "d" => DAY_MICROS,
        _ => 1_000_000,
    }
}

fn num_unit(t: Option<&Ty>) -> Option<&NumUnit> {
    match t {
        Some(Ty::Num(u)) => Some(u),
        Some(Ty::Slice(e, _)) => match &**e {
            Ty::Num(u) => Some(u),
            _ => None,
        },
        _ => None,
    }
}

/// A duration in microseconds, exact, from a number in `u`.
fn duration_micros(d: &Decimal, u: Option<&NumUnit>) -> Decimal {
    decimal::mul_exact(d, &int(micros_per(u)))
}

fn date_of_days(d: Date, n: i64) -> Result<Date, Code> {
    Date::from_ordinal(d.ordinal().checked_add(n).ok_or(Code::Range)?).ok_or(Code::Range)
}

fn instant_ok(us: i128) -> Result<Micros, Code> {
    if us < i128::from(calendar::min_instant()) || us > i128::from(calendar::max_instant()) {
        return Err(Code::Range);
    }
    Ok(us as Micros)
}

/// Seconds between two instants, exact.
fn seconds(a: Micros, b: Micros) -> Decimal {
    let diff = Decimal::from_i128(i128::from(a) - i128::from(b));
    diff.scale10(-6)
}

// -------------------------------------------------------------------------------------------
// The evaluator.

struct Eval<'a> {
    c: &'a Compiled,
    world: &'a mut dyn World,
    fuel: &'a mut Fuel,
    asof: Micros,
    scope: Vec<(String, V)>,
    lineage: BTreeSet<String>,
    manifests: BTreeMap<String, Manifest>,
    max_tx: Option<String>,
    max_asof: Option<String>,
    bindings: BTreeMap<String, String>,
}

/// A narrowing of a lookup's slice read along its dimension (15.13).
struct Narrow {
    key: String,
    lo: V,
    hi: V,
}

impl Eval<'_> {
    fn charge(&mut self, n: u64) -> Result<(), Code> {
        self.fuel.charge(n)
    }

    fn ty(&self, id: NodeId) -> Option<&Ty> {
        self.c.type_of(id)
    }

    fn node(&mut self, id: NodeId) -> Result<V, Code> {
        let ast = &self.c.ast;
        // Binary spines, iteratively: charge every node of the spine top-down, then evaluate the
        // bottom operand and each right operand in turn (left to right).
        if let Node::Op { op, args } = ast.node(id) {
            if args.len() == 2 && *op != Op::Pow {
                let mut spine = Vec::new();
                let mut cur = id;
                while let Node::Op { op, args } = ast.node(cur) {
                    if args.len() != 2 || *op == Op::Pow {
                        break;
                    }
                    self.charge(1)?;
                    spine.push((cur, *op, args[0], args[1]));
                    cur = args[0];
                }
                let mut acc = self.node(cur)?;
                for (nid, op, l, r) in spine.into_iter().rev() {
                    acc = self.binary(nid, op, acc, l, r)?;
                }
                return Ok(acc);
            }
        }
        self.charge(1)?;
        match ast.node(id) {
            Node::Num { value, .. } => Ok(V::Num(Decimal::parse(value).ok_or(Code::Domain)?)),
            Node::Str(s) => Ok(V::Str(s.clone())),
            Node::Bool(b) => Ok(V::Bool(*b)),
            Node::Date(s) => Ok(V::Date(Date::parse(s).ok_or(Code::Domain)?)),
            Node::Instant(s) => Ok(V::Instant(calendar::parse_instant(s).ok_or(Code::Domain)?)),
            Node::Var(n) => self.scope.iter().rev().find(|(m, _)| m == n).map(|(_, v)| v.clone()).ok_or(Code::Domain),
            Node::Op { op, args } => match op {
                Op::Neg => {
                    // A chain of minus signs, iteratively.
                    let mut n = 1u64;
                    let mut cur = args[0];
                    while let Node::Op { op: Op::Neg, args } = ast.node(cur) {
                        self.charge(1)?;
                        n += 1;
                        cur = args[0];
                    }
                    let mut v = self.node(cur)?;
                    for _ in 0..n {
                        v = self.neg(v)?;
                    }
                    Ok(v)
                }
                Op::Not => match self.node(args[0])? {
                    V::Bool(b) => Ok(V::Bool(!b)),
                    _ => Err(Code::Type),
                },
                Op::Pow => {
                    let b = self.node(args[0])?;
                    let x = self.node(args[1])?;
                    let (b, x) = (num(&b)?, num(&x)?);
                    if x.is_integer() {
                        let n = x.to_i64().unwrap_or(i64::MAX);
                        if n.unsigned_abs() <= 1024 {
                            self.charge(n.unsigned_abs())?;
                        }
                        Ok(V::Num(decimal::power_int(b, x)?))
                    } else {
                        self.charge(130)?;
                        Ok(V::Num(decimal::pow_expln(b, x)?))
                    }
                }
                _ => unreachable!("binary operators are evaluated as spines"),
            },
            Node::If { cond, then, els } => match self.node(*cond)? {
                V::Bool(true) => self.node(*then),
                V::Bool(false) => self.node(*els),
                _ => Err(Code::Type),
            },
            Node::Let { name, value, body } => {
                let v = self.node(*value)?;
                self.scope.push((name.clone(), v));
                let r = self.node(*body);
                self.scope.pop();
                r
            }
            Node::Read { .. } => self.read(id, None),
            Node::Call { fn_name, args, named } => self.call(id, fn_name, args, named),
            Node::Lambda { .. } | Node::Agg(_) | Node::Key(_) => Err(Code::Type),
        }
    }

    fn neg(&self, v: V) -> Result<V, Code> {
        match v {
            V::Num(d) => Ok(V::Num(decimal::negate(&d)?)),
            V::Slice(s) => {
                let mut members = Vec::with_capacity(s.members.len());
                for (k, m) in &s.members {
                    members.push((k.clone(), V::Num(decimal::negate(num(m)?)?)));
                }
                Ok(V::Slice(Rc::new(SliceV { keys: s.keys.clone(), members })))
            }
            _ => Err(Code::Type),
        }
    }

    /// A binary operator node `nid` with its left value and its operand nodes.
    fn binary(&mut self, nid: NodeId, op: Op, l: V, lid: NodeId, rid: NodeId) -> Result<V, Code> {
        match op {
            Op::And | Op::Or => {
                let lb = matches!(l, V::Bool(true));
                if !matches!(l, V::Bool(_)) {
                    return Err(Code::Type);
                }
                if (op == Op::And && !lb) || (op == Op::Or && lb) {
                    return Ok(V::Bool(lb));
                }
                match self.node(rid)? {
                    V::Bool(b) => Ok(V::Bool(b)),
                    _ => Err(Code::Type),
                }
            }
            _ => {
                let r = self.node(rid)?;
                let _ = nid;
                let lu = num_unit(self.ty(lid)).cloned();
                let ru = num_unit(self.ty(rid)).cloned();
                self.apply(op, &l, &r, lu.as_ref(), ru.as_ref())
            }
        }
    }

    /// An operator over two values; slices join (15.13).
    fn apply(&self, op: Op, l: &V, r: &V, lu: Option<&NumUnit>, ru: Option<&NumUnit>) -> Result<V, Code> {
        match (l, r) {
            (V::Slice(a), V::Slice(b)) => {
                // An inner join on keys.
                let mut members = Vec::new();
                let mut j = 0;
                for (k, x) in &a.members {
                    while j < b.members.len() && cmp_keys(&b.members[j].0, k) == std::cmp::Ordering::Less {
                        j += 1;
                    }
                    if j < b.members.len() && cmp_keys(&b.members[j].0, k) == std::cmp::Ordering::Equal {
                        members.push((k.clone(), scalar_op(op, x, &b.members[j].1, lu, ru)?));
                    }
                }
                Ok(V::Slice(Rc::new(SliceV { keys: a.keys.clone(), members })))
            }
            (V::Slice(a), s) => {
                let mut members = Vec::with_capacity(a.members.len());
                for (k, x) in &a.members {
                    members.push((k.clone(), scalar_op(op, x, s, lu, ru)?));
                }
                Ok(V::Slice(Rc::new(SliceV { keys: a.keys.clone(), members })))
            }
            (s, V::Slice(b)) => {
                let mut members = Vec::with_capacity(b.members.len());
                for (k, x) in &b.members {
                    members.push((k.clone(), scalar_op(op, s, x, lu, ru)?));
                }
                Ok(V::Slice(Rc::new(SliceV { keys: b.keys.clone(), members })))
            }
            _ => scalar_op(op, l, r, lu, ru),
        }
    }

    // ---------------------------------------------------------------------------------------
    // Reads.

    fn binding(&mut self, name: &str) -> Result<Binding, Code> {
        let b = self.world.function(name).ok_or(Code::Unresolved)?;
        self.bindings.insert(name.to_string(), b.start.clone());
        Ok(b)
    }

    fn note_version(&mut self, v: &Version) {
        if self.max_tx.as_deref().is_none_or(|t| v.tx.as_str() > t) {
            self.max_tx = Some(v.tx.clone());
        }
        self.note_asof(&v.asof);
    }

    fn note_asof(&mut self, a: &str) {
        if self.max_asof.as_deref().is_none_or(|t| a > t) {
            self.max_asof = Some(a.to_string());
        }
    }

    /// The arguments of a read node, evaluated in ascending label order.
    fn read_args(&mut self, id: NodeId, f: &Function) -> Result<BTreeMap<String, PatternArg>, Code> {
        let Node::Read { args, .. } = self.c.ast.node(id) else { return Err(Code::Type) };
        let mut out = BTreeMap::new();
        for (name, a) in args {
            let ty = f.signature.args.get(name).map(|d| d.ty).ok_or(Code::Unresolved)?;
            let p = match a {
                ReadArg::Open => PatternArg::Open,
                ReadArg::Expr(x) => PatternArg::Value(typed(&self.node(*x)?, ty)?),
                ReadArg::Range(lo, hi) => {
                    let lo = typed(&self.node(*lo)?, ty)?;
                    let hi = typed(&self.node(*hi)?, ty)?;
                    if cmp_scalar(&lo, &hi) == std::cmp::Ordering::Greater {
                        return Err(Code::Domain);
                    }
                    PatternArg::Range(lo, hi)
                }
            };
            out.insert(name.clone(), p);
        }
        Ok(out)
    }

    /// A read node: one cell, or a slice (narrowed by a lookup's bound when given).
    fn read(&mut self, id: NodeId, narrow: Option<Narrow>) -> Result<V, Code> {
        let Node::Read { fn_name, .. } = self.c.ast.node(id) else { return Err(Code::Type) };
        let fn_name = fn_name.clone();
        let b = self.binding(&fn_name)?;
        let mut pattern = self.read_args(id, &b.function)?;
        if let Some(n) = narrow {
            let ty = b.function.signature.args.get(&n.key).map(|d| d.ty).ok_or(Code::Along)?;
            let (lo, hi) = (typed(&n.lo, ty)?, typed(&n.hi, ty)?);
            let (lo, hi) = match pattern.get(&n.key) {
                Some(PatternArg::Range(a, z)) => (
                    if cmp_scalar(a, &lo) == std::cmp::Ordering::Greater { a.clone() } else { lo },
                    if cmp_scalar(z, &hi) == std::cmp::Ordering::Less { z.clone() } else { hi },
                ),
                _ => (lo, hi),
            };
            pattern.insert(n.key.clone(), PatternArg::Range(lo, hi));
        }
        let open = pattern.values().any(|p| !matches!(p, PatternArg::Value(_)));
        if !open {
            self.charge(1)?;
            let args: Args = pattern
                .into_iter()
                .map(|(k, p)| match p {
                    PatternArg::Value(v) => (k, v),
                    _ => unreachable!("closed pattern"),
                })
                .collect();
            let cell = CellKey { name: fn_name, args };
            return match self.world.read(&cell, None, self.fuel)? {
                Some(v) => {
                    self.lineage.insert(v.receipt_hash.clone());
                    self.note_version(&v);
                    from_scalar(&v.value)
                }
                None => Err(Code::NotCovered),
            };
        }
        let slice = SliceKey { name: fn_name.clone(), args: pattern };
        let f = b.function.clone();
        let members: Vec<(Args, Version)> = if f.lang == "stored" {
            self.world.members(&slice, self.fuel)?
        } else {
            let cells = enumerate(&f, &slice)?;
            let mut out = Vec::with_capacity(cells.len());
            for args in cells {
                let cell = CellKey { name: fn_name.clone(), args };
                match self.world.read(&cell, None, self.fuel) {
                    Ok(Some(v)) => out.push((cell.args, v)),
                    Ok(None) => return Err(Code::Absent),
                    Err(c) if c.is_mesh_failure() => return Err(c),
                    Err(_) => return Err(Code::Absent),
                }
            }
            out
        };
        self.charge(1 + members.len() as u64)?;
        // Lineage: one entry for the slice.
        let obj = slice_object(&f, &slice);
        let mut hashes: Vec<String> = members.iter().map(|(_, v)| v.receipt_hash.clone()).collect();
        hashes.sort();
        hashes.dedup();
        let entry = slice_entry(&obj, &hashes);
        self.manifests.insert(entry.clone(), Manifest { entry: entry.clone(), slice: obj, members: hashes });
        self.lineage.insert(entry);
        for (_, v) in &members {
            self.note_version(v);
        }
        // The slice value: keys are the open and ranged arguments, ascending.
        let mut keys = Vec::new();
        for (k, p) in &slice.args {
            if !matches!(p, PatternArg::Value(_)) {
                let d = &f.signature.args[k];
                keys.push(KeyTy { name: k.clone(), ty: d.ty, unit: None, valid: d.valid });
            }
        }
        let mut ms = Vec::with_capacity(members.len());
        for (args, v) in &members {
            let mut key = Vec::with_capacity(keys.len());
            for k in &keys {
                key.push(from_scalar(args.get(&k.name).ok_or(Code::Absent)?)?);
            }
            ms.push((key, from_scalar(&v.value)?));
        }
        ms.sort_by(|a, b| cmp_keys(&a.0, &b.0));
        Ok(V::Slice(Rc::new(SliceV { keys, members: ms })))
    }

    // ---------------------------------------------------------------------------------------
    // Built-ins.

    fn call(&mut self, id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<V, Code> {
        let named_of = |n: &str| named.iter().find(|(k, _)| k == n).map(|(_, v)| *v);
        if AGGREGATES.contains(&f) {
            return self.aggregate_call(f, args);
        }
        match f {
            "at" | "latest" | "last_before" | "first_after" | "nearest" | "within" | "interpolate" => {
                self.lookup(id, f, args, named)
            }
            "map" | "filter" => self.map_filter(f, args),
            "scan" | "window" => self.scan_window(f, args, named),
            "otherwise" => match self.node(args[0]) {
                Err(c) if c.is_catchable() => self.node(args[1]),
                r => r,
            },
            "asof" => {
                if args.is_empty() {
                    return Ok(V::Instant(self.asof));
                }
                self.asof_read(args[0], args[1])
            }
            "versions" => self.versions(args),
            "abs" => Ok(V::Num(num(&self.node(args[0])?)?.abs().check_range()?)),
            "floor" => Ok(V::Num(num(&self.node(args[0])?)?.floor().check_range()?)),
            "ceiling" => Ok(V::Num(num(&self.node(args[0])?)?.ceiling().check_range()?)),
            "round" | "round_up" | "round_down" => {
                let x = self.node(args[0])?;
                let digits = match named_of("digits") {
                    Some(d) => self.node(d)?,
                    None => V::Num(Decimal::zero()),
                };
                let mode = match f {
                    "round" => RoundMode::Nearest,
                    "round_up" => RoundMode::Up,
                    _ => RoundMode::Down,
                };
                Ok(V::Num(decimal::round_digits(num(&x)?, num(&digits)?, mode)?))
            }
            "mod" => {
                let x = self.node(args[0])?;
                let d = self.node(args[1])?;
                Ok(V::Num(decimal::modulo(num(&x)?, num(&d)?)?))
            }
            "sqrt" => {
                let x = self.node(args[0])?;
                self.charge(10)?;
                Ok(V::Num(decimal::sqrt(num(&x)?)?))
            }
            "exp" => {
                let x = self.node(args[0])?;
                self.charge(50)?;
                Ok(V::Num(decimal::exp(num(&x)?)?))
            }
            "ln" => {
                let x = self.node(args[0])?;
                self.charge(80)?;
                Ok(V::Num(decimal::ln(num(&x)?)?))
            }
            "sin" | "cos" => {
                let x = self.node(args[0])?;
                self.charge(50)?;
                let mut r = num(&x)?.clone();
                if num_unit(self.ty(args[0])).and_then(|u| u.text().into()).as_deref() == Some("deg") {
                    r = decimal::radians40(&r);
                }
                let (s, c) = decimal::sincos40(&r)?;
                let v = if f == "sin" { s } else { c };
                Ok(V::Num(decimal::rnd(v, decimal::PRECISION).check_range()?))
            }
            "atan2" => {
                let y = self.node(args[0])?;
                let x = self.node(args[1])?;
                self.charge(60)?;
                Ok(V::Num(decimal::atan2(num(&y)?, num(&x)?)?))
            }
            "pi" => Ok(V::Num(decimal::pi())),
            "haversine" => {
                let mut v = Vec::with_capacity(4);
                for a in args {
                    v.push(num(&self.node(*a)?)?.clone());
                }
                self.charge(400)?;
                Ok(V::Num(decimal::haversine(&v[0], &v[1], &v[2], &v[3])?))
            }
            "convert" => {
                let x = self.node(args[0])?;
                let x = num(&x)?;
                let target = match self.c.ast.node(args[1]) {
                    Node::Str(s) => s.clone(),
                    _ => return Err(Code::Enum),
                };
                let from = num_unit(self.ty(args[0])).cloned().unwrap_or(NumUnit::Known(crate::units::Unit::one()));
                let kind = check::convert_factor(&from, &target).ok_or(Code::UnitMismatch)?;
                Ok(V::Num(convert(x, kind)?))
            }
            "date" => {
                let y = int_of(&self.node(args[0])?)?;
                let m = int_of(&self.node(args[1])?)?;
                let d = int_of(&self.node(args[2])?)?;
                Ok(V::Date(Date::new(y, m, d).ok_or(Code::Domain)?))
            }
            "year" | "month" | "day" | "weekday" | "end_of_month" => {
                let V::Date(d) = self.node(args[0])? else { return Err(Code::Type) };
                Ok(match f {
                    "year" => V::Num(int(i64::from(d.year))),
                    "month" => V::Num(int(i64::from(d.month))),
                    "day" => V::Num(int(i64::from(d.day))),
                    "weekday" => V::Num(int(i64::from(d.weekday()))),
                    _ => V::Date(d.end_of_month()),
                })
            }
            "add_days" | "add_months" | "business_days" => self.business(f, args, named),
            "year_fraction" => {
                let V::Date(a) = self.node(args[0])? else { return Err(Code::Type) };
                let V::Date(b) = self.node(args[1])? else { return Err(Code::Type) };
                let Node::Str(conv) = self.c.ast.node(args[2]) else { return Err(Code::Enum) };
                Ok(V::Num(calendar::year_fraction(a, b, conv)?))
            }
            "is_holiday" => {
                let V::Date(d) = self.node(args[0])? else { return Err(Code::Type) };
                let hol = self.holidays(args[1])?;
                Ok(V::Bool(hol.binary_search(&d).is_ok()))
            }
            "date_of" => {
                let V::Instant(i) = self.node(args[0])? else { return Err(Code::Type) };
                Ok(V::Date(calendar::date_of(i).ok_or(Code::Range)?))
            }
            "instant" => {
                let V::Date(d) = self.node(args[0])? else { return Err(Code::Type) };
                Ok(V::Instant(instant_ok(i128::from(d.ordinal()) * i128::from(DAY_MICROS))?))
            }
            "len" => {
                let V::Str(s) = self.node(args[0])? else { return Err(Code::Type) };
                Ok(V::Num(int(text::len(&s) as i64)))
            }
            "upper" | "lower" | "trim" => {
                let V::Str(s) = self.node(args[0])? else { return Err(Code::Type) };
                let r = match f {
                    "upper" => text::upper(&s),
                    "lower" => text::lower(&s),
                    _ => text::trim(&s),
                };
                let r = text::finish(r)?;
                self.charge(1 + r.len() as u64 / 64)?;
                Ok(V::Str(r))
            }
            "concat" => {
                let mut out = String::new();
                for a in args {
                    let V::Str(s) = self.node(*a)? else { return Err(Code::Type) };
                    out.push_str(&s);
                    if out.len() > 4 * text::MAX_STRING_BYTES {
                        return Err(Code::Range);
                    }
                }
                let r = text::finish(out)?;
                self.charge(1 + r.len() as u64 / 64)?;
                Ok(V::Str(r))
            }
            "substr" => {
                let V::Str(s) = self.node(args[0])? else { return Err(Code::Type) };
                let start = int_of(&self.node(args[1])?)?;
                let length = int_of(&self.node(args[2])?)?;
                Ok(V::Str(text::finish(text::substr(&s, start, length)?)?))
            }
            "find" => {
                let V::Str(needle) = self.node(args[0])? else { return Err(Code::Type) };
                let V::Str(hay) = self.node(args[1])? else { return Err(Code::Type) };
                let start = match named_of("start") {
                    Some(s) => int_of(&self.node(s)?)?,
                    None => 1,
                };
                Ok(V::Num(int(text::find(&needle, &hay, start)?)))
            }
            "text" => {
                let v = self.node(args[0])?;
                let s = to_scalar(&v)?;
                Ok(V::Str(text::finish(s.lexical().to_string())?))
            }
            "fixed" => {
                let x = self.node(args[0])?;
                let d = self.node(args[1])?;
                Ok(V::Str(text::finish(fixed(num(&x)?, num(&d)?)?)?))
            }
            _ => Err(Code::UnknownFunction),
        }
    }

    fn aggregate_call(&mut self, f: &str, args: &[NodeId]) -> Result<V, Code> {
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.node(*a)?);
        }
        let values: Vec<V> = if vals.len() == 1 {
            match &vals[0] {
                V::Slice(s) => s.members.iter().map(|(_, v)| v.clone()).collect(),
                v => vec![v.clone()],
            }
        } else {
            vals
        };
        let n = values.len() as u64;
        self.charge(if f == "median" { n * (64 - n.leading_zeros() as u64) } else { n })?;
        let elem_unit = num_unit(self.ty(args[0])).cloned();
        aggregate(f, &values, elem_unit.as_ref())
    }

    fn slice_of(&mut self, id: NodeId) -> Result<Rc<SliceV>, Code> {
        match self.node(id)? {
            V::Slice(s) => Ok(s),
            _ => Err(Code::Type),
        }
    }

    fn keys_of(&self, id: NodeId) -> Vec<KeyTy> {
        match self.ty(id) {
            Some(Ty::Slice(_, k)) => k.clone(),
            _ => Vec::new(),
        }
    }

    fn along(&self, named: &[(String, NodeId)]) -> Option<String> {
        named.iter().find(|(k, _)| k == "along").and_then(|(_, v)| match self.c.ast.node(*v) {
            Node::Key(k) => Some(k.clone()),
            _ => None,
        })
    }

    fn lookup(&mut self, _id: NodeId, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<V, Code> {
        let keys = self.keys_of(args[0]);
        let along = self.along(named);
        let d = check::dimension(&keys, along.as_deref()).map_err(|e| e.code)?;
        let dim = keys[d].clone();
        let bound_id = if f == "within" { Some(args[2]) } else { named.iter().find(|(k, _)| k == "bound").map(|(_, v)| *v) };
        let method = named.iter().find(|(k, _)| k == "method").and_then(|(_, v)| match self.c.ast.node(*v) {
            Node::Str(s) => Some(s.clone()),
            _ => None,
        });
        let is_read = matches!(self.c.ast.node(args[0]), Node::Read { .. });
        // Evaluation: with a bound over a read, the bound narrows the read (15.13).
        let (s, x, bound) = if is_read && bound_id.is_some() {
            let x = if f == "latest" { None } else { Some(self.node(args[1])?) };
            let bv = self.node(bound_id.unwrap_or(0))?;
            let bu = num_unit(self.ty(bound_id.unwrap_or(0))).cloned();
            let narrow = self.narrow(f, &dim, x.as_ref(), &bv, bu.as_ref())?;
            self.charge(1)?; // the read node
            let s = match self.read(args[0], Some(narrow))? {
                V::Slice(s) => s,
                _ => return Err(Code::Type),
            };
            (s, x, Some((bv, bu)))
        } else {
            let s = self.slice_of(args[0])?;
            let x = if f == "latest" { None } else { Some(self.node(args[1])?) };
            let bound = match bound_id {
                Some(b) if f == "within" => {
                    let bv = self.node(b)?;
                    Some((bv, num_unit(self.ty(b)).cloned()))
                }
                Some(b) => {
                    let bv = self.node(b)?;
                    Some((bv, num_unit(self.ty(b)).cloned()))
                }
                None => None,
            };
            (s, x, bound)
        };
        self.charge(s.members.len() as u64)?;
        // The bound as an exact distance along the dimension.
        let reach = match &bound {
            Some((bv, bu)) => Some(distance_of(&dim, num(bv)?, bu.as_ref())),
            None => None,
        };
        // Groups: the members by the other keys, in key order.
        let mut groups: Vec<(Vec<V>, Vec<(V, V)>)> = Vec::new();
        for (key, val) in &s.members {
            let g: Vec<V> = key.iter().enumerate().filter(|(i, _)| *i != d).map(|(_, v)| v.clone()).collect();
            let kv = key[d].clone();
            match groups.iter_mut().find(|(gk, _)| cmp_keys(gk, &g) == std::cmp::Ordering::Equal) {
                Some((_, ms)) => ms.push((kv, val.clone())),
                None => groups.push((g, vec![(kv, val.clone())])),
            }
        }
        groups.sort_by(|a, b| cmp_keys(&a.0, &b.0));
        for (_, ms) in &mut groups {
            ms.sort_by(|a, b| cmp_v(&a.0, &b.0));
        }
        if f == "within" {
            let x = x.ok_or(Code::Type)?;
            let reach = reach.ok_or(Code::Type)?;
            let mut members = Vec::new();
            for (key, val) in &s.members {
                if distance(&key[d], &x)?.is_some_and(|dist| dist <= reach) {
                    members.push((key.clone(), val.clone()));
                }
            }
            return Ok(V::Slice(Rc::new(SliceV { keys: s.keys.clone(), members })));
        }
        let mut results: Vec<(Vec<V>, V)> = Vec::new();
        for (g, ms) in &groups {
            if let Some(v) = self.lookup_one(f, ms, x.as_ref(), reach.as_ref(), method.as_deref(), &dim)? {
                results.push((g.clone(), v));
            }
        }
        if keys.len() == 1 {
            return match results.into_iter().next() {
                Some((_, v)) => Ok(v),
                None => Err(Code::Absent),
            };
        }
        let mut rest = s.keys.clone();
        rest.remove(d);
        Ok(V::Slice(Rc::new(SliceV { keys: rest, members: results })))
    }

    /// The range a bound narrows a lookup's read to, ends rounded inward and clamped.
    fn narrow(&self, f: &str, dim: &KeyTy, x: Option<&V>, b: &V, bu: Option<&NumUnit>) -> Result<Narrow, Code> {
        let b = num(b)?;
        let (lo, hi) = match dim.ty {
            Type::Instant => {
                let reach = duration_micros(b, bu);
                let down = reach.floor().to_i64().unwrap_or(i64::MAX);
                let (lo, hi): (i128, i128) = match (f, x) {
                    ("latest", _) => (i128::from(self.asof) - i128::from(down), i128::from(calendar::max_instant())),
                    (_, Some(V::Instant(x))) => {
                        let x = i128::from(*x);
                        match f {
                            "last_before" => (x - i128::from(down), x),
                            "first_after" => (x, x + i128::from(down)),
                            _ => (x - i128::from(down), x + i128::from(down)),
                        }
                    }
                    _ => return Err(Code::Type),
                };
                let clamp = |v: i128| v.clamp(i128::from(calendar::min_instant()), i128::from(calendar::max_instant())) as i64;
                (V::Instant(clamp(lo)), V::Instant(clamp(hi)))
            }
            Type::Date => {
                let Some(V::Date(x)) = x else { return Err(Code::Metric) };
                let days = b.floor().to_i64().unwrap_or(i64::MAX / 4).min(4_000_000);
                let o = x.ordinal();
                let (lo, hi) = match f {
                    "last_before" => (o - days, o),
                    "first_after" => (o, o + days),
                    _ => (o - days, o + days),
                };
                let min = Date { year: 1, month: 1, day: 1 }.ordinal();
                let max = Date { year: 9999, month: 12, day: 31 }.ordinal();
                (
                    V::Date(Date::from_ordinal(lo.clamp(min, max)).ok_or(Code::Range)?),
                    V::Date(Date::from_ordinal(hi.clamp(min, max)).ok_or(Code::Range)?),
                )
            }
            Type::Number => {
                let Some(V::Num(x)) = x else { return Err(Code::Metric) };
                let lo = decimal::sub_exact(x, b);
                let hi = decimal::add_exact(x, b);
                match f {
                    "last_before" => (V::Num(lo), V::Num(x.clone())),
                    "first_after" => (V::Num(x.clone()), V::Num(hi)),
                    _ => (V::Num(lo), V::Num(hi)),
                }
            }
            _ => return Err(Code::Metric),
        };
        Ok(Narrow { key: dim.name.clone(), lo, hi })
    }

    /// One group's lookup over members ascending along the dimension.
    fn lookup_one(
        &mut self,
        f: &str,
        ms: &[(V, V)],
        x: Option<&V>,
        reach: Option<&Decimal>,
        method: Option<&str>,
        dim: &KeyTy,
    ) -> Result<Option<V>, Code> {
        let within = |k: &V| -> Result<bool, Code> {
            match (reach, x) {
                (Some(r), Some(x)) => Ok(distance(k, x)?.is_some_and(|d| d <= *r)),
                _ => Ok(true),
            }
        };
        match f {
            "at" => {
                let x = x.ok_or(Code::Type)?;
                Ok(ms.iter().find(|(k, _)| eq_v(k, x)).map(|(_, v)| v.clone()))
            }
            "latest" => {
                let Some((k, v)) = ms.last() else { return Ok(None) };
                if let Some(r) = reach {
                    // Older than asof() - bound: none.
                    let V::Instant(k) = k else { return Err(Code::Metric) };
                    let age = Decimal::from_i128(i128::from(self.asof) - i128::from(*k));
                    if age > *r {
                        return Ok(None);
                    }
                }
                Ok(Some(v.clone()))
            }
            "last_before" => {
                let x = x.ok_or(Code::Type)?;
                for (k, v) in ms.iter().rev() {
                    if cmp_v(k, x) != std::cmp::Ordering::Greater {
                        return Ok(if within(k)? { Some(v.clone()) } else { None });
                    }
                }
                Ok(None)
            }
            "first_after" => {
                let x = x.ok_or(Code::Type)?;
                for (k, v) in ms {
                    if cmp_v(k, x) != std::cmp::Ordering::Less {
                        return Ok(if within(k)? { Some(v.clone()) } else { None });
                    }
                }
                Ok(None)
            }
            "nearest" => {
                let x = x.ok_or(Code::Type)?;
                let mut best: Option<(Decimal, &V)> = None;
                for (k, v) in ms {
                    let Some(d) = distance(k, x)? else { return Err(Code::Metric) };
                    if reach.is_some_and(|r| d > *r) {
                        continue;
                    }
                    if best.as_ref().is_none_or(|(b, _)| d < *b) {
                        best = Some((d, v));
                    }
                }
                Ok(best.map(|(_, v)| v.clone()))
            }
            "interpolate" => {
                let x = x.ok_or(Code::Type)?;
                if let Some((_, v)) = ms.iter().find(|(k, _)| eq_v(k, x)) {
                    return Ok(Some(v.clone()));
                }
                let before = ms.iter().rev().find(|(k, _)| cmp_v(k, x) == std::cmp::Ordering::Less);
                let after = ms.iter().find(|(k, _)| cmp_v(k, x) == std::cmp::Ordering::Greater);
                let (Some((k0, v0)), Some((k1, v1))) = (before, after) else { return Ok(None) };
                if !within(k0)? || !within(k1)? {
                    return Ok(None);
                }
                if method == Some("step") {
                    return Ok(Some(v0.clone()));
                }
                let (v0, v1) = (num(v0)?, num(v1)?);
                let pos = |a: &V, b: &V| -> Result<Decimal, Code> {
                    Ok(match (a, b) {
                        (V::Instant(a), V::Instant(b)) => seconds(*a, *b),
                        (V::Date(a), V::Date(b)) => int(a.ordinal() - b.ordinal()),
                        (V::Num(a), V::Num(b)) => decimal::sub(a, b)?,
                        _ => return Err(Code::Metric),
                    })
                };
                let _ = dim;
                let dv = decimal::sub(v1, v0)?;
                let dx = pos(x, k0)?;
                let span = pos(k1, k0)?;
                let r = decimal::add(v0, &decimal::div(&decimal::mul(&dv, &dx)?, &span)?)?;
                Ok(Some(V::Num(r)))
            }
            _ => Err(Code::Type),
        }
    }

    fn map_filter(&mut self, f: &str, args: &[NodeId]) -> Result<V, Code> {
        let s = self.slice_of(args[0])?;
        let Node::Lambda { params, body } = self.c.ast.node(args[1]) else { return Err(Code::Type) };
        let (params, body) = (params.clone(), *body);
        let mut members = Vec::with_capacity(s.members.len());
        for (key, val) in &s.members {
            let base = self.scope.len();
            self.scope.push((params[0].clone(), val.clone()));
            for p in &params[1..] {
                let i = s.keys.iter().position(|k| &k.name == p).ok_or(Code::LambdaParam)?;
                self.scope.push((p.clone(), key[i].clone()));
            }
            let r = self.node(body);
            self.scope.truncate(base);
            let r = r?;
            if f == "filter" {
                if matches!(r, V::Bool(true)) {
                    members.push((key.clone(), val.clone()));
                }
            } else {
                members.push((key.clone(), r));
            }
        }
        Ok(V::Slice(Rc::new(SliceV { keys: s.keys.clone(), members })))
    }

    fn scan_window(&mut self, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<V, Code> {
        let keys = self.keys_of(args[0]);
        let s = self.slice_of(args[0])?;
        let width = if f == "window" {
            let w = int_of(&self.node(args[1])?)?;
            if w < 1 {
                return Err(Code::Domain);
            }
            Some(w as usize)
        } else {
            None
        };
        let agg_id = if f == "scan" { args[1] } else { args[2] };
        let Node::Agg(agg) = self.c.ast.node(agg_id) else { return Err(Code::Enum) };
        let agg = agg.clone();
        let along = self.along(named);
        let d = check::dimension(&keys, along.as_deref()).map_err(|e| e.code)?;
        let n = s.members.len() as u64;
        // scan over n: n; window over n, width w: w for each member with w - 1 predecessors.
        self.charge(match width {
            Some(w) => n.saturating_sub(w as u64 - 1).saturating_mul(w as u64),
            None => n,
        })?;
        let unit = num_unit(self.ty(args[0])).cloned();
        // Groups by the other keys; each group in dimension order.
        let mut groups: Vec<(Vec<V>, Vec<(Vec<V>, V)>)> = Vec::new();
        for (key, val) in &s.members {
            let g: Vec<V> = key.iter().enumerate().filter(|(i, _)| *i != d).map(|(_, v)| v.clone()).collect();
            match groups.iter_mut().find(|(gk, _)| cmp_keys(gk, &g) == std::cmp::Ordering::Equal) {
                Some((_, ms)) => ms.push((key.clone(), val.clone())),
                None => groups.push((g, vec![(key.clone(), val.clone())])),
            }
        }
        let mut members = Vec::with_capacity(s.members.len());
        for (_, mut ms) in groups {
            ms.sort_by(|a, b| cmp_v(&a.0[d], &b.0[d]));
            let vals: Vec<V> = ms.iter().map(|(_, v)| v.clone()).collect();
            for i in 0..ms.len() {
                let from = match width {
                    Some(w) => {
                        if i + 1 < w {
                            continue;
                        }
                        i + 1 - w
                    }
                    None => 0,
                };
                let r = aggregate(&agg, &vals[from..=i], unit.as_ref())?;
                members.push((ms[i].0.clone(), r));
            }
        }
        members.sort_by(|a, b| cmp_keys(&a.0, &b.0));
        Ok(V::Slice(Rc::new(SliceV { keys: s.keys.clone(), members })))
    }

    fn asof_read(&mut self, cell_id: NodeId, t_id: NodeId) -> Result<V, Code> {
        let Node::Read { fn_name, .. } = self.c.ast.node(cell_id) else { return Err(Code::TxArg) };
        let fn_name = fn_name.clone();
        let b = self.binding(&fn_name)?;
        let pattern = self.read_args(cell_id, &b.function)?;
        let V::Instant(t) = self.node(t_id)? else { return Err(Code::Type) };
        if t > self.asof {
            return Err(Code::Future);
        }
        self.charge(1)?;
        let args: Args = pattern
            .into_iter()
            .filter_map(|(k, p)| match p {
                PatternArg::Value(v) => Some((k, v)),
                _ => None,
            })
            .collect();
        let tt = calendar::instant_text(t).ok_or(Code::Range)?;
        let cell = CellKey { name: fn_name, args };
        let v = self.world.read(&cell, Some(&tt), self.fuel)?.ok_or(Code::NotCovered)?;
        self.lineage.insert(v.receipt_hash.clone());
        self.note_version(&v);
        self.note_asof(&tt);
        from_scalar(&v.value)
    }

    fn versions(&mut self, args: &[NodeId]) -> Result<V, Code> {
        let Node::Read { fn_name, .. } = self.c.ast.node(args[0]) else { return Err(Code::TxArg) };
        let fn_name = fn_name.clone();
        let b = self.binding(&fn_name)?;
        let pattern = self.read_args(args[0], &b.function)?;
        let V::Instant(from) = self.node(args[1])? else { return Err(Code::Type) };
        let V::Instant(to) = self.node(args[2])? else { return Err(Code::Type) };
        if to > self.asof {
            return Err(Code::Future);
        }
        let cargs: Args = pattern
            .into_iter()
            .filter_map(|(k, p)| match p {
                PatternArg::Value(v) => Some((k, v)),
                _ => None,
            })
            .collect();
        let (ft, tt) = (calendar::instant_text(from).ok_or(Code::Range)?, calendar::instant_text(to).ok_or(Code::Range)?);
        let cell = CellKey { name: fn_name, args: cargs };
        let vs = self.world.versions(&cell, &ft, &tt, self.fuel)?;
        self.charge(1 + vs.len() as u64)?;
        let cell_obj = cell_object(&b.function, &cell);
        let obj = Value::Object(vec![
            ("cell".to_string(), cell_obj),
            ("from".to_string(), Value::String(ft)),
            ("to".to_string(), Value::String(tt.clone())),
        ]);
        let mut hashes: Vec<String> = vs.iter().map(|v| v.receipt_hash.clone()).collect();
        hashes.sort();
        hashes.dedup();
        let entry = slice_entry(&obj, &hashes);
        self.manifests.insert(entry.clone(), Manifest { entry: entry.clone(), slice: obj, members: hashes });
        self.lineage.insert(entry);
        for v in &vs {
            self.note_version(v);
        }
        self.note_asof(&tt);
        let mut members = Vec::with_capacity(vs.len());
        for v in &vs {
            members.push((vec![V::Instant(calendar::parse_instant(&v.tx).ok_or(Code::Domain)?)], from_scalar(&v.value)?));
        }
        members.sort_by(|a, b| cmp_keys(&a.0, &b.0));
        let key = KeyTy { name: "tx".to_string(), ty: Type::Instant, unit: None, valid: true };
        Ok(V::Slice(Rc::new(SliceV { keys: vec![key], members })))
    }

    /// The dates of a calendar slice (one open date argument), ascending.
    fn holidays(&mut self, id: NodeId) -> Result<Vec<Date>, Code> {
        let s = self.slice_of(id)?;
        let mut out = Vec::with_capacity(s.members.len());
        for (k, _) in &s.members {
            if let Some(V::Date(d)) = k.first() {
                out.push(*d);
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }

    fn business(&mut self, f: &str, args: &[NodeId], named: &[(String, NodeId)]) -> Result<V, Code> {
        let V::Date(d) = self.node(args[0])? else { return Err(Code::Type) };
        let second = self.node(args[1])?;
        let mut holidays = Vec::new();
        let mut weekend = None;
        let mut roll = None;
        for (k, v) in named {
            match k.as_str() {
                "calendar" => holidays = self.holidays(*v)?,
                "weekend" => {
                    let Node::Str(w) = self.c.ast.node(*v) else { return Err(Code::Enum) };
                    weekend = Some(Weekend::from_name(w).ok_or(Code::Enum)?);
                }
                "roll" => {
                    let Node::Str(r) = self.c.ast.node(*v) else { return Err(Code::Enum) };
                    roll = Some(Roll::from_name(r).ok_or(Code::Enum)?);
                }
                _ => {}
            }
        }
        let business = named.iter().any(|(k, _)| k == "calendar" || k == "weekend" || k == "roll");
        let rules = BusinessDays { weekend: weekend.unwrap_or(Weekend::SatSun), holidays: &holidays };
        let fuel = &mut *self.fuel;
        let mut step = |n: u64| fuel.charge(n);
        match f {
            "add_days" => {
                let n = int_of(&second)?;
                if business {
                    Ok(V::Date(rules.add(d, n, &mut step)?))
                } else {
                    Ok(V::Date(date_of_days(d, n)?))
                }
            }
            "add_months" => {
                let n = int_of(&second)?;
                let m = d.add_months(n).ok_or(Code::Range)?;
                if business {
                    Ok(V::Date(rules.roll(m, roll.unwrap_or(Roll::Following), &mut step)?))
                } else {
                    Ok(V::Date(m))
                }
            }
            _ => {
                let V::Date(to) = second else { return Err(Code::Type) };
                Ok(V::Num(int(rules.between(d, to, &mut step)?)))
            }
        }
    }
}

/// The distance between two keys of one type: instants in microseconds, dates in days,
/// numbers as the absolute difference; `None` for types without a metric.
fn distance(a: &V, b: &V) -> Result<Option<Decimal>, Code> {
    Ok(match (a, b) {
        (V::Instant(a), V::Instant(b)) => Some(Decimal::from_i128((i128::from(*a) - i128::from(*b)).abs())),
        (V::Date(a), V::Date(b)) => Some(int((a.ordinal() - b.ordinal()).abs())),
        (V::Num(a), V::Num(b)) => Some(decimal::sub_exact(a, b).abs()),
        _ => None,
    })
}

/// A bound as a distance along the dimension: instants in microseconds, dates in days.
fn distance_of(dim: &KeyTy, b: &Decimal, bu: Option<&NumUnit>) -> Decimal {
    match dim.ty {
        Type::Instant => duration_micros(b, bu),
        _ => b.clone(),
    }
}

/// A value as a typed scalar of type `ty` (a read argument).
fn typed(v: &V, ty: Type) -> Result<Scalar, Code> {
    let s = to_scalar(v)?;
    if s.ty() != ty {
        return Err(Code::Type);
    }
    Ok(s)
}

/// An operator over two scalar values.
fn scalar_op(op: Op, l: &V, r: &V, lu: Option<&NumUnit>, ru: Option<&NumUnit>) -> Result<V, Code> {
    use std::cmp::Ordering;
    if op.is_comparison() {
        let o = cmp_v(l, r);
        return Ok(V::Bool(match op {
            Op::Eq => o == Ordering::Equal,
            Op::Ne => o != Ordering::Equal,
            Op::Lt => o == Ordering::Less,
            Op::Le => o != Ordering::Greater,
            Op::Gt => o == Ordering::Greater,
            _ => o != Ordering::Less,
        }));
    }
    match (op, l, r) {
        (Op::Cat, V::Str(a), V::Str(b)) => {
            if a.len() + b.len() > 4 * text::MAX_STRING_BYTES {
                return Err(Code::Range);
            }
            Ok(V::Str(text::finish(format!("{a}{b}"))?))
        }
        (Op::Add, V::Num(a), V::Num(b)) => Ok(V::Num(decimal::add(a, b)?)),
        (Op::Sub, V::Num(a), V::Num(b)) => Ok(V::Num(decimal::sub(a, b)?)),
        (Op::Mul, V::Num(a), V::Num(b)) => Ok(V::Num(decimal::mul(a, b)?)),
        (Op::Div, V::Num(a), V::Num(b)) => Ok(V::Num(decimal::div(a, b)?)),
        (Op::Add | Op::Sub, V::Instant(t), V::Num(n)) | (Op::Add, V::Num(n), V::Instant(t)) => {
            let u = if matches!(l, V::Num(_)) { lu } else { ru };
            let us = duration_micros(n, u);
            if !us.is_integer() {
                return Err(Code::Domain);
            }
            let us = us.to_i64().ok_or(Code::Range)?;
            let sign: i128 = if op == Op::Sub { -1 } else { 1 };
            Ok(V::Instant(instant_ok(i128::from(*t) + sign * i128::from(us))?))
        }
        (Op::Sub, V::Instant(a), V::Instant(b)) => Ok(V::Num(decimal::rnd(seconds(*a, *b), decimal::PRECISION).check_range()?)),
        (Op::Add | Op::Sub, V::Date(d), V::Num(n)) | (Op::Add, V::Num(n), V::Date(d)) => {
            if !n.is_integer() {
                return Err(Code::Domain);
            }
            let k = n.to_i64().ok_or(Code::Range)?;
            let k = if op == Op::Sub { -k } else { k };
            Ok(V::Date(date_of_days(*d, k)?))
        }
        (Op::Sub, V::Date(a), V::Date(b)) => Ok(V::Num(int(a.ordinal() - b.ordinal()))),
        _ => Err(Code::Type),
    }
}

/// An aggregate over values (15.7, 15.9.2).
fn aggregate(f: &str, values: &[V], unit: Option<&NumUnit>) -> Result<V, Code> {
    let _ = unit;
    if f == "count" {
        return Ok(V::Num(int(values.len() as i64)));
    }
    if f == "first" || f == "last" {
        let v = if f == "first" { values.first() } else { values.last() };
        return v.cloned().ok_or(Code::Empty);
    }
    if f == "min" || f == "max" {
        let mut best: Option<&V> = None;
        for v in values {
            let better = match best {
                None => true,
                Some(b) => {
                    let o = cmp_v(v, b);
                    if f == "min" {
                        o == std::cmp::Ordering::Less
                    } else {
                        o == std::cmp::Ordering::Greater
                    }
                }
            };
            if better {
                best = Some(v);
            }
        }
        return best.cloned().ok_or(Code::Empty);
    }
    let nums: Vec<Decimal> = values.iter().map(|v| num(v).cloned()).collect::<Result<_, _>>()?;
    Ok(V::Num(match f {
        "sum" => decimal::sum(&nums)?,
        "mean" => decimal::mean(&nums)?,
        "median" => decimal::median(&nums)?,
        "stdev" => decimal::stdev(&nums)?,
        "stdev_p" => decimal::stdev_p(&nums)?,
        _ => return Err(Code::Enum),
    }))
}

fn convert(x: &Decimal, kind: ConvertKind) -> Result<Decimal, Code> {
    match kind {
        ConvertKind::Ratio(a, b) => {
            Ok(decimal::div_round(&decimal::mul_exact(x, &int(a)), &int(b), decimal::PRECISION, Mode::HalfEven)
                .check_range()?)
        }
        ConvertKind::CelsiusToKelvin => decimal::add(x, &Decimal::parse("273.15").ok_or(Code::Domain)?),
        ConvertKind::KelvinToCelsius => decimal::sub(x, &Decimal::parse("273.15").ok_or(Code::Domain)?),
        ConvertKind::DegToRad => decimal::rnd(decimal::radians40(x), decimal::PRECISION).check_range(),
        ConvertKind::RadToDeg => {
            let r = decimal::div_p(x, &decimal::constants().rad_per_deg, decimal::WORKING);
            decimal::rnd(r, decimal::PRECISION).check_range()
        }
    }
}

/// Excel's FIXED without separators: rounded half away from zero to `digits` decimals and
/// written with exactly that many.
fn fixed(x: &Decimal, digits: &Decimal) -> Result<String, Code> {
    let d = decimal::digits_of(digits)?;
    let r = decimal::round_digits(x, digits, RoundMode::Nearest)?;
    let mut s = r.to_canonical();
    if d > 0 {
        let have = s.find('.').map(|p| s.len() - p - 1).unwrap_or_else(|| {
            s.push('.');
            0
        });
        for _ in have..d as usize {
            s.push('0');
        }
    }
    Ok(s)
}

/// The cells of a computed function's slice: the declared extent within the pattern (15.13),
/// in key order.
fn enumerate(f: &Function, slice: &SliceKey) -> Result<Vec<Args>, Code> {
    let mut lists: Vec<(String, Vec<Scalar>)> = Vec::new();
    for (name, p) in &slice.args {
        let decl = f.signature.args.get(name).ok_or(Code::Absent)?;
        let values: Vec<Scalar> = match p {
            PatternArg::Value(v) => vec![v.clone()],
            PatternArg::Open | PatternArg::Range(..) => {
                let mut vals = match &decl.extent {
                    Some(x) => extent_values(decl.ty, x)?,
                    None => match p {
                        PatternArg::Range(lo, hi) => range_values(decl.ty, lo, hi)?,
                        _ => return Err(Code::Unbounded),
                    },
                };
                if let PatternArg::Range(lo, hi) = p {
                    vals.retain(|v| {
                        cmp_scalar(v, lo) != std::cmp::Ordering::Less && cmp_scalar(v, hi) != std::cmp::Ordering::Greater
                    });
                }
                vals.sort_by(cmp_scalar);
                vals
            }
        };
        lists.push((name.clone(), values));
    }
    let total: u128 = lists.iter().map(|(_, v)| v.len() as u128).product();
    if total > 1_048_576 {
        return Err(Code::Unbounded);
    }
    let mut out: Vec<Args> = vec![Args::new()];
    for (name, values) in &lists {
        let mut next = Vec::with_capacity(out.len() * values.len());
        for a in &out {
            for v in values {
                let mut b = a.clone();
                b.insert(name.clone(), v.clone());
                next.push(b);
            }
        }
        out = next;
    }
    Ok(out)
}

fn extent_values(ty: Type, x: &Extent) -> Result<Vec<Scalar>, Code> {
    match x {
        Extent::Values(v) => v.iter().map(|s| Scalar::from_lexical(ty, s).map_err(|_| Code::Domain)).collect(),
        Extent::Range(lo, hi) => {
            let lo = Scalar::from_lexical(ty, lo).map_err(|_| Code::Domain)?;
            let hi = Scalar::from_lexical(ty, hi).map_err(|_| Code::Domain)?;
            range_values(ty, &lo, &hi)
        }
    }
}

/// The values of a range over a date (daily) or integer-valued number (by 1) argument.
fn range_values(ty: Type, lo: &Scalar, hi: &Scalar) -> Result<Vec<Scalar>, Code> {
    match ty {
        Type::Date => {
            let a = Date::parse(lo.lexical()).ok_or(Code::Domain)?.ordinal();
            let b = Date::parse(hi.lexical()).ok_or(Code::Domain)?.ordinal();
            if b - a > 1_048_576 {
                return Err(Code::Unbounded);
            }
            (a..=b)
                .map(|o| {
                    let d = Date::from_ordinal(o).ok_or(Code::Range)?;
                    Scalar::from_lexical(Type::Date, &d.text()).map_err(|_| Code::Range)
                })
                .collect()
        }
        Type::Number => {
            let a = Decimal::parse(lo.lexical()).ok_or(Code::Domain)?.ceiling().to_i64().ok_or(Code::Unbounded)?;
            let b = Decimal::parse(hi.lexical()).ok_or(Code::Domain)?.floor().to_i64().ok_or(Code::Unbounded)?;
            if b.saturating_sub(a) > 1_048_576 {
                return Err(Code::Unbounded);
            }
            (a..=b).map(|n| Scalar::from_lexical(Type::Number, &n.to_string()).map_err(|_| Code::Range)).collect()
        }
        _ => Err(Code::Unbounded),
    }
}

fn split_name(fq: &str) -> (&str, &str) {
    match fq.rfind('/') {
        Some(p) => (&fq[..p], &fq[p + 1..]),
        None => ("", fq),
    }
}

/// The cell object (4.4) of a complete cell.
pub fn cell_object(f: &Function, cell: &CellKey) -> Value {
    let (ns, name) = split_name(&cell.name);
    let args = Value::Object(cell.args.iter().map(|(k, v)| (k.clone(), v.to_value())).collect());
    Value::Object(vec![
        ("args".to_string(), args),
        ("fn".to_string(), Value::String(name.to_string())),
        ("fn_hash".to_string(), Value::String(f.hash.clone())),
        ("ns".to_string(), Value::String(ns.to_string())),
    ])
}

/// The slice object (4.4) of a pattern: a cell object with `*` and range objects.
pub fn slice_object(f: &Function, slice: &SliceKey) -> Value {
    let (ns, name) = split_name(&slice.name);
    let args = Value::Object(
        slice
            .args
            .iter()
            .map(|(k, p)| {
                let v = match p {
                    PatternArg::Value(v) => v.to_value(),
                    PatternArg::Open => Value::String("*".to_string()),
                    PatternArg::Range(lo, hi) => Value::Object(vec![
                        ("hi".to_string(), Value::String(hi.lexical().to_string())),
                        ("lo".to_string(), Value::String(lo.lexical().to_string())),
                        ("type".to_string(), Value::String(lo.ty().as_str().to_string())),
                    ]),
                };
                (k.clone(), v)
            })
            .collect(),
    );
    Value::Object(vec![
        ("args".to_string(), args),
        ("fn".to_string(), Value::String(name.to_string())),
        ("fn_hash".to_string(), Value::String(f.hash.clone())),
        ("ns".to_string(), Value::String(ns.to_string())),
    ])
}

/// The `inputs` entry of a slice read: `slice:<slice id>:<members hash>` (15.13).
pub fn slice_entry(slice: &Value, members: &[String]) -> String {
    let id = datumesh_core::sha256_hex(&slice.to_jcs());
    let arr = Value::Array(members.iter().map(|m| Value::String(m.clone())).collect());
    let mh = datumesh_core::sha256_hex(&arr.to_jcs());
    format!("slice:{id}:{mh}")
}
