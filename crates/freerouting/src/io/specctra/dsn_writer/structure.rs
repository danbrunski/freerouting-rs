//! The structure scope: layers, boundaries, via list, rules, snap angle, control, planes and
//! keepouts.
//!
//! Port of `Structure.writeScope` and its helpers, `Layer.writeScope`, `Plane.writeScope` and
//! `Rule.writeDefaultRule` (in `rules.rs`).

use freerouting_dsn::ast::{
    Area as DsnArea, Boundary, Control, Keepout, KeepoutKind, Layer as DsnLayer, LayerRef,
    LayerType, Plane, Rule, SnapAngle, Structure, ViaPadstacks,
};
use std::collections::HashSet;

use freerouting_geometry::{IntBox, Shape};

use super::Cx;
use super::rules::ViaPlan;
use crate::board::items::{AreaKind, ItemKind};
use crate::board::structure::AngleRestriction;
use crate::rules::ItemClass;

/// The distance the DSN reader adds around the `pcb` rectangle (`Structure.createBoard`).
const READER_BOUNDS_OFFSET: i32 = 1000;

pub(super) fn structure(
    cx: &Cx,
    rules: Vec<Rule>,
    layer_rules: &[Vec<Rule>],
    via: Option<&ViaPlan>,
) -> Structure {
    let board = cx.board;
    let (planes, keepouts) = planes_and_keepouts(cx);
    Structure {
        layers: layers(cx, layer_rules),
        boundaries: boundaries(cx),
        via: Some(via_padstacks(cx, via)),
        rules,
        keepouts,
        planes,
        autoroute_settings: None,
        control: Some(Control {
            via_at_smd: Some(
                board
                    .rules
                    .via_infos
                    .iter()
                    .any(|(_, v)| v.attach_smd_allowed),
            ),
        }),
        flip_style_rotate_first: None,
        snap_angle: Some(match board.rules.trace_angle_restriction() {
            AngleRestriction::NinetyDegree => SnapAngle::NinetyDegree,
            AngleRestriction::FortyfiveDegree => SnapAngle::FortyfiveDegree,
            AngleRestriction::None => SnapAngle::None,
        }),
    }
}

/// `Structure.writeLayers`: a layer carries its own rule where its trace width or its
/// clearances differ from layer 0's.
fn layers(cx: &Cx, rules: &[Vec<Rule>]) -> Vec<DsnLayer> {
    let board = cx.board;
    let use_nets = power_layer_nets(cx);
    board
        .layer_structure
        .layers
        .iter()
        .enumerate()
        .map(|(i, layer)| DsnLayer {
            name: layer.name.clone(),
            layer_type: Some(if layer.is_signal {
                LayerType::Signal
            } else {
                LayerType::Power
            }),
            rules: rules.get(i).cloned().unwrap_or_default(),
            use_net: use_nets.get(i).cloned().unwrap_or_default(),
        })
        .collect()
}

/// The `use_net` of each power layer. Java writes none, so a re-read board loses which nets
/// have a plane (`contains_plane`). The nets of the conduction areas of a layer are listed on
/// it, and a plane net without an area on a power layer on the first power layer.
fn power_layer_nets(cx: &Cx) -> Vec<Vec<String>> {
    let board = cx.board;
    let mut result: Vec<Vec<String>> = vec![Vec::new(); board.layer_count()];
    let mut listed: HashSet<&str> = HashSet::new();
    for item in cx.items.iter().copied() {
        let ItemKind::Area(area) = &item.kind else {
            continue;
        };
        let layer = usize::try_from(area.layer()).unwrap_or(usize::MAX);
        if !matches!(area.kind, AreaKind::Conduction { .. })
            || board
                .layer_structure
                .layers
                .get(layer)
                .is_none_or(|l| l.is_signal)
        {
            continue;
        }
        for &number in item.net_numbers() {
            if let Some(net) = board.rules.nets.get(number)
                && net.contains_plane()
                && !result[layer].contains(&net.name)
            {
                result[layer].push(net.name.clone());
                listed.insert(net.name.as_str());
            }
        }
    }
    if let Some(first) = board
        .layer_structure
        .layers
        .iter()
        .position(|l| !l.is_signal)
    {
        for net in board.rules.nets.iter() {
            if net.contains_plane() && listed.insert(net.name.as_str()) {
                result[first].push(net.name.clone());
            }
        }
    }
    result
}

