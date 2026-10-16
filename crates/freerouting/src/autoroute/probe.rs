//! The autoroute probe format (A01.1): the Rust side of
//! `tools/java-reference/probes/autoroute/AutorouteProbe.java`.
//!
//! Every A-series probe compares Rust with Java as text. Both sides write
//! the same lines (A00, "Probes"):
//!
//! - one JSON object per line, keys in call order;
//! - integers in decimal, booleans as `true`/`false`, strings with `"` and
//!   `\` escaped and control characters as `\u00XX`;
//! - every `double` as the string `"0x"` + 16 lowercase hex digits of its raw
//!   bits ([`ProbeValue::Bits`]), so `-0.0`, `NaN` payloads and the last ulp
//!   survive; [`decode_bits`] turns them back into numbers for people;
//! - the first line is a header with the kind, the board, the Java commit
//!   (`"rust"` on this side) and the format version. Only the kind and the
//!   board are compared.
//!
//! Output above 1 MiB is committed as a digest ([`digest`]): the header,
//! then one line `<section> <line count> <FNV-1a 64>` per section, where a
//! section starts at every line whose first key is `"stage"` (its value is
//! the section name) and runs to the next one. The hash is [`fnv64`] over
//! the section's lines, each followed by `\n`. Both sides compute it by hand,
//! with no dependency.

use std::fmt::Write as _;

/// The format version written in every header.
pub const FORMAT_VERSION: i64 = 1;

/// The size above which a probe output is committed as a digest.
pub const DIGEST_THRESHOLD_BYTES: usize = 1024 * 1024;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

/// One value of a probe record.
#[derive(Clone, Debug, PartialEq)]
pub enum ProbeValue {
    /// A Java `int` or `long`, printed in decimal.
    Int(i64),
    /// A Java `double`, printed as `"0x%016x"` of its raw bits.
    Bits(f64),
    /// A Java `boolean`.
    Bool(bool),
    /// A Java `String`, escaped as the Java writer escapes it.
    Str(String),
    /// A JSON array of values.
    List(Vec<ProbeValue>),
}

impl ProbeValue {
    /// A list of integers.
    pub fn ints<I: IntoIterator<Item = i64>>(values: I) -> ProbeValue {
        ProbeValue::List(values.into_iter().map(ProbeValue::Int).collect())
    }

    /// A list of doubles as raw bits.
    pub fn bits_list<I: IntoIterator<Item = f64>>(values: I) -> ProbeValue {
        ProbeValue::List(values.into_iter().map(ProbeValue::Bits).collect())
    }

    /// A list of booleans.
    pub fn bools<I: IntoIterator<Item = bool>>(values: I) -> ProbeValue {
        ProbeValue::List(values.into_iter().map(ProbeValue::Bool).collect())
    }

    /// A string value.
    pub fn str(value: impl Into<String>) -> ProbeValue {
        ProbeValue::Str(value.into())
    }

    fn write(&self, out: &mut String) {
        match self {
            ProbeValue::Int(value) => {
                let _ = write!(out, "{value}");
            }
            ProbeValue::Bits(value) => {
                out.push('"');
                out.push_str(&bits(*value));
                out.push('"');
            }
            ProbeValue::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            ProbeValue::Str(value) => write_string(out, value),
            ProbeValue::List(values) => {
                out.push('[');
                for (i, value) in values.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    value.write(out);
                }
                out.push(']');
            }
        }
    }
}

impl From<i32> for ProbeValue {
    fn from(value: i32) -> ProbeValue {
        ProbeValue::Int(i64::from(value))
    }
}

impl From<i64> for ProbeValue {
    fn from(value: i64) -> ProbeValue {
        ProbeValue::Int(value)
    }
}

impl From<usize> for ProbeValue {
    fn from(value: usize) -> ProbeValue {
        ProbeValue::Int(i64::try_from(value).unwrap_or(i64::MAX))
    }
}

impl From<f64> for ProbeValue {
    fn from(value: f64) -> ProbeValue {
        ProbeValue::Bits(value)
    }
}

