//! Session files in the exact text layout of Java's `SesWriter`.
//!
//! The plain writer ([`Session::to_ses`](crate::Session::to_ses)) puts a scope's closing
//! bracket on its own line only when needed, prints numbers without a trailing `.0` and never
//! uses exponents. Java's writer does none of that, and everything that consumes Freerouting's
//! output (KiCad's importer, the plugin, the board-parity gate that compares session files) was
//! built against Java's text. This module reproduces it byte for byte, so a Java reference file
//! is an oracle and the output does not change when the engine does.
//!
//! What is ported, at `cda540f`:
//!
//! * `IndentFileWriter`: [`JavaIndentWriter`] (`startScope`, `endScope`, `newLine`, two spaces
//!   per level, UTF-8, no newline at the end of the file);
//! * `IdentifierType.write`: [`java_ident`], with one fix (see below);
//! * `Double.toString` as the JDK prints it since 19 ([`java_double`]) and
//!   `BigDecimal.valueOf(d).stripTrailingZeros().toPlainString()` ([`java_plain_decimal`], used
//!   for component rotations);
//! * the write order of `SesWriter.writeSessionScope`, the reduced `Parser.writeScope`,
//!   `Resolution.writeScope` and the `writeScopeInt` of `Rectangle`, `Circle`, `Polygon`,
//!   `PolygonPath` and `PolylinePath` ([`write_session_java`]).
//!
//! # Integers and doubles
//!
//! Java writes some coordinates as `int` (path corners, wire widths, padstack shapes, the
//! shape of a conduction area) and some as `double` (placement and via coordinates, the holes of
//! a conduction area). The syntax tree keeps every number as `f64`, so the serializer knows
//! from the scope which form to use. Values of an `int` scope are expected to be whole numbers
//! (the board writer rounds them with Java's `Math.round`); a fractional value is rounded half
//! up, and the sum of that is all this crate knows of Java's rounding.
//!
//! # Differences from Java
//!
//! `IdentifierType.write` removes outer double quotes in a loop with `substring(1, len - 2)`,
//! which also drops the last real character of the name. [`java_ident`] cuts only the two
//! quotes (`identifier_with_outer_quotes_keeps_last_char`). A name that arrives with outer
//! quotes is rare (the DSN reader strips them), and no reference session depends on it.
//!
//! Pieces of the tree that Java's writer has no place for are not written: a parser scope's
//! `string_quote`, `space_in_quoted_tokens` and `generated_by_freerouting` (the reduced scope),
//! `absolute` and `attach on` of a padstack, a wire's net, clearance class and window-less
//! extras, and a via's net and class.

use std::borrow::Cow;
use std::io::{self, Write};

use crate::ast::*;

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

/// The shortest decimal digits of the positive finite `v` as Java's `Double.toString` chooses
/// them, and the scientific exponent: `v = d.ddd * 10^exponent`.
///
/// Rust and Java (JDK 19 and later) both pick the shortest digit string that reads back as `v`,
/// the closest to `v` if there are several. Java renders at least two digits, and for a value
/// whose shortest form has one digit it takes the two-digit decimal closest to `v` (this shows
/// for subnormals only: `Double.MIN_VALUE` prints as `4.9E-324`, not `5E-324`).
fn java_digits(v: f64) -> (String, i32) {
    fn split(sci: &str) -> (String, i32) {
        let (mantissa, exponent) = sci.split_once('e').unwrap_or((sci, "0"));
        let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
        (digits, exponent.parse().unwrap_or(0))
    }
    let (digits, exponent) = split(&format!("{v:e}"));
    if digits.len() == 1 {
        let two = format!("{v:.1e}");
        if two.parse::<f64>().ok() == Some(v) {
            let (two_digits, two_exponent) = split(&two);
            if !two_digits.ends_with('0') {
                return (two_digits, two_exponent);
            }
        }
    }
    (digits, exponent)
}

