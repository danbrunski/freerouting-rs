//! C05.1: the Java-layout session serializer (`freerouting_dsn::write::java_style`).
//!
//! * `route_sessions_round_trip_to_their_own_bytes`: every non-empty
//!   `fixtures/reference/route/*.ses`, which Java's `SesWriter` wrote, gives back exactly its
//!   own bytes from `Session::parse` and `write_session_java`.
//! * Ports of `IdentifierTypeTest` (`identifier_type_write`) and of
//!   `SesRoundTripTest.placementRotationFormattingMatchesKicadStyle`.
//! * `java_double_matches_java`, `java_plain_decimal_matches_java` and
//!   `java_ident_matches_java`: tables printed by `JavaNumberFormatProbe.java` at the pin:
//!
//!   ```text
//!   tools/java-reference/probe.sh <pin-checkout> tools/java-reference/probes/JavaNumberFormatProbe.java
//!   ```
//!
//! Java tests not ported: none of `IdentifierTypeTest` (it has one test).

use std::path::PathBuf;

use freerouting_dsn::Session;
use freerouting_dsn::write::java_style::{
    java_double, java_ident, java_plain_decimal, write_session_java,
};

const DOUBLES: &[(u64, &str, &str)] = &[
    (0x0, "0.0", "0"),
    (0x8000000000000000, "-0.0", "0"),
    (0x3ff0000000000000, "1.0", "1"),
    (0xbff0000000000000, "-1.0", "-1"),
    (0x4024000000000000, "10.0", "10"),
    (0x4059000000000000, "100.0", "100"),
    (0x4075300000000000, "339.0", "339"),
    (0x4075280000000000, "338.5", "338.5"),
    (0x4066800000000000, "180.0", "180"),
    (0x3fe0000000000000, "0.5", "0.5"),
    (0xc046a00000000000, "-45.25", "-45.25"),
    (0x402880102e84fa5b, "12.2501234567", "12.2501234567"),
    (0x3f50624dd2f1a9fc, "0.001", "0.001"),
    (
        0x3f50624dd2f1a9fb,
        "9.999999999999998E-4",
        "0.0009999999999999998",
    ),
    (0x3f1a36e2eb1c432d, "1.0E-4", "0.0001"),
    (0x3ee4f8b588e368f1, "1.0E-5", "0.00001"),
    (0x40fe240b33333333, "123456.7", "123456.7"),
    (0x412e847fcccccccd, "999999.9", "999999.9"),
    (0x416312cfe0000000, "9999999.0", "9999999"),
    (0x416312cfffffffff, "9999999.999999998", "9999999.999999998"),
    (0x416312d000000000, "1.0E7", "10000000"),
    (0x419d6eca40000000, "1.2345E8", "123450000"),
    (0x417059ca80000000, "1.7145E7", "17145000"),
    (
        0x4172c5e87fffffff,
        "1.9684999999999996E7",
        "19684999.999999996",
    ),
    (0xc120473000000000, "-533400.0", "-533400"),
    (0x412cae3000000000, "939800.0", "939800"),
    (0x414360f000000000, "2540000.0", "2540000"),
    (0x44b52d02c7e14af6, "1.0E23", "100000000000000000000000"),
    (0x3ddb7cdfd9d7bdbb, "1.0E-10", "0.0000000001"),
    (
        0x1,
        "4.9E-324",
        "0.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000049",
    ),
    (
        0x2,
        "9.9E-324",
        "0.0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000099",
    ),
    (
        0x10000000000000,
        "2.2250738585072014E-308",
        "0.000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000022250738585072014",
    ),
    (
        0xfffffffffffff,
        "2.225073858507201E-308",
        "0.00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000002225073858507201",
    ),
    (
        0x7fefffffffffffff,
        "1.7976931348623157E308",
        "179769313486231570000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    ),
    (0x3eff75104d551d69, "3.0E-5", "0.00003"),
    (0x3fb999999999999a, "0.1", "0.1"),
    (0x3fc999999999999a, "0.2", "0.2"),
    (
        0x3fd3333333333334,
        "0.30000000000000004",
        "0.30000000000000004",
    ),
    (0x444b1ae4d6e2ef50, "1.0E21", "1000000000000000000000"),
    (0x419d6f3454000000, "1.23456789E8", "123456789"),
    (0x4011666666666666, "4.35", "4.35"),
    (0x4005666666666666, "2.675", "2.675"),
    (
        0xe,
        "6.9E-323",
        "0.000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000069",
    ),
    (
        0x3,
        "1.5E-323",
        "0.000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000015",
    ),
    (0x40f86a0000000000, "100000.0", "100000"),
    (
        0x3f50624dd2f1a9fd,
        "0.0010000000000000002",
        "0.0010000000000000002",
    ),
    (0x447c7e83209e90b2, "8.41E21", "8410000000000000000000"),
    (0x4480f0cf064dd592, "1.0E22", "10000000000000000000000"),
    (
        0x4340000000000000,
        "9.007199254740992E15",
        "9007199254740992",
    ),
    (0x4050000000000000, "64.0", "64"),
    (0x407528000002af32, "338.50000001", "338.50000001"),
    (0x3f40624dd2f1a9fc, "5.0E-4", "0.0005"),
];

const IDENTS_DOUBLE_QUOTE: &[(&str, &str)] = &[
    ("600", "\"600\""),
    ("-600", "\"-600\""),
    ("test", "test"),
    ("test-with-reserved", "\"test-with-reserved\""),
    ("600a", "\"600a\""),
    ("", ""),
    ("a b", "\"a b\""),
    ("a(b", "\"a(b\""),
    ("a)b", "\"a)b\""),
    ("a;b", "\"a;b\""),
    ("a_b", "\"a_b\""),
    ("a/b", "\"a/b\""),
    ("a~b", "\"a~b\""),
    ("a{b", "\"a{b\""),
    ("a}b", "\"a}b\""),
    ("-", "\"-\""),
    ("-a", "\"-a\""),
    ("--1", "\"--1\""),
    ("1", "\"1\""),
    ("-1x", "\"-1x\""),
    ("F.Cu", "F.Cu"),
    ("+5V", "+5V"),
    ("GND", "GND"),
    ("\u{00e9}t\u{00e9}", "\"\u{00e9}t\u{00e9}\""),
    ("\u{041a}\u{0438}\u{0440}", "\"\u{041a}\u{0438}\u{0440}\""),
    ("a\"b", "ab"),
    ("\"quoted\"", "quote"),
    ("\"ab\"", "a"),
    ("\"abc\"", "ab"),
    ("\"\"quoted\"\"", "quoted"),
    ("x\"", "x"),
    ("\"x", "x"),
    ("1\u{000a}abc", "1\u{000a}abc"),
    ("1abc\u{000a}", "1abc\u{000a}"),
    ("\u{00b5}", "\"\u{00b5}\""),
    ("\"a b\"", "\"a \""),
    ("'q'", "'q'"),
    ("a'b", "a'b"),
    ("\u{0000}", "\"\u{0000}\""),
    ("(1)", "\"(1)\""),
    ("v1.0", "v1.0"),
    ("0", "\"0\""),
    ("-.5", "\"-.5\""),
];

const IDENTS_SINGLE_QUOTE: &[(&str, &str)] = &[
    ("600", "'600'"),
    ("-600", "'-600'"),
    ("test", "test"),
    ("test-with-reserved", "'test-with-reserved'"),
    ("600a", "'600a'"),
    ("", ""),
    ("a b", "'a b'"),
    ("a(b", "'a(b'"),
    ("a)b", "'a)b'"),
    ("a;b", "'a;b'"),
    ("a_b", "'a_b'"),
    ("a/b", "'a/b'"),
    ("a~b", "'a~b'"),
    ("a{b", "'a{b'"),
    ("a}b", "'a}b'"),
    ("-", "'-'"),
    ("-a", "'-a'"),
    ("--1", "'--1'"),
    ("1", "'1'"),
    ("-1x", "'-1x'"),
    ("F.Cu", "F.Cu"),
    ("+5V", "+5V"),
    ("GND", "GND"),
    ("\u{00e9}t\u{00e9}", "'\u{00e9}t\u{00e9}'"),
    ("\u{041a}\u{0438}\u{0440}", "'\u{041a}\u{0438}\u{0440}'"),
    ("a\"b", "a\"b"),
    ("\"quoted\"", "quote"),
    ("\"ab\"", "a"),
    ("\"abc\"", "ab"),
    ("\"\"quoted\"\"", "\"quoted"),
    ("x\"", "x\""),
    ("\"x", "\"x"),
    ("1\u{000a}abc", "1\u{000a}abc"),
    ("1abc\u{000a}", "1abc\u{000a}"),
    ("\u{00b5}", "'\u{00b5}'"),
    ("\"a b\"", "'a '"),
    ("'q'", "q"),
    ("a'b", "ab"),
    ("\u{0000}", "'\u{0000}'"),
    ("(1)", "'(1)'"),
    ("v1.0", "v1.0"),
    ("0", "'0'"),
    ("-.5", "'-.5'"),
];

/// Names whose Java output comes from `substring(1, len - 2)` dropping the last real character
/// of a name in outer quotes; the Rust output keeps it. `(name, quote, Rust output)`.
const OUTER_QUOTE_FIXES: &[(&str, &str, &str)] = &[
    ("\"quoted\"", "\"", "quoted"),
    ("\"ab\"", "\"", "ab"),
    ("\"abc\"", "\"", "abc"),
    ("\"a b\"", "\"", "\"a b\""),
    ("\"quoted\"", "'", "quoted"),
    ("\"ab\"", "'", "ab"),
    ("\"abc\"", "'", "abc"),
    ("\"\"quoted\"\"", "'", "quoted"),
    ("\"a b\"", "'", "'a b'"),
];

fn route_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reference/route")
}

