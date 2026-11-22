//! The classes round (CONTRACT-classes-round.md), the parts that need API that does not exist yet:
//! this file does not compile until it does. The names are the ones this file chose where the
//! contract gives none (marked CHOSEN):
//!
//! - K7: `datumesh_formula::definition::{ClassDraft, define_method_in_draft}`. The contract names the
//!   function `define_method(class, draft, name, source, signature, callees, at)`, but `definition::define_method`
//!   already exists with another signature (`class, name, source, signature, aliases, callees, at`), so
//!   this file calls the new one `define_method_in_draft` (CHOSEN). `ClassDraft::from_value(&Value)` reads
//!   `{"keys": {...}, "order": [...], "parent": "<class hash>"?, "methods": {"<name>": <signature>}}` (CHOSEN).
//! - K8: `datumesh_formula::call::fold_label(&str) -> String`.
//! - K10: the hook `CallEnv::chain(&self, cell: &CellRef, asof: &str) -> Result<Option<String>, String>`: the
//!   value (a key encoding) of the cell a link read, as of `asof`; `None`: no value (CHOSEN).

mod common;

use std::cell::RefCell;
use std::collections::BTreeMap;

use common::round::*;
use datumesh_core::json::Value;
use datumesh_core::{CellRef, ClassDescriptor, FunctionDescriptor};
use datumesh_formula::call::{fold_label, parse_call, Call, CallEnv, CallError, MapCallEnv};
use datumesh_formula::definition::{define_method_in_draft, ClassDraft};

// ---------------------------------------------------------------------------------------------
// K7. `define_method` over a draft: `this` is a class that is not yet bound.

fn draft() -> ClassDraft {
    // keys k (number) and j (string); a method `m` taking the keys first, one (`late`) that does not.
    let m = r#"{"args": {"d": {"type": "date", "valid": true}, "j": {"type": "string", "valid": false},
        "k": {"type": "number", "unit": "1", "valid": false}}, "order": ["k", "j", "d"],
        "returns": {"type": "number", "unit": "1"}}"#;
    let late = r#"{"args": {"d": {"type": "date", "valid": true}, "j": {"type": "string", "valid": false},
        "k": {"type": "number", "unit": "1", "valid": false}}, "order": ["d", "k", "j"],
        "returns": {"type": "number", "unit": "1"}}"#;
    ClassDraft::from_value(&v(&format!(
        r#"{{"keys": {{"k": {{"type": "number", "unit": "1"}}, "j": {{"type": "string", "key_normalisation": "none"}}}},
        "order": ["k", "j"], "methods": {{"m": {m}, "late": {late}}}}}"#
    )))
    .unwrap_or_else(|e| panic!("{e}"))
}

const NEW: &str = "calc.example.net/fresh";

fn n_sig() -> Value {
    v(&sub_method_sig())
}

fn define_n(src: &str, name: &str) -> Result<datumesh_formula::definition::Definition, datumesh_formula::DefineError> {
    // No callee is bound: the class `fresh` does not exist yet.
    let callees = datumesh_formula::Env::new("calc.example.net/none", datumesh_formula::Signature::from_value(&n_sig()).unwrap());
    define_method_in_draft(NEW, &draft(), name, src, &n_sig(), &callees, AT)
}

