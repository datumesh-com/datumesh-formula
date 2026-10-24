//! Exact decimal arithmetic (SPEC 15.9): every operation computes its exact result, rounds it
//! once to 34 significant digits half-even and applies the range rule; the transcendental
//! functions follow the named 40-digit algorithms of 15.9.3. Integers only; no float anywhere.

use core::cmp::Ordering;
use core::fmt;

use crate::bigint::{pow10, Big};
use crate::Code;

/// The precision of every result: 34 significant digits (IEEE 754 decimal128).
pub const PRECISION: u64 = 34;
/// The working precision of the transcendental algorithms: 40 significant digits.
pub const WORKING: u64 = 40;
/// The longest canonical number (SPEC 3.1, 4.5): 128 characters, sign and point included.
pub const MAX_CHARS: usize = 128;

/// An exact decimal number: `(-1)^neg * coef * 10^exp`, normalised (no trailing zeros in
/// `coef`; zero is `0 * 10^0`, never negative).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Decimal {
    neg: bool,
    coef: Big,
    exp: i64,
}

/// A rounding direction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    /// To nearest, ties to even (the rule of 15.9).
    HalfEven,
    /// To nearest, ties away from zero (Excel's ROUND).
    HalfAway,
    /// Toward zero (truncation; Excel's ROUNDDOWN).
    Down,
    /// Away from zero (Excel's ROUNDUP).
    Up,
    /// Toward minus infinity.
    Floor,
    /// Toward plus infinity.
    Ceiling,
}

impl Decimal {
    /// Zero.
    pub fn zero() -> Decimal {
        Decimal { neg: false, coef: Big::zero(), exp: 0 }
    }

    /// An integer.
    pub fn from_i64(v: i64) -> Decimal {
        Decimal::make(v < 0, Big::from_u64(v.unsigned_abs()), 0)
    }

    /// An integer.
    pub fn from_i128(v: i128) -> Decimal {
        Decimal::make(v < 0, Big::from_u128(v.unsigned_abs()), 0)
    }

    fn make(neg: bool, mut coef: Big, mut exp: i64) -> Decimal {
        if coef.is_zero() {
            return Decimal::zero();
        }
        // Strip trailing zeros.
        loop {
            let mut q = coef.clone();
            let r = q.divrem_small_in_place(10);
            if r != 0 {
                break;
            }
            coef = q;
            exp += 1;
        }
        Decimal { neg, coef, exp }
    }

    /// Parse the canonical lexical form of SPEC 3.1 (`-?(0|[1-9][0-9]*)(\.[0-9]*[1-9])?`, not
    /// `-0`, at most 128 characters). Anything else is `None`.
    pub fn parse(s: &str) -> Option<Decimal> {
        let b = s.as_bytes();
        if b.is_empty() || b.len() > MAX_CHARS {
            return None;
        }
        let (neg, rest) = if b[0] == b'-' { (true, &b[1..]) } else { (false, b) };
        let (int, frac) = match rest.iter().position(|&c| c == b'.') {
            Some(p) => (&rest[..p], Some(&rest[p + 1..])),
            None => (rest, None),
        };
        if int.is_empty() || !int.iter().all(u8::is_ascii_digit) || (int.len() > 1 && int[0] == b'0') {
            return None;
        }
        let mut digits = int.to_vec();
        let mut exp = 0i64;
        if let Some(f) = frac {
            if f.is_empty() || !f.iter().all(u8::is_ascii_digit) || f[f.len() - 1] == b'0' {
                return None;
            }
            digits.extend_from_slice(f);
            exp = -(f.len() as i64);
        }
        let d = Decimal::make(neg, Big::from_digits(&digits), exp);
        if neg && d.is_zero() {
            return None; // "-0"
        }
        Some(d)
    }

    /// Whether the value is zero.
    pub fn is_zero(&self) -> bool {
        self.coef.is_zero()
    }

    /// Whether the value is negative.
    pub fn is_negative(&self) -> bool {
        self.neg
    }

    /// Whether the value is an integer.
    pub fn is_integer(&self) -> bool {
        self.exp >= 0
    }

    /// The value as an `i64`, if it is an integer that fits.
    pub fn to_i64(&self) -> Option<i64> {
        if self.exp < 0 || self.exp > 18 {
            return if self.is_zero() { Some(0) } else { None };
        }
        let v = self.coef.to_u64()?.checked_mul(10u64.pow(self.exp as u32))?;
        let v = i64::try_from(v).ok()?;
        Some(if self.neg { -v } else { v })
    }

