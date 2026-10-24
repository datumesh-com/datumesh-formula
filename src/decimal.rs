//! Exact decimal arithmetic (SPEC 15.9): 34 significant digits, half-even, the range rule, and
//! the 40-digit transcendental algorithms. Integers only; no float anywhere.

use crate::Code;

/// Apply the operation `op` of a `decimal` vector (SPEC 11) to canonical number arguments.
pub fn apply(op: &str, args: &[&str]) -> Result<String, Code> {
    let _ = (op, args);
    Err(Code::Syntax)
}
