# API changes in datumesh-formula 0.1.1

Generated with `cargo public-api -ss --color=never diff 0.1.0` (cargo-public-api 0.52.0,
nightly-tools image) against 0.1.0 on crates.io; the full output is at the end. Nothing removed
or changed: additive only.

## Added

| Item | What |
|---|---|
| `call` module: `parse_call`, `Call` (`cell`, `fn_hash`, `uri()`), `CallEnv`, `MapCallEnv`, `CallError` | Calls (SPEC 5.8, spec-0.4.2): a read of literals, optionally pinned (`name@<fn_hash>`) and followed by `?asof=`, parsed against an environment's aliases, bindings and descriptors by hash, in 5.8's order and codes; the cell URI it is written as. No engine needed. |
| `impl CallEnv for PgStore` and `for dyn Store + Send + Sync` (feature `engine`) | A store as the call environment (bindings and descriptors; no alias map). |
| `eval::World::blob` (provided: `None`) | Blob content by SHA-256 for `text_of`; the evaluator checks its length and hash. Provided, so no `World` breaks. The engine adapter answers `None` until datumesh-engine 0.3.2 gives a runtime blob content. |
| `memory::MemoryWorld::add_blob` | A vector world's `blobs`. |
| `Code::BlobUnavailable` (`E_FORMULA_BLOB_UNAVAILABLE`) | The evaluator's failure when `text_of` cannot obtain the content (`Code` is `#[non_exhaustive]`). |

## Behaviour (not in the listing)

- The lexer reads a pin (`@` and 64 lowercase hex digits directly after a qualified name or
  alias, L4a); anything else after such an `@` is `E_FORMULA_NAME`. A pinned read in a body is
  `E_FORMULA_PLACEMENT`.
- `fn_hash` as an argument name is `E_CANON_ARG_NAME` (`DefineError::Descriptor`) in
  `check_descriptor` and `define_function` (2.4, spec-0.4.2).
- Built-ins `text_of(blob)` and `remove_chars(text, chars)` (15.7, spec-0.4.2 at `a468606`),
  with their fuel.
- Dependencies: `datumesh-core` 0.3.5, `datumesh-engine` 0.3.1, `datumesh-store` 0.3.2.

## Full diff

