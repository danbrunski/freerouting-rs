//! A small JSON reader for the reference files and the tools' own output.
//!
//! Numbers are kept as `f64`; that is enough for statistics and timings.

use std::collections::BTreeMap;

/// A parsed JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    /// Parses `text`.
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut parser = Parser {
            bytes: text.as_bytes(),
            pos: 0,
        };
        let value = parser.value()?;
        parser.skip_ws();
        if parser.pos != parser.bytes.len() {
            return Err(format!("trailing characters at byte {}", parser.pos));
        }
        Ok(value)
    }

    /// The member `key` of an object.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(members) => members.get(key),
            _ => None,
        }
    }

    /// The value at a dotted path such as `statistics.vias.total_count`.
    pub fn path(&self, path: &str) -> Option<&Json> {
        path.split('.').try_fold(self, |v, key| v.get(key))
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.bytes.get(self.pos).copied()
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", c as char, self.pos))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        match self.peek() {
            Some(b'{') => {
                self.pos += 1;
                let mut members = BTreeMap::new();
                if self.peek() == Some(b'}') {
                    self.pos += 1;
                    return Ok(Json::Object(members));
                }
                loop {
                    self.skip_ws();
                    let key = self.string()?;
                    self.expect(b':')?;
                    members.insert(key, self.value()?);
                    match self.peek() {
                        Some(b',') => self.pos += 1,
                        Some(b'}') => {
                            self.pos += 1;
                            return Ok(Json::Object(members));
                        }
                        _ => return Err(format!("expected ',' or '}}' at byte {}", self.pos)),
                    }
                }
            }
            Some(b'[') => {
                self.pos += 1;
                let mut values = Vec::new();
                if self.peek() == Some(b']') {
                    self.pos += 1;
                    return Ok(Json::Array(values));
                }
                loop {
                    values.push(self.value()?);
                    match self.peek() {
                        Some(b',') => self.pos += 1,
                        Some(b']') => {
                            self.pos += 1;
                            return Ok(Json::Array(values));
                        }
                        _ => return Err(format!("expected ',' or ']' at byte {}", self.pos)),
                    }
                }
            }
            Some(b'"') => Ok(Json::String(self.string()?)),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(_) => self.number(),
            None => Err("unexpected end of JSON".into()),
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(format!("unexpected token at byte {}", self.pos))
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        while self.pos < self.bytes.len()
            && matches!(
                self.bytes[self.pos],
                b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E'
            )
        {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap_or("");
        text.parse()
            .map(Json::Number)
            .map_err(|_| format!("bad number at byte {start}"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .bytes
            .get(self.pos..self.pos + 4)
            .and_then(|d| std::str::from_utf8(d).ok())
            .and_then(|d| u32::from_str_radix(d, 16).ok())
            .ok_or_else(|| format!("bad \\u escape at byte {}", self.pos))?;
        self.pos += 4;
        Ok(digits)
    }

    fn string(&mut self) -> Result<String, String> {
        if self.bytes.get(self.pos) != Some(&b'"') {
            return Err(format!("expected a string at byte {}", self.pos));
        }
        self.pos += 1;
        let mut out = String::new();
        let mut run_start = self.pos;
        loop {
            let Some(&c) = self.bytes.get(self.pos) else {
                return Err("unterminated string".into());
            };
            match c {
                b'"' => {
                    out.push_str(&String::from_utf8_lossy(&self.bytes[run_start..self.pos]));
                    self.pos += 1;
                    return Ok(out);
                }
                b'\\' => {
                    out.push_str(&String::from_utf8_lossy(&self.bytes[run_start..self.pos]));
                    let escape = self.bytes.get(self.pos + 1).copied();
                    self.pos += 2;
                    match escape {
                        Some(b'n') => out.push('\n'),
                        Some(b't') => out.push('\t'),
                        Some(b'r') => out.push('\r'),
                        Some(b'b') => out.push('\u{8}'),
                        Some(b'f') => out.push('\u{c}'),
                        Some(b'u') => {
                            let mut code = self.hex4()?;
                            if (0xD800..0xDC00).contains(&code)
                                && self.bytes.get(self.pos..self.pos + 2) == Some(b"\\u")
                            {
                                self.pos += 2;
                                let low = self.hex4()?;
                                code =
                                    0x10000 + ((code - 0xD800) << 10) + (low.wrapping_sub(0xDC00));
                            }
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        Some(other) => out.push(other as char),
                        None => return Err("unterminated escape".into()),
                    }
                    run_start = self.pos;
                }
                _ => self.pos += 1,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_values() {
        let json =
            Json::parse(r#"{"a": {"b": [1, 2.5, -3e2]}, "s": "x'y\n", "t": true, "n": null}"#)
                .unwrap();
        assert_eq!(
            json.path("a.b"),
            Some(&Json::Array(vec![
                Json::Number(1.0),
                Json::Number(2.5),
                Json::Number(-300.0)
            ]))
        );
        assert_eq!(json.get("s").and_then(Json::as_str), Some("x'y\n"));
        assert_eq!(json.get("t"), Some(&Json::Bool(true)));
        assert_eq!(json.get("n"), Some(&Json::Null));
        assert!(Json::parse("{\"a\": 1,}").is_err());
        assert!(Json::parse("[1] x").is_err());
    }
}
