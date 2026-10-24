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
    Number,
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

/// Lex the whole body.
pub(crate) fn lex(src: &str) -> Result<Vec<Token>, Error> {
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
                let mut j = i + 10;
                while j < b.len() && (is_word_char(b[j]) || b[j] == b':' || b[j] == b'.') {
                    j += 1;
                }
                let text = &src[i..j];
                if j - i != 27 || parse_instant(text).is_none() {
                    return Err(err(Code::Instant, i, format!("not a canonical instant: {text}")));
                }
                out.push(Token { kind: Kind::Instant, text: text.to_string(), unit: None, pos: start });
                i = j;
                continue;
            }
            let text = &src[i..i + 10];
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
                i = end;
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
            i = j;
            continue;
        }
        // L5, L6: a number and its unit suffix.
        if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let mut j = i;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let mut integer = true;
            if j < b.len() && b[j] == b'.' && b.get(j + 1) != Some(&b'.') {
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
                return Err(err(Code::Reserved, i, format!("{} is reserved for period keys (1.3b)", &src[i..j + 1])));
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
            out.push(Token { kind: Kind::Number, text: text.to_string(), unit, pos: start });
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
/// the last label not all digits, then one or more `/segment`.
fn qualified_shape(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'-' || b[j] == b'.') {
        j += 1;
    }
    let host = &b[i..j];
    if !host.contains(&b'.') || b.get(j) != Some(&b'/') || !b.get(j + 1).is_some_and(|&c| is_word_char(c)) {
        return None;
    }
    let last = host.rsplit(|&c| c == b'.').next().unwrap_or(&[]);
    if last.is_empty() || last.iter().all(u8::is_ascii_digit) {
        return None;
    }
    while j < b.len() && b[j] == b'/' && b.get(j + 1).is_some_and(|&c| is_word_char(c)) {
        j += 1;
        while j < b.len() && is_word_char(b[j]) {
            j += 1;
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
        let mut j = k + 6;
        while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'-' || b[j] == b'=' || b[j] == b'.') {
            j += 1;
        }
        let text = &src[k..j];
        if units::is_index_unit(text) && !b.get(j).is_some_and(|&c| is_word_char(c)) {
            return Ok((Some(text.to_string()), j));
        }
        return Err(err(Code::Unit, k, format!("not a listed unit: {text}")));
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
