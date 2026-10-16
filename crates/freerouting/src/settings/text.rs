//! Java's text number conventions: `Integer.parseInt`, `Double.parseDouble`,
//! `Float.parseFloat`, `Double.toString`, `Float.toString` and
//! `String.split`.

/// The text of Java's `NumberFormatException` for `input`.
fn number_format_message(input: &str) -> String {
    format!("For input string: \"{input}\"")
}

/// `Integer.parseInt`.
pub fn parse_int(text: &str) -> Result<i32, String> {
    text.parse::<i32>().map_err(|_| number_format_message(text))
}

/// `Long.parseLong`.
pub fn parse_long(text: &str) -> Result<i64, String> {
    text.parse::<i64>().map_err(|_| number_format_message(text))
}

/// The decimal body of `Double.parseDouble` (after trimming and removing
/// the type suffix) as text that Rust's parser reads the same way, or
/// `None` when Java throws. Hexadecimal floating point is not accepted.
fn validated_decimal(text: &str) -> Option<&str> {
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let bytes = unsigned.as_bytes();
    let mut i = 0;
    let mut mantissa_digits = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
        mantissa_digits += 1;
    }
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
            mantissa_digits += 1;
        }
    }
    if mantissa_digits == 0 {
        return None;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            i += 1;
        }
        let exp_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == exp_start {
            return None;
        }
    }
    (i == bytes.len()).then_some(text)
}

/// The Java floating point literal grammar around a Rust-parsable body:
/// returns `Ok(Some(body))` for a decimal number, `Ok(None)` is not used.
fn java_float_body(text: &str) -> Result<FloatText<'_>, String> {
    let trimmed = text.trim_matches(|c: char| c <= ' ');
    if trimmed.is_empty() {
        return Err(if text.is_empty() {
            "empty String".to_string()
        } else {
            number_format_message(text)
        });
    }
    let (negative, unsigned) = match trimmed.as_bytes()[0] {
        b'-' => (true, &trimmed[1..]),
        b'+' => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    if unsigned == "NaN" {
        return Ok(FloatText::Nan);
    }
    if unsigned == "Infinity" {
        return Ok(FloatText::Infinity(negative));
    }
    let body = match trimmed.as_bytes().last() {
        Some(b'f' | b'F' | b'd' | b'D') => &trimmed[..trimmed.len() - 1],
        _ => trimmed,
    };
    validated_decimal(body)
        .map(FloatText::Decimal)
        .ok_or_else(|| number_format_message(text))
}

enum FloatText<'a> {
    Nan,
    Infinity(bool),
    Decimal(&'a str),
}

/// `Double.parseDouble`: surrounding whitespace is trimmed and a trailing
/// `d`, `D`, `f` or `F` is accepted, which Rust's `parse` does not do.
pub fn parse_double(text: &str) -> Result<f64, String> {
    Ok(match java_float_body(text)? {
        FloatText::Nan => f64::NAN,
        FloatText::Infinity(negative) => {
            if negative {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }
        }
        FloatText::Decimal(body) => body.parse().map_err(|_| number_format_message(text))?,
    })
}

/// `Float.parseFloat`.
pub fn parse_float(text: &str) -> Result<f32, String> {
    Ok(match java_float_body(text)? {
        FloatText::Nan => f32::NAN,
        FloatText::Infinity(negative) => {
            if negative {
                f32::NEG_INFINITY
            } else {
                f32::INFINITY
            }
        }
        FloatText::Decimal(body) => body.parse().map_err(|_| number_format_message(text))?,
    })
}

/// `Boolean.parseBoolean`: `true` in any case, everything else is false.
pub fn parse_boolean(text: &str) -> bool {
    text.eq_ignore_ascii_case("true")
}

/// `String.split(separator)` for a one-character separator: when there is a
/// match, trailing empty strings are dropped.
pub fn java_split(text: &str, separator: char) -> Vec<&str> {
    if !text.contains(separator) {
        return vec![text];
    }
    let mut parts: Vec<&str> = text.split(separator).collect();
    while parts.last() == Some(&"") {
        parts.pop();
    }
    parts
}

/// Java's `Double.toString`.
pub fn java_double_to_string(value: f64) -> String {
    if !value.is_finite() {
        return non_finite(value.is_nan(), value > 0.0);
    }
    let a = value.abs();
    // A one-digit shortest form is replaced by the two-digit decimal
    // closest to the exact value (`4.9E-324`, not `5.0E-324`), when that
    // still reads back as the same number.
    let mut sci = format!("{a:e}");
    if !sci.contains('.') {
        let two = format!("{a:.1e}");
        if two.parse::<f64>() == Ok(a) && !two.contains(".0e") {
            sci = two;
        }
    }
    format_java(value.is_sign_negative(), &sci, a == 0.0, java_scientific(a))
}

