# API changes in datumesh-formula 0.3.0

Generated with `cargo public-api -ss --color=never` (cargo-public-api 0.52.0, nightly-tools image)
on the committed listing at 0.2.0 (`public-api.txt`, `public-api-no-engine.txt`), with the default
features (`engine`) and with `--no-default-features`, built against `datumesh-core` 0.3.8 (commit
`0d53fa8`, a local copy until it is on crates.io), `datumesh-store` 0.4.1 (branch
`step-1.5-namespace-validation`, commit `db69efc`) and `datumesh-engine` 0.4.2 (the minimum). The release is
**breaking**: it is 0.3.0 and not 0.2.1, for the reasons below. The listings are regenerated
(`public-api.txt`, `public-api-no-engine.txt`). **No item is removed or changed in its signature**;
everything that breaks a user is an addition to a type that does not declare `#[non_exhaustive]`.

## Breaking

- `ast::Node` (and the re-export `formula::Node`) gains the variant `Dispatch { on, method, classes,
  args }` (SPEC 16.6). `Node` is not `#[non_exhaustive]`, so an exhaustive `match` on it no longer
  compiles.
- `Node::Read` gains the member `fn_hash: Option<String>` (a read pinned to a function hash, the
  `super` read of 16.3). A pattern `Node::Read { fn_name, args }` without `..` no longer compiles.
- `ArgDecl` (`formula::ArgDecl`) gains `extras: datumesh_core::ArgExtras` (`classes`, `default`,
  `key_normalisation`, 7); `Signature` gains `returns_classes: Option<Vec<String>>` (7, 16.6).
  Struct literals of these no longer compile; `Signature::from_value` and `ArgDecl` readers are
  unchanged.
- `Env` (`formula::Env`) gains the public members `classes: Classes` and `method_of: Option<String>`
  (16); `Env::new` and `Env::from_value` are unchanged in their signatures.
- `call::Call` gains `resolved: Option<datumesh_core::Resolved>` (5.5, 16.7); `call::MapCallEnv` gains
  `classes`, `aliases_tx`, `now` and `lookups`. Struct literals of either no longer compile.
- Dependencies: `datumesh-core` 0.3.8 (was 0.3.6) for `ClassDescriptor`, `define_class`,
  `ClassEnv`, `KeyNormalisation`, `parse_object_key` and `ArgExtras`; with feature `engine`,
  `datumesh-store` 0.4.1 (was 0.4.0) for `put_class`, `class`, `bind_class`, `resolve_class` and
  `resolve_class_binding`. Users on the earlier versions of the two move to these.

## Additive

- `formula::Code` (`#[non_exhaustive]`): `Method` (`E_FORMULA_METHOD`), `Dispatch`
  (`E_FORMULA_DISPATCH`), `Key` (`E_FORMULA_KEY`, a fault), `DispatchKey` (`E_FORMULA_DISPATCH_KEY`,
  a fault), and, since the clarifications of `c8896ad`, `ClassParent` (`E_CLASS_PARENT`, 10.2: the
  define of a `super` read with no class name bound to the parent's hash). `Code::ALL` in the order
  of 15.11, `ClassParent` last.
- `formula::Classes` (new module `classes`): flattened class descriptors by name, `by_hash`.
- `definition`: `define_method`, `define_class`, `publish_class` (feature `engine`), `ClassCallee`,
  `ClassDefinition`; the trait `Callees` gains the provided methods `bound_class`,
  `class_descriptor`, `class_name` and `function_descriptor` (each answers "none" by default).
- `eval`: `ClassBinding`; the trait `World` gains the provided methods `class` (none by default),
  `function_by_hash` (none) and `read_pinned` (`E_FORMULA_UNSUPPORTED` by default; the engine's
  runtime overrides both, so `super` works through the engine since datumesh-engine 0.4.2).
- `call`: the trait `CallEnv` gains the provided methods `bound_class`, `aliases_tx`, `now` and
  `lookup` (each answers none by default).
- `memory::MemoryWorld`: `bind_class`, and the three `World` methods above.
- Feature `engine`: `CallEnv` and `Callees` are implemented for `PgStore` and for `dyn Store` (and
  `dyn Store + Send + Sync`) with the class methods, from the store's classes.

## Behaviour (no new hash)

- Requests (5.8): a call's arguments are read by the declared type of their position (so an
  unquoted `2` in a string position is the string `"2"`, and a quoted `"2"` in a number position is
  `E_CANON_TYPE`); a bare name in the function position resolves through the alias map. A call the
  0.2.0 parser answered `E_PARSE_URI` for a method, a label or an object may now answer.
