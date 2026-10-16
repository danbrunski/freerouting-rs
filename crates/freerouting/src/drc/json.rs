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

/// A JSON syntax error from [`JsonValue::parse`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonParseError {
    /// What was wrong.
    pub message: String,
    /// Byte offset in the input.
    pub offset: usize,
}

impl std::fmt::Display for JsonParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at offset {}", self.message, self.offset)
    }
}

impl std::error::Error for JsonParseError {}

impl JsonValue {
    /// Parses a JSON document.
    ///
    /// Numbers keep their source text (`Number("1500")`, `Number("2.50")`),
    /// so they convert exactly as Java's `Integer.parseInt` and
    /// `Double.parseDouble` would. Object members keep their order, and
    /// duplicate names are kept. The text is strict JSON plus `//`, `#` and
    /// `/* */` comments; Gson's other lenient forms (unquoted names, single
    /// quotes) are errors. Anything after the value except whitespace and
    /// comments is an error, as in `JsonParser.parseReader`.
    pub fn parse(text: &str) -> Result<JsonValue, JsonParseError> {
        let mut parser = JsonParser {
            bytes: text.as_bytes(),
            text,
            pos: 0,
        };
        parser.skip_blank()?;
        let value = parser.value(0)?;
        parser.skip_blank()?;
        if parser.pos != parser.bytes.len() {
            return Err(parser.error("Did not consume the entire document"));
        }
        Ok(value)
    }
}

const MAX_JSON_DEPTH: usize = 256;

struct JsonParser<'a> {
    bytes: &'a [u8],
    text: &'a str,
    pos: usize,
}

