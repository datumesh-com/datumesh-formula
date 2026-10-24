//! Resolution (SPEC 15.5): the parse tree becomes the canonical AST, in source order.
//!
//! Aliases become full names, every read's callee must be bound, positional read arguments bind
//! by the callee's `order`, built-in arguments bind to their parameters, `if(c, a, b)` becomes an
//! `if` node and `and(a, b, c)` becomes `(a and b) and c`. Nothing else changes.

use crate::ast::{Ast, Node, NodeId, Op, ReadArg};
use crate::builtins::{params, Params, AGGREGATES};
use crate::env::Env;
use crate::names::is_name;
use crate::parse::{Id, PArg, PNode, PVal, Tree};
use crate::{Code, Error};

/// The most nodes a canonical AST may have (15.3).
pub const MAX_NODES: usize = 65_536;

/// Resolve a parse tree. Without an environment, aliases are errors and reads must be labelled
/// (a printed body always is), and callees are not checked.
pub(crate) fn resolve(tree: &Tree, env: Option<&Env>) -> Result<Ast, Error> {
    let mut r = Resolver { tree, env, out: Vec::with_capacity(tree.nodes.len()) };
    let root = r.node(tree.root)?;
    let ast = Ast { nodes: r.out, root };
    if ast.node_count() > MAX_NODES {
        return Err(Error::new(Code::Limit, "more than 65,536 nodes"));
    }
    Ok(ast)
}

struct Resolver<'a> {
    tree: &'a Tree,
    env: Option<&'a Env>,
    out: Vec<Node>,
}

fn e(code: Code, detail: impl Into<String>) -> Error {
    Error::new(code, detail)
}

/// A variable, `let` name or lambda parameter: a word with a capital is reserved; otherwise it
/// must be a name (2.2).
pub(crate) fn check_variable(w: &str) -> Result<(), Error> {
    if w.bytes().any(|c| c.is_ascii_uppercase()) {
        return Err(e(Code::Reserved, format!("{w}: words with capitals are reserved for key literals (1.3b)")));
    }
    if !is_name(w) {
        return Err(e(Code::Name, format!("not a name (SPEC 2.2): {w}")));
    }
    Ok(())
}

