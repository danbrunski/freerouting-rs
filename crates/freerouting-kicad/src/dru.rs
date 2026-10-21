//! The board's `.kicad_dru` custom rules (K07.5).
//!
//! KiCad's DSN export ignores this file, so Java never saw its rules. The
//! reader reads it, says what became of every rule, and applies the part the
//! DSN can say when [`crate::ReadOptions::apply_design_rules`] is on:
//!
//! - a **clearance** constraint (`min`) between two net classes becomes a
//!   `(class_class (classes a b) (rule (clearance v)))`; with a `(layer ...)`
//!   clause it becomes a `layer_rule` of that class pair;
//! - a **track width** constraint (`opt`) on net classes sets those classes'
//!   `(width v)`; the Default class also sets the structure's width rule;
//! - (Q22.5) a **clearance** whose condition asks about item types
//!   (`A.Type`, `B.Type == A.Type`, `A.isPlated()`, `A.Pad_Type`) becomes the
//!   DSN's typed clearances, and a **hole_clearance** a floor of
//!   `router.hole_clearance_um` ([`DruReport::hole_clearance`]); see
//!   [`item_types`] for the mapping, what is applied more widely (with a
//!   warning in [`DruReport::warnings`]) and what is left out.
//!
//! A DSN input gets the same with [`apply_to_pcb`] (`--kicad-dru`, Q22.5).
//! A `(layer ...)` clause that names no copper layer of the board (`inner` on
//! a two-layer board) leaves its rule out.
//!
//! Class-pair rules apply in file order and a later rule overrides an earlier
//! one for the same target (KiCad evaluates rules last to first and the first
//! match wins, so the last matching rule in the file wins); item-type rules
//! keep the largest value of a kind pair. Everything else is reported by name
//! and kept out: a condition with a term this module cannot evaluate
//! (`insideCourtyard`, `intersectsArea`, `inDiffPair`, ...) makes the rule
//! UNSUPPORTED and names the first such term, in both modes; so does a
//! constraint kind other than `clearance`, `hole_clearance` and `track_width`
//! and a rule without a condition.
//!
//! # Conditions
//!
//! The condition language is tokenised and parsed into a [`Cond`] tree:
//! `A.NetClass == 'x'` and `!=` (exact compare with the net's effective
//! class name, so an aggregate class `Power,Default` is named as a whole),
//! `A.hasNetclass('x')` (also true when an aggregate has `x` as a member),
//! `A.hasExactNetclass('x')`, `onLayer('F.Cu')` and `A.Layer == 'F.Cu'` (layer
//! selection, only in the top-level conjunction), `&&`, `||`, `!` and
//! parentheses; the item terms `X.Type == 't'`, `B.Type == A.Type`,
//! `X.isPlated()`, `X.Pad_Type == 'v'` and `A.Net == B.Net` / `!=` (the
//! router keeps clearances only between different nets). `B.` terms refer to
//! the other item. The class pairs a rule
//! applies to are found by evaluating the condition for every ordered pair of
//! the board's net classes; a pair counts when either order is true. The
//! Default class is `Default` in a condition and `kicad_default` in the DSN.
//!
//! Syntax follows KRT's `kicad_dru.py` (KiCadRoutingTools, MIT; ideas only,
//! no code taken): unsupported rules are kept and reported, never dropped
//! (KRT #770).

use freerouting_dsn::ast::{ClassClass, LayerRule, Network, Rule, Structure};

use crate::sexpr::Tree;

mod item_types;

pub use item_types::{ItemType, Kind};

/// A `(constraint kind (min v) (opt v) (max v))` value in nanometres.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bounds {
    pub min: Option<i64>,
    pub opt: Option<i64>,
    pub max: Option<i64>,
}

