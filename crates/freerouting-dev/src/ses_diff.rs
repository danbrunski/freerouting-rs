//! `freerouting-dev ses-diff`: compares two sessions for the same board.
//!
//! Each session is compared structurally ([`SessionGeometry`]) and by the
//! statistics of the board after importing it ([`SessionStatistics`]).
//! Two sessions are equivalent when neither differs.
//!
//! **The import replaces the input's wiring.** A session repeats every trace
//! and via of the board it was written from except the system-fixed ones
//! (the writer, like Java's `SesWriter`), so it repeats the input's own
//! unfixed and user-fixed wiring too. Importing it into a plain copy of the
//! input board put a second copy of that wiring on the board, which inflated
//! the vias and the length, and left the input's unfixed traces in place
//! where the router had moved or ripped them. [`import_routed_session`]
//! first removes the wiring the session repeats, so the board is the routed
//! board the session was written from; every measure here imports with it.

use freerouting::board::BasicBoard;
use freerouting::board::items::{Item, ItemId};
use freerouting::board::structure::{FixedState, Unit};
use freerouting::drc::DesignRulesChecker;
use freerouting::drc::json::JsonValue;
use freerouting::dsn::Session;
use freerouting::io::SesImportSummary;
use freerouting::io::specctra::import_session;
use freerouting::scoring::BoardStatistics;

use crate::session_geometry::{Display, NetDifference, SessionGeometry, net_differences};

/// The statistics compared after importing a session (decision D4's
/// aggregate fields plus bends).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SessionStatistics {
    /// `connections.incomplete_count`.
    pub incomplete_connections: usize,
    /// `clearance_violations.total_count`, from the full clearance check.
    pub clearance_violations: usize,
    /// `vias.total_count`.
    pub vias: usize,
    /// `traces.total_length_mm`.
    pub trace_length_mm: f32,
    /// `bends.total_count`.
    pub bends: usize,
}

/// The names of the compared statistics, as in Java's `BoardStatistics`
/// JSON.
pub const STATISTIC_FIELDS: [&str; 5] = [
    "connections.incomplete_count",
    "clearance_violations.total_count",
    "vias.total_count",
    "traces.total_length_mm",
    "bends.total_count",
];

impl SessionStatistics {
    /// The compared fields of `stats`.
    pub fn from_board_statistics(stats: &BoardStatistics) -> Self {
        SessionStatistics {
            incomplete_connections: stats.connections.incomplete_count,
            clearance_violations: stats.clearance_violations.total_count,
            vias: stats.vias.total_count,
            trace_length_mm: stats.traces.total_length_mm,
            bends: stats.bends.total_count,
        }
    }

    /// The statistics of `board`, which runs the design rules checker.
    pub fn of(board: &BasicBoard) -> Self {
        Self::from_board_statistics(&BoardStatistics::with_unit(board, Unit::Mm))
    }

    /// The fields in [`STATISTIC_FIELDS`] order, as numbers.
    pub fn values(&self) -> [f64; 5] {
        [
            self.incomplete_connections as f64,
            self.clearance_violations as f64,
            self.vias as f64,
            f64::from(self.trace_length_mm),
            self.bends as f64,
        ]
    }

    /// The fields as a JSON object.
    pub fn to_json(&self) -> JsonValue {
        JsonValue::Object(vec![
            (
                STATISTIC_FIELDS[0].into(),
                JsonValue::int(self.incomplete_connections as i64),
            ),
            (
                STATISTIC_FIELDS[1].into(),
                JsonValue::int(self.clearance_violations as i64),
            ),
            (STATISTIC_FIELDS[2].into(), JsonValue::int(self.vias as i64)),
            (
                STATISTIC_FIELDS[3].into(),
                JsonValue::two_decimals(f64::from(self.trace_length_mm)),
            ),
            (
                STATISTIC_FIELDS[4].into(),
                JsonValue::int(self.bends as i64),
            ),
        ])
    }
}

/// One side of a comparison: the session's geometry and its import.
#[derive(Clone, Debug)]
pub struct ImportedSession {
    pub geometry: SessionGeometry,
    pub import: SesImportSummary,
    pub statistics: SessionStatistics,
    /// Incomplete connections per net name, for the nets that have any.
    incomplete_by_net: Vec<(String, usize)>,
}

