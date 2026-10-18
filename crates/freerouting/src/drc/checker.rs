//! The design rules checker.
//!
//! Port of `drc.DesignRulesChecker` and of the counts of
//! `drc.NetRoutingLedger`.

use std::collections::BTreeSet;

use crate::util::{HashMap, HashSet};

use freerouting_geometry::{FloatPoint, PolylineShapeOps};

use super::clearance::ClearanceViolation;
use super::incompletes::{AirLine, NetIncompletes, max_connections, net_item_lists};
use super::json::java_format_fixed;
use super::report::{DrcPosition, DrcReport, DrcViolation, DrcViolationItem, FREEROUTING_VERSION};
use super::summary::{CongestionZone, DiagnosticViolation, DrcSummary};
use super::unconnected::{UnconnectedItems, UnconnectedKind, all_unconnected_items};
use super::zone_islands::{ZoneIslandKind, ZoneIslandViolation, zone_island_violations};
use crate::board::BasicBoard;
use crate::board::items::{AreaKind, Item, ItemId, ItemKind};
use crate::board::structure::{ComponentId, Unit};
use crate::scoring::BoardStatistics;

/// Everything the checker finds on a board, as drawable data.
///
/// All coordinates are board coordinates.
#[derive(Clone, Debug, Default)]
pub struct DrcFindings {
    /// The clearance violations, each pair of items and layer once.
    pub clearance_violations: Vec<ClearanceViolation>,
    /// Split nets, dangling traces and dangling vias.
    pub unconnected_items: Vec<UnconnectedItems>,
    /// Isolated and dead copper islands of filled conduction areas.
    pub zone_islands: Vec<ZoneIslandViolation>,
    /// The incomplete connections of every net (index 0 is net 1).
    pub incompletes: Vec<NetIncompletes>,
}

impl DrcFindings {
    /// All airlines of all nets.
    pub fn airlines(&self) -> impl Iterator<Item = &AirLine> + '_ {
        self.incompletes.iter().flat_map(|n| n.airlines.iter())
    }
}

impl BasicBoard {
    /// Runs the design rules check: clearance violations, unconnected
    /// items, zone islands and the incomplete connections of all nets.
    ///
    /// The entry point for a viewer: call it after loading or changing
    /// the board and draw the result, all in board coordinates:
    ///
    /// - each [`AirLine`] of [`DrcFindings::airlines`] as a straight line
    ///   from `from_corner` to `to_corner` (it connects `from_item` and
    ///   `to_item` of net `net_number`; their layer ranges are
    ///   `from_layers` and `to_layers`),
    /// - each [`ClearanceViolation`] as its `shape` on its `layer`, between
    ///   `first_item` and `second_item`, labelled with `expected_clearance`
    ///   and `actual_clearance` (board units),
    /// - the items of each [`UnconnectedItems`] highlighted, and the
    ///   trapezoids of each [`ZoneIslandViolation::island`] filled on its
    ///   layer.
    ///
    /// Use a [`DesignRulesChecker`] to query parts of the check or to
    /// generate the JSON report.
    pub fn design_rules_check(&self) -> DrcFindings {
        DesignRulesChecker::new(self).check()
    }

    /// The incomplete connections of every net (index 0 is net 1): the
    /// airlines (ratsnest) a viewer draws, each a line between two items of
    /// a net that are not connected yet. See
    /// [`design_rules_check`](Self::design_rules_check).
    pub fn incompletes(&self) -> Vec<NetIncompletes> {
        DesignRulesChecker::new(self).net_incompletes().to_vec()
    }
}

/// The design rules checker of a board.
///
/// Port of `drc.DesignRulesChecker`. The board must not change while a
/// checker borrows it; the checker caches item contacts and the incomplete
/// connections.
///
/// A viewer calls [`check`](Self::check) once per board revision and
/// draws the returned [`DrcFindings`]: the airlines of
/// [`DrcFindings::incompletes`] as lines between their corners, the
/// [`ClearanceViolation::shape`]s on their layers, and the items of the
/// unconnected and zone island findings highlighted.
pub struct DesignRulesChecker<'a> {
    board: &'a BasicBoard,
    contacts: HashMap<ItemId, Vec<ItemId>>,
    net_incompletes: Option<Vec<NetIncompletes>>,
    max_connections: usize,
    check_zone_islands: bool,
}

