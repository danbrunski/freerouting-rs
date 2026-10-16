//! Writing the routing result of a board as a Specctra session file.
//!
//! Port of `io.specctra.SesWriter` (at `cda540f`). Content and layout are separate:
//! [`session_from_board`] decides what goes into the file and builds a
//! [`Session`] syntax tree; [`write_ses`] and [`write_ses_to`] write it either in the exact
//! text layout of Java's `IndentFileWriter` ([`SesStyle::Java`], the default, see
//! [`freerouting_dsn::write::java_style`]) or in the plain style of
//! [`Session::to_ses`]. In Java style the output is byte for byte what Java writes for the same
//! board, so every Java reference session is an oracle and KiCad's importer and the plugin see
//! the same text whichever engine routed.
//!
//! ```text
//! (session "name"                       the design name, ".dsn" replaced by ".ses"
//!   (base_design "name")
//!   (placement (resolution um 10) (component <package> (place <name> x y side rot))...)
//!   (was_is)                            pin swaps (none unless a pin's net was swapped)
//!   (routes (resolution) (parser host_cad host_version ...)
//!     (library_out (padstack ...(attach off)))   the via padstacks, each name once
//!     (network_out (net <name> (wire (path ...)) (via ...) (wire (polygon ...))))))
//! ```
//!
//! All coordinates are in resolution units: the transform is `scale =
//! dsn_to_board(1) / resolution` with offset 0.
//!
//! # Differences from Java
//!
//! * **Speed.** Java scans every board item once per component (`writeComponents`) and once per
//!   net (`getConnectableItems`). One pass collects the components that still own an item, one
//!   pass buckets the connectable items by net, both in item-list order. The output is the same.
//! * **An unplaced component** is written as `(place name)`; Java throws a
//!   `NullPointerException` there.
//! * **Endpoint snapping** ([`snapped_endpoint`]) is ported exactly. It cannot fire at the
//!   pin: `Trace.getNormalContacts` admits a drill item only when the trace corner equals its
//!   centre, and the snap then returns `None` because the distance 0 is at most 0.5. The test
//!   `endpoint_snaps_never_fire` (tests/ses_round_trip.rs) counts the snaps on every fixture.
//! * The outer-quote fix of `java_ident`, see [`freerouting_dsn::write::java_style`].
//!
//! # Java behaviour kept
//!
//! Java's `SesReader` divides session coordinates by `dsn_to_board(1) / resolution`, and
//! `SesWriter` divides board coordinates by the same factor, so the two are inverse only where
//! the factor is 1. It is 1 on every fixture but `Issue676-ch32v-tx118s` (0.1), whose written
//! session reads back 100 times too large. The port keeps both sides, because the written text
//! must be Java's (`ses_round_trip.rs` lists the board as an expected mismatch).
//!
//! Via padstacks that a rules file adds to the library (`-dr`, adjacent `<design>.rules`) are
//! written once the rules reader (C04) puts them there.

use std::collections::{BTreeSet, HashSet};
use std::io::{self, Write};

use freerouting_dsn::Session;
use freerouting_dsn::ast::{
    self as dsn, ComponentPlacement, LayerRef, Padstack as DsnPadstack, ParserInfo, PinRef,
    PinSwap, Place, PlaceLocation, Placement, Point as DsnPoint, Resolution, Routes, SessionNet,
    Shape as DsnShape, Side, Via as DsnVia, Wire, WireType, WiringItem,
};
use freerouting_dsn::write::java_style::write_session_java_to;
use freerouting_geometry::jmath::round_to_i32;
use freerouting_geometry::{
    Area, FloatPoint, IntBox, PolylineShape, PolylineShapeOps, Shape, TileShape,
};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::board::structure::{ComponentId, FixedState, Unit};
use crate::io::CoordinateTransform;
use crate::library::{PackageId, Padstack};

/// The text layout of a written session file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SesStyle {
    /// Java's layout, byte for byte (`IndentFileWriter`): `Double.toString` numbers including
    /// exponent form from 10^7, `(routes ` with a trailing space, two spaces of indent and no
    /// newline at the end of the file.
    #[default]
    Java,
    /// The plain layout of [`Session::to_ses`]: no exponents, no trailing spaces.
    Plain,
}

