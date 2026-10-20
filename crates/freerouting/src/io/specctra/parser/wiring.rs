//! The wiring scope: traces, vias and conduction areas.
//!
//! Port of `io.specctra.parser.Wiring.readScope`.

use std::collections::BTreeMap;

use freerouting_dsn::ast;
use freerouting_geometry::jmath::{d2i, round_to_i32};
use freerouting_geometry::{FloatPoint, IntBox, IntPoint, Line, Point, Polygon, Polyline};

use super::ReadContext;
use super::library::find_padstack;
use super::shape::{DsnShape, DsnShapeKind, transform_area_to_board};
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::board::structure::FixedState;
use crate::library::PadstackId;
use crate::rules::{ItemClass, Net, NetClassId, Nets};
use crate::util::CaseInsensitive;

/// The board's net numbers by name (any case, Java `equalsIgnoreCase`), each
/// list in net-number order.
///
/// Java's `Nets.getNets` scans every net for each wire, via and pin, which is
/// quadratic on big boards (`jetson-agx-thor-baseboard`: 25,615 nets, 24,319
/// wires and vias and 7,303 pins took about 0.4 s of a 1.3 s build, K13.4).
/// The placement and wiring scopes add no nets, so [`read_wiring`] and
/// `insert_components` build this once, and the lookups return what the
/// scans return, in the same order.
pub(super) struct NetNames(BTreeMap<CaseInsensitive, Vec<i32>>);

impl NetNames {
    pub(super) fn new(nets: &Nets) -> Self {
        let mut map: BTreeMap<CaseInsensitive, Vec<i32>> = BTreeMap::new();
        for net in nets.iter() {
            map.entry(CaseInsensitive(net.name.clone()))
                .or_default()
                .push(net.net_number);
        }
        NetNames(map)
    }

    fn numbers(&self, name: &str) -> &[i32] {
        self.0
            .get(&CaseInsensitive(name.to_owned()))
            .map_or(&[], Vec::as_slice)
    }

    /// Java `Nets.getNet(name, subnet)`: the first net with both, as
    /// [`Nets::get_by_name`] finds it.
    pub(super) fn get<'a>(&self, nets: &'a Nets, name: &str, subnet: i32) -> Option<&'a Net> {
        self.numbers(name)
            .iter()
            .filter_map(|&number| nets.get(number))
            .find(|n| n.subnet_number == subnet)
    }
}

/// The board nets of a DSN net id: the subnet if one is given, else all
/// subnets of the name (Java `Wiring.getSubnets`).
fn subnets(
    board: &RoutingBoard,
    names: &NetNames,
    net: Option<&ast::NetId>,
) -> Vec<(i32, NetClassId)> {
    let Some(net) = net else {
        return Vec::new();
    };
    let subnet = net.subnet.unwrap_or(0);
    let nets = &board.rules.nets;
    if subnet > 0 {
        names
            .get(nets, &net.name, subnet)
            .map(|n| vec![(n.net_number, n.net_class())])
            .unwrap_or_default()
    } else {
        names
            .numbers(&net.name)
            .iter()
            .filter_map(|&number| nets.get(number))
            .map(|n| (n.net_number, n.net_class()))
            .collect()
    }
}

/// Java `IntBox.contains(FloatPoint)`.
fn box_contains(b: &IntBox, p: FloatPoint) -> bool {
    p.x >= f64::from(b.ll.x)
        && p.y >= f64::from(b.ll.y)
        && p.x <= f64::from(b.ur.x)
        && p.y <= f64::from(b.ur.y)
}