impl From<bool> for ProbeValue {
    fn from(value: bool) -> ProbeValue {
        ProbeValue::Bool(value)
    }
}

impl From<&str> for ProbeValue {
    fn from(value: &str) -> ProbeValue {
        ProbeValue::Str(value.to_string())
    }
}

impl From<String> for ProbeValue {
    fn from(value: String) -> ProbeValue {
        ProbeValue::Str(value)
    }
}

/// A `double` as the probes write it: `0x` + its 16 raw-bit hex digits.
pub fn bits(value: f64) -> String {
    format!("0x{:016x}", value.to_bits())
}

/// The `double` of a [`bits`] text, if it is one.
pub fn decode_bits(text: &str) -> Option<f64> {
    let hex = text.strip_prefix("0x")?;
    (hex.len() == 16)
        .then(|| u64::from_str_radix(hex, 16).ok())
        .flatten()
        .map(f64::from_bits)
}

fn write_string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// One record as a line.
pub fn record_line(fields: &[(&str, ProbeValue)]) -> String {
    let mut out = String::from("{");
    for (i, (key, value)) in fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        write_string(&mut out, key);
        out.push(':');
        value.write(&mut out);
    }
    out.push('}');
    out
}

/// Collects the lines of one probe output.
#[derive(Clone, Debug, Default)]
pub struct ProbeWriter {
    lines: Vec<String>,
}

impl ProbeWriter {
    /// An empty output.
    pub fn new() -> ProbeWriter {
        ProbeWriter::default()
    }

    /// Writes the header line: kind, board, Java commit and format version.
    pub fn header(&mut self, kind: &str, board: &str, commit: &str) {
        self.record(&[
            ("kind", kind.into()),
            ("board", board.into()),
            ("javaCommit", commit.into()),
            ("format", ProbeValue::Int(FORMAT_VERSION)),
        ]);
    }

    /// Writes one record.
    pub fn record(&mut self, fields: &[(&str, ProbeValue)]) {
        self.lines.push(record_line(fields));
    }

    /// Writes the record of an error at the point where Java throws: the
    /// Java exception text and where Java throws it.
    pub fn exception(&mut self, exception: &str, at: &str) {
        self.record(&[("exception", exception.into()), ("at", at.into())]);
    }

    /// The lines written so far.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The lines, consuming the writer.
    pub fn into_lines(self) -> Vec<String> {
        self.lines
    }

    /// The output as text: every line followed by `\n`.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            out.push_str(line);
            out.push('\n');
        }
        out
    }
}