impl<'a> DesignRulesChecker<'a> {
    pub fn new(board: &'a BasicBoard) -> Self {
        DesignRulesChecker {
            board,
            contacts: HashMap::default(),
            net_incompletes: None,
            max_connections: 0,
            check_zone_islands: true,
        }
    }

    /// The checked board.
    pub fn board(&self) -> &'a BasicBoard {
        self.board
    }

    /// Turns the copper island check of conduction areas on (the default)
    /// or off. The island check is the most expensive part of a check on
    /// boards with large filled planes; when it is off,
    /// [`zone_island_violations`](Self::zone_island_violations) finds
    /// nothing and the reports leave the islands out. Not in Java.
    pub fn set_check_zone_islands(&mut self, enabled: bool) {
        self.check_zone_islands = enabled;
    }

    /// Runs all checks: clearance, unconnected items, zone islands and the
    /// incomplete connections.
    pub fn check(&mut self) -> DrcFindings {
        DrcFindings {
            clearance_violations: self.all_clearance_violations(),
            unconnected_items: self.all_unconnected_items(),
            zone_islands: self.zone_island_violations(),
            incompletes: self.net_incompletes().to_vec(),
        }
    }

    /// Java `getAllClearanceViolations`.
    pub fn all_clearance_violations(&self) -> Vec<ClearanceViolation> {
        self.board.all_clearance_violations()
    }

    /// Java `getAllUnconnectedItems`.
    pub fn all_unconnected_items(&mut self) -> Vec<UnconnectedItems> {
        all_unconnected_items(self.board, &mut self.contacts)
    }

    /// Java `getZoneIslandViolations`.
    pub fn zone_island_violations(&mut self) -> Vec<ZoneIslandViolation> {
        if !self.check_zone_islands {
            return Vec::new();
        }
        zone_island_violations(self.board, &mut self.contacts)
    }

    /// Java `calculateAllIncompletes`: (re)calculates the incomplete
    /// connections of all nets.
    pub fn calculate_all_incompletes(&mut self) {
        let lists = net_item_lists(self.board);
        self.max_connections = max_connections(self.board, &lists);
        // The nets on the analysis threads, each worker with its own
        // contacts cache (pure values), merged in net order.
        let board = self.board;
        let nets: Vec<(i32, &Vec<ItemId>)> = (1..).zip(lists.iter()).collect();
        let (incompletes, caches) =
            crate::parallel::map_with(&nets, HashMap::default, |contacts, &(net, items)| {
                NetIncompletes::new_cached(board, net, items, contacts)
            });
        for cache in caches {
            self.contacts.extend(cache);
        }
        self.net_incompletes = Some(incompletes);
    }

    /// The incomplete connections of all nets (index 0 is net 1).
    pub fn net_incompletes(&mut self) -> &[NetIncompletes] {
        if self.net_incompletes.is_none() {
            self.calculate_all_incompletes();
        }
        self.net_incompletes.as_deref().expect("calculated")
    }

    /// Java `getNetIncompletes`.
    pub fn net_incompletes_of(&mut self, net_number: i32) -> Option<&NetIncompletes> {
        let all = self.net_incompletes();
        if net_number <= 0 {
            return None;
        }
        all.get(net_number as usize - 1)
    }

    /// Java `maxConnections`: per net the pins and conduction areas minus
    /// one.
    pub fn max_connections(&mut self) -> usize {
        self.net_incompletes();
        self.max_connections
    }

    /// Java `getIncompleteCount()`.
    pub fn incomplete_count(&mut self) -> usize {
        self.net_incompletes()
            .iter()
            .map(NetIncompletes::count)
            .sum()
    }

    /// Java `getIncompleteCount(int)`.
    pub fn incomplete_count_of(&mut self, net_number: i32) -> usize {
        self.net_incompletes_of(net_number)
            .map_or(0, NetIncompletes::count)
    }

    /// Java `incompleteNetNumbers`.
    pub fn incomplete_net_numbers(&mut self) -> BTreeSet<i32> {
        self.net_incompletes()
            .iter()
            .filter(|n| n.count() > 0)
            .map(|n| n.net_number)
            .collect()
    }

    /// Java `getLengthViolationCount`.
    pub fn length_violation_count(&mut self) -> usize {
        self.net_incompletes()
            .iter()
            .filter(|n| n.length_violation() != 0.0)
            .count()
    }

    /// Java `getLengthViolation(int)`.
    pub fn length_violation(&mut self, net_number: i32) -> f64 {
        self.net_incompletes_of(net_number)
            .map_or(0.0, NetIncompletes::length_violation)
    }

    /// Java `getAllAirlines`.
    pub fn all_airlines(&mut self) -> Vec<AirLine> {
        self.net_incompletes()
            .iter()
            .flat_map(|n| n.airlines.iter().cloned())
            .collect()
    }

    // ------------------------------------------------------------------
    // Report

    /// Java `generateReport` followed by the scores `DrcRunner` adds: the
    /// DRC report of the board in KiCad's JSON schema, with coordinates in
    /// `coordinate_unit` (`mm`, `mil`, `inch` or `um`; the board unit
    /// otherwise).
    pub fn generate_report(&mut self, source_file: &str, coordinate_unit: &str) -> DrcReport {
        let mut report = DrcReport::new(
            coordinate_unit,
            source_file,
            &format!("Freerouting {FREEROUTING_VERSION}"),
        );
        let violations = self.all_clearance_violations();
        for violation in &violations {
            report
                .violations
                .push(self.clearance_report_violation(violation, coordinate_unit));
        }
        for unconnected in self.all_unconnected_items() {
            let entry = self.unconnected_report_violation(&unconnected, coordinate_unit);
            if unconnected.kind == UnconnectedKind::UnconnectedItems {
                report.unconnected_items.push(entry);
            } else {
                report.violations.push(entry);
            }
        }
        for zone in self.zone_island_violations() {
            let entry = self.zone_report_violation(&zone, coordinate_unit);
            if zone.kind == ZoneIslandKind::IsolatedIslandUnconnected {
                report.unconnected_items.push(entry);
            } else {
                report.violations.push(entry);
            }
        }
        let max = self.max_connections();
        let incomplete = self.incomplete_count();
        let statistics =
            BoardStatistics::with_drc_results(self.board, Unit::Mm, &violations, max, incomplete);
        report.quality_score = Some(f64::from(statistics.router_score_v2_default()));
        report.optimizer_score = Some(f64::from(statistics.optimizer_score_v2_default()));
        report
    }

    /// Board units per millimetre for the summary messages, congestion zones and
    /// hints. PARITY(quirk, java_quirks.drc_units_10000_per_mm):
    /// DesignRulesChecker.java:455-475, 1338-1365 hard-code 10000; `false` takes
    /// the board's own scale.
    fn board_units_per_mm(&self) -> f64 {
        if self.board.java_quirks().drc_units_10000_per_mm {
            return 10000.0;
        }
        let communication = &self.board.communication;
        let dsn = communication.coordinate_transform.board_to_dsn(1.0);
        let mm = Unit::scale(dsn, communication.unit, Unit::Mm);
        if mm > 0.0 && mm.is_finite() {
            1.0 / mm
        } else {
            10000.0
        }
    }

    /// Java `convertCoordinate`.
    fn convert_coordinate(&self, value: f64, coordinate_unit: &str) -> f64 {
        let communication = &self.board.communication;
        let dsn = communication.coordinate_transform.board_to_dsn(value);
        let board_unit = communication.unit;
        let target = Unit::from_name(coordinate_unit)
            .filter(|_| matches!(coordinate_unit, "mm" | "mil" | "inch" | "um"))
            .unwrap_or(board_unit);
        if target != board_unit {
            Unit::scale(dsn, board_unit, target)
        } else {
            dsn
        }
    }

    fn position(&self, point: FloatPoint, unit: &str) -> DrcPosition {
        DrcPosition {
            x: self.convert_coordinate(point.x, unit),
            y: self.convert_coordinate(point.y, unit),
        }
    }

    fn report_item(&self, id: ItemId, description: String, unit: &str) -> DrcViolationItem {
        let centre = self.board.item(id).map_or(FloatPoint::new(0.0, 0.0), |i| {
            i.bounding_box().centre_of_gravity()
        });
        DrcViolationItem {
            description,
            pos: self.position(centre, unit),
            uuid: id.0.to_string(),
        }
    }

    fn clearance_report_violation(&self, v: &ClearanceViolation, unit: &str) -> DrcViolation {
        let first = self.item_description(v.first_item);
        let second = self.item_description(v.second_item);
        let is_hole = |id: ItemId| {
            self.board
                .item(id)
                .is_some_and(|i| i.is_via() || i.is_pin())
        };
        let hole = is_hole(v.first_item) || is_hole(v.second_item);
        let description = format!(
            "{} violation between {first} and {second} (expected: {} {unit}, actual: {} {unit})",
            if hole { "Hole clearance" } else { "Clearance" },
            java_format_fixed(self.convert_coordinate(v.expected_clearance, unit), 4),
            java_format_fixed(self.convert_coordinate(v.actual_clearance, unit), 4),
        );
        DrcViolation {
            description,
            items: vec![
                self.report_item(v.first_item, first, unit),
                self.report_item(v.second_item, second, unit),
            ],
            severity: "error".into(),
            violation_type: if hole { "holeClearance" } else { "clearance" }.into(),
        }
    }

    fn unconnected_report_violation(&self, u: &UnconnectedItems, unit: &str) -> DrcViolation {
        match u.kind {
            UnconnectedKind::TrackDangling | UnconnectedKind::ViaDangling => {
                let (description, item_description) = if u.kind == UnconnectedKind::ViaDangling {
                    (
                        "Via is not connected or connected on only one layer",
                        self.item_description(u.first_item),
                    )
                } else {
                    (
                        "Track has unconnected end",
                        self.detailed_trace_description(u.first_item, unit),
                    )
                };
                DrcViolation {
                    description: description.into(),
                    items: vec![self.report_item(u.first_item, item_description, unit)],
                    severity: "warning".into(),
                    violation_type: u.kind.type_name().into(),
                }
            }
            UnconnectedKind::UnconnectedItems => {
                let items = u
                    .all_items
                    .iter()
                    .map(|id| self.report_item(*id, self.item_description(*id), unit))
                    .collect();
                let from = self.item_description(u.first_item);
                let description = match u.second_item {
                    Some(second) => format!(
                        "Unconnected items: {from} and {} ({} total items in net)",
                        self.item_description(second),
                        u.all_items.len()
                    ),
                    None => format!("Unconnected item: {from}"),
                };
                DrcViolation {
                    description,
                    items,
                    severity: "warning".into(),
                    violation_type: u.kind.type_name().into(),
                }
            }
        }
    }

    fn zone_report_violation(&self, z: &ZoneIslandViolation, unit: &str) -> DrcViolation {
        let centre = z.center();
        let mut items = vec![DrcViolationItem {
            description: self.item_description(z.conduction_area),
            pos: self.position(centre, unit),
            uuid: z.conduction_area.0.to_string(),
        }];
        for id in &z.items_in_island {
            items.push(self.report_item(*id, self.item_description(*id), unit));
        }
        let net_name = if z.net_number > 0 {
            self.board
                .rules
                .nets
                .get(z.net_number)
                .map_or("unknown", |n| n.name.as_str())
        } else {
            "unknown"
        };
        let area = z.conduction_area.0;
        let description = match z.kind {
            ZoneIslandKind::IsolatedIslandDeadCopper => format!(
                "Isolated copper island without electrical connection on net [{net_name}] \
                 (Conduction Area #{area})"
            ),
            ZoneIslandKind::IsolatedIslandUnconnected => format!(
                "Isolated copper island on net [{net_name}] containing {} disconnected item(s) \
                 from Conduction Area #{area}",
                z.items_in_island.len()
            ),
        };
        DrcViolation {
            description,
            items,
            severity: match z.kind {
                ZoneIslandKind::IsolatedIslandUnconnected => "error",
                ZoneIslandKind::IsolatedIslandDeadCopper => "warning",
            }
            .into(),
            violation_type: z.kind.type_name().into(),
        }
    }

    /// The name of the first net of `item` in brackets with a leading
    /// space, or nothing.
    fn net_suffix(&self, item: &Item) -> String {
        if item.net_count() == 0 {
            return String::new();
        }
        // Java throws for an unknown net; the port prints "null".
        let name = self
            .board
            .rules
            .nets
            .get(item.net_number(0))
            .map_or("null", |n| n.name.as_str());
        format!(" [{name}]")
    }

    /// Java `getItemDescription`.
    pub fn item_description(&self, id: ItemId) -> String {
        let Some(item) = self.board.item(id) else {
            return "null".into();
        };
        let kind = match &item.kind {
            ItemKind::Trace(_) => "Trace",
            ItemKind::Via(_) => "Via",
            ItemKind::Pin(_) => "Pin",
            ItemKind::Area(area) => match area.kind {
                AreaKind::Conduction { .. } => "Conduction Area",
                AreaKind::Obstacle => "ObstacleArea",
                AreaKind::ViaObstacle => "ViaObstacleArea",
                AreaKind::TraceObstacle => "TraceObstacleArea",
                AreaKind::ComponentObstacle => "ComponentObstacleArea",
            },
            ItemKind::ComponentOutline(_) => "ComponentOutline",
            ItemKind::BoardOutline(_) => "BoardOutline",
        };
        format!("{kind}{}", self.net_suffix(item))
    }

    /// Java `getDetailedTraceDescription`.
    fn detailed_trace_description(&self, id: ItemId, unit: &str) -> String {
        let Some(item) = self.board.item(id) else {
            return "Track".into();
        };
        let mut result = format!("Track{}", self.net_suffix(item));
        if let Some(trace) = item.as_trace() {
            let layer_name = self
                .board
                .layer_structure
                .layers
                .get(trace.layer as usize)
                .map_or("", |l| l.name.as_str());
            let length = self.convert_coordinate(trace.length(), unit);
            result.push_str(&format!(
                " on {layer_name}, length {} {unit}",
                java_format_fixed(length, 4)
            ));
        }
        result
    }

    // ------------------------------------------------------------------
    // Summary

    /// Java `generateSummary`: a concise diagnostic summary with spatial
    /// congestion clusters and hints.
    pub fn generate_summary(&mut self) -> DrcSummary {
        let mut summary = DrcSummary::default();
        let per_mm = self.board_units_per_mm();
        let violations = self.all_clearance_violations();
        summary.clearance_violations_count = violations.len();
        let unconnected = self.all_unconnected_items();
        let mut unconnected_nets: HashSet<i32> = HashSet::default();
        let mut dangling_stubs = 0;
        for u in &unconnected {
            let first = self.board.item(u.first_item);
            if u.kind != UnconnectedKind::UnconnectedItems {
                dangling_stubs += 1;
            } else if let Some(item) = first.filter(|i| i.net_count() > 0) {
                unconnected_nets.insert(item.net_number(0));
            } else {
                dangling_stubs += 1;
            }
        }

        for v in &violations {
            let layer_name = self.layer_name(v.layer);
            let item1 = self.item_with_context(v.first_item);
            let item2 = self.item_with_context(v.second_item);
            let expected_mm = v.expected_clearance / per_mm;
            let actual_mm = v.actual_clearance / per_mm;
            let shortfall_mm = ((v.expected_clearance - v.actual_clearance) / per_mm).max(0.0);
            summary.violations.push(DiagnosticViolation {
                violation_type: "clearance".into(),
                severity: "error".into(),
                explanation: format!(
                    "Clearance violation between {item1} and {item2} on {layer_name}: required \
                     {} mm, found {} mm (shortfall: {} mm).",
                    java_format_fixed(expected_mm, 3),
                    java_format_fixed(actual_mm, 3),
                    java_format_fixed(shortfall_mm, 3)
                ),
                layer: layer_name,
                expected_clearance_mm: Some(expected_mm),
                actual_clearance_mm: Some(actual_mm),
                shortfall_mm: Some(shortfall_mm),
                items: vec![item1, item2],
            });
        }

        for u in &unconnected {
            let item1 = self.item_with_context(u.first_item);
            let item2 = u.second_item.map(|i| self.item_with_context(i));
            let net_name = self
                .board
                .item(u.first_item)
                .filter(|i| i.net_count() > 0)
                .and_then(|i| self.board.rules.nets.get(i.net_number(0)))
                .map_or("unknown".to_string(), |n| n.name.clone());
            let explanation = match (u.kind, &item2) {
                (UnconnectedKind::TrackDangling, _) => {
                    format!("Dangling trace segment on net [{net_name}] with unconnected endpoint.")
                }
                (UnconnectedKind::ViaDangling, _) => {
                    format!("Dangling via on net [{net_name}] connected on at most one layer.")
                }
                (_, Some(item2)) => format!(
                    "Unconnected net [{net_name}]: break between {item1} and {item2} ({} \
                     elements).",
                    u.all_items.len()
                ),
                (_, None) => format!("Unconnected item on net [{net_name}]: {item1}."),
            };
            let mut items = vec![item1];
            items.extend(item2);
            summary.violations.push(DiagnosticViolation {
                violation_type: u.kind.type_name().into(),
                severity: "warning".into(),
                layer: "all".into(),
                explanation,
                expected_clearance_mm: None,
                actual_clearance_mm: None,
                shortfall_mm: None,
                items,
            });
        }

        for z in self.zone_island_violations() {
            let layer_name = self.layer_name(z.layer);
            let net_name = self
                .board
                .rules
                .nets
                .get(z.net_number)
                .map_or(format!("Net {}", z.net_number), |n| n.name.clone());
            let mut items = vec![format!("Zone on {layer_name} [{net_name}]")];
            items.extend(z.items_in_island.iter().map(|i| self.item_with_context(*i)));
            let (severity, explanation) = match z.kind {
                ZoneIslandKind::IsolatedIslandUnconnected => {
                    if z.net_number > 0 {
                        unconnected_nets.insert(z.net_number);
                    } else {
                        dangling_stubs += 1;
                    }
                    (
                        "error",
                        format!(
                            "Copper pour on {layer_name} [net {net_name}] is fragmented: island \
                             contains {} pin(s)/via(s) disconnected from main pour.",
                            z.items_in_island.len()
                        ),
                    )
                }
                ZoneIslandKind::IsolatedIslandDeadCopper => {
                    let resolution = self.board.communication.get_resolution(Unit::Mm);
                    let area_mm2 = z.area_in_board_units() / (resolution * resolution);
                    (
                        "warning",
                        format!(
                            "Dead copper island detected on {layer_name} [net {net_name}]: area \
                             {} mm\u{b2} with no electrical connections.",
                            java_format_fixed(area_mm2, 2)
                        ),
                    )
                }
            };
            summary.violations.push(DiagnosticViolation {
                violation_type: z.kind.type_name().into(),
                severity: severity.into(),
                layer: layer_name,
                explanation,
                expected_clearance_mm: None,
                actual_clearance_mm: None,
                shortfall_mm: None,
                items,
            });
        }

        summary.unconnected_nets_count = unconnected_nets.len() + dangling_stubs;
        summary.congestion_zones = detect_congestion_zones(&violations, per_mm);
        summary.hints =
            actionable_hints(&violations, &unconnected, &summary.congestion_zones, per_mm);
        summary
    }

    fn layer_name(&self, layer: i32) -> String {
        match self
            .board
            .layer_structure
            .layers
            .get(layer.max(-1) as usize)
        {
            Some(l) if layer >= 0 => l.name.clone(),
            _ => format!("Layer {layer}"),
        }
    }

    /// Java `formatItemWithContext`.
    fn item_with_context(&self, id: ItemId) -> String {
        let Some(item) = self.board.item(id) else {
            return "none".into();
        };
        let mut result = match &item.kind {
            ItemKind::Pin(pin) => {
                let mut s = "Pin ".to_string();
                match self.board.components.get(ComponentId(item.component_id())) {
                    Some(component) => {
                        s.push_str(&component.name);
                        s.push('.');
                        match self.board.pin_name(item) {
                            Some(name) => s.push_str(name),
                            None => s.push_str(&(pin.pin_index + 1).to_string()),
                        }
                    }
                    None => s.push_str(&format!("#{}", pin.pin_index + 1)),
                }
                s
            }
            ItemKind::Trace(_) => "Trace".into(),
            ItemKind::Via(_) => "Via".into(),
            ItemKind::Area(area) => match area.kind {
                AreaKind::Conduction { .. } => "ConductionArea",
                AreaKind::Obstacle => "ObstacleArea",
                AreaKind::ViaObstacle => "ViaObstacleArea",
                AreaKind::TraceObstacle => "TraceObstacleArea",
                AreaKind::ComponentObstacle => "ComponentObstacleArea",
            }
            .into(),
            ItemKind::ComponentOutline(_) => "ComponentOutline".into(),
            ItemKind::BoardOutline(_) => "BoardOutline".into(),
        };
        if item.net_count() > 0
            && let Some(net) = self.board.rules.nets.get(item.net_number(0))
        {
            result.push_str(&format!(" (net {})", net.name));
        }
        result
    }
}

