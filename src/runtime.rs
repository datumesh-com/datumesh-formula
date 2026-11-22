//! The engine's [`Runtime`] for `lang = "formula"` (feature `engine`): the evaluator of
//! [`eval`] over the engine's [`Inputs`], as a [`World`].
//!
//! - A descriptor is checked (15.6, [`check_descriptor`]) against its callees as bound now in
//!   the runtime's [`Callees`] (the store), and compiled once per function hash. When a read's
//!   name resolves, at evaluation, to a function other than the one the body was checked
//!   against (a callee rebound, or a name read as of an earlier `T`), the body is checked again
//!   against it (15.8), with the same codes. A body that does not check against the latest
//!   bindings is checked against those of the evaluation asof (`Inputs::binding`, which relies
//!   on every callee's name for that evaluation).
//! - Reads go through the engine: `read_cell`, `read_asof`, `read_slice`, `read_versions` and
//!   `binding`, which charge them as 15.12 says; the evaluator charges every node and built-in
//!   to the [`Budget`]. The engine derives the lineage, the manifests, `tx` and `asof`.
//! - The evaluation asof is asked of the engine (`Inputs::asof`) only by `asof()` and a bounded
//!   `latest`, whose functions are asof-dependent ([`Runtime::asof_dependent`] from the
//!   checker's flag), and to order `E_FORMULA_FUTURE` before a failure of the cell's arguments
//!   in `asof(c, T)` and `versions(c, from, to)`.
//! - A pinned read (`super`, 16.3) is the engine's `read_pinned`, of the function stored under
//!   the parent's hash (found through the callees), which relies on no binding.
//! - A fault is [`EvalError::Fault`] with its code, so the engine answers
//!   `NotCovered { fault }`; a computed input's fault comes back as the formula code, which
//!   `otherwise` may catch (`E_FORMULA_EMPTY`, `E_FORMULA_ABSENT`). The engine's own errors
//!   (an input not evaluated yet, without a value, a cycle, the depth, the budget) pass through
//!   unchanged. Any other code (a define-time code, `E_FORMULA_UNRESOLVED` at a `T` before the
//!   binding) is [`EvalError::Runtime`] with the code first in its message.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use datumesh_core::{Arg, Cell, ClassDescriptor, Fault, FunctionDescriptor, Scalar};
use datumesh_engine::{Budget, EvalError, FnBinding, Inputs, Pattern, Runtime, SlicePattern};

use crate::definition::{Callees, LATEST};
use crate::env::{Env, Function};
use crate::eval::{self, Args, Binding, CellKey, ClassBinding, PatternArg, SliceKey, Stop, Version, World};
use crate::{check_descriptor, Code, Compiled, DefineError};

/// Counts of a runtime's evaluations ([`FormulaRuntime::stats`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RuntimeStats {
    /// `evaluate` calls (attempts).
    pub attempts: u64,
    /// Attempts that completed: with a value or a fault (the cells evaluated).
    pub completed: u64,
    /// Attempts suspended on an input not evaluated yet (run again by the engine).
    pub suspended: u64,
    /// Completed attempts that faulted.
    pub faults: u64,
}

#[derive(Default)]
struct Counters {
    attempts: AtomicU64,
    completed: AtomicU64,
    suspended: AtomicU64,
    faults: AtomicU64,
}

/// A check again after a rebinding: the body compiled anew when its types changed, or the code.
type Recheck = Result<Option<Arc<Compiled>>, Code>;

/// A function hash and the hashes of its callees as an evaluation saw them.
type PreparedKey = (String, Vec<String>);

/// A descriptor compiled against its callees.
struct Prepared {
    function: Arc<Function>,
    env: Env,
    compiled: Arc<Compiled>,
}

