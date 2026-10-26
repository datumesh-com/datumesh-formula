//! The parser of SPEC 15.3-15.4 and Appendix A: a recursive-descent reading of the PEG, with the
//! precedence table of 15.4 and its rejected forms. The result is a parse tree; resolution
//! (15.5) turns it into the canonical AST.

use crate::lex::{is_keyword, Kind, Token};
use crate::{Code, Error};

/// A node index in the parse tree's arena.
pub(crate) type Id = u32;

/// A binary operator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Cat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub(crate) fn text(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Pow => "^",
            BinOp::Cat => "&",
            BinOp::Eq => "=",
            BinOp::Ne => "<>",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "and",
            BinOp::Or => "or",
        }
    }

    pub(crate) fn from_text(s: &str) -> Option<BinOp> {
        Some(match s {
            "+" => BinOp::Add,
            "-" => BinOp::Sub,
            "*" => BinOp::Mul,
            "/" => BinOp::Div,
            "^" => BinOp::Pow,
            "&" => BinOp::Cat,
            "=" => BinOp::Eq,
            "<>" => BinOp::Ne,
            "<" => BinOp::Lt,
            "<=" => BinOp::Le,
            ">" => BinOp::Gt,
            ">=" => BinOp::Ge,
            "and" => BinOp::And,
            "or" => BinOp::Or,
            _ => return None,
        })
    }

    pub(crate) fn is_comparison(self) -> bool {
        matches!(self, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
    }
}

/// A read or call argument value.
#[derive(Clone, Debug)]
pub(crate) enum PVal {
    Expr(Id),
    Open,
    Range(Id, Id),
}

/// A read or call argument: an optional label and a value.
#[derive(Clone, Debug)]
pub(crate) struct PArg {
    pub(crate) label: Option<String>,
    pub(crate) value: PVal,
}

/// A parse-tree node.
#[derive(Clone, Debug)]
pub(crate) enum PNode {
    Num {
        text: String,
        unit: Option<String>,
    },
    Str(String),
    Bool(bool),
    Date(String),
    Instant(String),
    Var(String),
    Bin(BinOp, Id, Id),
    Neg(Id),
    Not(Id),
    If(Id, Id, Id),
    Let(String, Id, Id),
    /// A read: the qualified name or alias (without `@`), its pin (a request form, 5.8) and
    /// its arguments.
    Read {
        name: String,
        alias: bool,
        pin: Option<String>,
        args: Vec<PArg>,
    },
    /// A built-in call (name lowercased).
    Call {
        name: String,
        args: Vec<PArg>,
    },
    Lambda(Vec<String>, Id),
}

/// The parse tree: an arena and its root.
pub(crate) struct Tree {
    pub(crate) nodes: Vec<PNode>,
    pub(crate) root: Id,
}

impl Tree {
    pub(crate) fn get(&self, id: Id) -> &PNode {
        &self.nodes[id as usize]
    }
}

/// The deepest syntactic nesting allowed (15.3).
pub const MAX_NESTING: u32 = 256;

/// The built-in names of 15.7 (lowercase), with `if`, `and`, `or`.
pub const BUILTINS: &[&str] = &[
    "sum",
    "count",
    "mean",
    "min",
    "max",
    "first",
    "last",
    "median",
    "stdev",
    "stdev_p",
    "at",
    "latest",
    "last_before",
    "first_after",
    "nearest",
    "within",
    "interpolate",
    "map",
    "filter",
    "scan",
    "window",
    "otherwise",
    "asof",
    "versions",
    "abs",
    "floor",
    "ceiling",
    "round",
    "round_up",
    "round_down",
    "mod",
    "sqrt",
    "exp",
    "ln",
    "sin",
    "cos",
    "atan2",
    "pi",
    "haversine",
    "convert",
    "if",
    "and",
    "or",
    "date",
    "year",
    "month",
    "day",
    "weekday",
    "end_of_month",
    "add_days",
    "add_months",
    "business_days",
    "year_fraction",
    "is_holiday",
    "date_of",
    "instant",
    "len",
    "upper",
    "lower",
    "trim",
    "concat",
    "substr",
    "find",
    "text",
    "fixed",
];

/// Function names that do not exist, when called (15.2.1 rule 3).
pub const FORBIDDEN: &[&str] = &[
    "now",
    "today",
    "rand",
    "randbetween",
    "randarray",
    "indirect",
    "offset",
    "info",
    "cell",
    "clock",
    "time",
    "random",
    "sleep",
    "fetch",
    "http",
    "eval",
];

