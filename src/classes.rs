//! Classes in the formula dialect (SPEC section 16, spec-0.5.0): the class descriptors a
//! definition or a call may name, as bound when it is read.
//!
//! The class descriptors themselves are `datumesh-core`'s ([`datumesh_core::ClassDescriptor`],
//! `define_class`, `ClassEnv`, `KeyNormalisation`, `parse_object_key`): this crate uses them
//! and does not reimplement them. A method `m` of the class named `N` is the function named
//! `N/m` (16.3), so a [`Classes`] holds the descriptors by class name and the functions of the
//! methods stay in the environment's functions (`Env::functions`).

use std::collections::BTreeMap;

use datumesh_core::json::Value;
use datumesh_core::ClassDescriptor;

use crate::env::EnvError;

/// Flattened class descriptors by class name, as bound at the time of a definition (the `classes`
/// of a `formula` vector's `env`, SPEC 11), of an evaluation or of a call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Classes {
    by_name: BTreeMap<String, ClassDescriptor>,
}

impl Classes {
    /// No classes.
    pub fn new() -> Classes {
        Classes::default()
    }

    /// Add (or replace) the class `name`.
    pub fn insert(&mut self, name: &str, descriptor: ClassDescriptor) {
        self.by_name.insert(name.to_string(), descriptor);
    }

    /// The flattened descriptor of the class `name`.
    pub fn get(&self, name: &str) -> Option<&ClassDescriptor> {
        self.by_name.get(name)
    }

    /// The class whose class hash (16.1) is `class_hash`, with its name: how a parent or an
    /// interface is found (a flattened descriptor names its parent by hash, 16.2).
    pub fn by_hash(&self, class_hash: &str) -> Option<(&str, &ClassDescriptor)> {
        self.by_name.iter().find(|(_, d)| d.hash() == class_hash).map(|(n, d)| (n.as_str(), d))
    }

    /// The class names, ascending.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(String::as_str)
    }

    /// Whether there are no classes.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Read `{class name: flattened descriptor}` (a vector's `env.classes`). A descriptor core
    /// does not accept is an [`EnvError`].
    pub fn from_value(v: &Value) -> Result<Classes, EnvError> {
        let m = v.as_object().ok_or_else(|| EnvError("classes is not an object".to_string()))?;
        let mut out = Classes::new();
        for (name, d) in m {
            let d = ClassDescriptor::from_value(d).map_err(|e| EnvError(format!("class {name}: {e}")))?;
            out.insert(name, d);
        }
        Ok(out)
    }
}

/// Why a method's definer is not found: a parent hash no class name is bound to (the hash), or the
/// environment's own failure.
#[derive(Debug)]
pub(crate) enum DefinerError<E> {
    /// No class name is bound to this parent hash.
    Unnamed(String),
    /// The environment failed.
    Failed(E),
}

/// The class a method `m` of `desc` is read from (16.3, K5): `None` when `desc` writes `m` itself, else the
/// name of the class that defines it. Deciding whether `m` is inherited needs only the parent's flattened
/// descriptor, found by its hash (`flat`, which holds the classes by name and the descriptors held by hash
/// only): an override is the subclass's own and needs no name. An inherited `m` is defined by the top of
/// the run of ancestors that carry it with the same function hash, and that class's name is `name`, the
/// class name bound to its hash. A hash that is not held, or with no name where a name is needed, is
/// `DefinerError::Unnamed`.
pub(crate) fn definer<E>(
    desc: &ClassDescriptor,
    m: &str,
    mut flat: impl FnMut(&str) -> Result<Option<ClassDescriptor>, E>,
    mut name: impl FnMut(&str) -> Result<Option<String>, E>,
) -> Result<Option<String>, DefinerError<E>> {
    let Some(h) = desc.methods.get(m) else { return Ok(None) };
    let mut cur = desc.clone();
    let mut defining: Option<String> = None;
    while let Some(ph) = cur.parent.clone() {
        let Some(pdesc) = flat(&ph).map_err(DefinerError::Failed)? else {
            return Err(DefinerError::Unnamed(ph));
        };
        if pdesc.methods.get(m) != Some(h) {
            break;
        }
        defining = Some(ph);
        cur = pdesc;
    }
    let Some(dh) = defining else { return Ok(None) };
    match name(&dh).map_err(DefinerError::Failed)? {
        Some(n) => Ok(Some(n)),
        None => Err(DefinerError::Unnamed(dh)),
    }
}