/// The formula runtime: `Engine::builder(..).runtime(Arc::new(FormulaRuntime::new(store)))`.
pub struct FormulaRuntime {
    callees: Arc<dyn Callees + Send + Sync>,
    prepared: Mutex<HashMap<String, Arc<Prepared>>>,
    /// Descriptors compiled against the bindings an evaluation saw: (function hash, callee
    /// hashes) to the compiled body.
    prepared_at: Mutex<HashMap<PreparedKey, Arc<Prepared>>>,
    /// Checks again after a rebinding: (function hash, name, callee hash) to the result.
    rechecked: Mutex<HashMap<(String, String, String), Recheck>>,
    functions: Mutex<HashMap<String, Arc<Function>>>,
    counters: Counters,
}

struct StoreCallees(Arc<dyn datumesh_store::Store + Send + Sync>);

impl Callees for StoreCallees {
    fn callee(&self, name: &str, at: &str) -> Result<Option<crate::definition::Callee>, String> {
        (*self.0).callee(name, at)
    }
    fn bound_class(&self, name: &str, at: &str) -> Result<Option<crate::definition::ClassCallee>, String> {
        (*self.0).bound_class(name, at)
    }
    fn class_descriptor(&self, class_hash: &str) -> Result<Option<datumesh_core::ClassDescriptor>, String> {
        (*self.0).class_descriptor(class_hash)
    }
    /// The first class name the store binds the hash to, as of the latest (K5): a dispatch to an
    /// inherited method names its defining class through it.
    fn class_name(&self, class_hash: &str) -> Result<Option<String>, String> {
        crate::call::store_class_name_of(&*self.0, class_hash, None)
    }
    fn function_descriptor(&self, fn_hash: &str) -> Result<Option<datumesh_core::json::Value>, String> {
        (*self.0).function_descriptor(fn_hash)
    }
}

impl std::fmt::Debug for FormulaRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FormulaRuntime").field("stats", &self.stats()).finish_non_exhaustive()
    }
}

impl FormulaRuntime {
    /// A runtime whose callees' descriptors come from `store` (the engine's).
    pub fn new(store: Arc<dyn datumesh_store::Store + Send + Sync>) -> FormulaRuntime {
        FormulaRuntime::with_callees(Arc::new(StoreCallees(store)))
    }

    /// A runtime whose callees' descriptors come from `callees`.
    pub fn with_callees(callees: Arc<dyn Callees + Send + Sync>) -> FormulaRuntime {
        FormulaRuntime {
            callees,
            prepared: Mutex::new(HashMap::new()),
            prepared_at: Mutex::new(HashMap::new()),
            rechecked: Mutex::new(HashMap::new()),
            functions: Mutex::new(HashMap::new()),
            counters: Counters::default(),
        }
    }

    /// The counts of its evaluations so far.
    pub fn stats(&self) -> RuntimeStats {
        let c = &self.counters;
        RuntimeStats {
            attempts: c.attempts.load(Ordering::Relaxed),
            completed: c.completed.load(Ordering::Relaxed),
            suspended: c.suspended.load(Ordering::Relaxed),
            faults: c.faults.load(Ordering::Relaxed),
        }
    }

    /// The function of a descriptor, by function hash.
    fn function_of(&self, fn_hash: &str, d: &FunctionDescriptor) -> Result<Arc<Function>, EvalError> {
        self.function_of_value(fn_hash, &d.to_value())
    }

    /// The function of a descriptor's JSON value, by function hash.
    fn function_of_value(&self, fn_hash: &str, value: &datumesh_core::json::Value) -> Result<Arc<Function>, EvalError> {
        let mut fs = self.functions.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(f) = fs.get(fn_hash) {
            return Ok(f.clone());
        }
        let f = Arc::new(Function::from_value(value).map_err(|e| EvalError::Runtime(e.to_string()))?);
        fs.insert(fn_hash.to_string(), f.clone());
        Ok(f)
    }