    /// The negation (exact).
    pub fn neg(&self) -> Decimal {
        if self.is_zero() {
            return Decimal::zero();
        }
        Decimal { neg: !self.neg, coef: self.coef.clone(), exp: self.exp }
    }

    /// The absolute value (exact).
    pub fn abs(&self) -> Decimal {
        Decimal { neg: false, coef: self.coef.clone(), exp: self.exp }
    }

    /// The number of significant digits of the coefficient.
    fn digits(&self) -> u64 {
        self.coef.digit_count()
    }

    /// The exponent of the leading digit: `floor(log10(abs(x)))` for non-zero `x`.
    pub(crate) fn adjusted(&self) -> i64 {
        self.exp + self.digits() as i64 - 1
    }

    /// The canonical lexical form (SPEC 3.1).
    pub fn to_canonical(&self) -> String {
        let digits = self.coef.to_digits();
        let mut s = String::new();
        if self.neg {
            s.push('-');
        }
        if self.exp >= 0 {
            s.push_str(&digits);
            if !self.is_zero() {
                for _ in 0..self.exp {
                    s.push('0');
                }
            }
        } else {
            let point = digits.len() as i64 + self.exp; // digits before the point
            if point > 0 {
                s.push_str(&digits[..point as usize]);
                s.push('.');
                s.push_str(&digits[point as usize..]);
            } else {
                s.push_str("0.");
                for _ in 0..(-point) {
                    s.push('0');
                }
                s.push_str(&digits);
            }
        }
        s
    }

    /// The length of the canonical form, without building it.
    pub fn canonical_len(&self) -> u64 {
        let d = self.digits();
        let sign = u64::from(self.neg);
        if self.is_zero() {
            1
        } else if self.exp >= 0 {
            sign + d + self.exp as u64
        } else {
            let point = d as i64 + self.exp;
            if point > 0 {
                sign + d + 1
            } else {
                sign + 2 + (-point) as u64 + d
            }
        }
    }

    /// The range rule (15.9.1): the canonical form has at most 128 characters.
    pub fn check_range(self) -> Result<Decimal, Code> {
        if self.canonical_len() as usize > MAX_CHARS {
            Err(Code::Range)
        } else {
            Ok(self)
        }
    }

    /// Round to a multiple of `10^e`.
    pub(crate) fn round_to_exp(&self, e: i64, mode: Mode) -> Decimal {
        if self.exp >= e {
            return self.clone();
        }
        let k = (e - self.exp) as u64;
        finish(self.neg, &self.coef, self.exp, false, k, mode)
    }

    /// Round to `p` significant digits.
    pub(crate) fn round_sig(&self, p: u64, mode: Mode) -> Decimal {
        let d = self.digits();
        if d <= p {
            return self.clone();
        }
        finish(self.neg, &self.coef, self.exp, false, d - p, mode)
    }

    /// Round to an integer, half-even.
    pub(crate) fn round_integer_half_even(&self) -> Decimal {
        self.round_to_exp(0, Mode::HalfEven)
    }

    /// The floor (exact).
    pub fn floor(&self) -> Decimal {
        self.round_to_exp(0, Mode::Floor)
    }

    /// The ceiling (exact).
    pub fn ceiling(&self) -> Decimal {
        self.round_to_exp(0, Mode::Ceiling)
    }

    /// `self * 10^n`, exact.
    pub(crate) fn scale10(&self, n: i64) -> Decimal {
        if self.is_zero() {
            return Decimal::zero();
        }
        Decimal { neg: self.neg, coef: self.coef.clone(), exp: self.exp + n }
    }
}

/// Drop the last `k` digits of `coef * 10^exp` (k >= 1), rounding by `mode`; `sticky` says
/// that non-zero digits below the coefficient were already dropped.
fn finish(neg: bool, coef: &Big, exp: i64, sticky: bool, k: u64, mode: Mode) -> Decimal {
    let p = pow10(k);
    let (mut q, r) = coef.divrem(&p);
    let twice = r.mul_small(2);
    let half_cmp = match twice.cmp(&p) {
        Ordering::Equal if sticky => Ordering::Greater,
        o => o,
    };
    let inexact = !r.is_zero() || sticky;
    let up = match mode {
        Mode::HalfEven => half_cmp == Ordering::Greater || (half_cmp == Ordering::Equal && !q.is_even()),
        Mode::HalfAway => half_cmp != Ordering::Less,
        Mode::Down => false,
        Mode::Up => inexact,
        Mode::Floor => inexact && neg,
        Mode::Ceiling => inexact && !neg,
    };
    if up {
        q = q.add(&Big::from_u64(1));
    }
    Decimal::make(neg, q, exp + k as i64)
}