/// Reads the wiring scope. Returns false where Java's reader does (a via
/// with an unknown padstack).
pub(crate) fn read_wiring(ctx: &mut ReadContext, wiring: &ast::Wiring) -> bool {
    let names = NetNames::new(&ctx.board().rules.nets);
    for item in &wiring.items {
        match item {
            ast::WiringItem::Wire(wire) => read_wire(ctx, &names, wire),
            ast::WiringItem::Via(via) => {
                if !read_via(ctx, &names, via) {
                    return false;
                }
            }
        }
    }
    // Java catches exceptions of the normalization and turns them into the
    // warning "Wiring: normalization of traces failed"; the port has no
    // such failure.
    ctx.board().normalize_all_traces();
    true
}

/// Java `Wiring.readWireScope`.
fn read_wire(ctx: &mut ReadContext, names: &NetNames, wire: &ast::Wire) {
    let structure = ctx.layer_structure.clone();
    let mut path: Option<DsnShape> = None;
    let mut border: Option<DsnShape> = None;
    for shape in &wire.shapes {
        let resolved = DsnShape::resolve(shape, Some(&structure));
        match shape {
            ast::Shape::Path { .. } | ast::Shape::PolylinePath { .. } => path = resolved,
            _ => border = resolved,
        }
    }
    let holes: Vec<DsnShape> = wire
        .windows
        .iter()
        .filter_map(|w| DsnShape::resolve(w, Some(&structure)))
        .collect();
    if let Some(net) = &wire.net {
        ctx.scope_identifier = net.name.clone();
    }
    let fixed = wire
        .wire_type
        .as_ref()
        .map_or(FixedState::Unfixed, |t| t.fixed_state().into());
    if path.is_none() && border.is_none() {
        let message = format!("Wiring: wire has no shape at '{}'", ctx.scope_identifier);
        ctx.warn(message);
        return;
    }
    let transform = ctx.coordinate_transform;
    let scope = ctx.scope_identifier.clone();
    let board = ctx.board.as_mut().expect("board");
    let default_class = board.rules.default_net_class();
    let found = subnets(board, names, wire.net.as_ref());
    let net_numbers: Vec<i32> = found.iter().map(|(n, _)| *n).collect();
    let net_class = found.last().map_or(default_class, |(_, c)| *c);
    let mut clearance_class = -1;
    if let Some(name) = &wire.clearance_class {
        clearance_class = board.rules.clearance_matrix.get_no(name);
    }
    let (layer, half_width) = match &path {
        Some(p) => (
            p.layer.clone(),
            round_to_i32(transform.dsn_to_board(p.width / 2.0)),
        ),
        None => (border.as_ref().expect("border").layer.clone(), 0),
    };
    if layer.no < 0 || layer.no >= board.layer_count() as i32 {
        let message = format!(
            "Wiring: wire ignored \u{2014} unknown layer '{}' at '{scope}'",
            layer.name
        );
        ctx.warn(message);
        return;
    }
    let default_item_class = |board: &RoutingBoard, item_class: ItemClass| {
        board
            .rules
            .net_classes
            .get(net_class)
            .default_item_clearance_classes
            .get(item_class)
    };
    let bounding_box = board.bounding_box();
    let result: Option<ItemId> = if let Some(border) = border {
        if clearance_class < 0 {
            clearance_class = default_item_class(board, ItemClass::Area);
        }
        let mut shapes = vec![border];
        shapes.extend(holes);
        match transform_area_to_board(&shapes, &transform) {
            Some(area) => Some(board.insert_conduction_area(
                area,
                layer.no,
                &net_numbers,
                clearance_class,
                false,
                fixed,
            )),
            None => {
                board
                    .log
                    .push("BasicBoard.insert_conduction_area: area is null".into());
                None
            }
        }
    } else {
        let path = path.expect("path");
        if clearance_class < 0 {
            clearance_class = default_item_class(board, ItemClass::Trace);
        }
        match path.kind {
            DsnShapeKind::PolygonPath => {
                let mut corners: Vec<Point> = Vec::with_capacity(path.coor.len() / 2);
                for p in path.coor.chunks_exact(2) {
                    let corner = transform.dsn_to_board_point(p[0], p[1]);
                    if !box_contains(&bounding_box, corner) {
                        let message = format!(
                            "Wiring: wire corner ({},{}) is outside board bounds at '{scope}'",
                            d2i(p[0]),
                            d2i(p[1])
                        );
                        ctx.warn(message);
                        return;
                    }
                    corners.push(Point::Int(corner.round()));
                }
                let polygon = Polygon::new(&corners);
                let polygon_corners = polygon.corner_array();
                let has_distinct_corner = polygon_corners
                    .iter()
                    .skip(1)
                    .any(|c| *c != polygon_corners[0]);
                if polygon_corners.len() < 2 || !has_distinct_corner {
                    let message = format!(
                        "Wiring: degenerate wire trace skipped (all {} corners are identical \
                         \u{2014} zero-length trace) on layer '{}'. This is likely a DSN export \
                         issue in your EDA tool.",
                        polygon_corners.len(),
                        layer.name
                    );
                    ctx.warnings.push(message.clone());
                    ctx.log(message);
                    return;
                }
                board.insert_trace_without_cleaning(
                    Polyline::from_polygon(&polygon),
                    layer.no,
                    half_width,
                    &net_numbers,
                    clearance_class,
                    fixed,
                )
            }
            _ => {
                let lines: Vec<Line> = path
                    .coor
                    .chunks_exact(4)
                    .map(|c| {
                        let a = transform.dsn_to_board_point(c[0], c[1]).round();
                        let b = transform.dsn_to_board_point(c[2], c[3]).round();
                        Line::new(a, b)
                    })
                    .collect();
                board.insert_trace_without_cleaning(
                    Polyline::new(&lines),
                    layer.no,
                    half_width,
                    &net_numbers,
                    clearance_class,
                    fixed,
                )
            }
        }
    };
    if let Some(id) = result
        && board.item(id).is_some_and(|i| i.net_count() == 0)
    {
        try_correct_net(board, id);
    }
}

