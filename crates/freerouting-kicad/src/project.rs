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

/// Board Setup's minimum manufacturing rules
/// (`board.design_settings.rules`, Q18), in nm. A rule the file leaves out
/// (or no project file) is KiCad's default, as KiCad itself loads it (the
/// reader takes KiCad 6+ boards only, whose rules live in the project)
/// (`board_design_settings.h`: track 0.2 mm, via 0.5 mm, through drill
/// 0.3 mm, annular ring 0.1 mm, copper to edge 0.5 mm, copper to hole 0.25 mm, minimum
/// clearance 0, which is none, hole to hole 0.25 mm); `None` when the file
/// sets it to 0 (KiCad's "no minimum"). The router takes them as hard floors
/// (`router.board_minimums.*`, `router.copper_to_edge_clearance_um`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoardMinimums {
    /// `min_track_width` (`m_TrackMinWidth`).
    pub track_width: Option<i32>,
    /// `min_via_diameter` (`m_ViasMinSize`).
    pub via_diameter: Option<i32>,
    /// `min_through_hole_diameter` (`m_MinThroughDrill`).
    pub through_drill: Option<i32>,
    /// `min_via_annular_width` (`m_ViasMinAnnularWidth`).
    pub via_annular: Option<i32>,
    /// `min_copper_edge_clearance` (`m_CopperEdgeClearance`).
    pub copper_edge_clearance: Option<i32>,
    /// `min_hole_clearance` (`m_HoleClearance`): copper to a drilled hole.
    pub hole_clearance: Option<i32>,
    /// `min_clearance` (`m_MinClearance`): copper to copper of another net,
    /// under every net class (Q22.10). KiCad's default is 0, so a missing
    /// key is `None`.
    pub clearance: Option<i32>,
    /// `min_hole_to_hole` (`m_HoleToHoleMin`): drill hole to drill hole, of
    /// any net, edge to edge (Q22.11).
    pub hole_to_hole: Option<i32>,
}

impl Default for BoardMinimums {
    /// KiCad's defaults.
    fn default() -> Self {
        BoardMinimums {
            track_width: Some(mm_to_nm(0.2)),
            via_diameter: Some(mm_to_nm(0.5)),
            through_drill: Some(mm_to_nm(0.3)),
            via_annular: Some(mm_to_nm(0.1)),
            copper_edge_clearance: Some(mm_to_nm(0.5)),
            hole_clearance: Some(mm_to_nm(0.25)),
            clearance: None,
            hole_to_hole: Some(mm_to_nm(0.25)),
        }
    }
}

impl BoardMinimums {
    /// No minimum known.
    pub const NONE: BoardMinimums = BoardMinimums {
        track_width: None,
        via_diameter: None,
        through_drill: None,
        via_annular: None,
        copper_edge_clearance: None,
        hole_clearance: None,
        clearance: None,
        hole_to_hole: None,
    };

    /// The minimums of a `rules` object; a missing key keeps KiCad's default.
    pub fn of_rules(rules: &Value) -> BoardMinimums {
        let d = BoardMinimums::default();
        let rule = |key, default: Option<i32>| match mm(rules, key) {
            Some(v) => Some(v).filter(|v| *v > 0),
            None => default,
        };
        BoardMinimums {
            track_width: rule("min_track_width", d.track_width),
            via_diameter: rule("min_via_diameter", d.via_diameter),
            through_drill: rule("min_through_hole_diameter", d.through_drill),
            via_annular: rule("min_via_annular_width", d.via_annular),
            copper_edge_clearance: rule("min_copper_edge_clearance", d.copper_edge_clearance),
            hole_clearance: rule("min_hole_clearance", d.hole_clearance),
            clearance: rule("min_clearance", d.clearance),
            hole_to_hole: rule("min_hole_to_hole", d.hole_to_hole),
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
    /// Board Setup's minimum manufacturing rules (Q18); KiCad's defaults
    /// where the file leaves them out, unknown where it sets them to 0.
    pub minimums: BoardMinimums,
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
            minimums: BoardMinimums::default(),
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
            p.minimums = BoardMinimums::of_rules(rules);
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
