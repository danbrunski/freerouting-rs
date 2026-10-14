//! Hand-written lexer for Specctra files, replacing the JFlex scanner
//! (`SpecctraFileDescription.flex` / `SpecctraDsnStreamReader`).
//!
//! The Java scanner is stateful: the parser switches it between a default
//! state, a `NAME` state and raw "read the next string" calls. This lexer keeps
//! that shape. The parser asks for the next token in a [`Mode`], or for a raw
//! string with [`Lexer::raw`]. Atoms are returned as text plus a `quoted` flag;
//! whether an atom is a keyword, a number or a name is decided by the parser
//! from context.
//!
//! Token rules, matching the Java scanner where it is well defined:
//!
//! * `(` and `)` are always single tokens.
//! * A token that starts with `"` or `'` is a quoted string that ends at the
//!   next occurrence of the same quote character. There are no escapes: a
//!   backslash is an ordinary character. Quoted strings may span lines.
//! * Any other token is a bare atom: a run of characters up to whitespace or a
//!   parenthesis. Quote characters inside a bare atom are ordinary characters
//!   (`abc"def` is one atom), as are `#`, `$`, `/`, `-`, `+`, non-ASCII
//!   letters and so on.
//! * In [`Mode::Default`], `# ...` (to the end of the line) and `/* ... */`
//!   are comments when they start a token. Names are read in [`Mode::Name`],
//!   where they are not, so `#WLRXD` is a valid net name.
//! * The `(string_quote ")` directive's argument is read with
//!   [`Lexer::atom_ignoring_quotes`], like the scanner's `IGNORE_QUOTE` state.
//!
//! Differences from the JFlex scanner are listed in the crate documentation.

use crate::error::{Error, Pos, Result};

/// How the next token is scanned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Keywords, numbers and structure. Comments are skipped.
    Default,
    /// Names: like `Default`, but `#` and `/*` do not start comments.
    Name,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Atom<'a> {
    /// The text without the surrounding quotes.
    pub text: &'a str,
    pub quoted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenKind<'a> {
    Open,
    Close,
    Atom(Atom<'a>),
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token<'a> {
    pub kind: TokenKind<'a>,
    pub pos: Pos,
}

/// A saved lexer position, for lookahead.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Mark {
    pos: usize,
    line: u32,
    col: u32,
}

pub(crate) struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    line: u32,
    col: u32,
}

fn is_ws(b: u8) -> bool {
    // Space, tab, line feed, vertical tab, form feed, carriage return and
    // backspace (the Java raw string reader treats code 8 as a blank).
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | 0x08)
}

fn is_delim(b: u8) -> bool {
    is_ws(b) || b == b'(' || b == b')'
}