/// The session file of `board` as text, in `style`. `design_name` is the name of the design
/// file (`board.dsn`); the session is named after it with `.dsn` replaced by `.ses`.
pub fn write_ses(board: &BasicBoard, design_name: &str, style: SesStyle) -> String {
    let mut bytes = Vec::new();
    // Writing to a Vec cannot fail.
    let _ = write_ses_to(board, design_name, style, &mut bytes);
    String::from_utf8(bytes).unwrap_or_default()
}

/// Writes the session file of `board` to `out`. The output is the same as [`write_ses`]'s;
/// `out` is flushed and not closed. Wrap a file in a `BufWriter`.
pub fn write_ses_to<W: Write>(
    board: &BasicBoard,
    design_name: &str,
    style: SesStyle,
    mut out: W,
) -> io::Result<()> {
    let session = session_from_board(board, design_name);
    match style {
        SesStyle::Java => write_session_java_to(&session, string_quote(board), out),
        SesStyle::Plain => {
            out.write_all(session.to_ses().as_bytes())?;
            out.flush()
        }
    }
}

/// The string quote of the board's DSN (`"` unless the file said otherwise).
fn string_quote(board: &BasicBoard) -> &str {
    board
        .communication
        .specctra_parser_info
        .as_ref()
        .map_or("\"", |info| info.string_quote.as_str())
}

/// The content of the session file of `board`: Java's `SesWriter.write` without the layout.
pub fn session_from_board(board: &BasicBoard, design_name: &str) -> Session {
    let session_name = design_name.replace(".dsn", ".ses");
    let communication = &board.communication;
    let scale =
        communication.coordinate_transform.dsn_to_board(1.0) / f64::from(communication.resolution);
    let transform = CoordinateTransform::new(scale, 0.0, 0.0);
    let resolution = Resolution {
        unit: dsn_unit(communication.unit),
        value: communication.resolution,
    };
    Session {
        name: session_name,
        base_design: Some(design_name.to_string()),
        placement: Some(placement(board, &transform, resolution)),
        was_is: Some(was_is(board)),
        routes: Some(Routes {
            resolution: Some(resolution),
            parser: Some(parser_info(board)),
            library_out: library_out(board, &transform),
            network_out: network_out(board, &transform),
        }),
    }
}

pub(crate) fn dsn_unit(unit: Unit) -> dsn::Unit {
    match unit {
        Unit::Mil => dsn::Unit::Mil,
        Unit::Inch => dsn::Unit::Inch,
        Unit::Mm => dsn::Unit::Mm,
        Unit::Um => dsn::Unit::Um,
    }
}

// ---------------------------------------------------------------------------
// Placement and was_is
// ---------------------------------------------------------------------------

/// `writePlacement`: for each library package in order, the components that use it and still
/// own an item.
fn placement(
    board: &BasicBoard,
    transform: &CoordinateTransform,
    resolution: Resolution,
) -> Placement {
    // One pass: the components that own an item (Java scans the item list per component).
    let owners: HashSet<i32> = board.items().map(Item::component_id).collect();
    // Components by package, in component order.
    let mut by_package: Vec<Vec<ComponentId>> =
        vec![Vec::new(); board.library.packages.count() + 1];
    for component in board.components.iter() {
        let PackageId(package) = component.package();
        if owners.contains(&component.id.0) && package < by_package.len() {
            by_package[package].push(component.id);
        }
    }
    let kicad = board.communication.host_cad_is_kicad();
    let mut components = Vec::new();
    for package in board.library.packages.iter() {
        let places: Vec<Place> = by_package[package.id.0]
            .iter()
            .filter_map(|id| board.components.get(*id))
            .map(|component| {
                let location = component.exact_location().map(|at| {
                    let [x, y] = transform.board_to_dsn_point(at);
                    PlaceLocation {
                        at: DsnPoint { x, y },
                        side: if component.placed_on_front() {
                            Side::Front
                        } else {
                            Side::Back
                        },
                        rotation: component.rotation_in_degree(),
                    }
                });
                Place {
                    name: component.name.clone(),
                    location,
                    position_locked: component.position_fixed && !kicad,
                    pins: Vec::new(),
                    keepouts: Vec::new(),
                    via_keepouts: Vec::new(),
                    place_keepouts: Vec::new(),
                    part_number: None,
                }
            })
            .collect();
        if !places.is_empty() {
            components.push(ComponentPlacement {
                image: package.name.clone(),
                places,
            });
        }
    }
    Placement {
        resolution: Some(resolution),
        flip_style_rotate_first: None,
        components,
    }
}

