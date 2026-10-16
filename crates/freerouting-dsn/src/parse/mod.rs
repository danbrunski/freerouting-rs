//! Recursive-descent parser from tokens to the syntax tree.
//!
//! Every `(keyword ...)` scope handler is called after the opening bracket and
//! the keyword have been read, and consumes everything up to and including
//! the matching closing bracket. Unknown scopes are skipped, like the Java
//! reader's `skipScope`. Problems the Java reader logs and recovers from are
//! recorded as [`Warning`]s; problems it cannot recover from are [`Error`]s.

mod common;
mod dsn;
mod rules;
mod ses;

use crate::error::{Error, Pos, Result, Warning};
use crate::lexer::{Atom, Lexer, Mode, Token, TokenKind, parse_f64, parse_i32};

pub(crate) use dsn::parse_pcb;
pub(crate) use rules::parse_rules;
pub(crate) use ses::parse_session;

/// The result of reading the next element inside a scope.
enum Item {
    /// `(keyword`: the canonical keyword, or an empty string when the scope
    /// does not start with a bare word (`("x" ...)`, `(( ...`, `()`).
    Scope(String, Pos),
    /// The closing bracket of the current scope.
    End,
}

/// Returns the canonical spelling of a keyword: lower case, with the aliases
/// the Java scanner accepts mapped to one name.
pub(crate) fn canonical_keyword(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let canonical = match lower.as_str() {
        "circ" => "circle",
        "clear" => "clearance",
        "comp" => "component",
        "poly" => "polygon",
        "rectangle" => "rect",
        "prefered_direction" => "preferred_direction",
        "prefered_direction_trace_costs" => "preferred_direction_trace_costs",
        "against_prefered_direction_trace_costs" => "against_preferred_direction_trace_costs",
        "generated_by_freerouting" => "generated_by_freeroute",
        _ => return lower,
    };
    canonical.to_string()
}

pub(crate) struct Parser<'a> {
    lexer: Lexer<'a>,
    warnings: Vec<Warning>,
}

impl<'a> Parser<'a> {
    pub fn new(src: &'a str) -> Self {
        Parser {
            lexer: Lexer::new(src),
            warnings: Vec::new(),
        }
    }

    pub fn into_warnings(self) -> Vec<Warning> {
        self.warnings
    }

    fn warn(&mut self, pos: Pos, message: impl Into<String>) {
        self.warnings.push(Warning {
            pos,
            message: message.into(),
        });
    }