fn is_quote(b: u8) -> bool {
    b == b'"' || b == b'\''
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        let mut lexer = Lexer {
            src,
            pos: 0,
            line: 1,
            col: 1,
        };
        if src.starts_with('\u{feff}') {
            lexer.pos = '\u{feff}'.len_utf8();
        }
        lexer
    }

    pub fn mark(&self) -> Mark {
        Mark {
            pos: self.pos,
            line: self.line,
            col: self.col,
        }
    }

    pub fn reset(&mut self, mark: Mark) {
        self.pos = mark.pos;
        self.line = mark.line;
        self.col = mark.col;
    }

    pub fn current_pos(&self) -> Pos {
        Pos {
            line: self.line,
            column: self.col,
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn byte_at(&self, index: usize) -> Option<u8> {
        self.src.as_bytes().get(index).copied()
    }

    /// Moves to byte offset `end`, updating line and column.
    fn advance_to(&mut self, end: usize) {
        let bytes = self.src.as_bytes();
        for i in self.pos..end {
            match bytes[i] {
                b'\n' => {
                    self.line += 1;
                    self.col = 1;
                }
                b'\r' => {
                    if bytes.get(i + 1) != Some(&b'\n') {
                        self.line += 1;
                        self.col = 1;
                    }
                }
                b if b & 0xC0 == 0x80 => {} // UTF-8 continuation byte
                _ => self.col += 1,
            }
        }
        self.pos = end;
    }

    fn skip_whitespace(&mut self, comments: bool, sep: Option<u8>) -> Result<()> {
        loop {
            let Some(b) = self.peek_byte() else {
                return Ok(());
            };
            if is_ws(b) || Some(b) == sep {
                self.advance_to(self.pos + 1);
            } else if comments && b == b'#' {
                let rest = &self.src[self.pos..];
                let end = rest
                    .find(['\n', '\r'])
                    .map_or(self.src.len(), |i| self.pos + i);
                self.advance_to(end);
            } else if comments && b == b'/' && self.byte_at(self.pos + 1) == Some(b'*') {
                let start = self.current_pos();
                match self.src[self.pos + 2..].find("*/") {
                    Some(i) => self.advance_to(self.pos + 2 + i + 2),
                    None => return Err(Error::new(start, "unterminated /* comment")),
                }
            } else {
                return Ok(());
            }
        }
    }

    fn quoted(&mut self, quote: u8) -> Result<Token<'a>> {
        let pos = self.current_pos();
        let start = self.pos + 1;
        match self.src[start..].find(quote as char) {
            Some(i) => {
                let text = &self.src[start..start + i];
                self.advance_to(start + i + 1);
                Ok(Token {
                    kind: TokenKind::Atom(Atom { text, quoted: true }),
                    pos,
                })
            }
            None => Err(Error::new(
                pos,
                format!(
                    "unterminated quoted string (missing closing {})",
                    quote as char
                ),
            )),
        }
    }

    fn bare(&mut self, sep: Option<u8>) -> Token<'a> {
        let pos = self.current_pos();
        let bytes = self.src.as_bytes();
        let start = self.pos;
        let mut end = start;
        while end < bytes.len() && !is_delim(bytes[end]) && Some(bytes[end]) != sep {
            end += 1;
        }
        let text = &self.src[start..end];
        self.advance_to(end);
        Token {
            kind: TokenKind::Atom(Atom {
                text,
                quoted: false,
            }),
            pos,
        }
    }

    /// Reads the next token.
    pub fn next(&mut self, mode: Mode) -> Result<Token<'a>> {
        self.skip_whitespace(mode == Mode::Default, None)?;
        let pos = self.current_pos();
        match self.peek_byte() {
            None => Ok(Token {
                kind: TokenKind::Eof,
                pos,
            }),
            Some(b'(') => {
                self.advance_to(self.pos + 1);
                Ok(Token {
                    kind: TokenKind::Open,
                    pos,
                })
            }
            Some(b')') => {
                self.advance_to(self.pos + 1);
                Ok(Token {
                    kind: TokenKind::Close,
                    pos,
                })
            }
            Some(b) if is_quote(b) => self.quoted(b),
            Some(_) => Ok(self.bare(None)),
        }
    }

    /// Returns the next token without consuming it.
    pub fn peek(&mut self, mode: Mode) -> Result<Token<'a>> {
        let mark = self.mark();
        let token = self.next(mode);
        self.reset(mark);
        token
    }

    /// Reads the next string the way the Java scanner's `nextString` does:
    /// skips blanks (and `sep`), then reads a quoted string, or a bare run that
    /// also stops at `sep`. Returns `None` without consuming anything if the
    /// next character is a parenthesis or the input ends.
    pub fn raw(&mut self, sep: Option<u8>) -> Result<Option<Token<'a>>> {
        self.skip_whitespace(false, sep)?;
        match self.peek_byte() {
            None | Some(b'(') | Some(b')') => Ok(None),
            Some(b) if is_quote(b) => self.quoted(b).map(Some),
            Some(_) => Ok(Some(self.bare(sep))),
        }
    }

    /// Reads the next bare run with quote characters treated as ordinary
    /// characters, like the Java scanner's `IGNORE_QUOTE` state. Used for the
    /// argument of `(string_quote ")`.
    pub fn atom_ignoring_quotes(&mut self) -> Result<Option<Token<'a>>> {
        self.skip_whitespace(false, None)?;
        match self.peek_byte() {
            None | Some(b'(') | Some(b')') => Ok(None),
            Some(_) => Ok(Some(self.bare(None))),
        }
    }

    /// Consumes `b` if it is the very next byte (no blanks skipped).
    pub fn eat_byte(&mut self, b: u8) -> bool {
        if self.peek_byte() == Some(b) {
            self.advance_to(self.pos + 1);
            true
        } else {
            false
        }
    }
}