impl ImportedSession {
    /// Imports `session` into a copy of `board` ([`import_routed_session`])
    /// and measures it.
    pub fn new(board: &BasicBoard, session: &Session) -> Self {
        let geometry = SessionGeometry::new(board, session);
        let mut copy = board.clone();
        let import = import_routed_session(&mut copy, session);
        let statistics = SessionStatistics::of(&copy);
        let mut checker = DesignRulesChecker::new(&copy);
        let mut incomplete_by_net = Vec::new();
        for name in geometry.nets.keys() {
            let Some(net) = copy.rules.nets.get_by_name(name, 1) else {
                continue;
            };
            let count = checker.incomplete_count_of(net.net_number);
            if count > 0 {
                incomplete_by_net.push((name.clone(), count));
            }
        }
        ImportedSession {
            geometry,
            import,
            statistics,
            incomplete_by_net,
        }
    }

    /// The incomplete connections of the net `name` after the import (only
    /// nets that appear in either session are counted).
    pub fn incomplete_of(&self, name: &str) -> usize {
        self.incomplete_by_net
            .iter()
            .find(|(n, _)| n == name)
            .map_or(0, |(_, c)| *c)
    }
}

/// The result of comparing two sessions.
#[derive(Clone, Debug)]
pub struct SesDiff {
    pub a: ImportedSession,
    pub b: ImportedSession,
    /// Every net whose wires or vias differ, in name order.
    pub nets: Vec<NetDifference>,
}

/// Whether a session written from a board repeats its `item`: the session
/// writer (`ses_writer::network_out`, Java `SesWriter.writeNetwork`) saves
/// every trace and via but the system-fixed ones.
pub fn session_repeats(item: &Item) -> bool {
    (item.is_trace() || item.is_via()) && item.fixed_state() != FixedState::SystemFixed
}

/// Removes from `board` the traces and vias that a session of it repeats
/// ([`session_repeats`]), user-fixed ones included. Returns how many it
/// removed; an item of a component is never removed (the board forbids it),
/// and none of a loaded board's traces or vias belongs to one.
pub fn remove_session_wiring(board: &mut BasicBoard) -> usize {
    let ids: Vec<ItemId> = board
        .items()
        .filter(|item| session_repeats(item))
        .map(Item::id)
        .collect();
    let mut removed = 0;
    for id in ids {
        // `remove_item` refuses user-fixed items.
        board.set_fixed_state(id, FixedState::Unfixed);
        if board.remove_item(id) {
            removed += 1;
        }
    }
    removed
}

/// Imports `session` into `board` in place of the wiring the session
/// repeats: the board becomes the routed board the session was written
/// from, its system-fixed wiring once and the session's wiring once.
/// `board` is the input the session was routed from.
pub fn import_routed_session(board: &mut BasicBoard, session: &Session) -> SesImportSummary {
    remove_session_wiring(board);
    import_session(board, session)
}

/// Parses a session file's text.
pub fn parse_session(text: &str) -> Result<Session, String> {
    Session::parse(text).map_err(|e| format!("not a Specctra session file: {e}"))
}

impl SesDiff {
    /// Compares `a` and `b`, each imported into its own copy of `board`.
    pub fn new(board: &BasicBoard, a: &Session, b: &Session) -> Self {
        let a = ImportedSession::new(board, a);
        let b = ImportedSession::new(board, b);
        let nets = net_differences(&a.geometry, &b.geometry);
        SesDiff { a, b, nets }
    }

    /// Whether the geometry and the statistics are the same.
    pub fn equivalent(&self) -> bool {
        self.nets.is_empty() && self.a.statistics == self.b.statistics
    }

    /// The statistics that differ: (field, a, b).
    pub fn statistics_deltas(&self) -> Vec<(&'static str, f64, f64)> {
        let a = self.a.statistics.values();
        let b = self.b.statistics.values();
        STATISTIC_FIELDS
            .iter()
            .zip(a.iter().zip(b.iter()))
            .filter(|(_, (x, y))| x != y)
            .map(|(field, (x, y))| (*field, *x, *y))
            .collect()
    }

