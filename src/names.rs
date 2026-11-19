//! Names (SPEC 2.1-2.3).

/// A name (2.2): `lower *( lower / DIGIT ) *( "_" 1*( lower / DIGIT ) )`, at most 64 characters.
pub fn is_name(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 64 || !b[0].is_ascii_lowercase() {
        return false;
    }
    let mut prev_underscore = false;
    for &c in b {
        if c == b'_' {
            if prev_underscore {
                return false;
            }
            prev_underscore = true;
        } else if c.is_ascii_lowercase() || c.is_ascii_digit() {
            prev_underscore = false;
        } else {
            return false;
        }
    }
    !prev_underscore
}

/// A DNS name (2.1): two or more labels of `a-z 0-9 -`, 1-63 characters, not starting or ending
/// with `-`; at most 253 characters; the last label not all digits.
pub fn is_dns_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = s.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    for l in &labels {
        let b = l.as_bytes();
        if b.is_empty() || b.len() > 63 || b[0] == b'-' || b[b.len() - 1] == b'-' {
            return false;
        }
        if !b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-') {
            return false;
        }
    }
    !labels[labels.len() - 1].bytes().all(|c| c.is_ascii_digit())
}

/// A fully qualified function name (2.3, since `spec-0.6.1`): `dns-name *( "/" segment ) "/" name`,
/// where a segment after the host is a name or a DNS name, and the last segment is a name. At
/// most 512 characters.
pub fn is_function_name(s: &str) -> bool {
    if s.len() > 512 {
        return false;
    }
    let mut parts = s.split('/');
    let host = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    let Some((last, middle)) = rest.split_last() else {
        return false;
    };
    is_dns_name(host) && middle.iter().all(|p| is_name(p) || is_dns_name(p)) && is_name(last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        for s in ["fx_rate", "base", "a01", "v2_final"] {
            assert!(is_name(s), "{s}");
        }
        for s in ["FxRate", "_x", "x_", "a__b", "1st", ""] {
            assert!(!is_name(s), "{s}");
        }
        assert!(is_function_name("rates.example.org/fx_rate"));
        assert!(is_function_name("a-b.org/f"));
        assert!(is_function_name("example.org/central_bank/stats/fx_rate"));
        assert!(!is_function_name("Rates.Example.org/fx_rate"));
        assert!(!is_function_name("rates.example.org/fx__rate"));
        assert!(!is_function_name("1.2/f"));
        assert!(is_function_name("datumesh.com/relay/ecb.europa.eu/fx_reference"));
        assert!(is_function_name("datumesh.com/relay/bank-x.example/rate"));
        assert!(is_function_name("datumesh.com/relay/xn--bcher-kva.example/rate"));
        assert!(is_function_name("datumesh.com/relay/ecb.europa.eu/v2.example/rate"));
        assert!(!is_function_name("datumesh.com/relay/ECB.europa.eu/rate"));
        assert!(!is_function_name("datumesh.com/relay/ecb.europa.eu./rate"));
        assert!(!is_function_name("datumesh.com/relay/ecb_x.europa.eu/rate"));
        assert!(!is_function_name("datumesh.com/relay/ecb..eu/rate"));
        assert!(!is_function_name("datumesh.com/relay/-ecb.europa.eu/rate"));
        assert!(!is_function_name("datumesh.com/relay/10.0.0.1/rate"));
        assert!(!is_function_name("datumesh.com/relay/bank-x/rate"));
        assert!(!is_function_name("datumesh.com/relay/ecb.europa.eu"));
        assert!(!is_function_name("datumesh.com/relay/ecb.europa.eu/rate.x"));
        assert!(!is_function_name("datumesh.com/relay/ecb.europa.eu/rate-x"));
    }
}
