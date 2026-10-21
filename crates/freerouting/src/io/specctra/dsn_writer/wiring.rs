//! The wiring scope: traces, vias and the conduction areas on signal layers.
//!
//! Port of `Wiring.writeScope`, `writeWireScope`, `writeViaScope`, `writeConductionAreaScope`,
//! `writeNet` and `writeFixedState`.

use freerouting_dsn::ast::{
    LayerRef, NetId, Point as DsnPoint, Shape as DsnShape, Via, Wire, Wiring, WiringItem,
};

use super::{Cx, fixed_state_type};
use crate::board::items::{AreaKind, Item, ItemKind};

/// A net id as the wiring writes it: name and subnet number.
fn net_id(cx: &Cx, item: &Item) -> Option<NetId> {
    let net = cx.board.rules.nets.get(*item.net_numbers().first()?)?;
    Some(NetId {
        name: net.name.clone(),
        subnet: Some(net.subnet_number),
    })
}

/// The wires first, then the vias, then the conduction areas, each in item list order.
pub(super) fn wiring(cx: &Cx) -> Wiring {
    let mut wires = Vec::new();
    let mut vias = Vec::new();
    let mut areas = Vec::new();
    for item in cx.items.iter().copied() {
        match &item.kind {
            ItemKind::Trace(_) => wires.extend(wire(cx, item).map(WiringItem::Wire)),
            ItemKind::Via(_) => vias.extend(via(cx, item).map(WiringItem::Via)),
            ItemKind::Area(_) => areas.extend(conduction_area(cx, item).map(WiringItem::Wire)),
            _ => {}
        }
    }
    wires.extend(vias);
    wires.extend(areas);
    Wiring { items: wires }
}

/// `writeWireScope`: a `polyline_path` of the trace's lines. A trace without a net is left
/// out, as in Java.
fn wire(cx: &Cx, item: &Item) -> Option<Wire> {
    let ItemKind::Trace(trace) = &item.kind else {
        return None;
    };
    let net = net_id(cx, item)?;
    let t = &cx.transform;
    let mut points = Vec::with_capacity(2 * trace.polyline.lines().len());
    for line in trace.polyline.lines() {
        for end in [line.a(), line.b()] {
            let [x, y] = t.board_to_dsn_point(end.to_float());
            points.push(DsnPoint { x, y });
        }
    }
    Some(Wire {
        shapes: vec![DsnShape::PolylinePath {
            layer: LayerRef::Named(cx.layer_name(trace.layer)),
            width: t.board_to_dsn(f64::from(trace.half_width.wrapping_mul(2))),
            points,
        }],
        windows: Vec::new(),
        net: Some(net),
        clearance_class: Some(cx.class_name(item.clearance_class())),
        wire_type: fixed_state_type(item.fixed_state()),
        attr: None,
    })
}

/// `writeViaScope`.
fn via(cx: &Cx, item: &Item) -> Option<Via> {
    let ItemKind::Via(via) = &item.kind else {
        return None;
    };
    let padstack = cx.board.library.padstacks.get(via.drill.padstack)?;
    let [x, y] = cx.transform.board_to_dsn_point(via.drill.center.to_float());
    Some(Via {
        padstack: padstack.name.clone(),
        at: DsnPoint { x, y },
        net: net_id(cx, item),
        clearance_class: Some(cx.class_name(item.clearance_class())),
        via_type: fixed_state_type(item.fixed_state()),
    })
}

/// `writeConductionAreaScope`: a conduction area on a signal layer with exactly one net.
/// (The fixed state is not written by Java.)
fn conduction_area(cx: &Cx, item: &Item) -> Option<Wire> {
    let ItemKind::Area(area) = &item.kind else {
        return None;
    };
    if !matches!(area.kind, AreaKind::Conduction { .. }) || item.net_count() != 1 {
        return None;
    }
    let layer = usize::try_from(area.layer())
        .ok()
        .and_then(|l| cx.board.layer_structure.layers.get(l))?;
    if !layer.is_signal || super::in_plane_scope(cx, item) {
        return None;
    }
    let net = net_id(cx, item)?;
    let (shape, windows) = cx.area_shapes(area.area(), LayerRef::Named(layer.name.clone()), false);
    Some(Wire {
        shapes: vec![shape?],
        windows,
        net: Some(net),
        clearance_class: Some(cx.class_name(item.clearance_class())),
        wire_type: fixed_state_type(item.fixed_state()),
        // Q22.4: a net's fixed copper shape stays one when read back
        attr: area
            .is_fixed_obstacle()
            .then(|| freerouting_dsn::ast::WIRE_ATTR_OBSTACLE.to_owned()),
    })
}
