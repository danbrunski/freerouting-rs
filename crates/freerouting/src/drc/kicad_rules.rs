//! K15.11: the KiCad rules a KiCad-counted DRC report checks with, which the
//! router's model does not carry.
//!
//! Not Java. A `.kicad_pcb` read (or a DSN routed with `--kicad-dru`) gives
//! three things the board's clearance matrix and items do not:
//!
//! - **the clearance of an unconditional custom rule.** KiCad's DRC takes a
//!   custom rule over the net-class clearance ("winner takes all",
//!   `DRC_ENGINE::EvalRules`), then raises it to Board Setup's minimum
//!   clearance. The router keeps the larger of the class and the rule (K15.3:
//!   the rule's `min` is a floor that only raises), so a pair between the rule
//!   and the class clearance is a violation for the router and not for KiCad.
//!   The report measures with the rule's value ([`clearance_um`]); the router's
//!   settings and routes are unchanged. Only a `.kicad_dru` whose clearance
//!   rules are all unconditional gives a value: with a conditional one too,
//!   which rule wins a pair depends on the file order, and the report keeps
//!   the router's matrix.
//! - **KiCad's hole clearance** ([`hole_clearance_um`]): Board Setup's
//!   `min_hole_clearance`, raised by the `.kicad_dru`'s `hole_clearance`
//!   floor (KiCad's default 0.25 mm without a `.kicad_pro`).
//! - **the pads' drilled holes** ([`holes`]): the DSN carries no pad drill,
//!   and a copper-less round hole only as a keepout grown by the hole
//!   clearance.
//!
//! - **the zones' fills and island removal** ([`zones`], K15.16): the DSN
//!   carries a zone as its outline; KiCad's connectivity joins items through
//!   the fill and its filler removes the islands no pad reaches. The fill is
//!   used only while it is current: [`fills_state`] is the board's
//!   [`state_hash`](BasicBoard::state_hash) when the rules were put on it, and
//!   a board that changed since (a route, an edit, a session) is counted with
//!   the router's fill model instead (`drc/kicad_connectivity.rs`).
//!
//! [`KicadReportInputs`] holds them as read (DSN coordinates); the board keeps
//! them in board coordinates ([`KicadReportRules`],
//! `BasicBoard::kicad_report_rules`), set by `run_drc` and `prepare_job`, and
//! kept by snapshots. A DSN board without them reports with the router's
//! clearances and no pad hole clearance.
//!
//! [`clearance_um`]: KicadReportRules::clearance_um
//! [`hole_clearance_um`]: KicadReportRules::hole_clearance_um
//! [`holes`]: KicadReportRules::holes
//! [`zones`]: KicadReportRules::zones
//! [`fills_state`]: KicadReportRules::fills_state

use freerouting_geometry::FloatPoint;
use freerouting_kicad::PadHole;
use freerouting_kicad::dru::DruReport;
pub use freerouting_kicad::zone_fills::IslandRemoval;
use freerouting_kicad::zone_fills::ZoneFill;

use crate::board::BasicBoard;
use crate::board::pad_holes::PadHoleOnBoard;
use crate::io::kicad::KicadInput;

/// A pad's drilled hole on the board: the segment `start..end` (one point
/// for a round hole) widened by `radius`, in board units.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardHole {
    /// The footprint's reference.
    pub reference: String,
    /// The pad number (empty for an NPTH pad).
    pub pad: String,
    pub start: FloatPoint,
    pub end: FloatPoint,
    pub radius: f64,
    /// False for an NPTH pad.
    pub plated: bool,
}

impl BoardHole {
    /// The hole's centre.
    pub fn centre(&self) -> FloatPoint {
        FloatPoint::new(
            (self.start.x + self.end.x) / 2.0,
            (self.start.y + self.end.y) / 2.0,
        )
    }
}

