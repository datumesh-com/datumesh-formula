//! The canonical AST (SPEC 15.5): its JSON form and hash, and the printer of 15.6.
//!
//! The tree is an arena, so that a body of 65,536 nodes in one chain (`a + a + ...`) is
//! neither dropped nor serialised recursively.

use datumesh_core::json::Value;

use crate::parse::BinOp;

/// A node index in an [`Ast`].
pub type NodeId = u32;

/// An operator node's operator: a binary operator, `neg` or `not`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `*`.
    Mul,
    /// `/`.
    Div,
    /// `^`.
    Pow,
    /// `&`.
    Cat,
    /// `=`.
    Eq,
    /// `<>`.
    Ne,
    /// `<`.
    Lt,
    /// `<=`.
    Le,
    /// `>`.
    Gt,
    /// `>=`.
    Ge,
    /// `and`.
    And,
    /// `or`.
    Or,
    /// Unary minus.
    Neg,
    /// `not`.
    Not,
}

impl Op {
    /// The operator as written in the AST's `op` member.
    pub fn text(self) -> &'static str {
        match self {
            Op::Neg => "neg",
            Op::Not => "not",
            o => o.binary().map(BinOp::text).unwrap_or(""),
        }
    }

    pub(crate) fn binary(self) -> Option<BinOp> {
        Some(match self {
            Op::Add => BinOp::Add,
            Op::Sub => BinOp::Sub,
            Op::Mul => BinOp::Mul,
            Op::Div => BinOp::Div,
            Op::Pow => BinOp::Pow,
            Op::Cat => BinOp::Cat,
            Op::Eq => BinOp::Eq,
            Op::Ne => BinOp::Ne,
            Op::Lt => BinOp::Lt,
            Op::Le => BinOp::Le,
            Op::Gt => BinOp::Gt,
            Op::Ge => BinOp::Ge,
            Op::And => BinOp::And,
            Op::Or => BinOp::Or,
            Op::Neg | Op::Not => return None,
        })
    }

    pub(crate) fn from_binary(b: BinOp) -> Op {
        match b {
            BinOp::Add => Op::Add,
            BinOp::Sub => Op::Sub,
            BinOp::Mul => Op::Mul,
            BinOp::Div => Op::Div,
            BinOp::Pow => Op::Pow,
            BinOp::Cat => Op::Cat,
            BinOp::Eq => Op::Eq,
            BinOp::Ne => Op::Ne,
            BinOp::Lt => Op::Lt,
            BinOp::Le => Op::Le,
            BinOp::Gt => Op::Gt,
            BinOp::Ge => Op::Ge,
            BinOp::And => Op::And,
            BinOp::Or => Op::Or,
        }
    }

    /// Whether this is a comparison.
    pub fn is_comparison(self) -> bool {
        matches!(self, Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge)
    }

    /// The level of the precedence table of 15.4 (1 tightest).
    pub(crate) fn level(self) -> u8 {
        match self {
            Op::Pow => 3,
            Op::Neg => 4,
            Op::Mul | Op::Div => 5,
            Op::Add | Op::Sub => 6,
            Op::Cat => 7,
            Op::Eq | Op::Ne | Op::Lt | Op::Le | Op::Gt | Op::Ge => 8,
            Op::Not => 9,
            Op::And => 10,
            Op::Or => 11,
        }
    }
}

/// A read argument: an expression, `*` (open) or a range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadArg {
    /// An expression.
    Expr(NodeId),
    /// The open marker `*`.
    Open,
    /// A range `[lo..hi]`.
    Range(NodeId, NodeId),
}