/// Java `Wiring.tryCorrectNet`: a trace without net (a Mentor turret
/// trace) gets the net of an item it touches at an end.
fn try_correct_net(board: &mut RoutingBoard, id: ItemId) {
    let Some(trace) = board.item(id).and_then(|i| i.as_trace()) else {
        return;
    };
    let (first, last) = (trace.first_corner(), trace.last_corner());
    let mut contacts = board.trace_normal_contacts(id, &first, true);
    contacts.extend(board.trace_normal_contacts(id, &last, true));
    let corrected = contacts
        .iter()
        .filter_map(|c| board.item(*c))
        .find(|item| item.net_count() == 1)
        .map(|item| item.net_number(0));
    if let Some(net) = corrected
        && net != 0
    {
        board.assign_net_no(id, net);
    }
}

/// Java `Wiring.viaExists`.
fn via_exists(
    board: &RoutingBoard,
    location: IntPoint,
    padstack: PadstackId,
    nets: &[i32],
) -> bool {
    let Some(padstack) = board.library.padstacks.get(padstack) else {
        return false;
    };
    let (from, to) = (padstack.from_layer(), padstack.to_layer());
    board
        .pick_items(&Point::Int(location), from)
        .into_iter()
        .filter_map(|id| board.item(id))
        .filter(|item| item.is_via())
        .any(|via| {
            via.nets_equal_no(nets)
                && via.drill().is_some_and(|d| d.center == location)
                && via.first_layer() == from
                && via.last_layer() == to
        })
}

