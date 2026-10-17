//! Q01.4: which pours routing cut (a report; it changes nothing).
//!
//! For each filled conduction area, in ascending item id, the islands of
//! Java's detailed fill ([`conduction_fill`], `ConductionArea.
//! getDetailedFillArea` as the DRC computes it) on the board before and
//! after routing, and per island the pins and vias of the area's net whose
//! centre lies on it (the fill model's anchors). Per pour net the report
//! gives the island count before and after, the pins attached to some
//! island before and to none after, and, for each area whose island count
//! rose, the foreign nets of the traces routing added across it (the cut).
//! The idea is KiCadRoutingTools' `pour_guard.audit_pour_cuts` (at
//! `0486e799`: open pads per zone net before and after; it reports and
//! doesn't refuse); no code is ported.
//!
//! The fill is Java's: KiCad's minimum-thickness opening is not modelled
//! (`drc::fill_model`), so a neck KiCad's refill removes still counts as
//! copper here.

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::{PolylineShapeOps, TileShape};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::board::structure::ComponentId;
use crate::drc::conduction_fill;

/// The pour-cut report of one routing run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PourAudit {
    /// One entry per net that has a filled conduction area, ascending net
    /// number.
    pub nets: Vec<PourNetAudit>,
}

/// The report of one pour net.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PourNetAudit {
    pub net: i32,
    pub net_name: String,
    /// The net's filled conduction areas, ascending item id.
    pub areas: Vec<i32>,
    /// The islands of all the net's areas before routing.
    pub islands_before: usize,
    /// The islands after routing.
    pub islands_after: usize,
    /// The pins attached to an island before and to none after, ascending
    /// item id, as `component-pin` (or `#id`).
    pub lost_pins: Vec<String>,
    /// The areas whose island count rose, with the foreign nets of the new
    /// traces that cross them.
    pub cuts: Vec<PourCut>,
}

/// One cut pour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PourCut {
    pub area: i32,
    pub layer: String,
    pub islands_before: usize,
    pub islands_after: usize,
    /// The names of the foreign nets of the traces in `after` but not in
    /// `before` that overlap the area, ascending net number.
    pub by_nets: Vec<String>,
}

impl PourAudit {
    /// True if no pour lost an island connection or was split.
    pub fn is_clean(&self) -> bool {
        self.nets.iter().all(PourNetAudit::is_clean)
    }

    /// The report as JSON (`pour_audit` in the CLI result, C08), keys in a
    /// fixed order.
    pub fn to_json(&self) -> String {
        let nets: Vec<String> = self.nets.iter().map(PourNetAudit::to_json).collect();
        format!("{{\"nets\":[{}]}}", nets.join(","))
    }
}

impl PourNetAudit {
    /// True if the net's pours were not split and lost no pin.
    pub fn is_clean(&self) -> bool {
        self.islands_after <= self.islands_before && self.lost_pins.is_empty()
    }

    fn to_json(&self) -> String {
        let cuts: Vec<String> = self.cuts.iter().map(PourCut::to_json).collect();
        format!(
            "{{\"net\":{},\"net_name\":{},\"areas\":[{}],\"islands_before\":{},\
             \"islands_after\":{},\"lost_pins\":[{}],\"cuts\":[{}]}}",
            self.net,
            json_string(&self.net_name),
            join_numbers(&self.areas),
            self.islands_before,
            self.islands_after,
            join_strings(&self.lost_pins),
            cuts.join(",")
        )
    }
}

impl PourCut {
    fn to_json(&self) -> String {
        format!(
            "{{\"area\":{},\"layer\":{},\"islands_before\":{},\"islands_after\":{},\
             \"by_nets\":[{}]}}",
            self.area,
            json_string(&self.layer),
            self.islands_before,
            self.islands_after,
            join_strings(&self.by_nets)
        )
    }
}

