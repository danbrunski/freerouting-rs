//! The structure scope: layers, outline, rules, keepouts and planes. Creates
//! the board.
//!
//! Port of `io.specctra.parser.Structure.readScope` and its helpers.

use std::collections::HashSet;

use freerouting_dsn::ast;
use freerouting_geometry::jmath::round_to_i32;
use freerouting_geometry::{
    Area, CRIT_INT, IntBox, PolylineShape, PolylineShapeOps, Shape, TileShape, TileShapeOps,
};

use super::layer::{DsnLayer, DsnLayerStructure};
use super::shape::{DsnArea, DsnShape, DsnShapeKind, rectangle_to_board, transform_area_to_board};
use super::{NetKey, PlaneInfo, ReadContext};
use crate::board::RoutingBoard;
use crate::board::state::{Communication, SpecctraParserInfo};
use crate::board::structure::{AngleRestriction, FixedState, Layer, LayerStructure};
use crate::datastructures::ItemIdGenerator;
use crate::io::CoordinateTransform;
use crate::rules::{BoardRules, ClearanceMatrix, ItemClass};

/// What the structure scope collects before the board exists.
///
/// Port of `Structure.BoardConstructionInfo`.
#[derive(Default)]
struct BoardConstructionInfo {
    layer_info: Vec<DsnLayer>,
    bounding_shape: Option<DsnShape>,
    outline_shapes: Vec<DsnShape>,
    outline_clearance_class_name: Option<String>,
    default_rules: Vec<ast::Rule>,
    layer_dependent_rules: Vec<(String, Vec<ast::Rule>)>,
}

/// Java `Structure.addBoundaryShape`.
fn add_boundary_shape(ctx: &mut ReadContext, info: &mut BoardConstructionInfo, shape: DsnShape) {
    if shape.is_path() {
        info.outline_shapes.push(shape);
        return;
    }
    if shape.layer.is_pcb() {
        if info.bounding_shape.is_none() {
            info.bounding_shape = Some(shape);
        } else {
            info.outline_shapes.push(shape);
        }
    } else if shape.layer.is_signal_pseudo_layer() {
        info.outline_shapes.push(shape);
    } else {
        ctx.log("Structure.add_boundary_shape: unexpected layer at boundary");
    }
}

fn snap_angle(angle: ast::SnapAngle) -> AngleRestriction {
    match angle {
        ast::SnapAngle::None => AngleRestriction::None,
        ast::SnapAngle::NinetyDegree => AngleRestriction::NinetyDegree,
        ast::SnapAngle::FortyfiveDegree => AngleRestriction::FortyfiveDegree,
    }
}