/// `writeWasIs`: the pins whose net was swapped to another pin.
fn was_is(board: &BasicBoard) -> Vec<PinSwap> {
    let pin_ref = |id: ItemId| -> Option<PinRef> {
        let item = board.item(id)?;
        let component = board.components.get(ComponentId(item.component_id()))?;
        Some(PinRef {
            component: component.name.clone(),
            pin: board.pin_name(item)?.to_string(),
        })
    };
    let mut swaps = Vec::new();
    for id in board.pins() {
        let Some(pin) = board.item(id).and_then(Item::as_pin) else {
            continue;
        };
        if pin.changed_to == id {
            continue;
        }
        if let (Some(was), Some(is)) = (pin_ref(id), pin_ref(pin.changed_to)) {
            swaps.push(PinSwap { was, is });
        }
    }
    swaps
}

/// The reduced `Parser.writeScope`: host, constants and the write resolution (its first
/// character only, like Java).
fn parser_info(board: &BasicBoard) -> ParserInfo {
    let Some(info) = &board.communication.specctra_parser_info else {
        return ParserInfo::default();
    };
    ParserInfo {
        string_quote: None,
        space_in_quoted_tokens: None,
        host_cad: info.host_cad.clone(),
        host_version: info.host_version.clone(),
        constants: info.constants.clone(),
        write_resolution: info
            .write_resolution
            .as_ref()
            .map(|w| dsn::WriteResolution {
                name: w.char_name.chars().take(1).collect(),
                value: w.positive_int,
            }),
        generated_by_freerouting: false,
    }
}

// ---------------------------------------------------------------------------
// library_out
// ---------------------------------------------------------------------------

/// `writeLibrary`: the via padstacks in library order, each name once.
fn library_out(board: &BasicBoard, transform: &CoordinateTransform) -> Vec<DsnPadstack> {
    let mut written: HashSet<&str> = HashSet::new();
    let mut result = Vec::new();
    for no in 0..board.library.via_padstack_count() {
        let Some(padstack) = board.library.get_via_padstack(no) else {
            continue;
        };
        if !written.insert(padstack.name.as_str()) {
            continue;
        }
        if let Some(converted) = padstack_out(board, padstack, transform) {
            result.push(converted);
        }
    }
    result
}

/// `writePadstack`: the shapes from the first to the last layer that has one, relative
/// coordinates rounded to integers, and `(attach off)` when vias may not be attached.
fn padstack_out(
    board: &BasicBoard,
    padstack: &Padstack,
    transform: &CoordinateTransform,
) -> Option<DsnPadstack> {
    let layer_count = board.layer_count() as i32;
    let first = (0..layer_count).find(|l| padstack.get_shape(*l).is_some())?;
    let last = (0..layer_count)
        .rev()
        .find(|l| padstack.get_shape(*l).is_some())?;
    let mut shapes = Vec::new();
    for layer in first..=last {
        let Some(shape) = padstack.get_shape(layer) else {
            continue;
        };
        let name = &board.layer_structure.layers[layer as usize].name;
        let layer_ref = LayerRef::Named(name.clone());
        if let Some(converted) = board_shape_to_dsn(&shape.to_shape(), layer_ref, transform, true) {
            shapes.push(round_shape(converted));
        }
    }
    Some(DsnPadstack {
        name: padstack.name.clone(),
        shapes,
        attach: (!padstack.attach_allowed).then_some(false),
        absolute: None,
    })
}

// ---------------------------------------------------------------------------
// Shapes
// ---------------------------------------------------------------------------

/// `CoordinateTransform.boardToDsn(Shape, Layer)` and, with `relative`, `boardToDsnRel`:
/// a box becomes a rectangle, any other polyline shape a polygon of its corners, a circle a
/// circle. The coordinates are not rounded.
pub(crate) fn board_shape_to_dsn(
    shape: &Shape,
    layer: LayerRef,
    transform: &CoordinateTransform,
    relative: bool,
) -> Option<DsnShape> {
    let point = |p: FloatPoint| {
        let [x, y] = if relative {
            transform.board_to_dsn_rel(p)
        } else {
            transform.board_to_dsn_point(p)
        };
        DsnPoint { x, y }
    };
    match shape {
        Shape::Polyline(PolylineShape::Tile(TileShape::IntBox(IntBox { ll, ur }))) => {
            let ll = point(ll.to_float());
            let ur = point(ur.to_float());
            Some(DsnShape::Rect {
                layer,
                x1: ll.x,
                y1: ll.y,
                x2: ur.x,
                y2: ur.y,
            })
        }
        Shape::Polyline(polyline) => Some(DsnShape::Polygon {
            layer,
            aperture_width: 0.0,
            points: polyline
                .corner_approx_arr()
                .into_iter()
                .map(point)
                .collect(),
        }),
        Shape::Circle(circle) => Some(DsnShape::Circle {
            layer,
            diameter: 2.0 * transform.board_to_dsn(f64::from(circle.radius)),
            center: point(circle.center.to_float()),
        }),
    }
}

