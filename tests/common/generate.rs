//! Random formula trees and bodies for the round-trip and determinism tests: a small
//! xorshift generator (no dependency) and typed templates over the vectors' worlds.
#![allow(dead_code)]

use datumesh_formula::{Ast, Node, NodeId, Op, ReadArg, BUILTIN_NAMES};

/// xorshift64*.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    /// True with probability `permille` / 1000.
    pub fn chance(&mut self, permille: u64) -> bool {
        (self.next() % 1000) < permille
    }

    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

// ---------------------------------------------------------------------------------------------
// Random canonical ASTs (structure only: they need not type-check).

const NAMES: &[&str] = &["a", "b", "x1", "rate", "sum", "count", "v_2", "date", "zz", "station", "i"];
const FNS: &[&str] = &["a.org/f", "data.example.org/daily", "x-y.example.net/a/b_c", "rates.example.org/fx_rate"];
const LABELS: &[&str] = &["base", "date", "in", "if", "quote", "t", "x"];
const UNITS: &[&str] = &["EUR", "m", "m/s", "km/h", "min", "degC", "percent", "index:2015=100", "W/m2", "kWh", "d"];
const NUMS: &[&str] = &["0", "1", "42", "1.5", "0.001", "123456789012345678901234567890.5", "7"];
const STRS: &[&str] = &["", "abc", "say \"hi\"", "Zürich", "日本", "😀", "\u{1e9b}\u{323}", "\"\""];
const BINOPS: &[Op] = &[
    Op::Add,
    Op::Sub,
    Op::Mul,
    Op::Div,
    Op::Pow,
    Op::Cat,
    Op::Eq,
    Op::Ne,
    Op::Lt,
    Op::Le,
    Op::Gt,
    Op::Ge,
    Op::And,
    Op::Or,
];

struct TreeGen<'a> {
    rng: &'a mut Rng,
    nodes: Vec<Node>,
}

impl TreeGen<'_> {
    fn push(&mut self, n: Node) -> NodeId {
        self.nodes.push(n);
        (self.nodes.len() - 1) as NodeId
    }

    fn leaf(&mut self) -> NodeId {
        let n = match self.rng.below(7) {
            0 => {
                let unit = if self.rng.chance(300) { Some(self.rng.pick(UNITS).to_string()) } else { None };
                Node::Num { value: self.rng.pick(NUMS).to_string(), unit }
            }
            1 => Node::Str(self.rng.pick(STRS).to_string()),
            2 => Node::Bool(self.rng.chance(500)),
            3 => Node::Date(self.rng.pick(&["2024-02-29", "0001-01-01", "9999-12-31"]).to_string()),
            4 => Node::Instant("2024-03-15T16:00:00.250000Z".to_string()),
            _ => Node::Var(self.rng.pick(NAMES).to_string()),
        };
        self.push(n)
    }

    fn expr(&mut self, depth: u32) -> NodeId {
        if depth == 0 || self.rng.chance(250) {
            return self.leaf();
        }
        let d = depth - 1;
        match self.rng.below(12) {
            0..=3 => {
                let op = *self.rng.pick(BINOPS);
                let l = self.expr(d);
                let r = self.expr(d);
                self.push(Node::Op { op, args: vec![l, r] })
            }
            4 => {
                let x = self.expr(d);
                self.push(Node::Op { op: Op::Neg, args: vec![x] })
            }
            5 => {
                let x = self.expr(d);
                self.push(Node::Op { op: Op::Not, args: vec![x] })
            }
            6 => {
                let c = self.expr(d);
                let t = self.expr(d);
                let e = self.expr(d);
                self.push(Node::If { cond: c, then: t, els: e })
            }
            7 => {
                let name = self.rng.pick(NAMES).to_string();
                let v = self.expr(d);
                let b = self.expr(d);
                self.push(Node::Let { name, value: v, body: b })
            }
            8 => {
                let mut labels: Vec<&str> = LABELS.to_vec();
                let n = 1 + self.rng.below(3);
                let mut args = Vec::new();
                for _ in 0..n {
                    let i = self.rng.below(labels.len());
                    let l = labels.remove(i);
                    let a = match self.rng.below(4) {
                        0 => ReadArg::Open,
                        1 => {
                            let lo = self.expr(d);
                            let hi = self.expr(d);
                            ReadArg::Range(lo, hi)
                        }
                        _ => ReadArg::Expr(self.expr(d)),
                    };
                    args.push((l.to_string(), a));
                }
                args.sort_by(|a, b| a.0.cmp(&b.0));
                let fn_name = self.rng.pick(FNS).to_string();
                self.push(Node::Read { fn_name, fn_hash: None, args })
            }
            _ => self.call(d),
        }
    }

    fn call(&mut self, d: u32) -> NodeId {
        // Required parameters positional, optional ones (a random subset) named, in order.
        let specs: &[(&str, usize, &[&str])] = &[
            ("abs", 1, &[]),
            ("round", 1, &["digits"]),
            ("at", 2, &["along"]),
            ("latest", 1, &["bound", "along"]),
            ("interpolate", 2, &["method", "bound", "along"]),
            ("add_months", 2, &["calendar", "roll", "weekend"]),
            ("find", 2, &["start"]),
            ("haversine", 4, &[]),
            ("pi", 0, &[]),
            ("asof", 0, &[]),
            ("asof", 2, &[]),
            ("sum", 1, &[]),
            ("concat", 3, &[]),
            ("map", 2, &[]),
            ("filter", 2, &[]),
            ("scan", 2, &["along"]),
            ("window", 3, &["along"]),
        ];
        let (name, required, optional) = *self.rng.pick(specs);
        assert!(BUILTIN_NAMES.contains(&name));
        let mut args = Vec::new();
        for i in 0..required {
            let a = if (name == "map" || name == "filter") && i == 1 {
                let params: Vec<String> = match self.rng.below(2) {
                    0 => vec!["v".to_string()],
                    _ => vec!["v".to_string(), "date".to_string()],
                };
                let body = self.expr(d);
                self.push(Node::Lambda { params, body })
            } else if (name == "scan" && i == 1) || (name == "window" && i == 2) {
                let a = self.rng.pick(&["sum", "mean", "max"]).to_string();
                self.push(Node::Agg(a))
            } else {
                self.expr(d)
            };
            args.push(a);
        }
        let mut named = Vec::new();
        for p in optional {
            if self.rng.chance(500) {
                let v = if *p == "along" {
                    let k = self.rng.pick(NAMES).to_string();
                    self.push(Node::Key(k))
                } else {
                    self.expr(d)
                };
                named.push((p.to_string(), v));
            }
        }
        self.push(Node::Call { fn_name: name.to_string(), args, named })
    }
}