/// Reads the structure scope and creates the board. Returns false where
/// Java's `Structure.readScope` does.
///
/// The syntax tree no longer knows the order of the sub-scopes; they are
/// applied as if the layers came first, which is the order every exporter
/// writes.
pub(crate) fn read_structure(ctx: &mut ReadContext, structure: &ast::Structure) -> bool {
    let mut info = BoardConstructionInfo::default();

    // Layers.
    for layer in &structure.layers {
        if !layer.rules.is_empty() {
            info.layer_dependent_rules
                .push((layer.name.clone(), layer.rules.clone()));
        }
        let is_signal = match &layer.layer_type {
            None | Some(ast::LayerType::Signal) | Some(ast::LayerType::Jumper) => true,
            Some(ast::LayerType::Power) => false,
            Some(ast::LayerType::Other(name)) => {
                ctx.log(format!(
                    "Structure.read_layer_scope: the layer '{}' has an unknown layer type '{name}'",
                    layer.name
                ));
                continue;
            }
        };
        let no = info.layer_info.len() as i32;
        info.layer_info.push(DsnLayer::new(
            layer.name.clone(),
            no,
            is_signal,
            layer.use_net.clone(),
        ));
    }
    ctx.layer_structure = DsnLayerStructure::new(info.layer_info.clone());

    // Boundaries: the shapes after the first come first, the first is added
    // last.
    for boundary in &structure.boundaries {
        let mut shapes = boundary.shapes.iter();
        let first = shapes.next().and_then(|s| DsnShape::resolve(s, None));
        for shape in shapes {
            if let Some(shape) = DsnShape::resolve(shape, None) {
                add_boundary_shape(ctx, &mut info, shape);
            }
        }
        if boundary.clearance_class.is_some() {
            info.outline_clearance_class_name = boundary.clearance_class.clone();
        }
        match first {
            Some(shape) => add_boundary_shape(ctx, &mut info, shape),
            None => ctx.log("Structure.read_boundary_scope: shape is null"),
        }
    }

    if let Some(via) = &structure.via {
        let mut names = via.padstacks.clone();
        names.extend(via.spare.iter().cloned());
        ctx.via_padstack_names = Some(names);
    }
    info.default_rules.extend(structure.rules.iter().cloned());

    // Keepouts, grouped by kind as Java inserts them.
    let mut keepouts: [Vec<DsnArea>; 3] = Default::default();
    for keepout in &structure.keepouts {
        let index = match keepout.kind {
            ast::KeepoutKind::Keepout => 0,
            ast::KeepoutKind::ViaKeepout => 1,
            ast::KeepoutKind::PlaceKeepout => 2,
        };
        match DsnArea::resolve(&keepout.area, Some(&ctx.layer_structure), false) {
            Some(area) => keepouts[index].push(area),
            // Java keeps a null area here and fails with an exception when
            // it inserts the keepouts.
            None => ctx.log("Shape.read_area_scope: could not read shape of keepout"),
        }
    }

    let skip_windows = ctx
        .host_cad
        .as_deref()
        .is_some_and(|cad| crate::util::equals_ignore_case(cad, "allegro"));
    for plane in &structure.planes {
        ctx.scope_identifier = plane.net.clone();
        match DsnArea::resolve(&plane.area, Some(&ctx.layer_structure), skip_windows) {
            Some(area) => ctx.plane_list.push(PlaneInfo {
                area,
                net_name: plane.net.clone(),
            }),
            None => ctx.log("Plane.read_scope: could not read the plane shape"),
        }
    }
    if structure.autoroute_settings.is_some() {
        ctx.autoroute_settings_read = true;
    }
    if let Some(via_at_smd) = structure.control.as_ref().and_then(|c| c.via_at_smd) {
        ctx.via_at_smd_allowed = via_at_smd;
    }
    let flip_style_rotate_first = structure.flip_style_rotate_first.unwrap_or(false);
    if let Some(angle) = structure.snap_angle {
        ctx.snap_angle = snap_angle(angle);
    }

    let mut result = true;
    if ctx.board.is_none() {
        result = create_board(ctx, &mut info);
    }
    if ctx.board.is_none() {
        return false;
    }
    if flip_style_rotate_first {
        ctx.board().components.set_flip_style_rotate_first(true);
    }
    for (index, list) in keepouts.iter().enumerate() {
        for area in list {
            if !insert_keepout(ctx, area, index) {
                return false;
            }
        }
    }
    if !insert_planes(ctx) {
        return false;
    }
    insert_missing_power_planes(ctx, &info.layer_info);
    result
}

/// The nets with a plane on a power layer.
fn nets_with_plane_on_power_layer(planes: &[PlaneInfo]) -> HashSet<String> {
    planes
        .iter()
        .filter(|p| p.area.shapes.iter().any(|s| !s.layer.is_signal))
        .map(|p| p.net_name.clone())
        .collect()
}

