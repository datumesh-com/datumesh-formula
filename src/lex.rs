//! The lexer of SPEC 15.2: longest match, the classes L1-L9 tried in order at each position.
//! The whole body is lexed before it is parsed, so a lexical error anywhere is reported first.

use datumesh_core::{Scalar, Type};

use crate::calendar::{parse_instant, Date};
use crate::decimal::{Decimal, MAX_CHARS};
use crate::{units, Code, Error};

/// The kind of a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Instant,
    Date,
    Qualified,
    Alias,
    /// `@` and 64 lowercase hex digits directly after a qualified name or alias (L4a, a pin).
    Pin,
    Number,
    /// Since spec-0.6.0 (C3): a request's digit-led word, a run beginning with a digit, continuing
    /// with letters, digits or `_` and holding a letter, that the number and period lexers do not
    /// accept whole. Only in a request, and a value only in a string position.
    DigitWord,
    /// A period key (L5 with D, M or Y, 15.15): `1M`, `10Y`; a string in a key position.
    Period,
    Str,
    Word,
    Punct,
    End,
}

/// A token: its kind, its text (a string's decoded content; an alias without `@`; a number's
/// digits), a number's unit suffix, and its byte position.
#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub(crate) kind: Kind,
    pub(crate) text: String,
    pub(crate) unit: Option<String>,
    pub(crate) pos: usize,
}

impl Token {
    pub(crate) fn is(&self, p: &str) -> bool {
        self.kind == Kind::Punct && self.text == p
    }

    /// Whether this is the word `w`, compared case-insensitively.
    pub(crate) fn is_word(&self, w: &str) -> bool {
        self.kind == Kind::Word && self.text.eq_ignore_ascii_case(w)
    }

    /// How the token reads in a message.
    pub(crate) fn describe(&self) -> String {
        match self.kind {
            Kind::End => "the end".to_string(),
            Kind::Str => format!("\"{}\"", self.text),
            Kind::Alias => format!("@{}", self.text),
            _ => format!("'{}'", self.text),
        }
    }
}

/// The punctuation of L9, two-character forms first.
const PUNCT2: &[&str] = &["..", "->", "<>", "<=", ">="];
const PUNCT1: &[u8] = b"()[],:*+-/^&=<>.";

pub(crate) fn is_word_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

fn err(code: Code, pos: usize, detail: impl Into<String>) -> Error {
    Error::new(code, format!("{} at {pos}", detail.into()))
}

/// The maximum body size (SPEC 4.5): 1 MiB.
pub const MAX_BODY: usize = 1_048_576;

/// The keywords of 15.2.1, lowercase.
pub(crate) const KEYWORDS: &[&str] = &["if", "then", "else", "let", "in", "and", "or", "not", "true", "false"];

pub(crate) fn is_keyword(w: &str) -> bool {
    KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(w))
}

/// L4a (spec-0.4.2): after a qualified name or alias ending at `i`, an `@` directly following
/// is a pin, `@` and 64 lowercase hex digits not followed by a word character; anything else
/// after such an `@` is `E_FORMULA_NAME`. Returns where lexing continues.
fn pin_after(src: &str, i: usize, out: &mut Vec<Token>) -> Result<usize, Error> {
    let b = src.as_bytes();
    if b.get(i) != Some(&b'@') {
        return Ok(i);
    }
    let mut j = i + 1;
    while j < b.len() && (is_word_char(b[j]) || b[j] == b'/') {
        j += 1;
    }
    let hex = &src[i + 1..j];
    if hex.len() != 64 || !hex.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
        return Err(err(Code::Name, i, format!("not a pin (@ and 64 lowercase hex digits): @{hex}")));
    }
    out.push(Token { kind: Kind::Pin, text: hex.to_string(), unit: None, pos: i });
    Ok(j)
}

/// Lex the whole body.
pub(crate) fn lex(src: &str) -> Result<Vec<Token>, Error> {
    lex_with(src, false)
}

/// Lex a request (5.8, since spec-0.6.0 C3): as a body, except that a digit-led word is a
/// [`Kind::DigitWord`] and a date followed directly by a letter, digit or `_` is `E_PARSE_URI`.
pub(crate) fn lex_request(src: &str) -> Result<Vec<Token>, Error> {
    lex_with(src, true)
}