/// `Double.toString(v)` as the JDK prints it since 19: the shortest digits that read back
/// as `v`, at least one digit after the point, and `d.dddE±n` (no plus sign) below 10^-3 and at
/// or above 10^7.
pub fn java_double(v: f64) -> String {
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0" } else { "0.0" }.to_string();
    }
    let magnitude = v.abs();
    let (digits, exponent) = java_digits(magnitude);
    let mut out = String::with_capacity(digits.len() + 8);
    if v < 0.0 {
        out.push('-');
    }
    if (1e-3..1e7).contains(&magnitude) {
        if exponent >= 0 {
            let int_len = exponent as usize + 1;
            if digits.len() <= int_len {
                out.push_str(&digits);
                out.extend(std::iter::repeat_n('0', int_len - digits.len()));
                out.push_str(".0");
            } else {
                out.push_str(&digits[..int_len]);
                out.push('.');
                out.push_str(&digits[int_len..]);
            }
        } else {
            out.push_str("0.");
            out.extend(std::iter::repeat_n('0', (-exponent - 1) as usize));
            out.push_str(&digits);
        }
    } else {
        out.push_str(&digits[..1]);
        out.push('.');
        out.push_str(if digits.len() > 1 { &digits[1..] } else { "0" });
        out.push('E');
        out.push_str(&exponent.to_string());
    }
    out
}

/// `BigDecimal.valueOf(v).stripTrailingZeros().toPlainString()`: the digits of
/// `Double.toString(v)` without trailing zeros and without an exponent. Used for component
/// rotations: `339.0` is `339`, `338.5` stays, `0.0` and `-0.0` are `0`.
///
/// `NaN` and the infinities (which make `BigDecimal.valueOf` throw) are written like
/// [`java_double`].
pub fn java_plain_decimal(v: f64) -> String {
    if !v.is_finite() {
        return java_double(v);
    }
    if v == 0.0 {
        return "0".to_string();
    }
    let (digits, exponent) = java_digits(v.abs());
    let digits = digits.trim_end_matches('0');
    let mut out = String::new();
    if v < 0.0 {
        out.push('-');
    }
    let int_len = i64::from(exponent) + 1;
    if int_len <= 0 {
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-int_len) as usize));
        out.push_str(digits);
    } else if digits.len() as i64 <= int_len {
        out.push_str(digits);
        out.extend(std::iter::repeat_n(
            '0',
            (int_len - digits.len() as i64) as usize,
        ));
    } else {
        out.push_str(&digits[..int_len as usize]);
        out.push('.');
        out.push_str(&digits[int_len as usize..]);
    }
    out
}

/// A value of an `int` scope as Java prints it: `String.valueOf((int) Math.round(x))` for
/// the whole numbers the board writer hands over (see the module docs).
fn java_int(v: f64) -> String {
    if v.is_nan() {
        return "0".to_string();
    }
    let rounded = (v + 0.5).floor();
    (rounded as i64 as i32).to_string()
}

// ---------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------

/// The characters that force quotes around a name (`SesWriter.write`).
const RESERVED: [char; 10] = ['(', ')', ' ', ';', '-', '_', '/', '~', '{', '}'];

/// `IdentifierType.write`: `name` as it goes into a session file, quoted with `quote` when it
/// has to be.
///
/// Outer double quotes of the name are removed, then every occurrence of `quote`. The result is
/// put in quotes if it contains a reserved character (`( ) space ; - _ / ~ { }`), any byte that
/// is not positive in Java's signed `byte` (a non-ASCII character or NUL) or, failing that,
/// matches `^-?\d.*` (a number must not read back as a number).
///
/// Differs from Java in one place: the unwrapping of outer quotes keeps the last real character.
pub fn java_ident<'a>(name: &'a str, quote: &str) -> Cow<'a, str> {
    let mut name = name;
    while name.len() > 2 && name.starts_with('"') && name.ends_with('"') {
        name = &name[1..name.len() - 1];
    }
    let name: Cow<'a, str> = if !quote.is_empty() && name.contains(quote) {
        Cow::Owned(name.replace(quote, ""))
    } else {
        Cow::Borrowed(name)
    };
    if needs_quotes(&name) {
        Cow::Owned(format!("{quote}{name}{quote}"))
    } else {
        name
    }
}

fn needs_quotes(name: &str) -> bool {
    if name.contains(RESERVED) || name.bytes().any(|b| b == 0 || b >= 0x80) {
        return true;
    }
    looks_like_number(name)
}