fn insert_planes(ctx: &mut ReadContext) -> bool {
    let dedicated = nets_with_plane_on_power_layer(&ctx.plane_list);
    let planes = std::mem::take(&mut ctx.plane_list);
    let mut result = true;
    for plane in &planes {
        let key = NetKey::new(&plane.net_name, 1);
        if ctx.netlist.add_net(key) {
            ctx.board().rules.add_net(&plane.net_name, 1, true);
        }
        let Some(net) = ctx.board().rules.nets.get_by_name(&plane.net_name, 1) else {
            ctx.log("Plane.read_scope: net not found");
            continue;
        };
        let net_number = net.net_number;
        let net_class = net.net_class();
        let transform = ctx.coordinate_transform;
        let plane_area = transform_area_to_board(&plane.area.shapes, &transform);
        let layer = plane.area.layer().clone();
        if layer.is_signal && dedicated.contains(&plane.net_name) {
            // A copper pour of a net that has a plane layer: its outline
            // would make all pads of the net look connected.
            ctx.log(format!(
                "Ignoring the copper pour of net '{}' on signal layer '{}': the net has a \
                 dedicated plane layer, so its pads are routed to that plane.",
                plane.net_name, layer.name
            ));
            continue;
        }
        if layer.no < 0 {
            ctx.log("Plane.read_scope: unexpected layer name");
            result = false;
            break;
        }
        let board = ctx.board();
        let clearance_class = match &plane.area.clearance_class {
            Some(name) => {
                let no = board.rules.clearance_matrix.get_no(name);
                if no < 0 {
                    board
                        .log
                        .push("Structure.read_scope: clearance class not found".into());
                    BoardRules::clearance_class_none()
                } else {
                    no
                }
            }
            None => board
                .rules
                .net_classes
                .get(net_class)
                .default_item_clearance_classes
                .get(ItemClass::Area),
        };
        match plane_area {
            Some(area) => {
                board.insert_conduction_area(
                    area,
                    layer.no,
                    &[net_number],
                    clearance_class,
                    false,
                    FixedState::SystemFixed,
                );
            }
            None => board
                .log
                .push("BasicBoard.insert_conduction_area: area is null".into()),
        }
    }
    ctx.plane_list = planes;
    result
}

/// Java `Structure.insertMissingPowerPlanes`: a power layer with nets but
/// no conduction area gets one over the whole board.
fn insert_missing_power_planes(ctx: &mut ReadContext, layer_info: &[DsnLayer]) {
    let conduction_layers: Vec<i32> = {
        let board = ctx.board();
        board
            .conduction_areas()
            .into_iter()
            .filter_map(|id| board.item(id).and_then(|i| i.as_area()).map(|a| a.layer()))
            .collect()
    };
    for layer in layer_info {
        if layer.is_signal || conduction_layers.contains(&layer.no) || layer.net_names.is_empty() {
            continue;
        }
        let net_name = layer.net_names[0].clone();
        if ctx.netlist.add_net(NetKey::new(&net_name, 1)) {
            ctx.board().rules.add_net(&net_name, 1, true);
        }
        let board = ctx.board();
        let Some(net_number) = board
            .rules
            .nets
            .get_by_name(&net_name, 1)
            .map(|n| n.net_number)
        else {
            board.log.push(format!(
                "Structure.insert_missing_power_planes: net not found at '{net_name}'"
            ));
            continue;
        };
        let area = Area::Shape(Shape::from(board.bounding_box()));
        board.insert_conduction_area(
            area,
            layer.no,
            &[net_number],
            BoardRules::clearance_class_none(),
            false,
            FixedState::SystemFixed,
        );
    }
}

/// Java `Structure.insertKeepout`. `kind` is 0 for keepouts, 1 for via
/// keepouts and 2 for place keepouts.
fn insert_keepout(ctx: &mut ReadContext, area: &DsnArea, kind: usize) -> bool {
    let transform = ctx.coordinate_transform;
    let Some(keepout_area) = transform_area_to_board(&area.shapes, &transform) else {
        ctx.log("Shape.transform_area_to_board: keepout area could not be converted");
        return true;
    };
    if keepout_area.dimension() < 2 {
        ctx.log(format!(
            "Keepout zone '{}' was skipped because its geometry is degenerate (e.g. zero-area \
             polygon). This is likely a DSN export issue in your EDA tool. The board will be \
             routed without this keepout constraint.",
            area.name.as_deref().unwrap_or("null")
        ));
        return true;
    }
    let layer = area.layer().clone();
    if layer.is_signal_pseudo_layer() {
        let signal_layers: Vec<i32> = ctx
            .layer_structure
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.is_signal)
            .map(|(i, _)| i as i32)
            .collect();
        for i in signal_layers {
            if (i as usize) < ctx.board().layer_count() {
                insert_keepout_on_layer(ctx.board(), &keepout_area, i, area, kind);
            }
        }
    } else if layer.no >= 0 {
        insert_keepout_on_layer(ctx.board(), &keepout_area, layer.no, area, kind);
    } else {
        ctx.log("Structure.insert_keepout: unknown layer name");
        return false;
    }
    true
}