/// Java `detectCongestionZones`: greedy clustering of the violation
/// centres within 5 mm (assuming 10000 board units per mm like Java).
fn detect_congestion_zones(violations: &[ClearanceViolation], per_mm: f64) -> Vec<CongestionZone> {
    const CLUSTER_RADIUS_MM: f64 = 5.0;
    let points: Vec<(f64, f64)> = violations
        .iter()
        .map(|v| {
            let p = v.shape.centre_of_gravity().round();
            (f64::from(p.x) / per_mm, f64::from(p.y) / per_mm)
        })
        .collect();
    let mut visited = vec![false; points.len()];
    let mut zones = Vec::new();
    for i in 0..points.len() {
        if visited[i] {
            continue;
        }
        visited[i] = true;
        let (x1, y1) = points[i];
        let (mut count, mut sum_x, mut sum_y) = (1usize, x1, y1);
        for j in i + 1..points.len() {
            if visited[j] {
                continue;
            }
            let (x2, y2) = points[j];
            if (x1 - x2).hypot(y1 - y2) <= CLUSTER_RADIUS_MM {
                visited[j] = true;
                count += 1;
                sum_x += x2;
                sum_y += y2;
            }
        }
        if count >= 2 {
            let (avg_x, avg_y) = (sum_x / count as f64, sum_y / count as f64);
            zones.push(CongestionZone {
                center_x_mm: avg_x,
                center_y_mm: avg_y,
                radius_mm: CLUSTER_RADIUS_MM,
                violation_count: count,
                description: format!(
                    "High bottleneck concentration: {count} violations within {} mm of ({}, {}) \
                     mm.",
                    java_format_fixed(CLUSTER_RADIUS_MM, 1),
                    java_format_fixed(avg_x, 2),
                    java_format_fixed(avg_y, 2)
                ),
            });
        }
    }
    zones
}