/// A canonical AST node (15.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    /// A number: its canonical lexical form and optional unit suffix.
    Num {
        /// The canonical form.
        value: String,
        /// The unit suffix.
        unit: Option<String>,
    },
    /// A string.
    Str(String),
    /// A boolean.
    Bool(bool),
    /// A date.
    Date(String),
    /// An instant.
    Instant(String),
    /// A variable: an argument, a `let` name or a lambda parameter.
    Var(String),
    /// An operator and its operands.
    Op {
        /// The operator.
        op: Op,
        /// One or two operands.
        args: Vec<NodeId>,
    },
    /// `if cond then a else b`.
    If {
        /// The condition.
        cond: NodeId,
        /// The `then` branch.
        then: NodeId,
        /// The `else` branch.
        els: NodeId,
    },
    /// `let name = value in body`.
    Let {
        /// The name.
        name: String,
        /// The value.
        value: NodeId,
        /// The body.
        body: NodeId,
    },
    /// A cell read or slice: the function and its arguments, ascending by name.
    Read {
        /// The fully qualified function name.
        fn_name: String,
        /// The arguments, ascending by name.
        args: Vec<(String, ReadArg)>,
    },
    /// A built-in call: required arguments positional, optional ones named (in the built-in's
    /// parameter order, which is how they print; the JSON form sorts them).
    Call {
        /// The built-in's name, lowercase.
        fn_name: String,
        /// The positional (required) arguments.
        args: Vec<NodeId>,
        /// The named (optional) arguments, in parameter order.
        named: Vec<(String, NodeId)>,
    },
    /// A lambda.
    Lambda {
        /// The parameters.
        params: Vec<String>,
        /// The body.
        body: NodeId,
    },
    /// The aggregate of `scan` and `window`.
    Agg(String),
    /// The dimension named by `along`.
    Key(String),
}

/// A canonical AST: an arena of nodes and its root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ast {
    pub(crate) nodes: Vec<Node>,
    pub(crate) root: NodeId,
}

impl Ast {
    /// An AST from its arena and root, if every node refers only to nodes before it (as a
    /// parse builds them) and the root is in the arena.
    pub fn from_parts(nodes: Vec<Node>, root: NodeId) -> Option<Ast> {
        let ast = Ast { nodes, root };
        if root as usize >= ast.nodes.len() {
            return None;
        }
        for i in 0..ast.nodes.len() {
            if ast.children(i as NodeId).iter().any(|&c| c as usize >= i) {
                return None;
            }
        }
        Some(ast)
    }

    /// The root node's id.
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// A node.
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    /// The number of nodes in the JSON form (open and range markers included).
    pub fn node_count(&self) -> usize {
        let mut n = 0;
        for node in &self.nodes {
            n += 1;
            if let Node::Read { args, .. } = node {
                n += args.iter().filter(|(_, a)| !matches!(a, ReadArg::Expr(_))).count();
            }
        }
        n
    }

    pub(crate) fn children(&self, id: NodeId) -> Vec<NodeId> {
        match self.node(id) {
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
            Node::Call { args, named, .. } => args.iter().copied().chain(named.iter().map(|(_, n)| *n)).collect(),
            Node::Lambda { body, .. } => vec![*body],
            _ => Vec::new(),
        }
    }

    /// The deepest path from the root, counted in nodes (computed without recursion).
    pub fn depth(&self) -> usize {
        // Children always precede their parents in the arena.
        let mut d = vec![1usize; self.nodes.len()];
        for i in 0..self.nodes.len() {
            let m = self.children(i as NodeId).iter().map(|c| d[*c as usize]).max().unwrap_or(0);
            d[i] = m + 1;
        }
        d.get(self.root as usize).copied().unwrap_or(0)
    }

