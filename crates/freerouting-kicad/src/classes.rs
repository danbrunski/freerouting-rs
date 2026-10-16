//! Net classes: which class each net has, and what the class says (K05.3).
//!
//! `NET_SETTINGS::GetEffectiveNetClass` and `makeEffectiveNetclass`
//! (`net_settings.cpp`, tag `10.0.0`, GPL-2.0-or-later):
//!
//! - the classes that apply to a net are the explicit assignment of its name
//!   plus every pattern that matches it (the empty net is always Default);
//! - the applicable classes are sorted by priority (a lower number wins),
//!   then by name; each value comes from the first of them that has it, and
//!   anything no class has comes from the Default class;
//! - one class that has every value is the net's class as it is. Several
//!   classes, or a class that needed the Default for a missing value, make an
//!   **aggregate class** named by its members joined with `,`
//!   (`NETCLASS::GetName`, `Power,Default`); that name is what the DSN export
//!   writes.
//!
//! A pattern matches the whole net name, either as a regular expression or
//! as a wildcard (`*` any text, `?` any character): `EDA_COMBINED_MATCHER`
//! with `CTX_NETCLASS`, which tries `EDA_PATTERN_MATCH_REGEX_ANCHORED` and
//! `EDA_PATTERN_MATCH_WILDCARD_ANCHORED`. The regular expression dialect is
//! the `regex` crate's, not wxWidgets' ARE; the two agree on the usual
//! patterns (literals, `.`, `*`, `+`, `[..]`, `|`, groups).

use regex::Regex;

use crate::project::{ClassDef, Project};

/// The values a class gives a net.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassValues {
    pub clearance: i32,
    pub track_width: i32,
    pub via_diameter: i32,
    pub via_drill: i32,
}

/// A net's effective class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveClass {
    /// `Default`, a class name, or an aggregate such as `Power,Default`.
    pub name: String,
    pub values: ClassValues,
}

struct Matcher {
    wildcard: String,
    regex: Option<Regex>,
}

impl Matcher {
    fn new(pattern: &str) -> Matcher {
        // EDA_PATTERN_MATCH_REGEX_ANCHORED: add ^ and $ when missing.
        let mut r = String::new();
        if !pattern.starts_with('^') {
            r.push('^');
        }
        r.push_str(pattern);
        if !pattern.ends_with('$') {
            r.push('$');
        }
        // wx's regular expressions refuse a quantifier with nothing to repeat, also
        // behind the added `^`; the `regex` crate accepts `^*` and would then match
        // anywhere in the name (`*VMEPX.*` of vme-wren, K07.6).
        let dangling = pattern.starts_with(['*', '+', '?']);
        Matcher {
            wildcard: pattern.to_owned(),
            regex: if dangling { None } else { Regex::new(&r).ok() },
        }
    }

    fn matches(&self, name: &str) -> bool {
        self.regex.as_ref().is_some_and(|r| r.is_match(name)) || glob(&self.wildcard, name)
    }
}

