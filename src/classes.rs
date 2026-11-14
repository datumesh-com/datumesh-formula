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