impl Ord for Decimal {
    fn cmp(&self, other: &Decimal) -> Ordering {
        match (self.neg, other.neg) {
            (false, true) => return Ordering::Greater,
            (true, false) => return Ordering::Less,
            _ => {}
        }
        let mag = cmp_abs(self, other);
        if self.neg {
            mag.reverse()
        } else {
            mag
        }
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Decimal) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn cmp_abs(a: &Decimal, b: &Decimal) -> Ordering {
    match (a.is_zero(), b.is_zero()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Less,
        (false, true) => return Ordering::Greater,
        _ => {}
    }
    match a.adjusted().cmp(&b.adjusted()) {
        Ordering::Equal => {}
        o => return o,
    }
    let e = a.exp.min(b.exp);
    let ca = a.coef.mul(&pow10((a.exp - e) as u64));
    let cb = b.coef.mul(&pow10((b.exp - e) as u64));
    ca.cmp(&cb)
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_canonical())
    }
}

// ---------------------------------------------------------------------------------------------
// Exact operations.

/// The exact sum.
pub(crate) fn add_exact(a: &Decimal, b: &Decimal) -> Decimal {
    if a.is_zero() {
        return b.clone();
    }
    if b.is_zero() {
        return a.clone();
    }
    let e = a.exp.min(b.exp);
    let ca = a.coef.mul(&pow10((a.exp - e) as u64));
    let cb = b.coef.mul(&pow10((b.exp - e) as u64));
    if a.neg == b.neg {
        Decimal::make(a.neg, ca.add(&cb), e)
    } else {
        match ca.cmp(&cb) {
            Ordering::Equal => Decimal::zero(),
            Ordering::Greater => Decimal::make(a.neg, ca.sub(&cb), e),
            Ordering::Less => Decimal::make(b.neg, cb.sub(&ca), e),
        }
    }
}

/// The exact difference.
pub(crate) fn sub_exact(a: &Decimal, b: &Decimal) -> Decimal {
    add_exact(a, &b.neg())
}

/// The exact product.
pub(crate) fn mul_exact(a: &Decimal, b: &Decimal) -> Decimal {
    Decimal::make(a.neg != b.neg, a.coef.mul(&b.coef), a.exp + b.exp)
}

/// `a / b` rounded once to `p` significant digits by `mode`; `b` non-zero.
pub(crate) fn div_round(a: &Decimal, b: &Decimal, p: u64, mode: Mode) -> Decimal {
    debug_assert!(!b.is_zero());
    if a.is_zero() {
        return Decimal::zero();
    }
    let da = a.digits() as i64;
    let db = b.digits() as i64;
    // Scale the numerator so that the integer quotient has at least p + 2 digits.
    let s = (p as i64 + 2 + db - da).max(0);
    let n = a.coef.mul(&pow10(s as u64));
    let (q, r) = n.divrem(&b.coef);
    let neg = a.neg != b.neg;
    let exp = a.exp - b.exp - s;
    let d = q.digit_count();
    if d <= p {
        // Cannot happen with the scaling above unless the quotient is exact.
        debug_assert!(r.is_zero());
        return Decimal::make(neg, q, exp);
    }
    finish(neg, &q, exp, !r.is_zero(), d - p, mode)
}

/// Round half-even to `p` significant digits.
pub(crate) fn rnd(x: Decimal, p: u64) -> Decimal {
    x.round_sig(p, Mode::HalfEven)
}

/// `a + b` rounded half-even to `p` digits.
pub(crate) fn add_p(a: &Decimal, b: &Decimal, p: u64) -> Decimal {
    rnd(add_exact(a, b), p)
}

/// `a - b` rounded half-even to `p` digits.
pub(crate) fn sub_p(a: &Decimal, b: &Decimal, p: u64) -> Decimal {
    rnd(sub_exact(a, b), p)
}

/// `a * b` rounded half-even to `p` digits.
pub(crate) fn mul_p(a: &Decimal, b: &Decimal, p: u64) -> Decimal {
    rnd(mul_exact(a, b), p)
}

/// `a / b` rounded half-even to `p` digits; `b` non-zero.
pub(crate) fn div_p(a: &Decimal, b: &Decimal, p: u64) -> Decimal {
    div_round(a, b, p, Mode::HalfEven)
}

