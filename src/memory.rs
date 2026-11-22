//! An in-memory [`World`]: functions with binding starts, stored receipts, and computed cells
//! evaluated on demand (memoised, with cycle and depth limits) and given receipts. It is the
//! world of the `formula-eval` and `excel` vectors (SPEC 11), and a model of what the engine
//! does in phase B.
//!
//! Computed receipts follow the vectors' conventions: signed by `mesh.example.net` with key
//! `cd42547c321bfc08`, licence `CC0-1.0`, the reference evaluator's summary, `issued` the
//! later of the receipt's `asof` and 2020-01-01 (the vectors' key window). Only the receipt
//! hash is computed; nothing is signed.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use datumesh_core::json::Value;
use datumesh_core::{Arg, ClassDescriptor, Scalar, Type, Withdrawal};

use crate::coverage::{EvalObserver, NoObserver};
use crate::env::{Env, EnvError, Function};
use crate::eval::{
    self, Args, Binding, CellKey, ClassBinding, Fuel, Outcome, SliceKey, Stop, Version, World, ZERO_INSTANT,
};
use crate::{define, Code, Compiled};

/// The vectors' default fuel (15.12): 10,000,000.
pub const DEFAULT_FUEL: u64 = 10_000_000;
/// The vectors' default evaluation depth limit: 2,000 cells.
pub const DEFAULT_MAX_DEPTH: usize = 2000;

/// The receipt conventions of the vectors' computed receipts.
#[derive(Clone, Debug)]
pub struct Conventions {
    /// `computed_by`.
    pub computed_by: String,
    /// `licence`.
    pub licence: String,
    /// `summary`.
    pub summary: String,
    /// The earliest `issued` (the signing key's window start).
    pub issued_from: String,
}

impl Default for Conventions {
    fn default() -> Conventions {
        Conventions {
            computed_by: "mesh.example.net#cd42547c321bfc08".to_string(),
            licence: "CC0-1.0".to_string(),
            summary: "Computed by the Datumesh formula reference evaluator.".to_string(),
            issued_from: "2020-01-01T00:00:00.000000Z".to_string(),
        }
    }
}

/// A stored receipt, or a withdrawal (SPEC 8.1, R1): the null version of its cell, with no value.
#[derive(Clone, Debug)]
struct Stored {
    cell: CellKey,
    fn_hash: String,
    tx: String,
    asof: String,
    /// `None` for a withdrawal.
    version: Option<Version>,
}

/// A computed cell's evaluation: the outcome and its receipt.
#[derive(Clone, Debug)]
pub struct Computed {
    /// The evaluation's outcome.
    pub outcome: Outcome,
    /// The receipt without `sig`.
    pub receipt: Value,
    /// Its receipt hash.
    pub receipt_hash: String,
}

/// An in-memory world.
#[derive(Clone)]
pub struct MemoryWorld {
    functions: BTreeMap<String, Binding>,
    classes: BTreeMap<String, ClassBinding>,
    stored: Vec<Stored>,
    /// Compiled bodies by function hash; cleared when a name is bound.
    compiled: HashMap<String, Arc<Compiled>>,
    /// Computed versions by cell, evaluation asof and function hash.
    memo: HashMap<(CellKey, String, String), Result<Option<Version>, Code>>,
    /// Blob content by SHA-256 (a vector world's `blobs`, spec-0.4.2).
    blobs: HashMap<String, Vec<u8>>,
    computed: HashMap<String, Computed>,
    stack: Vec<CellKey>,
    /// The evaluation depth limit, in cells.
    pub max_depth: usize,
    /// The request's fuel (15.12), shared by every evaluation the world makes.
    pub fuel: Fuel,
    asof: String,
    /// The receipt conventions.
    pub conventions: Conventions,
}

fn bad(s: impl Into<String>) -> EnvError {
    EnvError(s.into())
}