fn lex_with(src: &str, request: bool) -> Result<Vec<Token>, Error> {
    if src.len() > MAX_BODY {
        return Err(Error::new(Code::Limit, "the body is over 1 MiB"));
    }
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b' ' || c == b'\t' || c == b'\n' {
            i += 1;
            continue;
        }
        let start = i;
        // L1, L2: instants and dates.
        if date_shape(b, i) {
            if b.get(i + 10) == Some(&b'T') {
                // Anything shaped YYYY-MM-DDT... is an instant or E_FORMULA_INSTANT.
                let end = (i + 27).min(b.len());
                let text = src.get(i..end).unwrap_or("");
                if end - i != 27 || parse_instant(text).is_none() || b.get(end).is_some_and(|&c| is_word_char(c)) {
                    let mut j = i + 10;
                    while j < b.len() && (is_word_char(b[j]) || b[j] == b':' || b[j] == b'.') {
                        j += 1;
                    }
                    return Err(err(Code::Instant, i, format!("not a canonical instant: {}", &src[i..j])));
                }
                out.push(Token { kind: Kind::Instant, text: text.to_string(), unit: None, pos: start });
                i = end;
                continue;
            }
            let text = &src[i..i + 10];
            if request && b.get(i + 10).is_some_and(|&c| is_word_char(c)) {
                return Err(err(Code::Syntax, i, format!("not a call: {text} is followed by a word character")));
            }
            if b.get(i + 10).is_some_and(|&c| is_word_char(c)) || Date::parse(text).is_none() {
                return Err(err(Code::Date, i, format!("not a date: {text}")));
            }
            out.push(Token { kind: Kind::Date, text: text.to_string(), unit: None, pos: start });
            i += 10;
            continue;
        }
        // L3: a qualified name.
        if c.is_ascii_alphanumeric() {
            if let Some(end) = qualified_shape(b, i) {
                let text = &src[i..end];
                if !crate::names::is_function_name(text) {
                    return Err(err(Code::Name, i, format!("not a function name: {text}")));
                }
                out.push(Token { kind: Kind::Qualified, text: text.to_string(), unit: None, pos: start });
                i = pin_after(src, end, &mut out)?;
                continue;
            }
        }
        // L4: an alias.
        if c == b'@' {
            let mut j = i + 1;
            loop {
                while j < b.len() && is_word_char(b[j]) {
                    j += 1;
                }
                if j < b.len() && b[j] == b'/' && b.get(j + 1).is_some_and(|&c| is_word_char(c)) {
                    j += 1;
                    continue;
                }
                break;
            }
            let text = &src[i + 1..j];
            if text.is_empty() || !text.split('/').all(crate::names::is_name) {
                return Err(err(Code::Name, i, format!("not an alias: @{text}")));
            }
            out.push(Token { kind: Kind::Alias, text: text.to_string(), unit: None, pos: start });
            i = pin_after(src, j, &mut out)?;
            continue;
        }
        // L5, L6: a number and its unit suffix; in a request, possibly a digit-led word.
        if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            if request && c.is_ascii_digit() {
                if let Some(end) = digit_led_end(src, i) {
                    out.push(Token { kind: Kind::DigitWord, text: src[i..end].to_string(), unit: None, pos: start });
                    i = end;
                    continue;
                }
            }
            let (tok, next) = number_token(src, i)?;
            out.push(tok);
            i = next;
            continue;
        }
        // L7: a string.
        if c == b'"' {
            let mut j = i + 1;
            let mut content = String::new();
            loop {
                match src[j..].find('"') {
                    None => return Err(err(Code::String, i, "an unterminated string")),
                    Some(p) => {
                        content.push_str(&src[j..j + p]);
                        j += p + 1;
                        if b.get(j) == Some(&b'"') {
                            content.push('"');
                            j += 1;
                        } else {
                            break;
                        }
                    }
                }
            }
            if let Err(e) = Scalar::from_lexical(Type::String, &content) {
                return Err(err(Code::String, i, format!("not a valid string (SPEC 3.5): {e:?}")));
            }
            out.push(Token { kind: Kind::Str, text: content, unit: None, pos: start });
            i = j;
            continue;
        }
        // L8: a word.
        if c.is_ascii_alphabetic() {
            let mut j = i;
            while j < b.len() && is_word_char(b[j]) {
                j += 1;
            }
            out.push(Token { kind: Kind::Word, text: src[i..j].to_string(), unit: None, pos: start });
            i = j;
            continue;
        }
        // L9: punctuation.
        if b[i..].starts_with(b"==") || b[i..].starts_with(b"!=") {
            return Err(err(Code::Syntax, i, "write = and <>, not == and !="));
        }
        if let Some(p) = PUNCT2.iter().find(|p| b[i..].starts_with(p.as_bytes())) {
            out.push(Token { kind: Kind::Punct, text: p.to_string(), unit: None, pos: start });
            i += 2;
            continue;
        }
        if PUNCT1.contains(&c) {
            out.push(Token { kind: Kind::Punct, text: (c as char).to_string(), unit: None, pos: start });
            i += 1;
            continue;
        }
        let ch = src[i..].chars().next().unwrap_or('?');
        return Err(err(Code::Syntax, i, format!("unexpected character {ch:?}")));
    }
    out.push(Token { kind: Kind::End, text: String::new(), unit: None, pos: b.len() });
    Ok(out)
}