fn int(v: i64) -> Decimal {
    Decimal::from_i64(v)
}

fn result(x: Decimal) -> Result<Decimal, Code> {
    rnd(x, PRECISION).check_range()
}

// ---------------------------------------------------------------------------------------------
// The operations of 15.9.2.

/// `a + b`.
pub fn add(a: &Decimal, b: &Decimal) -> Result<Decimal, Code> {
    result(add_exact(a, b))
}

/// `a - b`.
pub fn sub(a: &Decimal, b: &Decimal) -> Result<Decimal, Code> {
    result(sub_exact(a, b))
}

/// `a * b`.
pub fn mul(a: &Decimal, b: &Decimal) -> Result<Decimal, Code> {
    result(mul_exact(a, b))
}

/// `a / b`; a zero divisor is `E_FORMULA_DIV_ZERO`.
pub fn div(a: &Decimal, b: &Decimal) -> Result<Decimal, Code> {
    if b.is_zero() {
        return Err(Code::DivZero);
    }
    div_round(a, b, PRECISION, Mode::HalfEven).check_range()
}

/// `-a`, rounded and range-checked like every operation.
pub fn negate(a: &Decimal) -> Result<Decimal, Code> {
    result(a.neg())
}

/// `x ^ n` for an integer `n` with `abs(n) <= 1024`: the exact power rounded once; for `n < 0`,
/// one divided by the exact power, rounded once.
pub fn power_int(x: &Decimal, n: &Decimal) -> Result<Decimal, Code> {
    let n = match n.to_i64() {
        Some(n) if n.unsigned_abs() <= 1024 => n,
        _ => return Err(Code::Domain),
    };
    if n == 0 {
        return Ok(int(1));
    }
    if x.is_zero() {
        return if n < 0 { Err(Code::DivZero) } else { Ok(Decimal::zero()) };
    }
    let m = n.unsigned_abs() as i64;
    // Magnitude bounds: 10^(m*a) <= |x|^m < 10^(m*(a+1)). Outside the range rule's reach, the
    // result cannot be in range whatever its rounding.
    let a = x.adjusted();
    let lo = if n > 0 { m * a } else { -m * (a + 1) };
    let hi = if n > 0 { m * (a + 1) } else { -m * a + 1 };
    if lo >= 130 || hi <= -130 {
        return Err(Code::Range);
    }
    let mut acc = Decimal::make(false, Big::from_u64(1), 0);
    let mut base = x.abs();
    let mut e = m as u64;
    while e > 0 {
        if e & 1 == 1 {
            acc = mul_exact(&acc, &base);
        }
        e >>= 1;
        if e > 0 {
            base = mul_exact(&base, &base);
        }
    }
    if x.neg && m % 2 == 1 {
        acc = acc.neg();
    }
    if n > 0 {
        result(acc)
    } else {
        div_round(&int(1), &acc, PRECISION, Mode::HalfEven).check_range()
    }
}

/// `x ^ y`: integer `y` by [`power_int`], otherwise POW-EXPLN.
pub fn power(x: &Decimal, y: &Decimal) -> Result<Decimal, Code> {
    if y.is_integer() {
        power_int(x, y)
    } else {
        pow_expln(x, y)
    }
}

/// Excel's ROUND: half away from zero to `digits` decimals (negative: tens, hundreds...);
/// ROUNDUP and ROUNDDOWN by `mode`. Exact, then the range rule.
pub fn round_digits(x: &Decimal, digits: &Decimal, mode: RoundMode) -> Result<Decimal, Code> {
    let d = digits_of(digits)?;
    let m = match mode {
        RoundMode::Nearest => Mode::HalfAway,
        RoundMode::Up => Mode::Up,
        RoundMode::Down => Mode::Down,
    };
    let e = d.checked_neg().ok_or(Code::Domain)?;
    if x.is_zero() || x.exp >= e {
        return Ok(x.clone());
    }
    // Rounding at a position above the leading digit: zero, or one unit of it away from zero.
    if e > x.adjusted() + 1 {
        if m != Mode::Up {
            return Ok(Decimal::zero());
        }
        if e > MAX_CHARS as i64 {
            return Err(Code::Range);
        }
        return Decimal::make(x.neg, Big::from_u64(1), e).check_range();
    }
    x.round_to_exp(e, m).check_range()
}

