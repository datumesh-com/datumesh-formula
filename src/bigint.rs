//! A small unsigned integer of 64-bit limbs, little-endian, for exact decimal coefficients.
//!
//! The coefficients of section 15.9 are mostly 34 or 40 digits, but exact intermediates (a sum of
//! numbers with distant exponents, an integer power, a quotient's scaled numerator) are larger,
//! so the integer grows as needed. Only what decimal arithmetic needs is implemented.

use core::cmp::Ordering;

/// An unsigned integer. The limb vector has no high zero limbs; zero is the empty vector.
#[derive(Clone, PartialEq, Eq, Hash, Default, Debug)]
pub(crate) struct Big {
    limbs: Vec<u64>,
}

const TEN19: u64 = 10_000_000_000_000_000_000;

impl Big {
    pub(crate) fn zero() -> Big {
        Big { limbs: Vec::new() }
    }

    pub(crate) fn from_u64(v: u64) -> Big {
        let mut b = Big { limbs: vec![v] };
        b.trim();
        b
    }

    pub(crate) fn from_u128(v: u128) -> Big {
        let mut b = Big { limbs: vec![v as u64, (v >> 64) as u64] };
        b.trim();
        b
    }

    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// The value, if it fits in a `u64`.
    pub(crate) fn to_u64(&self) -> Option<u64> {
        match self.limbs.len() {
            0 => Some(0),
            1 => Some(self.limbs[0]),
            _ => None,
        }
    }

    pub(crate) fn is_even(&self) -> bool {
        self.limbs.first().is_none_or(|l| l & 1 == 0)
    }

    pub(crate) fn bits(&self) -> u64 {
        match self.limbs.last() {
            None => 0,
            Some(top) => (self.limbs.len() as u64 - 1) * 64 + (64 - top.leading_zeros() as u64),
        }
    }

    /// Parse a run of ASCII digits (the caller has checked them).
    pub(crate) fn from_digits(digits: &[u8]) -> Big {
        let mut b = Big::zero();
        for chunk in digits.chunks(19) {
            let mut v: u64 = 0;
            let mut scale: u64 = 1;
            for &d in chunk {
                v = v * 10 + u64::from(d - b'0');
                scale *= 10;
            }
            b.mul_small_add(scale, v);
        }
        b
    }

    /// The decimal digits, without leading zeros ("0" for zero).
    pub(crate) fn to_digits(&self) -> String {
        if self.is_zero() {
            return "0".to_string();
        }
        let mut parts = Vec::new();
        let mut n = self.clone();
        while !n.is_zero() {
            let r = n.divrem_small_in_place(TEN19);
            parts.push(r);
        }
        let mut s = String::with_capacity(parts.len() * 19);
        let last = parts.len() - 1;
        for (i, p) in parts.iter().rev().enumerate() {
            if i == 0 {
                s.push_str(&p.to_string());
            } else {
                let t = p.to_string();
                for _ in t.len()..19 {
                    s.push('0');
                }
                s.push_str(&t);
            }
            let _ = last;
        }
        s
    }

    /// The number of decimal digits (1 for zero).
    pub(crate) fn digit_count(&self) -> u64 {
        if self.is_zero() {
            return 1;
        }
        // Estimate from the bit length, then correct against a power of ten.
        let bits = self.bits();
        let est = (bits - 1) * 30103 / 100000 + 1; // floor((bits-1) log10 2) + 1 <= digits
        let mut d = est;
        while *self >= pow10(d) {
            d += 1;
        }
        d
    }

    /// self = self * m + a.
    pub(crate) fn mul_small_add(&mut self, m: u64, a: u64) {
        let mut carry: u128 = u128::from(a);
        for limb in self.limbs.iter_mut() {
            let t = u128::from(*limb) * u128::from(m) + carry;
            *limb = t as u64;
            carry = t >> 64;
        }
        if carry != 0 {
            self.limbs.push(carry as u64);
        }
        self.trim();
    }

    pub(crate) fn mul_small(&self, m: u64) -> Big {
        let mut b = self.clone();
        b.mul_small_add(m, 0);
        b
    }

    /// Divide in place by `d` (non-zero), returning the remainder.
    pub(crate) fn divrem_small_in_place(&mut self, d: u64) -> u64 {
        let mut rem: u128 = 0;
        for limb in self.limbs.iter_mut().rev() {
            let cur = (rem << 64) | u128::from(*limb);
            *limb = (cur / u128::from(d)) as u64;
            rem = cur % u128::from(d);
        }
        self.trim();
        rem as u64
    }