/// L5, L6 and the period keys (15.15): the number, period or number with unit token at `i`, and
/// where it ends.
fn number_token(src: &str, i: usize) -> Result<(Token, usize), Error> {
    let b = src.as_bytes();
    let mut j = i;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
    }
    let mut integer = true;
    // The run: digits and points, stopping before "..".
    while j < b.len() && b[j] == b'.' && b.get(j + 1) != Some(&b'.') {
        integer = false;
        j += 1;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
    }
    if j < b.len() && (b[j] == b'e' || b[j] == b'E') {
        let k = if matches!(b.get(j + 1), Some(b'+') | Some(b'-')) { j + 2 } else { j + 1 };
        if b.get(k).is_some_and(u8::is_ascii_digit) {
            j = k;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            integer = false;
        }
    }
    let text = &src[i..j];
    if text.len() > MAX_CHARS || Decimal::parse(text).is_none() {
        return Err(err(Code::Number, i, format!("not a canonical number: {text}")));
    }
    // Period keys: digits directly followed by D, M or Y.
    if integer
        && matches!(b.get(j), Some(b'D') | Some(b'M') | Some(b'Y'))
        && !b.get(j + 1).is_some_and(|&c| is_word_char(c))
    {
        return Ok((Token { kind: Kind::Period, text: src[i..j + 1].to_string(), unit: None, pos: i }, j + 1));
    }
    let mut k = j;
    while k < b.len() && (b[k] == b' ' || b[k] == b'\t') {
        k += 1;
    }
    let (unit, after) = unit_suffix(src, k)?;
    let next = match unit {
        Some(_) => after,
        None => j,
    };
    Ok((Token { kind: Kind::Number, text: text.to_string(), unit, pos: i }, next))
}

/// Since spec-0.6.0 (C3, 5.8): the end of the digit-led word at `i` in a request. The run of word
/// characters there must hold a letter, and the number lexer must not accept it whole (as a
/// number, a period, or a number with a unit covering the run). So `1e3`, which is not a canonical
/// number, is a word; a run without a letter (`01`) is never one.
fn digit_led_end(src: &str, i: usize) -> Option<usize> {
    let b = src.as_bytes();
    let mut j = i;
    while j < b.len() && is_word_char(b[j]) {
        j += 1;
    }
    if !b[i..j].iter().any(u8::is_ascii_alphabetic) {
        return None;
    }
    match number_token(src, i) {
        Ok((_, end)) if end >= j => None,
        _ => Some(j),
    }
}

/// `4DIGIT "-" 2DIGIT "-" 2DIGIT` at `i`, not preceded by a word character.
fn date_shape(b: &[u8], i: usize) -> bool {
    b.len() >= i + 10
        && b[i..i + 4].iter().all(u8::is_ascii_digit)
        && b[i + 4] == b'-'
        && b[i + 5..i + 7].iter().all(u8::is_ascii_digit)
        && b[i + 7] == b'-'
        && b[i + 8..i + 10].iter().all(u8::is_ascii_digit)
}