```text
Removed items from the public API
=================================
(none)

Changed items in the public API
===============================
(none)

Added items to the public API
=============================
+pub mod datumesh_formula::call
+#[non_exhaustive] pub enum datumesh_formula::call::CallError
+pub datumesh_formula::call::CallError::Code(&'static str, alloc::string::String)
+pub datumesh_formula::call::CallError::Env(alloc::string::String)
+impl datumesh_formula::call::CallError
+pub fn datumesh_formula::call::CallError::code(&self) -> &str
+impl core::clone::Clone for datumesh_formula::call::CallError
+pub fn datumesh_formula::call::CallError::clone(&self) -> Self
+impl core::cmp::Eq for datumesh_formula::call::CallError
+impl core::cmp::PartialEq for datumesh_formula::call::CallError
+pub fn datumesh_formula::call::CallError::eq(&self, &Self) -> bool
+impl core::error::Error for datumesh_formula::call::CallError
+impl core::fmt::Debug for datumesh_formula::call::CallError
+pub fn datumesh_formula::call::CallError::fmt(&self, &mut core::fmt::Formatter<'_>) -> core::fmt::Result
+pub fn datumesh_formula::call::CallError::fmt(&self, &mut core::fmt::Formatter<'_>) -> core::fmt::Result
+impl core::fmt::Display for datumesh_formula::call::CallError
+impl core::marker::StructuralPartialEq for datumesh_formula::call::CallError
+pub struct datumesh_formula::call::Call
+pub datumesh_formula::call::Call::cell: datumesh_core::model::CellRef
+pub datumesh_formula::call::Call::fn_hash: core::option::Option<alloc::string::String>
+impl datumesh_formula::call::Call
+pub fn datumesh_formula::call::Call::uri(&self) -> alloc::string::String
+impl core::clone::Clone for datumesh_formula::call::Call
+pub fn datumesh_formula::call::Call::clone(&self) -> Self
+impl core::cmp::Eq for datumesh_formula::call::Call
+impl core::cmp::PartialEq for datumesh_formula::call::Call
+pub fn datumesh_formula::call::Call::eq(&self, &Self) -> bool
+impl core::fmt::Debug for datumesh_formula::call::Call
+pub fn datumesh_formula::call::Call::fmt(&self, &mut core::fmt::Formatter<'_>) -> core::fmt::Result
+impl core::marker::StructuralPartialEq for datumesh_formula::call::Call
+pub struct datumesh_formula::call::MapCallEnv
+pub datumesh_formula::call::MapCallEnv::aliases: alloc::collections::btree::map::BTreeMap<alloc::string::String, alloc::string::String>
+pub datumesh_formula::call::MapCallEnv::descriptors: alloc::collections::btree::map::BTreeMap<alloc::string::String, datumesh_core::descriptor::FunctionDescriptor>
+pub datumesh_formula::call::MapCallEnv::functions: alloc::collections::btree::map::BTreeMap<alloc::string::String, datumesh_core::descriptor::FunctionDescriptor>
+impl datumesh_formula::call::MapCallEnv
+pub fn datumesh_formula::call::MapCallEnv::from_value(&datumesh_core::json::Value) -> core::result::Result<datumesh_formula::call::MapCallEnv, datumesh_core::error::Error>
+impl core::clone::Clone for datumesh_formula::call::MapCallEnv
+pub fn datumesh_formula::call::MapCallEnv::clone(&self) -> Self
+impl core::default::Default for datumesh_formula::call::MapCallEnv
+pub fn datumesh_formula::call::MapCallEnv::default() -> Self
+impl core::fmt::Debug for datumesh_formula::call::MapCallEnv
+pub fn datumesh_formula::call::MapCallEnv::fmt(&self, &mut core::fmt::Formatter<'_>) -> core::fmt::Result
+impl datumesh_formula::call::CallEnv for datumesh_formula::call::MapCallEnv
+impl datumesh_formula::call::CallEnv for datumesh_formula::call::MapCallEnv
+pub fn datumesh_formula::call::MapCallEnv::alias(&self, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
+pub fn datumesh_formula::call::MapCallEnv::alias(&self, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
+pub fn datumesh_formula::call::MapCallEnv::bound(&self, &str, core::option::Option<&str>) -> core::result::Result<core::option::Option<datumesh_core::descriptor::FunctionDescriptor>, alloc::string::String>
+pub fn datumesh_formula::call::MapCallEnv::bound(&self, &str, core::option::Option<&str>) -> core::result::Result<core::option::Option<datumesh_core::descriptor::FunctionDescriptor>, alloc::string::String>
+pub fn datumesh_formula::call::MapCallEnv::pinned(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::descriptor::FunctionDescriptor>, alloc::string::String>
+pub fn datumesh_formula::call::MapCallEnv::pinned(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::descriptor::FunctionDescriptor>, alloc::string::String>
+pub trait datumesh_formula::call::CallEnv
+pub fn datumesh_formula::call::CallEnv::alias(&self, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
+pub fn datumesh_formula::call::CallEnv::bound(&self, &str, core::option::Option<&str>) -> core::result::Result<core::option::Option<datumesh_core::descriptor::FunctionDescriptor>, alloc::string::String>
+pub fn datumesh_formula::call::CallEnv::pinned(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::descriptor::FunctionDescriptor>, alloc::string::String>
+impl datumesh_formula::call::CallEnv for (dyn datumesh_store::Store + core::marker::Send + core::marker::Sync)
+pub fn (dyn datumesh_store::Store + core::marker::Send + core::marker::Sync)::alias(&self, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
+pub fn (dyn datumesh_store::Store + core::marker::Send + core::marker::Sync)::bound(&self, &str, core::option::Option<&str>) -> core::result::Result<core::option::Option<datumesh_core::descriptor::FunctionDescriptor>, alloc::string::String>
+pub fn (dyn datumesh_store::Store + core::marker::Send + core::marker::Sync)::pinned(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::descriptor::FunctionDescriptor>, alloc::string::String>
+impl datumesh_formula::call::CallEnv for datumesh_store::pg::PgStore
+pub fn datumesh_store::pg::PgStore::alias(&self, &str) -> core::result::Result<core::option::Option<alloc::string::String>, alloc::string::String>
+pub fn datumesh_store::pg::PgStore::bound(&self, &str, core::option::Option<&str>) -> core::result::Result<core::option::Option<datumesh_core::descriptor::FunctionDescriptor>, alloc::string::String>
+pub fn datumesh_store::pg::PgStore::pinned(&self, &str) -> core::result::Result<core::option::Option<datumesh_core::descriptor::FunctionDescriptor>, alloc::string::String>
+pub fn datumesh_formula::call::parse_call(&str, &dyn datumesh_formula::call::CallEnv) -> core::result::Result<datumesh_formula::call::Call, datumesh_formula::call::CallError>
+pub fn datumesh_formula::eval::World::blob(&mut self, &str) -> core::result::Result<core::option::Option<alloc::vec::Vec<u8>>, datumesh_formula::eval::Stop<Self::Error>>
+pub fn datumesh_formula::memory::MemoryWorld::blob(&mut self, &str) -> core::result::Result<core::option::Option<alloc::vec::Vec<u8>>, datumesh_formula::eval::Stop<Self::Error>>
+pub fn datumesh_formula::memory::MemoryWorld::blob(&mut self, &str) -> core::result::Result<core::option::Option<alloc::vec::Vec<u8>>, datumesh_formula::eval::Stop<Self::Error>>
+pub fn datumesh_formula::memory::MemoryWorld::add_blob(&mut self, &str, alloc::vec::Vec<u8>)
+pub datumesh_formula::Code::BlobUnavailable

```
