//! The board's `.kicad_pro` project file (K05.3): net classes, the patterns
//! that assign nets to them, and the two design settings the export reads.
//!
//! Ported from `NET_SETTINGS` (`common/project/net_settings.cpp`, tag
//! `10.0.0`, GPL-2.0-or-later): the JSON layout, the schema migrations that
//! old projects go through (KiCad 6 projects have schema 2: a class's `nets`
//! list becomes pattern assignments, and priorities are numbered in file
//! order), and the units (`mmToIU` of the JSON number).
//!
//! `serde_json` runs with `float_roundtrip`, so a number parses to the same
//! `double` as `strtod` and `units::mm_to_nm` rounds it the way KiCad does.

use serde_json::Value;

use crate::error::{ErrorKind, KicadError};
use crate::units::mm_to_nm;

/// `std::numeric_limits<int>::max()`, the priority of the Default class.
pub const DEFAULT_PRIORITY: i32 = i32::MAX;

/// One net class as the project file defines it. A field is `None` when the
/// file leaves it out (KiCad's `HasX()` is false).
#[derive(Clone, Debug, PartialEq)]
pub struct ClassDef {
    pub name: String,
    pub priority: i32,
    pub clearance: Option<i32>,
    pub track_width: Option<i32>,
    pub via_diameter: Option<i32>,
    pub via_drill: Option<i32>,
    pub uvia_diameter: Option<i32>,
    pub uvia_drill: Option<i32>,
    pub diff_pair_width: Option<i32>,
    pub diff_pair_gap: Option<i32>,
    /// Schematic units (mils) in the file; only whether it is present matters.
    pub has_wire_width: bool,
    pub has_bus_width: bool,
}

impl ClassDef {
    /// `NETCLASS( name, true )`: KiCad's built-in defaults.
    pub fn builtin_default() -> ClassDef {
        ClassDef {
            name: "Default".to_owned(),
            priority: DEFAULT_PRIORITY,
            clearance: Some(mm_to_nm(0.2)),
            track_width: Some(mm_to_nm(0.2)),
            via_diameter: Some(mm_to_nm(0.6)),
            via_drill: Some(mm_to_nm(0.3)),
            uvia_diameter: Some(mm_to_nm(0.3)),
            uvia_drill: Some(mm_to_nm(0.1)),
            diff_pair_width: Some(mm_to_nm(0.2)),
            diff_pair_gap: Some(mm_to_nm(0.25)),
            has_wire_width: true,
            has_bus_width: true,
        }
    }

    /// An implicit class (named by an assignment, defined nowhere):
    /// `getOrAddImplicitNetcless`, priority `INT_MAX - 1`, no values.
    pub fn implicit(name: &str) -> ClassDef {
        ClassDef {
            name: name.to_owned(),
            priority: DEFAULT_PRIORITY - 1,
            clearance: None,
            track_width: None,
            via_diameter: None,
            via_drill: None,
            uvia_diameter: None,
            uvia_drill: None,
            diff_pair_width: None,
            diff_pair_gap: None,
            has_wire_width: false,
            has_bus_width: false,
        }
    }
}

/// What the reader takes from a `.kicad_pro`.
#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    /// The Default class (the file's, or the built-in one).
    pub default: ClassDef,
    /// The other classes, in file order.
    pub classes: Vec<ClassDef>,
    /// `(pattern, class)` in order.
    pub patterns: Vec<(String, String)>,
    /// Explicit net to class assignments, sorted by net name.
    pub assignments: Vec<(String, Vec<String>)>,
    /// `board.design_settings.rules.max_error`, nm (default 0.005 mm).
    pub max_error: i32,
    /// `board.design_settings.rules.min_hole_clearance`, nm (default 0.25 mm).
    pub hole_clearance: i32,
    /// Problems worth a warning.
    pub notes: Vec<String>,
}

impl Default for Project {
    /// No project file: KiCad's defaults.
    fn default() -> Self {
        Project {
            default: ClassDef::builtin_default(),
            classes: Vec::new(),
            patterns: Vec::new(),
            assignments: Vec::new(),
            max_error: mm_to_nm(0.005),
            hole_clearance: mm_to_nm(0.25),
            notes: Vec::new(),
        }
    }
}

fn mm(v: &Value, key: &str) -> Option<i32> {
    v.get(key)?.as_f64().map(mm_to_nm)
}