    /// The functions a body pins its reads to (16.3, `super`), found by their hashes in the
    /// callees whatever their names are bound to now: a pinned read relies on no binding. A
    /// function not found is left for the checker to report (`E_FORMULA_UNRESOLVED`); a body that
    /// does not parse is left for the checker too.
    fn add_pinned(&self, body: &str, env: &mut Env) -> Result<(), DefineError> {
        let Ok(reads) = crate::pinned_reads(body) else { return Ok(()) };
        for (_, hash) in reads {
            if env.pinned.contains_key(&hash) {
                continue;
            }
            if let Some(v) = self.callees.function_descriptor(&hash).map_err(DefineError::Callees)? {
                let f = Function::from_value(&v).map_err(|e| DefineError::Callees(e.0))?;
                env.pinned.insert(hash, f);
            }
        }
        Ok(())
    }

    /// The descriptor checked against its callees as bound now, compiled.
    fn prepare(&self, d: &FunctionDescriptor, name: &str) -> Result<Arc<Prepared>, DefineError> {
        let fn_hash = d.hash();
        if let Some(p) = self.prepared.lock().unwrap_or_else(|p| p.into_inner()).get(&fn_hash) {
            return Ok(p.clone());
        }
        let value = d.to_value();
        let function = Arc::new(Function::from_value(&value).map_err(|e| DefineError::Callees(e.0))?);
        let mut env = Env::new(name, function.signature.clone());
        for callee in &d.inputs {
            if callee == name {
                continue;
            }
            if let Some(c) = self.callees.callee(callee, LATEST).map_err(DefineError::Callees)? {
                env.add_function(callee, &c.descriptor).map_err(|e| DefineError::Callees(e.0))?;
            }
        }
        for class in input_classes(d) {
            if let Some(c) = self.callees.bound_class(&class, LATEST).map_err(DefineError::Callees)? {
                env.classes.insert(&class, c.descriptor);
            }
        }
        self.add_pinned(&d.body, &mut env)?;
        env.method_of = method_class(&env, name, &function.hash);
        let compiled = Arc::new(check_descriptor(name, &value, &env)?);
        let p = Arc::new(Prepared { function, env, compiled });
        // Errors are not kept: a later binding may cure them.
        self.prepared.lock().unwrap_or_else(|p| p.into_inner()).insert(fn_hash, p.clone());
        Ok(p)
    }

    /// The descriptor checked against its callees as bound as of the evaluation asof (the
    /// engine's `binding`, which relies on each name as a read does): for a body that does not
    /// check against the latest bindings, at an asof where an earlier one holds.
    fn prepare_at(
        &self,
        d: &FunctionDescriptor,
        name: &str,
        inputs: &mut dyn Inputs,
    ) -> Result<Arc<Prepared>, EvalError> {
        let value = d.to_value();
        let function = Arc::new(Function::from_value(&value).map_err(|e| EvalError::Runtime(e.to_string()))?);
        let mut env = Env::new(name, function.signature.clone());
        let mut hashes = Vec::new();
        for callee in &d.inputs {
            if callee == name {
                continue;
            }
            if let Some(b) = inputs.binding(callee, None)? {
                let f = self.function_of(&b.fn_hash, &b.descriptor)?;
                env.functions.insert(callee.clone(), (*f).clone());
                hashes.push(b.fn_hash);
            } else {
                hashes.push(String::new());
            }
        }
        for class in input_classes(d) {
            if let Some(c) = self.callees.bound_class(&class, LATEST).map_err(EvalError::Runtime)? {
                env.classes.insert(&class, c.descriptor);
            }
        }
        self.add_pinned(&d.body, &mut env).map_err(|e| EvalError::Runtime(e.to_string()))?;
        env.method_of = method_class(&env, name, &function.hash);
        env.evaluating = true;
        let key = (function.hash.clone(), hashes);
        if let Some(p) = self.prepared_at.lock().unwrap_or_else(|p| p.into_inner()).get(&key) {
            return Ok(p.clone());
        }
        let compiled = Arc::new(check_descriptor(name, &value, &env).map_err(|e| match e {
            DefineError::Formula(e) => runtime_error(e.code, &e.detail),
            other => EvalError::Runtime(other.to_string()),
        })?);
        let p = Arc::new(Prepared { function, env, compiled });
        self.prepared_at.lock().unwrap_or_else(|p| p.into_inner()).insert(key, p.clone());
        Ok(p)
    }

