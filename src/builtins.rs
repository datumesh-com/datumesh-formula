//! The built-ins of SPEC 15.7: parameters for binding (15.5), and enumeration lists.

/// How a built-in takes its arguments.
pub(crate) enum Params {
    /// Required parameters (positional) and optional ones (labelled when given).
    Fixed(&'static [&'static str], &'static [&'static str]),
    /// One or more (or two or more) unlabelled arguments.
    Variadic(usize),
    /// `asof()` or `asof(cell, T)`.
    Asof,
}

/// The aggregates (one slice or one or more scalars of one type).
pub(crate) const AGGREGATES: &[&str] =
    &["sum", "count", "mean", "min", "max", "first", "last", "median", "stdev", "stdev_p"];

/// The lookups (15.13).
pub(crate) const LOOKUPS: &[&str] = &["at", "latest", "last_before", "first_after", "nearest", "within", "interpolate"];

/// Interpolation methods (`spline` is `E_FORMULA_UNSUPPORTED`).
pub(crate) const METHODS: &[&str] = &["linear", "step"];

/// Weekend rules.
pub(crate) const WEEKENDS: &[&str] = &["sat-sun", "fri-sat", "sun", "none"];

/// Roll conventions.
pub(crate) const ROLLS: &[&str] = &["following", "modified_following", "preceding"];

pub(crate) fn params(name: &str) -> Option<Params> {
    use Params::*;
    Some(match name {
        "sum" | "count" | "mean" | "min" | "max" | "first" | "last" | "median" | "stdev" | "stdev_p" | "concat" => {
            Variadic(1)
        }
        "and" | "or" => Variadic(2),
        "asof" => Asof,
        "at" => Fixed(&["s", "key"], &["along"]),
        "latest" => Fixed(&["s"], &["bound", "along"]),
        "last_before" | "first_after" | "nearest" => Fixed(&["s", "x"], &["bound", "along"]),
        "within" => Fixed(&["s", "x", "bound"], &["along"]),
        "interpolate" => Fixed(&["s", "x"], &["method", "bound", "along"]),
        "map" | "filter" => Fixed(&["s", "f"], &[]),
        "scan" => Fixed(&["s", "aggregate"], &["along"]),
        "window" => Fixed(&["s", "n", "aggregate"], &["along"]),
        "otherwise" => Fixed(&["x", "fallback"], &[]),
        "versions" => Fixed(&["cell", "from", "to"], &[]),
        "abs" | "floor" | "ceiling" | "sqrt" | "exp" | "ln" | "sin" | "cos" | "text" => Fixed(&["x"], &[]),
        "round" | "round_up" | "round_down" => Fixed(&["x"], &["digits"]),
        "mod" => Fixed(&["x", "divisor"], &[]),
        "atan2" => Fixed(&["y", "x"], &[]),
        "pi" => Fixed(&[], &[]),
        "haversine" => Fixed(&["lat1", "lon1", "lat2", "lon2"], &[]),
        "convert" => Fixed(&["x", "unit"], &[]),
        "if" => Fixed(&["condition", "then", "else"], &[]),
        "date" => Fixed(&["year", "month", "day"], &[]),
        "year" | "month" | "day" | "weekday" | "end_of_month" => Fixed(&["d"], &[]),
        "add_days" => Fixed(&["date", "n"], &["calendar", "weekend"]),
        "add_months" => Fixed(&["date", "n"], &["calendar", "roll", "weekend"]),
        "business_days" => Fixed(&["from", "to"], &["calendar", "weekend"]),
        "year_fraction" => Fixed(&["from", "to", "convention"], &[]),
        "is_holiday" => Fixed(&["date", "calendar"], &[]),
        "date_of" => Fixed(&["instant"], &[]),
        "instant" => Fixed(&["date"], &[]),
        "len" | "upper" | "lower" | "trim" => Fixed(&["text"], &[]),
        "substr" => Fixed(&["text", "start", "length"], &[]),
        "find" => Fixed(&["needle", "text"], &["start"]),
        "fixed" => Fixed(&["x", "digits"], &[]),
        _ => return None,
    })
}
