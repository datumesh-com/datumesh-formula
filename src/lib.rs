//! The Datumesh formula dialect (SPEC section 15, spec-0.4.0).
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod bigint;
pub mod decimal;
mod error;

pub use error::{Code, Error};
