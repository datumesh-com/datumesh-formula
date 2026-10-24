//! The units of `units.md` (spec-0.4.0) and the unit algebra of SPEC 15.10.
//!
//! The literal lists are copied from the machine-readable blocks of `units.md`.

use std::collections::BTreeMap;

use crate::decimal::Decimal;

/// Currency codes (`units.md` 2.1), withdrawn codes included.
pub const CURRENCIES: &[&str] = &[
    "AED", "AFN", "ALL", "AMD", "AOA", "ARS", "AUD", "AWG", "AZN", "BAM", "BBD", "BDT", "BHD", "BIF", "BMD", "BND",
    "BOB", "BOV", "BRL", "BSD", "BTN", "BWP", "BYN", "BZD", "CAD", "CDF", "CHE", "CHF", "CHW", "CLF", "CLP", "CNY",
    "COP", "COU", "CRC", "CUP", "CVE", "CZK", "DJF", "DKK", "DOP", "DZD", "EGP", "ERN", "ETB", "EUR", "FJD", "FKP",
    "GBP", "GEL", "GHS", "GIP", "GMD", "GNF", "GTQ", "GYD", "HKD", "HNL", "HTG", "HUF", "IDR", "ILS", "INR", "IQD",
    "IRR", "ISK", "JMD", "JOD", "JPY", "KES", "KGS", "KHR", "KMF", "KPW", "KRW", "KWD", "KYD", "KZT", "LAK", "LBP",
    "LKR", "LRD", "LSL", "LYD", "MAD", "MDL", "MGA", "MKD", "MMK", "MNT", "MOP", "MRU", "MUR", "MVR", "MWK", "MXN",
    "MXV", "MYR", "MZN", "NAD", "NGN", "NIO", "NOK", "NPR", "NZD", "OMR", "PAB", "PEN", "PGK", "PHP", "PKR", "PLN",
    "PYG", "QAR", "RON", "RSD", "RUB", "RWF", "SAR", "SBD", "SCR", "SDG", "SEK", "SGD", "SHP", "SLE", "SOS", "SRD",
    "SSP", "STN", "SVC", "SYP", "SZL", "THB", "TJS", "TMT", "TND", "TOP", "TRY", "TTD", "TWD", "TZS", "UAH", "UGX",
    "USD", "USN", "UYI", "UYU", "UYW", "UZS", "VED", "VES", "VND", "VUV", "WST", "XAD", "XAF", "XAG", "XAU", "XBA",
    "XBB", "XBC", "XBD", "XCD", "XCG", "XDR", "XOF", "XPD", "XPF", "XPT", "XSU", "XTS", "XUA", "YER", "ZAR", "ZMW",
    "ZWG", "BGN", "CYP", "EEK", "GRD", "HRK", "LTL", "LVL", "MTL", "ROL", "SIT", "SKK", "TRL",
];

/// Physical units (`units.md` 2.2).
pub const PHYSICAL: &[&str] = &[
    "m", "kg", "s", "A", "K", "mol", "cd", "degC", "m/s", "W", "kWh", "mm", "cm", "km/h", "hPa", "W/m2", "min", "h",
    "d", "km", "deg",
];

/// Ratios and counts (`units.md` 2.3).
pub const RATIOS: &[&str] = &["percent", "permille", "bp", "1", "count"];

/// Whether `s` is a currency code of `units.md` 2.1.
pub fn is_currency(s: &str) -> bool {
    CURRENCIES.contains(&s)
}

/// Whether `s` is a literal unit of `units.md` (section 2), index units included.
pub fn is_unit(s: &str) -> bool {
    CURRENCIES.contains(&s) || PHYSICAL.contains(&s) || RATIOS.contains(&s) || is_index_unit(s)
}

/// Whether `s` is a valid index unit (`units.md` 2.4): `index:<period>=<value>`, a real year,
/// month or day from 0001, a canonical value greater than zero, at most 64 characters.
pub fn is_index_unit(s: &str) -> bool {
    if s.len() > 64 {
        return false;
    }
    let Some(rest) = s.strip_prefix("index:") else { return false };
    let Some((period, value)) = rest.split_once('=') else { return false };
    let ok_period = match period.len() {
        4 => digits(period) && &period[..4] != "0000",
        7 => {
            digits(&period[..4]) && &period[4..5] == "-" && digits(&period[5..7]) && &period[..4] != "0000" && {
                let m: u32 = period[5..7].parse().unwrap_or(0);
                (1..=12).contains(&m)
            }
        }
        10 => crate::calendar::Date::parse(period).is_some(),
        _ => false,
    };
    ok_period && Decimal::parse(value).is_some_and(|v| !v.is_negative() && !v.is_zero())
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Every literal unit that can follow a number as a suffix (all but `1`), longest first.
pub(crate) fn suffix_units() -> &'static [&'static str] {
    use std::sync::OnceLock;
    static U: OnceLock<Vec<&'static str>> = OnceLock::new();
    U.get_or_init(|| {
        let mut v: Vec<&'static str> =
            CURRENCIES.iter().chain(PHYSICAL).chain(RATIOS).copied().filter(|u| *u != "1").collect();
        v.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
        v
    })
}

/// A unit of the checker (15.10): a map from atoms to non-zero integer exponents. An atom is a
/// literal unit (indivisible: `kWh`, `m/s`) or `arg:<name>`. The empty map is `1`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Unit(pub(crate) BTreeMap<String, i64>);

impl Unit {
    /// The dimensionless unit `1`.
    pub fn one() -> Unit {
        Unit(BTreeMap::new())
    }

    /// One atom to the first power (`"1"` is the empty unit).
    pub fn atom(a: &str) -> Unit {
        let mut m = BTreeMap::new();
        if a != "1" {
            m.insert(a.to_string(), 1);
        }
        Unit(m)
    }

    /// Whether this is `1`.
    pub fn is_one(&self) -> bool {
        self.0.is_empty()
    }

    /// The product (`sign` 1) or quotient (`sign` -1).
    pub fn mul(&self, other: &Unit, sign: i64) -> Unit {
        let mut m = self.0.clone();
        for (a, e) in &other.0 {
            let v = m.get(a).copied().unwrap_or(0) + sign * e;
            if v == 0 {
                m.remove(a);
            } else {
                m.insert(a.clone(), v);
            }
        }
        Unit(m)
    }

    /// Every exponent multiplied by `n`.
    pub fn pow(&self, n: i64) -> Unit {
        if n == 0 {
            return Unit::one();
        }
        Unit(self.0.iter().map(|(a, e)| (a.clone(), e * n)).collect())
    }

    /// Every exponent halved, if all are even.
    pub fn sqrt(&self) -> Option<Unit> {
        if self.0.values().any(|e| e % 2 != 0) {
            return None;
        }
        Some(Unit(self.0.iter().map(|(a, e)| (a.clone(), e / 2)).collect()))
    }

    /// The single literal atom to the first power, if the unit is exactly that.
    pub fn single(&self) -> Option<&str> {
        if self.0.len() == 1 {
            let (a, e) = self.0.iter().next()?;
            if *e == 1 {
                return Some(a);
            }
        }
        None
    }

    /// The text of the vectors' `returns.unit`: atoms in ascending order, separated by a space,
    /// each with `^<exponent>` unless it is 1; `1` for the empty unit.
    pub fn text(&self) -> String {
        if self.0.is_empty() {
            return "1".to_string();
        }
        let parts: Vec<String> =
            self.0.iter().map(|(a, e)| if *e == 1 { a.clone() } else { format!("{a}^{e}") }).collect();
        parts.join(" ")
    }
}
