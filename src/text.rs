//! The text built-ins of SPEC 15.7 with Unicode 15.1 case data carried in `casing_table.rs`.

use unicode_normalization::UnicodeNormalization;

use crate::casing_table::{CASED, CASE_IGNORABLE, LOWER, UPPER};
use crate::Code;

/// The most UTF-8 bytes in a string (SPEC 4.5).
pub const MAX_STRING_BYTES: usize = 1024;

fn in_ranges(t: &[(u32, u32)], c: u32) -> bool {
    t.binary_search_by(|&(a, b)| {
        if b < c {
            core::cmp::Ordering::Less
        } else if a > c {
            core::cmp::Ordering::Greater
        } else {
            core::cmp::Ordering::Equal
        }
    })
    .is_ok()
}

fn mapping(t: &[(u32, &'static str)], c: char) -> Option<&'static str> {
    t.binary_search_by_key(&(c as u32), |&(k, _)| k).ok().map(|i| t[i].1)
}

/// A computed string: NFC (Unicode 15.1 data or later; stable for assigned code points), at most
/// 1024 bytes (`E_FORMULA_RANGE`).
pub fn finish(s: String) -> Result<String, Code> {
    let n: String = if unicode_normalization::is_nfc(&s) { s } else { s.nfc().collect() };
    if n.len() > MAX_STRING_BYTES {
        return Err(Code::Range);
    }
    Ok(n)
}

/// Full uppercase mapping, locale-free.
pub fn upper(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match mapping(UPPER, c) {
            Some(m) => out.push_str(m),
            None => out.push(c),
        }
    }
    out
}

/// Full lowercase mapping, locale-free, with the Final_Sigma context of capital sigma.
pub fn lower(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == '\u{03A3}' {
            out.push(if final_sigma(&chars, i) { '\u{03C2}' } else { '\u{03C3}' });
            continue;
        }
        match mapping(LOWER, c) {
            Some(m) => out.push_str(m),
            None => out.push(c),
        }
    }
    out
}

fn ignorable(c: char) -> bool {
    in_ranges(CASE_IGNORABLE, c as u32)
}

fn cased(c: char) -> bool {
    in_ranges(CASED, c as u32)
}

/// `\p{cased} \p{case-ignorable}* U+03A3 !( \p{case-ignorable}* \p{cased} )`.
fn final_sigma(chars: &[char], i: usize) -> bool {
    let before = chars[..i].iter().rev().find(|&&c| !ignorable(c));
    if !before.is_some_and(|&c| cased(c)) {
        return false;
    }
    let after = chars[i + 1..].iter().find(|&&c| !ignorable(c));
    !after.is_some_and(|&c| cased(c))
}

/// Excel's TRIM: U+0020 trimmed at both ends and runs of it collapsed to one.
pub fn trim(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for part in s.split(' ').filter(|p| !p.is_empty()) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(part);
    }
    out
}

/// The number of code points.
pub fn len(s: &str) -> usize {
    s.chars().count()
}

/// Excel's MID: `length` code points from the 1-based `start`.
pub fn substr(s: &str, start: i64, length: i64) -> Result<String, Code> {
    if start < 1 || length < 0 {
        return Err(Code::Domain);
    }
    Ok(s.chars().skip((start - 1) as usize).take(length as usize).collect())
}

/// Excel's FIND: the 1-based position of `needle` in `text` from `start`, case-sensitive; 0
/// when not found.
pub fn find(needle: &str, text: &str, start: i64) -> Result<i64, Code> {
    let chars: Vec<char> = text.chars().collect();
    if start < 1 || start as usize > chars.len() + 1 {
        return Err(Code::Domain);
    }
    let n: Vec<char> = needle.chars().collect();
    let from = (start - 1) as usize;
    if n.is_empty() {
        return Ok(start);
    }
    for i in from..chars.len() {
        if chars[i..].starts_with(&n) {
            return Ok(i as i64 + 1);
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cases() {
        assert_eq!(upper("Zürich"), "ZÜRICH");
        assert_eq!(upper("ß"), "SS");
        assert_eq!(lower("ÄBC"), "äbc");
        assert_eq!(lower("ΑΣ"), "ας");
        assert_eq!(lower("ΑΣ Β"), "ας β");
        assert_eq!(lower("Σ"), "σ");
        assert_eq!(lower("İ"), "i\u{307}");
        assert_eq!(trim("  a   b  "), "a b");
        assert_eq!(substr("Datumesh", 3, 4).unwrap(), "tume");
        assert_eq!(find("m", "mama", 2).unwrap(), 3);
        assert_eq!(find("x", "Datumesh", 1).unwrap(), 0);
    }
}
