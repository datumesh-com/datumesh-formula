//! Every case of every `decimal` vector (SPEC 11, 15.9), to the last digit.

mod common;

use datumesh_formula::decimal;

#[test]
fn decimal_vectors() {
    let mut cases = 0;
    let mut files = 0;
    let mut failures = Vec::new();
    for (path, v) in common::vectors_of_kind("decimal") {
        files += 1;
        let op = v.get("op").and_then(|x| x.as_str()).unwrap().to_string();
        let list = common::array(v.get("cases").unwrap());
        let results = common::array(v.get("results").unwrap());
        assert_eq!(list.len(), results.len(), "{path}");
        let mut algorithm_vs_mpdecimal = 0;
        if let Some(mp) = v.get("mpdecimal") {
            for (r, m) in results.iter().zip(common::array(mp)) {
                // A hard case: the algorithm's result differs from the correctly rounded one
                // (faults are not compared: mpdecimal has no range rule).
                if let (Some(m), Some(r)) = (m.as_str(), r.as_str()) {
                    if !r.starts_with("E_") && m != r {
                        algorithm_vs_mpdecimal += 1;
                    }
                }
            }
            let hard = common::integer(v.get("hard_cases").unwrap());
            assert_eq!(hard, algorithm_vs_mpdecimal, "{path}: hard_cases");
        }
        for (case, expected) in list.iter().zip(results) {
            cases += 1;
            let args: Vec<&str> = common::array(case).iter().map(|a| a.as_str().unwrap()).collect();
            let got = match decimal::apply(&op, &args) {
                Ok(s) => s,
                Err(code) => code.as_str().to_string(),
            };
            if got != expected.as_str().unwrap() {
                failures.push(format!("{path} {op}{args:?}: got {got}, expected {}", expected.as_str().unwrap()));
            }
        }
    }
    assert_eq!(files, 20);
    eprintln!("decimal vectors: {files} files, {cases} cases, {} failures", failures.len());
    for f in failures.iter().take(40) {
        eprintln!("  {f}");
    }
    assert!(failures.is_empty());
}