/// The digits argument of `round`, `round_up`, `round_down` and `fixed`: an integer.
pub(crate) fn digits_of(digits: &Decimal) -> Result<i64, Code> {
    if !digits.is_integer() {
        return Err(Code::Domain);
    }
    digits.to_i64().ok_or(Code::Domain)
}

/// The direction of [`round_digits`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RoundMode {
    /// ROUND: half away from zero.
    Nearest,
    /// ROUNDUP: away from zero.
    Up,
    /// ROUNDDOWN: toward zero.
    Down,
}

/// Excel's MOD: `x - divisor * floor(x / divisor)`, exact; the sign of the divisor.
pub fn modulo(x: &Decimal, d: &Decimal) -> Result<Decimal, Code> {
    if d.is_zero() {
        return Err(Code::DivZero);
    }
    // floor(x / d) exactly: the quotient rounded toward minus infinity at the units digit.
    let q = floor_div(x, d);
    sub_exact(x, &mul_exact(d, &q)).check_range()
}

/// `floor(a / b)`, exact.
fn floor_div(a: &Decimal, b: &Decimal) -> Decimal {
    // a / b = (ca * 10^ea) / (cb * 10^eb). Bring both to a common exponent, then divide integers.
    let e = a.exp.min(b.exp);
    let ca = a.coef.mul(&pow10((a.exp - e) as u64));
    let cb = b.coef.mul(&pow10((b.exp - e) as u64));
    let (q, r) = ca.divrem(&cb);
    let neg = a.neg != b.neg;
    let q = if neg && !r.is_zero() { q.add(&Big::from_u64(1)) } else { q };
    Decimal::make(neg, q, 0)
}

// ---------------------------------------------------------------------------------------------
// Aggregates (15.7, 15.9.2).

/// The exact sum, rounded once; 0 for no values.
pub fn sum(values: &[Decimal]) -> Result<Decimal, Code> {
    result(exact_sum(values))
}

fn exact_sum(values: &[Decimal]) -> Decimal {
    let mut acc = Decimal::zero();
    for v in values {
        acc = add_exact(&acc, v);
    }
    acc
}

/// The exact sum divided by the count, rounded once.
pub fn mean(values: &[Decimal]) -> Result<Decimal, Code> {
    if values.is_empty() {
        return Err(Code::Empty);
    }
    div_round(&exact_sum(values), &int(values.len() as i64), PRECISION, Mode::HalfEven).check_range()
}

/// The middle value; for an even count, the two middle values' exact sum halved, rounded once.
pub fn median(values: &[Decimal]) -> Result<Decimal, Code> {
    if values.is_empty() {
        return Err(Code::Empty);
    }
    let mut v = values.to_vec();
    v.sort();
    let n = v.len();
    if n % 2 == 1 {
        Ok(v[n / 2].clone())
    } else {
        let s = add_exact(&v[n / 2 - 1], &v[n / 2]);
        div_round(&s, &int(2), PRECISION, Mode::HalfEven).check_range()
    }
}

/// The sample standard deviation (15.9.2); needs two values.
pub fn stdev(values: &[Decimal]) -> Result<Decimal, Code> {
    deviation(values, true)
}

/// The population standard deviation (15.9.2); needs one value.
pub fn stdev_p(values: &[Decimal]) -> Result<Decimal, Code> {
    deviation(values, false)
}

fn deviation(values: &[Decimal], sample: bool) -> Result<Decimal, Code> {
    let n = values.len();
    if n == 0 || (sample && n < 2) {
        return Err(Code::Empty);
    }
    let m = mean(values)?;
    let mut ss = Decimal::zero();
    for x in values {
        let d = sub_exact(x, &m);
        ss = add_exact(&ss, &mul_exact(&d, &d));
    }
    let ss = rnd(ss, PRECISION).check_range()?;
    let den = int(if sample { n as i64 - 1 } else { n as i64 });
    let v = div_round(&ss, &den, PRECISION, Mode::HalfEven).check_range()?;
    sqrt(&v)
}

// ---------------------------------------------------------------------------------------------
// The transcendental algorithms (15.9.3).

/// The constants of 15.9.3.
pub(crate) struct Constants {
    pub(crate) pi: Decimal,
    pub(crate) half_pi: Decimal,
    pub(crate) ln10: Decimal,
    pub(crate) half_pi_60: Decimal,
    pub(crate) ln10_60: Decimal,
    pub(crate) rad_per_deg: Decimal,
}

