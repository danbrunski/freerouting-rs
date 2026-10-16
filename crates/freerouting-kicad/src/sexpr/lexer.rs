//! The s-expression lexer.
//!
//! Tokens are `(`, `)`, bare atoms and quoted strings. An atom ends at
//! whitespace, `(`, `)` or `"`. Inside a quoted string `\"` and `\\` are
//! escapes and parentheses are text (KiCadRoutingTools issue #113: a part
//! number `"TCR2EF115,LM(CT"` made a naive paren counter swallow the
//! following footprints).
//!
//! Tokens carry byte offsets only. Line and column are counted from the offset
//! when an error is built, so the hot loop does no line bookkeeping. The lexer
//! also tracks the nesting depth, because a stray `)` and a file that ends
//! inside a list are lexical errors in KiCad too.

use crate::error::{ErrorKind, KicadError};

/// What a [`Token`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Open,
    Close,
    Atom,
    /// A quoted string; the token range is the text between the quotes,
    /// still escaped.
    Str,
}

/// A token as a byte range of the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub start: u32,
    pub end: u32,
}

/// Interprets `bytes` as UTF-8, or fails naming the first bad byte offset
/// (KiCad writes UTF-8; anything else is a damaged or foreign file).
pub fn decode(bytes: &[u8]) -> Result<&str, KicadError> {
    std::str::from_utf8(bytes).map_err(|e| {
        KicadError::new(
            ErrorKind::Syntax,
            format!(
                "the file is not UTF-8: first invalid byte at offset {}",
                e.valid_up_to()
            ),
        )
    })
}

#[inline]
fn is_atom_end(b: u8) -> bool {
    b <= b' ' || b == b'(' || b == b')' || b == b'"'
}

/// Iterator over the tokens of a source text. After the first error it
/// yields nothing more.
pub struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    depth: u32,
    done: bool,
}

impl<'a> Lexer<'a> {
    /// A lexer over `src`. Sources of 4 GiB or more are refused when the first
    /// token is read, because token offsets are `u32`.
    pub fn new(src: &'a str) -> Self {
        Lexer {
            src,
            pos: 0,
            depth: 0,
            done: false,
        }
    }

    /// Current nesting depth (open lists not yet closed).
    pub fn depth(&self) -> u32 {
        self.depth
    }

    fn fail(&mut self, offset: usize, message: &str) -> Option<Result<Token, KicadError>> {
        self.done = true;
        Some(Err(KicadError::at(
            ErrorKind::Syntax,
            self.src,
            offset,
            message,
        )))
    }

    /// The offset of the innermost `(` that is still open at the end of the
    /// source. Rescans the text; used on the error path only.
    fn innermost_open(&self) -> usize {
        let bytes = self.src.as_bytes();
        let mut stack: Vec<usize> = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'(' => stack.push(i),
                b')' => {
                    stack.pop();
                }
                b'"' => i = skip_string(bytes, i + 1),
                _ => {}
            }
            i += 1;
        }
        stack.last().copied().unwrap_or(0)
    }
}

/// Index of the closing quote of the string whose content starts at `i`, or
/// `bytes.len()` when it never closes.
#[inline]
fn skip_string(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return i,
            _ => i += 1,
        }
    }
    bytes.len()
}