    /// A report for people: the first `max_nets` differing nets with their
    /// differing wires and vias, then the statistics.
    pub fn to_text(&self, max_nets: usize) -> String {
        let mut out = String::new();
        let d = self.a.geometry.denominator;
        if self.equivalent() {
            out.push_str("equivalent\n");
        } else {
            out.push_str(&format!(
                "different: {} net(s) differ, {} statistic(s) differ\n",
                self.nets.len(),
                self.statistics_deltas().len()
            ));
        }
        for net in self.nets.iter().take(max_nets) {
            out.push_str(&format!(
                "net {}: incomplete {} -> {}\n",
                net.net,
                self.a.incomplete_of(&net.net),
                self.b.incomplete_of(&net.net)
            ));
            let lines = net
                .wires_only_in_a
                .iter()
                .map(|w| {
                    (
                        "a",
                        Display {
                            item: w,
                            denominator: d,
                        }
                        .to_string(),
                    )
                })
                .chain(net.wires_only_in_b.iter().map(|w| {
                    (
                        "b",
                        Display {
                            item: w,
                            denominator: d,
                        }
                        .to_string(),
                    )
                }))
                .chain(net.vias_only_in_a.iter().map(|v| {
                    (
                        "a",
                        Display {
                            item: v,
                            denominator: d,
                        }
                        .to_string(),
                    )
                }))
                .chain(net.vias_only_in_b.iter().map(|v| {
                    (
                        "b",
                        Display {
                            item: v,
                            denominator: d,
                        }
                        .to_string(),
                    )
                }));
            for (side, line) in lines {
                out.push_str(&format!("  only in {side}: {line}\n"));
            }
        }
        if self.nets.len() > max_nets {
            out.push_str(&format!(
                "... and {} more net(s)\n",
                self.nets.len() - max_nets
            ));
        }
        out.push_str("statistics (a -> b):\n");
        let a = self.a.statistics.values();
        let b = self.b.statistics.values();
        for (i, field) in STATISTIC_FIELDS.iter().enumerate() {
            let mark = if a[i] == b[i] { "" } else { "  (differs)" };
            out.push_str(&format!(
                "  {field}: {} -> {}{mark}\n",
                format_value(a[i]),
                format_value(b[i])
            ));
        }
        for (side, s) in [("a", &self.a), ("b", &self.b)] {
            out.push_str(&format!(
                "import {side}: {} wires, {} vias, {} errors\n",
                s.import.wires_imported, s.import.vias_imported, s.import.errors_encountered
            ));
        }
        out
    }

    /// The whole comparison as JSON (every differing net).
    pub fn to_json(&self) -> JsonValue {
        let d = self.a.geometry.denominator;
        let strings = |items: Vec<String>| {
            JsonValue::Array(items.into_iter().map(JsonValue::String).collect())
        };
        let nets = self
            .nets
            .iter()
            .map(|net| {
                let wires = |w: &[crate::session_geometry::WireKey]| {
                    strings(
                        w.iter()
                            .map(|w| {
                                Display {
                                    item: w,
                                    denominator: d,
                                }
                                .to_string()
                            })
                            .collect(),
                    )
                };
                let vias = |v: &[crate::session_geometry::ViaKey]| {
                    strings(
                        v.iter()
                            .map(|v| {
                                Display {
                                    item: v,
                                    denominator: d,
                                }
                                .to_string()
                            })
                            .collect(),
                    )
                };
                JsonValue::Object(vec![
                    ("net".into(), JsonValue::string(net.net.clone())),
                    (
                        "incomplete_a".into(),
                        JsonValue::int(self.a.incomplete_of(&net.net) as i64),
                    ),
                    (
                        "incomplete_b".into(),
                        JsonValue::int(self.b.incomplete_of(&net.net) as i64),
                    ),
                    ("wires_only_in_a".into(), wires(&net.wires_only_in_a)),
                    ("wires_only_in_b".into(), wires(&net.wires_only_in_b)),
                    ("vias_only_in_a".into(), vias(&net.vias_only_in_a)),
                    ("vias_only_in_b".into(), vias(&net.vias_only_in_b)),
                ])
            })
            .collect();
        let side = |s: &ImportedSession| {
            JsonValue::Object(vec![
                (
                    "wires".into(),
                    JsonValue::int(s.geometry.wire_count() as i64),
                ),
                ("vias".into(), JsonValue::int(s.geometry.via_count() as i64)),
                (
                    "wires_imported".into(),
                    JsonValue::int(s.import.wires_imported as i64),
                ),
                (
                    "vias_imported".into(),
                    JsonValue::int(s.import.vias_imported as i64),
                ),
                (
                    "import_errors".into(),
                    JsonValue::int(s.import.errors_encountered as i64),
                ),
                ("statistics".into(), s.statistics.to_json()),
            ])
        };
        JsonValue::Object(vec![
            ("equivalent".into(), JsonValue::Bool(self.equivalent())),
            ("a".into(), side(&self.a)),
            ("b".into(), side(&self.b)),
            ("nets".into(), JsonValue::Array(nets)),
        ])
    }
}

fn format_value(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v:.2}")
    }
}

impl SessionStatistics {
    /// The statistics of a copy of `board` with `session` imported in place
    /// of the wiring it repeats ([`import_routed_session`]).
    pub fn of_session(board: &BasicBoard, session: &Session) -> (Self, SesImportSummary) {
        let mut copy = board.clone();
        let import = import_routed_session(&mut copy, session);
        (Self::of(&copy), import)
    }
}
