//! K15.16: the board's copper zones as KiCad's connectivity reads them.
//!
//! KiCad's DRC joins items through a zone's fill (`ZONE::GetFilledPolysList`),
//! one polygon per island, not through its outline, and its zone filler
//! removes the islands no pad reaches as the zone's `island_removal_mode`
//! says. The DSN tree carries neither: a plane is the zone's outline
//! ([`crate::zones`]). The DRC report's KiCad counting reads them here, in
//! DSN coordinates (µm, y up, as [`map_pt`] maps the board):
//!
//! - **the fill**: each `(filled_polygon (layer L) [(island)] (pts ...))` of
//!   the zone ([`read_fills`]), one island per polygon (KiCad saves the
//!   fractured fill: every outline is one island with its holes joined in);
//! - **whether it is filled** ([`is_filled`], `ZONE::IsFilled`, and at least
//!   one `filled_polygon`: a zone marked filled without one, as a
//!   hand-written file has it, has no known fill);
//! - **the island removal** (`(fill ... (island_removal_mode M)
//!   (island_area_min A))`; `parseZONE`: 0 always, 1 never, 2 below the area
//!   in mm²; `ZONE_SETTINGS`'s default is 0, always);
//! - **the minimum thickness** (`(min_thickness T)`, K15.23): the router's
//!   stub cleanup judges a trace end on a pour by the fill it models, opened
//!   by this width as KiCad's filler opens it;
//! - **the clearance** (`(connect_pads ... (clearance C))`, K15.42): the
//!   router's pour target and contacts keep it from other nets' copper.
//!
//! Only the board's own zones are read, not the zones footprints own, and not
//! rule areas or zones without a net.

use freerouting_dsn::ast::Point;

use crate::layers::Layers;
use crate::nets::NetTable;
use crate::sexpr::Tree;
use crate::units::map_pt;
use crate::zones::{Zone, ZoneLayerMap, is_filled, layer_id_order, read_fills};

/// What KiCad's zone filler does with an island no pad reaches
/// (`ISLAND_REMOVAL_MODE`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IslandRemoval {
    /// `0`, `ALWAYS` (KiCad's default).
    Always,
    /// `1`, `NEVER`.
    Never,
    /// `2`, `AREA`: islands below this area, in µm², are removed.
    BelowArea(f64),
}

/// One copper zone of the board on one of its copper layers.
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneFill {
    /// The zone's net as the DSN tree names it (`"` written `''`).
    pub net: String,
    /// The DSN layer name.
    pub layer: String,
    /// `ZONE::IsFilled`.
    pub filled: bool,
    pub removal: IslandRemoval,
    /// The zone's outline, without a repeated first point.
    pub outline: Vec<Point>,
    /// The fill's islands on this layer, in file order, each without a
    /// repeated first point.
    pub islands: Vec<Vec<Point>>,
    /// K15.23: `(min_thickness T)` in µm, else `ZONE_SETTINGS`'s 0.25 mm: the
    /// filler's minimum-thickness opening removes every part of the fill a
    /// disc of this diameter does not fit in.
    pub min_thickness: f64,
    /// K15.42: the zone's own clearance in µm (`(connect_pads ... (clearance
    /// C))`, else 0.5 mm; `ZONE::GetLocalClearance`). Where no `.kicad_dru`
    /// rule matches, KiCad's filler keeps the larger of it and the net
    /// classes' clearance from other nets' copper (`DRC_ENGINE::EvalRules`).
    pub clearance: f64,
}

/// `ZONE_SETTINGS::m_ZoneMinThickness`'s default, 0.25 mm, in µm.
pub const DEFAULT_MIN_THICKNESS_UM: f64 = 250.0;

/// The zone's `(min_thickness T)` (mm in the file) in µm.
fn min_thickness(tree: &Tree, zone: &Zone) -> f64 {
    tree.node(zone.node)
        .child("min_thickness")
        .and_then(|n| n.f64(1))
        .filter(|mm| mm.is_finite() && *mm >= 0.0)
        .map_or(DEFAULT_MIN_THICKNESS_UM, |mm| mm * 1000.0)
}

/// The island removal of the zone at `node`'s `(fill ...)` list.
fn island_removal(tree: &Tree, zone: &Zone) -> IslandRemoval {
    let fill = tree.node(zone.node).child("fill");
    let value = |name: &str| fill.and_then(|f| f.child(name)).and_then(|n| n.f64(1));
    match value("island_removal_mode").map(|m| m as i64) {
        Some(1) => IslandRemoval::Never,
        Some(2) => {
            // mm² in the file; µm² here
            IslandRemoval::BelowArea(value("island_area_min").unwrap_or(0.0) * 1e6)
        }
        _ => IslandRemoval::Always,
    }
}

/// The fills of `zones` (the board's own, as parsed), per zone in file order
/// and per layer in KiCad's layer-id order.
pub fn zone_fills(
    tree: &Tree,
    zones: &[Zone],
    map: &ZoneLayerMap,
    layers: &Layers,
    nets: &NetTable,
) -> Vec<ZoneFill> {
    let count = layers.count();
    let mut out = Vec::new();
    for zone in zones.iter().filter(|z| !z.rule_area && z.net > 0) {
        let Some(net) = nets.name(zone.net).filter(|n| !n.is_empty()) else {
            continue;
        };
        let node = tree.node(zone.node);
        let order = layer_id_order(zone.layers, count);
        let filled = is_filled(node);
        let fills = if filled {
            read_fills(node, map, order.first().copied())
        } else {
            Vec::new()
        };
        // a zone marked filled without any `filled_polygon` was never filled
        // (a hand-written file): its fill is not known
        let filled = filled && !fills.is_empty();
        let removal = island_removal(tree, zone);
        let min_thickness = min_thickness(tree, zone);
        let to_dsn = |pts: &[crate::legacy::Pt]| -> Vec<Point> {
            pts.iter()
                .map(|p| map_pt(i64::from(p.0), i64::from(p.1)))
                .collect()
        };
        for p in order {
            out.push(ZoneFill {
                net: net.replace('"', "''"),
                layer: layers.name(p).to_owned(),
                filled,
                removal,
                outline: to_dsn(&zone.outline),
                islands: fills
                    .iter()
                    .filter(|(l, _)| *l == p)
                    .map(|(_, pts)| to_dsn(pts))
                    .collect(),
                min_thickness,
                clearance: f64::from(zone.clearance) / 1000.0,
            });
        }
    }
    out
}