    /// The body checked again with `name` meaning `function` (15.8).
    fn recheck(&self, p: &Prepared, name: &str, function: &Function) -> Result<Option<Arc<Compiled>>, Code> {
        let key = (p.function.hash.clone(), name.to_string(), function.hash.clone());
        if let Some(r) = self.rechecked.lock().unwrap_or_else(|p| p.into_inner()).get(&key) {
            return r.clone();
        }
        let r = if name == p.env.name {
            // The function reading itself under a name now meaning another function.
            if function.signature == p.function.signature {
                Ok(None)
            } else {
                Err(Code::Type)
            }
        } else {
            let mut env = p.env.clone();
            env.evaluating = true;
            env.functions.insert(name.to_string(), function.clone());
            match check_descriptor(&p.env.name, &p.function.value, &env) {
                Ok(c) if c.types_eq(&p.compiled) => Ok(None),
                Ok(c) => Ok(Some(Arc::new(c))),
                Err(e) => Err(e.formula_code().unwrap_or(Code::Syntax)),
            }
        };
        self.rechecked.lock().unwrap_or_else(|p| p.into_inner()).insert(key, r.clone());
        r
    }
}

/// The classes a descriptor's inputs name: each input `N/m` that is a method of a class `N`
/// (16.3), and a dispatch's classes (their methods are inputs).
fn input_classes(d: &FunctionDescriptor) -> std::collections::BTreeSet<String> {
    let mut out: std::collections::BTreeSet<String> = d
        .inputs
        .iter()
        .filter_map(|i| i.rsplit_once('/').map(|(c, _)| c.to_string()))
        .filter(|c| crate::names::is_function_name(c))
        .collect();
    out.extend(d.arg_extras.values().filter_map(|x| x.classes.clone()).flatten());
    out.extend(d.returns_classes.iter().flatten().cloned());
    out
}

/// The class whose method `name` with this hash is, among the environment's classes (16.3).
fn method_class(env: &Env, name: &str, fn_hash: &str) -> Option<String> {
    let (class, method) = name.rsplit_once('/')?;
    let c = env.classes.get(class)?;
    (c.methods.get(method).map(String::as_str) == Some(fn_hash)).then(|| class.to_string())
}

fn runtime_error(code: Code, detail: &str) -> EvalError {
    EvalError::Runtime(format!("{code}: {detail}"))
}

impl Runtime for FormulaRuntime {
    fn lang(&self) -> &str {
        "formula"
    }

