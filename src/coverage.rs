//! Branch coverage and edge facts (Build 2 Part 3, E5): what the evaluator records, and what a
//! body's signature and body make the cases a test suite must exercise.
//!
//! A branch point is an `if` (two arms: 0 = then, 1 = else) or an `otherwise` (two arms: 0 =
//! the first argument gave a value, 1 = it faulted with a catchable code and the fallback was
//! taken). The language has no `choose`: a chain of `if`s is one. The id of a point is the arena
//! index of its node, which a parse and a re-parse of the canonical body give alike.
//!
//! An [`EvalObserver`] is told each arm of the evaluated function's own body when it is taken
//! (before the arm is evaluated, so an arm that then faults is still recorded). The arms of
//! callees are not reported: their ids belong to other bodies.

use std::collections::BTreeSet;

use datumesh_core::Type;

use crate::ast::{Ast, Node, NodeId, Op, ReadArg};
use crate::env::{Extent, Signature};
use crate::Compiled;

/// Told of each branch arm the evaluator takes in a function's own body.
pub trait EvalObserver {
    /// The arm `arm` of the branch point `id` was taken.
    fn branch_taken(&mut self, id: NodeId, arm: usize);
}

/// The observer that records nothing (what the plain evaluation uses).
pub(crate) struct NoObserver;

impl EvalObserver for NoObserver {
    fn branch_taken(&mut self, _id: NodeId, _arm: usize) {}
}

/// The kind of a branch point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchKind {
    /// An `if`: 0 = then, 1 = else.
    If,
    /// An `otherwise`: 0 = the first argument gave a value, 1 = the fallback was taken.
    Otherwise,
}

/// A branch point of a body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchPoint {
    /// The arena index of the node.
    pub id: NodeId,
    /// Its kind.
    pub kind: BranchKind,
    /// The number of arms (2 for both kinds).
    pub arms: usize,
}

/// The nodes reachable from the root, by arena index.
fn reachable(ast: &Ast) -> Vec<bool> {
    let mut seen = vec![false; ast.nodes.len()];
    let mut stack = vec![ast.root];
    while let Some(id) = stack.pop() {
        if !std::mem::replace(&mut seen[id as usize], true) {
            stack.extend(ast.children(id));
        }
    }
    seen
}

/// The branch points of a body, ascending by id.
pub fn branches(ast: &Ast) -> Vec<BranchPoint> {
    let reach = reachable(ast);
    let mut out = Vec::new();
    for (i, node) in ast.nodes.iter().enumerate() {
        if !reach[i] {
            continue;
        }
        let kind = match node {
            Node::If { .. } => BranchKind::If,
            Node::Call { fn_name, .. } if fn_name == "otherwise" => BranchKind::Otherwise,
            _ => continue,
        };
        out.push(BranchPoint { id: i as NodeId, kind, arms: 2 });
    }
    out
}

/// The branch arms recorded by an evaluation (or by several): a set of `(id, arm)`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    taken: BTreeSet<(NodeId, usize)>,
}

impl Coverage {
    /// An empty coverage.
    pub fn new() -> Coverage {
        Coverage::default()
    }

    /// The arms taken, ascending.
    pub fn taken(&self) -> Vec<(NodeId, usize)> {
        self.taken.iter().copied().collect()
    }

    /// Whether the arm `arm` of `id` was taken.
    pub fn contains(&self, id: NodeId, arm: usize) -> bool {
        self.taken.contains(&(id, arm))
    }

    /// Adds every arm of `other`.
    pub fn union(&mut self, other: &Coverage) {
        self.taken.extend(other.taken.iter().copied());
    }

    /// The number of arms taken, of the arms of `points`.
    pub fn covered(&self, points: &[BranchPoint]) -> usize {
        points.iter().map(|p| (0..p.arms).filter(|&a| self.contains(p.id, a)).count()).sum()
    }

    /// The number of arms of `points`.
    pub fn total(points: &[BranchPoint]) -> usize {
        points.iter().map(|p| p.arms).sum()
    }