    pub(crate) fn add(&self, other: &Big) -> Big {
        let (a, b) = if self.limbs.len() >= other.limbs.len() { (self, other) } else { (other, self) };
        let mut out = Vec::with_capacity(a.limbs.len() + 1);
        let mut carry = 0u64;
        for i in 0..a.limbs.len() {
            let x = a.limbs[i];
            let y = if i < b.limbs.len() { b.limbs[i] } else { 0 };
            let (s1, c1) = x.overflowing_add(y);
            let (s2, c2) = s1.overflowing_add(carry);
            out.push(s2);
            carry = u64::from(c1) + u64::from(c2);
        }
        if carry != 0 {
            out.push(carry);
        }
        let mut r = Big { limbs: out };
        r.trim();
        r
    }

    /// self - other; requires self >= other.
    pub(crate) fn sub(&self, other: &Big) -> Big {
        debug_assert!(*self >= *other);
        let mut out = Vec::with_capacity(self.limbs.len());
        let mut borrow = 0u64;
        for i in 0..self.limbs.len() {
            let x = self.limbs[i];
            let y = if i < other.limbs.len() { other.limbs[i] } else { 0 };
            let (d1, b1) = x.overflowing_sub(y);
            let (d2, b2) = d1.overflowing_sub(borrow);
            out.push(d2);
            borrow = u64::from(b1) + u64::from(b2);
        }
        let mut r = Big { limbs: out };
        r.trim();
        r
    }

    pub(crate) fn mul(&self, other: &Big) -> Big {
        if self.is_zero() || other.is_zero() {
            return Big::zero();
        }
        let mut out = vec![0u64; self.limbs.len() + other.limbs.len()];
        for (i, &x) in self.limbs.iter().enumerate() {
            let mut carry: u128 = 0;
            for (j, &y) in other.limbs.iter().enumerate() {
                let t = u128::from(x) * u128::from(y) + u128::from(out[i + j]) + carry;
                out[i + j] = t as u64;
                carry = t >> 64;
            }
            let mut k = i + other.limbs.len();
            while carry != 0 {
                let t = u128::from(out[k]) + carry;
                out[k] = t as u64;
                carry = t >> 64;
                k += 1;
            }
        }
        let mut r = Big { limbs: out };
        r.trim();
        r
    }

    fn shl_bits(&self, s: u32) -> Big {
        if s == 0 || self.is_zero() {
            return self.clone();
        }
        let mut out = Vec::with_capacity(self.limbs.len() + 1);
        let mut carry = 0u64;
        for &l in &self.limbs {
            out.push((l << s) | carry);
            carry = l >> (64 - s);
        }
        if carry != 0 {
            out.push(carry);
        }
        Big { limbs: out }
    }

    fn shr_bits(&self, s: u32) -> Big {
        if s == 0 || self.is_zero() {
            return self.clone();
        }
        let mut out = vec![0u64; self.limbs.len()];
        for i in 0..self.limbs.len() {
            let hi = if i + 1 < self.limbs.len() { self.limbs[i + 1] << (64 - s) } else { 0 };
            out[i] = (self.limbs[i] >> s) | hi;
        }
        let mut r = Big { limbs: out };
        r.trim();
        r
    }

    /// (self / d, self % d); `d` must be non-zero. Knuth's algorithm D.
    pub(crate) fn divrem(&self, d: &Big) -> (Big, Big) {
        assert!(!d.is_zero(), "division by zero in Big::divrem");
        if *self < *d {
            return (Big::zero(), self.clone());
        }
        if d.limbs.len() == 1 {
            let mut q = self.clone();
            let r = q.divrem_small_in_place(d.limbs[0]);
            return (q, Big::from_u64(r));
        }
        let s = d.limbs.last().copied().unwrap_or(1).leading_zeros();
        let v = d.shl_bits(s);
        let mut u = self.shl_bits(s).limbs;
        u.push(0);
        let n = v.limbs.len();
        let m = u.len() - n - 1;
        let mut q = vec![0u64; m + 1];
        let vtop = u128::from(v.limbs[n - 1]);
        let vnext = u128::from(v.limbs[n - 2]);
        let b: u128 = 1u128 << 64;
        for j in (0..=m).rev() {
            let num = (u128::from(u[j + n]) << 64) | u128::from(u[j + n - 1]);
            let mut qhat = num / vtop;
            let mut rhat = num % vtop;
            while qhat >= b || qhat * vnext > ((rhat << 64) | u128::from(u[j + n - 2])) {
                qhat -= 1;
                rhat += vtop;
                if rhat >= b {
                    break;
                }
            }
            // Multiply and subtract.
            let mut borrow: i128 = 0;
            let mut carry: u128 = 0;
            for i in 0..n {
                let p = qhat * u128::from(v.limbs[i]) + carry;
                carry = p >> 64;
                let t = i128::from(u[i + j]) - i128::from(p as u64) + borrow;
                u[i + j] = t as u64;
                borrow = t >> 64;
            }
            let t = i128::from(u[j + n]) - (carry as i128) + borrow;
            u[j + n] = t as u64;
            borrow = t >> 64;
            if borrow < 0 {
                // Add back.
                qhat -= 1;
                let mut c: u128 = 0;
                for i in 0..n {
                    let t = u128::from(u[i + j]) + u128::from(v.limbs[i]) + c;
                    u[i + j] = t as u64;
                    c = t >> 64;
                }
                u[j + n] = u[j + n].wrapping_add(c as u64);
            }
            q[j] = qhat as u64;
        }
        let mut qb = Big { limbs: q };
        qb.trim();
        let mut rb = Big { limbs: u[..n].to_vec() };
        rb.trim();
        (qb, rb.shr_bits(s))
    }