    fn evaluate(
        &self,
        descriptor: &FunctionDescriptor,
        cell: &Cell,
        inputs: &mut dyn Inputs,
        budget: &mut Budget,
    ) -> Result<Scalar, EvalError> {
        self.counters.attempts.fetch_add(1, Ordering::Relaxed);
        let name = cell.function_name();
        let p = match self.prepare(descriptor, &name) {
            Ok(p) => p,
            // Against the latest bindings the body does not check: against those of the asof.
            Err(DefineError::Formula(_)) => self.prepare_at(descriptor, &name, inputs)?,
            Err(other) => return Err(EvalError::Runtime(other.to_string())),
        };
        let mut args = Args::new();
        for (k, a) in &cell.args {
            match a {
                Arg::Value(s) => {
                    args.insert(k.clone(), s.clone());
                }
                Arg::Slice => return Err(runtime_error(Code::Type, &format!("argument {k} is open"))),
            }
        }
        let mut world = EngineWorld { rt: self, p: &p, inputs, budget, budget_error: None };
        let r = eval::evaluate_value(&p.compiled, &p.function, &args, &mut world);
        let budget_error = world.budget_error.take();
        let r = r.map_err(|s| match s {
            Stop::World(e) => e,
            Stop::Code(Code::Budget) => budget_error.unwrap_or(EvalError::BudgetExceeded { fuel: budget.limit() }),
            Stop::Code(c) => match Fault::from_code(c.as_str()) {
                Some(f) => EvalError::Fault(f),
                None => runtime_error(c, "evaluation"),
            },
        });
        match &r {
            Ok(_) => {
                self.counters.completed.fetch_add(1, Ordering::Relaxed);
            }
            Err(EvalError::Fault(_)) => {
                self.counters.completed.fetch_add(1, Ordering::Relaxed);
                self.counters.faults.fetch_add(1, Ordering::Relaxed);
            }
            Err(EvalError::Suspended) => {
                self.counters.suspended.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {}
        }
        r
    }

    fn asof_dependent(&self, descriptor: &FunctionDescriptor) -> bool {
        // The checker's flag is syntactic (`asof()`, or `latest` with a bound): read from the
        // canonical AST, which a printed body has without an environment.
        crate::canonical_ast(&descriptor.body, None).is_ok_and(|a| a.reads_asof())
    }
}

/// The evaluator's world over the engine's inputs, for one attempt.
struct EngineWorld<'a> {
    rt: &'a FormulaRuntime,
    p: &'a Prepared,
    inputs: &'a mut dyn Inputs,
    budget: &'a mut Budget,
    /// The budget's error, for `E_FORMULA_BUDGET`.
    budget_error: Option<EvalError>,
}

fn core_args(args: &Args) -> datumesh_core::Args {
    args.iter().map(|(k, s)| (k.clone(), Arg::Value(s.clone()))).collect()
}

/// An engine error as the evaluator sees it: a fault as its code (catchable by `otherwise`
/// where 15.12 says), anything else the world's.
fn stop(e: EvalError) -> Stop<EvalError> {
    match e {
        EvalError::Fault(f) => match Code::from_name(f.as_str()) {
            Some(c) => Stop::Code(c),
            None => Stop::World(EvalError::Fault(f)),
        },
        other => Stop::World(other),
    }
}

fn version(i: datumesh_engine::Input) -> Version {
    Version { value: i.value, receipt_hash: i.receipt_hash, tx: i.tx, asof: i.asof }
}

