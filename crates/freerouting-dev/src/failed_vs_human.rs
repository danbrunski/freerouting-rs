//! `freerouting-dev failed-vs-human`: the connections an engine leaves
//! unrouted, next to what the human-routed original did (F03.2).
//!
//! The pattern is KiCadRoutingTools' `tests/stress/compare_failed_nets.py`.
//! Inputs:
//!
//! - the **unrouted** board (`board.dsn`, e.g. `target/corpus/prepped/<id>.dsn`);
//! - the **engine's session** for it (`engine.ses`);
//! - the **human-routed original** as a DSN with its tracks kept
//!   (`tools/corpus/prep.sh` writes `prepped/<id>.human.dsn`).
//!
//! For every incomplete connection (airline) of the board after importing
//! the session (in place of the board's wiring that the session repeats,
//! [`crate::ses_diff::import_routed_session`]), the report lists the net,
//! the two endpoints (the pins of each connected cluster), the distance, and
//! what the human's copper did for the same two ends: whether it connected
//! them, on which layers, with how many vias, at which widths and with how
//! much **clearance headroom**.
//!
//! Each connection is tagged:
//!
//! - `router gap`: the human routed it;
//! - `no human route`: the human original does not connect these two ends
//!   either (also the case for a connection that exists only in the router's
//!   cluster model, or a board whose original is unrouted);
//! - `infeasible`: the static diagnostic of Q03 says that no trace of the
//!   net class width plus clearance reaches one of the endpoint pins (so the
//!   router can't be blamed; `human.min_width_mm` shows how narrow the human
//!   went).
//!
//! `router gap` and `infeasible` can both apply: the human routed a pin the
//! class width can't reach (necked down, or a narrower class).
//!
//! Headroom is the smallest gap between the human's copper of the net and
//! any foreign item on the same layer, minus the required clearance, sampled
//! at trace corners and segment midpoints. It is an upper bound of the true
//! minimum (a closest approach between samples is missed), reported in mm;
//! `null` when no foreign copper is within 1 mm beyond the clearance. A
//! value near zero means the human routed at the limit (the DSN's integer
//! rounding adds a few µm of noise).
//!
//! Pins are matched between the boards by (component, pin) name. The
//! engine's own board is read with its own connectivity (`connected_set`
//! over normal contacts, the model the router used); the human original by
//! geometric overlap of same-net copper (see `overlap_clusters`).

use std::collections::{BTreeMap, BTreeSet};

use freerouting::board::items::{Item, ItemId};
use freerouting::board::structure::{ComponentId, Unit};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::drc::DesignRulesChecker;
use freerouting::drc::json::JsonValue;
use freerouting::dsn::Session;
use freerouting::geometry::{FloatPoint, IntBox, Shape};
use freerouting::quality::feasibility::{FeasibilityOptions, Verdict, check_board};

/// A pin as (component, pin name).
pub type PinName = (String, String);

/// One end of an unrouted connection.
#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    /// The pins of the end's connected cluster, sorted.
    pub pins: Vec<PinName>,
    /// The airline's end in DSN millimetres.
    pub x_mm: f64,
    pub y_mm: f64,
}

impl Endpoint {
    fn label(&self) -> String {
        match self.pins.as_slice() {
            [] => format!("(no pin) at {:.2},{:.2}", self.x_mm, self.y_mm),
            [(c, p)] => format!("{c}-{p}"),
            [(c, p), rest @ ..] => format!("{c}-{p}+{}", rest.len()),
        }
    }
}

/// What the human's copper did for one connection.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HumanRoute {
    /// The human connected the two ends.
    pub routed: bool,
    pub layers: Vec<String>,
    pub vias: usize,
    pub tracks: usize,
    pub min_width_mm: f64,
    pub max_width_mm: f64,
    pub length_mm: f64,
    /// Smallest gap minus required clearance over the sampled points, mm.
    pub headroom_mm: Option<f64>,
}

/// One unrouted connection.
#[derive(Clone, Debug, PartialEq)]
pub struct FailedConnection {
    pub net: String,
    pub from: Endpoint,
    pub to: Endpoint,
    pub distance_mm: f64,
    pub human: HumanRoute,
    /// Endpoint pins the Q03 diagnostic calls infeasible, with their class
    /// width in mm.
    pub infeasible_pins: Vec<(PinName, f64)>,
    pub tags: Vec<&'static str>,
}