fn insert_keepout_on_layer(
    board: &mut RoutingBoard,
    keepout_area: &Area,
    layer: i32,
    area: &DsnArea,
    kind: usize,
) {
    let clearance_class = match &area.clearance_class {
        None => {
            let default = board.rules.default_net_class();
            board
                .rules
                .net_classes
                .get(default)
                .default_item_clearance_classes
                .get(ItemClass::Area)
        }
        Some(name) => {
            let no = board.rules.clearance_matrix.get_no(name);
            if no < 0 {
                board.log.push(format!(
                    "Keepout.insert_keepout: clearance class not found at '{name}'"
                ));
                BoardRules::clearance_class_none()
            } else {
                no
            }
        }
    };
    let area = keepout_area.clone();
    match kind {
        1 => board.insert_via_obstacle(area, layer, clearance_class, FixedState::SystemFixed),
        2 => board.insert_component_obstacle(area, layer, clearance_class, FixedState::SystemFixed),
        _ => board.insert_obstacle(area, layer, clearance_class, FixedState::SystemFixed),
    };
}

/// Java `Structure.promotePowerLayersWithoutPlane`: a power layer without
/// nets and without a plane becomes a signal layer.
fn promote_power_layers_without_plane(ctx: &mut ReadContext, info: &mut BoardConstructionInfo) {
    let layers_with_plane: HashSet<i32> = ctx
        .plane_list
        .iter()
        .flat_map(|p| p.area.shapes.iter().map(|s| s.layer.no))
        .collect();
    for layer in &mut info.layer_info {
        if !layer.is_signal && layer.net_names.is_empty() && !layers_with_plane.contains(&layer.no)
        {
            ctx.log.push(format!(
                "Layer '{}' is declared as a power layer but no plane is defined for it. It will \
                 be treated as a signal layer.",
                layer.name
            ));
            layer.is_signal = true;
        }
    }
}

/// Java `Rectangle.union`.
fn union(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[0].min(b[0]),
        a[1].min(b[1]),
        a[2].max(b[2]),
        a[3].max(b[3]),
    ]
}

