//! Writing a board's rules (and optionally the router settings) as a
//! Specctra `.rules` file (C04.6).
//!
//! Port of `io.specctra.RulesWriter.write` (Java at `cda540f`). Content and
//! layout are separate: [`rules_from_board`] decides what goes into the file
//! and builds a [`RulesFile`] syntax tree; [`RulesFile::to_rules`] writes it.
//! The items come in Java's order:
//!
//! | Item | Java |
//! |---|---|
//! | `(snap_angle ...)` | `Structure.writeSnapAngle` |
//! | `(autoroute_settings ...)`, only with settings | `AutorouteSettings.writeScope` |
//! | `(rule ...)`: width, default clearance, `smd_to_turn_gap`, a clearance per named class | `Rule.writeDefaultRule(.., 0)` |
//! | `(padstack ...)` for each library padstack that is a via padstack | `Library.writePadstackScope` |
//! | `(via ...)`, `(via_rule ...)` | `Network.writeViaInfos`, `writeViaRules` |
//! | `(class ...)` | `Network.writeNetClasses` |
//!
//! **What the format loses.** The default rule holds the clearance of layer 0
//! only, and one value per named class: the clearance of the class against
//! itself. Reading it back sets the default value first (every cell of the
//! matrix), so off-diagonal cells and other layers' values come back as the
//! default. That is Java's format; `tests/rules_writer.rs` pins the round trip
//! on what it can express.
//!
//! **Where it differs from Java** (docs/DIVERGENCES.md, "Rules files"):
//! - the tree is written in the plain syntax-tree style, with `snap_angle`
//!   and `via_rule`, which both readers accept. Java writes `snapAngle` and
//!   `viaRule`, which its own reader skips, so a Java round trip loses the
//!   snap angle and the via rules;
//! - the design name goes through the tree writer's name quoting, where Java
//!   writes it raw;
//! - trace costs are stored as Java's `float` text read back as a `double`:
//!   `2.8`, not `2.799999952316284` (Java writes `String.valueOf((float)
//!   cost)`).
//!
//! The shape conversion ([`board_shape_to_dsn`]) is shared with the DSN
//! writer (C06).

use freerouting_dsn::ast::{
    AutorouteLayerRule, AutorouteSettings, Circuit, Direction, LayerRef, LayerRule, LengthRule,
    NetClass, NetworkVia, Padstack as DsnPadstack, Point as DsnPoint, Rule, RulesFile, RulesItem,
    Shape as DsnShape, SnapAngle, ViaRule,
};
use freerouting_geometry::{FloatPoint, IntBox, PolylineShape, PolylineShapeOps, Shape, TileShape};

use crate::board::BasicBoard;
use crate::board::structure::AngleRestriction;
use crate::io::CoordinateTransform;
use crate::library::Padstack;
use crate::rules::NetClassId;
use crate::settings::RouterSettings;

/// The rules of `board` as a syntax tree (`RulesWriter.write`). `name` is the
/// design name of the header; `settings` adds the `autoroute_settings`
/// scope.
pub fn rules_from_board(
    board: &BasicBoard,
    settings: Option<&RouterSettings>,
    name: &str,
) -> RulesFile {
    let transform = board.communication.coordinate_transform;
    let mut items = vec![RulesItem::SnapAngle(snap_angle(
        board.rules.trace_angle_restriction(),
    ))];
    if let Some(settings) = settings {
        items.push(RulesItem::AutorouteSettings(autoroute_settings(
            board, settings,
        )));
    }
    items.push(RulesItem::Rule(default_rule(board, &transform, 0)));
    for padstack in board.library.padstacks.iter() {
        if board
            .library
            .get_via_padstack_by_name(&padstack.name)
            .is_some()
            && let Some(converted) = padstack_scope(board, padstack, &transform)
        {
            items.push(RulesItem::Padstack(converted));
        }
    }
    items.extend(via_infos(board).into_iter().map(RulesItem::Via));
    items.extend(via_rules(board).into_iter().map(RulesItem::ViaRule));
    items.extend(
        net_classes(board, &transform)
            .into_iter()
            .map(RulesItem::Class),
    );
    RulesFile {
        design: name.to_string(),
        items,
    }
}

/// The rules as text: `rules_from_board(..).to_rules()`.
pub fn write_rules(board: &BasicBoard, settings: Option<&RouterSettings>, name: &str) -> String {
    rules_from_board(board, settings, name).to_rules()
}