    /// The canonical JSON form's JCS bytes (RFC 8785): every object's members ascending, every
    /// string escaped by `datumesh-core`'s JCS. Written with an explicit stack.
    pub fn to_jcs(&self) -> Vec<u8> {
        enum Item<'a> {
            Node(NodeId),
            Text(&'static str),
            Str(&'a str),
            Arg(&'a ReadArg),
        }
        let mut out = Vec::new();
        let mut stack: Vec<Item> = vec![Item::Node(self.root)];
        while let Some(item) = stack.pop() {
            match item {
                Item::Text(t) => out.extend_from_slice(t.as_bytes()),
                Item::Str(s) => out.extend_from_slice(&Value::String(s.to_string()).to_jcs()),
                Item::Arg(a) => match a {
                    ReadArg::Expr(e) => stack.push(Item::Node(*e)),
                    ReadArg::Open => out.extend_from_slice(b"{\"node\":\"open\"}"),
                    ReadArg::Range(lo, hi) => {
                        stack.push(Item::Text(",\"node\":\"range\"}"));
                        stack.push(Item::Node(*lo));
                        stack.push(Item::Text(",\"lo\":"));
                        stack.push(Item::Node(*hi));
                        stack.push(Item::Text("{\"hi\":"));
                    }
                },
                Item::Node(id) => {
                    // Pushed in reverse.
                    let mut seq: Vec<Item> = Vec::new();
                    match self.node(id) {
                        Node::Num { value, unit } => {
                            seq.push(Item::Text("{\"node\":\"num\","));
                            if let Some(u) = unit {
                                seq.push(Item::Text("\"unit\":"));
                                seq.push(Item::Str(u));
                                seq.push(Item::Text(","));
                            }
                            seq.push(Item::Text("\"value\":"));
                            seq.push(Item::Str(value));
                            seq.push(Item::Text("}"));
                        }
                        Node::Str(s) => {
                            seq.push(Item::Text("{\"node\":\"str\",\"value\":"));
                            seq.push(Item::Str(s));
                            seq.push(Item::Text("}"));
                        }
                        Node::Bool(b) => {
                            seq.push(Item::Text(if *b {
                                "{\"node\":\"bool\",\"value\":true}"
                            } else {
                                "{\"node\":\"bool\",\"value\":false}"
                            }));
                        }
                        Node::Date(s) => {
                            seq.push(Item::Text("{\"node\":\"date\",\"value\":"));
                            seq.push(Item::Str(s));
                            seq.push(Item::Text("}"));
                        }
                        Node::Instant(s) => {
                            seq.push(Item::Text("{\"node\":\"instant\",\"value\":"));
                            seq.push(Item::Str(s));
                            seq.push(Item::Text("}"));
                        }
                        Node::Var(n) => {
                            seq.push(Item::Text("{\"name\":"));
                            seq.push(Item::Str(n));
                            seq.push(Item::Text(",\"node\":\"var\"}"));
                        }
                        Node::Op { op, args } => {
                            seq.push(Item::Text("{\"args\":["));
                            for (i, a) in args.iter().enumerate() {
                                if i > 0 {
                                    seq.push(Item::Text(","));
                                }
                                seq.push(Item::Node(*a));
                            }
                            seq.push(Item::Text("],\"node\":\"op\",\"op\":"));
                            seq.push(Item::Str(op.text()));
                            seq.push(Item::Text("}"));
                        }
                        Node::If { cond, then, els } => {
                            seq.push(Item::Text("{\"cond\":"));
                            seq.push(Item::Node(*cond));
                            seq.push(Item::Text(",\"else\":"));
                            seq.push(Item::Node(*els));
                            seq.push(Item::Text(",\"node\":\"if\",\"then\":"));
                            seq.push(Item::Node(*then));
                            seq.push(Item::Text("}"));
                        }
                        Node::Let { name, value, body } => {
                            seq.push(Item::Text("{\"body\":"));
                            seq.push(Item::Node(*body));
                            seq.push(Item::Text(",\"name\":"));
                            seq.push(Item::Str(name));
                            seq.push(Item::Text(",\"node\":\"let\",\"value\":"));
                            seq.push(Item::Node(*value));
                            seq.push(Item::Text("}"));
                        }
                        Node::Read { fn_name, args } => {
                            seq.push(Item::Text("{\"args\":{"));
                            for (i, (k, a)) in args.iter().enumerate() {
                                if i > 0 {
                                    seq.push(Item::Text(","));
                                }
                                seq.push(Item::Str(k));
                                seq.push(Item::Text(":"));
                                seq.push(Item::Arg(a));
                            }
                            seq.push(Item::Text("},\"fn\":"));
                            seq.push(Item::Str(fn_name));
                            seq.push(Item::Text(",\"node\":\"read\"}"));
                        }
                        Node::Call { fn_name, args, named } => {
                            seq.push(Item::Text("{\"args\":["));
                            for (i, a) in args.iter().enumerate() {
                                if i > 0 {
                                    seq.push(Item::Text(","));
                                }
                                seq.push(Item::Node(*a));
                            }
                            seq.push(Item::Text("],\"fn\":"));
                            seq.push(Item::Str(fn_name));
                            seq.push(Item::Text(",\"named\":{"));
                            let mut sorted: Vec<&(String, NodeId)> = named.iter().collect();
                            sorted.sort_by(|a, b| a.0.cmp(&b.0));
                            for (i, (k, a)) in sorted.into_iter().enumerate() {
                                if i > 0 {
                                    seq.push(Item::Text(","));
                                }
                                seq.push(Item::Str(k));
                                seq.push(Item::Text(":"));
                                seq.push(Item::Node(*a));
                            }
                            seq.push(Item::Text("},\"node\":\"call\"}"));
                        }
                        Node::Lambda { params, body } => {
                            seq.push(Item::Text("{\"body\":"));
                            seq.push(Item::Node(*body));
                            seq.push(Item::Text(",\"node\":\"lambda\",\"params\":["));
                            for (i, p) in params.iter().enumerate() {
                                if i > 0 {
                                    seq.push(Item::Text(","));
                                }
                                seq.push(Item::Str(p));
                            }
                            seq.push(Item::Text("]}"));
                        }
                        Node::Agg(n) => {
                            seq.push(Item::Text("{\"name\":"));
                            seq.push(Item::Str(n));
                            seq.push(Item::Text(",\"node\":\"agg\"}"));
                        }
                        Node::Key(n) => {
                            seq.push(Item::Text("{\"name\":"));
                            seq.push(Item::Str(n));
                            seq.push(Item::Text(",\"node\":\"key\"}"));
                        }
                    }
                    stack.extend(seq.into_iter().rev());
                }
            }
        }
        out
    }

    /// The canonical AST hash: SHA-256 of the JCS bytes, lowercase hex.
    pub fn hash(&self) -> String {
        datumesh_core::sha256_hex(&self.to_jcs())
    }

    /// The printed canonical AST (15.6), without the final LF.
    pub fn print(&self) -> String {
        print(self)
    }

    /// `code.body`: the printed canonical AST followed by one LF.
    pub fn body(&self) -> String {
        let mut s = print(self);
        s.push('\n');
        s
    }
}

/// The precedence level of a node as an operand (1: primary; 12: if, let, lambda).
fn level(ast: &Ast, id: NodeId) -> u8 {
    match ast.node(id) {
        Node::Op { op, .. } => op.level(),
        Node::If { .. } | Node::Let { .. } | Node::Lambda { .. } => 12,
        _ => 1,
    }
}

/// Whether a number literal prints as a negative number would; numbers never do (`-` is `neg`).
fn print(ast: &Ast) -> String {
    enum Item<'a> {
        Node(NodeId, bool),
        Text(&'a str),
        Owned(String),
    }
    let mut out = String::new();
    let mut stack: Vec<Item> = vec![Item::Node(ast.root, false)];
    while let Some(item) = stack.pop() {
        match item {
            Item::Text(t) => out.push_str(t),
            Item::Owned(t) => out.push_str(&t),
            Item::Node(id, paren) => {
                let mut seq: Vec<Item> = Vec::new();
                if paren {
                    seq.push(Item::Text("("));
                }
                match ast.node(id) {
                    Node::Num { value, unit } => {
                        seq.push(Item::Text(value));
                        if let Some(u) = unit {
                            seq.push(Item::Text(" "));
                            seq.push(Item::Text(u));
                        }
                    }
                    Node::Str(s) => seq.push(Item::Owned(format!("\"{}\"", s.replace('"', "\"\"")))),
                    Node::Bool(b) => seq.push(Item::Text(if *b { "true" } else { "false" })),
                    Node::Date(s) | Node::Instant(s) => seq.push(Item::Text(s)),
                    Node::Var(n) | Node::Agg(n) | Node::Key(n) => seq.push(Item::Text(n)),
                    Node::Op { op, args } => match op {
                        Op::Neg => {
                            seq.push(Item::Text("-"));
                            let a = args[0];
                            // The operand of unary minus is a postfix expression or another minus.
                            let l = level(ast, a);
                            seq.push(Item::Node(a, !(l == 1 || l == 4)));
                        }
                        Op::Not => {
                            seq.push(Item::Text("not "));
                            let a = args[0];
                            seq.push(Item::Node(a, level(ast, a) > 9));
                        }
                        Op::Pow => {
                            let (b, e) = (args[0], args[1]);
                            seq.push(Item::Node(b, level(ast, b) != 1));
                            seq.push(Item::Text(" ^ "));
                            // An exponent is a postfix expression after any number of minus signs.
                            let mut cur = e;
                            while let Node::Op { op: Op::Neg, args } = ast.node(cur) {
                                seq.push(Item::Text("-"));
                                cur = args[0];
                            }
                            seq.push(Item::Node(cur, level(ast, cur) != 1));
                        }
                        _ => {
                            let (l, r) = (args[0], args[1]);
                            let me = op.level();
                            let (lp, rp) = if op.is_comparison() {
                                (level(ast, l) >= me, level(ast, r) >= me)
                            } else {
                                (level(ast, l) > me, level(ast, r) >= me)
                            };
                            seq.push(Item::Node(l, lp));
                            seq.push(Item::Text(" "));
                            seq.push(Item::Text(op.text()));
                            seq.push(Item::Text(" "));
                            seq.push(Item::Node(r, rp));
                        }
                    },
                    Node::If { cond, then, els } => {
                        seq.push(Item::Text("if "));
                        seq.push(Item::Node(*cond, false));
                        seq.push(Item::Text(" then "));
                        seq.push(Item::Node(*then, false));
                        seq.push(Item::Text(" else "));
                        seq.push(Item::Node(*els, false));
                    }
                    Node::Let { name, value, body } => {
                        seq.push(Item::Text("let "));
                        seq.push(Item::Text(name));
                        seq.push(Item::Text(" = "));
                        seq.push(Item::Node(*value, false));
                        seq.push(Item::Text(" in "));
                        seq.push(Item::Node(*body, false));
                    }
                    Node::Read { fn_name, args } => {
                        seq.push(Item::Text(fn_name));
                        seq.push(Item::Text("("));
                        for (i, (k, a)) in args.iter().enumerate() {
                            if i > 0 {
                                seq.push(Item::Text(", "));
                            }
                            seq.push(Item::Text(k));
                            seq.push(Item::Text(": "));
                            match a {
                                ReadArg::Expr(e) => seq.push(Item::Node(*e, false)),
                                ReadArg::Open => seq.push(Item::Text("*")),
                                ReadArg::Range(lo, hi) => {
                                    seq.push(Item::Text("["));
                                    seq.push(Item::Node(*lo, false));
                                    seq.push(Item::Text(".."));
                                    seq.push(Item::Node(*hi, false));
                                    seq.push(Item::Text("]"));
                                }
                            }
                        }
                        seq.push(Item::Text(")"));
                    }
                    Node::Call { fn_name, args, named } => {
                        seq.push(Item::Text(fn_name));
                        seq.push(Item::Text("("));
                        let mut first = true;
                        for a in args {
                            if !first {
                                seq.push(Item::Text(", "));
                            }
                            first = false;
                            seq.push(Item::Node(*a, false));
                        }
                        for (k, a) in named {
                            if !first {
                                seq.push(Item::Text(", "));
                            }
                            first = false;
                            seq.push(Item::Text(k));
                            seq.push(Item::Text(": "));
                            seq.push(Item::Node(*a, false));
                        }
                        seq.push(Item::Text(")"));
                    }
                    Node::Lambda { params, body } => {
                        if params.len() == 1 {
                            seq.push(Item::Text(&params[0]));
                        } else {
                            seq.push(Item::Owned(format!("({})", params.join(", "))));
                        }
                        seq.push(Item::Text(" -> "));
                        seq.push(Item::Node(*body, false));
                    }
                }
                if paren {
                    seq.push(Item::Text(")"));
                }
                stack.extend(seq.into_iter().rev());
            }
        }
    }
    out
}
