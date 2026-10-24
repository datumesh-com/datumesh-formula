//! The formula codes of SPEC 15.11.

use core::fmt;

macro_rules! codes {
    ($($(#[$m:meta])* $v:ident => $s:literal,)*) => {
        /// A formula code (SPEC 15.11): a define-time error, an evaluation fault or a mesh failure.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[non_exhaustive]
        pub enum Code {
            $($(#[$m])* #[doc = concat!("`", $s, "`")] $v,)*
        }

        impl Code {
            /// The code as written in the specification, e.g. `"E_FORMULA_SYNTAX"`.
            pub const fn as_str(self) -> &'static str {
                match self { $(Code::$v => $s,)* }
            }

            /// The code named by `s`, if it is one of SPEC 15.11.
            pub fn from_name(s: &str) -> Option<Code> {
                match s { $($s => Some(Code::$v),)* _ => None }
            }

            /// Every code, in the order of SPEC 15.11.
            pub const ALL: &'static [Code] = &[$(Code::$v,)*];
        }
    };
}

codes! {
    Syntax => "E_FORMULA_SYNTAX",
    Number => "E_FORMULA_NUMBER",
    String => "E_FORMULA_STRING",
    Date => "E_FORMULA_DATE",
    Instant => "E_FORMULA_INSTANT",
    Unit => "E_FORMULA_UNIT",
    Name => "E_FORMULA_NAME",
    Reserved => "E_FORMULA_RESERVED",
    Forbidden => "E_FORMULA_FORBIDDEN",
    DynamicName => "E_FORMULA_DYNAMIC_NAME",
    Ambiguous => "E_FORMULA_AMBIGUOUS",
    Chain => "E_FORMULA_CHAIN",
    Placement => "E_FORMULA_PLACEMENT",
    Limit => "E_FORMULA_LIMIT",
    UnknownFunction => "E_FORMULA_UNKNOWN_FUNCTION",
    Alias => "E_FORMULA_ALIAS",
    Unresolved => "E_FORMULA_UNRESOLVED",
    UnknownName => "E_FORMULA_UNKNOWN_NAME",
    Shadow => "E_FORMULA_SHADOW",
    Arity => "E_FORMULA_ARITY",
    ArgUnknown => "E_FORMULA_ARG_UNKNOWN",
    ArgMissing => "E_FORMULA_ARG_MISSING",
    ArgDuplicate => "E_FORMULA_ARG_DUPLICATE",
    ArgOrder => "E_FORMULA_ARG_ORDER",
    Positional => "E_FORMULA_POSITIONAL",
    Order => "E_FORMULA_ORDER",
    Type => "E_FORMULA_TYPE",
    SliceValue => "E_FORMULA_SLICE_VALUE",
    UnitMismatch => "E_FORMULA_UNIT_MISMATCH",
    UnitOffset => "E_FORMULA_UNIT_OFFSET",
    UnitUnknown => "E_FORMULA_UNIT_UNKNOWN",
    Exponent => "E_FORMULA_EXPONENT",
    ReturnType => "E_FORMULA_RETURN_TYPE",
    ReturnUnit => "E_FORMULA_RETURN_UNIT",
    Unbounded => "E_FORMULA_UNBOUNDED",
    SliceBound => "E_FORMULA_SLICE_BOUND",
    Metric => "E_FORMULA_METRIC",
    Along => "E_FORMULA_ALONG",
    Join => "E_FORMULA_JOIN",
    LambdaParam => "E_FORMULA_LAMBDA_PARAM",
    LambdaRead => "E_FORMULA_LAMBDA_READ",
    Enum => "E_FORMULA_ENUM",
    TxArg => "E_FORMULA_TX_ARG",
    Unsupported => "E_FORMULA_UNSUPPORTED",
    Inputs => "E_FORMULA_INPUTS",
    NotCanonical => "E_FORMULA_NOT_CANONICAL",
    DivZero => "E_FORMULA_DIV_ZERO",
    Domain => "E_FORMULA_DOMAIN",
    Range => "E_FORMULA_RANGE",
    Empty => "E_FORMULA_EMPTY",
    Absent => "E_FORMULA_ABSENT",
    Future => "E_FORMULA_FUTURE",
    NotCovered => "E_FORMULA_NOT_COVERED",
    Budget => "E_FORMULA_BUDGET",
    Cycle => "E_FORMULA_CYCLE",
    Depth => "E_FORMULA_DEPTH",
}

impl Code {
    /// Whether this is an evaluation fault a mesh reports in a `not_covered` envelope's `fault`
    /// member (SPEC 5.5, 15.12): division by zero, domain, range, empty, absent, future.
    pub const fn is_fault(self) -> bool {
        matches!(self, Code::DivZero | Code::Domain | Code::Range | Code::Empty | Code::Absent | Code::Future)
    }

    /// Whether `otherwise` catches it (SPEC 15.12): `E_FORMULA_EMPTY` and `E_FORMULA_ABSENT`.
    pub const fn is_catchable(self) -> bool {
        matches!(self, Code::Empty | Code::Absent)
    }

    /// Whether it is the mesh's failure rather than the cell's (SPEC 15.12): budget, cycle,
    /// depth. A mesh answers these with an `error` envelope and `E_EVAL_UNAVAILABLE`.
    pub const fn is_mesh_failure(self) -> bool {
        matches!(self, Code::Budget | Code::Cycle | Code::Depth)
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A formula error: its code, the token for `E_FORMULA_FORBIDDEN`, and a human detail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// The code.
    pub code: Code,
    /// For `E_FORMULA_FORBIDDEN`, the forbidden name as written.
    pub token: Option<String>,
    /// A human-readable explanation; not normative.
    pub detail: String,
}

impl Error {
    /// An error with a code and a detail.
    pub fn new(code: Code, detail: impl Into<String>) -> Error {
        Error { code, token: None, detail: detail.into() }
    }
}

impl From<Code> for Error {
    fn from(code: Code) -> Error {
        Error { code, token: None, detail: String::new() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.detail.is_empty() {
            write!(f, "{}", self.code)
        } else {
            write!(f, "{}: {}", self.code, self.detail)
        }
    }
}

impl std::error::Error for Error {}