/// `this.m` is checked against the draft: it reads `fresh/m` by name (D95) with the keys first, though
/// the class is bound nowhere.
#[test]
fn k7_this_is_checked_against_the_draft() {
    let d = define_n("this.m(d) + 1", "calc.example.net/fresh/n").unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d.compiled.inputs, ["calc.example.net/fresh/m"]);
    assert_eq!(d.descriptor.body, "calc.example.net/fresh/m(d: d, j: j, k: k) + 1\n");
    // Same as the plain read, which also needs the draft's signature to type.
    assert_eq!(d.compiled.returns.to_value(), v(r#"{"type": "number", "unit": "1"}"#));
}

/// A method the draft lacks, a wrong argument and a wrong count are refused, as against a bound class.
#[test]
fn k7_this_refuses_what_the_draft_does_not_have() {
    let e = define_n("this.zz(d)", "calc.example.net/fresh/n").unwrap_err();
    assert_eq!(e.code(), "E_FORMULA_METHOD", "{e}");
    let e = define_n("this.m()", "calc.example.net/fresh/n").unwrap_err();
    assert_eq!(e.code(), "E_FORMULA_ARG_MISSING", "{e}");
    let e = define_n("this.m(\"x\")", "calc.example.net/fresh/n").unwrap_err();
    assert_eq!(e.code(), "E_FORMULA_TYPE", "{e}");
}

/// A method that does not take the class's keys first is refused (16.2 step 6), as a method read.
#[test]
fn k7_a_method_not_taking_the_keys_first_is_refused() {
    let e = define_n("this.late(d)", "calc.example.net/fresh/n").unwrap_err();
    assert_eq!(e.code(), "E_FORMULA_METHOD", "{e}");
}

/// A mutual read is refused: a method that reads itself through `this` cannot be defined (the class
/// descriptor holds each method's hash, and a function cannot be defined after itself). The code is the
/// implementer's: `E_FORMULA_CYCLE` is the one the library has for a cycle.
#[test]
fn k7_a_mutual_read_is_refused() {
    let e = define_n("this.m(d)", "calc.example.net/fresh/m").unwrap_err();
    assert!(matches!(e.code(), "E_FORMULA_CYCLE" | "E_FORMULA_METHOD" | "E_FORMULA_UNRESOLVED"), "{e}");
    assert!(define_n("this.m(d)", "calc.example.net/fresh/n").is_ok());
}

/// The draft's parent: an inherited method is read as `D/m` (K5), so a draft naming a parent reads the
/// parent's methods through `this` too.
#[test]
fn k7_this_reads_a_parents_method_through_the_draft() {
    let w = world();
    let sub_draft = ClassDraft::from_value(&v(&format!(
        r#"{{"keys": {}, "order": ["k", "j"], "parent": "{}", "methods": {{}}}}"#,
        String::from_utf8(w.sub.get("keys").unwrap().to_jcs()).unwrap(),
        datumesh_core::ClassDescriptor::from_value(&w.par).unwrap().hash()
    )))
    .unwrap_or_else(|e| panic!("{e}"));
    let mut env = w.env(&d_only(), false, true);
    env.classes.insert(NEW, ClassDescriptor::from_value(&w.sub).unwrap());
    let d = define_method_in_draft(SUB, &sub_draft, "calc.example.net/sub/n", "this.st(d)", &n_sig(), &env, AT)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d.compiled.inputs, ["calc.example.net/par/st"]);
}

// ---------------------------------------------------------------------------------------------
// K8. fold_label.

/// Unicode default case folding, then NFC; each run of White_Space one space; trimmed.
#[test]
fn k8_fold_label() {
    for (label, folded) in [
        ("apple", "apple"),
        ("Apple", "apple"),
        ("  Apple   ", "apple"),
        ("APPLE PIE", "apple pie"),
        ("a \t\n\r b", "a b"),
        ("a\u{a0}\u{2003}\u{3000}b", "a b"),
        ("\u{a0} x \u{2028}", "x"),
        // Full case folding: sharp s and ligatures fold to more than one letter.
        ("Stra\u{df}e", "strasse"),
        ("STRASSE", "strasse"),
        ("o\u{fb03}ce", "office"),
        // Final sigma folds to sigma.
        ("\u{3a3}\u{3a3}", "\u{3c3}\u{3c3}"),
        ("\u{3c2}", "\u{3c3}"),
        // Folded, then NFC: A + ring above is a with ring above, composed.
        ("A\u{30a}", "\u{e5}"),
        ("\u{c5}", "\u{e5}"),
        ("e\u{301}", "\u{e9}"),
        // Zero width space is not White_Space.
        ("a\u{200b}b", "a\u{200b}b"),
        ("", ""),
        ("   ", ""),
    ] {
        assert_eq!(fold_label(label), folded, "{label:?}");
    }
}

