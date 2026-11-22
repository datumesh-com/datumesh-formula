//! Resolution (SPEC 15.5, 16.3): the parse tree becomes the canonical AST, in source order.
//!
//! Aliases become full names, every read's callee must be bound, positional read arguments bind
//! by the callee's `order`, built-in arguments bind to their parameters, `if(c, a, b)` becomes an
//! `if` node and `and(a, b, c)` becomes `(a and b) and c`. Since spec-0.5.0 a class name is an
//! object (its keys bound by the class's `order`), a method call on an object is the read of the
//! method's function with the object's keys first (16.3), `this` is the receiver of the method
//! being defined, a key literal is a string in a `string` position, and a class-typed argument
//! takes an object's key encoding (16.5). Defaults `asof` are bound in the AST (16.7). The arena
//! is numbered in a canonical order, so equal trees are equal arenas. Nothing else changes.

use std::collections::BTreeMap;

use datumesh_core::{
    format_object_key, parse_object_key, ClassKey, KeyNormalisation, KeyValue, ObjectKey, Scalar, Type,
};

use crate::ast::{Ast, Node, NodeId, Op, ReadArg};
use crate::builtins::{params, Params, AGGREGATES};
use crate::classes::{definer, DefinerError};
use crate::env::{ArgDecl, Env, Signature, UnitDecl, UnitSide};
use crate::names::is_name;
use crate::parse::{Id, PArg, PNode, PVal, Tree};
use crate::{Code, Error};

/// The function a method `name` of the class `class` reads (16.3, K5): `D/name`, where D is the class
/// that defines it when it is inherited (the nearest ancestor whose own descriptor gave `name` the same
/// function hash, found through the parent hashes), else `class/name`. An unnamed parent hash is
/// `E_FORMULA_UNRESOLVED`.
pub(crate) fn method_name(env: &Env, class: &str, name: &str) -> Result<String, Error> {
    let desc = env.classes.get(class).ok_or_else(|| e(Code::Unresolved, format!("{class} is not bound")))?;
    match definer(
        desc,
        name,
        |ph| Ok::<_, std::convert::Infallible>(env.class_by_hash(ph)),
        |ph| Ok::<_, std::convert::Infallible>(env.classes.by_hash(ph).map(|(n, _)| n.to_string())),
    ) {
        Ok(Some(d)) => Ok(format!("{d}/{name}")),
        Ok(None) => Ok(format!("{class}/{name}")),
        Err(DefinerError::Unnamed(h)) => Err(e(
            Code::Unresolved,
            format!("no class name is bound to the parent hash {h} of {class}: its method {name} is not known (16.3)"),
        )),
        Err(DefinerError::Failed(never)) => match never {},
    }
}

/// The most nodes a canonical AST may have (15.3).
pub const MAX_NODES: usize = 65_536;

/// Resolve a parse tree. Without an environment, aliases are errors and reads must be labelled
/// (a printed body always is), and callees are not checked.
pub(crate) fn resolve(tree: &Tree, env: Option<&Env>) -> Result<Ast, Error> {
    let mut r = Resolver { tree, env, out: Vec::with_capacity(tree.nodes.len()) };
    let root = r.node(tree.root)?;
    let ast = canonical(r.out, root);
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

/// The signature of the environment's function with this hash (16.3).
pub(crate) fn signature_by_hash<'e>(env: &'e Env, fn_hash: &str) -> Option<&'e Signature> {
    env.functions.values().find(|f| f.hash == fn_hash).or_else(|| env.pinned.get(fn_hash)).map(|f| &f.signature)
}

/// A method's own signature: its arguments and order without the first `keys` of its order (the
/// class keys, 16.3), and the same return.
fn own_signature(sig: &Signature, keys: usize) -> Signature {
    let order = sig.order.clone().unwrap_or_default();
    let (keyed, own) = order.split_at(keys.min(order.len()));
    let args = sig.args.iter().filter(|(n, _)| !keyed.contains(n)).map(|(n, d)| (n.clone(), d.clone())).collect();
    Signature {
        args,
        order: Some(own.to_vec()),
        returns: sig.returns,
        returns_unit: sig.returns_unit.clone(),
        returns_classes: sig.returns_classes.clone(),
    }
}

/// A key of an object: the node it is written as (pushed where it is used) and its key value
/// (16.5), absent for `this`'s keys, which are the method's own arguments.
#[derive(Clone, Debug)]
struct Key {
    /// The key's node in the output (a literal, or an expression of a receiver, K7).
    node: NodeId,
    value: Option<KeyValue>,
}

/// An object (16.3): its class and every key bound.
struct Obj {
    class: String,
    keys: BTreeMap<String, Key>,
}