pub(crate) fn constants() -> &'static Constants {
    use std::sync::OnceLock;
    static C: OnceLock<Constants> = OnceLock::new();
    C.get_or_init(|| {
        let p = |s: &str| Decimal::parse(s).expect("constant");
        let pi = p("3.141592653589793238462643383279502884197");
        let rad_per_deg = div_p(&pi, &int(180), WORKING);
        Constants {
            pi,
            half_pi: p("1.570796326794896619231321691639751442099"),
            ln10: p("2.302585092994045684017991454684364207601"),
            half_pi_60: p("1.57079632679489661923132169163975144209858469968755291048747"),
            ln10_60: p("2.30258509299404568401799145468436420760110148862877297603333"),
            rad_per_deg,
        }
    })
}

/// SQRT40: the exact square root truncated to 40 significant digits. `x >= 0`.
pub(crate) fn sqrt40(x: &Decimal) -> Result<Decimal, Code> {
    if x.neg {
        return Err(Code::Domain);
    }
    if x.is_zero() {
        return Ok(Decimal::zero());
    }
    // Scale the coefficient by 10^k so that the exponent becomes even and the root has at least
    // 41 digits: sqrt(c * 10^k) * 10^((e - k) / 2).
    let d = x.digits() as i64;
    let mut k = (2 * (WORKING as i64 + 1) - d).max(0);
    if (x.exp - k).rem_euclid(2) != 0 {
        k += 1;
    }
    let c = x.coef.mul(&pow10(k as u64));
    let root = c.isqrt();
    let exp = (x.exp - k) / 2;
    let rd = root.digit_count();
    let r = Decimal::make(false, root, exp);
    let _ = rd;
    Ok(r.round_sig(WORKING, Mode::Down))
}

/// SQRT-T40: SQRT40, then half-even to 34.
pub fn sqrt(x: &Decimal) -> Result<Decimal, Code> {
    result(sqrt40(x)?)
}

/// EXP-T48 at 40 digits; `abs(x) <= 300` (`E_FORMULA_RANGE` beyond).
pub(crate) fn exp40(x: &Decimal) -> Result<Decimal, Code> {
    let c = constants();
    if x.abs() > int(300) {
        return Err(Code::Range);
    }
    let n = div_p(x, &c.ln10, WORKING).round_integer_half_even();
    let r = rnd(sub_exact(x, &mul_exact(&n, &c.ln10_60)), WORKING);
    let mut s = int(1);
    let mut t = int(1);
    for k in 1..=47 {
        t = div_p(&mul_p(&t, &r, WORKING), &int(k), WORKING);
        s = add_p(&s, &t, WORKING);
    }
    let n = n.to_i64().unwrap_or(0);
    Ok(s.scale10(n))
}

/// EXP-T48, then half-even to 34.
pub fn exp(x: &Decimal) -> Result<Decimal, Code> {
    result(exp40(x)?)
}

/// LN-ATANH80 at 40 digits; `x > 0` (`E_FORMULA_DOMAIN`).
pub(crate) fn ln40(x: &Decimal) -> Result<Decimal, Code> {
    let c = constants();
    if x.neg || x.is_zero() {
        return Err(Code::Domain);
    }
    // x = m * 10^k with 1 <= m < 10.
    let mut k = x.adjusted();
    let mut m = x.scale10(-k);
    if mul_exact(&m, &m) >= int(10) {
        m = m.scale10(-1);
        k += 1;
    }
    let one = int(1);
    let z = div_p(&sub_p(&m, &one, WORKING), &add_p(&m, &one, WORKING), WORKING);
    let z2 = mul_p(&z, &z, WORKING);
    let mut p = z;
    let mut s = Decimal::zero();
    for j in 0..80i64 {
        s = add_p(&s, &div_p(&p, &int(2 * j + 1), WORKING), WORKING);
        p = mul_p(&p, &z2, WORKING);
    }
    let lnm = mul_p(&int(2), &s, WORKING);
    Ok(add_p(&lnm, &mul_p(&int(k), &c.ln10, WORKING), WORKING))
}

/// LN-ATANH80, then half-even to 34.
pub fn ln(x: &Decimal) -> Result<Decimal, Code> {
    result(ln40(x)?)
}

/// POW-EXPLN for a non-integer `y`.
pub fn pow_expln(x: &Decimal, y: &Decimal) -> Result<Decimal, Code> {
    if x.is_zero() {
        return if y.neg { Err(Code::DivZero) } else { Ok(Decimal::zero()) };
    }
    if x.neg {
        return Err(Code::Domain);
    }
    let l = ln40(x)?;
    result(exp40(&mul_p(y, &l, WORKING))?)
}