/// The end of a run shaped like a qualified name at `i`: host characters with at least one dot,
/// the last label not all digits, then one or more `/segment` (SPEC 15.2 L3, since `spec-0.6.1`).
///
/// A segment after the host is read as a run of letters, digits, `_`, `.` and `-`. A run that
/// contains `.` or `-` is a DNS segment only when a `/` follows it; otherwise it is the last
/// segment, a name, read as the longest prefix of word characters, so a `.` or `-` after it ends
/// the name. A run that is neither is left to `names::is_function_name`, which refuses it.
fn qualified_shape(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'-' || b[j] == b'.') {
        j += 1;
    }
    let host = &b[i..j];
    if host.split(|&c| c == b'.').any(|l| l.is_empty()) {
        return None;
    }
    if !host.contains(&b'.') || b.get(j) != Some(&b'/') {
        return None;
    }
    let last = host.rsplit(|&c| c == b'.').next().unwrap_or(&[]);
    if last.is_empty() || last.iter().all(u8::is_ascii_digit) {
        return None;
    }
    loop {
        // b[j] is the `/` before the segment.
        let start = j + 1;
        let mut k = start;
        while k < b.len() && (b[k].is_ascii_alphanumeric() || matches!(b[k], b'_' | b'.' | b'-')) {
            k += 1;
        }
        let dotted = b[start..k].iter().any(|&c| c == b'.' || c == b'-');
        if dotted && b.get(k) == Some(&b'/') {
            // A DNS segment: the whole run, checked by the caller.
            j = k;
            continue;
        }
        let mut e = start;
        while e < b.len() && is_word_char(b[e]) {
            e += 1;
        }
        if !dotted && b.get(e) == Some(&b'/') {
            // A plain name segment, not the last one.
            j = e;
            continue;
        }
        return Some(e);
    }
}

/// The end of `index:<period>=<canonical number>` at `k`, the longest canonical match.
fn index_unit_end(b: &[u8], k: usize) -> Option<usize> {
    let mut j = k + 6;
    let digits = |j: usize, n: usize| b.len() >= j + n && b[j..j + n].iter().all(u8::is_ascii_digit);
    if !digits(j, 4) {
        return None;
    }
    j += 4;
    for _ in 0..2 {
        if b.get(j) == Some(&b'-') && digits(j + 1, 2) {
            j += 3;
        }
    }
    if b.get(j) != Some(&b'=') {
        return None;
    }
    j += 1;
    let start = j;
    while j < b.len() && b[j].is_ascii_digit() {
        j += 1;
    }
    if j == start || (j - start > 1 && b[start] == b'0') {
        return None;
    }
    if b.get(j) == Some(&b'.') {
        let mut f = j + 1;
        let mut last_nonzero = None;
        while f < b.len() && b[f].is_ascii_digit() {
            if b[f] != b'0' {
                last_nonzero = Some(f);
            }
            f += 1;
        }
        if let Some(z) = last_nonzero {
            j = z + 1;
        }
    }
    Some(j)
}

/// L6: the longest listed unit at `k` not followed by a word character; a keyword is not a
/// suffix; any other word is `E_FORMULA_UNIT`.
fn unit_suffix(src: &str, k: usize) -> Result<(Option<String>, usize), Error> {
    let b = src.as_bytes();
    if k >= b.len() {
        return Ok((None, k));
    }
    let rest = &src[k..];
    // Index units.
    if rest.starts_with("index:") {
        if let Some(j) = index_unit_end(b, k) {
            let text = &src[k..j];
            if units::is_index_unit(text) && !b.get(j).is_some_and(|&c| is_word_char(c)) {
                return Ok((Some(text.to_string()), j));
            }
        }
        return Err(err(Code::Unit, k, "not a listed unit: index"));
    }
    for u in units::suffix_units() {
        if rest.starts_with(u) && !b.get(k + u.len()).is_some_and(|&c| is_word_char(c)) {
            return Ok((Some(u.to_string()), k + u.len()));
        }
    }
    if b[k].is_ascii_alphabetic() {
        let mut j = k;
        while j < b.len() && is_word_char(b[j]) {
            j += 1;
        }
        let w = &src[k..j];
        if is_keyword(w) {
            return Ok((None, k));
        }
        return Err(err(Code::Unit, k, format!("not a listed unit: {w}")));
    }
    Ok((None, k))
}