    fn next(&mut self) -> Result<Token<'a>> {
        self.lexer.next(Mode::Default)
    }

    fn peek(&mut self) -> Result<Token<'a>> {
        self.lexer.peek(Mode::Default)
    }

    fn next_name_token(&mut self) -> Result<Token<'a>> {
        self.lexer.next(Mode::Name)
    }

    fn peek_name_token(&mut self) -> Result<Token<'a>> {
        self.lexer.peek(Mode::Name)
    }

    fn eof_error(pos: Pos, context: &str) -> Error {
        Error::new(pos, format!("unexpected end of file in {context} scope"))
    }

    /// Reads `(keyword` at the start of a file and returns the canonical
    /// keyword.
    fn header(&mut self) -> Result<(String, Pos)> {
        let token = self.next()?;
        if token.kind != TokenKind::Open {
            return Err(Error::new(token.pos, "'(' expected at start of file"));
        }
        let token = self.next()?;
        match token.kind {
            TokenKind::Atom(Atom {
                text,
                quoted: false,
            }) => Ok((canonical_keyword(text), token.pos)),
            _ => Err(Error::new(token.pos, "keyword expected at start of file")),
        }
    }

    /// Reads the next element of a scope. Bare words outside sub-scopes are
    /// skipped with a warning, as the Java reader ignores them.
    fn item(&mut self, context: &str) -> Result<Item> {
        match self.item_or_eof(context)? {
            Some(item) => Ok(item),
            None => Err(Self::eof_error(self.lexer.current_pos(), context)),
        }
    }

    /// Like [`Parser::item`], but returns `None` at the end of the input.
    fn item_or_eof(&mut self, context: &str) -> Result<Option<Item>> {
        loop {
            let token = self.next()?;
            match token.kind {
                TokenKind::Close => return Ok(Some(Item::End)),
                TokenKind::Eof => return Ok(None),
                TokenKind::Atom(atom) => {
                    self.warn(
                        token.pos,
                        format!("unexpected '{}' in {context} scope ignored", atom.text),
                    );
                }
                TokenKind::Open => {
                    let next = self.peek()?;
                    if let TokenKind::Atom(Atom {
                        text,
                        quoted: false,
                    }) = next.kind
                    {
                        self.next()?;
                        return Ok(Some(Item::Scope(canonical_keyword(text), next.pos)));
                    }
                    return Ok(Some(Item::Scope(String::new(), token.pos)));
                }
            }
        }
    }

    /// Skips the rest of a scope whose opening bracket has been read,
    /// including nested scopes. Tokens are read in name mode, so `#` does not
    /// start a comment, as in the Java `skipScope`.
    fn skip_scope(&mut self) -> Result<()> {
        let start = self.lexer.current_pos();
        let mut depth = 1usize;
        while depth > 0 {
            let token = self.next_name_token()?;
            match token.kind {
                TokenKind::Open => depth += 1,
                TokenKind::Close => depth -= 1,
                TokenKind::Atom(_) => {}
                TokenKind::Eof => return Err(Self::eof_error(start, "skipped")),
            }
        }
        Ok(())
    }

    /// Skips an unknown scope.
    fn skip_unknown(&mut self, keyword: &str, pos: Pos, context: &str) -> Result<()> {
        if keyword.is_empty() {
            self.warn(pos, format!("scope without keyword in {context} skipped"));
        }
        self.skip_scope()
    }

    /// Reads up to the closing bracket of the current scope. Anything before
    /// it is skipped with a warning.
    fn finish_scope(&mut self, context: &str) -> Result<()> {
        loop {
            let token = self.next()?;
            match token.kind {
                TokenKind::Close => return Ok(()),
                TokenKind::Eof => return Err(Self::eof_error(token.pos, context)),
                TokenKind::Open => {
                    self.warn(token.pos, format!("unexpected scope in {context} skipped"));
                    self.skip_scope()?;
                }
                TokenKind::Atom(atom) => {
                    self.warn(
                        token.pos,
                        format!("unexpected '{}' in {context} scope ignored", atom.text),
                    );
                }
            }
        }
    }

    /// Reads a name: any atom, read without comment handling.
    fn name(&mut self, what: &str) -> Result<String> {
        let token = self.next_name_token()?;
        match token.kind {
            TokenKind::Atom(atom) => Ok(atom.text.to_string()),
            _ => Err(Error::new(
                token.pos,
                format!("{what} expected, found {}", describe(&token)),
            )),
        }
    }

    /// Reads a name if the next token is an atom.
    fn optional_name(&mut self) -> Result<Option<String>> {
        let token = self.peek_name_token()?;
        match token.kind {
            TokenKind::Atom(atom) => {
                self.next_name_token()?;
                Ok(Some(atom.text.to_string()))
            }
            _ => Ok(None),
        }
    }

    /// Reads names up to the closing bracket of the current scope. Nested
    /// scopes are skipped with a warning.
    fn names_to_end(&mut self, context: &str) -> Result<Vec<String>> {
        let mut names = Vec::new();
        loop {
            let token = self.next_name_token()?;
            match token.kind {
                TokenKind::Atom(atom) => names.push(atom.text.to_string()),
                TokenKind::Close => return Ok(names),
                TokenKind::Open => {
                    self.warn(token.pos, format!("unexpected scope in {context} skipped"));
                    self.skip_scope()?;
                }
                TokenKind::Eof => return Err(Self::eof_error(token.pos, context)),
            }
        }
    }

    /// Reads a string the way the Java scanner's `nextString` does: a quoted
    /// string, or a bare run that stops at blanks, brackets and `sep`. Returns
    /// an empty string if a bracket comes first.
    fn raw(&mut self, sep: Option<u8>) -> Result<String> {
        Ok(self
            .lexer
            .raw(sep)?
            .map(|token| match token.kind {
                TokenKind::Atom(atom) => atom.text.to_string(),
                _ => String::new(),
            })
            .unwrap_or_default())
    }

    /// Reads a list of raw strings up to the next bracket, splitting at blanks
    /// and `sep`. Empty quoted strings are skipped.
    fn raw_list(&mut self, sep: Option<u8>) -> Result<Vec<String>> {
        let mut list = Vec::new();
        while let Some(token) = self.lexer.raw(sep)? {
            match token.kind {
                TokenKind::Atom(atom) if !atom.text.is_empty() => {
                    list.push(atom.text.to_string());
                }
                _ => {}
            }
        }
        Ok(list)
    }

    /// Reads a number.
    fn number(&mut self, what: &str) -> Result<f64> {
        let token = self.next()?;
        number_of(&token).ok_or_else(|| {
            Error::new(
                token.pos,
                format!("number expected for {what}, found {}", describe(&token)),
            )
        })
    }

    /// Reads a number if the next token is one.
    fn optional_number(&mut self) -> Result<Option<f64>> {
        let token = self.peek()?;
        let value = number_of(&token);
        if value.is_some() {
            self.next()?;
        }
        Ok(value)
    }

    /// Reads an integer.
    fn integer(&mut self, what: &str) -> Result<i32> {
        let token = self.next()?;
        integer_of(&token).ok_or_else(|| {
            Error::new(
                token.pos,
                format!("integer expected for {what}, found {}", describe(&token)),
            )
        })
    }

    /// Reads a bare keyword if the next token is one, returning it in
    /// canonical form, without consuming anything else.
    fn optional_keyword(&mut self) -> Result<Option<(String, Pos)>> {
        let token = self.peek()?;
        if let TokenKind::Atom(Atom {
            text,
            quoted: false,
        }) = token.kind
        {
            self.next()?;
            return Ok(Some((canonical_keyword(text), token.pos)));
        }
        Ok(None)
    }

    /// `(keyword string)`, Java's `readStringScope`: one raw string, then the
    /// closing bracket.
    fn string_scope(&mut self, context: &str) -> Result<String> {
        let value = self.raw(None)?;
        self.finish_scope(context)?;
        Ok(value)
    }

    /// `(keyword on|off)`, Java's `readOnOffScope`. Anything but `on` is off.
    fn on_off_scope(&mut self, context: &str) -> Result<bool> {
        let token = self.next()?;
        let value = match token.kind {
            TokenKind::Atom(Atom {
                text,
                quoted: false,
            }) if text.eq_ignore_ascii_case("on") => true,
            TokenKind::Atom(Atom {
                text,
                quoted: false,
            }) if text.eq_ignore_ascii_case("off") => false,
            TokenKind::Close => {
                self.warn(token.pos, format!("on or off expected in {context}"));
                return Ok(false);
            }
            TokenKind::Eof => return Err(Self::eof_error(token.pos, context)),
            _ => {
                self.warn(token.pos, format!("on or off expected in {context}"));
                if token.kind == TokenKind::Open {
                    self.skip_scope()?;
                }
                false
            }
        };
        self.finish_scope(context)?;
        Ok(value)
    }

    /// `(keyword integer)`, Java's `readIntegerScope`. A missing or invalid
    /// value is `None`, with a warning.
    fn integer_scope(&mut self, context: &str) -> Result<Option<i32>> {
        let token = self.peek()?;
        let value = integer_of(&token);
        if value.is_some() {
            self.next()?;
        } else {
            self.warn(token.pos, format!("integer expected in {context}"));
        }
        self.finish_scope(context)?;
        Ok(value)
    }

    /// `(keyword number)`, Java's `readFloatScope`.
    fn float_scope(&mut self, context: &str) -> Result<Option<f64>> {
        let token = self.peek()?;
        let value = number_of(&token);
        if value.is_some() {
            self.next()?;
        } else {
            self.warn(token.pos, format!("number expected in {context}"));
        }
        self.finish_scope(context)?;
        Ok(value)
    }
}

/// The numeric value of a bare atom.
fn number_of(token: &Token<'_>) -> Option<f64> {
    match token.kind {
        TokenKind::Atom(Atom {
            text,
            quoted: false,
        }) => parse_f64(text),
        _ => None,
    }
}

fn integer_of(token: &Token<'_>) -> Option<i32> {
    match token.kind {
        TokenKind::Atom(Atom {
            text,
            quoted: false,
        }) => parse_i32(text),
        _ => None,
    }
}

fn describe(token: &Token<'_>) -> String {
    match token.kind {
        TokenKind::Open => "'('".to_string(),
        TokenKind::Close => "')'".to_string(),
        TokenKind::Eof => "end of file".to_string(),
        TokenKind::Atom(atom) if atom.quoted => format!("\"{}\"", atom.text),
        TokenKind::Atom(atom) => format!("'{}'", atom.text),
    }
}