/// Parse a lexed body.
pub(crate) fn parse(tokens: &[Token]) -> Result<Tree, Error> {
    let mut p = Parser { t: tokens, i: 0, nodes: Vec::new(), depth: 0 };
    let root = p.expr()?;
    if p.peek().kind != Kind::End {
        return Err(p.unexpected("the end"));
    }
    Ok(Tree { nodes: p.nodes, root })
}

struct Parser<'a> {
    t: &'a [Token],
    i: usize,
    nodes: Vec<PNode>,
    depth: u32,
}

fn err(code: Code, detail: impl Into<String>) -> Error {
    Error::new(code, detail)
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &'a Token {
        &self.t[self.i.min(self.t.len() - 1)]
    }

    fn peek_at(&self, k: usize) -> &'a Token {
        &self.t[(self.i + k).min(self.t.len() - 1)]
    }

    fn bump(&mut self) -> &'a Token {
        let t = self.peek();
        if self.i < self.t.len() - 1 {
            self.i += 1;
        }
        t
    }

    fn push(&mut self, n: PNode) -> Id {
        self.nodes.push(n);
        (self.nodes.len() - 1) as Id
    }

    fn unexpected(&self, expected: &str) -> Error {
        let t = self.peek();
        if t.is("->") {
            return err(Code::Placement, "a lambda is only an argument of map or filter");
        }
        if t.kind == Kind::Word && is_keyword(&t.text) {
            return err(Code::Syntax, format!("unexpected keyword, found {} at {}", t.describe(), t.pos));
        }
        err(Code::Syntax, format!("expected {expected}, found {} at {}", t.describe(), t.pos))
    }

    fn expect(&mut self, p: &str) -> Result<(), Error> {
        if self.peek().is(p) {
            self.bump();
            Ok(())
        } else {
            Err(self.unexpected(&format!("'{p}'")))
        }
    }

    fn expect_word(&mut self, w: &str) -> Result<(), Error> {
        if self.peek().is_word(w) {
            self.bump();
            Ok(())
        } else {
            Err(self.unexpected(&format!("'{w}'")))
        }
    }

    fn enter(&mut self) -> Result<(), Error> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            return Err(err(Code::Limit, "nesting deeper than 256"));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// `expr = or-expr / if-expr / let-expr`, and lambdas where an expression starts.
    fn expr(&mut self) -> Result<Id, Error> {
        self.enter()?;
        let r = self.expr_inner();
        self.leave();
        r
    }

    fn expr_inner(&mut self) -> Result<Id, Error> {
        let t = self.peek();
        if t.is_word("if") && !self.if_is_call() {
            return self.if_expr();
        }
        if t.is_word("let") {
            return self.let_expr();
        }
        self.or_expr()
    }

    /// An expression that may be a lambda (the second argument of `map` and `filter`).
    fn expr_or_lambda(&mut self) -> Result<Id, Error> {
        self.enter()?;
        let r = match self.lambda_head() {
            Some((params, n)) => {
                self.i += n;
                let body = self.expr()?;
                Ok(self.push(PNode::Lambda(params, body)))
            }
            None => self.expr_inner(),
        };
        self.leave();
        r
    }

    /// `name "->"` or `"(" name *( "," name ) ")" "->"` at the cursor: the parameter names and
    /// the number of tokens of the head.
    fn lambda_head(&self) -> Option<(Vec<String>, usize)> {
        let t = self.peek();
        if t.kind == Kind::Word && self.peek_at(1).is("->") {
            return Some((vec![t.text.clone()], 2));
        }
        if t.is("(") {
            let mut k = 1;
            let mut names = Vec::new();
            loop {
                let w = self.peek_at(k);
                if w.kind != Kind::Word {
                    return None;
                }
                names.push(w.text.clone());
                let sep = self.peek_at(k + 1);
                if sep.is(",") {
                    k += 2;
                    continue;
                }
                if sep.is(")") && self.peek_at(k + 2).is("->") {
                    return Some((names, k + 3));
                }
                return None;
            }
        }
        None
    }

    /// At `if (`: the function form exactly when the first parenthesised expression is
    /// followed by `,` (15.3), decided by scanning to the matching parenthesis.
    fn if_is_call(&self) -> bool {
        if !self.peek_at(1).is("(") {
            return false;
        }
        if self.peek_at(2).kind == Kind::Word && self.peek_at(3).is(":") {
            return false;
        }
        let mut depth = 0i64;
        let mut k = 1;
        loop {
            let t = self.peek_at(k);
            match t.kind {
                Kind::End => return false,
                Kind::Punct => match t.text.as_str() {
                    "(" | "[" => depth += 1,
                    ")" | "]" => {
                        depth -= 1;
                        if depth == 0 {
                            return false;
                        }
                    }
                    "," if depth == 1 => return true,
                    _ => {}
                },
                _ => {}
            }
            k += 1;
        }
    }

    fn if_expr(&mut self) -> Result<Id, Error> {
        self.bump(); // if
        let c = self.expr()?;
        self.expect_word("then")?;
        let a = self.expr()?;
        self.expect_word("else")?;
        let b = self.expr()?;
        Ok(self.push(PNode::If(c, a, b)))
    }

    fn let_expr(&mut self) -> Result<Id, Error> {
        self.bump(); // let
        let t = self.peek();
        if t.kind != Kind::Word {
            // As the reference reports it: an operator that cannot follow is its own error.
            let code = match t.text.as_str() {
                _ if t.kind != Kind::Punct => Code::Syntax,
                "=" | "<>" | "<" | "<=" | ">" | ">=" => Code::Chain,
                "^" => Code::Ambiguous,
                "." => Code::Reserved,
                "->" => Code::Placement,
                _ => Code::Syntax,
            };
            return Err(err(code, format!("expected a name after let, found {} at {}", t.describe(), t.pos)));
        }
        if is_keyword(&t.text) {
            return Err(err(Code::Reserved, format!("{} cannot be a name", t.text)));
        }
        let name = t.text.clone();
        self.bump();
        self.expect("=")?;
        let v = self.expr()?;
        self.expect_word("in")?;
        let body = self.expr()?;
        Ok(self.push(PNode::Let(name, v, body)))
    }

    fn or_expr(&mut self) -> Result<Id, Error> {
        let mut l = self.and_expr()?;
        while self.peek().is_word("or") {
            self.bump();
            let r = self.and_expr()?;
            l = self.push(PNode::Bin(BinOp::Or, l, r));
        }
        Ok(l)
    }

    fn and_expr(&mut self) -> Result<Id, Error> {
        let mut l = self.not_expr()?;
        while self.peek().is_word("and") {
            self.bump();
            let r = self.not_expr()?;
            l = self.push(PNode::Bin(BinOp::And, l, r));
        }
        Ok(l)
    }

    fn not_expr(&mut self) -> Result<Id, Error> {
        if self.peek().is_word("not") {
            self.bump();
            self.enter()?;
            let r = self.not_expr();
            self.leave();
            let x = r?;
            return Ok(self.push(PNode::Not(x)));
        }
        self.cmp_expr()
    }

    fn cmp_op(&self) -> Option<BinOp> {
        let t = self.peek();
        if t.kind != Kind::Punct {
            return None;
        }
        BinOp::from_text(&t.text).filter(|o| o.is_comparison())
    }

    fn cmp_expr(&mut self) -> Result<Id, Error> {
        let l = self.cat_expr()?;
        if let Some(op) = self.cmp_op() {
            self.bump();
            let r = self.cat_expr()?;
            if self.cmp_op().is_some() {
                return Err(err(Code::Chain, "comparisons do not chain: write and"));
            }
            return Ok(self.push(PNode::Bin(op, l, r)));
        }
        Ok(l)
    }

    fn cat_expr(&mut self) -> Result<Id, Error> {
        let mut l = self.add_expr()?;
        while self.peek().is("&") {
            self.bump();
            let r = self.add_expr()?;
            l = self.push(PNode::Bin(BinOp::Cat, l, r));
        }
        Ok(l)
    }

    fn add_expr(&mut self) -> Result<Id, Error> {
        let mut l = self.mul_expr()?;
        loop {
            let op = if self.peek().is("+") {
                BinOp::Add
            } else if self.peek().is("-") {
                BinOp::Sub
            } else {
                return Ok(l);
            };
            self.bump();
            let r = self.mul_expr()?;
            l = self.push(PNode::Bin(op, l, r));
        }
    }

    fn mul_expr(&mut self) -> Result<Id, Error> {
        let mut l = self.unary()?;
        loop {
            let op = if self.peek().is("*") {
                BinOp::Mul
            } else if self.peek().is("/") {
                BinOp::Div
            } else {
                return Ok(l);
            };
            self.bump();
            let r = self.unary()?;
            l = self.push(PNode::Bin(op, l, r));
        }
    }

    /// `unary = 1*"-" postfix / power`; "-" never takes a power.
    fn unary(&mut self) -> Result<Id, Error> {
        if self.peek().is("+") {
            return Err(err(Code::Syntax, "there is no unary +"));
        }
        if self.peek().is("-") {
            let mut n = 0;
            while self.peek().is("-") {
                self.bump();
                n += 1;
            }
            if self.peek().is("+") {
                return Err(err(Code::Syntax, "there is no unary +"));
            }
            let mut x = self.postfix()?;
            if self.peek().is("^") {
                return Err(err(Code::Ambiguous, "-a^b: write (-a)^b or -(a^b)"));
            }
            for _ in 0..n {
                x = self.push(PNode::Neg(x));
            }
            return Ok(x);
        }
        self.power()
    }

    /// `power = postfix [ "^" *"-" postfix ]`; no chains.
    fn power(&mut self) -> Result<Id, Error> {
        let base = self.postfix()?;
        if !self.peek().is("^") {
            return Ok(base);
        }
        self.bump();
        let mut n = 0;
        while self.peek().is("-") {
            self.bump();
            n += 1;
        }
        if self.peek().is("+") {
            return Err(err(Code::Syntax, "there is no unary +"));
        }
        let mut e = self.postfix()?;
        if self.peek().is("^") {
            return Err(err(Code::Ambiguous, "a^b^c: write (a^b)^c or a^(b^c)"));
        }
        for _ in 0..n {
            e = self.push(PNode::Neg(e));
        }
        Ok(self.push(PNode::Bin(BinOp::Pow, base, e)))
    }

    fn postfix(&mut self) -> Result<Id, Error> {
        let x = self.primary()?;
        let t = self.peek();
        if t.is(".") {
            return Err(err(Code::Reserved, "'.' after an expression is reserved for methods (1.3b)"));
        }
        if t.is("(") {
            return Err(err(Code::DynamicName, "only a name can be called"));
        }
        Ok(x)
    }

    fn primary(&mut self) -> Result<Id, Error> {
        let t = self.peek();
        match t.kind {
            Kind::Number => {
                self.bump();
                Ok(self.push(PNode::Num { text: t.text.clone(), unit: t.unit.clone() }))
            }
            Kind::Str => {
                self.bump();
                Ok(self.push(PNode::Str(t.text.clone())))
            }
            Kind::Date => {
                self.bump();
                Ok(self.push(PNode::Date(t.text.clone())))
            }
            Kind::Instant => {
                self.bump();
                Ok(self.push(PNode::Instant(t.text.clone())))
            }
            Kind::Qualified | Kind::Alias => {
                self.bump();
                let pin = if self.peek().kind == Kind::Pin {
                    let p = self.peek().text.clone();
                    self.bump();
                    Some(p)
                } else {
                    None
                };
                if !self.peek().is("(") {
                    return Err(err(
                        Code::Syntax,
                        format!("{} is not called: there are no function values", t.describe()),
                    ));
                }
                let args = self.args(true, "")?;
                Ok(self.push(PNode::Read { name: t.text.clone(), alias: t.kind == Kind::Alias, pin, args }))
            }
            Kind::Word => self.word(),
            Kind::Punct => match t.text.as_str() {
                "(" => {
                    self.bump();
                    self.enter()?;
                    let r = self.expr();
                    self.leave();
                    let x = r?;
                    self.expect(")")?;
                    Ok(x)
                }
                "*" => Err(err(Code::Placement, "* is an argument of a read only")),
                "[" => Err(err(Code::Placement, "a range is an argument of a read only")),
                "+" => Err(err(Code::Syntax, "there is no unary +")),
                "^" => Err(err(Code::Ambiguous, "write the parentheses: (-a)^b, -(a^b), (a^b)^c or a^(b^c)")),
                "." => Err(err(Code::Reserved, "'.' is reserved for methods (1.3b)")),
                "->" => Err(err(Code::Placement, "a lambda is only an argument of map or filter")),
                "=" | "<>" | "<" | "<=" | ">" | ">=" => Err(err(Code::Chain, "comparisons do not chain: write and")),
                _ => Err(self.unexpected("an expression")),
            },
            // A pin is lexed only after a name or alias, and consumed with it.
            Kind::End | Kind::Pin => Err(self.unexpected("an expression")),
        }
    }

    fn word(&mut self) -> Result<Id, Error> {
        let t = self.peek();
        let lower = t.text.to_ascii_lowercase();
        let called = self.peek_at(1).is("(");
        match lower.as_str() {
            "true" | "false" => {
                if called {
                    return Err(self.unexpected("an expression"));
                }
                self.bump();
                return Ok(self.push(PNode::Bool(lower == "true")));
            }
            "if" => {
                if self.if_is_call() {
                    return self.call("if");
                }
                return Err(err(Code::Placement, "if as an operand needs parentheses"));
            }
            "let" => return Err(err(Code::Placement, "let as an operand needs parentheses")),
            "not" => {
                if called {
                    return Err(err(Code::Placement, "not as an operand needs parentheses"));
                }
                return Err(self.unexpected("an expression"));
            }
            "and" | "or" if called => return self.call(&lower),
            "then" | "else" | "in" | "and" | "or" => return Err(self.unexpected("an expression")),
            "this" | "super" => return Err(err(Code::Reserved, format!("{} is reserved", t.text))),
            _ => {}
        }
        if called {
            if FORBIDDEN.contains(&lower.as_str()) {
                let mut e = err(Code::Forbidden, format!("{}() does not exist: formulas are pure", t.text));
                e.token = Some(t.text.clone());
                return Err(e);
            }
            if BUILTINS.contains(&lower.as_str()) {
                return self.call(&lower);
            }
            if lower == "ellipsoidal_distance" {
                return Err(err(Code::Unsupported, "ellipsoidal_distance is reserved for a later revision"));
            }
            return Err(err(
                Code::UnknownFunction,
                format!("{} is not a built-in; user functions are written qualified or as @alias", t.text),
            ));
        }
        self.bump();
        Ok(self.push(PNode::Var(t.text.clone())))
    }

    fn call(&mut self, name: &str) -> Result<Id, Error> {
        self.bump(); // the name
        let args = self.args(false, name)?;
        Ok(self.push(PNode::Call { name: name.to_string(), args }))
    }

    /// An argument list `( [ arg *( "," arg ) ] )` of a read (`read`) or of the built-in `callee`.
    fn args(&mut self, read: bool, _callee: &str) -> Result<Vec<PArg>, Error> {
        self.expect("(")?;
        let mut out: Vec<PArg> = Vec::new();
        if self.peek().is(")") {
            self.bump();
            return Ok(out);
        }
        loop {
            let mut label = None;
            if self.peek().kind == Kind::Word && self.peek_at(1).is(":") {
                label = Some(self.peek().text.clone());
                self.bump();
                self.bump();
            }
            let value = if read && self.peek().is("*") && (self.peek_at(1).is(",") || self.peek_at(1).is(")")) {
                self.bump();
                PVal::Open
            } else if self.peek().is("*") && (self.peek_at(1).is(",") || self.peek_at(1).is(")")) {
                return Err(err(Code::Placement, "* is an argument of a read only"));
            } else if self.peek().is("[") {
                if !read {
                    return Err(err(Code::Placement, "a range is an argument of a read only"));
                }
                self.bump();
                let lo = self.expr()?;
                self.expect("..")?;
                let hi = self.expr()?;
                self.expect("]")?;
                PVal::Range(lo, hi)
            } else if !read && self.lambda_head().is_some() {
                // Placement is checked at resolution (only the second argument of map and filter).
                PVal::Expr(self.expr_or_lambda()?)
            } else {
                PVal::Expr(self.expr()?)
            };
            if label.is_none() && out.iter().any(|a| a.label.is_some()) {
                return Err(err(Code::ArgOrder, "positional arguments come before labelled ones"));
            }
            if let Some(l) = &label {
                if out.iter().any(|a| a.label.as_deref() == Some(l.as_str())) {
                    return Err(err(Code::ArgDuplicate, format!("{l} is given twice")));
                }
            }
            out.push(PArg { label, value });
            if self.peek().is(",") {
                self.bump();
                continue;
            }
            self.expect(")")?;
            return Ok(out);
        }
    }
}
