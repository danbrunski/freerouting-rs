//! A small JSON writer producing the output of Java's
//! `GsonProvider.GSON` (pretty printing with two spaces, no HTML escaping,
//! doubles as `%.2f` decimals), and Java number formatting helpers.

use std::fmt::Write;

/// A JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    /// A number written verbatim (already formatted).
    Number(String),
    String(String),
    Array(Vec<JsonValue>),
    /// Members in insertion order.
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    /// A double written like Gson's `TwoDecimalDoubleAdapter`:
    /// `new BigDecimal(String.format("%.2f", value))`, `null` if not finite.
    pub fn two_decimals(value: f64) -> JsonValue {
        if !value.is_finite() {
            return JsonValue::Null;
        }
        JsonValue::Number(big_decimal_string(&java_format_fixed(value, 2)))
    }

    /// An integer.
    pub fn int(value: i64) -> JsonValue {
        JsonValue::Number(value.to_string())
    }

    /// A string.
    pub fn string(value: impl Into<String>) -> JsonValue {
        JsonValue::String(value.into())
    }

    /// Writes the value with Gson's pretty printing.
    pub fn to_pretty_string(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, indent: usize) {
        match self {
            JsonValue::Null => out.push_str("null"),
            JsonValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            JsonValue::Number(n) => out.push_str(n),
            JsonValue::String(s) => write_string(out, s),
            JsonValue::Array(values) => {
                if values.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (i, value) in values.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, indent + 1);
                    value.write(out, indent + 1);
                }
                newline(out, indent);
                out.push(']');
            }
            JsonValue::Object(members) => {
                // Gson leaves out null members.
                let members: Vec<_> = members
                    .iter()
                    .filter(|(_, v)| !matches!(v, JsonValue::Null))
                    .collect();
                if members.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (i, (name, value)) in members.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, indent + 1);
                    write_string(out, name);
                    out.push_str(": ");
                    value.write(out, indent + 1);
                }
                newline(out, indent);
                out.push('}');
            }
        }
    }
}

fn newline(out: &mut String, indent: usize) {
    out.push('\n');
    for _ in 0..indent {
        out.push_str("  ");
    }
}

/// Gson's string escaping without HTML escaping.
fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{c}' => out.push_str("\\f"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `BigDecimal(s).toString()` for a plain decimal `s`: a negative zero
/// loses its sign.
fn big_decimal_string(s: &str) -> String {
    match s.strip_prefix('-') {
        Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_string(),
        _ => s.to_string(),
    }
}

/// Java `String.format("%.Nf", value)` (`Locale.ROOT`): the shortest
/// decimal representation of `value` rounded half up to `digits`
/// fraction digits.
pub fn java_format_fixed(value: f64, digits: usize) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let negative = value.is_sign_negative();
    // The shortest round-trip digits and the decimal exponent.
    let sci = format!("{:e}", value.abs());
    let (mantissa, exponent) = sci.split_once('e').expect("exponent");
    let exponent: i64 = exponent.parse().expect("exponent value");
    let mut decimal_digits: Vec<u8> = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    // value = 0.d1 d2 d3 ... * 10^(exponent + 1)
    let mut point = exponent + 1;
    // The number of digits to keep: integer digits plus `digits`.
    let keep = point + digits as i64;
    if keep < 0 {
        decimal_digits.clear();
    } else if (keep as usize) < decimal_digits.len() {
        let round_up = decimal_digits[keep as usize] >= 5;
        decimal_digits.truncate(keep as usize);
        if round_up {
            let mut i = decimal_digits.len();
            loop {
                if i == 0 {
                    decimal_digits.insert(0, 1);
                    point += 1;
                    break;
                }
                i -= 1;
                if decimal_digits[i] == 9 {
                    decimal_digits[i] = 0;
                } else {
                    decimal_digits[i] += 1;
                    break;
                }
            }
        }
    }
    let digit = |i: i64| -> char {
        if i >= 0 && (i as usize) < decimal_digits.len() {
            (b'0' + decimal_digits[i as usize]) as char
        } else {
            '0'
        }
    };
    let mut result = String::new();
    if negative {
        result.push('-');
    }
    if point <= 0 {
        result.push('0');
    } else {
        for i in 0..point {
            result.push(digit(i));
        }
    }
    if digits > 0 {
        result.push('.');
        for i in 0..digits as i64 {
            result.push(digit(point + i));
        }
    }
    result
}

/// The current time as Java's
/// `ZonedDateTime.now().format(ISO_OFFSET_DATE_TIME)` in UTC.
pub fn iso_offset_date_time_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    iso_offset_date_time(now.as_secs() as i64, now.subsec_nanos())
}

/// `ISO_OFFSET_DATE_TIME` of a UTC time given as seconds and nanoseconds
/// since the epoch.
pub fn iso_offset_date_time(seconds: i64, nanos: u32) -> String {
    let days = seconds.div_euclid(86_400);
    let secs_of_day = seconds.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let mut result = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        secs_of_day / 3600,
        secs_of_day / 60 % 60,
        secs_of_day % 60
    );
    if nanos > 0 {
        // Java prints the fraction without trailing zeros.
        let fraction = format!("{nanos:09}");
        result.push('.');
        result.push_str(fraction.trim_end_matches('0'));
    }
    result.push('Z');
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_java() {
        assert_eq!(java_format_fixed(1000.0, 2), "1000.00");
        assert_eq!(java_format_fixed(0.125, 2), "0.13");
        // Java rounds the shortest representation "1.005" half up.
        assert_eq!(java_format_fixed(1.005, 2), "1.01");
        assert_eq!(java_format_fixed(-0.001, 2), "-0.00");
        assert_eq!(java_format_fixed(99.996, 2), "100.00");
        assert_eq!(java_format_fixed(0.0, 4), "0.0000");
        assert_eq!(java_format_fixed(111.4805, 2), "111.48");
        assert_eq!(java_format_fixed(1.0e-7, 4), "0.0000");
        assert_eq!(java_format_fixed(0.00005, 4), "0.0001");
        assert_eq!(java_format_fixed(123456789.0, 1), "123456789.0");
        assert_eq!(
            JsonValue::two_decimals(-0.001),
            JsonValue::Number("0.00".into())
        );
    }

    #[test]
    fn writes_pretty_json() {
        let value = JsonValue::Object(vec![
            ("a".into(), JsonValue::string("x\"<")),
            ("b".into(), JsonValue::Array(vec![])),
            ("n".into(), JsonValue::Null),
            (
                "c".into(),
                JsonValue::Array(vec![JsonValue::Object(vec![(
                    "x".into(),
                    JsonValue::two_decimals(1.5),
                )])]),
            ),
        ]);
        assert_eq!(
            value.to_pretty_string(),
            "{\n  \"a\": \"x\\\"<\",\n  \"b\": [],\n  \"c\": [\n    {\n      \"x\": 1.50\n    }\n  ]\n}"
        );
    }

    #[test]
    fn formats_dates() {
        assert_eq!(iso_offset_date_time(0, 0), "1970-01-01T00:00:00Z");
        assert_eq!(
            iso_offset_date_time(1_792_104_432, 779_568_150),
            "2026-10-15T22:47:12.77956815Z"
        );
    }
}