    /// The arms of `points` not taken, ascending.
    pub fn missing(&self, points: &[BranchPoint]) -> Vec<(NodeId, usize)> {
        let mut out: Vec<(NodeId, usize)> = points
            .iter()
            .flat_map(|p| (0..p.arms).map(move |a| (p.id, a)))
            .filter(|&(id, a)| !self.contains(id, a))
            .collect();
        out.sort();
        out
    }
}

impl EvalObserver for Coverage {
    fn branch_taken(&mut self, id: NodeId, arm: usize) {
        self.taken.insert((id, arm));
    }
}

/// The facts of a body and its signature that name the edge cases a test must cover (the
/// mesh's derived cases, 16.9 of the Build 2 contract, E5). Every list is ascending and holds
/// each name once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EdgeFacts {
    /// The `number` arguments that appear in a divisor of the body: the right operand of `/`,
    /// the second argument of `mod`, anywhere inside it. Static: an arm no input reaches counts.
    pub divisors: Vec<String>,
    /// The fully qualified names of the cell reads (every argument a single value).
    pub cell_inputs: Vec<String>,
    /// The fully qualified names of the slice reads (an open or a range argument).
    pub slice_inputs: Vec<String>,
    /// Every `number` argument, declared.
    pub number_args: Vec<String>,
    /// Every `date` argument, declared.
    pub date_args: Vec<String>,
    /// Every `instant` argument, declared.
    pub instant_args: Vec<String>,
    /// The `date` and `instant` arguments with a declared extent, and the extent.
    pub extent_args: Vec<(String, Extent)>,
}

/// Collects the `number` arguments named in the subtree of `root`.
fn divisor_args(ast: &Ast, root: NodeId, sig: &Signature, out: &mut BTreeSet<String>) {
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if let Node::Var(name) = ast.node(id) {
            if sig.args.get(name).is_some_and(|d| d.ty == Type::Number) {
                out.insert(name.clone());
            }
        }
        stack.extend(ast.children(id));
    }
}

/// The edge facts of a compiled body under its signature.
pub fn edge_facts(c: &Compiled, sig: &Signature) -> EdgeFacts {
    let ast = &c.ast;
    let reach = reachable(ast);
    let mut divisors = BTreeSet::new();
    let mut cell_inputs = BTreeSet::new();
    let mut slice_inputs = BTreeSet::new();
    for (i, node) in ast.nodes.iter().enumerate() {
        if !reach[i] {
            continue;
        }
        match node {
            Node::Op { op: Op::Div, args } => {
                if let Some(&d) = args.get(1) {
                    divisor_args(ast, d, sig, &mut divisors);
                }
            }
            Node::Call { fn_name, args, .. } if fn_name == "mod" => {
                if let Some(&d) = args.get(1) {
                    divisor_args(ast, d, sig, &mut divisors);
                }
            }
            Node::Read { fn_name, args, .. } => {
                if args.iter().any(|(_, a)| !matches!(a, ReadArg::Expr(_))) {
                    slice_inputs.insert(fn_name.clone());
                } else {
                    cell_inputs.insert(fn_name.clone());
                }
            }
            _ => {}
        }
    }
    let of_type =
        |t: Type| -> Vec<String> { sig.args.iter().filter(|(_, d)| d.ty == t).map(|(n, _)| n.clone()).collect() };
    let extent_args = sig
        .args
        .iter()
        .filter(|(_, d)| matches!(d.ty, Type::Date | Type::Instant))
        .filter_map(|(n, d)| d.extent.clone().map(|e| (n.clone(), e)))
        .collect();
    EdgeFacts {
        divisors: divisors.into_iter().collect(),
        cell_inputs: cell_inputs.into_iter().collect(),
        slice_inputs: slice_inputs.into_iter().collect(),
        number_args: of_type(Type::Number),
        date_args: of_type(Type::Date),
        instant_args: of_type(Type::Instant),
        extent_args,
    }
}