/// The receipt hash: SHA-256 of the receipt's JCS bytes without `sig` (8.3).
pub fn receipt_hash(receipt: &Value) -> String {
    let stripped = match receipt {
        Value::Object(m) => Value::Object(m.iter().filter(|(k, _)| k != "sig").cloned().collect()),
        v => v.clone(),
    };
    datumesh_core::sha256_hex(&stripped.to_jcs())
}

fn scalar_of(type_v: &Value, value: &Value) -> Result<Scalar, EnvError> {
    let t = type_v.as_str().and_then(Type::from_name).ok_or_else(|| bad("receipt type"))?;
    match (t, value) {
        (Type::Boolean, Value::Bool(b)) => Ok(Scalar::Boolean(*b)),
        (t, Value::String(s)) => Scalar::from_lexical(t, s).map_err(|e| bad(format!("receipt value: {e:?}"))),
        _ => Err(bad("receipt value shape")),
    }
}

fn args_of(v: &Value) -> Result<Args, EnvError> {
    let m = v.as_object().ok_or_else(|| bad("args"))?;
    let mut out = Args::new();
    for (k, a) in m {
        let s =
            scalar_of(a.get("type").ok_or_else(|| bad("arg type"))?, a.get("value").ok_or_else(|| bad("arg value"))?)?;
        out.insert(k.clone(), s);
    }
    Ok(out)
}

impl MemoryWorld {
    /// An empty world evaluating as of `asof`.
    pub fn new(asof: &str) -> MemoryWorld {
        MemoryWorld {
            functions: BTreeMap::new(),
            classes: BTreeMap::new(),
            stored: Vec::new(),
            compiled: HashMap::new(),
            memo: HashMap::new(),
            blobs: HashMap::new(),
            computed: HashMap::new(),
            stack: Vec::new(),
            max_depth: DEFAULT_MAX_DEPTH,
            fuel: Fuel::new(DEFAULT_FUEL),
            asof: asof.to_string(),
            conventions: Conventions::default(),
        }
    }

    /// The world of a vector: `{"functions": {name: {"descriptor", "bound"}}, "receipts": [...],
    /// "max_depth"?}` and, since spec-0.5.0, `"classes": {name: {"descriptor", "bound"}}`.
    pub fn from_value(world: &Value, asof: &str) -> Result<MemoryWorld, EnvError> {
        let mut w = MemoryWorld::new(asof);
        let fs = world.get("functions").and_then(Value::as_object).ok_or_else(|| bad("world without functions"))?;
        for (name, f) in fs {
            let d = f.get("descriptor").ok_or_else(|| bad("function without descriptor"))?;
            let bound = f.get("bound").and_then(Value::as_str).ok_or_else(|| bad("function without bound"))?;
            w.bind(name, d, bound)?;
        }
        if let Some(cs) = world.get("classes").and_then(Value::as_object) {
            for (name, c) in cs {
                let d = c.get("descriptor").ok_or_else(|| bad("class without descriptor"))?;
                let bound = c.get("bound").and_then(Value::as_str).ok_or_else(|| bad("class without bound"))?;
                w.bind_class(name, d, bound)?;
            }
        }
        if let Some(Value::Array(rs)) = world.get("receipts") {
            for r in rs {
                w.add_receipt(r)?;
            }
        }
        if let Some(bs) = world.get("blobs").and_then(Value::as_object) {
            for (sha, hex) in bs {
                let hex = hex.as_str().ok_or_else(|| bad("blob content"))?;
                w.add_blob(sha, hex_bytes(hex).ok_or_else(|| bad("blob content hex"))?);
            }
        }
        if let Some(d) = world.get("max_depth").and_then(Value::as_number_text) {
            w.max_depth = d.parse().map_err(|_| bad("max_depth"))?;
        }
        Ok(w)
    }

    /// Bind `name` to a descriptor from `start`.
    pub fn bind(&mut self, name: &str, descriptor: &Value, start: &str) -> Result<(), EnvError> {
        let f = Function::from_value(descriptor)?;
        self.functions.insert(name.to_string(), Binding { function: Arc::new(f), start: start.to_string() });
        self.compiled.clear();
        Ok(())
    }

