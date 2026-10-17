//! Specctra DSN, SES and rules files: lexer, typed syntax tree, and writer.
//!
//! Port of `app.freerouting.io.specctra` and `io.specctra.parser`. This crate
//! knows only the file format; building a board from it lives in `freerouting`.
//!
//! ```
//! use freerouting_dsn::Pcb;
//!
//! let pcb = Pcb::parse("(pcb demo (resolution um 10) (structure (layer F.Cu (type signal))))")?;
//! assert_eq!(pcb.structure.unwrap().layers[0].name, "F.Cu");
//! # Ok::<(), freerouting_dsn::Error>(())
//! ```
//!
//! # Reading
//!
//! [`Pcb::parse`], [`Session::parse`] and [`RulesFile::parse`] read a whole
//! file into the [`ast`] types. The `parse_with_warnings` variants also
//! return the problems that were skipped over. Files that are not valid UTF-8
//! can be decoded with [`decode`].
//!
//! The reader follows the Java reader's grammar: keywords are matched without
//! regard to case and with the same aliases (`clear`, `circ`, `rectangle`,
//! `prefered_direction`, ...); names are read with the same
//! token rules; unknown scopes are skipped, each unknown keyword with one
//! warning per file except KiCad's constraint-free `property` inside `layer`
//! (Q09.6; Java warns only for a scope without a keyword).
//! Errors and warnings carry the line and column where they were found.
//!
//! `wire_keepout` (KiCad's tracks-only rule area) is not an alias: the tree
//! keeps it as [`ast::KeepoutKind::WireKeepout`] and writes it back as
//! `wire_keepout`. Java's lexer reads it as `keepout`, and the engine's board
//! builder does the same unless `router.wire_keepout = "traces_only"` (Q09.3).
//! Inside a `(place ...)` scope, which holds names only, it is read as
//! `keepout`.
//!
//! # Writing
//!
//! [`Pcb::to_dsn`], [`Session::to_ses`] and [`RulesFile::to_rules`] write a
//! tree back as text. Reading the output gives back an identical tree.
//! [`Session::to_ses_java`] writes a session in the exact text layout of Java's
//! `SesWriter` (see [`write::java_style`]).
//!
//! # Differences from the Java reader
//!
//! The Java reader is a scanner and a board builder in one, and some of its
//! quirks come from that. This crate reads the same files, with these
//! differences:
//!
//! * Non-ASCII characters (Cyrillic, CJK, `µ`, `Ω`) are kept in names. The
//!   JFlex scanner drops characters outside its alphabet with a warning, which
//!   changes the names.
//! * Raw string reads (net names, pin references, class members, clearance
//!   types) skip tabs and newlines like spaces, and accept `'` as a quote
//!   character as the token scanner does. Java's raw reader stops at a newline
//!   in some places and only knows `"`.
//! * Empty quoted strings inside name lists are skipped wherever they occur;
//!   Java skips only a leading one (the KiCad 8 netlist bug) and ends the list
//!   at any other.
//! * Several scopes are read leniently where Java loses track of the brackets:
//!   a `class` without sub-scopes, a `layer_rule` with several `rule` scopes,
//!   a `place` without side or rotation, extra tokens before a closing bracket,
//!   unknown sub-scopes inside a `component` or `lock_type`. Such tokens are
//!   skipped with a warning instead.
//! * `autoroute_settings` in the structure is always read. Java reads it only
//!   if it comes before any keepout or plane scope.
//! * `(generated_by_freerouting)`, which the Java writer emits but its reader
//!   does not recognize, is accepted along with `(generated_by_freeroute)`.
//!   The writer emits `(generated_by_freeroute)`.
//! * The writer emits `snap_angle` and `via_rule`, which the reader
//!   recognizes; the Java writer emits `snapAngle` and `viaRule`, which it
//!   does not.
//! * Numbers may start with a decimal point (`.5`).
//! * `pcb` and `signal` as layer names are matched without regard to case or
//!   quoting in every shape; Java does so only in `polygon` scopes and
//!   otherwise compares the exact text.
//! * Degenerate shapes (paths with one point, zero-size circles) are kept in
//!   the tree; deciding what to do with them is up to the board builder.
//! * Names (padstacks, images, vias) may be any token, including words that
//!   are keywords elsewhere and numbers; Java requires a non-keyword string.
//! * An integer setting with a non-integer value is left unset with a
//!   warning; Java uses 0 and loses track of the brackets.
//! * An unterminated quoted string or `/*` comment is an error at its start.
//!   The JFlex scanner silently runs to the end of the file, after which the
//!   Java reader fails with a less specific message.

pub mod ast;
mod error;
mod lexer;
mod parse;
pub mod write;

use std::borrow::Cow;

pub use ast::{Pcb, RulesFile, Session};
pub use error::{Error, Pos, Result, Warning};

/// Decodes file contents: UTF-8 if valid, otherwise Latin-1, so that any
/// byte sequence can be handed to the parser.
pub fn decode(bytes: &[u8]) -> Cow<'_, str> {
    match std::str::from_utf8(bytes) {
        Ok(text) => Cow::Borrowed(text),
        Err(_) => Cow::Owned(bytes.iter().map(|&b| char::from(b)).collect()),
    }
}

fn run<T>(
    src: &str,
    read: impl FnOnce(&mut parse::Parser<'_>) -> Result<T>,
) -> Result<(T, Vec<Warning>)> {
    let mut parser = parse::Parser::new(src);
    let value = read(&mut parser)?;
    Ok((value, parser.into_warnings()))
}

impl Pcb {
    /// Reads a design file.
    pub fn parse(src: &str) -> Result<Pcb> {
        Self::parse_with_warnings(src).map(|(pcb, _)| pcb)
    }

    /// Reads a design file and returns the warnings too.
    pub fn parse_with_warnings(src: &str) -> Result<(Pcb, Vec<Warning>)> {
        run(src, parse::parse_pcb)
    }

    /// Writes the design as DSN text.
    pub fn to_dsn(&self) -> String {
        write::write_pcb(self)
    }
}

impl Session {
    /// Reads a session file.
    pub fn parse(src: &str) -> Result<Session> {
        Self::parse_with_warnings(src).map(|(session, _)| session)
    }

    /// Reads a session file and returns the warnings too.
    pub fn parse_with_warnings(src: &str) -> Result<(Session, Vec<Warning>)> {
        run(src, parse::parse_session)
    }

    /// Writes the session as SES text.
    pub fn to_ses(&self) -> String {
        write::write_session(self)
    }

    /// Writes the session in the exact text layout of Java's `SesWriter` (see
    /// [`write::java_style`]), with `"` as the string quote.
    pub fn to_ses_java(&self) -> String {
        write::java_style::write_session_java(self)
    }
}

impl RulesFile {
    /// Reads a rules file.
    pub fn parse(src: &str) -> Result<RulesFile> {
        Self::parse_with_warnings(src).map(|(rules, _)| rules)
    }

    /// Reads a rules file and returns the warnings too.
    pub fn parse_with_warnings(src: &str) -> Result<(RulesFile, Vec<Warning>)> {
        run(src, parse::parse_rules)
    }

    /// Writes the rules as text.
    pub fn to_rules(&self) -> String {
        write::write_rules(self)
    }
}