/// Java `Wiring.readViaScope`. False if the padstack is unknown.
fn read_via(ctx: &mut ReadContext, names: &NetNames, via: &ast::Via) -> bool {
    ctx.scope_identifier = via.padstack.clone();
    if let Some(net) = &via.net {
        ctx.scope_identifier = net.name.clone();
    }
    let fixed = via
        .via_type
        .as_ref()
        .map_or(FixedState::Unfixed, |t| t.fixed_state().into());
    let scope = ctx.scope_identifier.clone();
    let transform = ctx.coordinate_transform;
    let via_at_smd_allowed = ctx.via_at_smd_allowed;
    let board = ctx.board.as_mut().expect("board");
    let Some(padstack) = find_padstack(&board.library.padstacks, &via.padstack).cloned() else {
        let message = format!(
            "Wiring: via padstack '{}' not found at '{scope}'",
            via.padstack
        );
        ctx.warn(message);
        return false;
    };
    let found = subnets(board, names, via.net.as_ref());
    let mut messages = Vec::new();
    if let Some(net) = &via.net
        && found.is_empty()
    {
        messages.push(format!(
            "Wiring: via net '{}' not found at '{scope}'",
            net.name
        ));
    }
    let default_class = board.rules.default_net_class();
    let net_numbers: Vec<i32> = found.iter().map(|(n, _)| *n).collect();
    let net_class = found.last().map_or(default_class, |(_, c)| *c);
    let mut clearance_class = -1;
    if let Some(name) = &via.clearance_class {
        clearance_class = board.rules.clearance_matrix.get_no(name);
    }
    if clearance_class < 0 {
        clearance_class = board
            .rules
            .net_classes
            .get(net_class)
            .default_item_clearance_classes
            .get(ItemClass::Via);
    }
    let location = transform.dsn_to_board_point(via.at.x, via.at.y).round();
    if via_exists(board, location, padstack.id, &net_numbers) {
        messages.push(format!(
            "Wiring: duplicate via skipped at ({}, {})",
            location.x, location.y
        ));
    } else {
        let attach_allowed = via_at_smd_allowed && padstack.attach_allowed;
        board.insert_via(
            padstack.id,
            location,
            &net_numbers,
            clearance_class,
            fixed,
            attach_allowed,
        );
        if padstack.has_auxiliary_shapes() {
            messages.push(format!(
                "Wiring: Via at ({}, {}) uses non-convex padstack '{}'. Non-convex geometry on \
                 vias is not routable and auxiliary companion tiles are omitted.",
                location.x, location.y, padstack.name
            ));
        }
    }
    for message in messages {
        ctx.warn(message);
    }
    true
}

#[cfg(test)]
mod net_names_tests {
    use super::*;

    /// The index finds what `Nets::get_all_by_name` and `Nets::get_by_name`
    /// find, in the same order, for names that differ only in case
    /// (including characters whose case mapping is not one to one).
    #[test]
    fn net_names_find_what_the_scans_find() {
        let mut nets = Nets::default();
        let names = [
            ("GND", 1),
            ("gnd", 2),
            ("Gnd", 1),
            ("VCC", 1),
            ("ß", 1),
            ("SS", 1),
            ("İ", 1),
            ("i", 1),
            ("I", 3),
            ("ı", 1),
            ("Σ", 1),
            ("ς", 2),
            ("σ", 3),
            ("ǅ", 1),
            ("ǆ", 2),
            ("Ǆ", 3),
        ];
        for (name, subnet) in names {
            nets.add(name, subnet, false, NetClassId(0));
        }
        let index = NetNames::new(&nets);
        let queries = [
            "gnd", "GND", "vcc", "ß", "ss", "ẞ", "i", "İ", "ı", "I", "σ", "ς", "Σ", "ǅ", "ǆ",
            "none", "",
        ];
        for query in queries {
            let scanned: Vec<i32> = nets
                .get_all_by_name(query)
                .iter()
                .map(|n| n.net_number)
                .collect();
            assert_eq!(index.numbers(query), scanned.as_slice(), "{query}");
            for subnet in 0..4 {
                assert_eq!(
                    index.get(&nets, query, subnet).map(|n| n.net_number),
                    nets.get_by_name(query, subnet).map(|n| n.net_number),
                    "{query} subnet {subnet}"
                );
            }
        }
    }
}