    /// Bind the class `name` to a flattened class descriptor from `start` (SPEC 16.1, 9.7). The
    /// methods' functions are bound as functions (`<class>/<method>`, [`MemoryWorld::bind`]),
    /// as a vector's `world.functions` has them.
    pub fn bind_class(&mut self, name: &str, descriptor: &Value, start: &str) -> Result<(), EnvError> {
        let d =
            datumesh_core::ClassDescriptor::from_value(descriptor).map_err(|e| bad(format!("class {name}: {e}")))?;
        let class_hash = d.hash();
        self.compiled.clear();
        self.classes
            .insert(name.to_string(), ClassBinding { class: Arc::new(d), class_hash, start: start.to_string() });
        Ok(())
    }

    /// Give the world a blob's content under `sha256` (as a resolver would; not checked here).
    pub fn add_blob(&mut self, sha256: &str, content: Vec<u8>) {
        self.blobs.insert(sha256.to_string(), content);
    }

    /// Add a stored receipt, or a withdrawal (a receipt with `withdrawn` in place of `type` and
    /// `value`, parsed by [`Withdrawal`]). A withdrawal is a version of its cell with no value.
    pub fn add_receipt(&mut self, r: &Value) -> Result<(), EnvError> {
        if r.get("withdrawn").is_some() {
            let w = Withdrawal::from_value(r).map_err(|e| bad(format!("withdrawal: {e}")))?;
            let cell = w.cell();
            let mut args = Args::new();
            for (k, a) in &cell.args {
                let Arg::Value(s) = a else {
                    return Err(bad("withdrawal with an open argument"));
                };
                args.insert(k.clone(), s.clone());
            }
            self.stored.push(Stored {
                cell: CellKey { name: format!("{}/{}", cell.ns, cell.fn_name), args },
                fn_hash: cell.fn_hash.clone(),
                tx: w.tx().to_string(),
                asof: w.asof().to_string(),
                version: None,
            });
            return Ok(());
        }
        let cell = r.get("cell").ok_or_else(|| bad("receipt without cell"))?;
        let ns = cell.get("ns").and_then(Value::as_str).ok_or_else(|| bad("cell ns"))?;
        let f = cell.get("fn").and_then(Value::as_str).ok_or_else(|| bad("cell fn"))?;
        let fn_hash = cell.get("fn_hash").and_then(Value::as_str).ok_or_else(|| bad("cell fn_hash"))?;
        let args = args_of(cell.get("args").ok_or_else(|| bad("cell args"))?)?;
        let value = scalar_of(r.get("type").ok_or_else(|| bad("type"))?, r.get("value").ok_or_else(|| bad("value"))?)?;
        let tx = r.get("tx").and_then(Value::as_str).ok_or_else(|| bad("tx"))?;
        let asof = r.get("asof").and_then(Value::as_str).ok_or_else(|| bad("asof"))?;
        self.stored.push(Stored {
            cell: CellKey { name: format!("{ns}/{f}"), args },
            fn_hash: fn_hash.to_string(),
            tx: tx.to_string(),
            asof: asof.to_string(),
            version: Some(Version { value, receipt_hash: receipt_hash(r), tx: tx.to_string(), asof: asof.to_string() }),
        });
        Ok(())
    }

    /// The computed cells read as inputs so far and evaluated with a value: each cell, the asof
    /// it was evaluated as of, and its version (the computed receipt's hash, `tx` and `asof`), by cell and asof.
    pub fn evaluated(&self) -> Vec<(CellKey, String, Version)> {
        let mut out: Vec<(CellKey, String, Version)> = self
            .memo
            .iter()
            .filter_map(|((cell, at, _), r)| match r {
                Ok(Some(v)) => Some((cell.clone(), at.clone(), v.clone())),
                _ => None,
            })
            .collect();
        out.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        out
    }