impl Iterator for Lexer<'_> {
    type Item = Result<Token, KicadError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let bytes = self.src.as_bytes();
        if bytes.len() > u32::MAX as usize {
            return self.fail(0, "the file is 4 GiB or larger");
        }
        let mut i = self.pos;
        while i < bytes.len() && bytes[i] <= b' ' {
            i += 1;
        }
        if i >= bytes.len() {
            self.pos = i;
            if self.depth > 0 {
                let open = self.innermost_open();
                return self.fail(open, "end of file inside a list: this '(' is never closed");
            }
            self.done = true;
            return None;
        }
        let start = i;
        let token = match bytes[i] {
            b'(' => {
                self.depth += 1;
                self.pos = i + 1;
                Token {
                    kind: TokenKind::Open,
                    start: start as u32,
                    end: (i + 1) as u32,
                }
            }
            b')' => {
                if self.depth == 0 {
                    return self.fail(start, "unmatched ')'");
                }
                self.depth -= 1;
                self.pos = i + 1;
                Token {
                    kind: TokenKind::Close,
                    start: start as u32,
                    end: (i + 1) as u32,
                }
            }
            b'"' => {
                let close = skip_string(bytes, i + 1);
                if close >= bytes.len() {
                    return self.fail(start, "unterminated string");
                }
                self.pos = close + 1;
                Token {
                    kind: TokenKind::Str,
                    start: (start + 1) as u32,
                    end: close as u32,
                }
            }
            _ => {
                i += 1;
                while i < bytes.len() && !is_atom_end(bytes[i]) {
                    i += 1;
                }
                self.pos = i;
                Token {
                    kind: TokenKind::Atom,
                    start: start as u32,
                    end: i as u32,
                }
            }
        };
        Some(Ok(token))
    }
}

