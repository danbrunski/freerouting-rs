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
    /// K15.42: the least clearance KiCad's filler keeps between the zone and
    /// another net's copper beyond the router's, in board units: the zone's
    /// own clearance, which KiCad maxes with the net classes' where no
    /// `.kicad_dru` rule matches (`DRC_ENGINE::EvalRules`); 0 when the
    /// board's `.kicad_dru` has a clearance rule (a matching rule replaces
    /// both, and the router does not evaluate its condition). The router's
    /// pour target and contacts take it (`quality::pour_fill::opened_fill_core`).
    pub fill_clearance: f64,
    /// K15.45: the zone's `(priority N)` and whether it is a teardrop
    /// (`freerouting_kicad::zone_fills::ZoneFill`).
    pub priority: u32,
    pub teardrop: bool,
}

/// K15.38: a rule area of a `.kicad_pcb` on one board layer, in board
/// coordinates: the box of its outline and whether it bans copper pour (the
/// DSN keepouts the reader writes for it do not say;
/// `freerouting_kicad::rule_areas::RuleAreaExtent`).
#[derive(Clone, Debug, PartialEq)]
pub struct BoardRuleArea {
    /// The board layer; -1 if the board has no layer of its name.
    pub layer: i32,
    /// `[x0, y0, x1, y1]`.
    pub bbox: [f64; 4],
    /// KiCad's filler cuts every zone on [`layer`](Self::layer) at the
    /// area's outline.
    pub bans_copper_pour: bool,
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
    /// K15.26: the board was read from a `.kicad_pcb`, so [`zones`](Self::zones)
    /// lists the file's zones. A DSN input has none; its signal-layer pours
    /// take [`dsn_pours`](Self::dsn_pours).
    pub kicad_pcb: bool,
    /// K15.38: the `.kicad_pcb`'s rule areas per layer; empty for a DSN board.
    pub rule_areas: Vec<BoardRuleArea>,
    /// K15.26: a DSN input's signal-layer pours as the stub cleanup opens
    /// them (`router.cleanup.pour_*`).
    pub dsn_pours: DsnPourRules,
    /// K15.40: the least edge clearance KiCad's filler keeps on some layer of
    /// a board whose `.kicad_dru` has `edge_clearance` rules, in board units
    /// (the smallest of the rules' and Board Setup's), 0 without such a
    /// rule: the stub cleanup's fill model keeps no more than this from the
    /// board edge, as KiCad's filler takes a rule below Board Setup's value
    /// too.
    pub edge_clearance_least: f64,
    /// K15.45: the `.kicad_pcb`'s pads with a clearance of their own or of
    /// their footprint, sorted by centre; empty for a DSN board.
    pub pad_overrides: Vec<PadOverride>,
}

/// K15.45: a pad's own clearance (`(clearance v)` of the pad, else of its
/// footprint; `PAD::GetLocalClearance`), in board units: KiCad's clearance
/// constraint takes it in place of the net classes' and the zone's, raised
/// to Board Setup's minimum clearance (`DRC_ENGINE::EvalRules`' local
/// overrides), which the stub cleanup's fill model keeps from the pad where
/// it is below the router's clearance (`drc::zone_islands`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadOverride {
    /// The pad's centre.
    pub at: FloatPoint,
    /// The clearance, board units.
    pub clearance: f64,
}

impl KicadReportRules {
    /// K15.45: the clearance of the pad override centred within `slack`
    /// board units of `at`, the least of several; `None` without one.
    pub fn pad_override_at(&self, at: FloatPoint, slack: f64) -> Option<f64> {
        let start = self
            .pad_overrides
            .partition_point(|o| o.at.x < at.x - slack);
        self.pad_overrides[start..]
            .iter()
            .take_while(|o| o.at.x <= at.x + slack)
            .filter(|o| (o.at.y - at.y).abs() <= slack)
            .map(|o| o.clearance)
            .reduce(f64::min)
    }
}

