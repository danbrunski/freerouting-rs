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
//!   `(width v)`; the Default class also sets the structure's width rule.
//!
//! Rules apply in file order and a later rule overrides an earlier one for
//! the same target (KiCad evaluates rules last to first and the first match
//! wins, so the last matching rule in the file wins). Everything else is
//! reported by name and kept out: a condition with a term this module cannot
//! evaluate (`insideCourtyard`, `intersectsArea`, `inDiffPair`, item types,
//! ...) makes the rule UNSUPPORTED and names the first such term, in both
//! modes; so does a constraint kind other than `clearance` and `track_width`
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
//! parentheses. `B.` terms refer to the other item. The class pairs a rule
//! applies to are found by evaluating the condition for every ordered pair of
//! the board's net classes; a pair counts when either order is true. The
//! Default class is `Default` in a condition and `kicad_default` in the DSN.
//!
//! Syntax follows KRT's `kicad_dru.py` (KiCadRoutingTools, MIT; ideas only,
//! no code taken): unsupported rules are kept and reported, never dropped
//! (KRT #770).

use freerouting_dsn::ast::{ClassClass, LayerRule, Network, Rule, Structure};

use crate::sexpr::Tree;

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
    /// Expressible, but [`crate::ReadOptions::apply_design_rules`] is off.
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

/// Every rule of the file, in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DruReport {
    /// Whether a `.kicad_dru` was read at all.
    pub present: bool,
    pub rules: Vec<RuleReport>,
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
            if let Some(Tok::Str(value)) = self.peek().cloned() {
                self.pos += 1;
                let term = match (side, member.as_str()) {
                    (Some(side), "NetClass") => Some(Cond::NetClass {
                        side,
                        class: value,
                        exact: true,
                    }),
                    (Some(_), "Layer") => Some(Cond::Layer(value)),
                    _ => None,
                };
                if let Some(t) = term
                    && self.at_term_end()
                {
                    return if negate { Cond::Not(Box::new(t)) } else { t };
                }
            }
            return self.skip_term(start);
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
            Cond::NetClass { .. } | Cond::Layer(_) => None,
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

    /// Evaluates the class terms for an item pair. `b` is `None` for rules
    /// about a single item (a `B.` term is then an error).
    fn eval(&self, a: &str, b: Option<&str>) -> Result<bool, String> {
        Ok(match self {
            Cond::NetClass { side, class, exact } => {
                let name = match side {
                    Side::A => a,
                    Side::B => b.ok_or_else(|| "B.NetClass in a one-item rule".to_owned())?,
                };
                if *exact {
                    name == class
                } else {
                    name.split(',').any(|m| m == class)
                }
            }
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
/// to bottom.
pub fn apply(
    file: &DruFile,
    apply: bool,
    network: &mut Network,
    structure: &mut Structure,
    layers: &[String],
) -> DruReport {
    let mut report = DruReport {
        present: true,
        rules: Vec::new(),
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
            let status = rule_status(rule, kind, bounds, apply, network, structure, layers);
            rr.constraints.push(ConstraintReport {
                kind: kind.clone(),
                bounds: *bounds,
                status,
            });
        }
        report.rules.push(rr);
    }
    report
}

fn rule_status(
    rule: &DruRule,
    kind: &str,
    bounds: &Bounds,
    apply: bool,
    network: &mut Network,
    structure: &mut Structure,
    layers: &[String],
) -> Status {
    let unsupported = |why: String| Status::Unsupported(why);
    let Some(cond) = &rule.condition else {
        return unsupported("rule without a condition".to_owned());
    };
    if let Some(term) = cond.first_unsupported() {
        return unsupported(format!("condition term '{term}'"));
    }
    if !matches!(kind, "clearance" | "track_width") {
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
    let layer_names = match resolve_layers(&wanted, layers) {
        Ok(l) => l,
        Err(e) => return unsupported(e),
    };
    let classes: Vec<String> = network.classes.iter().map(|c| c.name.clone()).collect();
    if kind == "track_width" {
        let Some(width) = bounds.opt else {
            return unsupported("track_width without an opt value".to_owned());
        };
        if !layer_names.is_empty() {
            return unsupported("track_width on selected layers has no DSN form".to_owned());
        }
        let mut targets = Vec::new();
        for c in &classes {
            match class_cond
                .as_ref()
                .map_or(Ok(true), |cc| cc.eval(kicad_name(c), None))
            {
                Ok(true) => targets.push(c.clone()),
                Ok(false) => {}
                Err(e) => return unsupported(e),
            }
        }
        if !apply {
            return Status::Ignored("apply_design_rules is off".to_owned());
        }
        let width_um = width as f64 / 1000.0;
        for c in network
            .classes
            .iter_mut()
            .filter(|c| targets.contains(&c.name))
        {
            set_width(&mut c.rules, width_um);
        }
        if targets.iter().any(|t| t == DSN_DEFAULT) {
            set_width(&mut structure.rules, width_um);
        }
        return Status::Applied {
            effects: targets.len(),
        };
    }
    // clearance
    let Some(min) = bounds.min else {
        return unsupported("clearance without a min value".to_owned());
    };
    let mut pairs: Vec<(String, String)> = Vec::new();
    for (i, a) in classes.iter().enumerate() {
        for b in &classes[i..] {
            let eval = |x: &str, y: &str| match &class_cond {
                Some(cc) => cc.eval(kicad_name(x), Some(kicad_name(y))),
                None => Ok(true),
            };
            match (eval(a, b), eval(b, a)) {
                (Ok(f), Ok(r)) => {
                    if f || r {
                        pairs.push((a.clone(), b.clone()));
                    }
                }
                (Err(e), _) | (_, Err(e)) => return unsupported(e),
            }
        }
    }
    if !apply {
        return Status::Ignored("apply_design_rules is off".to_owned());
    }
    let value = min as f64 / 1000.0;
    let n = pairs.len();
    for (a, b) in pairs {
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
            name => match layers.iter().find(|l| l.as_str() == name) {
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