/// TRIG-T24 at 40 digits: `(sin x, cos x)`; `abs(x) <= 1000000` (`E_FORMULA_DOMAIN`).
pub(crate) fn sincos40(x: &Decimal) -> Result<(Decimal, Decimal), Code> {
    let c = constants();
    if x.abs() > int(1_000_000) {
        return Err(Code::Domain);
    }
    let k = div_p(x, &c.half_pi, WORKING).round_integer_half_even();
    let r = rnd(sub_exact(x, &mul_exact(&k, &c.half_pi_60)), WORKING);
    let r2 = mul_p(&r, &r, WORKING);
    let mut sin_r = r.clone();
    let mut t = r.clone();
    for j in 1..24i64 {
        t = div_p(&mul_p(&t, &r2, WORKING), &int((2 * j) * (2 * j + 1)), WORKING).neg();
        sin_r = add_p(&sin_r, &t, WORKING);
    }
    let mut cos_r = int(1);
    let mut t = int(1);
    for j in 1..24i64 {
        t = div_p(&mul_p(&t, &r2, WORKING), &int((2 * j - 1) * (2 * j)), WORKING).neg();
        cos_r = add_p(&cos_r, &t, WORKING);
    }
    let q = k.to_i64().unwrap_or(0).rem_euclid(4);
    Ok(match q {
        0 => (sin_r, cos_r),
        1 => (cos_r, sin_r.neg()),
        2 => (sin_r.neg(), cos_r.neg()),
        _ => (cos_r.neg(), sin_r),
    })
}

/// TRIG-T24 sine, half-even to 34.
pub fn sin(x: &Decimal) -> Result<Decimal, Code> {
    result(sincos40(x)?.0)
}

/// TRIG-T24 cosine, half-even to 34.
pub fn cos(x: &Decimal) -> Result<Decimal, Code> {
    result(sincos40(x)?.1)
}

/// ATAN2-T32 at 40 digits, in radians; `atan2(0, 0)` is `E_FORMULA_DOMAIN`.
pub(crate) fn atan2_40(y: &Decimal, x: &Decimal) -> Result<Decimal, Code> {
    let c = constants();
    if x.is_zero() && y.is_zero() {
        return Err(Code::Domain);
    }
    let a = y.abs();
    let b = x.abs();
    let swapped = a > b;
    let mut t = if swapped { div_p(&b, &a, WORKING) } else { div_p(&a, &b, WORKING) };
    let one = int(1);
    for _ in 0..2 {
        let inner = add_p(&one, &mul_p(&t, &t, WORKING), WORKING);
        t = div_p(&t, &add_p(&one, &sqrt40(&inner)?, WORKING), WORKING);
    }
    let t2 = mul_p(&t, &t, WORKING);
    let mut p = t;
    let mut s = Decimal::zero();
    for j in 0..32i64 {
        let term = div_p(&p, &int(2 * j + 1), WORKING);
        s = if j % 2 == 0 { add_p(&s, &term, WORKING) } else { sub_p(&s, &term, WORKING) };
        p = mul_p(&p, &t2, WORKING);
    }
    let mut r = mul_p(&int(4), &s, WORKING);
    if swapped {
        r = sub_p(&c.half_pi, &r, WORKING);
    }
    if x.neg {
        r = sub_p(&c.pi, &r, WORKING);
    }
    if y.neg {
        r = r.neg();
    }
    Ok(r)
}

/// ATAN2-T32, half-even to 34.
pub fn atan2(y: &Decimal, x: &Decimal) -> Result<Decimal, Code> {
    result(atan2_40(y, x)?)
}

/// The 40-digit pi rounded to 34.
pub fn pi() -> Decimal {
    rnd(constants().pi.clone(), PRECISION)
}

/// Degrees to radians at 40 digits.
pub(crate) fn radians40(x: &Decimal) -> Decimal {
    mul_p(x, &constants().rad_per_deg, WORKING)
}