    /// The integer square root: the greatest r with r * r <= self.
    pub(crate) fn isqrt(&self) -> Big {
        if self.is_zero() {
            return Big::zero();
        }
        // Start above the root: 2^ceil(bits/2).
        let mut x = Big::from_u64(1).shl_by((self.bits() + 1) / 2);
        loop {
            // y = (x + self / x) / 2
            let (q, _) = self.divrem(&x);
            let mut y = x.add(&q);
            y.divrem_small_in_place(2);
            if y >= x {
                return x;
            }
            x = y;
        }
    }

    fn shl_by(&self, bits: u64) -> Big {
        let limbs = (bits / 64) as usize;
        let rest = (bits % 64) as u32;
        let mut v = vec![0u64; limbs];
        v.extend_from_slice(&self.shl_bits(rest).limbs);
        let mut r = Big { limbs: v };
        r.trim();
        r
    }
}

/// 10^n.
pub(crate) fn pow10(n: u64) -> Big {
    let mut b = Big::from_u64(1);
    let mut left = n;
    while left >= 19 {
        b.mul_small_add(TEN19, 0);
        left -= 19;
    }
    if left > 0 {
        b.mul_small_add(10u64.pow(left as u32), 0);
    }
    b
}

impl Ord for Big {
    fn cmp(&self, other: &Big) -> Ordering {
        match self.limbs.len().cmp(&other.limbs.len()) {
            Ordering::Equal => {}
            o => return o,
        }
        for i in (0..self.limbs.len()).rev() {
            match self.limbs[i].cmp(&other.limbs[i]) {
                Ordering::Equal => {}
                o => return o,
            }
        }
        Ordering::Equal
    }
}

impl PartialOrd for Big {
    fn partial_cmp(&self, other: &Big) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(s: &str) -> Big {
        Big::from_digits(s.as_bytes())
    }

    #[test]
    fn digits_round_trip() {
        for s in ["0", "1", "18446744073709551615", "18446744073709551616", "123456789012345678901234567890123456789012345678901234567890"] {
            assert_eq!(b(s).to_digits(), s);
            assert_eq!(b(s).digit_count(), s.len() as u64);
        }
        assert_eq!(pow10(40).to_digits(), format!("1{}", "0".repeat(40)));
    }

    #[test]
    fn division() {
        let a = b("123456789012345678901234567890123456789012345678901234567890123456789");
        let d = b("98765432109876543210987654321");
        let (q, r) = a.divrem(&d);
        assert_eq!(q.mul(&d).add(&r), a);
        assert!(r < d);
        let d2 = b("340282366920938463463374607431768211457");
        let (q, r) = a.divrem(&d2);
        assert_eq!(q.mul(&d2).add(&r), a);
        assert!(r < d2);
        // A case that needs the add-back step.
        let u = Big { limbs: vec![0, 0, 0x8000_0000_0000_0000, 0x7fff_ffff_ffff_ffff] };
        let v = Big { limbs: vec![1, 0, 0x8000_0000_0000_0000] };
        let (q, r) = u.divrem(&v);
        assert_eq!(q.mul(&v).add(&r), u);
        assert!(r < v);
    }

    #[test]
    fn square_root() {
        for s in ["0", "1", "2", "4", "99", "100", "123456789012345678901234567890123456789012345678901234567890"] {
            let n = b(s);
            let r = n.isqrt();
            assert!(r.mul(&r) <= n);
            let r1 = r.add(&Big::from_u64(1));
            assert!(r1.mul(&r1) > n);
        }
    }
}