    /// A computed receipt produced during evaluation, by hash.
    pub fn computed(&self, hash: &str) -> Option<&Computed> {
        self.computed.get(hash)
    }

    /// The class whose method `name` is (16.3): the prefix class bound to `name`'s method hash.
    fn method_class(&self, name: &str, fn_hash: &str) -> Option<String> {
        let (class, method) = name.rsplit_once('/')?;
        let c = self.classes.get(class)?;
        (c.class.methods.get(method).map(String::as_str) == Some(fn_hash)).then(|| class.to_string())
    }

    /// The compiled body of the formula function `f` named `name` (by its hash, so that a pinned
    /// read compiles the function it is pinned to), checked against the bindings and classes as they
    /// are now: a callee rebound since define time is checked again, with the same codes (15.8).
    fn compiled_of(&mut self, name: &str, f: &Arc<Function>) -> Result<Arc<Compiled>, Code> {
        if let Some(c) = self.compiled.get(&f.hash) {
            return Ok(c.clone());
        }
        if f.lang != "formula" {
            return Err(Code::Unresolved);
        }
        let mut env = Env::new(name, f.signature.clone());
        for (n, other) in &self.functions {
            env.functions.insert(n.clone(), (*other.function).clone());
        }
        for (n, c) in &self.classes {
            env.classes.insert(n, (*c.class).clone());
        }
        env.method_of = self.method_class(name, &f.hash);
        env.evaluating = true;
        let c = Arc::new(
            crate::check_descriptor(name, &f.value, &env).map_err(|e| e.formula_code().unwrap_or(Code::Syntax))?,
        );
        self.compiled.insert(f.hash.clone(), c.clone());
        Ok(c)
    }

    /// The version of a stored cell answering as of `at`: the latest `tx` among its receipts and
    /// withdrawals with an `asof` at or before `at`. A withdrawal as that latest version means the
    /// cell has no value (`None`).
    fn stored_version(&self, cell: &CellKey, fn_hash: &str, at: &str) -> Option<Version> {
        let mut best: Option<&Stored> = None;
        for s in &self.stored {
            if s.cell == *cell && s.fn_hash == fn_hash && s.asof.as_str() <= at {
                let newer = match best {
                    None => true,
                    Some(b) => (s.tx.as_str(), s.asof.as_str()) > (b.tx.as_str(), b.asof.as_str()),
                };
                if newer {
                    best = Some(s);
                }
            }
        }
        best.and_then(|s| s.version.clone())
    }

    /// Evaluate a computed cell as of `asof`, building its receipt; fuel is charged to
    /// [`MemoryWorld::fuel`].
    pub fn compute(&mut self, cell: &CellKey, asof: &str) -> Result<Computed, Code> {
        let f = self.functions.get(&cell.name).ok_or(Code::Unresolved)?.function.clone();
        self.compute_with(&cell.name, f, cell, asof, &mut NoObserver)
    }

    /// [`MemoryWorld::compute`] with an observer, told each branch arm of the cell's own body as
    /// it is taken (the callees' arms are not reported). Always evaluates the cell (no memo), so
    /// the arms are those of this evaluation; the result is the same as `compute`'s.
    pub fn compute_observed(
        &mut self,
        cell: &CellKey,
        asof: &str,
        observer: &mut dyn EvalObserver,
    ) -> Result<Computed, Code> {
        let f = self.functions.get(&cell.name).ok_or(Code::Unresolved)?.function.clone();
        self.compute_with(&cell.name, f, cell, asof, observer)
    }