/// The object key of an object (16.5), for its encoding.
fn object_key(obj: &Obj) -> Result<ObjectKey, Error> {
    let mut pairs = Vec::with_capacity(obj.keys.len());
    for (name, k) in &obj.keys {
        let v = k.value.clone().ok_or_else(|| e(Code::Placement, format!("{name} is not a literal key")))?;
        pairs.push((name.clone(), v));
    }
    ObjectKey::new(&obj.class, pairs).map_err(|_| e(Code::Key, format!("the key of {} is not valid", obj.class)))
}

/// The key encoding of a string (16.5): exactly the encoding of what it decodes to.
fn decode(text: &str) -> Result<ObjectKey, Error> {
    let obj = parse_object_key(text).map_err(|_| e(Code::Key, format!("not a key encoding: {text}")))?;
    if format_object_key(&obj) != text {
        return Err(e(Code::Key, format!("not the canonical key encoding: {text}")));
    }
    Ok(obj)
}

/// A decoded key encoding checked against its class, as an object literal is (16.5, 16.3): its key
/// names are the class's, each value has its key's type (a class-typed key checked against its own
/// class), and each string value is canonical under its key's normalisation. Key names or types not
/// the class's are `E_FORMULA_TYPE`; a string not canonical is `E_FORMULA_KEY`. A class not bound
/// here is not checked.
fn check_encoding(env: &Env, obj: &ObjectKey) -> Result<(), Error> {
    let Some(desc) = env.classes.get(&obj.class) else { return Ok(()) };
    if !obj.keys.keys().eq(desc.keys.keys()) {
        return Err(e(Code::Type, format!("the keys of {} are not its class's", obj.class)));
    }
    for (k, v) in &obj.keys {
        match (desc.keys.get(k), v) {
            (Some(ClassKey::Scalar { ty, key_normalisation, .. }), KeyValue::Scalar(sc)) if sc.ty() == *ty => {
                if let (Scalar::String(t), Some(rule)) = (sc, key_normalisation) {
                    rule.check(t).map_err(|_| e(Code::Key, format!("{t:?} is not canonical under {rule}")))?;
                }
            }
            (Some(ClassKey::Class(c)), KeyValue::Object(o)) if o.class == *c => check_encoding(env, o)?,
            _ => return Err(e(Code::Type, format!("the key {k} of {} is not of its type", obj.class))),
        }
    }
    Ok(())
}

/// The children of a node, in the order the canonical numbering visits them.
fn children_of(n: &Node) -> Vec<NodeId> {
    match n {
        Node::Op { args, .. } => args.clone(),
        Node::If { cond, then, els } => vec![*cond, *then, *els],
        Node::Let { value, body, .. } => vec![*value, *body],
        Node::Read { args, .. } => {
            let mut v = Vec::new();
            for (_, a) in args {
                match a {
                    ReadArg::Expr(e) => v.push(*e),
                    ReadArg::Range(lo, hi) => {
                        v.push(*lo);
                        v.push(*hi);
                    }
                    ReadArg::Open => {}
                }
            }
            v
        }
        Node::Dispatch { on, args, .. } => std::iter::once(*on).chain(args.iter().map(|(_, x)| *x)).collect(),
        Node::Call { args, named, .. } => args.iter().copied().chain(named.iter().map(|(_, x)| *x)).collect(),
        Node::Lambda { body, .. } => vec![*body],
        _ => Vec::new(),
    }
}

/// The node with its children renamed by `f`.
fn renamed(n: &Node, f: &dyn Fn(NodeId) -> NodeId) -> Node {
    match n {
        Node::Op { op, args } => Node::Op { op: *op, args: args.iter().map(|x| f(*x)).collect() },
        Node::If { cond, then, els } => Node::If { cond: f(*cond), then: f(*then), els: f(*els) },
        Node::Let { name, value, body } => Node::Let { name: name.clone(), value: f(*value), body: f(*body) },
        Node::Read { fn_name, fn_hash, args } => Node::Read {
            fn_name: fn_name.clone(),
            fn_hash: fn_hash.clone(),
            args: args
                .iter()
                .map(|(k, a)| {
                    let a = match a {
                        ReadArg::Expr(x) => ReadArg::Expr(f(*x)),
                        ReadArg::Range(lo, hi) => ReadArg::Range(f(*lo), f(*hi)),
                        ReadArg::Open => ReadArg::Open,
                    };
                    (k.clone(), a)
                })
                .collect(),
        },
        Node::Dispatch { on, method, classes, args } => Node::Dispatch {
            on: f(*on),
            method: method.clone(),
            classes: classes.clone(),
            args: args.iter().map(|(k, x)| (k.clone(), f(*x))).collect(),
        },
        Node::Call { fn_name, args, named } => Node::Call {
            fn_name: fn_name.clone(),
            args: args.iter().map(|x| f(*x)).collect(),
            named: named.iter().map(|(k, x)| (k.clone(), f(*x))).collect(),
        },
        Node::Lambda { params, body } => Node::Lambda { params: params.clone(), body: f(*body) },
        other => other.clone(),
    }
}