fn read_class(entry: &Value) -> Option<ClassDef> {
    let name = entry.get("name")?.as_str()?.to_owned();
    Some(ClassDef {
        priority: entry
            .get("priority")
            .and_then(Value::as_i64)
            .map(|p| p.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
            .unwrap_or(DEFAULT_PRIORITY - 1),
        clearance: mm(entry, "clearance"),
        track_width: mm(entry, "track_width"),
        via_diameter: mm(entry, "via_diameter"),
        via_drill: mm(entry, "via_drill"),
        uvia_diameter: mm(entry, "microvia_diameter"),
        uvia_drill: mm(entry, "microvia_drill"),
        diff_pair_width: mm(entry, "diff_pair_width"),
        diff_pair_gap: mm(entry, "diff_pair_gap"),
        has_wire_width: entry.get("wire_width").is_some_and(Value::is_number),
        has_bus_width: entry.get("bus_width").is_some_and(Value::is_number),
        name,
    })
}

impl Project {
    /// Parses the text of a `.kicad_pro`.
    pub fn parse(text: &str) -> Result<Project, KicadError> {
        let root: Value = serde_json::from_str(text).map_err(|e| {
            KicadError::new(
                ErrorKind::Semantic,
                format!("the .kicad_pro file is not valid JSON: {e}"),
            )
        })?;
        let mut p = Project::default();
        if let Some(rules) = root
            .get("board")
            .and_then(|b| b.get("design_settings"))
            .and_then(|d| d.get("rules"))
        {
            if let Some(v) = mm(rules, "max_error") {
                p.max_error = v.clamp(mm_to_nm(0.0001), mm_to_nm(1.0));
            }
            if let Some(v) = mm(rules, "min_hole_clearance") {
                p.hole_clearance = v;
            }
        }
        let Some(ns) = root.get("net_settings") else {
            return Ok(p);
        };
        let schema = ns
            .get("meta")
            .and_then(|m| m.get("version"))
            .and_then(Value::as_i64)
            .unwrap_or(5);
        let class_values: Vec<&Value> = ns
            .get("classes")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter(|e| e.is_object() && e.get("name").is_some())
                    .collect()
            })
            .unwrap_or_default();
        let mut migrated_priority = 0;
        for entry in &class_values {
            let Some(mut def) = read_class(entry) else {
                continue;
            };
            if schema < 4 {
                // migrateSchema3to4: Default gets INT_MAX, the others 0, 1, ...
                if def.name == "Default" {
                    def.priority = DEFAULT_PRIORITY;
                } else {
                    def.priority = migrated_priority;
                    migrated_priority += 1;
                }
            }
            if schema < 3
                && let Some(nets) = entry.get("nets").and_then(Value::as_array)
            {
                // migrateSchema2to3: a class's nets become pattern assignments
                for net in nets.iter().filter_map(Value::as_str) {
                    p.patterns.push((net.to_owned(), def.name.clone()));
                }
            }
            if def.name == "Default" {
                p.default = def;
            } else {
                p.classes.push(def);
            }
        }
        if let Some(patterns) = ns.get("netclass_patterns").and_then(Value::as_array) {
            for e in patterns {
                if let (Some(pat), Some(class)) = (
                    e.get("pattern").and_then(Value::as_str),
                    e.get("netclass").and_then(Value::as_str),
                ) {
                    if pat.contains("..") && (pat.contains('[') || pat.contains('{')) {
                        p.notes.push(format!(
                            "net class pattern `{pat}` looks like a bus; bus expansion is not implemented and the pattern is used as written"
                        ));
                    }
                    if !p.patterns.iter().any(|(a, b)| a == pat && b == class) {
                        p.patterns.push((pat.to_owned(), class.to_owned()));
                    }
                }
            }
        }
        if let Some(assign) = ns.get("netclass_assignments").and_then(Value::as_object) {
            let mut v: Vec<(String, Vec<String>)> = assign
                .iter()
                .map(|(net, classes)| {
                    let list: Vec<String> = match classes {
                        Value::String(s) if !s.is_empty() => vec![s.clone()],
                        Value::Array(a) => a
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect(),
                        _ => Vec::new(),
                    };
                    (net.clone(), list)
                })
                .collect();
            v.sort();
            p.assignments = v;
        }
        Ok(p)
    }
}