/// The report.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub connections: Vec<FailedConnection>,
    /// Connections the human original leaves open, counted by overlap: per
    /// net, the clusters holding a pin, minus one (a board the human left
    /// partly unrouted, or whose pours the DSN export can't express).
    pub human_unrouted_items: usize,
}

impl Report {
    /// Counts per tag, in tag order.
    pub fn counts(&self) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for c in &self.connections {
            for tag in &c.tags {
                *counts.entry(*tag).or_insert(0) += 1;
            }
        }
        counts
    }

    /// For people: one block per connection, then the totals.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for c in &self.connections {
            out.push_str(&format!(
                "net {}: {} -> {}  {:.2} mm  [{}]\n",
                c.net,
                c.from.label(),
                c.to.label(),
                c.distance_mm,
                c.tags.join(", ")
            ));
            if c.human.routed {
                out.push_str(&format!(
                    "    human: {} on {}, {} via(s), width {:.3}-{:.3} mm, length {:.1} mm, \
                     headroom {}\n",
                    if c.human.tracks == 1 {
                        "1 track".to_string()
                    } else {
                        format!("{} tracks", c.human.tracks)
                    },
                    c.human.layers.join("+"),
                    c.human.vias,
                    c.human.min_width_mm,
                    c.human.max_width_mm,
                    c.human.length_mm,
                    c.human
                        .headroom_mm
                        .map_or("n/a".to_string(), |h| format!("{h:.3} mm"))
                ));
            } else {
                out.push_str("    human: no route between these ends\n");
            }
            for ((comp, pin), width) in &c.infeasible_pins {
                out.push_str(&format!(
                    "    infeasible: no {width:.3} mm trace reaches {comp}-{pin} (Q03)\n"
                ));
            }
        }
        let counts = self.counts();
        out.push_str(&format!(
            "{} unrouted connection(s): router gap {}, infeasible {}, no human route {} \
             (the human original leaves {} connection(s) open by overlap)\n",
            self.connections.len(),
            counts.get("router gap").unwrap_or(&0),
            counts.get("infeasible").unwrap_or(&0),
            counts.get("no human route").unwrap_or(&0),
            self.human_unrouted_items
        ));
        out
    }

    /// The report as JSON.
    pub fn to_json(&self) -> JsonValue {
        let num = |v: f64| JsonValue::Number(format!("{v:.3}"));
        let pins = |pins: &[PinName]| {
            JsonValue::Array(
                pins.iter()
                    .map(|(c, p)| JsonValue::string(format!("{c}-{p}")))
                    .collect(),
            )
        };
        let endpoint = |e: &Endpoint| {
            JsonValue::Object(vec![
                ("pins".into(), pins(&e.pins)),
                ("x_mm".into(), num(e.x_mm)),
                ("y_mm".into(), num(e.y_mm)),
            ])
        };
        let connections = self
            .connections
            .iter()
            .map(|c| {
                JsonValue::Object(vec![
                    ("net".into(), JsonValue::string(c.net.clone())),
                    ("from".into(), endpoint(&c.from)),
                    ("to".into(), endpoint(&c.to)),
                    ("distance_mm".into(), num(c.distance_mm)),
                    (
                        "tags".into(),
                        JsonValue::Array(c.tags.iter().map(|t| JsonValue::string(*t)).collect()),
                    ),
                    (
                        "human".into(),
                        JsonValue::Object(vec![
                            ("routed".into(), JsonValue::Bool(c.human.routed)),
                            (
                                "layers".into(),
                                JsonValue::Array(
                                    c.human
                                        .layers
                                        .iter()
                                        .cloned()
                                        .map(JsonValue::String)
                                        .collect(),
                                ),
                            ),
                            ("vias".into(), JsonValue::int(c.human.vias as i64)),
                            ("tracks".into(), JsonValue::int(c.human.tracks as i64)),
                            ("min_width_mm".into(), num(c.human.min_width_mm)),
                            ("max_width_mm".into(), num(c.human.max_width_mm)),
                            ("length_mm".into(), num(c.human.length_mm)),
                            (
                                "headroom_mm".into(),
                                c.human.headroom_mm.map_or(JsonValue::Null, num),
                            ),
                        ]),
                    ),
                    (
                        "infeasible_pins".into(),
                        JsonValue::Array(
                            c.infeasible_pins
                                .iter()
                                .map(|((comp, pin), width)| {
                                    JsonValue::Object(vec![
                                        ("pin".into(), JsonValue::string(format!("{comp}-{pin}"))),
                                        ("class_width_mm".into(), num(*width)),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect();
        let counts = self.counts();
        JsonValue::Object(vec![
            (
                "unrouted_connections".into(),
                JsonValue::int(self.connections.len() as i64),
            ),
            (
                "counts".into(),
                JsonValue::Object(
                    counts
                        .iter()
                        .map(|(k, v)| (k.to_string(), JsonValue::int(*v as i64)))
                        .collect(),
                ),
            ),
            ("connections".into(), JsonValue::Array(connections)),
        ])
    }
}

fn pin_name_of(board: &BasicBoard, item: &Item) -> Option<PinName> {
    if !item.is_pin() {
        return None;
    }
    let component = board.components.get(ComponentId(item.component_id()))?;
    let pin = board.pin_name(item)?;
    Some((component.name.clone(), pin.to_string()))
}

fn to_mm(board: &BasicBoard, point: FloatPoint) -> (f64, f64) {
    let comm = &board.communication;
    let [x, y] = comm.coordinate_transform.board_to_dsn_point(point);
    (
        Unit::scale(x, comm.unit, Unit::Mm),
        Unit::scale(y, comm.unit, Unit::Mm),
    )
}

fn layer_name(board: &BasicBoard, layer: i32) -> String {
    board
        .layer_structure
        .layers
        .get(layer as usize)
        .map_or_else(|| layer.to_string(), |l| l.name.clone())
}

/// The pins in the connected cluster of `item` on net `net`.
fn cluster_pins(board: &BasicBoard, item: ItemId, net: i32) -> Vec<PinName> {
    let mut pins: Vec<PinName> = board
        .connected_set(item, net, false)
        .into_iter()
        .filter_map(|id| board.item(id).and_then(|i| pin_name_of(board, i)))
        .collect();
    pins.sort();
    pins.dedup();
    pins
}

/// The clusters of one net's copper by geometric overlap on a common layer.
///
/// The engine's own contact model joins a trace to a pin only at the pin's
/// centre (Java's rule). KiCad's DSN export rounds track ends to whole
/// micrometres, so a human track ending on a pad centre of `x.5` µm is
/// "unconnected" in that model although it is plainly connected. The human
/// original is judged by overlap instead.
fn overlap_clusters(board: &BasicBoard, net: i32) -> Vec<BTreeSet<ItemId>> {
    let ids = board.connectable_items(net);
    let position: BTreeMap<ItemId, usize> =
        ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let mut parent: Vec<usize> = (0..ids.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for (i, id) in ids.iter().enumerate() {
        let Some(item) = board.item(*id) else {
            continue;
        };
        for index in 0..board.tile_shape_count(item) {
            let layer = item.shape_layer(index);
            let Some(tile) = board.tile_shape(item, index) else {
                continue;
            };
            for other in board.overlapping_objects(&tile, layer) {
                if let Some(&j) = position.get(&other) {
                    let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                    parent[ri] = rj;
                }
            }
        }
    }
    let mut groups: BTreeMap<usize, BTreeSet<ItemId>> = BTreeMap::new();
    for (i, id) in ids.iter().enumerate() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().insert(*id);
    }
    groups.into_values().collect()
}

struct HumanIndex<'a> {
    board: &'a BasicBoard,
    pins: BTreeMap<PinName, ItemId>,
    clusters: BTreeMap<i32, Vec<BTreeSet<ItemId>>>,
}

impl<'a> HumanIndex<'a> {
    fn new(board: &'a BasicBoard) -> Self {
        let pins = board
            .items()
            .filter_map(|item| pin_name_of(board, item).map(|name| (name, item.id())))
            .collect();
        HumanIndex {
            board,
            pins,
            clusters: BTreeMap::new(),
        }
    }

    /// The clusters an end of a connection belongs to in the human board:
    /// those holding one of its pins, or, for an end without pins (a pour
    /// or a via cluster), those with copper containing the end's point.
    fn end_clusters(&mut self, net: i32, pins: &[PinName], at: FloatPoint) -> Vec<usize> {
        let board = self.board;
        let clusters = self
            .clusters
            .entry(net)
            .or_insert_with(|| overlap_clusters(board, net));
        let pin_ids: Vec<ItemId> = pins
            .iter()
            .filter_map(|p| self.pins.get(p).copied())
            .collect();
        let contains_point = |set: &BTreeSet<ItemId>| {
            set.iter().any(|id| {
                board.item(*id).is_some_and(|item| {
                    (0..board.tile_shape_count(item)).any(|index| {
                        board
                            .tile_shape(item, index)
                            .is_some_and(|tile| Shape::from(tile).contains_float(at))
                    })
                })
            })
        };
        clusters
            .iter()
            .enumerate()
            .filter(|(_, set)| {
                if pins.is_empty() {
                    contains_point(set)
                } else {
                    pin_ids.iter().any(|id| set.contains(id))
                }
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// The human copper connecting the two ends, if the human connected them.
    fn route(
        &mut self,
        net: i32,
        from: (&[PinName], FloatPoint),
        to: (&[PinName], FloatPoint),
    ) -> Option<BTreeSet<ItemId>> {
        let a = self.end_clusters(net, from.0, from.1);
        let b = self.end_clusters(net, to.0, to.1);
        let common = a.iter().find(|i| b.contains(i))?;
        self.clusters.get(&net)?.get(*common).cloned()
    }
}

/// The human copper in `set`: layers, vias, widths, length, headroom.
fn describe_route(board: &BasicBoard, set: &BTreeSet<ItemId>, net: i32) -> HumanRoute {
    let units_per_mm = freerouting::quality::feasibility::units_per_mm(board);
    let mut layers = BTreeSet::new();
    let (mut vias, mut tracks) = (0, 0);
    let (mut min_w, mut max_w, mut length) = (f64::MAX, 0.0_f64, 0.0);
    let mut headroom: Option<f64> = None;
    for id in set {
        let Some(item) = board.item(*id) else {
            continue;
        };
        if item.is_via() {
            vias += 1;
            continue;
        }
        let Some(trace) = item.as_trace() else {
            continue;
        };
        tracks += 1;
        layers.insert(trace.layer);
        let width = f64::from(trace.half_width) * 2.0 / units_per_mm;
        min_w = min_w.min(width);
        max_w = max_w.max(width);
        length += trace.polyline.length_approx() / units_per_mm;
        if let Some(h) = trace_headroom(board, item, net) {
            headroom = Some(headroom.map_or(h, |old: f64| old.min(h)));
        }
    }
    HumanRoute {
        routed: true,
        layers: layers.into_iter().map(|l| layer_name(board, l)).collect(),
        vias,
        tracks,
        min_width_mm: if tracks == 0 { 0.0 } else { min_w },
        max_width_mm: max_w,
        length_mm: length,
        headroom_mm: headroom.map(|h| h / units_per_mm),
    }
}

/// How far the trace is from violating clearance against foreign copper, in
/// board units, sampled at its corners and segment midpoints.
fn trace_headroom(board: &BasicBoard, item: &Item, net: i32) -> Option<f64> {
    let trace = item.as_trace()?;
    let half = f64::from(trace.half_width);
    let mut samples = Vec::new();
    let corners = trace.polyline.corner_count();
    for i in 0..corners {
        let p = trace.polyline.corner(i).to_float();
        if let Some(prev) = samples.last().copied() {
            let q: FloatPoint = prev;
            samples.push(FloatPoint {
                x: (p.x + q.x) / 2.0,
                y: (p.y + q.y) / 2.0,
            });
        }
        samples.push(p);
    }
    // Foreign copper is looked for within the clearance and 1 mm beyond.
    let reach = half
        + f64::from(board.rules.clearance_matrix.max_value_on_layer(trace.layer))
        + freerouting::quality::feasibility::units_per_mm(board);
    let mut best: Option<f64> = None;
    for p in samples {
        let area = IntBox::new(
            (p.x - reach).floor() as i32,
            (p.y - reach).floor() as i32,
            (p.x + reach).ceil() as i32,
            (p.y + reach).ceil() as i32,
        );
        for other_id in board.overlapping_objects(&area.into(), trace.layer) {
            let Some(other) = board.item(other_id) else {
                continue;
            };
            let copper = other.is_drill_item() || other.is_trace() || other.is_conduction_area();
            if !copper || other.contains_net(net) {
                continue;
            }
            let clearance = f64::from(board.clearance_value(
                item.clearance_class(),
                other.clearance_class(),
                trace.layer,
            ));
            if let Some(gap) = copper_distance(board, other, trace.layer, p) {
                let gap = gap - half - clearance;
                best = Some(best.map_or(gap, |b| b.min(gap)));
            }
        }
    }
    best
}

/// The distance from `p` to the copper of `other` on `layer`, board units.
/// Pins and vias use their pad shape and traces their centre line less the
/// half width; other items use the search tree's tile shape, which is
/// enlarged by the clearance compensation, so their distance is a lower
/// bound.
fn copper_distance(board: &BasicBoard, other: &Item, layer: i32, p: FloatPoint) -> Option<f64> {
    if let Some(drill) = other.drill() {
        return drill.shape_on_layer(layer).map(|shape| shape.distance(p));
    }
    if let Some(trace) = other.as_trace() {
        if trace.layer != layer {
            return None;
        }
        let corners: Vec<FloatPoint> = (0..trace.polyline.corner_count())
            .map(|i| trace.polyline.corner(i).to_float())
            .collect();
        let nearest = corners
            .windows(2)
            .map(|w| point_segment_distance(p, w[0], w[1]))
            .fold(f64::MAX, f64::min);
        return Some(nearest - f64::from(trace.half_width));
    }
    let mut best: Option<f64> = None;
    for index in 0..board.tile_shape_count(other) {
        if other.shape_layer(index) != layer {
            continue;
        }
        let tile = board.tile_shape(other, index)?;
        let d = Shape::from(tile).distance(p);
        best = Some(best.map_or(d, |b| b.min(d)));
    }
    best
}

fn point_segment_distance(p: FloatPoint, a: FloatPoint, b: FloatPoint) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length_square = dx * dx + dy * dy;
    let t = if length_square == 0.0 {
        0.0
    } else {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / length_square).clamp(0.0, 1.0)
    };
    (p.x - (a.x + t * dx)).hypot(p.y - (a.y + t * dy))
}

/// Builds the report. `unrouted` is the input board, `human` the original
/// with its tracks. `feasibility` is the Q03 diagnostic's options, `None` to
/// skip it (no `infeasible` tags then).
pub fn analyze(
    unrouted: &RoutingBoard,
    session: &Session,
    human: &BasicBoard,
    feasibility: Option<&FeasibilityOptions>,
) -> Report {
    let mut engine: RoutingBoard = unrouted.clone();
    crate::ses_diff::import_routed_session(&mut engine, session);
    let airlines = DesignRulesChecker::new(&engine).all_airlines();
    let infeasible: BTreeMap<PinName, f64> = feasibility
        .map(|opts| {
            let units = freerouting::quality::feasibility::units_per_mm(unrouted);
            check_board(unrouted, opts)
                .pins
                .iter()
                .filter(|p| p.verdict == Verdict::Infeasible)
                .map(|p| ((p.component.clone(), p.pin.clone()), p.width as f64 / units))
                .collect()
        })
        .unwrap_or_default();
    let mut index = HumanIndex::new(human);
    let mut connections = Vec::new();
    for line in &airlines {
        let net_name = engine
            .rules
            .nets
            .get(line.net_number)
            .map(|n| n.name.clone())
            .unwrap_or_default();
        let from_pins = cluster_pins(&engine, line.from_item, line.net_number);
        let to_pins = cluster_pins(&engine, line.to_item, line.net_number);
        let human_net = human
            .rules
            .nets
            .get_by_name(&net_name, 1)
            .map_or(0, |n| n.net_number);
        let route = if human_net > 0 {
            index.route(
                human_net,
                (&from_pins, line.from_corner),
                (&to_pins, line.to_corner),
            )
        } else {
            None
        };
        let human_route = route.as_ref().map_or_else(HumanRoute::default, |set| {
            describe_route(human, set, human_net)
        });
        let infeasible_pins: Vec<(PinName, f64)> = from_pins
            .iter()
            .chain(to_pins.iter())
            .filter_map(|p| infeasible.get(p).map(|w| (p.clone(), *w)))
            .collect();
        let mut tags = Vec::new();
        if !infeasible_pins.is_empty() {
            tags.push("infeasible");
        }
        tags.push(if human_route.routed {
            "router gap"
        } else {
            "no human route"
        });
        let (fx, fy) = to_mm(&engine, line.from_corner);
        let (tx, ty) = to_mm(&engine, line.to_corner);
        connections.push(FailedConnection {
            net: net_name,
            distance_mm: ((tx - fx).powi(2) + (ty - fy).powi(2)).sqrt(),
            from: Endpoint {
                pins: from_pins,
                x_mm: fx,
                y_mm: fy,
            },
            to: Endpoint {
                pins: to_pins,
                x_mm: tx,
                y_mm: ty,
            },
            human: human_route,
            infeasible_pins,
            tags,
        });
    }
    connections.sort_by(|a, b| {
        a.net
            .cmp(&b.net)
            .then_with(|| a.from.pins.cmp(&b.from.pins))
            .then_with(|| a.to.pins.cmp(&b.to.pins))
    });
    Report {
        connections,
        human_unrouted_items: human_open_connections(human),
    }
}

/// The usage text.
pub const USAGE: &str = "usage: freerouting-dev failed-vs-human <board.dsn> <engine.ses> <human.dsn> \
[--json out.json] [--no-feasibility] [--step-um N]";

/// The command line: prints the report, writes `--json`. Exit status 0 when
/// it ran (the verdicts are the report), 2 on an error.
pub fn run(args: &[String]) -> Result<std::process::ExitCode, String> {
    let mut positional = Vec::new();
    let (mut json, mut step_um, mut feasibility) = (None, None, true);
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--no-feasibility" => feasibility = false,
            "--json" | "--step-um" => {
                let value = args
                    .get(i + 1)
                    .ok_or_else(|| format!("{} needs a value\n{USAGE}", args[i]))?;
                if args[i] == "--json" {
                    json = Some(value.clone());
                } else {
                    step_um = Some(
                        value
                            .parse::<f64>()
                            .ok()
                            .filter(|s| *s > 0.0)
                            .ok_or_else(|| format!("bad --step-um '{value}'\n{USAGE}"))?,
                    );
                }
                i += 1;
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown option {other}\n{USAGE}"));
            }
            other => positional.push(other.to_string()),
        }
        i += 1;
    }
    let [board, ses, human] = positional.as_slice() else {
        return Err(USAGE.to_string());
    };
    let unrouted = crate::load_board(std::path::Path::new(board))?;
    let session = crate::ses_diff::parse_session(&crate::read_text(std::path::Path::new(ses))?)
        .map_err(|e| format!("{ses}: {e}"))?;
    let human_board = crate::load_board(std::path::Path::new(human))?;
    let mut options = FeasibilityOptions::default();
    if let Some(step) = step_um {
        options.step_um = step;
    }
    let report = analyze(
        &unrouted,
        &session,
        &human_board,
        feasibility.then_some(&options),
    );
    print!("{}", report.to_text());
    if let Some(path) = json {
        std::fs::write(&path, report.to_json().to_pretty_string() + "\n")
            .map_err(|e| format!("{path}: {e}"))?;
    }
    Ok(std::process::ExitCode::SUCCESS)
}

/// Connections the board leaves open, counted by overlap clusters.
pub fn human_open_connections(board: &BasicBoard) -> usize {
    let mut nets = BTreeSet::new();
    for item in board.items().filter(|i| i.is_pin()) {
        nets.extend(item.net_numbers().iter().copied().filter(|n| *n > 0));
    }
    nets.into_iter()
        .map(|net| {
            let with_pin = overlap_clusters(board, net)
                .iter()
                .filter(|set| {
                    set.iter()
                        .any(|id| board.item(*id).is_some_and(Item::is_pin))
                })
                .count();
            with_pin.saturating_sub(1)
        })
        .sum()
}