/// The canonical arena: nodes numbered in post-order of a walk that visits children in
/// [`children_of`] order, so that equal trees have equal arenas.
fn canonical(nodes: Vec<Node>, root: NodeId) -> Ast {
    let mut map: Vec<Option<NodeId>> = vec![None; nodes.len()];
    let mut out: Vec<Node> = Vec::with_capacity(nodes.len());
    let mut stack: Vec<(NodeId, bool)> = vec![(root, false)];
    while let Some((id, expanded)) = stack.pop() {
        if map[id as usize].is_some() {
            continue;
        }
        if expanded {
            let n = renamed(&nodes[id as usize], &|x| map[x as usize].unwrap_or(0));
            map[id as usize] = Some(out.len() as NodeId);
            out.push(n);
        } else {
            stack.push((id, true));
            for c in children_of(&nodes[id as usize]).into_iter().rev() {
                if map[c as usize].is_none() {
                    stack.push((c, false));
                }
            }
        }
    }
    let root = map[root as usize].unwrap_or(0);
    Ast { nodes: out, root }
}

/// The literal node of a scalar of type `ty`, for a key of that type.
fn scalar_key(ty: Type, node: &Node) -> Option<Scalar> {
    match (ty, node) {
        (Type::String, Node::Str(s)) => Scalar::from_lexical(Type::String, s).ok(),
        (Type::Number, Node::Num { value, .. }) => Scalar::from_lexical(Type::Number, value).ok(),
        (Type::Date, Node::Date(s)) => Scalar::from_lexical(Type::Date, s).ok(),
        (Type::Instant, Node::Instant(s)) => Scalar::from_lexical(Type::Instant, s).ok(),
        (Type::Boolean, Node::Bool(b)) => Some(Scalar::Boolean(*b)),
        _ => None,
    }
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
            PNode::KeyLit(w) => {
                Err(e(Code::Reserved, format!("{w} is a key literal: a string only where a string key or argument is")))
            }
            PNode::This => {
                Err(e(Code::Reserved, "this is the receiver of a method call (this.m(...)) in a method's body"))
            }
            PNode::Super => Err(e(Code::Reserved, "super is reserved (16.3)")),
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
            PNode::Read { pin: Some(pin), name, args, .. } => {
                if !self.pins_allowed() {
                    return Err(Error::new(
                        Code::Placement,
                        format!("a pinned read of {name} is a request form (5.8), or super in a method's body"),
                    ));
                }
                self.pinned_read(name, pin, args)
            }
            PNode::Read { name, alias, args, .. } => self.read(name, *alias, args),
            PNode::Call { name, args } => self.call(name, args),
            PNode::Method { recv, name, qualified, args } => self.method(*recv, name, *qualified, args),
        }
    }

    /// Whether a pinned read is written here: in a method's body (`super`, 16.3) or a stored body.
    fn pins_allowed(&self) -> bool {
        self.env.is_some_and(|env| env.method_of.is_some() || env.stored_body)
    }

    /// A read pinned to a function hash (16.3): its arguments bound by the signature of that function,
    /// which is found by its hash, not by a binding.
    fn pinned_read(&mut self, name: &str, pin: &str, args: &[PArg]) -> Result<NodeId, Error> {
        let sig = self
            .env
            .and_then(|env| signature_by_hash(env, pin))
            .ok_or_else(|| e(Code::Unresolved, format!("{name}@{pin} has no descriptor here")))?;
        let bound = self.bind(name, Some(sig), Vec::new(), args)?;
        Ok(self.push(Node::Read { fn_name: name.to_string(), fn_hash: Some(pin.to_string()), args: bound }))
    }

    /// The class a read names, through the alias map for an alias (16.3): the full name, when it is
    /// a class of the environment.
    fn class_named(&self, name: &str, alias: bool) -> Option<String> {
        let env = self.env?;
        let fq = if alias { env.aliases.get(name)?.as_str() } else { name };
        env.classes.get(fq).map(|_| fq.to_string())
    }

    /// A read of a function: its arguments bound by the callee's signature (15.5). A class name
    /// is an object, which is a value only in a class-typed position or the receiver of a method
    /// (16.3): as a value it is `E_FORMULA_METHOD`.
    fn read(&mut self, name: &str, alias: bool, args: &[PArg]) -> Result<NodeId, Error> {
        let fq = if alias {
            match self.env.and_then(|env| env.aliases.get(name)) {
                Some(n) => n.clone(),
                None => return Err(e(Code::Alias, format!("@{name} is not in the alias map"))),
            }
        } else {
            name.to_string()
        };
        let env = self.env;
        let sig = match env {
            Some(env) => match env.signature_of(&fq) {
                Some(s) => Some(s),
                None if env.classes.get(&fq).is_some() => {
                    return Err(e(
                        Code::Method,
                        format!("{fq} is a class: an object is a receiver or a key, not a value"),
                    ));
                }
                None => return Err(e(Code::Unresolved, format!("{fq} has no binding"))),
            },
            None => None,
        };
        let bound = self.bind(&fq, sig, Vec::new(), args)?;
        Ok(self.push(Node::Read { fn_name: fq, fn_hash: None, args: bound }))
    }

    /// Bind the arguments of a read of `fq` (15.5). `pre` are arguments already bound (the keys of
    /// a method's object, 16.3), which the positional arguments follow. Checks in order: a
    /// positional past the order (`E_FORMULA_ARITY`), then per argument its label (unknown), its
    /// value's type and check, a label given twice; then the missing arguments, in JCS order of
    /// names (a defaulted one is bound to `asof`, 16.7).
    fn bind(
        &mut self,
        fq: &str,
        sig: Option<&Signature>,
        pre: Vec<(String, NodeId)>,
        args: &[PArg],
    ) -> Result<Vec<(String, ReadArg)>, Error> {
        let skip = pre.len();
        let positional = args.iter().filter(|a| a.label.is_none()).count();
        let avail: Vec<String> = if positional > 0 {
            match sig.and_then(|s| s.order.as_ref()) {
                Some(o) => {
                    let rest = o[skip.min(o.len())..].to_vec();
                    if positional > rest.len() {
                        return Err(e(Code::Arity, format!("{fq} takes {} arguments", rest.len())));
                    }
                    rest
                }
                None => {
                    return Err(e(Code::Positional, format!("{fq} declares no order: label its arguments")));
                }
            }
        } else {
            Vec::new()
        };
        let mut bound: Vec<(String, ReadArg)> = Vec::new();
        for (name, id) in pre {
            bound.push((name, ReadArg::Expr(id)));
        }
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
                    avail[i - 1].clone()
                }
            };
            let decl: Option<&ArgDecl> = match sig {
                Some(s) => match s.args.get(&label) {
                    Some(d) => Some(d),
                    None => return Err(e(Code::ArgUnknown, format!("{fq} has no argument {label}"))),
                },
                None => None,
            };
            if bound.iter().any(|(m, _)| *m == label) {
                return Err(e(Code::ArgDuplicate, format!("{label} is given twice")));
            }
            let v = self.arg(decl, &a.value)?;
            bound.push((label, v));
        }
        if let Some(sig) = sig {
            for (n, d) in &sig.args {
                if bound.iter().any(|(l, _)| l == n) {
                    continue;
                }
                if d.extras.default_asof {
                    let id = self.default_asof(d.ty);
                    bound.push((n.clone(), ReadArg::Expr(id)));
                } else if let Some(text) = &d.extras.default_literal {
                    let id = self.literal_default(d, text)?;
                    bound.push((n.clone(), ReadArg::Expr(id)));
                } else {
                    return Err(e(Code::ArgMissing, format!("{fq} needs {n} (an open argument is written *)")));
                }
            }
        }
        bound.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(bound)
    }

    /// A literal default (K13) as the literal written in the body (7, 16.7): the canonical AST holds it.
    fn literal_default(&mut self, d: &ArgDecl, text: &str) -> Result<NodeId, Error> {
        let node = match d.ty {
            Type::String => Node::Str(text.to_string()),
            Type::Boolean => Node::Bool(text == "true"),
            Type::Date => Node::Date(text.to_string()),
            Type::Instant => Node::Instant(text.to_string()),
            Type::Number => {
                let unit = match &d.unit {
                    Some(UnitDecl::Simple(UnitSide::Literal(u))) if u != "1" => Some(u.clone()),
                    _ => None,
                };
                match text.strip_prefix('-') {
                    Some(rest) => {
                        let inner = self.push(Node::Num { value: rest.to_string(), unit });
                        return Ok(self.push(Node::Op { op: Op::Neg, args: vec![inner] }));
                    }
                    None => Node::Num { value: text.to_string(), unit },
                }
            }
            _ => return Err(e(Code::Type, format!("a literal default of type {}", d.ty.as_str()))),
        };
        Ok(self.push(node))
    }

    /// `asof()` for an instant, `date_of(asof())` for a date (16.7): the default of an argument.
    fn default_asof(&mut self, ty: Type) -> NodeId {
        let a = self.push(Node::Call { fn_name: "asof".to_string(), args: Vec::new(), named: Vec::new() });
        if ty == Type::Date {
            self.push(Node::Call { fn_name: "date_of".to_string(), args: vec![a], named: Vec::new() })
        } else {
            a
        }
    }

    /// An argument value, by its declaration (absent without an environment).
    fn arg(&mut self, decl: Option<&ArgDecl>, v: &PVal) -> Result<ReadArg, Error> {
        match v {
            PVal::Open => Ok(ReadArg::Open),
            PVal::Range(lo, hi) => {
                let lo = self.node(*lo)?;
                let hi = self.node(*hi)?;
                Ok(ReadArg::Range(lo, hi))
            }
            PVal::Expr(x) => Ok(ReadArg::Expr(self.arg_expr(decl, *x)?)),
        }
    }

    /// An argument expression. In a `string` position a key literal is its string, checked by the
    /// argument's `key_normalisation` (16.4, `E_FORMULA_KEY`). In a class-typed position (`classes`,
    /// 7) an object is its key encoding (16.5) and a string must be one of those classes' keys.
    fn arg_expr(&mut self, decl: Option<&ArgDecl>, x: Id) -> Result<NodeId, Error> {
        let Some(d) = decl else { return self.node(x) };
        if let Some(classes) = &d.extras.classes {
            match self.tree.get(x) {
                PNode::Read { name, alias, pin: None, args } if self.class_named(name, *alias).is_some() => {
                    let class = self.class_named(name, *alias).unwrap_or_default();
                    if !classes.contains(&class) {
                        return Err(e(Code::Type, format!("{class} is not one of the classes of this position")));
                    }
                    let obj = self.object(&class, args, false)?;
                    let text = self.encode_key(&obj)?;
                    return Ok(self.push(Node::Str(text)));
                }
                PNode::Str(text) | PNode::KeyLit(text) => {
                    let text = text.clone();
                    let obj = decode(&text)?;
                    if !classes.contains(&obj.class) {
                        return Err(e(Code::Type, format!("{} is not one of the classes of this position", obj.class)));
                    }
                    if let Some(env) = self.env {
                        check_encoding(env, &obj)?;
                    }
                    return Ok(self.push(Node::Str(text)));
                }
                _ => {}
            }
        }
        if let PNode::KeyLit(text) = self.tree.get(x) {
            if d.ty != Type::String {
                return Err(e(Code::Type, format!("the key literal {text} is not a string")));
            }
            let text = text.clone();
            check_key(&d.extras.key_normalisation, &text)?;
            return Ok(self.push(Node::Str(text)));
        }
        let n = self.node(x)?;
        if d.ty == Type::String {
            if let Node::Str(s) = &self.out[n as usize] {
                check_key(&d.extras.key_normalisation, s)?;
            }
        }
        Ok(n)
    }

    /// An object `name(keys)` (16.3, 16.5): its keys bound positionally by the class's `order`
    /// or labelled, every key once, each checked against its declaration and normalisation.
    fn object(&mut self, name: &str, args: &[PArg], receiver: bool) -> Result<Obj, Error> {
        let env = self.env.ok_or_else(|| e(Code::Unresolved, format!("{name} is not bound")))?;
        let desc = env.classes.get(name).ok_or_else(|| e(Code::Unresolved, format!("{name} is not a class")))?;
        let mut keys: BTreeMap<String, Key> = BTreeMap::new();
        let mut i = 0;
        for a in args {
            let label = match &a.label {
                Some(l) => {
                    if !desc.keys.contains_key(l) {
                        return Err(e(Code::ArgUnknown, format!("{name} has no key {l}")));
                    }
                    l.clone()
                }
                None => {
                    if i >= desc.order.len() {
                        return Err(e(Code::Arity, format!("{name} has {} keys", desc.order.len())));
                    }
                    i += 1;
                    desc.order[i - 1].clone()
                }
            };
            if keys.contains_key(&label) {
                return Err(e(Code::ArgDuplicate, format!("{label} is given twice")));
            }
            let PVal::Expr(x) = &a.value else {
                return Err(e(Code::Placement, "an object has no * or range"));
            };
            let ck = desc.keys.get(&label).ok_or_else(|| e(Code::ArgUnknown, format!("{name} has no key {label}")))?;
            let key = self.key_value(ck, *x, receiver)?;
            keys.insert(label, key);
        }
        for n in desc.keys.keys() {
            if !keys.contains_key(n) {
                return Err(e(Code::ArgMissing, format!("{name} needs key {n}")));
            }
        }
        Ok(Obj { class: name.to_string(), keys })
    }

    /// The value of one key of an object, by the key's declaration (16.1, 16.5).
    fn key_value(&mut self, ck: &ClassKey, x: Id, receiver: bool) -> Result<Key, Error> {
        match ck {
            ClassKey::Scalar { ty, key_normalisation, .. } => {
                let (ty, rule) = (*ty, *key_normalisation);
                // A receiver's key may be any expression (K7, 16.3): the plain read's argument.
                if receiver
                    && !matches!(
                        self.tree.get(x),
                        PNode::Str(_)
                            | PNode::KeyLit(_)
                            | PNode::Num { .. }
                            | PNode::Date(_)
                            | PNode::Instant(_)
                            | PNode::Bool(_)
                    )
                {
                    let id = self.node(x)?;
                    return Ok(Key { node: id, value: None });
                }
                let node = match self.tree.get(x) {
                    PNode::Str(s) | PNode::KeyLit(s) if ty == Type::String => Node::Str(s.clone()),
                    PNode::Num { text, unit } if ty == Type::Number => {
                        Node::Num { value: text.clone(), unit: unit.clone() }
                    }
                    PNode::Date(s) if ty == Type::Date => Node::Date(s.clone()),
                    PNode::Instant(s) if ty == Type::Instant => Node::Instant(s.clone()),
                    PNode::Bool(b) if ty == Type::Boolean => Node::Bool(*b),
                    PNode::Var(_) | PNode::Neg(_) | PNode::Not(_) | PNode::Bin(..) | PNode::Call { .. } => {
                        return Err(e(Code::Placement, "an object's key is a literal"));
                    }
                    _ => return Err(e(Code::Type, format!("a key of type {}", ty.as_str()))),
                };
                let Some(scalar) = scalar_key(ty, &node) else {
                    return Err(e(Code::Type, format!("a key of type {}", ty.as_str())));
                };
                if let (Node::Str(s), Some(rule)) = (&node, rule) {
                    rule.check(s).map_err(|_| e(Code::Key, format!("{s:?} is not canonical under {rule}")))?;
                }
                let id = self.push(node);
                Ok(Key { node: id, value: Some(KeyValue::Scalar(scalar)) })
            }
            ClassKey::Class(cname) => match self.tree.get(x) {
                PNode::Read { name, alias, pin: None, args }
                    if self.class_named(name, *alias).as_deref() == Some(cname.as_str()) =>
                {
                    let obj = self.object(cname, args, false)?;
                    let ok = object_key(&obj)?;
                    let text = format_object_key(&ok);
                    let id = self.push(Node::Str(text));
                    Ok(Key { node: id, value: Some(KeyValue::Object(ok)) })
                }
                PNode::Str(s) | PNode::KeyLit(s) => {
                    let ok = decode(s)?;
                    if ok.class != *cname {
                        return Err(e(Code::Type, format!("{} is not {cname}", ok.class)));
                    }
                    if let Some(env) = self.env {
                        check_encoding(env, &ok)?;
                    }
                    let id = self.push(Node::Str(s.clone()));
                    Ok(Key { node: id, value: Some(KeyValue::Object(ok)) })
                }
                PNode::Read { .. } => Err(e(Code::Type, format!("a key of class {cname}"))),
                _ => Err(e(Code::Placement, "an object's key is a literal")),
            },
            _ => Err(e(Code::Unsupported, "a key of this kind")),
        }
    }

    /// A method call `recv.name(args)` (16.3, 16.6). The receiver is an object (a class's name, or
    /// `this` in a method's body), `super` (the parent's method, pinned to its hash), or a key: an
    /// argument declaring `classes`, or a read or dispatch returning a key, which dispatches among
    /// those classes. A method is the read of `<class>/<name>`, bound to the class's method hash,
    /// with the object's keys first; an extension method is a qualified function whose first
    /// arguments are the class's keys in order.
    fn method(&mut self, recv: Id, name: &str, qualified: bool, args: &[PArg]) -> Result<NodeId, Error> {
        let env = self.env.ok_or_else(|| e(Code::Reserved, "a method needs an environment"))?;
        let obj = match self.tree.get(recv) {
            PNode::Super => return self.super_method(env, name, qualified, args),
            PNode::This => {
                let cls = env.method_of.as_deref().ok_or_else(|| {
                    e(Code::Reserved, "this is the receiver of a method call in a method's body only")
                })?;
                let desc = env.classes.get(cls).ok_or_else(|| e(Code::Unresolved, format!("{cls} is not bound")))?;
                let mut keys = BTreeMap::new();
                for k in desc.keys.keys() {
                    let id = self.push(Node::Var(k.clone()));
                    keys.insert(k.clone(), Key { node: id, value: None });
                }
                Obj { class: cls.to_string(), keys }
            }
            PNode::Read { name: cname, alias, pin: None, args: oargs } if self.class_named(cname, *alias).is_some() => {
                let class = self.class_named(cname, *alias).unwrap_or_default();
                self.object(&class, oargs, true)?
            }
            _ => return self.dispatch(recv, name, qualified, args),
        };
        let desc =
            env.classes.get(&obj.class).ok_or_else(|| e(Code::Unresolved, format!("{} is not bound", obj.class)))?;
        if qualified {
            let sig = env.signature_of(name).ok_or_else(|| e(Code::Unresolved, format!("{name} has no binding")))?;
            let order = sig.order.clone().unwrap_or_default();
            let k = desc.order.len();
            if order.len() < k || order[..k] != desc.order[..] {
                return Err(e(Code::Method, format!("{name} does not take the keys of {} first", obj.class)));
            }
            for key in &desc.order {
                let want = match desc.keys.get(key) {
                    Some(ClassKey::Scalar { ty, .. }) => *ty,
                    _ => Type::String,
                };
                if sig.args.get(key).map(|d| d.ty) != Some(want) {
                    return Err(e(Code::Method, format!("{name}: the key {key} is not of the class's type")));
                }
            }
            let pre = self.pre_keys(&obj, &order[..k])?;
            let bound = self.bind(name, Some(sig), pre, args)?;
            return Ok(self.push(Node::Read { fn_name: name.to_string(), fn_hash: None, args: bound }));
        }
        if desc.lookup.as_deref() == Some(name) || !desc.methods.contains_key(name) {
            return Err(e(Code::Method, format!("{} has no method {name}", obj.class)));
        }
        let fq = method_name(env, &obj.class, name)?;
        // A method reads itself only where its class is bound with its hash (a stored override); a
        // method of a draft (K7) that reads its own name through `this` is a mutual read (16.3).
        if fq == env.name && !env.functions.contains_key(&fq) {
            return Err(e(Code::Cycle, format!("{fq} reads itself through its method call (16.3)")));
        }
        let want = &desc.methods[name];
        let sig = env.signature_of(&fq).ok_or_else(|| e(Code::Unresolved, format!("{fq} has no binding")))?;
        if let Some(f) = env.functions.get(&fq) {
            if f.hash != *want {
                return Err(e(Code::Method, format!("{fq} is not bound to the method's hash")));
            }
        }
        let order = sig.order.clone().unwrap_or_default();
        // The keys the method takes first: the leading keys of its class that its signature has (16.2
        // step 6), which a subclass's override or an inherited method may lack the last of.
        let k = desc.order.iter().take_while(|n| sig.args.contains_key(*n)).count();
        if order.len() < k || order[..k] != desc.order[..k] {
            return Err(e(Code::Method, format!("{fq} does not take the keys of {} first", obj.class)));
        }
        let pre = self.pre_keys(&obj, &order[..k])?;
        let bound = self.bind(&fq, Some(sig), pre, args)?;
        Ok(self.push(Node::Read { fn_name: fq, fn_hash: None, args: bound }))
    }

    /// `super.name(args)` in a method's body (16.3): the parent's method by its hash, read with the
    /// parent's keys (this class's own, by name) first. The parent is the class's `parent`, found by
    /// its hash in the environment; without a parent, or without the method, `E_FORMULA_METHOD`.
    fn super_method(&mut self, env: &Env, name: &str, qualified: bool, args: &[PArg]) -> Result<NodeId, Error> {
        let cls = env
            .method_of
            .as_deref()
            .ok_or_else(|| e(Code::Reserved, "super is the receiver of a method call in a method's body only"))?;
        let desc = env.classes.get(cls).ok_or_else(|| e(Code::Unresolved, format!("{cls} is not bound")))?;
        let parent = desc.parent.as_deref().ok_or_else(|| e(Code::Method, format!("{cls} has no parent")))?;
        let (pname, pdesc) = env
            .classes
            .by_hash(parent)
            .ok_or_else(|| e(Code::ClassParent, format!("no class name is bound to the parent of {cls}")))?;
        if qualified || pdesc.lookup.as_deref() == Some(name) || !pdesc.methods.contains_key(name) {
            return Err(e(Code::Method, format!("{pname} has no method {name}")));
        }
        let hash = pdesc.methods[name].clone();
        let fq = format!("{pname}/{name}");
        let sig =
            signature_by_hash(env, &hash).ok_or_else(|| e(Code::Unresolved, format!("{fq}@{hash} is not known")))?;
        let pre: Vec<(String, NodeId)> =
            pdesc.order.iter().map(|k| (k.clone(), self.push(Node::Var(k.clone())))).collect();
        let bound = self.bind(&fq, Some(sig), pre, args)?;
        Ok(self.push(Node::Read { fn_name: fq, fn_hash: Some(hash), args: bound }))
    }

    /// A dispatched method call (16.6): the receiver is a key, whose classes are the declared ones
    /// (an argument's `classes`) or the returned ones (a read or dispatch returning a key). Every
    /// class must have the method, bound to its hash, and the same signature once its keys are taken
    /// away; the arguments bind to that signature.
    fn dispatch(&mut self, recv: Id, name: &str, qualified: bool, args: &[PArg]) -> Result<NodeId, Error> {
        let env = self.env.ok_or_else(|| e(Code::Reserved, "a method needs an environment"))?;
        let declared = match self.tree.get(recv) {
            PNode::Var(v) => env.signature.args.get(v).and_then(|d| d.extras.classes.clone()),
            _ => None,
        };
        let on = self.node(recv)?;
        let classes = match declared {
            Some(c) => Some(c),
            None => self.returned_classes(on),
        };
        let Some(mut classes) = classes else {
            return Err(e(Code::Reserved, format!(".{name}(...) is reserved: its receiver is not an object or a key")));
        };
        if qualified {
            return Err(e(Code::Method, format!("{name}: a dispatched method is named by its name")));
        }
        classes.sort();
        classes.dedup();
        let mut own: Option<Signature> = None;
        for c in &classes {
            let desc = env.classes.get(c).ok_or_else(|| e(Code::Unresolved, format!("{c} is not bound")))?;
            if desc.lookup.as_deref() == Some(name) || !desc.methods.contains_key(name) {
                return Err(e(Code::Method, format!("{c} has no method {name}")));
            }
            let fq = method_name(env, c, name)?;
            let f = env.functions.get(&fq).ok_or_else(|| e(Code::Unresolved, format!("{fq} has no binding")))?;
            if f.hash != desc.methods[name] && !env.evaluating {
                return Err(e(Code::Method, format!("{fq} is not bound to the method's hash")));
            }
            let order = f.signature.order.clone().unwrap_or_default();
            // The keys the function takes first: the leading keys of the class its signature has (K5).
            let k = desc.order.iter().take_while(|n| f.signature.args.contains_key(*n)).count();
            if order.len() < k || order[..k] != desc.order[..k] {
                return Err(e(Code::Method, format!("{fq} does not take the keys of {c} first")));
            }
            let sig = own_signature(&f.signature, k);
            match &own {
                None => own = Some(sig),
                Some(prev) if *prev != sig => {
                    return Err(e(Code::Dispatch, format!("{name} differs in signature between the dispatch classes")));
                }
                Some(_) => {}
            }
        }
        let Some(own) = own else { return Err(e(Code::Dispatch, "a dispatch over no class")) };
        let mut bound = Vec::new();
        for (n, a) in self.bind(name, Some(&own), Vec::new(), args)? {
            match a {
                ReadArg::Expr(x) => bound.push((n, x)),
                _ => return Err(e(Code::Placement, "* and ranges are arguments of a read only")),
            }
        }
        Ok(self.push(Node::Dispatch { on, method: name.to_string(), classes, args: bound }))
    }

    /// The classes of the key a node returns (16.6): a read's or a dispatch's declared classes.
    fn returned_classes(&self, id: NodeId) -> Option<Vec<String>> {
        let env = self.env?;
        match &self.out[id as usize] {
            Node::Read { fn_name, fn_hash, .. } => {
                let sig = match fn_hash {
                    Some(h) => signature_by_hash(env, h),
                    None => env.signature_of(fn_name),
                };
                sig.and_then(|s| s.returns_classes.clone())
            }
            Node::Dispatch { classes, method, .. } => classes
                .first()
                .and_then(|c| method_name(env, c, method).ok())
                .and_then(|fq| env.signature_of(&fq))
                .and_then(|s| s.returns_classes.clone()),
            _ => None,
        }
    }

    /// The keys of an object named `names`, as the first arguments of a method's function.
    fn pre_keys(&self, obj: &Obj, names: &[String]) -> Result<Vec<(String, NodeId)>, Error> {
        names
            .iter()
            .map(|n| {
                obj.keys
                    .get(n)
                    .map(|k| (n.clone(), k.node))
                    .ok_or_else(|| e(Code::ArgMissing, format!("the object has no key {n}")))
            })
            .collect()
    }

    fn encode_key(&self, obj: &Obj) -> Result<String, Error> {
        Ok(format_object_key(&object_key(obj)?))
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
                    PNode::Var(w) | PNode::KeyLit(w) => Some(w.to_ascii_lowercase()),
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
                if let PNode::Var(w) | PNode::KeyLit(w) = self.tree.get(*x) {
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

/// A key-normalised string argument (7, 16.4): canonical under its rule, when it has one.
fn check_key(rule: &Option<KeyNormalisation>, s: &str) -> Result<(), Error> {
    match rule {
        Some(r) => r.check(s).map_err(|_| e(Code::Key, format!("{s:?} is not canonical under {r}"))),
        None => Ok(()),
    }
}