/// K15.16: a copper zone of a `.kicad_pcb` on one board layer, in board
/// coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardZone {
    /// The board's net of the zone; 0 if the board has no net of its name.
    pub net_number: i32,
    /// The board layer; -1 if the board has no layer of its name.
    pub layer: i32,
    /// `ZONE::IsFilled`: the file's fill is the zone's copper.
    pub filled: bool,
    /// What KiCad's filler does with an island no pad reaches.
    pub removal: IslandRemoval,
    /// The zone's outline.
    pub outline: Vec<FloatPoint>,
    /// The file's fill on this layer, one polygon per island.
    pub islands: Vec<Vec<FloatPoint>>,
    /// K15.23: the zone's minimum fill thickness in board units.
    pub min_thickness: f64,
}

/// The KiCad rules of a board's report (see the module docs).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KicadReportRules {
    /// The clearance of an unconditional custom rule, raised to Board Setup's
    /// minimum clearance, in µm; `None` keeps the router's matrix.
    pub clearance_um: Option<f64>,
    /// KiCad's copper-to-hole clearance in µm; `None` checks no pad holes.
    pub hole_clearance_um: Option<f64>,
    /// The pads' drilled holes, plated and not.
    pub holes: Vec<BoardHole>,
    /// K15.16: the board's copper zones per layer; empty for a DSN board.
    pub zones: Vec<BoardZone>,
    /// K15.16: the board's [`state_hash`](BasicBoard::state_hash) when the
    /// zones' fills were put on it; `None` without fills. The fills count only
    /// while the board's hash is this one.
    pub fills_state: Option<u128>,
}

impl KicadReportRules {
    /// True if the `.kicad_pcb`'s fills are `board`'s zone copper: every zone
    /// is filled and the board has not changed since the rules were put on
    /// it.
    pub fn fills_current(&self, board: &BasicBoard) -> bool {
        self.fills_state
            .is_some_and(|state| state == board.state_hash())
    }
}

/// [`KicadReportRules`] as read, in DSN coordinates (µm).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KicadReportInputs {
    pub clearance_um: Option<f64>,
    pub hole_clearance_um: Option<f64>,
    pub holes: Vec<PadHole>,
    /// K15.16: the board's copper zones with their fills.
    pub zones: Vec<ZoneFill>,
}

fn um(nm: i64) -> f64 {
    nm as f64 / 1000.0
}

/// The `min` of the `.kicad_dru`'s unconditional `clearance` rule in nm, if
/// every clearance rule of the file is unconditional.
pub fn unconditional_clearance_nm(report: &DruReport) -> Option<i64> {
    let conditional = report.rules.iter().any(|rule| {
        rule.condition.is_some() && rule.constraints.iter().any(|c| c.kind == "clearance")
    });
    if conditional {
        return None;
    }
    report.floors.clearance.as_ref().map(|floor| floor.nm)
}