#[test]
fn java_double_matches_java() {
    assert!(DOUBLES.len() >= 30);
    for (bits, text, _) in DOUBLES {
        let value = f64::from_bits(*bits);
        assert_eq!(java_double(value), *text, "bits {bits:#x}");
    }
}

#[test]
fn java_double_special_values() {
    assert_eq!(java_double(f64::NAN), "NaN");
    assert_eq!(java_double(f64::INFINITY), "Infinity");
    assert_eq!(java_double(f64::NEG_INFINITY), "-Infinity");
}

#[test]
fn java_plain_decimal_matches_java() {
    for (bits, _, plain) in DOUBLES {
        let value = f64::from_bits(*bits);
        assert_eq!(java_plain_decimal(value), *plain, "bits {bits:#x}");
    }
}

/// Port of `SesRoundTripTest.placementRotationFormattingMatchesKicadStyle`.
#[test]
fn placement_rotation_formatting_matches_kicad_style() {
    assert_eq!(java_plain_decimal(0.0), "0");
    assert_eq!(java_plain_decimal(339.0), "339");
    assert_eq!(java_plain_decimal(338.5), "338.5");
    assert_eq!(java_plain_decimal(12.2501234567), "12.2501234567");
    assert_eq!(java_plain_decimal(-45.25), "-45.25");
}