/// Java `generateActionableHints`.
fn actionable_hints(
    violations: &[ClearanceViolation],
    unconnected: &[UnconnectedItems],
    zones: &[CongestionZone],
    per_mm: f64,
) -> Vec<String> {
    let mut hints = Vec::new();
    let max_shortfall = violations
        .iter()
        .map(|v| (v.expected_clearance - v.actual_clearance) / per_mm)
        .fold(0.0, f64::max);
    if max_shortfall > 0.0 {
        hints.push(format!(
            "Maximum clearance shortfall is {} mm. Consider reducing trace width or clearance \
             class threshold if manufacturing constraints permit.",
            java_format_fixed(max_shortfall, 3)
        ));
    }
    if !zones.is_empty() {
        hints.push(format!(
            "Detected {} congested hotspot zone(s). Increasing pin escape distance or spreading \
             component pads in these areas may resolve routing conflicts.",
            zones.len()
        ));
    }
    if !unconnected.is_empty() {
        let dangling = unconnected
            .iter()
            .filter(|u| u.kind != UnconnectedKind::UnconnectedItems)
            .count();
        if dangling > 0 {
            hints.push(format!(
                "{dangling} dangling stub(s) detected. Running an optimizer cleanup pass will \
                 prune unneeded stubs."
            ));
        }
        let incomplete = unconnected.len() - dangling;
        if incomplete > 0 {
            hints.push(format!(
                "{incomplete} unrouted net connection(s) remain. Verify layer count or allow \
                 additional routing passes (-mp / maxPasses)."
            ));
        }
    }
    if hints.is_empty() {
        hints.push("All design rules satisfied. Board is DRC clean.".into());
    }
    hints
}