/// The `(int) Math.round` of each coordinate, which Java's `writeScopeInt` applies.
fn round_shape(shape: DsnShape) -> DsnShape {
    let r = |v: f64| f64::from(round_to_i32(v));
    let rp = |p: DsnPoint| DsnPoint {
        x: r(p.x),
        y: r(p.y),
    };
    match shape {
        DsnShape::Rect {
            layer,
            x1,
            y1,
            x2,
            y2,
        } => DsnShape::Rect {
            layer,
            x1: r(x1),
            y1: r(y1),
            x2: r(x2),
            y2: r(y2),
        },
        DsnShape::Circle {
            layer,
            diameter,
            center,
        } => DsnShape::Circle {
            layer,
            diameter: r(diameter),
            center: rp(center),
        },
        DsnShape::Polygon {
            layer,
            aperture_width,
            points,
        } => DsnShape::Polygon {
            layer,
            aperture_width,
            points: points.into_iter().map(rp).collect(),
        },
        other => other,
    }
}

// ---------------------------------------------------------------------------
// network_out
// ---------------------------------------------------------------------------

/// `writeNetwork`: nets `1..=max_net_number`; a net appears when it has a wire, a via or a
/// conduction area on a signal layer.
fn network_out(board: &BasicBoard, transform: &CoordinateTransform) -> Vec<SessionNet> {
    let max_net = board.rules.nets.max_net_number();
    let max_net = usize::try_from(max_net).unwrap_or(0);
    // One pass: the connectable items of every net, in item-list order.
    let mut buckets: Vec<Vec<ItemId>> = vec![Vec::new(); max_net + 1];
    for item in board.items() {
        if !item.is_connectable_kind() {
            continue;
        }
        for &net in item.net_numbers() {
            if let Some(bucket) = usize::try_from(net).ok().and_then(|n| buckets.get_mut(n)) {
                if net >= 1 {
                    bucket.push(item.id());
                }
            }
        }
    }
    let mut result = Vec::new();
    for (net_number, ids) in buckets.iter().enumerate().skip(1) {
        let items: Vec<WiringItem> = ids
            .iter()
            .filter_map(|id| board.item(*id))
            .filter(|item| item.fixed_state() != FixedState::SystemFixed)
            .filter_map(|item| wiring_item(board, item, transform))
            .collect();
        if !items.is_empty() {
            result.push(SessionNet {
                name: board
                    .rules
                    .nets
                    .get(net_number as i32)
                    .map(|net| net.name.clone())
                    .unwrap_or_default(),
                items,
            });
        }
    }
    result
}

/// Wire, via or conduction area of one item; `None` for other items and for a conduction
/// area Java skips (after writing the net's header, which cannot happen on a loaded board).
fn wiring_item(
    board: &BasicBoard,
    item: &Item,
    transform: &CoordinateTransform,
) -> Option<WiringItem> {
    match &item.kind {
        ItemKind::Trace(_) => Some(WiringItem::Wire(wire(board, item, transform))),
        ItemKind::Via(via) => {
            let padstack = board.library.padstacks.get(via.drill.padstack)?;
            let [x, y] = transform.board_to_dsn_point(via.drill.center.to_float());
            Some(WiringItem::Via(DsnVia {
                padstack: padstack.name.clone(),
                at: DsnPoint { x, y },
                net: None,
                clearance_class: None,
                via_type: fixed_state_type(item.fixed_state()),
            }))
        }
        ItemKind::Area(area)
            if area.is_conduction_area()
                && board.layer_structure.layers[item.first_layer() as usize].is_signal =>
        {
            conduction_area(board, item, transform)
        }
        _ => None,
    }
}