/// Parses a decimal number. Accepts an optional sign, digits with an optional
/// fraction (`1`, `1.5`, `1.`, `.5`) and an optional exponent (`1e5`,
/// `2.5E-3`). Rejects anything else, including `inf` and `nan`.
pub(crate) fn parse_f64(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    let mut i = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    let int_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let mut digits = i - int_start;
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        digits += i - frac_start;
    }
    if digits == 0 {
        return None;
    }
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        if matches!(bytes.get(i), Some(b'+' | b'-')) {
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
    if i != bytes.len() {
        return None;
    }
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Parses a decimal integer that fits in an `i32`.
pub(crate) fn parse_i32(text: &str) -> Option<i32> {
    let bytes = text.as_bytes();
    let digits = match bytes.first() {
        Some(b'+' | b'-') => &bytes[1..],
        _ => bytes,
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    text.parse::<i32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(src: &str, mode: Mode) -> Vec<String> {
        let mut lexer = Lexer::new(src);
        let mut out = Vec::new();
        loop {
            let token = lexer.next(mode).unwrap();
            match token.kind {
                TokenKind::Open => out.push("(".to_string()),
                TokenKind::Close => out.push(")".to_string()),
                TokenKind::Atom(a) if a.quoted => out.push(format!("q:{}", a.text)),
                TokenKind::Atom(a) => out.push(a.text.to_string()),
                TokenKind::Eof => break,
            }
        }
        out
    }

    #[test]
    fn parentheses_split_atoms() {
        assert_eq!(
            tokens("(boundary(path pcb 0 1 2))", Mode::Default),
            ["(", "boundary", "(", "path", "pcb", "0", "1", "2", ")", ")"]
        );
    }

    #[test]
    fn quoted_strings_keep_spaces_and_have_no_escapes() {
        assert_eq!(
            tokens(r#"("a b" 'c d' "x\y" "it's")"#, Mode::Default),
            ["(", "q:a b", "q:c d", r"q:x\y", "q:it's", ")"]
        );
    }

    #[test]
    fn quoted_strings_may_span_lines() {
        assert_eq!(tokens("\"x\ny\" z", Mode::Default), ["q:x\ny", "z"]);
    }

    #[test]
    fn empty_quoted_strings() {
        assert_eq!(tokens("(PCB '')", Mode::Default), ["(", "PCB", "q:", ")"]);
        assert_eq!(tokens("\"\"", Mode::Default), ["q:"]);
    }

    #[test]
    fn quote_inside_bare_atom_is_ordinary() {
        assert_eq!(
            tokens(r#"abc"def 1oz" default_"1A"#, Mode::Default),
            [r#"abc"def"#, r#"1oz""#, r#"default_"1A"#]
        );
    }

    #[test]
    fn quoted_string_followed_directly_by_text() {
        assert_eq!(tokens(r#""J3"-"D+""#, Mode::Default), ["q:J3", r#"-"D+""#]);
    }

    #[test]
    fn odd_identifier_characters() {
        assert_eq!(
            tokens(
                "Via[0-1]_600:300_um Round1$13.779528 /SCL ~RESET a|b x{y} \u{41c}\u{438}\u{43a}\u{440}\u{43e}",
                Mode::Default
            ),
            [
                "Via[0-1]_600:300_um",
                "Round1$13.779528",
                "/SCL",
                "~RESET",
                "a|b",
                "x{y}",
                "\u{41c}\u{438}\u{43a}\u{440}\u{43e}"
            ]
        );
    }

    #[test]
    fn comments_in_default_mode() {
        assert_eq!(
            tokens("(a # comment ( here\n b /* c ) */ c)", Mode::Default),
            ["(", "a", "b", "c", ")"]
        );
    }

    #[test]
    fn no_comments_in_name_mode() {
        assert_eq!(tokens("#WLRXD /*x", Mode::Name), ["#WLRXD", "/*x"]);
    }

    #[test]
    fn unterminated_string_reports_position() {
        let mut lexer = Lexer::new("(a\n  \"abc");
        lexer.next(Mode::Default).unwrap();
        lexer.next(Mode::Default).unwrap();
        let err = lexer.next(Mode::Default).unwrap_err();
        assert_eq!(err.pos, Pos { line: 2, column: 3 });
    }

    #[test]
    fn unterminated_comment_is_an_error() {
        let mut lexer = Lexer::new("a /* b");
        lexer.next(Mode::Default).unwrap();
        assert!(lexer.next(Mode::Default).is_err());
    }

    #[test]
    fn positions_count_lines_and_characters() {
        let mut lexer = Lexer::new("(\u{43f}\u{430} b\r\n  c\r d)");
        let mut positions = Vec::new();
        loop {
            let token = lexer.next(Mode::Default).unwrap();
            if token.kind == TokenKind::Eof {
                break;
            }
            positions.push((token.pos.line, token.pos.column));
        }
        assert_eq!(positions, [(1, 1), (1, 2), (1, 5), (2, 3), (3, 2), (3, 3)]);
    }

    #[test]
    fn byte_order_mark_is_skipped() {
        assert_eq!(
            tokens("\u{feff}(pcb x)", Mode::Default),
            ["(", "pcb", "x", ")"]
        );
    }

    #[test]
    fn raw_strings_stop_at_separator() {
        let mut lexer = Lexer::new(r#" "J3 -+"-"_VBUS #22" J2-VBUS)"#);
        let a = lexer.raw(Some(b'-')).unwrap().unwrap();
        assert_eq!(
            a.kind,
            TokenKind::Atom(Atom {
                text: "J3 -+",
                quoted: true
            })
        );
        assert!(lexer.eat_byte(b'-'));
        let b = lexer.raw(None).unwrap().unwrap();
        assert_eq!(
            b.kind,
            TokenKind::Atom(Atom {
                text: "_VBUS #22",
                quoted: true
            })
        );
        let c = lexer.raw(Some(b'-')).unwrap().unwrap();
        assert_eq!(
            c.kind,
            TokenKind::Atom(Atom {
                text: "J2",
                quoted: false
            })
        );
        assert!(lexer.eat_byte(b'-'));
        let d = lexer.raw(None).unwrap().unwrap();
        assert_eq!(
            d.kind,
            TokenKind::Atom(Atom {
                text: "VBUS",
                quoted: false
            })
        );
        assert_eq!(lexer.raw(None).unwrap(), None);
        assert_eq!(lexer.next(Mode::Default).unwrap().kind, TokenKind::Close);
    }

    #[test]
    fn raw_strings_keep_inner_quotes() {
        let mut lexer = Lexer::new(r#"default_"1A EXTERNAL 1oz")"#);
        let mut out = Vec::new();
        while let Some(token) = lexer.raw(Some(b'-')).unwrap() {
            if let TokenKind::Atom(a) = token.kind {
                out.push(a.text);
            }
        }
        assert_eq!(out, [r#"default_"1A"#, "EXTERNAL", r#"1oz""#]);
    }

    #[test]
    fn string_quote_argument_ignores_quotes() {
        let mut lexer = Lexer::new("(string_quote \")\n(x \"a b\")");
        lexer.next(Mode::Default).unwrap();
        lexer.next(Mode::Default).unwrap();
        let q = lexer.atom_ignoring_quotes().unwrap().unwrap();
        assert_eq!(
            q.kind,
            TokenKind::Atom(Atom {
                text: "\"",
                quoted: false
            })
        );
        assert_eq!(lexer.next(Mode::Default).unwrap().kind, TokenKind::Close);
        assert_eq!(
            tokens("(x \"a b\")", Mode::Default),
            ["(", "x", "q:a b", ")"]
        );
    }

    #[test]
    fn peek_does_not_consume() {
        let mut lexer = Lexer::new("a b");
        let p = lexer.peek(Mode::Default).unwrap();
        let n = lexer.next(Mode::Default).unwrap();
        assert_eq!(p, n);
        assert_eq!(
            lexer.next(Mode::Default).unwrap().kind,
            TokenKind::Atom(Atom {
                text: "b",
                quoted: false
            })
        );
    }

    #[test]
    fn numbers() {
        for (text, value) in [
            ("0", 0.0),
            ("-3", -3.0),
            ("+4", 4.0),
            ("1.5", 1.5),
            ("007", 7.0),
            ("1.", 1.0),
            (".5", 0.5),
            ("-.5", -0.5),
            ("1e5", 1e5),
            ("1E+5", 1e5),
            ("2.5e-3", 2.5e-3),
            ("162598.100000", 162598.1),
        ] {
            assert_eq!(parse_f64(text), Some(value), "{text}");
        }
        for text in [
            "", "-", ".", "e5", "1e", "1.5.5", "1abc", "inf", "NaN", "0x10", "1,5",
        ] {
            assert_eq!(parse_f64(text), None, "{text}");
        }
        assert_eq!(parse_i32("42"), Some(42));
        assert_eq!(parse_i32("-7"), Some(-7));
        assert_eq!(parse_i32("+7"), Some(7));
        assert_eq!(parse_i32("1.0"), None);
        assert_eq!(parse_i32("99999999999"), None);
        assert_eq!(parse_i32(""), None);
    }
}
