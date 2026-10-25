//! The brief's tests 6 to 9 (step 1.3), through the engine with the formula runtime on
//! PostgreSQL.
//!
//! - 6, self-reference: `fib(90)` in formula is the exact decimal, every `fib(k)` one memoised
//!   cell; a curve bootstrapped over ten tenors evaluates, each tenor one memoised cell.
//! - 7, aggregates and slice-as-lineage: a weighted mean over 1,000 stations has two slice
//!   entries; its receipt verifies at `recomputed`; a correction to one station dirties the
//!   mean; the old receipt still verifies at `signed`, its recomputation reporting
//!   `E_INPUT_SUPERSEDED`; the member set is fetchable from the slice entry.
//! - 8, lookups: `last_before` with a staleness bound over unsynchronised sensors gives the
//!   right reading or `not_covered` with `E_FORMULA_ABSENT`; `latest` is a pure function of the
//!   slice and the asof.
//! - 9, scans: a running total over 3,650 daily values is one cell per day with one slice entry,
//!   not a recurrence chain; a correction to a day dirties only the days on or after it.
#![cfg(feature = "engine")]

mod common;
#[path = "common/pg.rs"]
mod pg;

use datumesh_core::{verify, ErrorCode, Fault, Level, Manifest, Receipt, Resolver};
use datumesh_engine::Answer;
use datumesh_store::Store;
use pg::{cell_ref, date, instant, num, receipt, text, Mesh};

const T0: &str = "2026-01-01T00:00:00.000000Z";
const A: &str = "2026-06-01T00:00:00.000000Z";

const N1: &str = r#"{"type":"number","unit":"1","valid":false}"#;

/// The receipt verifies at recomputed with the engine as resolver; it joins the cross-check set.
fn recomputes(m: &Mesh, engine: &datumesh_engine::Engine, bytes: &[u8]) {
    let r = verify(bytes, &m.keys, Some(engine), None);
    assert_eq!(r.level, Some(Level::Recomputed), "{:?}", r.error());
    assert_eq!(r.code, None);
    m.dump("brief", bytes);
}

// ---------------------------------------------------------------------------------------------
// 6. Self-reference.