fn check_idents(table: &[(&str, &str)], quote: &str) {
    for (name, java) in table {
        let fixed = OUTER_QUOTE_FIXES
            .iter()
            .find(|(n, q, _)| n == name && *q == quote);
        let expected = fixed.map_or(*java, |(_, _, rust)| *rust);
        assert_eq!(
            java_ident(name, quote),
            expected,
            "name {name:?} with quote {quote:?}"
        );
    }
}

#[test]
fn java_ident_matches_java() {
    check_idents(IDENTS_DOUBLE_QUOTE, "\"");
    check_idents(IDENTS_SINGLE_QUOTE, "'");
}

/// Port of `IdentifierTypeTest.write` (its reserved characters are a subset of the SES
/// writer's).
#[test]
fn identifier_type_write() {
    assert_eq!(java_ident("600", "\""), "\"600\"");
    assert_eq!(java_ident("-600", "\""), "\"-600\"");
    assert_eq!(java_ident("test", "\""), "test");
    assert_eq!(
        java_ident("test-with-reserved", "\""),
        "\"test-with-reserved\""
    );
    assert_eq!(java_ident("600a", "\""), "\"600a\"");
}

/// The one deliberate difference: Java drops the last real character of a name in outer
/// quotes (`substring(1, len - 2)`); Rust cuts only the quotes.
#[test]
fn identifier_with_outer_quotes_keeps_last_char() {
    assert_eq!(java_ident("\"abc\"", "\""), "abc");
    assert_eq!(java_ident("\"quoted\"", "\""), "quoted");
    // Several layers of quotes are all removed.
    assert_eq!(java_ident("\"\"abc\"\"", "\""), "abc");
    // The check applies to names longer than two characters only.
    assert_eq!(java_ident("\"\"", "\""), "");
    // A name with a space inside still gets quotes of its own.
    assert_eq!(java_ident("\"a b\"", "\""), "\"a b\"");
}