/// Java's `Float.toString`.
pub fn java_float_to_string(value: f32) -> String {
    if !value.is_finite() {
        return non_finite(value.is_nan(), value > 0.0);
    }
    let a = value.abs();
    let mut sci = format!("{a:e}");
    if !sci.contains('.') {
        let two = format!("{a:.1e}");
        if two.parse::<f32>() == Ok(a) && !two.contains(".0e") {
            sci = two;
        }
    }
    format_java(
        value.is_sign_negative(),
        &sci,
        a == 0.0,
        java_scientific(f64::from(a)),
    )
}

fn non_finite(nan: bool, positive: bool) -> String {
    match (nan, positive) {
        (true, _) => "NaN",
        (false, true) => "Infinity",
        (false, false) => "-Infinity",
    }
    .to_string()
}

/// Whether Java writes `|v|` in computerised scientific notation: outside
/// [1e-3, 1e7), zero excepted.
fn java_scientific(a: f64) -> bool {
    a != 0.0 && !(1e-3..1e7).contains(&a)
}

/// Lays out Rust's shortest `{:e}` digits the way Java does: plain decimal
/// for 1e-3 <= |v| < 1e7, computerised scientific notation otherwise.
fn format_java(negative: bool, sci: &str, zero: bool, scientific: bool) -> String {
    let sign = if negative { "-" } else { "" };
    if zero {
        return format!("{sign}0.0");
    }
    let (mantissa, exponent) = sci.split_once('e').expect("exponent");
    let exponent: i32 = exponent.parse().expect("exponent value");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    if scientific {
        let (first, rest) = digits.split_at(1);
        let rest = if rest.is_empty() { "0" } else { rest };
        return format!("{sign}{first}.{rest}E{exponent}");
    }
    if exponent >= 0 {
        let int_len = exponent as usize + 1;
        let (int_part, frac_part) = if digits.len() <= int_len {
            (format!("{digits:0<int_len$}"), "0".to_string())
        } else {
            (digits[..int_len].to_string(), digits[int_len..].to_string())
        };
        format!("{sign}{int_part}.{frac_part}")
    } else {
        let zeros = "0".repeat((-exponent - 1) as usize);
        format!("{sign}0.{zeros}{digits}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_like_java() {
        assert_eq!(parse_int("42"), Ok(42));
        assert_eq!(parse_int("+7"), Ok(7));
        assert!(parse_int(" 7").is_err());
        assert!(parse_int("7.0").is_err());
        assert!(parse_int("2147483648").is_err());
        assert_eq!(parse_double(" 1.5 "), Ok(1.5));
        assert_eq!(parse_double("1.5d"), Ok(1.5));
        assert_eq!(parse_double("1.5F"), Ok(1.5));
        assert_eq!(parse_double(".5"), Ok(0.5));
        assert_eq!(parse_double("5."), Ok(5.0));
        assert_eq!(parse_double("1e3"), Ok(1000.0));
        assert_eq!(parse_double("-Infinity"), Ok(f64::NEG_INFINITY));
        assert!(parse_double("NaN").unwrap().is_nan());
        // Rust accepts these, Java does not.
        assert!(parse_double("inf").is_err());
        assert!(parse_double("nan").is_err());
        assert!(parse_double("infinity").is_err());
        assert!(parse_double("").is_err());
        assert!(parse_double("d").is_err());
        assert!(parse_double("1e").is_err());
        assert!(parse_double("0x1p3").is_err());
        assert_eq!(parse_float("0.035"), Ok(0.035_f32));
        assert!(!parse_boolean("yes"));
        assert!(parse_boolean("TRUE"));
    }

    #[test]
    fn formats_like_java() {
        assert_eq!(java_double_to_string(250.0), "250.0");
        assert_eq!(java_double_to_string(0.0), "0.0");
        assert_eq!(java_double_to_string(-0.0), "-0.0");
        assert_eq!(java_double_to_string(1.7), "1.7");
        assert_eq!(java_double_to_string(1.0e7), "1.0E7");
        assert_eq!(java_double_to_string(12345678.9), "1.23456789E7");
        assert_eq!(java_double_to_string(0.001), "0.001");
        assert_eq!(java_double_to_string(0.000123), "1.23E-4");
        assert_eq!(java_double_to_string(9.9), "9.9");
        assert_eq!(java_float_to_string(5_000_000.0), "5000000.0");
        assert_eq!(java_float_to_string(1000.0 / 3.0), "333.33334");
        assert_eq!(java_float_to_string(2.5), "2.5");
        assert_eq!(java_float_to_string(0.6), "0.6");
        assert_eq!(java_float_to_string(1.0e7), "1.0E7");
    }

    #[test]
    fn splits_like_java() {
        assert_eq!(java_split("a,b,,", ','), vec!["a", "b"]);
        assert_eq!(java_split("", ','), vec![""]);
        assert_eq!(java_split(",a", ','), vec!["", "a"]);
        assert_eq!(java_split(",,", ','), Vec::<&str>::new());
    }
}