#[test]
fn fib_90_is_exact_and_every_fib_is_one_memoised_cell() {
    let m = Mesh::new(&[]);
    let sig = format!(r#"{{"args":{{"n":{N1}}},"order":["n"],"returns":{{"type":"number","unit":"1"}}}}"#);
    let d = m.define(
        "calc.example.net/fib",
        "IF(n < 2, n, calc.example.net/fib(n - 1) + calc.example.net/fib(n - 2))",
        &sig,
        T0,
    );
    assert_eq!(d.descriptor.inputs, ["calc.example.net/fib"]);
    let (engine, rt) = m.engine();
    let fib = |n: &str| cell_ref("calc.example.net/fib", &[("n", num(n))]);
    let (bytes, r) = receipt(engine.get(&fib("90"), Some(A)));
    assert_eq!(r.value().lexical(), "2880067194370816120");
    // fib(0) .. fib(90): 91 cells, each evaluated once and memoised.
    assert_eq!(rt.stats().completed, 91, "{:?}", rt.stats());
    assert!(engine.memo_stats().cells >= 91, "{:?}", engine.memo_stats());
    // Its inputs are the receipts of fib(88) and fib(89).
    let (_, r89) = receipt(engine.get(&fib("89"), Some(A)));
    let (_, r88) = receipt(engine.get(&fib("88"), Some(A)));
    assert_eq!(r89.value().lexical(), "1779979416004714189");
    let mut want = [r88.hash().to_string(), r89.hash().to_string()];
    want.sort();
    assert_eq!(r.inputs(), &want[..]);
    assert_eq!(rt.stats().completed, 91, "answered from the memo");
    let (again, _) = receipt(engine.get(&fib("90"), Some(A)));
    assert_eq!(again, bytes);
    recomputes(&m, &engine, &bytes);
    // A fresh engine validates the stored versions instead of evaluating.
    let (fresh, rt2) = m.engine();
    let (b2, _) = receipt(fresh.get(&fib("90"), Some(A)));
    assert_eq!(b2, bytes);
    assert_eq!(rt2.stats().completed, 0, "{:?}", rt2.stats());
}

#[test]
fn a_bootstrapped_curve_over_ten_tenors() {
    let m = Mesh::new(&["rates.example.org"]);
    let par_sig = format!(r#"{{"args":{{"tenor":{N1}}},"order":["tenor"],"returns":{{"type":"number","unit":"1"}}}}"#);
    m.stored("rates.example.org/par", &par_sig, T0);
    let pars = ["0.03", "0.032", "0.0335", "0.0347", "0.0358", "0.0366", "0.0373", "0.0379", "0.0384", "0.0388"];
    for (i, p) in pars.iter().enumerate() {
        let tx = format!("2026-01-02T00:00:{:02}.000000Z", i);
        m.record("rates.example.org/par", &[("tenor", num(&(i + 1).to_string()))], num(p), &tx, &tx);
    }
    // The discount factor of tenor t: (1 - par(t) * sum of the earlier factors) / (1 + par(t)).
    let df = "IF(tenor = 1, 1 / (1 + rates.example.org/par(tenor)), \
              (1 - rates.example.org/par(tenor) * SUM(calc.example.net/df([1..tenor - 1]))) / (1 + rates.example.org/par(tenor)))";
    let d = m.define("calc.example.net/df", df, &par_sig, T0);
    assert_eq!(d.descriptor.inputs, ["calc.example.net/df", "rates.example.org/par"]);
    // Each factor prices its par swap at par: par(t) * (df(1) + ... + df(t)) + df(t) = 1.
    let check_sig = format!(r#"{{"args":{{"tenor":{N1}}},"order":["tenor"],"returns":"boolean"}}"#);
    m.define(
        "calc.example.net/at_par",
        "ABS(rates.example.org/par(tenor) * SUM(calc.example.net/df([1..tenor])) + calc.example.net/df(tenor) - 1) < 0.000000000000000000000000000001",
        &check_sig,
        T0,
    );
    let (engine, rt) = m.engine();
    let (b10, r10) = receipt(engine.get(&cell_ref("calc.example.net/df", &[("tenor", num("10"))]), Some(A)));
    assert_eq!(rt.stats().completed, 10, "each tenor evaluated once: {:?}", rt.stats());
    assert!(engine.memo_stats().cells >= 20, "{:?}", engine.memo_stats());
    assert!(r10.value().lexical().starts_with("0.6"), "df(10) = {}", r10.value().lexical());
    // df(10) reads one slice entry (df(1..9)) and one par receipt.
    assert_eq!(r10.inputs().iter().filter(|i| i.starts_with("slice:")).count(), 1);
    assert_eq!(r10.inputs().len(), 2);
    recomputes(&m, &engine, &b10);
    for t in 1..=10 {
        let tenor = num(&t.to_string());
        let (b, _) = receipt(engine.get(&cell_ref("calc.example.net/df", &[("tenor", tenor.clone())]), Some(A)));
        recomputes(&m, &engine, &b);
        let (_, ok) = receipt(engine.get(&cell_ref("calc.example.net/at_par", &[("tenor", tenor)]), Some(A)));
        assert_eq!(ok.value().lexical(), "true", "tenor {t}");
    }
    // The factors were not evaluated again: ten df cells, ten at_par cells, and the eleven
    // recomputations of the verifications (the engine as resolver evaluates the cell itself,
    // its inputs from the memo).
    assert_eq!(rt.stats().completed, 10 + 10 + 11, "{:?}", rt.stats());
}

// ---------------------------------------------------------------------------------------------
// 7. Aggregates and slice-as-lineage.

#[test]
fn a_weighted_mean_over_1000_stations() {
    let mut m = Mesh::new(&["weather.example.org"]);
    m.add_host("weather.example.org");
    let st = r#"{"type":"string","valid":false}"#;
    m.stored(
        "weather.example.org/temp",
        &format!(r#"{{"args":{{"station":{st}}},"order":["station"],"returns":{{"type":"number","unit":"K"}}}}"#),
        T0,
    );
    m.stored(
        "weather.example.org/weight",
        &format!(r#"{{"args":{{"station":{st}}},"order":["station"],"returns":{{"type":"number","unit":"1"}}}}"#),
        T0,
    );
    let station = |i: usize| text(&format!("s{i:04}"));
    let tx = |i: usize| format!("2026-03-01T{:02}:{:02}:{:02}.000000Z", i / 3600, (i / 60) % 60, i % 60);
    let (mut sum_tw, mut sum_w) = (0i64, 0i64);
    for i in 0..1000 {
        let t = 27315 + (i as i64 * 37) % 1500; // hundredths of a kelvin
        let w = 1 + (i as i64 * 7) % 5;
        sum_tw += t * w;
        sum_w += w;
        let temp = format!("{}.{:02}", t / 100, t % 100);
        m.record(
            "weather.example.org/temp",
            &[("station", station(i))],
            num(temp.trim_end_matches('0').trim_end_matches('.')),
            &tx(i),
            &tx(i),
        );
        m.record("weather.example.org/weight", &[("station", station(i))], num(&w.to_string()), &tx(i), &tx(i));
    }
    let body = "SUM(weather.example.org/temp(*) * weather.example.org/weight(*)) / SUM(weather.example.org/weight(*))";
    let sig = r#"{"args":{"region":{"type":"string","valid":false}},"order":["region"],"returns":{"type":"number","unit":"K"}}"#;
    m.define("calc.example.net/mean", body, sig, T0);
    let (engine, rt) = m.engine();
    let mean = cell_ref("calc.example.net/mean", &[("region", text("uk"))]);
    let (old, r) = receipt(engine.get(&mean, Some(A)));
    // The value: sum(t * w) / sum(w), exact here to 34 digits.
    let want = {
        // sum_tw / (100 * sum_w), at 34 significant digits, half-even: done in integers.
        let q = datumesh_formula::decimal::Decimal::parse(&sum_tw.to_string()).unwrap();
        let dvs = datumesh_formula::decimal::Decimal::parse(&(100 * sum_w).to_string()).unwrap();
        datumesh_formula::decimal::div(&q, &dvs).unwrap().to_canonical()
    };
    assert_eq!(r.value().lexical(), want);
    // Two slice entries, nothing else.
    assert_eq!(r.inputs().len(), 2, "{:?}", r.inputs());
    assert!(r.inputs().iter().all(|i| i.starts_with("slice:")));
    recomputes(&m, &engine, &old);
    // The member set from the slice entry: the manifest from the store, the members as receipts.
    for entry in r.inputs() {
        let bytes = m.store.manifest(entry).expect("manifest stored");
        let man = Manifest::from_json(&bytes).unwrap();
        assert_eq!(&man.entry(), entry);
        assert_eq!(man.members().len(), 1000);
        assert_eq!(entry.split(':').nth(1), Some(man.slice().id()));
        for h in man.members().iter().take(50) {
            let rb = m.store.receipt(h).expect("member receipt");
            let mr = Receipt::from_json(&rb).unwrap();
            assert!(man.slice().contains(&mr));
        }
    }
    // A correction to one station, recorded before the latest reading (backdated relative to the
    // mean's tx): it dirties the mean.
    let before = rt.stats().completed;
    let corr_tx = "2026-03-01T00:08:20.500000Z";
    let s0 = cell_ref("weather.example.org/temp", &[("station", station(0))]);
    m.record("weather.example.org/temp", &[("station", station(0))], num("400"), corr_tx, corr_tx);
    let s0_id = {
        let h = m.store.resolve_name("weather.example.org/temp", T0).unwrap().unwrap();
        datumesh_core::Cell { ns: s0.ns.clone(), fn_name: s0.fn_name.clone(), fn_hash: h, args: s0.args.clone() }.id()
    };
    engine.stored_version_added(&s0_id);
    let (new, r2) = receipt(engine.get(&mean, Some(A)));
    assert_eq!(rt.stats().completed, before + 1, "the mean evaluated again");
    assert_ne!(new, old);
    assert_ne!(r2.value(), r.value());
    let weights = r.inputs().iter().filter(|e| r2.inputs().contains(e)).count();
    assert_eq!(weights, 1, "the weights' entry is unchanged, the temperatures' is not");
    recomputes(&m, &engine, &new);
    // The old receipt still verifies (signed); recomputed, its input is superseded.
    let signed = verify(&old, &m.keys, None, None);
    assert!(signed.ok);
    assert_eq!(signed.level, Some(Level::Signed));
    let re = verify(&old, &m.keys, Some(&engine), None);
    assert!(re.ok, "{:?}", re.error());
    assert_eq!(re.level, Some(Level::Signed));
    assert_eq!(re.code, Some(ErrorCode::InputSuperseded));
    // The engine's lineage of the recomputation names the new temperatures' entry.
    let cell = r.cell().clone();
    let ev = engine.evaluation(&cell, r.asof()).unwrap();
    assert_eq!(ev.lineage.as_deref(), Some(r2.inputs()));
}

// ---------------------------------------------------------------------------------------------
// 8. Lookups: the weather example of the decision log.

const TEMP_SIG: &str = r#"{"args":{"station":{"type":"string","valid":false},"t":{"type":"instant","valid":true}},"order":["station","t"],"returns":{"type":"number","unit":"degC"}}"#;

fn sensors() -> Mesh {
    let mut m = Mesh::new(&["weather.example.org"]);
    m.add_host("weather.example.org");
    m.stored("weather.example.org/temp", TEMP_SIG, T0);
    // Unsynchronised sensors: each reading recorded two seconds after it was taken.
    let readings = [
        ("kew", "2026-03-01T10:00:13.000000Z", "11.2"),
        ("ely", "2026-03-01T10:03:30.000000Z", "8.9"),
        ("kew", "2026-03-01T10:09:47.000000Z", "11.6"),
        ("ely", "2026-03-01T10:14:55.000000Z", "9.3"),
        ("kew", "2026-03-01T10:21:05.000000Z", "12.1"),
    ];
    for (s, t, v) in readings {
        let tx = recorded(t);
        m.record("weather.example.org/temp", &[("station", text(s)), ("t", instant(t))], num(v), &tx, &tx);
    }
    m
}

/// Two seconds after `t` (its seconds below 58).
fn recorded(t: &str) -> String {
    let s: u32 = t[17..19].parse().unwrap();
    format!("{}{:02}{}", &t[..17], s + 2, &t[19..])
}

#[test]
fn last_before_with_a_staleness_bound() {
    let m = sensors();
    let sig = r#"{"args":{"station":{"type":"string","valid":false},"t":{"type":"instant","valid":true}},"order":["station","t"],"returns":{"type":"number","unit":"degC"}}"#;
    m.define("calc.example.net/reading", "LAST_BEFORE(weather.example.org/temp(station, *), t, 10 min)", sig, T0);
    let (engine, _) = m.engine();
    let at = |s: &str, t: &str| {
        engine.get(&cell_ref("calc.example.net/reading", &[("station", text(s)), ("t", instant(t))]), Some(A))
    };
    let (b, r) = receipt(at("kew", "2026-03-01T10:15:00.000000Z"));
    assert_eq!(r.value().lexical(), "11.6");
    // The lineage is the narrowed slice: kew's readings in [t - 10 min..t], one member.
    assert_eq!(r.inputs().len(), 1);
    let man = Manifest::from_json(&m.store.manifest(&r.inputs()[0]).unwrap()).unwrap();
    assert_eq!(man.members().len(), 1);
    recomputes(&m, &engine, &b);
    assert_eq!(receipt(at("ely", "2026-03-01T10:15:00.000000Z")).1.value().lexical(), "9.3");
    assert_eq!(receipt(at("kew", "2026-03-01T10:09:47.000000Z")).1.value().lexical(), "11.6", "at or before");
    assert_eq!(receipt(at("ely", "2026-03-01T10:12:00.000000Z")).1.value().lexical(), "8.9");
    // kew at 10:20: its last reading is 10m13s old, past the bound: no value, with the fault.
    assert_eq!(at("kew", "2026-03-01T10:20:00.000000Z").unwrap(), Answer::NotCovered { fault: Some(Fault::Absent) });
    // Before any reading.
    assert_eq!(at("ely", "2026-03-01T10:00:00.000000Z").unwrap(), Answer::NotCovered { fault: Some(Fault::Absent) });
}

#[test]
fn latest_is_a_pure_function_of_the_slice_and_the_asof() {
    let m = sensors();
    let sig = r#"{"args":{"station":{"type":"string","valid":false}},"order":["station"],"returns":{"type":"number","unit":"degC"}}"#;
    m.define("calc.example.net/last", "LATEST(weather.example.org/temp(station, *))", sig, T0);
    m.define("calc.example.net/fresh", "LATEST(weather.example.org/temp(station, *), 15 min)", sig, T0);
    let kew = [("station", text("kew"))];
    let a1 = "2026-03-01T10:12:00.000000Z";
    let a2 = "2026-03-01T10:30:00.000000Z";
    let a3 = "2026-03-01T10:45:00.000000Z";
    let (engine, _) = m.engine();
    // Unbounded: the last reading known as of the asof.
    let (b1, r1) = receipt(engine.get(&cell_ref("calc.example.net/last", &kew), Some(a1)));
    let (b2, r2) = receipt(engine.get(&cell_ref("calc.example.net/last", &kew), Some(a2)));
    assert_eq!(r1.value().lexical(), "11.6");
    assert_eq!(r2.value().lexical(), "12.1");
    assert_ne!(r1.inputs(), r2.inputs(), "different member sets, different entries");
    // The same slice and asof give the same receipt, from a fresh engine too.
    let (fresh, _) = m.engine();
    assert_eq!(receipt(fresh.get(&cell_ref("calc.example.net/last", &kew), Some(a1))).0, b1);
    assert_eq!(receipt(fresh.get(&cell_ref("calc.example.net/last", &kew), Some(a2))).0, b2);
    recomputes(&m, &engine, &b1);
    recomputes(&m, &engine, &b2);
    // Bounded: asof-dependent, the receipt's asof is the request's; past the bound, absent.
    let (f2, r) = receipt(engine.get(&cell_ref("calc.example.net/fresh", &kew), Some(a2)));
    assert_eq!(r.value().lexical(), "12.1");
    assert_eq!(r.asof(), a2);
    recomputes(&m, &engine, &f2);
    assert_eq!(
        engine.get(&cell_ref("calc.example.net/fresh", &kew), Some(a3)).unwrap(),
        Answer::NotCovered { fault: Some(Fault::Absent) }
    );
    // ely's last reading (10:14:55) is within 15 minutes of 10:20, not of 10:30.
    let ely = [("station", text("ely"))];
    let a4 = "2026-03-01T10:20:00.000000Z";
    let (_, r) = receipt(engine.get(&cell_ref("calc.example.net/fresh", &ely), Some(a4)));
    assert_eq!(r.value().lexical(), "9.3");
    assert_eq!(r.asof(), a4);
    assert_eq!(
        engine.get(&cell_ref("calc.example.net/fresh", &ely), Some(a2)).unwrap(),
        Answer::NotCovered { fault: Some(Fault::Absent) }
    );
}

// ---------------------------------------------------------------------------------------------
// 9. Scans: a running total.

#[test]
fn a_running_total_over_3650_days() {
    let mut m = Mesh::new(&["data.example.org"]);
    m.add_host("data.example.org");
    let dsig =
        r#"{"args":{"date":{"type":"date","valid":true}},"order":["date"],"returns":{"type":"number","unit":"EUR"}}"#;
    m.stored("data.example.org/daily", dsig, T0);
    let days: Vec<String> = (0..3650).map(day).collect();
    let mut cents = Vec::new();
    for (i, d) in days.iter().enumerate() {
        let c = 100 + (i as i64 * 37) % 900; // 1.00 .. 9.99 EUR
        cents.push(c);
        let tx = format!("{d}T23:00:00.000000Z");
        m.record("data.example.org/daily", &[("date", date(d))], num(&eur(c)), &tx, &tx);
    }
    let sig = r#"{"args":{"d":{"type":"date","valid":true}},"order":["d"],"returns":{"type":"number","unit":"EUR"}}"#;
    let def = m.define("calc.example.net/running", "SUM(data.example.org/daily([2016-01-01..d]))", sig, T0);
    assert_eq!(def.descriptor.inputs, ["data.example.org/daily"], "reads the daily values only, never itself");
    let (engine, rt) = m.engine();
    let sample: Vec<usize> = (0..3650).step_by(365).chain([1, 999, 1000, 3649]).collect();
    let running = |i: usize| cell_ref("calc.example.net/running", &[("d", date(&days[i]))]);
    let a1 = "2026-03-01T00:00:00.000000Z";
    let mut first = Vec::new();
    for &i in &sample {
        let (b, r) = receipt(engine.get(&running(i), Some(a1)));
        assert_eq!(r.value().lexical(), eur(cents[..=i].iter().sum()), "day {i}");
        // One cell per day, one slice entry: the days up to it, every member a daily receipt.
        assert_eq!(r.inputs().len(), 1);
        assert!(r.inputs()[0].starts_with("slice:"));
        let man = Manifest::from_json(&m.store.manifest(&r.inputs()[0]).unwrap()).unwrap();
        assert_eq!(man.members().len(), i + 1);
        first.push(b);
    }
    assert_eq!(rt.stats().completed as usize, sample.len());
    recomputes(&m, &engine, &first[sample.iter().position(|&i| i == 1000).unwrap()]);
    let correct = |m: &Mesh, i: usize, c: i64, tx: &str| {
        m.record("data.example.org/daily", &[("date", date(&days[i]))], num(&eur(c)), tx, tx);
        let h = m.store.resolve_name("data.example.org/daily", T0).unwrap().unwrap();
        let cell = cell_ref("data.example.org/daily", &[("date", date(&days[i]))]);
        let id = datumesh_core::Cell { ns: cell.ns, fn_name: cell.fn_name, fn_hash: h, args: cell.args }.id();
        engine.stored_version_added(&id);
    };
    // A correction to day 1000: the days on or after it are evaluated again, the others not.
    correct(&m, 1000, cents[1000] + 1, "2026-04-01T00:00:00.000000Z");
    cents[1000] += 1;
    let a2 = "2026-04-02T00:00:00.000000Z";
    let before = rt.stats().completed;
    for (k, &i) in sample.iter().enumerate() {
        let (b, r) = receipt(engine.get(&running(i), Some(a2)));
        assert_eq!(r.value().lexical(), eur(cents[..=i].iter().sum()), "day {i}");
        if i < 1000 {
            assert_eq!(b, first[k], "day {i} is before the correction: the same receipt");
        } else {
            assert_ne!(b, first[k], "day {i}");
        }
    }
    let after = sample.iter().filter(|&&i| i >= 1000).count() as u64;
    assert_eq!(rt.stats().completed - before, after, "only the days on or after the correction");
    // A correction to day one dirties every day (all are on or after it), and nothing else.
    correct(&m, 0, cents[0] + 2, "2026-05-01T00:00:00.000000Z");
    cents[0] += 2;
    let a3 = "2026-05-02T00:00:00.000000Z";
    let before = rt.stats().completed;
    for &i in &sample {
        let (_, r) = receipt(engine.get(&running(i), Some(a3)));
        assert_eq!(r.value().lexical(), eur(cents[..=i].iter().sum()), "day {i}");
    }
    assert_eq!((rt.stats().completed - before) as usize, sample.len());
}

fn day(i: usize) -> String {
    // 2016-01-01 plus i days.
    let mut d = datumesh_formula::calendar::Date::parse("2016-01-01").unwrap();
    d = datumesh_formula::calendar::Date::from_ordinal(d.ordinal() + i as i64).unwrap();
    d.text()
}

fn eur(c: i64) -> String {
    let s = format!("{}.{:02}", c / 100, c % 100);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