/// One term of a condition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cond {
    /// `X.NetClass == 'c'` (`exact` always) or `X.hasNetclass('c')`.
    NetClass {
        side: Side,
        class: String,
        exact: bool,
    },
    /// `onLayer('L')` or `A.Layer == 'L'`.
    Layer(String),
    /// `X.Type == 't'` (Q22.5), compared without case: `track` (and `arc`),
    /// `via`, `pad`, or a type the router keeps no clearance for.
    Type {
        side: Side,
        value: String,
    },
    /// `B.Type == A.Type` (either order).
    SameType,
    /// `X.isPlated()`: a via or a plated through-hole pad.
    Plated(Side),
    /// `X.Pad_Type == 'v'`, compared without case: `Through-hole`, `SMD`,
    /// `Edge connector` or `NPTH, mechanical`; false for an item that is not
    /// a pad.
    PadType {
        side: Side,
        value: String,
    },
    /// `A.Net == B.Net` (either order). The router keeps clearances only
    /// between different nets, so this term is false for every pair it
    /// models.
    SameNet,
    Not(Box<Cond>),
    And(Vec<Cond>),
    Or(Vec<Cond>),
    /// A term this module cannot evaluate, as written.
    Unsupported(String),
}

/// Which item of the pair a term talks about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    A,
    B,
}

/// What became of one constraint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Applied to the DSN tree; `effects` counts class pairs or classes.
    Applied { effects: usize },
    /// Not applied, for the reason given: [`crate::ReadOptions::apply_design_rules`]
    /// is off, the severity is `ignore`, the rule's layers are not on the
    /// board, or (Q22.5) it asks for no more than the DSN gives or covers no
    /// pair the router places.
    Ignored(String),
    /// Not expressible; the reason names the first unsupported term.
    Unsupported(String),
}

/// A constraint of a rule and its fate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstraintReport {
    pub kind: String,
    pub bounds: Bounds,
    pub status: Status,
}

/// A rule of the file and its fate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleReport {
    pub name: String,
    pub line: u32,
    pub condition: Option<String>,
    pub layers: Vec<String>,
    pub constraints: Vec<ConstraintReport>,
}

impl RuleReport {
    /// The reason of the first UNSUPPORTED constraint.
    pub fn unsupported_reason(&self) -> Option<&str> {
        self.constraints.iter().find_map(|c| match &c.status {
            Status::Unsupported(r) => Some(r.as_str()),
            _ => None,
        })
    }
}

/// A `hole_clearance` the router applies to every hole (Q22.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoleClearance {
    /// The largest `min` of the applied rules, in nanometres.
    pub nm: i64,
    /// The names of those rules, in file order.
    pub rules: Vec<String>,
}

/// Every rule of the file, in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DruReport {
    /// Whether a `.kicad_dru` was read at all.
    pub present: bool,
    pub rules: Vec<RuleReport>,
    /// Rules applied more widely than they ask (Q22.5), one line each, naming
    /// the rule.
    pub warnings: Vec<String>,
    /// The `hole_clearance` floor of the applied rules (Q22.5): the caller
    /// raises `router.hole_clearance_um` to it.
    pub hole_clearance: Option<HoleClearance>,
}

impl DruReport {
    /// The rules with at least one UNSUPPORTED constraint.
    pub fn unsupported(&self) -> impl Iterator<Item = &RuleReport> {
        self.rules
            .iter()
            .filter(|r| r.unsupported_reason().is_some())
    }

    /// One line per rule and constraint, as the CLI and the GUI print them.
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for r in &self.rules {
            for c in &r.constraints {
                let (state, why) = match &c.status {
                    Status::Applied { effects } => ("applied", format!("{effects} target(s)")),
                    Status::Ignored(w) => ("ignored", w.clone()),
                    Status::Unsupported(w) => ("UNSUPPORTED", w.clone()),
                };
                out.push(format!(
                    "design rule '{}' (line {}), constraint {}: {state} ({why})",
                    r.name, r.line, c.kind
                ));
            }
            if r.constraints.is_empty() {
                out.push(format!(
                    "design rule '{}' (line {}): UNSUPPORTED (no constraint)",
                    r.name, r.line
                ));
            }
        }
        out
    }
}

/// A parsed rule before it is applied.
#[derive(Clone, Debug, PartialEq)]
pub struct DruRule {
    pub name: String,
    pub line: u32,
    pub condition_text: Option<String>,
    pub condition: Option<Cond>,
    pub layers: Vec<String>,
    pub severity: Option<String>,
    pub constraints: Vec<(String, Bounds)>,
}

/// The parsed file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DruFile {
    pub version: Option<i64>,
    pub rules: Vec<DruRule>,
}