/// A random canonical AST of at most `depth` levels.
pub fn random_ast(rng: &mut Rng, depth: u32) -> Ast {
    let mut g = TreeGen { rng, nodes: Vec::new() };
    let root = g.expr(depth);
    Ast::from_parts(g.nodes, root).expect("a well-formed arena")
}

// ---------------------------------------------------------------------------------------------
// Typed bodies over the world of the formula-eval vectors.

pub const KINDS: &[&str] = &["EUR", "N1", "DEGC", "B", "S", "D", "T"];

fn templates(kind: &str) -> &'static [&'static str] {
    match kind {
        "EUR" => &[
            "a",
            "b",
            "{EUR} + {EUR}",
            "{EUR} - {EUR}",
            "{EUR} * {N1}",
            "{EUR} / {N1}",
            "-{EUR}",
            "abs({EUR})",
            "round({EUR}, {DIG})",
            "round_up({EUR}, {DIG})",
            "floor({EUR})",
            "mod({EUR}, {EUR})",
            "if {B} then {EUR} else {EUR}",
            "sum(data.example.org/daily(*))",
            "at(data.example.org/daily(*), {D})",
            "otherwise(at(data.example.org/daily(*), {D}), 0)",
            "last_before(data.example.org/daily(*), {D}, {DAYS})",
            "first_after(data.example.org/daily(*), {D})",
            "nearest(data.example.org/daily(*), {D})",
            "interpolate(data.example.org/daily(*), {D})",
            "interpolate(data.example.org/daily(*), {D}, \"step\")",
            "mean(data.example.org/daily([{D}..{D}]))",
            "data.example.org/daily(date: {D})",
            "data.example.org/price(item: {S})",
            "otherwise(sum(data.example.org/price(*) * data.example.org/qty(*)), 0)",
            "max(window(data.example.org/daily(*), {SMALLN}, mean))",
            "last(scan(data.example.org/daily(*), sum))",
            "sum(map(data.example.org/daily(*), v -> v * {N1}))",
            "sum(map(data.example.org/daily(*), (v, date) -> if weekday(date) > 5 then 0 EUR else v))",
            "sum(filter(data.example.org/daily(*), v -> v > {EUR}))",
            "median(data.example.org/daily(*))",
            "stdev(data.example.org/daily(*))",
            "asof(data.example.org/price(item: {S}), {T})",
            "first(versions(data.example.org/price(item: {S}), {T}, asof()))",
            "let w = {EUR} in w * 2",
            "mean({EUR}, {EUR}, 1)",
            "median({EUR}, {EUR}, {EUR})",
        ],
        "N1" => &[
            "x",
            "y",
            "n",
            "0",
            "1",
            "2",
            "0.5",
            "3",
            "1000",
            "{N1} + {N1}",
            "{N1} - {N1}",
            "{N1} * {N1}",
            "{N1} / {N1}",
            "({N1}) ^ {INT}",
            "({N1}) ^ 0.5",
            "sqrt({N1})",
            "exp({N1})",
            "ln({N1})",
            "sin({N1})",
            "cos({N1})",
            "atan2({N1}, {N1})",
            "pi()",
            "math.example.net/fib(n: {SMALLN})",
            "curve.example.net/df(tenor: {TENOR})",
            "sum(curve.example.net/df([1..{TENOR}]))",
            "otherwise(sum(math.example.net/recip(*)), {N1})",
            "math.example.net/recip(n: {N1})",
            "rates.example.org/rate(\"USD\", \"1M\")",
            "len({S})",
            "find(\"m\", {S})",
            "year({D})",
            "weekday({D})",
            "year_fraction({D}, {D}, \"{CONV}\")",
            "{EUR} / {EUR}",
            "if {B} then {N1} else {N1}",
            "count(data.example.org/daily(*)) / 1 count",
            "sin({N1} deg)",
            "haversine(plat, 0, 48.85, 2.35) / 1 m",
            "business_days({D}, {D}, calendar: calendars.example.org/holiday(\"gb-eaw\", *)) / 1 d",
            "({D} - {D}) / 1 d",
            "(t - instant({D})) / 1 s",
            "stdev({N1}, {N1})",
        ],
        "DEGC" => &[
            "t1",
            "latest(weather.example.org/temp({S}, *))",
            "last_before(weather.example.org/temp({S}, *), {T}, {MINS})",
            "interpolate(weather.example.org/temp({S}, *), {T})",
            "nearest(weather.example.org/temp({S}, *), {T})",
            "mean(last_before(weather.example.org/temp(*, *), {T}, {MINS}))",
            "weather.example.org/temp(station: {S}, t: {T})",
            "mean(within(weather.example.org/temp({S}, *), {T}, {MINS}))",
            "latest(weather.example.org/temp({S}, *), {MINS})",
            "{DEGC} + 1 K",
            "convert(convert({DEGC}, \"K\"), \"degC\")",
            "latest(at(weather.example.org/temp(*, *), {S}, along: station))",
        ],
        "B" => &[
            "flag",
            "{N1} > {N1}",
            "{EUR} = {EUR}",
            "is_holiday({D}, calendars.example.org/holiday(\"gb-eaw\", *))",
            "data.example.org/active(company: {S}) and flag",
            "{B} or {B}",
            "not {B}",
            "{S} = {S}",
            "{D} < {D}",
        ],
        "S" => &[
            "s",
            "\"abc\"",
            "upper({S})",
            "lower({S})",
            "trim({S})",
            "{S} & {S}",
            "concat({S}, \" \", {S})",
            "substr({S}, {SMALLN}, {SMALLN})",
            "text({N1})",
            "fixed({N1}, {DIG})",
            "data.example.org/name(company: {S})",
        ],
        "D" => &[
            "d",
            "d2",
            "2026-03-03",
            "add_days({D}, {INT})",
            "add_days({D}, {INT}, calendar: calendars.example.org/holiday(\"gb-eaw\", *))",
            "add_months({D}, {INT})",
            "add_months({D}, {INT}, calendar: calendars.example.org/holiday(\"gb-eaw\", *), roll: \"{ROLL}\")",
            "end_of_month({D})",
            "date_of({T})",
            "date(2026, {SMALLN}, {SMALLN})",
            "{D} + {INT}",
        ],
        _ => &["t", "asof()", "{T} + {MINS}", "instant({D})", "t + 0.5 s"],
    }
}