/// The pour-cut report of routing `before` into `after` (the same board
/// with routing added, so pins and areas keep their ids).
pub fn audit(before: &BasicBoard, after: &BasicBoard) -> PourAudit {
    let mut per_net: BTreeMap<i32, PourNetAudit> = BTreeMap::new();
    let mut attached: BTreeMap<i32, (BTreeSet<i32>, BTreeSet<i32>)> = BTreeMap::new();
    for id in filled_areas(before) {
        let Some(area) = before.item(id) else {
            continue;
        };
        let net = if area.net_count() > 0 {
            area.net_number(0)
        } else {
            0
        };
        let fill_before = conduction_fill(before, id);
        let fill_after = conduction_fill(after, id);
        let entry = per_net.entry(net).or_insert_with(|| PourNetAudit {
            net,
            net_name: net_name(before, net),
            areas: Vec::new(),
            islands_before: 0,
            islands_after: 0,
            lost_pins: Vec::new(),
            cuts: Vec::new(),
        });
        entry.areas.push(id.0);
        entry.islands_before += fill_before.len();
        entry.islands_after += fill_after.len();
        if fill_after.len() > fill_before.len() {
            entry.cuts.push(PourCut {
                area: id.0,
                layer: layer_name(before, area.first_layer()),
                islands_before: fill_before.len(),
                islands_after: fill_after.len(),
                by_nets: cutting_nets(before, after, area),
            });
        }
        let pins = attached.entry(net).or_default();
        pins.0.extend(attached_pins(before, &fill_before));
        pins.1.extend(attached_pins(after, &fill_after));
    }
    for (net, (pins_before, pins_after)) in attached {
        if let Some(entry) = per_net.get_mut(&net) {
            entry.lost_pins = pins_before
                .difference(&pins_after)
                .map(|id| pin_label(before, ItemId(*id)))
                .collect();
        }
    }
    PourAudit {
        nets: per_net.into_values().collect(),
    }
}

/// The filled conduction areas of `board`, ascending item id.
fn filled_areas(board: &BasicBoard) -> Vec<ItemId> {
    let mut ids = board.conduction_areas();
    ids.retain(|id| {
        board
            .item(*id)
            .and_then(Item::as_conduction_area)
            .is_some_and(|a| {
                matches!(
                    a.kind,
                    crate::board::items::AreaKind::Conduction {
                        is_filled: true,
                        ..
                    }
                )
            })
    });
    ids.sort_by_key(|id| id.0);
    ids
}

/// The pins among the islands' anchors, as raw ids.
fn attached_pins(board: &BasicBoard, fill: &[crate::drc::FillIsland]) -> Vec<i32> {
    fill.iter()
        .flat_map(|island| island.anchors.iter())
        .filter(|id| board.item(**id).is_some_and(Item::is_pin))
        .map(|id| id.0)
        .collect()
}

/// The names of the foreign nets whose traces in `after`, not in `before`,
/// overlap `area` (by area, not only at a border), ascending net number.
fn cutting_nets(before: &BasicBoard, after: &BasicBoard, area: &Item) -> Vec<String> {
    let area_tiles: Vec<TileShape> = (0..before.tile_shape_count(area))
        .filter_map(|i| before.tile_shape(area, i))
        .collect();
    let layer = area.first_layer();
    let mut nets = BTreeSet::new();
    for item in after.items() {
        if !item.is_trace() || item.first_layer() != layer || before.item(item.id()).is_some() {
            continue;
        }
        if item.shares_net(area) {
            continue;
        }
        let crosses = (0..after.tile_shape_count(item))
            .filter_map(|i| after.tile_shape(item, i))
            .any(|trace_tile| {
                area_tiles
                    .iter()
                    .any(|tile| trace_tile.intersection(tile).dimension() == 2)
            });
        if crosses {
            nets.extend(item.net_numbers().iter().copied().filter(|n| *n > 0));
        }
    }
    nets.into_iter().map(|n| net_name(before, n)).collect()
}

fn net_name(board: &BasicBoard, net: i32) -> String {
    board
        .rules
        .nets
        .get(net)
        .map_or_else(|| format!("net {net}"), |n| n.name.clone())
}

fn layer_name(board: &BasicBoard, layer: i32) -> String {
    board
        .layer_structure
        .layers
        .get(layer as usize)
        .map_or_else(|| format!("layer {layer}"), |l| l.name.clone())
}

/// `component-pin` for a pin, `#id` otherwise.
fn pin_label(board: &BasicBoard, id: ItemId) -> String {
    let Some(item) = board.item(id) else {
        return format!("#{}", id.0);
    };
    let component = board
        .components
        .get(ComponentId(item.component_id()))
        .map(|c| c.name.as_str());
    match (component, board.pin_name(item)) {
        (Some(component), Some(pin)) => format!("{component}-{pin}"),
        _ => format!("#{}", id.0),
    }
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn join_strings(items: &[String]) -> String {
    items
        .iter()
        .map(|s| json_string(s))
        .collect::<Vec<_>>()
        .join(",")
}

fn join_numbers(items: &[i32]) -> String {
    items
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