/// `name.matches("^-?\\d.*")`: Java's `.` does not match line terminators.
fn looks_like_number(name: &str) -> bool {
    let rest = name.strip_prefix('-').unwrap_or(name);
    let mut chars = rest.chars();
    chars.next().is_some_and(|c| c.is_ascii_digit())
        && !chars.any(|c| matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'))
}

// ---------------------------------------------------------------------------
// IndentFileWriter
// ---------------------------------------------------------------------------

/// `IndentFileWriter`: scopes with two spaces of indent per level. A scope starts with a new
/// line and `(`; its closing bracket is on a line of its own.
pub struct JavaIndentWriter<W: Write> {
    out: W,
    level: usize,
}

impl<W: Write> JavaIndentWriter<W> {
    pub fn new(out: W) -> Self {
        JavaIndentWriter { out, level: 0 }
    }

    /// Gives back the sink.
    pub fn into_inner(self) -> W {
        self.out
    }

    /// `startScope(newLine)`: optionally a new line, then `(`.
    pub fn start_scope_on(&mut self, new_line: bool) -> io::Result<()> {
        if new_line {
            self.new_line()?;
        }
        self.out.write_all(b"(")?;
        self.level += 1;
        Ok(())
    }

    /// `startScope()`: a new line, then `(`.
    pub fn start_scope(&mut self) -> io::Result<()> {
        self.start_scope_on(true)
    }

    /// `endScope()`: a new line at the outer level, then `)`.
    pub fn end_scope(&mut self) -> io::Result<()> {
        self.level = self.level.saturating_sub(1);
        self.new_line()?;
        self.out.write_all(b")")
    }

    /// `newLine()`: `\n` and the indent of the current level.
    pub fn new_line(&mut self) -> io::Result<()> {
        self.out.write_all(b"\n")?;
        for _ in 0..self.level {
            self.out.write_all(b"  ")?;
        }
        Ok(())
    }

    /// `write(String)`.
    pub fn write(&mut self, text: &str) -> io::Result<()> {
        self.out.write_all(text.as_bytes())
    }
}

// ---------------------------------------------------------------------------
// The session
// ---------------------------------------------------------------------------

/// Writes `session` in Java's layout, with `"` as the string quote.
pub fn write_session_java(session: &Session) -> String {
    write_session_java_with_quote(session, "\"")
}

/// Writes `session` in Java's layout, quoting names with `quote` (the board's
/// `string_quote`, which Java's `IdentifierType` is built with).
pub fn write_session_java_with_quote(session: &Session, quote: &str) -> String {
    let mut bytes = Vec::new();
    // Writing to a Vec cannot fail.
    let _ = write_session_java_to(session, quote, &mut bytes);
    String::from_utf8(bytes).unwrap_or_default()
}

/// Writes `session` in Java's layout to `out`.
pub fn write_session_java_to<W: Write>(session: &Session, quote: &str, out: W) -> io::Result<()> {
    let mut s = SessionWriter {
        file: JavaIndentWriter::new(out),
        quote,
    };
    s.session(session)?;
    s.file.out.flush()
}

struct SessionWriter<'q, W: Write> {
    file: JavaIndentWriter<W>,
    quote: &'q str,
}