/// Java `Structure.createBoard`.
fn create_board(ctx: &mut ReadContext, info: &mut BoardConstructionInfo) -> bool {
    let layer_count = info.layer_info.len();
    if layer_count == 0 {
        ctx.log("Structure.create_board: layers missing in structure scope");
        return false;
    }
    promote_power_layers_without_plane(ctx, info);
    if info.bounding_shape.is_none() {
        if info.outline_shapes.is_empty() {
            ctx.log("Structure.create_board: outline missing");
            ctx.board_outline_ok = false;
            return false;
        }
        // Java fails on a polyline path here; its box is left out.
        let boxes: Vec<[f64; 4]> = info
            .outline_shapes
            .iter()
            .filter_map(DsnShape::bounding_box)
            .collect();
        let Some(first) = boxes.first().copied() else {
            ctx.board_outline_ok = false;
            return false;
        };
        let bbox = boxes[1..].iter().fold(first, |acc, b| union(acc, *b));
        info.bounding_shape = Some(DsnShape {
            layer: info.outline_shapes[0].layer.clone(),
            kind: DsnShapeKind::Rectangle,
            width: 0.0,
            coor: bbox.to_vec(),
        });
    }
    let bounding_shape = info.bounding_shape.clone().expect("bounding shape");
    let Some(bounding_box) = bounding_shape.bounding_box() else {
        ctx.board_outline_ok = false;
        return false;
    };
    let board_layers: Vec<Layer> = info
        .layer_info
        .iter()
        .map(|l| Layer::new(l.name.clone(), l.is_signal))
        .collect();
    let board_layer_structure = LayerStructure::new(board_layers);
    ctx.layer_structure = DsnLayerStructure::new(info.layer_info.clone());

    // An approximate scaling between DSN and board coordinates.
    let mut scale_factor = ctx.resolution.max(1);
    let mut max_coor = 0.0_f64;
    for v in bounding_box {
        max_coor = max_coor.max((v * f64::from(ctx.resolution)).abs());
    }
    if max_coor == 0.0 {
        ctx.board_outline_ok = false;
        return false;
    }
    // Make the scale factor smaller if integers might overflow.
    while 5.0 * max_coor >= f64::from(CRIT_INT) {
        scale_factor /= 10;
        max_coor /= 10.0;
    }
    ctx.coordinate_transform = CoordinateTransform::new(f64::from(scale_factor), 0.0, 0.0);
    let transform = ctx.coordinate_transform;
    let bounds = rectangle_to_board(&bounding_box, &transform).offset(1000.0);

    let mut outline_shapes: Vec<PolylineShape> = Vec::new();
    for shape in &info.outline_shapes {
        let mut shape = shape.clone();
        if shape.kind == DsnShapeKind::PolygonPath {
            // The offset of non-convex shapes is not implemented.
            shape.width = 0.0;
        }
        // Java fails with an exception on circles and polyline paths here;
        // they are left out.
        if let Some(Shape::Polyline(board_shape)) = shape.transform_to_board(&transform)
            && board_shape.dimension() > 0
        {
            outline_shapes.push(board_shape);
        }
    }
    if outline_shapes.is_empty() {
        // Use the bounding shape if the outline is missing.
        match bounding_shape.transform_to_board(&transform) {
            Some(Shape::Polyline(shape)) => outline_shapes.push(shape),
            _ => {
                ctx.board_outline_ok = false;
                return false;
            }
        }
    }
    let hole_shapes = separate_holes(&mut outline_shapes);

    let clearance_matrix = ClearanceMatrix::default_instance(layer_count, 0);
    let mut rules = BoardRules::new(board_layer_structure.clone(), clearance_matrix);
    let communication = Communication {
        coordinate_transform: transform,
        unit: ctx.unit,
        resolution: ctx.resolution,
        specctra_parser_info: Some(SpecctraParserInfo {
            string_quote: ctx.string_quote.clone(),
            host_cad: ctx.host_cad.clone(),
            host_version: ctx.host_version.clone(),
            constants: ctx.constants.clone(),
            write_resolution: ctx.write_resolution.clone(),
            dsn_file_generated_by_host: ctx.dsn_file_generated_by_host,
        }),
        id_generator: ItemIdGenerator::new(),
    };
    if communication.host_is_old_kicad() {
        ctx.log(
            "Structure.create_board: The DSN file was exported from an old KiCad version that \
             has known compatibility issues. Please update KiCad to version 6 or newer.",
        );
    }
    update_board_rules(ctx, info, &mut rules);
    rules.set_trace_angle_restriction(ctx.snap_angle);

    // Java's MinimalBoardManager.createBoard.
    let outline_clearance_class = match &info.outline_clearance_class_name {
        Some(name) => rules.clearance_matrix.get_no(name).max(0),
        None => {
            let default = rules.default_net_class();
            rules
                .net_classes
                .get(default)
                .default_item_clearance_classes
                .get(ItemClass::Area)
        }
    };
    let mut board = RoutingBoard::new(
        bounds,
        board_layer_structure,
        outline_shapes,
        outline_clearance_class,
        rules,
        communication,
    );
    board.log.append(&mut ctx.log);
    // The holes of the outline become keepouts.
    for hole in hole_shapes {
        for layer in 0..layer_count {
            board.insert_obstacle(
                Area::Shape(Shape::Polyline(hole.clone())),
                layer as i32,
                0,
                FixedState::SystemFixed,
            );
        }
    }
    ctx.board = Some(board);
    true
}

/// An outline shape while holes are separated.
///
/// Port of `Structure.OutlineShape`.
struct OutlineShape {
    bounding_box: IntBox,
    convex_shapes: Option<Vec<TileShape>>,
    is_hole: bool,
}

