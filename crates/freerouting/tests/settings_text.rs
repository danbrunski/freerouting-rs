//! Differential check of the Java text conventions the settings use
//! (`Double.toString`, `Float.toString`, `Double.parseDouble`,
//! `Float.parseFloat`, `Integer.parseInt`, `Long.parseLong` and
//! `TextManager.parseTimespanString`) against Java 25.
//!
//! `data/settings/java_text_probe.tsv` was written by
//! `data/settings/JavaTextProbe.java.txt` (compile it as `Probe.java`) with
//! the jdk4py runtime of `tools/java-reference`; the expected values are
//! Java's, not reasoned. These are JDK semantics, so the probe does not
//! need the pinned Freerouting classes.

use std::path::Path;

use freerouting::settings::text::{
    java_double_to_string, java_float_to_string, parse_double, parse_float, parse_int, parse_long,
};
use freerouting::settings::timespan::parse_timespan_seconds;

fn unhex(hex: &str) -> String {
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
        .collect();
    String::from_utf8(bytes).unwrap()
}

/// Strings Java's integer parsers accept but ours do not, with the reason.
const EXPECTED_MISMATCHES: &[(&str, &str)] = &[
    (
        "٣",
        "Integer.parseInt accepts Unicode decimal digits; ours takes ASCII only",
    ),
    (
        "0x1p3",
        "Java reads hexadecimal floating point literals; ours does not",
    ),
    (
        "0x1.8p1",
        "Java reads hexadecimal floating point literals; ours does not",
    ),
];

#[test]
fn text_conventions_match_java() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/settings/java_text_probe.tsv");
    let text = std::fs::read_to_string(path).unwrap();
    let mut counts = [0usize; 4];
    let mut failures = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        match cols[0] {
            "D" => {
                let value = f64::from_bits(u64::from_str_radix(cols[1], 16).unwrap());
                let got = java_double_to_string(value);
                if got != cols[2] {
                    failures.push(format!(
                        "Double.toString({:#x}): Java {} / Rust {got}",
                        value.to_bits(),
                        cols[2]
                    ));
                }
                counts[0] += 1;
            }
            "F" => {
                let value = f32::from_bits(u32::from_str_radix(cols[1], 16).unwrap());
                let got = java_float_to_string(value);
                if got != cols[2] {
                    failures.push(format!(
                        "Float.toString({:#x}): Java {} / Rust {got}",
                        value.to_bits(),
                        cols[2]
                    ));
                }
                counts[1] += 1;
            }
            "P" => {
                let input = unhex(cols[1]);
                let bits =
                    |r: Result<u64, String>| r.map_or("ERR".to_string(), |b| format!("{b:x}"));
                let d = bits(parse_double(&input).map(|v| v.to_bits()));
                let f = bits(parse_float(&input).map(|v| u64::from(v.to_bits())));
                let i = parse_int(&input).map_or("ERR".to_string(), |v| v.to_string());
                let l = parse_long(&input).map_or("ERR".to_string(), |v| v.to_string());
                let known = EXPECTED_MISMATCHES.iter().any(|(s, _)| *s == input);
                // Java prints NaN with its canonical bits; any NaN compares equal.
                let nan = |s: &str, width: usize| {
                    let canonical = if width == 64 {
                        "7ff8000000000000"
                    } else {
                        "7fc00000"
                    };
                    s == canonical
                };
                let same = |java: &str, rust: &str, width: usize| {
                    java == rust || (nan(java, width) && nan(rust, width))
                };
                let ok =
                    same(cols[2], &d, 64) && same(cols[3], &f, 32) && cols[4] == i && cols[5] == l;
                if ok == known {
                    failures.push(format!(
                        "parse {input:?}: Java {} {} {} {} / Rust {d} {f} {i} {l}",
                        cols[2], cols[3], cols[4], cols[5]
                    ));
                }
                counts[2] += 1;
            }
            "T" => {
                let input = unhex(cols[1]);
                let got =
                    parse_timespan_seconds(&input).map_or("NULL".to_string(), |s| s.to_string());
                if got != cols[2] {
                    failures.push(format!(
                        "parseTimespanString({input:?}): Java {} / Rust {got}",
                        cols[2]
                    ));
                }
                counts[3] += 1;
            }
            other => panic!("unknown probe line {other}"),
        }
    }
    eprintln!("compared {counts:?} (double, float, parse, timespan) probe lines");
    assert!(counts.iter().all(|&c| c > 20), "{counts:?}");
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
