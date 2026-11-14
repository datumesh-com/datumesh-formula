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
    /// Since spec-0.5.0 (16.3): a class read used as a value, a method the class does not have
    /// or whose `<class>/<method>` is not bound to the class's method hash, `super` without a
    /// parent, an extension method that does not take the class keys first.
    Method => "E_FORMULA_METHOD",
    /// Since spec-0.5.0 (16.6): at define, a dispatched method whose signature (keys taken
    /// away) differs between the classes of the set; at evaluation, a method no longer bound.
    Dispatch => "E_FORMULA_DISPATCH",
    /// Since spec-0.5.0 (16.4): at define, a literal key not canonical under its
    /// normalisation or a malformed literal object key; at evaluation, the fault of a computed
    /// string argument not canonical under its argument's `key_normalisation`.
    Key => "E_FORMULA_KEY",
    DivZero => "E_FORMULA_DIV_ZERO",
    Domain => "E_FORMULA_DOMAIN",
    Range => "E_FORMULA_RANGE",
    Empty => "E_FORMULA_EMPTY",
    Absent => "E_FORMULA_ABSENT",
    Future => "E_FORMULA_FUTURE",
    /// Fault, since spec-0.5.0 (16.6): a value dispatched on that is not the key of an object of
    /// a class in the dispatch set.
    DispatchKey => "E_FORMULA_DISPATCH_KEY",
    NotCovered => "E_FORMULA_NOT_COVERED",
    Budget => "E_FORMULA_BUDGET",
    Cycle => "E_FORMULA_CYCLE",
    Depth => "E_FORMULA_DEPTH",
    /// `text_of` cannot obtain the blob's content (spec-0.4.2): the evaluator's failure.
    BlobUnavailable => "E_FORMULA_BLOB_UNAVAILABLE",
    /// Since spec-0.5.0 (10.2, 16.3): a define of a method whose body uses `super` when no class
    /// name is bound to the parent's hash in the definer's environment. A class code, not a formula
    /// one, returned by define.
    ClassParent => "E_CLASS_PARENT",
}

impl Code {
    /// Whether this is an evaluation fault a mesh reports in a `not_covered` envelope's `fault`
    /// member (SPEC 5.5, 15.12): division by zero, domain, range, empty, absent, future, and since
    /// spec-0.5.0 the key and dispatch-key faults.
    pub const fn is_fault(self) -> bool {
        matches!(
            self,
            Code::DivZero
                | Code::Domain
                | Code::Range
                | Code::Empty
                | Code::Absent
                | Code::Future
                | Code::Key
                | Code::DispatchKey
        )
    }

    /// Whether `otherwise` catches it (SPEC 15.12): `E_FORMULA_EMPTY` and `E_FORMULA_ABSENT`.
    pub const fn is_catchable(self) -> bool {
        matches!(self, Code::Empty | Code::Absent)
    }

    /// Whether it is the mesh's failure rather than the cell's (SPEC 15.12): budget, cycle,
    /// depth, and since spec-0.5.0 the other evaluation codes that are neither faults nor
    /// `E_FORMULA_NOT_COVERED`: an unresolved name (a binding absent as of the read, 15.8), a
    /// dispatch whose method is bound to another hash (16.6), and an unsupported read. A mesh
    /// answers these with an `error` envelope and `E_EVAL_UNAVAILABLE`.
    pub const fn is_mesh_failure(self) -> bool {
        matches!(
            self,
            Code::Budget
                | Code::Cycle
                | Code::Depth
                | Code::BlobUnavailable
                | Code::Unresolved
                | Code::Dispatch
                | Code::Unsupported
        )
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

/// Why a formula function could not be defined, or a formula descriptor fails its checks
/// (15.6): the descriptor's canonical form of section 7 (a core code such as `E_SCHEMA` or
/// `E_CANON_SET`), a formula code, or the callee source's own failure.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DefineError {
    /// The descriptor is not in the canonical form of section 7.
    Descriptor(datumesh_core::Error),
    /// A formula code of 15.11.
    Formula(Error),
    /// The callees could not be read (a store failure).
    Callees(String),
}

impl DefineError {
    /// The code: the core's (section 7) or the formula's; for a callee source's failure, which
    /// has no code, the text `callees unavailable`.
    pub fn code(&self) -> &str {
        match self {
            DefineError::Descriptor(e) => e.code_str(),
            DefineError::Formula(e) => e.code.as_str(),
            DefineError::Callees(_) => "callees unavailable",
        }
    }

    /// The formula code, when it is one.
    pub fn formula_code(&self) -> Option<Code> {
        match self {
            DefineError::Formula(e) => Some(e.code),
            _ => None,
        }
    }
}

impl From<Error> for DefineError {
    fn from(e: Error) -> DefineError {
        DefineError::Formula(e)
    }
}

impl fmt::Display for DefineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DefineError::Descriptor(e) => write!(f, "{}: {}", e.code_str(), e.detail()),
            DefineError::Formula(e) => write!(f, "{e}"),
            DefineError::Callees(m) => write!(f, "the callees could not be read: {m}"),
        }
    }
}

impl std::error::Error for DefineError {}