/// `writeFixedState`: nothing up to `SHOVE_FIXED`, `protect` above it (`SYSTEM_FIXED` never
/// reaches the writer).
fn fixed_state_type(state: FixedState) -> Option<WireType> {
    match state {
        FixedState::Unfixed | FixedState::ShoveFixed => None,
        FixedState::UserFixed => Some(WireType::Protect),
        FixedState::SystemFixed => Some(WireType::Fix),
    }
}

/// `writeWire`.
fn wire(board: &BasicBoard, item: &Item, transform: &CoordinateTransform) -> Wire {
    let trace = item.as_trace().expect("a trace");
    let layer_name = board.layer_structure.layers[trace.layer as usize]
        .name
        .clone();
    let width = f64::from(round_to_i32(
        transform.board_to_dsn(f64::from(trace.half_width.wrapping_mul(2))),
    ));
    let corners = trace.polyline.corners();
    let last = corners.len().saturating_sub(1);
    let mut points: Vec<DsnPoint> = Vec::with_capacity(corners.len());
    let mut previous: Option<[i32; 2]> = None;
    for (i, corner) in corners.iter().enumerate() {
        let mut at = corner.to_float();
        if i == 0 || i == last {
            if let Some(snapped) = snapped_endpoint(board, item.id(), i == 0) {
                at = snapped;
            }
        }
        let [x, y] = transform.board_to_dsn_point(at);
        let current = [round_to_i32(x), round_to_i32(y)];
        if i == 0 || previous != Some(current) {
            points.push(DsnPoint {
                x: f64::from(current[0]),
                y: f64::from(current[1]),
            });
            previous = Some(current);
        }
    }
    Wire {
        shapes: vec![DsnShape::Path {
            layer: LayerRef::Named(layer_name),
            width,
            points,
        }],
        windows: Vec::new(),
        net: None,
        clearance_class: None,
        wire_type: fixed_state_type(item.fixed_state()),
    }
}

/// `snappedEndpoint`: the exact centre of a drill item the trace ends on, when the end lies
/// within the pad's inradius but not at the centre; `None` to keep the corner.
///
/// Contacts are visited in item id order (Java's hash order is not defined; the first drill
/// item whose rule applies decides, and at most one can contain the corner).
pub fn snapped_endpoint(board: &BasicBoard, trace: ItemId, start_side: bool) -> Option<FloatPoint> {
    let item = board.item(trace)?;
    let trace_data = item.as_trace()?;
    let corner = if start_side {
        trace_data.first_corner()
    } else {
        trace_data.last_corner()
    };
    let corner = corner.to_float();
    let contacts: BTreeSet<ItemId> = if start_side {
        board.trace_start_contacts(trace)
    } else {
        board.trace_end_contacts(trace)
    };
    let layer = trace_data.layer;
    for contact in contacts {
        let Some(drill) = board.item(contact).and_then(Item::drill) else {
            continue;
        };
        if layer < drill.first_layer || layer > drill.last_layer {
            continue;
        }
        let Some(pad) = drill.shape_on_layer(layer) else {
            continue;
        };
        let center = drill.center.to_float();
        let distance = corner.distance(center);
        if distance <= 0.5 {
            // Already at the centre (within rounding): nothing to fix.
            return None;
        }
        if distance <= pad.border_distance(center) {
            return Some(center);
        }
    }
    None
}

/// `writeConductionArea`: the boundary as integers, the holes as doubles.
fn conduction_area(
    board: &BasicBoard,
    item: &Item,
    transform: &CoordinateTransform,
) -> Option<WiringItem> {
    if item.net_count() != 1 {
        return None;
    }
    let area = item.as_area()?;
    let layer = LayerRef::Named(
        board.layer_structure.layers[area.layer() as usize]
            .name
            .clone(),
    );
    let (boundary, holes): (Shape, Vec<Shape>) = match area.area() {
        Area::Shape(shape) => (shape.clone(), Vec::new()),
        other => (other.get_border(), other.get_holes()),
    };
    let shapes = board_shape_to_dsn(&boundary, layer.clone(), transform, false)
        .map(round_shape)
        .into_iter()
        .collect();
    let windows = holes
        .iter()
        .filter_map(|hole| board_shape_to_dsn(hole, layer.clone(), transform, false))
        .collect();
    Some(WiringItem::Wire(Wire {
        shapes,
        windows,
        net: None,
        clearance_class: None,
        wire_type: None,
    }))
}