/// Folding is idempotent: a folded label is its own folding (what `label_fold` calls canonical).
#[test]
fn k8_fold_label_is_idempotent() {
    for label in ["Apple  Pie", "Stra\u{df}e", "A\u{30a}  X", "\u{3a3}", "o\u{fb03}ce\u{a0}"] {
        let once = fold_label(label);
        assert_eq!(fold_label(&once), once, "{label:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// K10. Chains that parse.

struct Chain {
    inner: MapCallEnv,
    /// The cells read by links: (cell URI, asof).
    read: RefCell<Vec<(String, String)>>,
    values: BTreeMap<String, Option<String>>,
}

impl CallEnv for Chain {
    fn alias(&self, alias: &str) -> Result<Option<String>, String> {
        self.inner.alias(alias)
    }
    fn bound(&self, name: &str, asof: Option<&str>) -> Result<Option<FunctionDescriptor>, String> {
        self.inner.bound(name, asof)
    }
    fn pinned(&self, fn_hash: &str) -> Result<Option<FunctionDescriptor>, String> {
        self.inner.pinned(fn_hash)
    }
    fn bound_class(&self, name: &str, asof: Option<&str>) -> Result<Option<ClassDescriptor>, String> {
        self.inner.bound_class(name, asof)
    }
    fn now(&self) -> Result<Option<String>, String> {
        self.inner.now()
    }
    fn lookup(&self, class: &str, label: &str, asof: &str) -> Result<Option<String>, String> {
        self.inner.lookup(class, label, asof)
    }
    fn chain(&self, cell: &CellRef, asof: &str) -> Result<Option<String>, String> {
        let uri = datumesh_core::format_reference(&datumesh_core::Reference::from(cell.clone()));
        self.read.borrow_mut().push((uri.clone(), asof.to_string()));
        Ok(self.values.get(&uri).cloned().flatten())
    }
}

const NOW: &str = "2026-07-01T00:00:00.000000Z";
const OWNER0: &str = "cell://sec.example.org/company/owner?cik=0000320193";
const OWNER1: &str = "cell://sec.example.org/company/owner?cik=0000789019";
const KEY1: &str = "sec.example.org/company(cik:\"0000789019\")";
const KEY2: &str = "sec.example.org/company(cik:\"0001018724\")";

fn chain_env() -> Chain {
    let mut values = BTreeMap::new();
    values.insert(OWNER0.to_string(), Some(KEY1.to_string()));
    values.insert(OWNER1.to_string(), Some(KEY2.to_string()));
    Chain { inner: company_env(), read: RefCell::new(Vec::new()), values }
}

fn call_ok(src: &str, env: &dyn CallEnv) -> Call {
    parse_call(src, env).unwrap_or_else(|e| panic!("{src}: {e}"))
}

fn kinds(c: &Call) -> Vec<(String, String)> {
    c.resolved
        .as_ref()
        .map(|r| r.resolutions.iter().map(|x| (x.kind.as_str().to_string(), x.to.clone())).collect())
        .unwrap_or_default()
}

/// `N(k).m(a).n(b)`: the first method's cell is read for its key (as of the request's asof), and the
/// call is the second method on that object.
#[test]
fn k10_a_two_link_chain_parses() {
    let env = chain_env();
    let c = call_ok("sec.example.org/company(\"0000320193\").owner().fact(0000320193-24-000123)", &env);
    assert_eq!(c.uri(), "cell://sec.example.org/company/fact?accession=0000320193-24-000123&cik=0000789019");
    assert_eq!(*env.read.borrow(), [(OWNER0.to_string(), NOW.to_string())]);
}

/// With `?asof=` the links are read as of it; the key is read once per link.
#[test]
fn k10_links_are_read_as_of_the_requests_asof() {
    let env = chain_env();
    let asof = "2026-06-01T00:00:00.000000Z";
    let c = call_ok(&format!("sec.example.org/company(\"0000320193\").owner().owner().size()?asof={asof}"), &env);
    assert_eq!(c.uri(), format!("cell://sec.example.org/company/size?cik=0001018724&asof={asof}"));
    let read = env.read.borrow();
    assert_eq!(read.len(), 2);
    assert!(read.iter().all(|(_, a)| a == asof));
    assert!(read[0].0.starts_with(OWNER0), "{}", read[0].0);
}

/// `resolved` has one `chain` entry per link, `{asof, from, kind: chain, to}`, in order, between the
/// lookup (if any) and the final `method`.
#[test]
fn k10_resolved_has_the_chain_entries() {
    let env = chain_env();
    let c = call_ok("sec.example.org/company(\"0000320193\").owner().owner().size()", &env);
    let r = c.resolved.as_ref().expect("resolved");
    let chain: Vec<_> = r.resolutions.iter().filter(|x| x.kind.as_str() == "chain").collect();
    assert_eq!(chain.len(), 2, "{:?}", kinds(&c));
    assert_eq!(chain[0].to, KEY1);
    assert_eq!(chain[1].to, KEY2);
    assert!(chain.iter().all(|x| x.asof.as_deref() == Some(NOW)));
    assert!(chain[0].from.ends_with("owner"), "{}", chain[0].from);
    // The last resolution is the last method's.
    assert_eq!(r.resolutions.last().unwrap().kind.as_str(), "method");
    assert_eq!(r.resolutions.last().unwrap().to, "sec.example.org/company/size");
    // After a lookup.
    let c = call_ok("sec.example.org/company[apple].owner().size()", &env);
    let ks: Vec<String> = kinds(&c).into_iter().map(|k| k.0).collect();
    let first_chain = ks.iter().position(|k| k == "chain").expect("a chain entry");
    assert_eq!(ks[0], "lookup", "{ks:?}");
    assert!(first_chain > 0, "{ks:?}");
}

/// A chain from a label: the label is folded, looked up, and its object's method read.
#[test]
fn k10_a_chain_from_a_label() {
    let env = chain_env();
    let c = call_ok("sec.example.org/company[Apple].owner().fact(x)", &env);
    assert_eq!(c.uri(), "cell://sec.example.org/company/fact?accession=x&cik=0000789019");
}

/// A link whose previous cell has no value is that cell's outcome, not a parse error: the library says
/// so with a code of its own, and does not read the later links.
#[test]
fn k10_an_absent_intermediate_is_not_a_parse_error() {
    let mut env = chain_env();
    env.values.insert(OWNER0.to_string(), None);
    let e = parse_call("sec.example.org/company(\"0000320193\").owner().fact(x)", &env).unwrap_err();
    match e {
        CallError::Code(code, _) => assert_ne!(code, "E_PARSE_URI"),
        CallError::Env(_) => {}
        other => panic!("{other}"),
    }
}