impl World for EngineWorld<'_> {
    type Error = EvalError;

    fn function(&mut self, name: &str, at: Option<&str>) -> Result<Option<Binding>, EvalError> {
        let Some(FnBinding { fn_hash, start, descriptor, .. }) = self.inputs.binding(name, at)? else {
            return Ok(None);
        };
        let function = self.rt.function_of(&fn_hash, &descriptor)?;
        Ok(Some(Binding { function, start }))
    }

    fn read(&mut self, cell: &CellKey, at: Option<&str>) -> Result<Option<Version>, Stop<EvalError>> {
        let args = core_args(&cell.args);
        let r = match at {
            None => self.inputs.read_cell(&cell.name, &args),
            Some(t) => self.inputs.read_asof(&cell.name, &args, t),
        };
        r.map(|i| Some(version(i))).map_err(stop)
    }

    fn members(&mut self, slice: &SliceKey) -> Result<Vec<(Args, Version)>, Stop<EvalError>> {
        let pattern: SlicePattern = slice
            .args
            .iter()
            .map(|(k, p)| {
                let p = match p {
                    PatternArg::Value(v) => Pattern::Value(v.clone()),
                    PatternArg::Open => Pattern::Open,
                    PatternArg::Range(lo, hi) => Pattern::Range { lo: lo.clone(), hi: hi.clone() },
                };
                (k.clone(), p)
            })
            .collect();
        let members = self.inputs.read_slice(&slice.name, &pattern).map_err(stop)?;
        let mut out = Vec::with_capacity(members.len());
        for m in members {
            let mut args = Args::new();
            for (k, a) in m.args {
                match a {
                    Arg::Value(s) => {
                        args.insert(k, s);
                    }
                    Arg::Slice => return Err(Stop::Code(Code::Type)),
                }
            }
            out.push((args, version(m.input)));
        }
        Ok(out)
    }

    fn versions(&mut self, cell: &CellKey, from: &str, to: &str) -> Result<Vec<Version>, Stop<EvalError>> {
        let args = core_args(&cell.args);
        let vs = self.inputs.read_versions(&cell.name, &args, from, to).map_err(stop)?;
        Ok(vs.into_iter().map(version).collect())
    }

    fn charge(&mut self, n: u64) -> Result<(), Code> {
        self.budget.charge(n).map_err(|e| {
            self.budget_error = Some(e);
            Code::Budget
        })
    }

    fn asof(&mut self) -> Result<String, Stop<EvalError>> {
        Ok(self.inputs.asof())
    }

    /// A class as the store binds it (16.3, 9.7): the engine's inputs bind functions, and classes
    /// are the store's, through the callees.
    fn class_descriptor(&mut self, class_hash: &str, _at: Option<&str>) -> Result<Option<ClassDescriptor>, EvalError> {
        self.rt.callees.class_descriptor(class_hash).map_err(EvalError::Runtime)
    }

    fn class_name(&mut self, class_hash: &str, _at: Option<&str>) -> Result<Option<String>, EvalError> {
        self.rt.callees.class_name(class_hash).map_err(EvalError::Runtime)
    }

    fn class(&mut self, name: &str, at: Option<&str>) -> Result<Option<ClassBinding>, EvalError> {
        // The evaluator asks as of a method's binding start (the transaction that binds the class's
        // methods too), which does not make the receipt depend on the asof (`Inputs::asof` would).
        let at = at.unwrap_or(LATEST).to_string();
        let Some(c) = self.rt.callees.bound_class(name, &at).map_err(EvalError::Runtime)? else { return Ok(None) };
        Ok(Some(ClassBinding { class: Arc::new(c.descriptor), class_hash: c.class_hash, start: c.start }))
    }

    /// The engine's blob content (`Inputs::read_blob`, which checks the length and the hash);
    /// its `BlobUnavailable` passes through, so the request fails with the engine's error.
    fn blob(&mut self, sha256: &str, length: u64) -> Result<Option<Vec<u8>>, Stop<EvalError>> {
        self.inputs.read_blob(sha256, length).map(Some).map_err(Stop::World)
    }

    fn no_value(error: &EvalError) -> bool {
        matches!(error, EvalError::InputNotCovered { .. })
    }

    /// The function stored under `fn_hash` (the callees' descriptor with that hash), for a
    /// pinned read's signature (16.3 `super`). Its name is not resolved.
    fn function_by_hash(&mut self, fn_hash: &str) -> Result<Option<Arc<Function>>, EvalError> {
        let Some(value) = self.rt.callees.function_descriptor(fn_hash).map_err(EvalError::Runtime)? else {
            return Ok(None);
        };
        self.rt.function_of_value(fn_hash, &value).map(Some)
    }

    /// The cell of `cell.name` at `fn_hash`, as of the evaluation asof, through the engine's
    /// pinned read (`Inputs::read_pinned`, datumesh-engine 0.4.2): no name is resolved and no
    /// binding is relied on (16.3, 8.2 rule 2).
    fn read_pinned(&mut self, fn_hash: &str, cell: &CellKey) -> Result<Option<Version>, Stop<EvalError>> {
        let args = core_args(&cell.args);
        self.inputs.read_pinned(&cell.name, fn_hash, &args).map(|i| Some(version(i))).map_err(stop)
    }

    fn rebound(&mut self, name: &str, binding: &Binding) -> Result<Option<Arc<Compiled>>, Stop<EvalError>> {
        let same = if name == self.p.env.name {
            binding.function.hash == self.p.function.hash
        } else {
            self.p.env.functions.get(name).is_some_and(|f| f.hash == binding.function.hash)
        };
        if same {
            return Ok(None);
        }
        self.rt.recheck(self.p, name, &binding.function).map_err(Stop::Code)
    }
}