/// `Structure.writeBoundaries`: the bounding box on layer `pcb`, then each outline shape on
/// layer `signal` in a scope of its own.
fn boundaries(cx: &Cx) -> Vec<Boundary> {
    let board = cx.board;
    // The reader enlarges the box of the `pcb` rectangle by 1000, and Java writes the board's
    // box as it is, so every write and read grew the board by 1000 on each side.
    let bounds: IntBox = board
        .bounding_box()
        .offset(-f64::from(READER_BOUNDS_OFFSET));
    let t = &cx.transform;
    let [x1, y1] = t.board_to_dsn_point(bounds.ll.to_float());
    let [x2, y2] = t.board_to_dsn_point(bounds.ur.to_float());
    let outline_item = board.items().find(|i| i.as_board_outline().is_some());
    // The reader gives the outline the default area class unless a boundary names one.
    let outline_class = outline_item.map(|i| i.clearance_class()).and_then(|class| {
        (class != cx.default_class(ItemClass::Area)).then(|| cx.class_name(class))
    });
    let mut result = vec![Boundary {
        shapes: vec![freerouting_dsn::ast::Shape::Rect {
            layer: LayerRef::Pcb,
            x1,
            y1,
            x2,
            y2,
        }],
        clearance_class: outline_class,
    }];
    let Some(outline) = outline_item.and_then(|i| i.as_board_outline()) else {
        return result;
    };
    for shape in outline.shapes() {
        let board_shape = Shape::Polyline(shape.clone());
        if let Some(dsn) = super::board_shape_to_dsn(&board_shape, LayerRef::Signal, t, false) {
            result.push(Boundary {
                shapes: vec![dsn],
                clearance_class: None,
            });
        }
    }
    result
}

/// `Structure.writeViaPadstacks`.
fn via_padstacks(cx: &Cx, plan: Option<&ViaPlan>) -> ViaPadstacks {
    let library = &cx.board.library;
    let count = plan.map_or(usize::MAX, |p| p.structure_len);
    ViaPadstacks {
        padstacks: (0..library.via_padstack_count().min(count))
            .filter_map(|i| library.get_via_padstack(i))
            .map(|p| p.name.clone())
            .collect(),
        spare: Vec::new(),
    }
}

/// `Structure.writeConductionAreas` (the areas on power layers, as planes) and
/// `writeKeepouts`, in item list order.
fn planes_and_keepouts(cx: &Cx) -> (Vec<Plane>, Vec<Keepout>) {
    let board = cx.board;
    let mut planes = Vec::new();
    let mut keepouts = Vec::new();
    let default_area_class = cx.default_class(ItemClass::Area);
    for item in cx.items.iter().copied() {
        let ItemKind::Area(area) = &item.kind else {
            continue;
        };
        let layer = area.layer();
        let Some(board_layer) = usize::try_from(layer)
            .ok()
            .and_then(|l| board.layer_structure.layers.get(l))
        else {
            continue;
        };
        let layer_ref = LayerRef::Named(board_layer.name.clone());
        match area.kind {
            AreaKind::Conduction { .. } => {
                if item.net_count() != 1 {
                    continue;
                }
                let Some(net) = board.rules.nets.get(item.net_number(0)) else {
                    continue;
                };
                // Conduction areas on signal layers are written in the wiring scope, except
                // the fixed areas of plane nets: only a plane scope makes the reader mark the
                // net as a plane net.
                if !super::in_plane_scope(cx, item) {
                    continue;
                }
                let (Some(shape), windows) = cx.area_shapes(area.area(), layer_ref, false) else {
                    continue;
                };
                // The reader gives a plane the area class of its net's class.
                let reader_default = board
                    .rules
                    .net_class_of(net.net_number)
                    .map_or(default_area_class, |c| {
                        c.default_item_clearance_classes.get(ItemClass::Area)
                    });
                let class = item.clearance_class();
                planes.push(Plane {
                    net: net.name.clone(),
                    area: DsnArea {
                        name: None,
                        shape,
                        windows,
                        clearance_class: (class != reader_default).then(|| cx.class_name(class)),
                    },
                });
            }
            kind => {
                // Keepouts of a component are written with the component's package, and the
                // companion tiles of custom padstacks are not keepouts of their own.
                if item.component_id() != 0
                    || area.name.as_deref().is_some_and(|n| n.ends_with("_aux"))
                {
                    continue;
                }
                let (Some(shape), windows) = cx.area_shapes(area.area(), layer_ref, false) else {
                    continue;
                };
                let class = item.clearance_class();
                let java_writes = class > 0 && cx.class_name(class) != "default";
                let clearance_class =
                    (java_writes || class != default_area_class).then(|| cx.class_name(class));
                keepouts.push(Keepout {
                    kind: match kind {
                        AreaKind::ViaObstacle => KeepoutKind::ViaKeepout,
                        AreaKind::ComponentObstacle => KeepoutKind::PlaceKeepout,
                        _ => KeepoutKind::Keepout,
                    },
                    area: DsnArea {
                        name: None,
                        shape,
                        windows,
                        clearance_class,
                    },
                });
            }
        }
    }
    (planes, keepouts)
}