/// HAVERSINE (15.10): metres on a sphere of radius 6371008.8 m, to the millimetre.
pub fn haversine(lat1: &Decimal, lon1: &Decimal, lat2: &Decimal, lon2: &Decimal) -> Result<Decimal, Code> {
    let ninety = int(90);
    let full = int(360);
    if lat1.abs() > ninety || lon1.abs() > full || lat2.abs() > ninety || lon2.abs() > full {
        return Err(Code::Domain);
    }
    let w = WORKING;
    let two = int(2);
    let one = int(1);
    let phi1 = radians40(lat1);
    let phi2 = radians40(lat2);
    let lam1 = radians40(lon1);
    let lam2 = radians40(lon2);
    let dphi = sub_p(&phi2, &phi1, w);
    let dlam = sub_p(&lam2, &lam1, w);
    let s1 = sincos40(&div_p(&dphi, &two, w))?.0;
    let s2 = sincos40(&div_p(&dlam, &two, w))?.0;
    let c1 = sincos40(&phi1)?.1;
    let c2 = sincos40(&phi2)?.1;
    let a = mul_p(&s1, &s1, w);
    let b = mul_p(&mul_p(&c1, &c2, w), &mul_p(&s2, &s2, w), w);
    let mut h = add_p(&a, &b, w);
    if h.neg {
        h = Decimal::zero();
    }
    if h > one {
        h = one.clone();
    }
    let r = Decimal::parse("6371008.8").expect("radius");
    let angle = atan2_40(&sqrt40(&h)?, &sqrt40(&sub_p(&one, &h, w))?)?;
    let d = mul_p(&mul_p(&r, &two, w), &angle, w);
    d.round_to_exp(-3, Mode::HalfEven).check_range()
}

// ---------------------------------------------------------------------------------------------

/// Apply the operation `op` of a `decimal` vector (SPEC 11) to canonical number arguments:
/// `add sub mul div power sqrt exp ln pow sin cos atan2 haversine sum mean median stdev
/// stdev_p round mod`. A malformed argument or an unknown operation is `E_FORMULA_NUMBER`.
pub fn apply(op: &str, args: &[&str]) -> Result<String, Code> {
    let mut v = Vec::with_capacity(args.len());
    for a in args {
        v.push(Decimal::parse(a).ok_or(Code::Number)?);
    }
    let need = |n: usize| if v.len() == n { Ok(()) } else { Err(Code::Arity) };
    let r = match op {
        "add" => need(2).and_then(|_| add(&v[0], &v[1])),
        "sub" => need(2).and_then(|_| sub(&v[0], &v[1])),
        "mul" => need(2).and_then(|_| mul(&v[0], &v[1])),
        "div" => need(2).and_then(|_| div(&v[0], &v[1])),
        "power" => need(2).and_then(|_| power_int(&v[0], &v[1])),
        "pow" => need(2).and_then(|_| power(&v[0], &v[1])),
        "sqrt" => need(1).and_then(|_| sqrt(&v[0])),
        "exp" => need(1).and_then(|_| exp(&v[0])),
        "ln" => need(1).and_then(|_| ln(&v[0])),
        "sin" => need(1).and_then(|_| sin(&v[0])),
        "cos" => need(1).and_then(|_| cos(&v[0])),
        "atan2" => need(2).and_then(|_| atan2(&v[0], &v[1])),
        "haversine" => need(4).and_then(|_| haversine(&v[0], &v[1], &v[2], &v[3])),
        "sum" => sum(&v),
        "mean" => mean(&v),
        "median" => median(&v),
        "stdev" => stdev(&v),
        "stdev_p" => stdev_p(&v),
        "round" => need(2).and_then(|_| round_digits(&v[0], &v[1], RoundMode::Nearest)),
        "mod" => need(2).and_then(|_| modulo(&v[0], &v[1])),
        _ => Err(Code::Number),
    }?;
    Ok(r.to_canonical())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::parse(s).unwrap()
    }

    #[test]
    fn lexical() {
        for s in ["0", "1", "-1", "1.5", "0.001", "-0.25", "100", "123456789012345678901234567890.0000001"] {
            assert_eq!(d(s).to_canonical(), s);
            assert_eq!(d(s).canonical_len() as usize, s.len());
        }
        for s in ["", "-0", "01", "1.", ".5", "1.50", "+1", "1e5", "-", "0.0", "1..2"] {
            assert!(Decimal::parse(s).is_none(), "{s}");
        }
    }

    #[test]
    fn rounding() {
        assert_eq!(add(&d("1234567890123456789012345678901234"), &d("0.5")).unwrap().to_canonical(), "1234567890123456789012345678901234");
        assert_eq!(add(&d("1234567890123456789012345678901233"), &d("0.5")).unwrap().to_canonical(), "1234567890123456789012345678901234");
        assert_eq!(div(&d("2"), &d("3")).unwrap().to_canonical(), "0.6666666666666666666666666666666667");
        assert_eq!(sub(&d("1.5"), &d("1.5")).unwrap().to_canonical(), "0");
    }
}