/// The name as Java's `IdentifierType.write` leaves it, without the quoting
/// (the tree writer quotes where it must): outer double quotes removed, then
/// every occurrence of the board's string quote. Names that Java writes with
/// `IdentifierType` go through this; layer names and the design name are
/// written raw.
fn ident(name: &str, quote: &str) -> String {
    let mut name = name;
    while name.len() > 2 && name.starts_with('"') && name.ends_with('"') {
        name = &name[1..name.len() - 1];
    }
    if quote.is_empty() {
        name.to_string()
    } else {
        name.replace(quote, "")
    }
}

/// The board's string quote.
fn string_quote(board: &BasicBoard) -> String {
    board
        .communication
        .specctra_parser_info
        .as_ref()
        .map_or_else(|| "\"".to_string(), |info| info.string_quote.clone())
}

fn snap_angle(restriction: AngleRestriction) -> SnapAngle {
    match restriction {
        AngleRestriction::None => SnapAngle::None,
        AngleRestriction::NinetyDegree => SnapAngle::NinetyDegree,
        AngleRestriction::FortyfiveDegree => SnapAngle::FortyfiveDegree,
    }
}

/// A trace cost as Java writes it: `String.valueOf((float) cost)`, which a
/// reader parses back as a double.
fn float_cost(cost: f64) -> f64 {
    format!("{}", cost as f32).parse().unwrap_or(cost)
}

/// `AutorouteSettings.writeScope`.
fn autoroute_settings(board: &BasicBoard, settings: &RouterSettings) -> AutorouteSettings {
    let quote = string_quote(board);
    let layer_rules = board
        .layer_structure
        .layers
        .iter()
        .enumerate()
        .map(|(i, layer)| AutorouteLayerRule {
            layer: ident(&layer.name, &quote),
            active: Some(settings.layer_active(i)),
            preferred_direction: Some(if settings.preferred_direction_is_horizontal(i) {
                Direction::Horizontal
            } else {
                Direction::Vertical
            }),
            preferred_direction_trace_costs: Some(float_cost(
                settings.preferred_direction_trace_costs(i),
            )),
            against_preferred_direction_trace_costs: Some(float_cost(
                settings.against_preferred_direction_trace_costs(i),
            )),
        })
        .collect();
    AutorouteSettings {
        fanout: None,
        autoroute: Some(settings.run_router()),
        postroute: Some(settings.run_optimizer()),
        vias: Some(settings.allowed_via_types),
        via_costs: Some(settings.scoring.via_costs),
        plane_via_costs: Some(settings.scoring.plane_via_costs),
        start_ripup_costs: Some(settings.scoring.start_ripup_costs),
        start_pass_no: None,
        layer_rules,
    }
}

/// `Rule.writeDefaultRule`: the width of the default net class, the default
/// clearance on `layer`, the pin edge to turn distance, and the clearance of
/// every named class against itself.
fn default_rule(board: &BasicBoard, transform: &CoordinateTransform, layer: i32) -> Vec<Rule> {
    let quote = string_quote(board);
    let rules = &board.rules;
    let default_half_width = rules
        .default_net_class_id()
        .map_or(0, |id| rules.net_classes.get(id).trace_half_width(0));
    let matrix = &rules.clearance_matrix;
    let default_class = crate::rules::BoardRules::default_clearance_class();
    let mut result = vec![
        Rule::Width(2.0 * transform.board_to_dsn(f64::from(default_half_width))),
        Rule::Clearance {
            value: transform.board_to_dsn(f64::from(matrix.get_value(
                default_class,
                default_class,
                layer,
                false,
            ))),
            types: Vec::new(),
        },
        Rule::Clearance {
            value: transform.board_to_dsn(rules.pin_edge_to_turn_dist()),
            types: vec!["smd_to_turn_gap".to_string()],
        },
    ];
    for i in 1..matrix.class_count() as i32 {
        let name = matrix.get_name(i).unwrap_or_default();
        if name == "default" {
            continue;
        }
        result.push(Rule::Clearance {
            value: transform.board_to_dsn(f64::from(matrix.get_value(i, i, layer, false))),
            // Java writes `(type )` for a class with an empty name, which both
            // readers take as a clearance without a type.
            types: Some(ident(name, &quote))
                .filter(|n| !n.is_empty())
                .into_iter()
                .collect(),
        });
    }
    result
}

/// `Network.writeViaInfos`.
pub(crate) fn via_infos(board: &BasicBoard) -> Vec<NetworkVia> {
    let quote = string_quote(board);
    board
        .rules
        .via_infos
        .iter()
        .map(|(_, info)| NetworkVia {
            name: ident(&info.name, &quote),
            padstack: board
                .library
                .padstacks
                .get(info.padstack)
                .map(|p| ident(&p.name, &quote))
                .unwrap_or_default(),
            clearance_class: ident(
                board
                    .rules
                    .clearance_matrix
                    .get_name(info.clearance_class)
                    .unwrap_or_default(),
                &quote,
            ),
            attach: info.attach_smd_allowed,
        })
        .collect()
}