/// `EDA_PATTERN_MATCH_WILDCARD_ANCHORED`: `?` is any one character, `*` any
/// text, everything else is literal.
fn glob(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (usize::MAX, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || (p[pi] != '*' && p[pi] == t[ti])) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            mark = ti;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// All classes of a project and the rules that pick one for a net.
pub struct ClassTable {
    default: ClassDef,
    classes: Vec<ClassDef>,
    patterns: Vec<(Matcher, String)>,
    assignments: Vec<(String, Vec<String>)>,
}

impl ClassTable {
    pub fn new(project: &Project) -> ClassTable {
        ClassTable {
            default: project.default.clone(),
            classes: project.classes.clone(),
            patterns: project
                .patterns
                .iter()
                .map(|(p, c)| (Matcher::new(p), c.clone()))
                .collect(),
            assignments: project.assignments.clone(),
        }
    }

    pub fn default_class(&self) -> &ClassDef {
        &self.default
    }

    /// The non-default classes in project order.
    pub fn classes(&self) -> &[ClassDef] {
        &self.classes
    }

    fn explicit(&self, name: &str) -> ClassDef {
        if name == "Default" {
            return self.default.clone();
        }
        self.classes
            .iter()
            .find(|c| c.name == name)
            .cloned()
            .unwrap_or_else(|| ClassDef::implicit(name))
    }

    fn values_of(&self, c: &ClassDef) -> ClassValues {
        ClassValues {
            clearance: c.clearance.or(self.default.clearance).unwrap_or(200_000),
            track_width: c
                .track_width
                .or(self.default.track_width)
                .unwrap_or(200_000),
            via_diameter: c
                .via_diameter
                .or(self.default.via_diameter)
                .unwrap_or(600_000),
            via_drill: c.via_drill.or(self.default.via_drill).unwrap_or(300_000),
        }
    }

    /// The values of a class by name, filled from the Default.
    pub fn values_of_name(&self, name: &str) -> ClassValues {
        let c = self.explicit(name);
        self.values_of(&c)
    }

    /// `GetEffectiveNetClass( net )`.
    pub fn effective(&self, net: &str) -> EffectiveClass {
        let default_values = self.values_of(&self.default.clone());
        let default_result = || EffectiveClass {
            name: "Default".to_owned(),
            values: default_values,
        };
        if net.is_empty() {
            return default_result();
        }
        let mut names: Vec<String> = Vec::new();
        if let Some((_, list)) = self.assignments.iter().find(|(n, _)| n == net) {
            for c in list {
                if !names.contains(c) {
                    names.push(c.clone());
                }
            }
        }
        for (m, class) in &self.patterns {
            if m.matches(net) && !names.contains(class) {
                names.push(class.clone());
            }
        }
        if names.is_empty() {
            return default_result();
        }
        let mut members: Vec<ClassDef> = names.iter().map(|n| self.explicit(n)).collect();
        // priority first, then name (wxString::Cmp: code point order)
        members.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.name.cmp(&b.name))
        });
        let mut v = ClassValues {
            clearance: 0,
            track_width: 0,
            via_diameter: 0,
            via_drill: 0,
        };
        let mut have = Have::default();
        for c in members.iter().rev() {
            if let Some(x) = c.clearance {
                v.clearance = x;
                have.clearance = true;
            }
            if let Some(x) = c.track_width {
                v.track_width = x;
                have.track_width = true;
            }
            if let Some(x) = c.via_diameter {
                v.via_diameter = x;
                have.via_diameter = true;
            }
            if let Some(x) = c.via_drill {
                v.via_drill = x;
                have.via_drill = true;
            }
            have.uvia_diameter |= c.uvia_diameter.is_some();
            have.uvia_drill |= c.uvia_drill.is_some();
            have.dp_width |= c.diff_pair_width.is_some();
            have.dp_gap |= c.diff_pair_gap.is_some();
            have.wire |= c.has_wire_width;
            have.bus |= c.has_bus_width;
        }
        // addMissingDefaults
        let d = &self.default;
        let mut added_default = !have.all();
        if !have.clearance {
            v.clearance = d.clearance.unwrap_or(200_000);
        }
        if !have.track_width {
            v.track_width = d.track_width.unwrap_or(200_000);
        }
        if !have.via_diameter {
            v.via_diameter = d.via_diameter.unwrap_or(600_000);
        }
        if !have.via_drill {
            v.via_drill = d.via_drill.unwrap_or(300_000);
        }
        if members.len() == 1 && !added_default {
            return EffectiveClass {
                name: members[0].name.clone(),
                values: v,
            };
        }
        let mut name: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
        if added_default {
            name.push("Default");
        }
        if !added_default && members.len() == 1 {
            added_default = false;
        }
        let _ = added_default;
        EffectiveClass {
            name: name.join(","),
            values: v,
        }
    }
}

#[derive(Default)]
struct Have {
    clearance: bool,
    track_width: bool,
    via_diameter: bool,
    via_drill: bool,
    uvia_diameter: bool,
    uvia_drill: bool,
    dp_width: bool,
    dp_gap: bool,
    wire: bool,
    bus: bool,
}

impl Have {
    fn all(&self) -> bool {
        self.clearance
            && self.track_width
            && self.via_diameter
            && self.via_drill
            && self.uvia_diameter
            && self.uvia_drill
            && self.dp_width
            && self.dp_gap
            && self.wire
            && self.bus
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// K07.6: `*VMEPX.*` is a wildcard (`*` any text, `.` a dot). The `regex` crate
    /// takes `^*VMEPX.*$` as a regular expression without a start anchor, which
    /// wx refuses, so `/bus/VMEPX_IO0` wrongly got the class in vme-wren.
    #[test]
    fn leading_star_pattern_is_a_wildcard_only() {
        let project = Project::parse(
            r#"{"net_settings": {"classes": [
                {"name": "Default", "clearance": 0.2, "track_width": 0.2},
                {"name": "VMEPX", "clearance": 0.1, "track_width": 0.1, "priority": 0,
                 "via_diameter": 0.6, "via_drill": 0.3, "microvia_diameter": 0.3,
                 "microvia_drill": 0.1, "diff_pair_width": 0.2, "diff_pair_gap": 0.2,
                 "wire_width": 6, "bus_width": 12}],
              "netclass_patterns": [{"netclass": "VMEPX", "pattern": "*VMEPX.*"}]}}"#,
        )
        .unwrap();
        let table = ClassTable::new(&project);
        assert_eq!(table.effective("/bus/VMEPX.IO0").name, "VMEPX");
        assert_eq!(table.effective("/bus/VMEPX_USERIO_A.IO0").name, "Default");
        assert_eq!(table.effective("VMEPX.A").name, "VMEPX");
    }
}