/// Replaces `#` comments (outside strings) by blanks so line numbers stay.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_str = false;
    let mut in_comment = false;
    let mut prev = '\0';
    for ch in text.chars() {
        if in_comment {
            if ch == '\n' {
                in_comment = false;
                out.push('\n');
            } else {
                out.push(' ');
            }
            prev = ch;
            continue;
        }
        if in_str {
            if ch == '"' && prev != '\\' {
                in_str = false;
            }
        } else if ch == '"' {
            in_str = true;
        } else if ch == '#' {
            in_comment = true;
            out.push(' ');
            prev = ch;
            continue;
        }
        out.push(ch);
        prev = ch;
    }
    out
}

/// A distance with a unit, in nanometres. A bare number is millimetres.
pub fn parse_distance(text: &str) -> Option<i64> {
    let t = text.trim();
    let split = t.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let v: f64 = num.trim().parse().ok()?;
    let nm_per_unit = match unit.trim() {
        "" | "mm" => 1e6,
        "um" => 1e3,
        "nm" => 1.0,
        "in" => 25.4e6,
        "mil" => 25.4e3,
        _ => return None,
    };
    let nm = (v * nm_per_unit).round();
    nm.is_finite().then_some(nm as i64)
}

impl DruFile {
    /// Parses the text of a `.kicad_dru`.
    pub fn parse(text: &str) -> Result<DruFile, crate::KicadError> {
        let clean = strip_comments(text);
        let tree = Tree::parse(&clean)?;
        let mut file = DruFile::default();
        for item in tree.root().items().filter(|i| i.is_list()) {
            match item.head() {
                Some("version") => file.version = item.atom(1).and_then(|v| v.parse().ok()),
                Some("rule") => file.rules.push(parse_rule(&item)),
                _ => {}
            }
        }
        Ok(file)
    }
}

fn parse_rule(item: &crate::sexpr::NodeRef<'_>) -> DruRule {
    let name = item
        .str_value(1)
        .map(|n| n.into_owned())
        .unwrap_or_default();
    let mut rule = DruRule {
        name,
        line: item.line(),
        condition_text: None,
        condition: None,
        layers: Vec::new(),
        severity: None,
        constraints: Vec::new(),
    };
    for c in item.items().skip(2).filter(|c| c.is_list()) {
        match c.head() {
            Some("condition") => {
                if let Some(t) = c.str_value(1) {
                    rule.condition = Some(parse_condition(&t));
                    rule.condition_text = Some(t.into_owned());
                }
            }
            Some("layer") => {
                rule.layers
                    .extend((1..c.len()).filter_map(|i| c.str_value(i).map(|s| s.into_owned())));
            }
            Some("severity") => rule.severity = c.str_value(1).map(|s| s.into_owned()),
            Some("constraint") => {
                let kind = c.str_value(1).map(|k| k.into_owned()).unwrap_or_default();
                let mut b = Bounds::default();
                for v in c.items().skip(2).filter(|v| v.is_list()) {
                    let n = v.str_value(1).and_then(|t| parse_distance(&t));
                    match v.head() {
                        Some("min") => b.min = n,
                        Some("opt") => b.opt = n,
                        Some("max") => b.max = n,
                        _ => {}
                    }
                }
                rule.constraints.push((kind, b));
            }
            _ => {}
        }
    }
    rule
}

// ---- the condition language ------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Str(String),
    Num(String),
    Punct(&'static str),
    Other(String),
}

fn tokenize(text: &str) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '\'' || c == '"' {
            let mut s = String::new();
            i += 1;
            while i < chars.len() && chars[i] != c {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                }
                s.push(chars[i]);
                i += 1;
            }
            i += 1;
            out.push(Tok::Str(s));
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '.') {
                i += 1;
            }
            out.push(Tok::Num(chars[start..i].iter().collect()));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
        } else {
            let two: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let p2 = ["&&", "||", "==", "!=", "<=", ">="]
                .into_iter()
                .find(|p| *p == two);
            if let Some(p) = p2 {
                out.push(Tok::Punct(p));
                i += 2;
                continue;
            }
            let p1 = ["(", ")", "!", ",", "<", ">"]
                .into_iter()
                .find(|p| p.starts_with(c));
            match p1 {
                Some(p) => out.push(Tok::Punct(p)),
                None => out.push(Tok::Other(c.to_string())),
            }
            i += 1;
        }
    }
    out
}

struct CondParser {
    toks: Vec<Tok>,
    pos: usize,
}