impl Resolver<'_> {
    fn push(&mut self, n: Node) -> NodeId {
        self.out.push(n);
        (self.out.len() - 1) as NodeId
    }

    fn node(&mut self, id: Id) -> Result<NodeId, Error> {
        match self.tree.get(id) {
            PNode::Bin(..) => {
                // A left spine of binary operators, iteratively.
                let mut spine = Vec::new();
                let mut cur = id;
                while let PNode::Bin(op, l, r) = self.tree.get(cur) {
                    spine.push((*op, *r));
                    cur = *l;
                }
                let mut acc = self.node(cur)?;
                for (op, r) in spine.into_iter().rev() {
                    let rr = self.node(r)?;
                    acc = self.push(Node::Op { op: Op::from_binary(op), args: vec![acc, rr] });
                }
                Ok(acc)
            }
            PNode::Neg(_) => {
                let mut n = 0;
                let mut cur = id;
                while let PNode::Neg(x) = self.tree.get(cur) {
                    n += 1;
                    cur = *x;
                }
                let mut acc = self.node(cur)?;
                for _ in 0..n {
                    acc = self.push(Node::Op { op: Op::Neg, args: vec![acc] });
                }
                Ok(acc)
            }
            PNode::Not(x) => {
                let a = self.node(*x)?;
                Ok(self.push(Node::Op { op: Op::Not, args: vec![a] }))
            }
            PNode::Num { text, unit } => Ok(self.push(Node::Num { value: text.clone(), unit: unit.clone() })),
            PNode::Str(s) => Ok(self.push(Node::Str(s.clone()))),
            PNode::Bool(b) => Ok(self.push(Node::Bool(*b))),
            PNode::Date(s) => Ok(self.push(Node::Date(s.clone()))),
            PNode::Instant(s) => Ok(self.push(Node::Instant(s.clone()))),
            PNode::Var(w) => {
                check_variable(w)?;
                Ok(self.push(Node::Var(w.clone())))
            }
            PNode::If(c, a, b) => {
                let c = self.node(*c)?;
                let a = self.node(*a)?;
                let b = self.node(*b)?;
                Ok(self.push(Node::If { cond: c, then: a, els: b }))
            }
            PNode::Let(name, v, b) => {
                check_variable(name)?;
                let v = self.node(*v)?;
                let b = self.node(*b)?;
                Ok(self.push(Node::Let { name: name.clone(), value: v, body: b }))
            }
            PNode::Lambda(params, body) => {
                for p in params {
                    check_variable(p)?;
                }
                let b = self.node(*body)?;
                Ok(self.push(Node::Lambda { params: params.clone(), body: b }))
            }
            PNode::Read { name, alias, args } => self.read(name, *alias, args),
            PNode::Call { name, args } => self.call(name, args),
        }
    }

    fn read(&mut self, name: &str, alias: bool, args: &[PArg]) -> Result<NodeId, Error> {
        let fq = if alias {
            match self.env.and_then(|env| env.aliases.get(name)) {
                Some(n) => n.clone(),
                None => return Err(e(Code::Alias, format!("@{name} is not in the alias map"))),
            }
        } else {
            name.to_string()
        };
        let sig = match self.env {
            Some(env) => match env.signature_of(&fq) {
                Some(s) => Some(s),
                None => return Err(e(Code::Unresolved, format!("{fq} has no binding"))),
            },
            None => None,
        };
        let positional = args.iter().filter(|a| a.label.is_none()).count();
        let order: Vec<String> = if positional > 0 {
            match sig.and_then(|s| s.order.as_ref()) {
                Some(o) => {
                    if positional > o.len() {
                        return Err(e(Code::Arity, format!("{fq} takes {} arguments", o.len())));
                    }
                    o.clone()
                }
                None => {
                    return Err(e(Code::Positional, format!("{fq} declares no order: label its arguments")));
                }
            }
        } else {
            Vec::new()
        };
        let mut bound: Vec<(String, ReadArg)> = Vec::new();
        let mut i = 0;
        for a in args {
            let label = match &a.label {
                Some(l) => {
                    if !is_name(l) {
                        return Err(e(Code::Name, format!("not an argument name (SPEC 2.2): {l}")));
                    }
                    l.clone()
                }
                None => {
                    i += 1;
                    order[i - 1].clone()
                }
            };
            let v = match &a.value {
                PVal::Open => ReadArg::Open,
                PVal::Expr(x) => ReadArg::Expr(self.node(*x)?),
                PVal::Range(lo, hi) => {
                    let lo = self.node(*lo)?;
                    let hi = self.node(*hi)?;
                    ReadArg::Range(lo, hi)
                }
            };
            bound.push((label, v));
        }
        // Labels against the callee: duplicates, unknown labels, then missing arguments.
        for (k, (l, _)) in bound.iter().enumerate() {
            if bound[..k].iter().any(|(m, _)| m == l) {
                return Err(e(Code::ArgDuplicate, format!("{l} is given twice")));
            }
        }
        if let Some(sig) = sig {
            for (l, _) in &bound {
                if !sig.args.contains_key(l) {
                    return Err(e(Code::ArgUnknown, format!("{fq} has no argument {l}")));
                }
            }
            for n in sig.args.keys() {
                if !bound.iter().any(|(l, _)| l == n) {
                    return Err(e(Code::ArgMissing, format!("{fq} needs {n} (an open argument is written *)")));
                }
            }
        }
        bound.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(self.push(Node::Read { fn_name: fq, args: bound }))
    }

    fn call(&mut self, name: &str, args: &[PArg]) -> Result<NodeId, Error> {
        let Some(spec) = params(name) else {
            return Err(e(Code::UnknownFunction, format!("{name} is not a built-in")));
        };
        match spec {
            Params::Variadic(min) => {
                if args.len() < min {
                    let what = if min == 1 { "one" } else { "two" };
                    return Err(e(Code::Arity, format!("{name}() takes {what} or more arguments")));
                }
                let mut vals = Vec::new();
                for a in args {
                    if let Some(l) = &a.label {
                        return Err(e(Code::ArgUnknown, format!("{name}() has no parameter {l}")));
                    }
                    vals.push(self.value(&a.value)?);
                }
                if name == "and" || name == "or" {
                    let op = if name == "and" { Op::And } else { Op::Or };
                    let mut acc = vals[0];
                    for v in &vals[1..] {
                        acc = self.push(Node::Op { op, args: vec![acc, *v] });
                    }
                    return Ok(acc);
                }
                Ok(self.push(Node::Call { fn_name: name.to_string(), args: vals, named: Vec::new() }))
            }
            Params::Asof => {
                if !args.is_empty() && args.len() != 2 {
                    return Err(e(Code::Arity, "asof() or asof(cell, T)"));
                }
                let mut vals = Vec::new();
                for a in args {
                    if let Some(l) = &a.label {
                        return Err(e(Code::ArgUnknown, format!("asof() has no parameter {l}")));
                    }
                    vals.push(self.value(&a.value)?);
                }
                Ok(self.push(Node::Call { fn_name: "asof".to_string(), args: vals, named: Vec::new() }))
            }
            Params::Fixed(required, optional) => {
                let all: Vec<&str> = required.iter().chain(optional.iter()).copied().collect();
                let mut slots: Vec<Option<NodeId>> = vec![None; all.len()];
                let mut i = 0;
                for a in args {
                    let k = match &a.label {
                        None => {
                            if i >= all.len() {
                                return Err(e(Code::Arity, format!("{name}() takes at most {} arguments", all.len())));
                            }
                            i += 1;
                            i - 1
                        }
                        Some(l) => match all.iter().position(|p| p == l) {
                            None => return Err(e(Code::ArgUnknown, format!("{name}() has no parameter {l}"))),
                            Some(k) => {
                                if slots[k].is_some() {
                                    return Err(e(Code::ArgDuplicate, format!("{l} is given twice")));
                                }
                                k
                            }
                        },
                    };
                    let v = self.param_value(name, all[k], &a.value)?;
                    slots[k] = Some(v);
                }
                for (k, p) in required.iter().enumerate() {
                    if slots[k].is_none() {
                        return Err(e(Code::Arity, format!("{name}() needs {p}")));
                    }
                }
                if name == "if" {
                    let (c, a, b) = (slots[0].unwrap_or(0), slots[1].unwrap_or(0), slots[2].unwrap_or(0));
                    return Ok(self.push(Node::If { cond: c, then: a, els: b }));
                }
                let pos: Vec<NodeId> = slots[..required.len()].iter().map(|s| s.unwrap_or(0)).collect();
                let mut named: Vec<(String, NodeId)> = Vec::new();
                for (k, p) in optional.iter().enumerate() {
                    if let Some(v) = slots[required.len() + k] {
                        named.push((p.to_string(), v));
                    }
                }
                Ok(self.push(Node::Call { fn_name: name.to_string(), args: pos, named }))
            }
        }
    }

    fn value(&mut self, v: &PVal) -> Result<NodeId, Error> {
        if let PVal::Expr(x) = v {
            if matches!(self.tree.get(*x), PNode::Lambda(..)) {
                return Err(e(Code::Placement, "a lambda is only the second argument of map or filter"));
            }
        }
        match v {
            PVal::Expr(x) => self.node(*x),
            // The parser admits `*` and ranges only in reads.
            PVal::Open | PVal::Range(..) => Err(e(Code::Placement, "* and ranges are arguments of a read only")),
        }
    }

    /// A built-in argument: the aggregate of `scan` and `window` and the dimension of `along`
    /// are words, not variables.
    fn param_value(&mut self, callee: &str, param: &str, v: &PVal) -> Result<NodeId, Error> {
        if param == "fn" && (callee == "map" || callee == "filter") {
            if let PVal::Expr(x) = v {
                if matches!(self.tree.get(*x), PNode::Lambda(..)) {
                    return self.node(*x);
                }
            }
        }
        if param == "aggregate" && (callee == "scan" || callee == "window") {
            let word = match v {
                PVal::Expr(x) => match self.tree.get(*x) {
                    PNode::Var(w) => Some(w.to_ascii_lowercase()),
                    _ => None,
                },
                _ => None,
            };
            return match word {
                Some(w) if AGGREGATES.contains(&w.as_str()) => Ok(self.push(Node::Agg(w))),
                _ => Err(e(
                    Code::Enum,
                    format!("the aggregate of {callee}() is one of sum, count, mean, min, max, first, last, median, stdev, stdev_p"),
                )),
            };
        }
        if param == "along" {
            if let PVal::Expr(x) = v {
                if let PNode::Var(w) = self.tree.get(*x) {
                    if !is_name(w) {
                        return Err(e(Code::Name, format!("not an argument name (SPEC 2.2): {w}")));
                    }
                    return Ok(self.push(Node::Key(w.clone())));
                }
            }
            // Anything else stays an expression; the checker reports E_FORMULA_ALONG.
        }
        self.value(v)
    }
}
