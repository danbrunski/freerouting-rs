//! Rule areas to keepouts (K06.2, K06.3).
//!
//! KiCad's DSN export picks the kind of a rule area from its tracks and vias
//! flags only (`SPECCTRA_DB::FromBOARD`, "zones flagged keepout areas become
//! keepout", `specctra_export.cpp` at tag `10.0.0`):
//!
//! ```text
//! vias && tracks -> keepout     vias -> via_keepout
//! tracks         -> wire_keepout    else -> keepout
//! ```
//!
//! so an area that bans only footprints, pads or copper pour, or a KiCad 9
//! placement area that bans nothing, becomes a full keepout and costs routing
//! room. This reader maps by flags instead:
//!
//! | `not_allowed` flags | emitted |
//! |---|---|
//! | tracks and vias | `keepout` |
//! | vias only | `via_keepout` |
//! | tracks only | `wire_keepout` (the board builds it as a keepout, as Java does; a warning says so) |
//! | footprints | also a `place_keepout` on the same layers |
//! | pads, copper pour, nothing | nothing, one warning |
//!
//! Per area the keepouts come per copper layer: the board's rule areas in
//! KiCad's layer-id order (F.Cu, B.Cu, In1.Cu ...), a footprint's in physical
//! order (`LSET::CuStack`). The outline is the first polygon and the others
//! are `(window ...)` holes.

use freerouting_dsn::ast::{Area, Keepout, KeepoutKind, LayerRef, Point, Shape};

use crate::error::Warning;
use crate::layers::Layers;
use crate::legacy::Pt;
use crate::zones::{RuleFlags, Zone};

/// What the reader made of one rule area (K06.7).
#[derive(Clone, Debug, PartialEq)]
pub struct RuleAreaReport {
    /// The reference of the footprint that owns the area.
    pub owner: Option<String>,
    pub name: Option<String>,
    /// Line of the zone in the file.
    pub line: u32,
    /// DSN names of the copper layers the area lives on, in the order the
    /// keepouts were emitted.
    pub layers: Vec<String>,
    pub flags: RuleFlags,
    /// The keepout kinds emitted per layer, as DSN keywords.
    pub emitted: Vec<&'static str>,
    /// Why nothing, or something else, was emitted.
    pub note: Option<String>,
}

fn closed(points: &[Pt], map: &dyn Fn(Pt) -> Point) -> Vec<Point> {
    let mut v: Vec<Point> = points.iter().map(|&p| map(p)).collect();
    if let Some(f) = v.first().copied() {
        v.push(f);
    }
    v
}

fn polygon(layer: &str, points: &[Pt], map: &dyn Fn(Pt) -> Point) -> Shape {
    Shape::Polygon {
        layer: LayerRef::Named(layer.to_owned()),
        aperture_width: 0.0,
        points: closed(points, map),
    }
}

/// The DSN kinds a rule area gets: the track/via kind (if any) and whether it
/// also gets a `place_keepout`.
pub fn kinds_of(flags: &RuleFlags) -> (Option<KeepoutKind>, bool) {
    let routing = match (flags.vias, flags.tracks) {
        (true, true) => Some(KeepoutKind::Keepout),
        (true, false) => Some(KeepoutKind::ViaKeepout),
        (false, true) => Some(KeepoutKind::WireKeepout),
        (false, false) => None,
    };
    (routing, flags.footprints)
}

/// Builds the keepouts of one rule area. `order` lists the physical positions
/// of its layers in emission order; `map` takes a point of the zone to the
/// frame of the tree (board frame, or relative to a footprint). The holes of
/// a footprint's area are written on the zone's first layer whichever layer the
/// keepout is on (`zone->GetLayer()` in `makeIMAGE`): `window_layer` is that
/// layer's position, `None` for a board area (holes on the keepout's layer).
#[allow(clippy::too_many_arguments)]
pub fn rule_area_keepouts(
    zone: &Zone,
    order: &[usize],
    layers: &Layers,
    owner: Option<&str>,
    window_layer: Option<usize>,
    map: &dyn Fn(Pt) -> Point,
    warnings: &mut Vec<Warning>,
    line: u32,
) -> (Vec<Keepout>, RuleAreaReport) {
    let label = {
        let who = match &zone.name {
            Some(n) if !n.is_empty() => format!("rule area '{n}'"),
            _ => format!("rule area at line {line}"),
        };
        match owner {
            Some(o) => format!("{who} of footprint {o}"),
            None => who,
        }
    };
    let (routing, place) = kinds_of(&zone.flags);
    let layer_names: Vec<String> = order.iter().map(|&p| layers.name(p).to_owned()).collect();
    let mut note = None;
    let mut warn = |message: String| {
        note = Some(message.clone());
        warnings.push(Warning { line, message });
    };
    match routing {
        Some(KeepoutKind::WireKeepout) => warn(format!(
            "{label} bans tracks but not vias: built as wire_keepout, which the board treats as a \
             keepout that also blocks vias (as Java does)"
        )),
        None if !place => {
            let what = if zone.flags.copper_pour {
                "bans only copper pour: the plane on its layers is not cut"
            } else if zone.flags.pads {
                "bans only pads"
            } else {
                "bans nothing that the router can use"
            };
            warn(format!("{label} {what}; no keepout emitted"));
        }
        _ => {}
    }
    let mut keepouts = Vec::new();
    let mut emitted: Vec<&'static str> = Vec::new();
    for &p in order {
        let name = layers.name(p);
        let area = |kind: KeepoutKind| Keepout {
            kind,
            area: Area {
                name: None,
                shape: polygon(name, &zone.outline, map),
                windows: zone
                    .holes
                    .iter()
                    .map(|h| polygon(window_layer.map_or(name, |w| layers.name(w)), h, map))
                    .collect(),
                clearance_class: None,
            },
        };
        if let Some(kind) = routing {
            keepouts.push(area(kind));
        }
        if place {
            keepouts.push(area(KeepoutKind::PlaceKeepout));
        }
    }
    if let Some(kind) = routing {
        emitted.push(kind.keyword());
    }
    if place {
        emitted.push(KeepoutKind::PlaceKeepout.keyword());
    }
    let report = RuleAreaReport {
        owner: owner.map(str::to_owned),
        name: zone.name.clone(),
        line,
        layers: layer_names,
        flags: zone.flags,
        emitted,
        note,
    };
    (keepouts, report)
}