impl CondParser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn eat(&mut self, p: &'static str) -> bool {
        if self.peek() == Some(&Tok::Punct(p)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn or(&mut self) -> Cond {
        let mut parts = vec![self.and()];
        while self.eat("||") {
            parts.push(self.and());
        }
        if parts.len() == 1 {
            parts.remove(0)
        } else {
            Cond::Or(parts)
        }
    }

    fn and(&mut self) -> Cond {
        let mut parts = vec![self.unary()];
        while self.eat("&&") {
            parts.push(self.unary());
        }
        if parts.len() == 1 {
            parts.remove(0)
        } else {
            Cond::And(parts)
        }
    }

    fn unary(&mut self) -> Cond {
        if self.eat("!") {
            return Cond::Not(Box::new(self.unary()));
        }
        if self.eat("(") {
            let inner = self.or();
            self.eat(")");
            return inner;
        }
        self.atom()
    }

    /// Consumes tokens up to the next `&&`, `||` or unbalanced `)` and
    /// returns them as text: the term this module does not understand.
    fn skip_term(&mut self, start: usize) -> Cond {
        let mut depth = 0i32;
        while let Some(t) = self.peek() {
            match t {
                Tok::Punct("(") => depth += 1,
                Tok::Punct(")") if depth == 0 => break,
                Tok::Punct(")") => depth -= 1,
                Tok::Punct("&&" | "||") if depth == 0 => break,
                _ => {}
            }
            self.pos += 1;
        }
        let text: Vec<String> = self.toks[start..self.pos].iter().map(tok_text).collect();
        Cond::Unsupported(text.join(" "))
    }

    fn atom(&mut self) -> Cond {
        let start = self.pos;
        let Some(Tok::Ident(name)) = self.peek().cloned() else {
            return self.skip_term(start);
        };
        self.pos += 1;
        let (side, member) = match name.split_once('.') {
            Some(("A", m)) => (Some(Side::A), m.to_owned()),
            Some(("B", m)) => (Some(Side::B), m.to_owned()),
            _ => (None, name.clone()),
        };
        // A.NetClass == 'x' / != 'x' / A.Layer == 'x'
        if matches!(self.peek(), Some(Tok::Punct("==" | "!="))) {
            let negate = self.peek() == Some(&Tok::Punct("!="));
            self.pos += 1;
            let term = match self.peek().cloned() {
                Some(Tok::Str(value)) => {
                    self.pos += 1;
                    match (side, member.as_str()) {
                        (Some(side), "NetClass") => Some(Cond::NetClass {
                            side,
                            class: value,
                            exact: true,
                        }),
                        (Some(_), "Layer") => Some(Cond::Layer(value)),
                        (Some(side), "Type") => Some(Cond::Type {
                            side,
                            value: value.to_ascii_lowercase(),
                        }),
                        (Some(side), "Pad_Type") => Some(Cond::PadType {
                            side,
                            value: value.to_ascii_lowercase(),
                        }),
                        _ => None,
                    }
                }
                // `B.Type == A.Type`, `A.Net != B.Net`
                Some(Tok::Ident(other)) => {
                    self.pos += 1;
                    let pair = (name.as_str(), other.as_str());
                    match pair {
                        ("A.Type", "B.Type") | ("B.Type", "A.Type") => Some(Cond::SameType),
                        ("A.Net", "B.Net") | ("B.Net", "A.Net") => Some(Cond::SameNet),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(t) = term
                && self.at_term_end()
            {
                return if negate { Cond::Not(Box::new(t)) } else { t };
            }
            return self.skip_term(start);
        }
        // A.isPlated()
        if let (Some(side), "isPlated") = (side, member.as_str())
            && self.peek() == Some(&Tok::Punct("("))
            && self.toks.get(self.pos + 1) == Some(&Tok::Punct(")"))
        {
            self.pos += 2;
            if self.at_term_end() {
                return Cond::Plated(side);
            }
            self.pos -= 2;
        }
        // A.hasNetclass('x'), onLayer('x')
        if self.peek() == Some(&Tok::Punct("(")) {
            let call = (side, member.as_str());
            let arg = match (self.toks.get(self.pos + 1), self.toks.get(self.pos + 2)) {
                (Some(Tok::Str(s)), Some(Tok::Punct(")"))) => Some(s.clone()),
                _ => None,
            };
            if let Some(value) = arg {
                let term = match call {
                    (Some(side), "hasNetclass") => Some(Cond::NetClass {
                        side,
                        class: value,
                        exact: false,
                    }),
                    (Some(side), "hasExactNetclass") => Some(Cond::NetClass {
                        side,
                        class: value,
                        exact: true,
                    }),
                    (None, "onLayer") => Some(Cond::Layer(value)),
                    _ => None,
                };
                if let Some(t) = term {
                    self.pos += 3;
                    if self.at_term_end() {
                        return t;
                    }
                    self.pos -= 3;
                }
            }
        }
        self.skip_term(start)
    }

    fn at_term_end(&self) -> bool {
        matches!(self.peek(), None | Some(Tok::Punct("&&" | "||" | ")")))
    }
}

fn tok_text(t: &Tok) -> String {
    match t {
        Tok::Ident(s) | Tok::Num(s) | Tok::Other(s) => s.clone(),
        Tok::Str(s) => format!("'{s}'"),
        Tok::Punct(p) => (*p).to_owned(),
    }
}

/// Parses a condition string into a [`Cond`].
pub fn parse_condition(text: &str) -> Cond {
    let mut p = CondParser {
        toks: tokenize(text),
        pos: 0,
    };
    let cond = p.or();
    if p.pos < p.toks.len() {
        // trailing tokens: an unbalanced `)` or an operator the grammar lacks
        let start = p.pos;
        let rest = p.skip_term(start);
        return Cond::And(vec![cond, rest]);
    }
    cond
}

impl Cond {
    /// The first unsupported term, left to right.
    pub fn first_unsupported(&self) -> Option<&str> {
        match self {
            Cond::Unsupported(t) => Some(t),
            Cond::Not(c) => c.first_unsupported(),
            Cond::And(v) | Cond::Or(v) => v.iter().find_map(Cond::first_unsupported),
            Cond::NetClass { .. }
            | Cond::Layer(_)
            | Cond::Type { .. }
            | Cond::SameType
            | Cond::Plated(_)
            | Cond::PadType { .. }
            | Cond::SameNet => None,
        }
    }

    /// True when a term asks about an item's type, plating or pad type
    /// (Q22.5): the rule then goes to [`item_types`].
    pub fn has_item_terms(&self) -> bool {
        match self {
            Cond::Type { .. } | Cond::SameType | Cond::Plated(_) | Cond::PadType { .. } => true,
            Cond::Not(c) => c.has_item_terms(),
            Cond::And(v) | Cond::Or(v) => v.iter().any(Cond::has_item_terms),
            _ => false,
        }
    }

    /// Splits the layer terms of the top-level conjunction off. A layer term
    /// anywhere else is returned as an error naming it.
    fn split_layers(&self) -> Result<(Option<Cond>, Vec<String>), String> {
        fn has_layer(c: &Cond) -> bool {
            match c {
                Cond::Layer(_) => true,
                Cond::Not(c) => has_layer(c),
                Cond::And(v) | Cond::Or(v) => v.iter().any(has_layer),
                _ => false,
            }
        }
        match self {
            Cond::Layer(l) => Ok((None, vec![l.clone()])),
            Cond::And(v) => {
                let mut layers = Vec::new();
                let mut rest = Vec::new();
                for c in v {
                    match c {
                        Cond::Layer(l) => layers.push(l.clone()),
                        c if has_layer(c) => {
                            return Err("layer term inside || or !".to_owned());
                        }
                        c => rest.push(c.clone()),
                    }
                }
                let rest = match rest.len() {
                    0 => None,
                    1 => rest.pop(),
                    _ => Some(Cond::And(rest)),
                };
                Ok((rest, layers))
            }
            c if has_layer(c) => Err("layer term inside || or !".to_owned()),
            c => Ok((Some(c.clone()), Vec::new())),
        }
    }

    /// Evaluates the class and item terms for an item pair. `b` is `None`
    /// for rules about a single item (a `B.` term is then an error).
    fn eval(&self, a: Probe<'_>, b: Option<Probe<'_>>) -> Result<bool, String> {
        let probe = |side: &Side| match side {
            Side::A => Ok(a),
            Side::B => b.ok_or_else(|| "a B. term in a one-item rule".to_owned()),
        };
        let item = |side: &Side| {
            probe(side)?
                .item
                .ok_or_else(|| "an item term in a class-only rule".to_owned())
        };
        Ok(match self {
            Cond::NetClass { side, class, exact } => {
                let name = probe(side)?.class;
                if *exact {
                    name == class
                } else {
                    name.split(',').any(|m| m == class)
                }
            }
            Cond::Type { side, value } => item(side)?.has_type(value),
            Cond::SameType => item(&Side::A)?.kicad_type() == item(&Side::B)?.kicad_type(),
            Cond::Plated(side) => item(side)?.is_plated(),
            Cond::PadType { side, value } => item(side)?.pad_type() == Some(value.as_str()),
            Cond::SameNet => false,
            Cond::Not(c) => !c.eval(a, b)?,
            Cond::And(v) => {
                for c in v {
                    if !c.eval(a, b)? {
                        return Ok(false);
                    }
                }
                true
            }
            Cond::Or(v) => {
                for c in v {
                    if c.eval(a, b)? {
                        return Ok(true);
                    }
                }
                false
            }
            Cond::Layer(_) | Cond::Unsupported(_) => {
                return Err("term outside the class language".to_owned());
            }
        })
    }
}

// ---- applying ----------------------------------------------------------------

/// The DSN name of KiCad's Default class.
const DSN_DEFAULT: &str = "kicad_default";

fn kicad_name(dsn: &str) -> &str {
    if dsn == DSN_DEFAULT { "Default" } else { dsn }
}

/// Reports every rule of `file` and, when `apply` is on, applies the
/// expressible ones to the tree. `layers` are the copper layers' names, top
/// to bottom. The tree's lengths are micrometres (the reader's DSN frame).
pub fn apply(
    file: &DruFile,
    apply: bool,
    network: &mut Network,
    structure: &mut Structure,
    layers: &[String],
) -> DruReport {
    apply_in_units(file, apply, network, structure, layers, 1000.0)
}

/// [`apply`] on a DSN tree whose lengths are `nm_per_unit` nanometres each.
pub fn apply_in_units(
    file: &DruFile,
    apply: bool,
    network: &mut Network,
    structure: &mut Structure,
    layers: &[String],
    nm_per_unit: f64,
) -> DruReport {
    let mut report = DruReport {
        present: true,
        ..DruReport::default()
    };
    let mut ctx = ApplyCtx {
        apply,
        layers,
        nm_per_unit,
        base: item_types::Base::of(network, structure),
        typed: item_types::Typed::default(),
        warnings: Vec::new(),
        hole: None,
    };
    for rule in &file.rules {
        let mut rr = RuleReport {
            name: rule.name.clone(),
            line: rule.line,
            condition: rule.condition_text.clone(),
            layers: rule.layers.clone(),
            constraints: Vec::new(),
        };
        for (kind, bounds) in &rule.constraints {
            let status = rule_status(rule, kind, bounds, &mut ctx, network, structure);
            rr.constraints.push(ConstraintReport {
                kind: kind.clone(),
                bounds: *bounds,
                status,
            });
        }
        report.rules.push(rr);
    }
    if apply {
        ctx.typed.emit(&ctx.base, network, structure);
    }
    report.warnings = ctx.warnings;
    report.hole_clearance = ctx.hole;
    report
}

/// Q22.5: applies the rules of a `.kicad_dru` (its text) to a parsed DSN, as
/// `--kicad-dru` does for a DSN input. The rules are applied
/// ([`apply`] with `apply` on); the copper layers are the structure's
/// signal and power layers, and lengths are in the unit of the DSN's
/// `resolution` (mil without one, as the DSN reader).
pub fn apply_to_pcb(
    pcb: &mut freerouting_dsn::ast::Pcb,
    text: &str,
) -> Result<DruReport, crate::KicadError> {
    use freerouting_dsn::ast::Unit;
    let file = DruFile::parse(text)?;
    let nm_per_unit = match pcb.resolution.map(|r| r.unit) {
        Some(Unit::Um) => 1e3,
        Some(Unit::Mm) => 1e6,
        Some(Unit::Inch) => 25.4e6,
        Some(Unit::Mil) | None => 25.4e3,
    };
    let structure = pcb.structure.get_or_insert_with(Structure::default);
    let layers: Vec<String> = structure.layers.iter().map(|l| l.name.clone()).collect();
    let network = pcb.network.get_or_insert_with(Network::default);
    Ok(apply_in_units(
        &file,
        true,
        network,
        structure,
        &layers,
        nm_per_unit,
    ))
}

/// The state of one [`apply_in_units`] call.
struct ApplyCtx<'a> {
    apply: bool,
    layers: &'a [String],
    nm_per_unit: f64,
    /// The DSN's clearances before any rule.
    base: item_types::Base,
    /// The item-type clearances, emitted after the last rule.
    typed: item_types::Typed,
    warnings: Vec<String>,
    hole: Option<HoleClearance>,
}

/// One item of a pair, as a condition sees it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Probe<'a> {
    /// The KiCad name of the item's net class.
    pub class: &'a str,
    /// `None` for the class-only rules of K07.5.
    pub item: Option<item_types::ItemType>,
}

impl<'a> Probe<'a> {
    fn class(class: &'a str) -> Probe<'a> {
        Probe { class, item: None }
    }
}

fn rule_status(
    rule: &DruRule,
    kind: &str,
    bounds: &Bounds,
    ctx: &mut ApplyCtx<'_>,
    network: &mut Network,
    structure: &mut Structure,
) -> Status {
    let unsupported = |why: String| Status::Unsupported(why);
    let Some(cond) = &rule.condition else {
        return unsupported("rule without a condition".to_owned());
    };
    if let Some(term) = cond.first_unsupported() {
        return unsupported(format!("condition term '{term}'"));
    }
    if !matches!(kind, "clearance" | "track_width" | "hole_clearance") {
        return unsupported(format!("constraint '{kind}' has no DSN form"));
    }
    if let Some(sev) = &rule.severity
        && sev == "ignore"
    {
        return Status::Ignored("severity ignore".to_owned());
    }
    // layer selection: the (layer ...) clause plus the top-level layer terms
    let (class_cond, cond_layers) = match cond.split_layers() {
        Ok(x) => x,
        Err(e) => return unsupported(e),
    };
    let mut wanted: Vec<String> = rule.layers.clone();
    wanted.extend(cond_layers);
    let layer_names = match resolve_layers(&wanted, ctx.layers) {
        Ok(l) => l,
        Err(e) => return unsupported(e),
    };
    if !wanted.is_empty() && layer_names.is_empty() {
        return Status::Ignored("its layers are not copper layers of the board".to_owned());
    }
    let classes: Vec<String> = network.classes.iter().map(|c| c.name.clone()).collect();
    let typed = class_cond.as_ref().is_some_and(Cond::has_item_terms);
    match kind {
        "track_width" => track_width(
            bounds,
            class_cond.as_ref(),
            &layer_names,
            &classes,
            ctx,
            network,
            structure,
        ),
        "hole_clearance" => {
            let Some(min) = bounds.min else {
                return unsupported("hole_clearance without a min value".to_owned());
            };
            item_types::hole_clearance(rule, min, class_cond.as_ref(), &classes, ctx)
        }
        _ if typed => {
            let Some(min) = bounds.min else {
                return unsupported("clearance without a min value".to_owned());
            };
            let all_layers = layer_names.is_empty() || layer_names.len() == ctx.layers.len();
            item_types::clearance(rule, min, class_cond.as_ref(), all_layers, &classes, ctx)
        }
        _ => class_clearance(
            bounds,
            class_cond.as_ref(),
            layer_names,
            &classes,
            ctx,
            network,
        ),
    }
}

/// A `track_width` constraint: the `opt` width of the classes it selects.
fn track_width(
    bounds: &Bounds,
    class_cond: Option<&Cond>,
    layer_names: &[String],
    classes: &[String],
    ctx: &ApplyCtx<'_>,
    network: &mut Network,
    structure: &mut Structure,
) -> Status {
    let Some(width) = bounds.opt else {
        return Status::Unsupported("track_width without an opt value".to_owned());
    };
    if !layer_names.is_empty() {
        return Status::Unsupported("track_width on selected layers has no DSN form".to_owned());
    }
    let mut targets = Vec::new();
    for c in classes {
        // a width is a track's: the rule is evaluated for a track
        let probe = Probe {
            class: kicad_name(c),
            item: Some(item_types::ItemType::Track),
        };
        match class_cond.map_or(Ok(true), |cc| cc.eval(probe, None)) {
            Ok(true) => targets.push(c.clone()),
            Ok(false) => {}
            Err(e) => return Status::Unsupported(e),
        }
    }
    if !ctx.apply {
        return Status::Ignored("apply_design_rules is off".to_owned());
    }
    let width = width as f64 / ctx.nm_per_unit;
    for c in network
        .classes
        .iter_mut()
        .filter(|c| targets.contains(&c.name))
    {
        set_width(&mut c.rules, width);
    }
    if targets.iter().any(|t| t == DSN_DEFAULT) {
        set_width(&mut structure.rules, width);
    }
    Status::Applied {
        effects: targets.len(),
    }
}

/// A `clearance` constraint whose condition names only net classes (K07.5):
/// a `class_class` rule for every class pair it selects.
fn class_clearance(
    bounds: &Bounds,
    class_cond: Option<&Cond>,
    layer_names: Vec<String>,
    classes: &[String],
    ctx: &mut ApplyCtx<'_>,
    network: &mut Network,
) -> Status {
    let Some(min) = bounds.min else {
        return Status::Unsupported("clearance without a min value".to_owned());
    };
    let mut pairs: Vec<(String, String)> = Vec::new();
    for (i, a) in classes.iter().enumerate() {
        for b in &classes[i..] {
            let eval = |x: &str, y: &str| match class_cond {
                Some(cc) => cc.eval(
                    Probe::class(kicad_name(x)),
                    Some(Probe::class(kicad_name(y))),
                ),
                None => Ok(true),
            };
            match (eval(a, b), eval(b, a)) {
                (Ok(f), Ok(r)) => {
                    if f || r {
                        pairs.push((a.clone(), b.clone()));
                    }
                }
                (Err(e), _) | (_, Err(e)) => return Status::Unsupported(e),
            }
        }
    }
    if !ctx.apply {
        return Status::Ignored("apply_design_rules is off".to_owned());
    }
    let value = min as f64 / ctx.nm_per_unit;
    let n = pairs.len();
    for (a, b) in pairs {
        if layer_names.is_empty() {
            ctx.typed.class_pair(&a, &b, value);
        }
        let clearance = Rule::Clearance {
            value,
            types: Vec::new(),
        };
        network.class_classes.push(if layer_names.is_empty() {
            ClassClass {
                classes: vec![a, b],
                rules: vec![clearance],
                layer_rules: Vec::new(),
            }
        } else {
            ClassClass {
                classes: vec![a, b],
                rules: Vec::new(),
                layer_rules: vec![LayerRule {
                    layers: layer_names.clone(),
                    rules: vec![clearance],
                }],
            }
        });
    }
    Status::Applied { effects: n }
}

fn set_width(rules: &mut Vec<Rule>, width_um: f64) {
    match rules.iter_mut().find(|r| matches!(r, Rule::Width(_))) {
        Some(r) => *r = Rule::Width(width_um),
        None => rules.insert(0, Rule::Width(width_um)),
    }
}

/// KiCad's `outer` and `inner` keywords and layer names, as DSN layer names.
fn resolve_layers(wanted: &[String], layers: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for w in wanted {
        let names: Vec<&String> = match w.as_str() {
            "outer" => layers
                .first()
                .into_iter()
                .chain(layers.last().filter(|_| layers.len() > 1))
                .collect(),
            "inner" => layers
                .iter()
                .skip(1)
                .take(layers.len().saturating_sub(2))
                .collect(),
            name => match layers
                .iter()
                .find(|l| l.as_str() == name)
                .or_else(|| kicad_layer(name, layers))
            {
                Some(l) => vec![l],
                None => return Err(format!("layer '{name}' is not a copper layer of the board")),
            },
        };
        for n in names {
            if !out.contains(n) {
                out.push(n.clone());
            }
        }
    }
    Ok(out)
}

/// A KiCad copper layer name on a DSN whose layers have other names (Q22.5):
/// `F.Cu` is the first layer, `B.Cu` the last, `In<n>.Cu` the n-th.
fn kicad_layer<'a>(name: &str, layers: &'a [String]) -> Option<&'a String> {
    if layers.len() < 2 {
        return None;
    }
    match name {
        "F.Cu" => layers.first(),
        "B.Cu" => layers.last(),
        _ => {
            let n: usize = name.strip_prefix("In")?.strip_suffix(".Cu")?.parse().ok()?;
            (n >= 1 && n + 1 < layers.len()).then(|| &layers[n])
        }
    }
}