    /// [`MemoryWorld::compute`] for the function `f` (the one named `name`, or one pinned to by its
    /// hash), told of the arms of its own body by `observer`.
    fn compute_with(
        &mut self,
        name: &str,
        f: Arc<Function>,
        cell: &CellKey,
        asof: &str,
        observer: &mut dyn EvalObserver,
    ) -> Result<Computed, Code> {
        if self.stack.contains(cell) {
            return Err(Code::Cycle);
        }
        if self.stack.len() >= self.max_depth {
            return Err(Code::Depth);
        }
        let compiled = self.compiled_of(name, &f)?;
        let saved = std::mem::replace(&mut self.asof, asof.to_string());
        self.stack.push(cell.clone());
        let r = eval::evaluate_observed(&compiled, &f, &cell.args, asof, self, observer);
        self.stack.pop();
        self.asof = saved;
        let outcome = r.map_err(|s| match s {
            Stop::Code(c) => c,
            Stop::World(never) => match never {},
        })?;
        let receipt = self.receipt(&f, cell, &outcome);
        let hash = receipt_hash(&receipt);
        let c = Computed { outcome, receipt, receipt_hash: hash.clone() };
        self.computed.insert(hash, c.clone());
        Ok(c)
    }

    /// The version of a cell as of `at`, without charging the read: a stored version, or the
    /// computed cell evaluated (memoised).
    fn version(&mut self, cell: &CellKey, at: &str) -> Result<Option<Version>, Code> {
        let f = self.functions.get(&cell.name).ok_or(Code::Unresolved)?.function.clone();
        self.version_with(&cell.name, f, cell, at)
    }

    /// [`MemoryWorld::version`] of the function `f`.
    fn version_with(
        &mut self,
        name: &str,
        f: Arc<Function>,
        cell: &CellKey,
        at: &str,
    ) -> Result<Option<Version>, Code> {
        if f.lang == "stored" {
            return Ok(self.stored_version(cell, &f.hash, at));
        }
        let key = (cell.clone(), at.to_string(), f.hash.clone());
        if let Some(r) = self.memo.get(&key) {
            return r.clone();
        }
        let r = self.compute_with(name, f, cell, at, &mut NoObserver).map(|c| {
            Some(Version {
                value: c.outcome.value.clone(),
                receipt_hash: c.receipt_hash.clone(),
                tx: c.outcome.tx.clone().unwrap_or_else(|| ZERO_INSTANT.to_string()),
                asof: c.outcome.receipt_asof.clone(),
            })
        });
        if !matches!(r, Err(c) if c.is_mesh_failure()) {
            self.memo.insert(key, r.clone());
        }
        r
    }

    fn receipt(&self, f: &Function, cell: &CellKey, o: &Outcome) -> Value {
        let s = |x: &str| Value::String(x.to_string());
        let (ns, _) = cell.name.rsplit_once('/').unwrap_or(("", &cell.name));
        let valid: Vec<(String, Value)> = cell
            .args
            .iter()
            .filter(|(k, _)| f.signature.args.get(*k).is_some_and(|d| d.valid))
            .map(|(k, v)| (k.clone(), v.to_value()))
            .collect();
        let tx = o.tx.clone().unwrap_or_else(|| ZERO_INSTANT.to_string());
        let issued = if o.receipt_asof > self.conventions.issued_from {
            o.receipt_asof.clone()
        } else {
            self.conventions.issued_from.clone()
        };
        let (t, v) = match &o.value {
            Scalar::Boolean(b) => ("boolean", Value::Bool(*b)),
            other => (other.ty().as_str(), s(other.lexical())),
        };
        Value::Object(vec![
            ("asof".to_string(), s(&o.receipt_asof)),
            ("cell".to_string(), eval::cell_object(f, cell)),
            ("computed_by".to_string(), s(&self.conventions.computed_by)),
            ("inputs".to_string(), Value::Array(o.lineage.iter().map(|x| s(x)).collect())),
            ("issued".to_string(), s(&issued)),
            ("licence".to_string(), s(&self.conventions.licence)),
            ("proto".to_string(), s("datumesh/1")),
            ("publisher".to_string(), s(ns)),
            ("summary".to_string(), s(&self.conventions.summary)),
            ("tx".to_string(), s(&tx)),
            ("type".to_string(), s(t)),
            ("valid".to_string(), Value::Object(valid)),
            ("value".to_string(), v),
        ])
    }
}

impl World for MemoryWorld {
    type Error = std::convert::Infallible;