/// Unescapes the content of a quoted string the way KiCad's `DSNLEXER` does
/// (`common/dsnlexer.cpp` at `10.0.0`, outside specctra mode): `\a \b \f \n
/// \r \t \v`, `\"`, `\\`, one- or two-digit hex `\xHH`, and one- to
/// three-digit octal `\NNN`. A hex escape without digits is `x`; an
/// unknown escape that is not octal is a backslash followed by the
/// character. Returns the input unchanged when it has no backslash.
pub fn unescape(raw: &str) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    if !raw.as_bytes().contains(&b'\\') {
        return Cow::Borrowed(raw);
    }
    let b = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' {
            out.push(b[i]);
            i += 1;
            continue;
        }
        i += 1;
        if i >= b.len() {
            break; // KiCad treats a trailing backslash as an unterminated string
        }
        let c = b[i];
        i += 1;
        match c {
            b'"' | b'\\' => out.push(c),
            b'a' => out.push(0x07),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'v' => out.push(0x0b),
            b'x' => {
                let mut n = 0;
                let mut v = 0u32;
                while n < 2 && i + n < b.len() && b[i + n].is_ascii_hexdigit() {
                    v = v * 16 + (b[i + n] as char).to_digit(16).unwrap_or(0);
                    n += 1;
                }
                out.push(if n == 0 { b'x' } else { v as u8 });
                i += n;
            }
            _ => {
                i -= 1;
                let mut n = 0;
                let mut v = 0u32;
                while n < 3 && i + n < b.len() && (b'0'..=b'7').contains(&b[i + n]) {
                    v = v * 8 + (b[i + n] - b'0') as u32;
                    n += 1;
                }
                if n == 0 {
                    out.push(b'\\');
                } else {
                    out.push(v as u8);
                }
                i += n;
            }
        }
    }
    Cow::Owned(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(TokenKind, &str)> {
        Lexer::new(src)
            .map(|t| {
                let t = t.unwrap();
                (t.kind, &src[t.start as usize..t.end as usize])
            })
            .collect()
    }

    #[test]
    fn issue_113_parens_inside_a_string_are_text() {
        // KiCadRoutingTools #113: a part number with an unbalanced '(' inside quotes.
        let src = r#"(property "Part" "TCR2EF115,LM(CT") (footprint "next")"#;
        let t = kinds(src);
        assert_eq!(t[3], (TokenKind::Str, "TCR2EF115,LM(CT"));
        assert_eq!(t[4], (TokenKind::Close, ")"));
        assert_eq!(t[6], (TokenKind::Atom, "footprint"));
        assert_eq!(t.len(), 9);
    }

    #[test]
    fn escaped_quotes_stay_in_the_string() {
        let src = r#"(a "a \"quoted\" word")"#;
        let t = kinds(src);
        assert_eq!(t[2], (TokenKind::Str, r#"a \"quoted\" word"#));
        assert_eq!(unescape(t[2].1), "a \"quoted\" word");
    }

    #[test]
    fn backslash_pairs_and_trailing_backslash_quote() {
        // "a\\" ends at the quote after the escaped backslash.
        let src = r#"(a "a\\") (b)"#;
        let t = kinds(src);
        assert_eq!(t[2], (TokenKind::Str, r"a\\"));
        assert_eq!(unescape(t[2].1), "a\\");
        assert_eq!(t[5], (TokenKind::Atom, "b"));
    }

    #[test]
    fn string_with_embedded_newline() {
        let src = "(text \"line one\nline two\") (after)";
        let t = kinds(src);
        assert_eq!(t[2], (TokenKind::Str, "line one\nline two"));
        assert_eq!(t[5], (TokenKind::Atom, "after"));
    }

    #[test]
    fn numbers_are_atoms_and_stop_at_delimiters() {
        let t = kinds("(xy 1.5 -2e-3)");
        assert_eq!(
            t,
            vec![
                (TokenKind::Open, "("),
                (TokenKind::Atom, "xy"),
                (TokenKind::Atom, "1.5"),
                (TokenKind::Atom, "-2e-3"),
                (TokenKind::Close, ")"),
            ]
        );
        // an atom ends at a quote and at a paren without whitespace
        let t = kinds("(a(b)\"c\"d)");
        let texts: Vec<_> = t.iter().map(|x| x.1).collect();
        assert_eq!(texts, ["(", "a", "(", "b", ")", "c", "d", ")"]);
    }

    #[test]
    fn error_unterminated_string_points_at_the_opening_quote() {
        let src = "(a\n  (b \"never closed)\n(c))";
        let err = Lexer::new(src).find_map(|t| t.err()).unwrap();
        assert_eq!(err.kind, ErrorKind::Syntax);
        assert_eq!((err.line, err.column), (2, 6));
        assert!(err.message.contains("unterminated string"));
    }

    #[test]
    fn error_stray_close_paren() {
        let src = "(a)\n )";
        let err = Lexer::new(src).find_map(|t| t.err()).unwrap();
        assert_eq!((err.line, err.column), (2, 2));
        assert!(err.message.contains("unmatched ')'"));
    }

    #[test]
    fn error_end_of_file_inside_a_list_points_at_the_innermost_open_paren() {
        let src = "(kicad_pcb\n  (net 1 \"a(b\")\n  (footprint\n    (at 1 2)";
        let err = Lexer::new(src).find_map(|t| t.err()).unwrap();
        assert_eq!((err.line, err.column), (3, 3));
        assert!(err.message.contains("never closed"));
    }

    #[test]
    fn after_an_error_nothing_more_is_yielded() {
        let mut l = Lexer::new(")(a)");
        assert!(l.next().unwrap().is_err());
        assert!(l.next().is_none());
    }

    #[test]
    fn utf8_check_names_the_first_bad_byte() {
        let bytes = b"(a \"ok\") \xff(b)";
        let err = decode(bytes).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Syntax);
        assert!(err.message.contains("offset 9"), "{}", err.message);
        assert_eq!(decode("(é)".as_bytes()).unwrap(), "(é)");
    }

    #[test]
    fn column_counts_characters() {
        let src = "(é\" ";
        let err = Lexer::new(src).find_map(|t| t.err()).unwrap();
        assert_eq!((err.line, err.column), (1, 3));
    }

    #[test]
    fn unescape_follows_dsnlexer() {
        assert_eq!(unescape("plain"), "plain");
        assert_eq!(unescape(r"a\nb\tc\\d\"), "a\nb\tc\\d");
        assert_eq!(unescape(r"\x41\x4a\x4"), "AJ\u{4}");
        assert_eq!(unescape(r"\101\60"), "A0");
        assert_eq!(unescape(r"\xg"), "xg");
        assert_eq!(unescape(r"\q"), "\\q");
    }
}