impl<W: Write> SessionWriter<'_, W> {
    fn ident(&mut self, name: &str) -> io::Result<()> {
        let text = java_ident(name, self.quote);
        self.file.write(&text)
    }

    fn session(&mut self, session: &Session) -> io::Result<()> {
        self.file.start_scope_on(false)?;
        self.file.write("session ")?;
        self.ident(&session.name)?;
        if let Some(base_design) = &session.base_design {
            self.file.new_line()?;
            self.file.write("(base_design ")?;
            self.ident(base_design)?;
            self.file.write(")")?;
        }
        if let Some(placement) = &session.placement {
            self.placement(placement)?;
        }
        if let Some(swaps) = &session.was_is {
            self.was_is(swaps)?;
        }
        if let Some(routes) = &session.routes {
            self.routes(routes)?;
        }
        self.file.end_scope()
    }

    fn resolution(&mut self, resolution: &Resolution) -> io::Result<()> {
        self.file.new_line()?;
        self.file.write("(resolution ")?;
        self.file.write(resolution.unit.name())?;
        self.file.write(" ")?;
        self.file.write(&resolution.value.to_string())?;
        self.file.write(")")
    }

    fn placement(&mut self, placement: &Placement) -> io::Result<()> {
        self.file.start_scope()?;
        self.file.write("placement")?;
        if let Some(resolution) = &placement.resolution {
            self.resolution(resolution)?;
        }
        for component in &placement.components {
            self.file.start_scope()?;
            self.file.write("component ")?;
            self.ident(&component.image)?;
            for place in &component.places {
                self.place(place)?;
            }
            self.file.end_scope()?;
        }
        self.file.end_scope()
    }

    fn place(&mut self, place: &Place) -> io::Result<()> {
        self.file.new_line()?;
        self.file.write("(place ")?;
        self.ident(&place.name)?;
        if let Some(location) = &place.location {
            self.file.write(" ")?;
            self.file.write(&java_double(location.at.x))?;
            self.file.write(" ")?;
            self.file.write(&java_double(location.at.y))?;
            self.file.write(match location.side {
                Side::Front => " front ",
                Side::Back => " back ",
            })?;
            self.file.write(&java_plain_decimal(location.rotation))?;
            if place.position_locked {
                self.file.new_line()?;
                self.file.write(" (lock_type position)")?;
            }
        }
        self.file.write(")")
    }

    fn pin_ref(&mut self, pin: &PinRef) -> io::Result<()> {
        self.ident(&pin.component)?;
        self.file.write("-")?;
        self.ident(&pin.pin)
    }

    fn was_is(&mut self, swaps: &[PinSwap]) -> io::Result<()> {
        self.file.start_scope()?;
        self.file.write("was_is")?;
        for swap in swaps {
            self.file.new_line()?;
            self.file.write("(pins ")?;
            self.pin_ref(&swap.was)?;
            self.file.write(" ")?;
            self.pin_ref(&swap.is)?;
            self.file.write(")")?;
        }
        self.file.end_scope()
    }

    fn routes(&mut self, routes: &Routes) -> io::Result<()> {
        self.file.start_scope()?;
        self.file.write("routes ")?;
        if let Some(resolution) = &routes.resolution {
            self.resolution(resolution)?;
        }
        if let Some(parser) = &routes.parser {
            self.parser(parser)?;
        }
        self.file.start_scope()?;
        self.file.write("library_out ")?;
        for padstack in &routes.library_out {
            self.padstack(padstack)?;
        }
        self.file.end_scope()?;
        self.file.start_scope()?;
        self.file.write("network_out ")?;
        for net in &routes.network_out {
            self.net(net)?;
        }
        self.file.end_scope()?;
        self.file.end_scope()
    }

    /// `Parser.writeScope(file, parserInfo, identifierType, true)`: the reduced scope.
    fn parser(&mut self, info: &ParserInfo) -> io::Result<()> {
        self.file.start_scope()?;
        self.file.write("parser")?;
        if let Some(host_cad) = &info.host_cad {
            self.file.new_line()?;
            self.file.write("(host_cad ")?;
            self.ident(host_cad)?;
            self.file.write(")")?;
        }
        if let Some(host_version) = &info.host_version {
            self.file.new_line()?;
            self.file.write("(host_version ")?;
            self.ident(host_version)?;
            self.file.write(")")?;
        }
        for (name, value) in &info.constants {
            self.file.new_line()?;
            self.file.write("(constant ")?;
            for part in [name, value] {
                self.ident(part)?;
                self.file.write(" ")?;
            }
            self.file.write(")")?;
        }
        if let Some(resolution) = &info.write_resolution {
            self.file.new_line()?;
            self.file.write("(write_resolution ")?;
            let first: String = resolution.name.chars().take(1).collect();
            self.file.write(&first)?;
            self.file.write(" ")?;
            self.file.write(&resolution.value.to_string())?;
            self.file.write(")")?;
        }
        self.file.end_scope()
    }

    fn padstack(&mut self, padstack: &Padstack) -> io::Result<()> {
        self.file.start_scope()?;
        self.file.write("padstack ")?;
        self.ident(&padstack.name)?;
        for shape in &padstack.shapes {
            self.file.start_scope()?;
            self.file.write("shape")?;
            self.shape(shape, true)?;
            self.file.end_scope()?;
        }
        if padstack.attach == Some(false) {
            self.file.new_line()?;
            self.file.write("(attach off)")?;
        }
        self.file.end_scope()
    }

    fn layer(&mut self, layer: &LayerRef) -> io::Result<()> {
        match layer {
            LayerRef::Named(name) => self.ident(name),
            other => self.ident(other.name()),
        }
    }

    /// The `writeScopeInt` (`int_form`) or `writeScope` of a shape class.
    fn shape(&mut self, shape: &Shape, int_form: bool) -> io::Result<()> {
        let number = |v: f64| {
            if int_form {
                java_int(v)
            } else {
                java_double(v)
            }
        };
        match shape {
            Shape::Rect {
                layer,
                x1,
                y1,
                x2,
                y2,
            } => {
                self.file.new_line()?;
                self.file.write("(rect ")?;
                self.layer(layer)?;
                for v in [x1, y1, x2, y2] {
                    self.file.write(" ")?;
                    self.file.write(&number(*v))?;
                }
                self.file.write(")")
            }
            Shape::Circle {
                layer,
                diameter,
                center,
            } => {
                self.file.new_line()?;
                self.file.write("(circle ")?;
                self.layer(layer)?;
                for v in [diameter, &center.x, &center.y] {
                    self.file.write(" ")?;
                    self.file.write(&number(*v))?;
                }
                self.file.write(")")
            }
            Shape::Polygon { layer, points, .. } => {
                self.file.start_scope()?;
                self.file.write("polygon ")?;
                self.layer(layer)?;
                self.file.write(" 0")?;
                self.corners(points, &number)?;
                self.file.end_scope()
            }
            Shape::Path {
                layer,
                width,
                points,
            } => {
                self.file.start_scope()?;
                self.file.write("path ")?;
                self.layer(layer)?;
                self.file.write(" ")?;
                self.file.write(&number(*width))?;
                self.corners(points, &number)?;
                self.file.end_scope()
            }
            Shape::PolylinePath {
                layer,
                width,
                points,
            } => {
                self.file.start_scope()?;
                self.file.write("polyline_path ")?;
                self.layer(layer)?;
                self.file.write(" ")?;
                self.file.write(&number(*width))?;
                for line in points.chunks_exact(2) {
                    self.file.new_line()?;
                    for p in line {
                        self.file.write(&number(p.x))?;
                        self.file.write(" ")?;
                        self.file.write(&number(p.y))?;
                        self.file.write(" ")?;
                    }
                }
                self.file.end_scope()
            }
        }
    }

    fn corners(&mut self, points: &[Point], number: &dyn Fn(f64) -> String) -> io::Result<()> {
        for p in points {
            self.file.new_line()?;
            self.file.write(&number(p.x))?;
            self.file.write(" ")?;
            self.file.write(&number(p.y))?;
        }
        Ok(())
    }

    fn net(&mut self, net: &SessionNet) -> io::Result<()> {
        self.file.start_scope()?;
        self.file.write("net ")?;
        self.ident(&net.name)?;
        for item in &net.items {
            match item {
                WiringItem::Wire(wire) => self.wire(wire)?,
                WiringItem::Via(via) => self.via(via)?,
            }
        }
        self.file.end_scope()
    }

    fn wire(&mut self, wire: &Wire) -> io::Result<()> {
        let is_trace = matches!(
            wire.shapes.first(),
            Some(Shape::Path { .. } | Shape::PolylinePath { .. })
        );
        self.file.start_scope()?;
        if is_trace {
            self.file.write("wire")?;
            for shape in &wire.shapes {
                self.shape(shape, true)?;
            }
            self.fixed_state(wire.wire_type.as_ref())?;
        } else {
            // A conduction area: the boundary as `int`s, the holes as doubles.
            self.file.write("wire ")?;
            for shape in &wire.shapes {
                self.shape(shape, true)?;
            }
            for window in &wire.windows {
                self.file.start_scope()?;
                self.file.write("window")?;
                self.shape(window, false)?;
                self.file.end_scope()?;
            }
            // Q22.4's own attribute; Java writes none
            if let Some(attr) = &wire.attr {
                self.file.start_scope()?;
                self.file.write("attr ")?;
                self.ident(attr)?;
                self.file.end_scope()?;
            }
        }
        self.file.end_scope()
    }

    fn via(&mut self, via: &Via) -> io::Result<()> {
        self.file.start_scope()?;
        self.file.write("via ")?;
        self.ident(&via.padstack)?;
        self.file.write(" ")?;
        self.file.write(&java_double(via.at.x))?;
        self.file.write(" ")?;
        self.file.write(&java_double(via.at.y))?;
        self.fixed_state(via.via_type.as_ref())?;
        self.file.end_scope()
    }

    /// `writeFixedState`: nothing up to `SHOVE_FIXED`, `(type fix)` for `SYSTEM_FIXED` and
    /// `(type protect)` for the rest.
    fn fixed_state(&mut self, wire_type: Option<&WireType>) -> io::Result<()> {
        let state = wire_type.map_or(FixedState::Unfixed, WireType::fixed_state);
        let text = match state {
            FixedState::Unfixed | FixedState::ShoveFixed => return Ok(()),
            FixedState::SystemFixed => "(type fix)",
            FixedState::UserFixed => "(type protect)",
        };
        self.file.new_line()?;
        self.file.write(text)
    }
}