/// The `.kicad_dru` rules of a read `.kicad_pcb` as KiCad applies them: the
/// read's own report when it applied them, else the file evaluated again on a
/// copy of the tree (`-drc` reads the board without its rules, as KiCad's
/// DSN export has none, but KiCad's DRC checks them).
fn kicad_rules(input: &KicadInput) -> DruReport {
    let applied = input.rules.rules.iter().any(|rule| {
        rule.constraints
            .iter()
            .any(|c| matches!(c.status, freerouting_kicad::dru::Status::Applied { .. }))
    });
    if applied || !input.rules.present {
        return input.rules.clone();
    }
    let text = input
        .rules_file
        .as_ref()
        .and_then(|path| std::fs::read(path).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    text.and_then(|text| freerouting_kicad::dru::apply_to_pcb(&mut input.pcb.clone(), &text).ok())
        .unwrap_or_else(|| input.rules.clone())
}

impl KicadReportInputs {
    /// The rules of a `.kicad_pcb` read with its `.kicad_pro` and `.kicad_dru`.
    pub fn of_kicad(input: &KicadInput) -> KicadReportInputs {
        let minimums = &input.extras.board_minimums;
        let mut inputs = KicadReportInputs {
            clearance_um: None,
            hole_clearance_um: match &input.project_file {
                Some(_) => minimums.hole_clearance.map(|nm| um(i64::from(nm))),
                None => freerouting_kicad::project::BoardMinimums::default()
                    .hole_clearance
                    .map(|nm| um(i64::from(nm))),
            },
            holes: input.extras.holes.clone(),
            zones: input.extras.zone_fills.clone(),
        };
        inputs.apply_rules(&kicad_rules(input), minimums.clearance.map(i64::from));
        inputs
    }

    /// Applies a `.kicad_dru`'s rules (`--kicad-dru` on a DSN input, or a
    /// `.kicad_pcb`'s own): the unconditional clearance, raised to
    /// `min_clearance_nm` (Board Setup's), and the hole clearance floor.
    pub fn apply_rules(&mut self, report: &DruReport, min_clearance_nm: Option<i64>) {
        if let Some(nm) = unconditional_clearance_nm(report) {
            self.clearance_um = Some(um(nm.max(min_clearance_nm.unwrap_or(0))));
        }
        if let Some(floor) = &report.hole_clearance {
            let rule = um(floor.nm);
            self.hole_clearance_um = Some(self.hole_clearance_um.map_or(rule, |v| v.max(rule)));
        }
    }

    /// The pads' holes in `board`'s coordinates.
    pub fn holes_on_board(&self, board: &BasicBoard) -> Vec<BoardHole> {
        let transform = &board.communication.coordinate_transform;
        self.holes
            .iter()
            .map(|hole| {
                let (w, h) = hole.drill;
                let half = (w.max(h) - w.min(h)) / 2.0;
                let (sin, cos) = hole.orient.to_radians().sin_cos();
                let (dx, dy) = if w >= h {
                    (half * cos, half * sin)
                } else {
                    (-half * sin, half * cos)
                };
                BoardHole {
                    reference: hole.reference.clone(),
                    pad: hole.pad.clone(),
                    start: transform.dsn_to_board_point(hole.at.x - dx, hole.at.y - dy),
                    end: transform.dsn_to_board_point(hole.at.x + dx, hole.at.y + dy),
                    radius: transform.dsn_to_board(w.min(h) / 2.0),
                    plated: hole.plated,
                }
            })
            .collect()
    }

    /// K15.19: puts the pads' holes on `board`'s pins
    /// ([`BasicBoard::set_pad_holes`]), as the CLI's `prepare_job` and the
    /// desktop app both do before routing.
    pub fn set_pad_holes(&self, board: &mut BasicBoard) -> usize {
        let holes: Vec<PadHoleOnBoard> = self
            .holes_on_board(board)
            .iter()
            .map(|hole| PadHoleOnBoard {
                start: hole.start,
                end: hole.end,
                radius: hole.radius,
            })
            .collect();
        board.set_pad_holes(&holes)
    }

    /// The rules in `board`'s coordinates.
    pub fn on_board(&self, board: &BasicBoard) -> KicadReportRules {
        let transform = &board.communication.coordinate_transform;
        let holes = self.holes_on_board(board);
        let point = |p: &freerouting_dsn::ast::Point| transform.dsn_to_board_point(p.x, p.y);
        let zones: Vec<BoardZone> = self
            .zones
            .iter()
            .map(|zone| BoardZone {
                net_number: board
                    .rules
                    .nets
                    .get_by_name(&zone.net, 1)
                    .map_or(0, |net| net.net_number),
                layer: board.layer_structure.get_no(&zone.layer),
                filled: zone.filled,
                removal: match zone.removal {
                    // µm² to board units²
                    IslandRemoval::BelowArea(um2) => {
                        IslandRemoval::BelowArea(um2 * transform.dsn_to_board(1.0).powi(2))
                    }
                    other => other,
                },
                outline: zone.outline.iter().map(point).collect(),
                islands: zone
                    .islands
                    .iter()
                    .map(|island| island.iter().map(point).collect())
                    .collect(),
                min_thickness: transform.dsn_to_board(zone.min_thickness),
            })
            .collect();
        let all_filled = !zones.is_empty() && zones.iter().all(|z| z.filled);
        KicadReportRules {
            clearance_um: self.clearance_um,
            hole_clearance_um: self.hole_clearance_um,
            holes,
            fills_state: all_filled.then(|| board.state_hash()),
            zones,
        }
    }
}