    fn function(&mut self, name: &str, at: Option<&str>) -> Result<Option<Binding>, Self::Error> {
        let at = at.unwrap_or(&self.asof);
        Ok(self.functions.get(name).filter(|b| b.start.as_str() <= at).cloned())
    }

    fn read(&mut self, cell: &CellKey, at: Option<&str>) -> Result<Option<Version>, Stop<Self::Error>> {
        self.charge(1)?;
        let at = at.map(str::to_string).unwrap_or_else(|| self.asof.clone());
        Ok(self.version(cell, &at)?)
    }

    fn members(&mut self, slice: &SliceKey) -> Result<Vec<(Args, Version)>, Stop<Self::Error>> {
        let b = self.functions.get(&slice.name).ok_or(Code::Unresolved)?.clone();
        let asof = self.asof.clone();
        let mut out = Vec::new();
        if b.function.lang == "stored" {
            let mut cells: Vec<CellKey> = Vec::new();
            for s in &self.stored {
                if s.cell.name == slice.name
                    && s.fn_hash == b.function.hash
                    && slice.matches(&s.cell.args)
                    && !cells.contains(&s.cell)
                {
                    cells.push(s.cell.clone());
                }
            }
            for c in cells {
                if let Some(v) = self.stored_version(&c, &b.function.hash, &asof) {
                    out.push((c.args, v));
                }
            }
        } else {
            // A computed function's slice: every cell of the extent must have a value.
            for args in eval::extent_cells(&b.function, slice)? {
                let cell = CellKey { name: slice.name.clone(), args };
                match self.version(&cell, &asof) {
                    Ok(Some(v)) => out.push((cell.args, v)),
                    Err(c) if c.is_mesh_failure() => return Err(c.into()),
                    Ok(None) | Err(_) => return Err(Code::Absent.into()),
                }
            }
        }
        out.sort_by(|a, b| {
            for (x, y) in a.0.values().zip(b.0.values()) {
                match eval::cmp_scalar(x, y) {
                    std::cmp::Ordering::Equal => {}
                    o => return o,
                }
            }
            std::cmp::Ordering::Equal
        });
        self.charge(1 + out.len() as u64)?;
        Ok(out)
    }

    fn versions(&mut self, cell: &CellKey, from: &str, to: &str) -> Result<Vec<Version>, Stop<Self::Error>> {
        let b = self.functions.get(&cell.name).ok_or(Code::Unresolved)?.clone();
        // Withdrawals are not listed: a version listing gives the values a cell has had (a
        // withdrawal is the null version, and the cell's value before it is listed as its own).
        let mut out: Vec<Version> = self
            .stored
            .iter()
            .filter(|s| {
                s.cell == *cell
                    && s.fn_hash == b.function.hash
                    && s.tx.as_str() >= from
                    && s.tx.as_str() <= to
                    && s.asof.as_str() <= self.asof.as_str()
            })
            .filter_map(|s| s.version.clone())
            .collect();
        out.sort_by(|a, b| a.tx.cmp(&b.tx));
        self.charge(1 + out.len() as u64)?;
        Ok(out)
    }

    fn charge(&mut self, n: u64) -> Result<(), Code> {
        self.fuel.charge(n)
    }

    fn asof(&mut self) -> Result<String, Stop<Self::Error>> {
        Ok(self.asof.clone())
    }

    fn blob(&mut self, sha256: &str, _length: u64) -> Result<Option<Vec<u8>>, Stop<Self::Error>> {
        Ok(self.blobs.get(sha256).cloned())
    }

    fn class(&mut self, name: &str, at: Option<&str>) -> Result<Option<ClassBinding>, Self::Error> {
        let at = at.unwrap_or(&self.asof);
        Ok(self.classes.get(name).filter(|b| b.start.as_str() <= at).cloned())
    }

    fn class_descriptor(
        &mut self,
        class_hash: &str,
        _at: Option<&str>,
    ) -> Result<Option<ClassDescriptor>, Self::Error> {
        Ok(self.classes.values().find(|b| b.class_hash == class_hash).map(|b| (*b.class).clone()))
    }