/// FNV-1a 64 over `lines`, each followed by `\n`.
pub fn fnv64<S: AsRef<str>>(lines: &[S]) -> u64 {
    let mut hash = FNV_OFFSET;
    for line in lines {
        for b in line.as_ref().bytes().chain(std::iter::once(b'\n')) {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    hash
}

/// The `kind` and `board` of a header line.
pub fn header_identity(line: &str) -> Option<(String, String)> {
    let field = |key: &str| {
        let start = line.find(&format!("\"{key}\":\""))? + key.len() + 4;
        let end = start + line[start..].find('"')?;
        Some(line[start..end].to_string())
    };
    Some((field("kind")?, field("board")?))
}

/// The digest form of `lines` (header first): the header, then one line
/// `<section> <line count> <fnv64>` per section. Lines before the first
/// section are not hashed.
pub fn digest<S: AsRef<str>>(lines: &[S]) -> Vec<String> {
    let mut out = vec![
        lines
            .first()
            .map(|l| l.as_ref().to_string())
            .unwrap_or_default(),
    ];
    let mut section: Option<(String, Vec<&str>)> = None;
    let flush = |section: &mut Option<(String, Vec<&str>)>, out: &mut Vec<String>| {
        if let Some((name, body)) = section.take() {
            out.push(format!("{name} {} {:016x}", body.len(), fnv64(&body)));
        }
    };
    for line in lines.iter().skip(1).map(AsRef::as_ref) {
        if let Some(rest) = line.strip_prefix("{\"stage\":\"") {
            flush(&mut section, &mut out);
            let name = rest.split('"').next().unwrap_or_default().to_string();
            section = Some((name, Vec::new()));
        }
        if let Some((_, body)) = section.as_mut() {
            body.push(line);
        }
    }
    flush(&mut section, &mut out);
    out
}

/// The index of the first line where `java` and `rust` differ, comparing
/// only the kind and board of the headers. `None` if they are equal.
pub fn first_difference<J: AsRef<str>, R: AsRef<str>>(java: &[J], rust: &[R]) -> Option<usize> {
    match (java.first(), rust.first()) {
        (Some(j), Some(r)) if header_identity(j.as_ref()) == header_identity(r.as_ref()) => {}
        _ => return Some(0),
    }
    let n = java.len().max(rust.len());
    (1..n).find(|&i| java.get(i).map(AsRef::as_ref) != rust.get(i).map(AsRef::as_ref))
}

/// The `"0x…"` values of `line` decoded, for a failure message: each as
/// `0x… = <value>`.
pub fn explain_bits(line: &str) -> String {
    let mut out = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find("\"0x") {
        let text = &rest[start + 1..];
        let end = text.find('"').unwrap_or(text.len());
        if let Some(value) = decode_bits(&text[..end]) {
            out.push(format!("{} = {value:e}", &text[..end]));
        }
        rest = &text[end..];
    }
    out.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_written_like_the_java_writer() {
        let line = record_line(&[
            ("i", ProbeValue::Int(i64::from(i32::MIN))),
            ("d", ProbeValue::Bits(-0.0)),
            ("b", true.into()),
            ("s", "a\"b\\c\u{1}".into()),
            (
                "l",
                ProbeValue::List(vec![ProbeValue::ints([1, 2]), ProbeValue::List(vec![])]),
            ),
        ]);
        assert_eq!(
            line,
            r#"{"i":-2147483648,"d":"0x8000000000000000","b":true,"s":"a\"b\\c\u0001","l":[[1,2],[]]}"#
        );
    }

    #[test]
    fn bits_round_trip() {
        for value in [0.1, -0.0, f64::MIN_POSITIVE, 4.9e-324, f64::MAX] {
            assert_eq!(
                decode_bits(&bits(value)).map(f64::to_bits),
                Some(value.to_bits())
            );
        }
        assert_eq!(decode_bits("0x12"), None);
        assert!(explain_bits(r#"{"d":"0x3fb999999999999a"}"#).contains("1e-1"));
    }

    #[test]
    fn fnv64_hashes_lines_with_newlines() {
        // FNV-1a 64 of "a\n".
        let mut expected = FNV_OFFSET;
        for b in *b"a\n" {
            expected = (expected ^ u64::from(b)).wrapping_mul(FNV_PRIME);
        }
        assert_eq!(fnv64(&["a"]), expected);
        assert_eq!(fnv64::<&str>(&[]), FNV_OFFSET);
    }

    #[test]
    fn digest_splits_at_stage_lines() {
        let lines = [
            r#"{"kind":"k","board":"b"}"#,
            r#"{"stage":"one","x":1}"#,
            r#"{"y":2}"#,
            r#"{"stage":"two"}"#,
        ];
        let digest = digest(&lines);
        assert_eq!(digest.len(), 3);
        assert_eq!(digest[1], format!("one 2 {:016x}", fnv64(&lines[1..3])));
        assert_eq!(digest[2], format!("two 1 {:016x}", fnv64(&lines[3..])));
    }

    #[test]
    fn first_difference_ignores_the_commit() {
        let mut java = ProbeWriter::new();
        java.header("k", "b", "a773a70ec");
        java.record(&[("x", 1.into())]);
        let mut rust = ProbeWriter::new();
        rust.header("k", "b", "rust");
        rust.record(&[("x", 1.into())]);
        assert_eq!(first_difference(java.lines(), rust.lines()), None);
        rust.record(&[("x", 2.into())]);
        assert_eq!(first_difference(java.lines(), rust.lines()), Some(2));
    }
}