/// Java `Structure.separateHoles`: removes the shapes that lie inside
/// another shape from `outline_shapes` and returns them.
fn separate_holes(outline_shapes: &mut Vec<PolylineShape>) -> Vec<PolylineShape> {
    let mut shapes: Vec<OutlineShape> = outline_shapes
        .iter()
        .map(|s| OutlineShape {
            bounding_box: s.bounding_box(),
            convex_shapes: s.split_to_convex(),
            is_hole: false,
        })
        .collect();
    for i in 0..shapes.len() {
        for j in 0..shapes.len() {
            if i == j || shapes[j].is_hole {
                continue;
            }
            if !shapes[j]
                .bounding_box
                .contains_regular(&shapes[i].bounding_box.into())
            {
                continue;
            }
            // As in Java, the last containing candidate decides.
            shapes[i].is_hole = contains_all_corners(&shapes[j], &outline_shapes[i]);
        }
    }
    let mut holes = Vec::new();
    let mut kept = Vec::new();
    for (shape, info) in outline_shapes.drain(..).zip(&shapes) {
        if info.is_hole {
            holes.push(shape);
        } else {
            kept.push(shape);
        }
    }
    *outline_shapes = kept;
    holes
}

/// Java `OutlineShape.containsAllCorners`.
fn contains_all_corners(this: &OutlineShape, other: &PolylineShape) -> bool {
    let Some(convex_shapes) = &this.convex_shapes else {
        return false;
    };
    (0..other.border_line_count()).all(|i| {
        let corner = other.corner(i);
        convex_shapes.iter().any(|piece| piece.contains(&corner))
    })
}

/// Java `Structure.updateBoardRules`: applies the structure rules to the
/// new board's rules.
fn update_board_rules(ctx: &mut ReadContext, info: &BoardConstructionInfo, rules: &mut BoardRules) {
    let transform = ctx.coordinate_transform;
    let quote = ctx.string_quote.clone();
    let mut smd_to_turn_gap_found = false;
    for rule in &info.default_rules {
        if let ast::Rule::Clearance { value, types } = rule
            && set_clearance_rule(ctx, *value, types, -1, &transform, rules, &quote)
        {
            smd_to_turn_gap_found = true;
        }
    }
    for rule in &info.default_rules {
        if let ast::Rule::Width(width) = rule {
            let half_width = round_to_i32(transform.dsn_to_board(*width) / 2.0);
            rules.set_default_trace_half_widths(half_width);
        }
    }
    for (layer_name, layer_rules) in &info.layer_dependent_rules {
        let layer = ctx.layer_structure.get_no(layer_name);
        if layer < 0 {
            continue;
        }
        for rule in layer_rules {
            match rule {
                ast::Rule::Width(width) => {
                    let half_width = round_to_i32(transform.dsn_to_board(*width) / 2.0);
                    rules.set_default_trace_half_width(layer as usize, half_width);
                }
                ast::Rule::Clearance { value, types } => {
                    set_clearance_rule(ctx, *value, types, layer, &transform, rules, &quote);
                }
                ast::Rule::Length(_) => {}
            }
        }
    }
    if !smd_to_turn_gap_found {
        let value = f64::from(rules.min_trace_half_width());
        rules.set_pin_edge_to_turn_dist(value);
    }
}

/// True if a clearance type pairs a class with `wire`.
pub(crate) fn contains_wire_clearance_pair(types: &[String]) -> bool {
    types
        .iter()
        .any(|t| t.starts_with("wire_") || t.ends_with("_wire"))
}

fn set_matrix_value(rules: &mut BoardRules, i: i32, j: i32, layer: i32, value: i32) {
    if i < 0 || j < 0 {
        return;
    }
    if layer < 0 {
        rules
            .clearance_matrix
            .set_value(i as usize, j as usize, value);
    } else {
        rules
            .clearance_matrix
            .set_value_on_layer(i as usize, j as usize, layer as usize, value);
    }
}