    fn class_name(&mut self, class_hash: &str, _at: Option<&str>) -> Result<Option<String>, Self::Error> {
        Ok(self.classes.iter().find(|(_, b)| b.class_hash == class_hash).map(|(n, _)| n.clone()))
    }

    fn function_by_hash(&mut self, fn_hash: &str) -> Result<Option<Arc<Function>>, Self::Error> {
        Ok(self.functions.values().find(|b| b.function.hash == fn_hash).map(|b| b.function.clone()))
    }

    fn read_pinned(&mut self, fn_hash: &str, cell: &CellKey) -> Result<Option<Version>, Stop<Self::Error>> {
        self.charge(1)?;
        let f = self.functions.values().find(|b| b.function.hash == fn_hash).map(|b| b.function.clone());
        let f = f.ok_or(Stop::Code(Code::Unresolved))?;
        let at = self.asof.clone();
        Ok(self.version_with(&cell.name, f, cell, &at)?)
    }
}

/// The result of a `formula-eval` or `excel` vector's evaluation.
#[derive(Clone, Debug)]
pub struct VectorResult {
    /// The evaluated cell's computed receipt and outcome.
    pub computed: Computed,
    /// Fuel used.
    pub fuel_used: u64,
    /// Every computed cell read as an input and evaluated with a value
    /// ([`MemoryWorld::evaluated`]).
    pub evaluated: Vec<(CellKey, String, Version)>,
}

/// Evaluate the cell of a `formula-eval` or `excel` vector (`world`, `cell`, `asof`, `fuel`?).
pub fn run_vector(v: &Value) -> Result<Result<VectorResult, Code>, EnvError> {
    let asof = v.get("asof").and_then(Value::as_str).ok_or_else(|| bad("vector without asof"))?;
    let mut world = MemoryWorld::from_value(v.get("world").ok_or_else(|| bad("vector without world"))?, asof)?;
    let cell = v.get("cell").ok_or_else(|| bad("vector without cell"))?;
    let name = cell.get("fn").and_then(Value::as_str).ok_or_else(|| bad("cell fn"))?;
    let args = args_of(cell.get("args").ok_or_else(|| bad("cell args"))?)?;
    let limit = match v.get("fuel").and_then(Value::as_number_text) {
        Some(t) => t.parse().map_err(|_| bad("fuel"))?,
        None => DEFAULT_FUEL,
    };
    // An excel vector's function is its formula, defined over the world's functions.
    if v.get("kind").and_then(Value::as_str) == Some("excel") {
        let sig = crate::env::Signature::from_value(v.get("signature").ok_or_else(|| bad("excel without signature"))?)?;
        let mut env = Env::new(name, sig);
        for (n, b) in &world.functions {
            env.functions.insert(n.clone(), (*b.function).clone());
        }
        let source = v.get("formula").and_then(Value::as_str).ok_or_else(|| bad("excel without formula"))?;
        let c = define(source, &env).map_err(|e| bad(format!("excel formula: {e}")))?;
        let descriptor = Value::Object(vec![
            (
                "code".to_string(),
                Value::Object(vec![
                    ("body".to_string(), Value::String(c.body.clone())),
                    ("lang".to_string(), Value::String("formula".to_string())),
                ]),
            ),
            ("inputs".to_string(), Value::Array(c.inputs.iter().map(|i| Value::String(i.clone())).collect())),
            ("signature".to_string(), v.get("signature").cloned().unwrap_or(Value::Null)),
        ]);
        world.bind(name, &descriptor, ZERO_INSTANT)?;
    }
    world.fuel = Fuel::new(limit);
    let key = CellKey { name: name.to_string(), args };
    let r = world.compute(&key, asof);
    let evaluated = world.evaluated();
    Ok(r.map(|computed| VectorResult { computed, fuel_used: world.fuel.used, evaluated }))
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}