fn fill(hole: &str) -> &'static [&'static str] {
    match hole {
        "DIG" => &["0", "1", "2", "-1"],
        "INT" => &["0", "1", "2", "-1", "-3", "12"],
        "SMALLN" => &["0", "1", "2", "3", "5", "7"],
        "TENOR" => &["1", "2", "3", "10", "11"],
        "DAYS" => &["1 d", "7 d", "30 d"],
        "MINS" => &["1 min", "10 min", "15 min", "1 h", "30 s"],
        "CONV" => &["act/360", "act/365f", "act/act_isda", "30/360", "30e/360", "30/360_us"],
        _ => &["following", "modified_following", "preceding"],
    }
}

/// A random body of type `kind` (one of [`KINDS`]).
pub fn body(rng: &mut Rng, kind: &str, depth: u32) -> String {
    let all = templates(kind);
    let choices: Vec<&str> = if depth == 0 {
        let simple: Vec<&str> = all.iter().copied().filter(|t| !t.contains('{')).collect();
        if simple.is_empty() {
            all.to_vec()
        } else {
            simple
        }
    } else {
        all.to_vec()
    };
    let t = *rng.pick(&choices);
    let mut out = String::new();
    let mut rest = t;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let j = rest[i..].find('}').map(|j| i + j).unwrap_or(rest.len() - 1);
        let hole = &rest[i + 1..j];
        if KINDS.contains(&hole) {
            let sub = body(rng, hole, depth.saturating_sub(1));
            if sub.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '"') {
                out.push_str(&sub);
            } else {
                out.push('(');
                out.push_str(&sub);
                out.push(')');
            }
        } else {
            out.push_str(rng.pick(fill(hole)));
        }
        rest = &rest[j + 1..];
    }
    out.push_str(rest);
    out
}