/// Java `Structure.appendClearanceClass`.
fn append_clearance_class(rules: &mut BoardRules, name: &str) -> i32 {
    rules.clearance_matrix.append_class(name);
    let result = rules.clearance_matrix.get_no(name);
    let item_class = match name {
        "via" => Some(ItemClass::Via),
        "pin" => Some(ItemClass::Pin),
        "smd" => Some(ItemClass::Smd),
        "area" => Some(ItemClass::Area),
        _ => None,
    };
    let default = rules.default_net_class();
    if let Some(item_class) = item_class {
        rules
            .net_classes
            .get_mut(default)
            .default_item_clearance_classes
            .set(item_class, result);
    }
    result
}

/// Java `Structure.setClearanceRule`: a DSN clearance rule on all layers
/// (`layer < 0`) or one layer. Returns true if it contained
/// `smd_to_turn_gap`.
pub(crate) fn set_clearance_rule(
    ctx: &mut ReadContext,
    rule_value: f64,
    types: &[String],
    layer: i32,
    transform: &CoordinateTransform,
    rules: &mut BoardRules,
    string_quote: &str,
) -> bool {
    let mut result = false;
    let clearance = round_to_i32(transform.dsn_to_board(rule_value));
    if types.is_empty() {
        if layer < 0 {
            rules.clearance_matrix.set_default_value(clearance);
        } else {
            rules
                .clearance_matrix
                .set_default_value_on_layer(layer as usize, clearance);
        }
        return result;
    }
    if contains_wire_clearance_pair(types) {
        for name in ["via", "smd", "pin", "area"] {
            append_clearance_class(rules, name);
        }
    }
    for current in types {
        if current.eq_ignore_ascii_case("smd_to_turn_gap") {
            rules.set_pin_edge_to_turn_dist(f64::from(clearance));
            result = true;
            continue;
        }
        let pair: [String; 2] = if types.len() == 2 {
            let mut pair = [types[0].clone(), types[1].clone()];
            for i in 0..2 {
                pair[i] = pair[i].replace('"', "");
                if pair[1].starts_with('_') {
                    pair[1] = pair[1][1..].to_string();
                }
            }
            pair
        } else if !string_quote.is_empty() && current.starts_with(string_quote) {
            let rest = &current[string_quote.len()..];
            let parts: Vec<&str> = rest.splitn(2, string_quote).collect();
            if parts.len() != 2 || !parts[1].starts_with('_') {
                ctx.log(format!(
                    "Structure.set_clearance_rule: '_' expected at '{rest}'"
                ));
                continue;
            }
            [parts[0].to_string(), parts[1][1..].to_string()]
        } else {
            let parts: Vec<&str> = current.splitn(2, '_').collect();
            if parts.len() != 2 {
                // Types with more than one underscore are not implemented.
                continue;
            }
            [parts[0].to_string(), parts[1].to_string()]
        };
        let class_no = |rules: &mut BoardRules, name: &str| -> i32 {
            let no = if name == "wire" {
                1
            } else {
                rules.clearance_matrix.get_no(name)
            };
            if no < 0 {
                append_clearance_class(rules, name)
            } else {
                no
            }
        };
        let first = class_no(rules, &pair[0]);
        let second = class_no(rules, &pair[1]);
        set_matrix_value(rules, first, second, layer, clearance);
        set_matrix_value(rules, second, first, layer, clearance);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting_geometry::IntPoint;

    #[test]
    fn separate_holes_finds_the_inner_square() {
        let outer = PolylineShape::from(IntBox::new(0, 0, 100, 100));
        let inner = PolylineShape::from(IntBox::new(10, 10, 20, 20));
        let other = PolylineShape::from(IntBox::new(200, 0, 300, 100));
        let mut shapes = vec![outer, inner, other];
        let holes = separate_holes(&mut shapes);
        assert_eq!(holes.len(), 1);
        assert_eq!(holes[0].bounding_box(), IntBox::new(10, 10, 20, 20));
        assert_eq!(shapes.len(), 2);
        assert!(shapes[0].contains(&freerouting_geometry::Point::Int(IntPoint::new(50, 50))));
    }

    #[test]
    fn wire_clearance_pairs() {
        assert!(contains_wire_clearance_pair(&["wire_smd".into()]));
        assert!(contains_wire_clearance_pair(&["via_wire".into()]));
        assert!(!contains_wire_clearance_pair(&["smd_pin".into()]));
    }
}