- Bodies: a period key (`1M`) and an uppercase word are key literals (a string where a string
  key or argument is; `E_FORMULA_RESERVED` elsewhere, as before). A body that was refused by 0.2.0
  for an uppercase word or a period may now be accepted in a string position.
- Defaults, key normalisation, `this`, method calls, dispatch and `super` change no hash of a
  descriptor that 0.2.0 accepted: a descriptor without `classes`, `default` or `key_normalisation`
  is byte-identical to its 0.2.0 form (7).
- The AST's arena is numbered in a canonical order (post-order); the JSON and the printed body are
  unchanged.
- Clarifications of spec-0.5.0 at `c8896ad` (SPEC-FINDINGS H and I; no hash changes, no API
  signature changes): `Code::is_mesh_failure` is true for `E_FORMULA_UNRESOLVED`, `E_FORMULA_DISPATCH`
  and `E_FORMULA_UNSUPPORTED` as well as for budget, cycle, depth and blob codes (15.12: they are
  the mesh's failures, never faults and not memoized); a dispatch relies on the method's binding
  only, and the class is read as of the evaluation asof; a quoted key encoding in a class-typed
  position is checked against its class; `E_FORMULA_ARG_MISSING` for a call that needs a now and
  has none; an extension method not taking the class keys is `E_ARG_UNKNOWN` in a call.

## Full diff

Output of `diff` between the 0.2.0 and 0.3.0 listings (`public-api.txt`), with the default features;
the `--no-default-features` listing differs by the same items without the engine's.

```text
> pub datumesh_formula::ast::Node::Dispatch
> pub datumesh_formula::ast::Node::Dispatch::args: alloc::vec::Vec<(alloc::string::String, datumesh_formula::ast::NodeId)>
> pub datumesh_formula::ast::Node::Dispatch::classes: alloc::vec::Vec<alloc::string::String>
> pub datumesh_formula::ast::Node::Dispatch::method: alloc::string::String
> pub datumesh_formula::ast::Node::Dispatch::on: datumesh_formula::ast::NodeId
> pub datumesh_formula::ast::Node::Read::fn_hash: core::option::Option<alloc::string::String>
> pub datumesh_formula::call::Call::resolved: core::option::Option<datumesh_core::envelope::Resolved>
> pub datumesh_formula::call::MapCallEnv::aliases_tx: core::option::Option<alloc::string::String>
> pub datumesh_formula::call::MapCallEnv::classes: alloc::collections::btree::map::BTreeMap<alloc::string::String, datumesh_core::class::ClassDescriptor>
> pub datumesh_formula::call::MapCallEnv::lookups: alloc::collections::btree::map::BTreeMap<alloc::string::String, alloc::collections::btree::map::BTreeMap<alloc::string::String, alloc::string::String>>
> pub datumesh_formula::call::MapCallEnv::now: core::option::Option<alloc::string::String>
> pub fn datumesh_formula::call::MapCallEnv::aliases_tx(&self) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_formula::call::MapCallEnv::bound_class(&self, &str, core::option::Option<&str>) -> core::result::Result<core::option::Option<datumesh_core::class::ClassDescriptor>, alloc::string::String>
> pub fn datumesh_formula::call::MapCallEnv::lookup(&self, &str, &str, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_formula::call::MapCallEnv::now(&self) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_formula::call::CallEnv::aliases_tx(&self) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_formula::call::CallEnv::bound_class(&self, &str, core::option::Option<&str>) -> core::result::Result<core::option::Option<datumesh_core::class::ClassDescriptor>, alloc::string::String>
> pub fn datumesh_formula::call::CallEnv::lookup(&self, &str, &str, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_formula::call::CallEnv::now(&self) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn (dyn datumesh_store::Store + core::marker::Send + core::marker::Sync)::aliases_tx(&self) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn (dyn datumesh_store::Store + core::marker::Send + core::marker::Sync)::bound_class(&self, &str, core::option::Option<&str>) -> core::result::Result<core::option::Option<datumesh_core::class::ClassDescriptor>, alloc::string::String>
> pub fn (dyn datumesh_store::Store + core::marker::Send + core::marker::Sync)::lookup(&self, &str, &str, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn (dyn datumesh_store::Store + core::marker::Send + core::marker::Sync)::now(&self) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_formula::call::MapCallEnv::aliases_tx(&self) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_formula::call::MapCallEnv::bound_class(&self, &str, core::option::Option<&str>) -> core::result::Result<core::option::Option<datumesh_core::class::ClassDescriptor>, alloc::string::String>
> pub fn datumesh_formula::call::MapCallEnv::lookup(&self, &str, &str, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_formula::call::MapCallEnv::now(&self) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_store::pg::PgStore::aliases_tx(&self) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_store::pg::PgStore::bound_class(&self, &str, core::option::Option<&str>) -> core::result::Result<core::option::Option<datumesh_core::class::ClassDescriptor>, alloc::string::String>
> pub fn datumesh_store::pg::PgStore::lookup(&self, &str, &str, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_store::pg::PgStore::now(&self) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub mod datumesh_formula::classes
> pub struct datumesh_formula::classes::Classes
> impl datumesh_formula::classes::Classes
> pub fn datumesh_formula::classes::Classes::by_hash(&self, &str) -> core::option::Option<(&str, &datumesh_core::class::ClassDescriptor)>
> pub fn datumesh_formula::classes::Classes::from_value(&datumesh_core::json::Value) -> core::result::Result<datumesh_formula::classes::Classes, datumesh_formula::EnvError>
> pub fn datumesh_formula::classes::Classes::get(&self, &str) -> core::option::Option<&datumesh_core::class::ClassDescriptor>
> pub fn datumesh_formula::classes::Classes::insert(&mut self, &str, datumesh_core::class::ClassDescriptor)
> pub fn datumesh_formula::classes::Classes::is_empty(&self) -> bool
> pub fn datumesh_formula::classes::Classes::names(&self) -> impl core::iter::traits::iterator::Iterator<Item = &str>
> pub fn datumesh_formula::classes::Classes::new() -> datumesh_formula::classes::Classes
> impl core::clone::Clone for datumesh_formula::classes::Classes
> pub fn datumesh_formula::classes::Classes::clone(&self) -> Self
> impl core::cmp::Eq for datumesh_formula::classes::Classes
> impl core::cmp::PartialEq for datumesh_formula::classes::Classes
> pub fn datumesh_formula::classes::Classes::eq(&self, &Self) -> bool
> impl core::default::Default for datumesh_formula::classes::Classes
> pub fn datumesh_formula::classes::Classes::default() -> Self
> impl core::fmt::Debug for datumesh_formula::classes::Classes
> pub fn datumesh_formula::classes::Classes::fmt(&self, &mut core::fmt::Formatter<'_>) -> core::fmt::Result
> impl core::marker::StructuralPartialEq for datumesh_formula::classes::Classes
> pub struct datumesh_formula::definition::ClassCallee
> pub datumesh_formula::definition::ClassCallee::class_hash: alloc::string::String
> pub datumesh_formula::definition::ClassCallee::descriptor: datumesh_core::class::ClassDescriptor
> pub datumesh_formula::definition::ClassCallee::start: alloc::string::String
> impl core::clone::Clone for datumesh_formula::definition::ClassCallee
> pub fn datumesh_formula::definition::ClassCallee::clone(&self) -> Self
> impl core::cmp::PartialEq for datumesh_formula::definition::ClassCallee
> pub fn datumesh_formula::definition::ClassCallee::eq(&self, &Self) -> bool
> impl core::fmt::Debug for datumesh_formula::definition::ClassCallee
> pub fn datumesh_formula::definition::ClassCallee::fmt(&self, &mut core::fmt::Formatter<'_>) -> core::fmt::Result
> impl core::marker::StructuralPartialEq for datumesh_formula::definition::ClassCallee
> pub struct datumesh_formula::definition::ClassDefinition
> pub datumesh_formula::definition::ClassDefinition::bytes: alloc::vec::Vec<u8>
> pub datumesh_formula::definition::ClassDefinition::class_hash: alloc::string::String
> pub datumesh_formula::definition::ClassDefinition::descriptor: datumesh_core::class::ClassDescriptor
> pub datumesh_formula::definition::ClassDefinition::name: alloc::string::String
> impl core::clone::Clone for datumesh_formula::definition::ClassDefinition
> pub fn datumesh_formula::definition::ClassDefinition::clone(&self) -> Self
> impl core::fmt::Debug for datumesh_formula::definition::ClassDefinition
> pub fn datumesh_formula::definition::ClassDefinition::fmt(&self, &mut core::fmt::Formatter<'_>) -> core::fmt::Result
> pub fn datumesh_formula::definition::Callees::bound_class(&self, &str, &str) -> core::result::Result<core::option::Option<datumesh_formula::definition::ClassCallee>, alloc::string::String>
> pub fn datumesh_formula::definition::Callees::class_descriptor(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::class::ClassDescriptor>, alloc::string::String>
> pub fn datumesh_formula::definition::Callees::class_name(&self, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_formula::definition::Callees::function_descriptor(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::json::Value>, alloc::string::String>
> pub fn (dyn datumesh_store::Store + core::marker::Send + core::marker::Sync)::bound_class(&self, &str, &str) -> core::result::Result<core::option::Option<datumesh_formula::definition::ClassCallee>, alloc::string::String>
> pub fn (dyn datumesh_store::Store + core::marker::Send + core::marker::Sync)::class_descriptor(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::class::ClassDescriptor>, alloc::string::String>
> pub fn (dyn datumesh_store::Store + core::marker::Send + core::marker::Sync)::class_name(&self, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn (dyn datumesh_store::Store + core::marker::Send + core::marker::Sync)::function_descriptor(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::json::Value>, alloc::string::String>
> pub fn datumesh_formula::Env::bound_class(&self, &str, &str) -> core::result::Result<core::option::Option<datumesh_formula::definition::ClassCallee>, alloc::string::String>
> pub fn datumesh_formula::Env::class_descriptor(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::class::ClassDescriptor>, alloc::string::String>
> pub fn datumesh_formula::Env::class_name(&self, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_formula::Env::function_descriptor(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::json::Value>, alloc::string::String>
> pub fn datumesh_store::pg::PgStore::bound_class(&self, &str, &str) -> core::result::Result<core::option::Option<datumesh_formula::definition::ClassCallee>, alloc::string::String>
> pub fn datumesh_store::pg::PgStore::class_descriptor(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::class::ClassDescriptor>, alloc::string::String>
> pub fn datumesh_store::pg::PgStore::class_name(&self, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_store::pg::PgStore::function_descriptor(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::json::Value>, alloc::string::String>
> pub fn dyn datumesh_store::Store::bound_class(&self, &str, &str) -> core::result::Result<core::option::Option<datumesh_formula::definition::ClassCallee>, alloc::string::String>
> pub fn dyn datumesh_store::Store::class_descriptor(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::class::ClassDescriptor>, alloc::string::String>
> pub fn dyn datumesh_store::Store::class_name(&self, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn dyn datumesh_store::Store::function_descriptor(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::json::Value>, alloc::string::String>
> pub fn datumesh_formula::definition::define_class(&str, &[u8], &str, &dyn datumesh_formula::definition::Callees, &str) -> core::result::Result<datumesh_formula::definition::ClassDefinition, datumesh_formula::DefineError>
> pub fn datumesh_formula::definition::define_method(&str, &str, &str, &datumesh_core::json::Value, &alloc::collections::btree::map::BTreeMap<alloc::string::String, alloc::string::String>, &dyn datumesh_formula::definition::Callees, &str) -> core::result::Result<datumesh_formula::definition::Definition, datumesh_formula::DefineError>
> pub fn datumesh_formula::definition::publish_class<S: datumesh_store::Store + ?core::marker::Sized>(&S, &datumesh_formula::definition::ClassDefinition, &str) -> core::result::Result<alloc::string::String, datumesh_store::error::Error>
> pub struct datumesh_formula::eval::ClassBinding
> pub datumesh_formula::eval::ClassBinding::class: alloc::rcs::arc::Arc<datumesh_core::class::ClassDescriptor>
> pub datumesh_formula::eval::ClassBinding::class_hash: alloc::string::String
> pub datumesh_formula::eval::ClassBinding::start: alloc::string::String
> impl core::clone::Clone for datumesh_formula::eval::ClassBinding
> pub fn datumesh_formula::eval::ClassBinding::clone(&self) -> Self
> impl core::fmt::Debug for datumesh_formula::eval::ClassBinding
> pub fn datumesh_formula::eval::ClassBinding::fmt(&self, &mut core::fmt::Formatter<'_>) -> core::fmt::Result
> pub fn datumesh_formula::eval::World::class(&mut self, &str, core::option::Option<&str>) -> core::result::Result<core::option::Option<datumesh_formula::eval::ClassBinding>, Self::Error>
> pub fn datumesh_formula::eval::World::function_by_hash(&mut self, &str) -> core::result::Result<core::option::Option<alloc::rcs::arc::Arc<datumesh_formula::Function>>, Self::Error>
> pub fn datumesh_formula::eval::World::read_pinned(&mut self, &str, &datumesh_formula::eval::CellKey) -> core::result::Result<core::option::Option<datumesh_formula::eval::Version>, datumesh_formula::eval::Stop<Self::Error>>
> pub fn datumesh_formula::memory::MemoryWorld::class(&mut self, &str, core::option::Option<&str>) -> core::result::Result<core::option::Option<datumesh_formula::eval::ClassBinding>, Self::Error>
> pub fn datumesh_formula::memory::MemoryWorld::function_by_hash(&mut self, &str) -> core::result::Result<core::option::Option<alloc::rcs::arc::Arc<datumesh_formula::Function>>, Self::Error>
> pub fn datumesh_formula::memory::MemoryWorld::read_pinned(&mut self, &str, &datumesh_formula::eval::CellKey) -> core::result::Result<core::option::Option<datumesh_formula::eval::Version>, datumesh_formula::eval::Stop<Self::Error>>
> pub fn datumesh_formula::memory::MemoryWorld::bind_class(&mut self, &str, &datumesh_core::json::Value, &str) -> core::result::Result<(), datumesh_formula::EnvError>
> pub fn datumesh_formula::memory::MemoryWorld::class(&mut self, &str, core::option::Option<&str>) -> core::result::Result<core::option::Option<datumesh_formula::eval::ClassBinding>, Self::Error>
> pub fn datumesh_formula::memory::MemoryWorld::function_by_hash(&mut self, &str) -> core::result::Result<core::option::Option<alloc::rcs::arc::Arc<datumesh_formula::Function>>, Self::Error>
> pub fn datumesh_formula::memory::MemoryWorld::read_pinned(&mut self, &str, &datumesh_formula::eval::CellKey) -> core::result::Result<core::option::Option<datumesh_formula::eval::Version>, datumesh_formula::eval::Stop<Self::Error>>
> pub datumesh_formula::Code::ClassParent
> pub datumesh_formula::Code::Dispatch
> pub datumesh_formula::Code::DispatchKey
> pub datumesh_formula::Code::Key
> pub datumesh_formula::Code::Method
> pub datumesh_formula::Node::Dispatch
> pub datumesh_formula::Node::Dispatch::args: alloc::vec::Vec<(alloc::string::String, datumesh_formula::ast::NodeId)>
> pub datumesh_formula::Node::Dispatch::classes: alloc::vec::Vec<alloc::string::String>
> pub datumesh_formula::Node::Dispatch::method: alloc::string::String
> pub datumesh_formula::Node::Dispatch::on: datumesh_formula::ast::NodeId
> pub datumesh_formula::Node::Read::fn_hash: core::option::Option<alloc::string::String>
> pub datumesh_formula::ArgDecl::extras: datumesh_core::descriptor::ArgExtras
> pub struct datumesh_formula::Classes
> impl datumesh_formula::classes::Classes
> pub fn datumesh_formula::classes::Classes::by_hash(&self, &str) -> core::option::Option<(&str, &datumesh_core::class::ClassDescriptor)>
> pub fn datumesh_formula::classes::Classes::from_value(&datumesh_core::json::Value) -> core::result::Result<datumesh_formula::classes::Classes, datumesh_formula::EnvError>
> pub fn datumesh_formula::classes::Classes::get(&self, &str) -> core::option::Option<&datumesh_core::class::ClassDescriptor>
> pub fn datumesh_formula::classes::Classes::insert(&mut self, &str, datumesh_core::class::ClassDescriptor)
> pub fn datumesh_formula::classes::Classes::is_empty(&self) -> bool
> pub fn datumesh_formula::classes::Classes::names(&self) -> impl core::iter::traits::iterator::Iterator<Item = &str>
> pub fn datumesh_formula::classes::Classes::new() -> datumesh_formula::classes::Classes
> impl core::clone::Clone for datumesh_formula::classes::Classes
> pub fn datumesh_formula::classes::Classes::clone(&self) -> Self
> impl core::cmp::Eq for datumesh_formula::classes::Classes
> impl core::cmp::PartialEq for datumesh_formula::classes::Classes
> pub fn datumesh_formula::classes::Classes::eq(&self, &Self) -> bool
> impl core::default::Default for datumesh_formula::classes::Classes
> pub fn datumesh_formula::classes::Classes::default() -> Self
> impl core::fmt::Debug for datumesh_formula::classes::Classes
> pub fn datumesh_formula::classes::Classes::fmt(&self, &mut core::fmt::Formatter<'_>) -> core::fmt::Result
> impl core::marker::StructuralPartialEq for datumesh_formula::classes::Classes
> pub datumesh_formula::Env::classes: datumesh_formula::classes::Classes
> pub datumesh_formula::Env::method_of: core::option::Option<alloc::string::String>
> pub fn datumesh_formula::Env::bound_class(&self, &str, &str) -> core::result::Result<core::option::Option<datumesh_formula::definition::ClassCallee>, alloc::string::String>
> pub fn datumesh_formula::Env::class_descriptor(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::class::ClassDescriptor>, alloc::string::String>
> pub fn datumesh_formula::Env::class_name(&self, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
> pub fn datumesh_formula::Env::function_descriptor(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::json::Value>, alloc::string::String>
> pub datumesh_formula::Signature::returns_classes: core::option::Option<alloc::vec::Vec<alloc::string::String>>
```