impl JsonParser<'_> {
    fn error(&self, message: &str) -> JsonParseError {
        JsonParseError {
            message: message.to_string(),
            offset: self.pos,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_blank(&mut self) -> Result<(), JsonParseError> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.pos += 1,
                Some(b'#') => self.skip_line(),
                Some(b'/') => match self.bytes.get(self.pos + 1) {
                    Some(b'/') => self.skip_line(),
                    Some(b'*') => {
                        let start = self.pos;
                        self.pos += 2;
                        loop {
                            match self.peek() {
                                None => {
                                    self.pos = start;
                                    return Err(self.error("Unterminated comment"));
                                }
                                Some(b'*') if self.bytes.get(self.pos + 1) == Some(&b'/') => {
                                    self.pos += 2;
                                    break;
                                }
                                Some(_) => self.pos += 1,
                            }
                        }
                    }
                    _ => return Ok(()),
                },
                _ => return Ok(()),
            }
        }
    }

    fn skip_line(&mut self) {
        while let Some(b) = self.peek() {
            self.pos += 1;
            if b == b'\n' {
                break;
            }
        }
    }

    fn expect(&mut self, byte: u8, what: &str) -> Result<(), JsonParseError> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.error(what))
        }
    }

    fn literal(&mut self, word: &str, value: JsonValue) -> Result<JsonValue, JsonParseError> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(self.error("Unexpected character"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<JsonValue, JsonParseError> {
        if depth > MAX_JSON_DEPTH {
            return Err(self.error("Nesting too deep"));
        }
        match self.peek() {
            None => Err(self.error("Unexpected end of input")),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(JsonValue::String(self.string()?)),
            Some(b't') => self.literal("true", JsonValue::Bool(true)),
            Some(b'f') => self.literal("false", JsonValue::Bool(false)),
            Some(b'n') => self.literal("null", JsonValue::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("Unexpected character")),
        }
    }

    fn object(&mut self, depth: usize) -> Result<JsonValue, JsonParseError> {
        self.pos += 1;
        let mut members = Vec::new();
        self.skip_blank()?;
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(JsonValue::Object(members));
        }
        loop {
            self.skip_blank()?;
            if self.peek() != Some(b'"') {
                return Err(self.error("Expected a quoted member name"));
            }
            let name = self.string()?;
            self.skip_blank()?;
            self.expect(b':', "Expected ':' after the member name")?;
            self.skip_blank()?;
            let value = self.value(depth + 1)?;
            members.push((name, value));
            self.skip_blank()?;
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(JsonValue::Object(members));
                }
                _ => return Err(self.error("Expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<JsonValue, JsonParseError> {
        self.pos += 1;
        let mut values = Vec::new();
        self.skip_blank()?;
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(JsonValue::Array(values));
        }
        loop {
            self.skip_blank()?;
            values.push(self.value(depth + 1)?);
            self.skip_blank()?;
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(JsonValue::Array(values));
                }
                _ => return Err(self.error("Expected ',' or ']'")),
            }
        }
    }

    fn number(&mut self) -> Result<JsonValue, JsonParseError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        let digits = |p: &mut Self| {
            let from = p.pos;
            while matches!(p.peek(), Some(b'0'..=b'9')) {
                p.pos += 1;
            }
            p.pos - from
        };
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                digits(self);
            }
            _ => return Err(self.error("Malformed number")),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if digits(self) == 0 {
                return Err(self.error("Malformed number"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if digits(self) == 0 {
                return Err(self.error("Malformed number"));
            }
        }
        Ok(JsonValue::Number(self.text[start..self.pos].to_string()))
    }

    fn hex4(&mut self) -> Result<u32, JsonParseError> {
        let end = self.pos + 4;
        let digits = self
            .text
            .get(self.pos..end)
            .filter(|d| d.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| self.error("Malformed \\u escape"))?;
        self.pos = end;
        Ok(u32::from_str_radix(digits, 16).expect("four hex digits"))
    }

    fn string(&mut self) -> Result<String, JsonParseError> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            let Some(b) = self.peek() else {
                return Err(self.error("Unterminated string"));
            };
            match b {
                b'"' => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    let Some(escape) = self.peek() else {
                        return Err(self.error("Unterminated string"));
                    };
                    self.pos += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first)
                                && self.bytes[self.pos..].starts_with(b"\\u")
                            {
                                let save = self.pos;
                                self.pos += 2;
                                let second = self.hex4()?;
                                if (0xDC00..0xE000).contains(&second) {
                                    0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                                } else {
                                    self.pos = save;
                                    first
                                }
                            } else {
                                first
                            };
                            out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                        }
                        _ => {
                            self.pos -= 1;
                            return Err(self.error("Invalid escape sequence"));
                        }
                    }
                }
                b if b < 0x20 => return Err(self.error("Unescaped control character")),
                _ => {
                    let c = self.text[self.pos..]
                        .chars()
                        .next()
                        .expect("a character at a valid offset");
                    out.push(c);
                    self.pos += c.len_utf8();
                }
            }
        }
    }
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
    fn parses_json_keeping_number_text() {
        let value = JsonValue::parse(
            "{ \"a\": [1, 2.50, -3e2, true, null], \"b\": {\"c\": \"x\\u00e9\\n\"} } // end",
        )
        .unwrap();
        assert_eq!(
            value,
            JsonValue::Object(vec![
                (
                    "a".into(),
                    JsonValue::Array(vec![
                        JsonValue::Number("1".into()),
                        JsonValue::Number("2.50".into()),
                        JsonValue::Number("-3e2".into()),
                        JsonValue::Bool(true),
                        JsonValue::Null,
                    ])
                ),
                (
                    "b".into(),
                    JsonValue::Object(vec![("c".into(), JsonValue::string("x\u{e9}\n"))])
                ),
            ])
        );
        assert_eq!(
            JsonValue::parse("\"\\ud83d\\ude00\"").unwrap(),
            JsonValue::string("\u{1F600}")
        );
    }

    #[test]
    fn rejects_malformed_json() {
        for bad in [
            "",
            "{ this is not valid json !! }",
            "{\"a\": 1,}",
            "[1 2]",
            "{\"a\": 01}",
            "{\"a\": 1.}",
            "{} {}",
            "\"abc",
            "{\"a\"}",
            "/* open",
        ] {
            assert!(JsonValue::parse(bad).is_err(), "{bad:?} must fail");
        }
        let deep = "[".repeat(1000);
        assert!(JsonValue::parse(&deep).is_err());
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