#[test]
fn a_number_is_quoted_but_not_a_name_with_a_line_break() {
    assert_eq!(java_ident("12", "\""), "\"12\"");
    assert_eq!(java_ident("1\nabc", "\""), "1\nabc");
    assert_eq!(java_ident("a1", "\""), "a1");
}

/// Every Java-written route session reads back to its own bytes. Non-empty files only: the
/// empty ones are boards on which Java wrote nothing (the checklist C05.1 counts 160; the
/// reference holds 116 non-empty files at this commit).
const EXPECTED_EXCEPTIONS: &[(&str, &str)] = &[];

#[test]
fn route_sessions_round_trip_to_their_own_bytes() {
    let mut checked = 0;
    let mut empty = 0;
    let mut failures = Vec::new();
    let mut entries: Vec<_> = std::fs::read_dir(route_dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "ses"))
        .collect();
    entries.sort();
    for path in entries {
        let bytes = std::fs::read(&path).unwrap();
        if bytes.is_empty() {
            empty += 1;
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let text = freerouting_dsn::decode(&bytes);
        let session = match Session::parse(&text) {
            Ok(session) => session,
            Err(e) => {
                failures.push(format!("{name}: parse error {e}"));
                continue;
            }
        };
        let written = write_session_java(&session);
        checked += 1;
        if written.as_bytes() != bytes.as_slice() {
            let at = written
                .bytes()
                .zip(bytes.iter().copied())
                .position(|(a, b)| a != b)
                .unwrap_or(written.len().min(bytes.len()));
            let expected = EXPECTED_EXCEPTIONS.iter().any(|(n, _)| *n == name);
            if !expected {
                let from = at.saturating_sub(40);
                failures.push(format!(
                    "{name}: first difference at byte {at}: java {:?} / rust {:?}",
                    String::from_utf8_lossy(&bytes[from..(at + 40).min(bytes.len())]),
                    String::from_utf8_lossy(
                        &written.as_bytes()[from..(at + 40).min(written.len())]
                    ),
                ));
            }
        }
    }
    eprintln!("{checked} sessions checked, {empty} empty files skipped");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(checked >= 100, "only {checked} sessions found");
}

/// The session files of C01.3's `ses` dump are Java's `SesWriter` output for the unrouted and
/// the routed board of each subset board; they round trip too (skipped where the dump is not
/// there).
#[test]
fn ses_dump_sessions_round_trip_to_their_own_bytes() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reference/ses");
    let mut checked = 0;
    for dir in ["unrouted", "routed"] {
        let Ok(read) = std::fs::read_dir(base.join(dir)) else {
            continue;
        };
        for entry in read {
            let path = entry.unwrap().path();
            if path.is_dir() || path.extension().is_none_or(|e| e != "ses") {
                continue;
            }
            let bytes = std::fs::read(&path).unwrap();
            if bytes.is_empty() {
                continue;
            }
            let session = Session::parse(&freerouting_dsn::decode(&bytes)).unwrap();
            assert_eq!(
                write_session_java(&session).as_bytes(),
                bytes.as_slice(),
                "{}",
                path.display()
            );
            checked += 1;
        }
    }
    eprintln!("{checked} dump sessions checked");
}

/// `java_double` output reads back as the same double, for ordinary and extreme values.
#[test]
fn java_double_reads_back() {
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    for _ in 0..200_000 {
        // xorshift64*
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let bits = state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let value = f64::from_bits(bits);
        if !value.is_finite() {
            continue;
        }
        let text = java_double(value);
        assert_eq!(text.parse::<f64>().unwrap().to_bits(), bits, "{text}");
        assert!(text.contains('.'), "{text}");
    }
}