/// K15.26: what a DSN input does not carry about its signal-layer pours, in
/// board units: the zones' minimum thickness and clearance, 0 when unknown
/// (`router.cleanup.pour_min_thickness_um`, `pour_clearance_um`; the KiCad
/// plugin knows them). The stub cleanup's fill model of such a pour is
/// opened by the thickness and keeps the clearance from other nets' copper
/// (`quality::pour_fill`); K15.33: a net of `pour_min_thickness_nets` takes
/// its own thickness. K15.29: whether KiCad's filler
/// removes the pours' islands no pad reaches (`router.cleanup.pour_island_removal`),
/// and the nets whose zones keep them (`pour_islands_kept_nets`); the stub
/// cleanup then drops those islands (`quality::pour_islands`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DsnPourRules {
    pub min_thickness: f64,
    pub clearance: f64,
    /// K15.29: the islands no pad reaches are removed, as KiCad's
    /// `island_removal_mode` 0 does.
    pub island_removal: bool,
    /// K15.29: the board's nets whose pours keep every island.
    pub islands_kept_nets: std::collections::BTreeSet<i32>,
    /// K15.33: per net, the minimum thickness of its zones where it is not
    /// [`min_thickness`](Self::min_thickness)
    /// (`router.cleanup.pour_min_thickness_nets`).
    pub min_thickness_by_net: std::collections::BTreeMap<i32, f64>,
}

impl DsnPourRules {
    /// K15.33: the minimum thickness of net `net`'s pours, in board units.
    pub fn min_thickness_of(&self, net: i32) -> f64 {
        self.min_thickness_by_net
            .get(&net)
            .copied()
            .unwrap_or(self.min_thickness)
    }
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
    /// K15.26: read from a `.kicad_pcb` ([`KicadReportRules::kicad_pcb`]).
    pub kicad_pcb: bool,
    /// K15.38: the rule areas ([`KicadReportRules::rule_areas`]).
    pub rule_areas: Vec<freerouting_kicad::rule_areas::RuleAreaExtent>,
    /// K15.45: the pads' own clearances ([`KicadReportRules::pad_overrides`]):
    /// centre (DSN, µm) and clearance (µm, at least Board Setup's minimum).
    pub pad_overrides: Vec<(freerouting_dsn::ast::Point, f64)>,
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

/// K15.42: the `.kicad_dru` has a `clearance` constraint, with a condition
/// or without.
pub fn has_clearance_rule(report: &DruReport) -> bool {
    report
        .rules
        .iter()
        .any(|rule| rule.constraints.iter().any(|c| c.kind == "clearance"))
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
            kicad_pcb: true,
            rule_areas: input.extras.rule_areas.clone(),
            pad_overrides: input
                .extras
                .pad_clearances
                .iter()
                .map(|pad| {
                    let at =
                        freerouting_kicad::units::map_pt(i64::from(pad.at.0), i64::from(pad.at.1));
                    let floor = minimums.clearance.map_or(0, i64::from);
                    (at, um(i64::from(pad.local).max(floor)))
                })
                .collect(),
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
        // K15.42: a clearance rule wins over the zones' own clearance where
        // it matches, so their clearance is no longer known.
        if has_clearance_rule(report) {
            for zone in &mut self.zones {
                zone.clearance = 0.0;
            }
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

    /// K15.45: the pads' own clearances on `board`, sorted by centre.
    fn pad_overrides_on_board(&self, board: &BasicBoard) -> Vec<PadOverride> {
        let transform = &board.communication.coordinate_transform;
        let mut out: Vec<PadOverride> = self
            .pad_overrides
            .iter()
            .map(|(at, clearance)| PadOverride {
                at: transform.dsn_to_board_point(at.x, at.y),
                clearance: transform.dsn_to_board(*clearance),
            })
            .collect();
        out.sort_by(|a, b| a.at.x.total_cmp(&b.at.x).then(a.at.y.total_cmp(&b.at.y)));
        out
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
                fill_clearance: transform.dsn_to_board(zone.clearance.max(0.0)),
                priority: zone.priority,
                teardrop: zone.teardrop,
            })
            .collect();
        let all_filled = !zones.is_empty() && zones.iter().all(|z| z.filled);
        let rule_areas = self
            .rule_areas
            .iter()
            .flat_map(|area| {
                let [x0, y0, x1, y1] = area.bbox;
                let (a, b) = (
                    transform.dsn_to_board_point(x0, y0),
                    transform.dsn_to_board_point(x1, y1),
                );
                let bbox = [a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)];
                area.layers.iter().map(move |layer| BoardRuleArea {
                    layer: board.layer_structure.get_no(layer),
                    bbox,
                    bans_copper_pour: area.bans_copper_pour,
                })
            })
            .collect();
        KicadReportRules {
            clearance_um: self.clearance_um,
            hole_clearance_um: self.hole_clearance_um,
            holes,
            fills_state: all_filled.then(|| board.state_hash()),
            zones,
            kicad_pcb: self.kicad_pcb,
            rule_areas,
            dsn_pours: DsnPourRules::default(),
            edge_clearance_least: 0.0,
            pad_overrides: self.pad_overrides_on_board(board),
        }
    }
}
