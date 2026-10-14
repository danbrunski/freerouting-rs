//! Writers from the syntax tree back to Specctra text.
//!
//! The output reads back to an identical tree. Scopes are written in a fixed
//! order, so the text is not byte-for-byte the input. Names are quoted only
//! when they have to be.

mod common;
mod dsn;
mod rules;
mod ses;

pub(crate) use dsn::write_pcb;
pub(crate) use rules::write_rules;
pub(crate) use ses::write_session;

use std::borrow::Cow;

/// Points written on one line before wrapping.
const POINTS_PER_LINE: usize = 4;
/// Names written on one line before wrapping.
const NAMES_PER_LINE: usize = 8;

pub(crate) struct Writer {
    out: String,
    /// One entry per open scope: whether it has a nested scope or a line
    /// break, in which case its closing bracket goes on a line of its own.
    open: Vec<bool>,
    /// A line break was just written: the next word needs no separator.
    line_start: bool,
}

impl Writer {
    pub fn new() -> Self {
        Writer {
            out: String::new(),
            open: Vec::new(),
            line_start: false,
        }
    }

    pub fn finish(mut self) -> String {
        while !self.open.is_empty() {
            self.close();
        }
        self.out.push('\n');
        self.out
    }

    fn newline(&mut self) {
        self.out.push('\n');
        for _ in 0..self.open.len() {
            self.out.push_str("  ");
        }
    }

    /// Starts a scope `(keyword` on a new line.
    pub fn open(&mut self, keyword: &str) {
        if let Some(has_children) = self.open.last_mut() {
            *has_children = true;
            self.newline();
        }
        self.out.push('(');
        self.out.push_str(keyword);
        self.open.push(false);
        self.line_start = false;
    }

    /// Starts a scope `(keyword` on the current line.
    pub fn open_inline(&mut self, keyword: &str) {
        self.separator();
        self.out.push('(');
        self.out.push_str(keyword);
        self.open.push(false);
    }

    pub fn close(&mut self) {
        if self.open.pop() == Some(true) {
            self.newline();
        }
        self.out.push(')');
        self.line_start = false;
    }

    /// Breaks the line inside the current scope.
    pub fn line_break(&mut self) {
        if let Some(has_children) = self.open.last_mut() {
            *has_children = true;
        }
        self.newline();
        self.line_start = true;
    }

    fn separator(&mut self) {
        if self.line_start {
            self.line_start = false;
        } else {
            self.out.push(' ');
        }
    }

    /// Writes a word as is: a keyword or number.
    pub fn word(&mut self, word: &str) {
        self.separator();
        self.out.push_str(word);
    }

    /// Writes a name, quoting it if needed.
    pub fn name(&mut self, name: &str) {
        let quoted = quote(name, None);
        self.word(&quoted);
    }

    /// Writes a name that must be read back as a name, never as a keyword.
    pub fn quoted(&mut self, name: &str) {
        let quoted = force_quote(name);
        self.word(&quoted);
    }

    pub fn number(&mut self, value: f64) {
        self.word(&format_number(value));
    }

    pub fn integer(&mut self, value: i32) {
        self.word(&value.to_string());
    }

    pub fn on_off(&mut self, keyword: &str, value: bool) {
        self.open(keyword);
        self.word(if value { "on" } else { "off" });
        self.close();
    }

    /// `(keyword name)`.
    pub fn name_scope(&mut self, keyword: &str, value: &str) {
        self.open(keyword);
        self.name(value);
        self.close();
    }

    /// Writes names, wrapping long lists.
    pub fn names<'s>(&mut self, names: impl IntoIterator<Item = &'s String>) {
        for (i, name) in names.into_iter().enumerate() {
            if i > 0 && i % NAMES_PER_LINE == 0 {
                self.line_break();
            }
            self.name(name);
        }
    }
}

/// Formats a number so that it parses back to the same value: the shortest
/// decimal representation, never in exponent form.
pub(crate) fn format_number(value: f64) -> String {
    format!("{value}")
}

fn is_blank(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | '\u{8}')
}

/// Quotes `name` if it would not read back as one token: if it is empty,
/// starts with a quote character or a comment, or contains blanks, brackets
/// or `sep`.
pub(crate) fn quote(name: &str, sep: Option<char>) -> Cow<'_, str> {
    let needs_quotes = name.is_empty()
        || name.starts_with(['"', '\'', '#'])
        || name.starts_with("/*")
        || name
            .chars()
            .any(|c| is_blank(c) || c == '(' || c == ')' || Some(c) == sep);
    if needs_quotes {
        force_quote(name)
    } else {
        Cow::Borrowed(name)
    }
}

/// Quotes `name` with `"`, or with `'` if it contains `"`.
pub(crate) fn force_quote(name: &str) -> Cow<'_, str> {
    let q = if name.contains('"') && !name.contains('\'') {
        '\''
    } else {
        '"'
    };
    Cow::Owned(format!("{q}{name}{q}"))
}