/// `Network.writeViaRules`.
pub(crate) fn via_rules(board: &BasicBoard) -> Vec<ViaRule> {
    let quote = string_quote(board);
    board
        .rules
        .via_rules
        .iter()
        .map(|(_, rule)| ViaRule {
            name: ident(&rule.name, &quote),
            vias: rule
                .vias()
                .iter()
                .map(|v| ident(&board.rules.via_infos.get(*v).name, &quote))
                .collect(),
        })
        .collect()
}

/// `Network.writeNetClasses`.
pub(crate) fn net_classes(board: &BasicBoard, transform: &CoordinateTransform) -> Vec<NetClass> {
    board
        .rules
        .net_classes
        .iter()
        .map(|(id, _)| net_class(board, id, transform))
        .collect()
}

/// `Network.writeNetClass`, with `Rule.writeScope` and `writeCircuit`.
fn net_class(board: &BasicBoard, id: NetClassId, transform: &CoordinateTransform) -> NetClass {
    let quote = string_quote(board);
    let rules = &board.rules;
    let class = rules.net_classes.get(id);
    let mut nets = Vec::new();
    for number in 1..=rules.nets.max_net_number() {
        if let Some(net) = rules.nets.get(number)
            && net.net_class() == id
        {
            nets.push(ident(&net.name, &quote));
        }
    }
    let layer_count = board.layer_structure.count();
    let width = |layer: i32| {
        Rule::Width(2.0 * transform.board_to_dsn(f64::from(class.trace_half_width(layer))))
    };
    // The width, then a layer rule for each layer that differs from layer 0.
    let layer_rules = (1..layer_count as i32)
        .filter(|layer| class.trace_half_width(*layer) != class.trace_half_width(0))
        .map(|layer| LayerRule {
            layers: vec![board.layer_structure.layers[layer as usize].name.clone()],
            rules: vec![width(layer)],
        })
        .collect();
    let use_layer = (0..class.layer_count())
        .filter(|i| class.is_active_routing_layer(*i as i32))
        .map(|i| board.layer_structure.layers[i].name.clone())
        .collect();
    let min = class.minimum_trace_length();
    let max = class.maximum_trace_length();
    let length = (min > 0.0 || max > 0.0).then(|| LengthRule {
        max: if max <= 0.0 {
            -1.0
        } else {
            transform.board_to_dsn(max)
        },
        min: Some(if min <= 0.0 {
            0.0
        } else {
            transform.board_to_dsn(min)
        }),
    });
    NetClass {
        name: ident(class.name(), &quote),
        nets,
        clearance_class: Some(ident(
            rules
                .clearance_matrix
                .get_name(class.trace_clearance_class())
                .unwrap_or_default(),
            &quote,
        )),
        via_rule: class
            .via_rule()
            .map(|r| ident(&rules.via_rules.get(r).name, &quote)),
        rules: vec![width(0)],
        layer_rules,
        circuit: Some(Circuit {
            length,
            use_via: Vec::new(),
            use_layer,
        }),
        shove_fixed: class.is_shove_fixed().then_some(true),
        pull_tight: (!class.pull_tight()).then_some(false),
    }
}

/// `Library.writePadstackScope`: the shapes from the first to the last layer
/// that has one, each followed by its auxiliary shapes, relative
/// coordinates, not rounded. `None` (with Java's warning) when no layer has
/// a shape.
pub(crate) fn padstack_scope(
    board: &BasicBoard,
    padstack: &Padstack,
    transform: &CoordinateTransform,
) -> Option<DsnPadstack> {
    let quote = string_quote(board);
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
        let layer_ref = LayerRef::Named(board.layer_structure.layers[layer as usize].name.clone());
        shapes.extend(board_shape_to_dsn(
            &shape.to_shape(),
            layer_ref.clone(),
            transform,
            true,
        ));
        for aux in padstack.get_auxiliary_shapes(layer).unwrap_or_default() {
            shapes.extend(board_shape_to_dsn(
                &aux.to_shape(),
                layer_ref.clone(),
                transform,
                true,
            ));
        }
    }
    Some(DsnPadstack {
        name: ident(&padstack.name, &quote),
        shapes,
        attach: (!padstack.attach_allowed).then_some(false),
        absolute: padstack.placed_absolute.then_some(true),
    })
}

/// `CoordinateTransform.boardToDsn(Shape, Layer)` and, with `relative`,
/// `boardToDsnRel`: a box becomes a rectangle, any other polyline shape a
/// polygon of its corners, a circle a circle. The coordinates are not
/// rounded.
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
