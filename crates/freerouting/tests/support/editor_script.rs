//! Replays a JSON editor script through [`Editor::apply`] (I03.8): the editor mode of
//! `examples/edit_script.rs`, with the JSON reader of `edit_script.rs`.
//!
//! An editor script is a script whose settings name the editor:
//!
//! ```json
//! {"settings": {"tool": "editor", "dsn_insert": "  (part_library ...)\n"},
//!  "commands": [{"cmd": "click", "x": 1084100, "y": -855500},
//!               {"cmd": "insert_keepout", "kind": "tile", "points": [[0, 0], [300, 0], [300, 300]]},
//!               {"cmd": "undo"}]}
//! ```
//!
//! `dsn_insert` (optional) is text put before the DSN's `(network` scope, the way the `editor`
//! probe adds a logical part. Every [`Command`] has a name:
//! - pointer: `press`, `move`, `drag_to`, `click` (`x`, `y`), `release`, `complete`, `cancel`;
//! - tools and layers: `set_tool` (`tool`: `route`, `drag`, `select`), `set_layer` (`layer`),
//!   `next_layer`, `previous_layer`, `signal_layer` (`digit`), `toggle_push`;
//! - settings: `update_settings` with any of `layer`, `push`, `stitch`, `all_layers`,
//!   `tidy_width`, `accuracy` (the others keep their value); `update_options` with any of
//!   `drag_components` and `filter` (an array of Java choice names, `FIXED`/`UNFIXED` included);
//! - history: `undo`, `redo`, `snapshot`;
//! - sessions: `start_route`, `start_pin_swap`, `select_at`, `toggle_select`, `start_hole`
//!   (`x`, `y`), `start_keepout` (`kind`: `tile`, `polygon`, `circle`; `x`, `y`);
//! - selection: `select_region` (`box`: `[llx, lly, urx, ury]`), `extend` (`extent`: `nets`,
//!   `components`, `connected_sets`, `connections`), `filter`, `delete`, `fix` (`fixed`);
//! - one-step constructions: `insert_keepout` (`kind`, `points`: the clicks; a circle takes
//!   `points[0]` and `radius`; `layer`, default 0), `add_hole` (`area`, `points`).
//!
//! Each command gives one line: the mode, the status key and arguments, the delta (inserted,
//! removed and changed ids, changed nets), the layer, the pointer warp, the selection and
//! `maxGeneratedId`. The canonical item dump, the pin nets and the area tiles follow at the end.

#![allow(dead_code)]

use freerouting::board::items::ItemId;
use freerouting::board::{ItemFilter, RoutingBoard, SelectableChoice};
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::{FloatPoint, IntBox};
use freerouting::interactive::select::Extent;
use freerouting::interactive::{
    Command, EditSettings, Editor, EditorOptions, KeepoutKind, KeepoutShape, ModeKind, Response,
    Tool,
};

use crate::board_probe::{Line, bits};
use crate::edit_script::{Json, parse_json};
use crate::item_dump::dump;

/// A parsed editor script.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorScript {
    /// Text to put before the DSN's `(network` scope.
    pub dsn_insert: Option<String>,
    pub commands: Vec<(String, Command)>,
}

/// True if the script text names the editor in its settings.
pub fn is_editor_script(text: &str) -> bool {
    parse_json(text).is_ok_and(|json| {
        json.get("settings")
            .and_then(|s| s.get("tool"))
            .and_then(Json::as_str)
            == Some("editor")
    })
}

/// The script's `dsn_insert` setting.
pub fn dsn_insert(text: &str) -> Option<String> {
    parse_json(text).ok().and_then(|json| {
        json.get("settings")
            .and_then(|s| s.get("dsn_insert"))
            .and_then(Json::as_str)
            .map(str::to_string)
    })
}

/// `text` with `insert` before its `(network` scope.
pub fn insert_before_network(text: &str, insert: &str) -> Result<String, String> {
    let at = text
        .find("  (network")
        .ok_or("the DSN has no \"  (network\" scope")?;
    Ok(format!("{}{}{}", &text[..at], insert, &text[at..]))
}

fn points_of(c: &Json, i: usize) -> Result<Vec<FloatPoint>, String> {
    let Some(Json::Array(list)) = c.get("points") else {
        return Err(format!("command {i}: no \"points\" array"));
    };
    list.iter()
        .map(|p| match p {
            Json::Array(xy) if xy.len() == 2 => match (xy[0].as_f64(), xy[1].as_f64()) {
                (Some(x), Some(y)) => Ok(FloatPoint::new(x, y)),
                _ => Err(format!("command {i}: a point is not two numbers")),
            },
            _ => Err(format!("command {i}: a point is not [x, y]")),
        })
        .collect()
}

fn choice(name: &str) -> Option<SelectableChoice> {
    SelectableChoice::ALL
        .into_iter()
        .find(|c| c.java_name() == name)
}

fn keepout_kind(name: &str, i: usize) -> Result<KeepoutKind, String> {
    match name {
        "tile" => Ok(KeepoutKind::Tile),
        "polygon" => Ok(KeepoutKind::Polygon),
        "circle" => Ok(KeepoutKind::Circle),
        _ => Err(format!("command {i}: unknown keepout kind {name:?}")),
    }
}

/// Parses an editor script (see the module docs). Settings and options updates start from the
/// defaults of `layer_count` layers and the previous update.
pub fn parse_editor_script(text: &str, layer_count: usize) -> Result<EditorScript, String> {
    let json = parse_json(text)?;
    let dsn_insert = json
        .get("settings")
        .and_then(|s| s.get("dsn_insert"))
        .and_then(Json::as_str)
        .map(str::to_string);
    let Some(Json::Array(list)) = json.get("commands") else {
        return Err("no \"commands\" array".to_string());
    };
    let mut settings = EditSettings::new(layer_count);
    let mut options = EditorOptions::default();
    let mut commands = Vec::new();
    for (i, c) in list.iter().enumerate() {
        let num = |key: &str| {
            c.get(key)
                .and_then(Json::as_f64)
                .ok_or(format!("command {i}: no number \"{key}\""))
        };
        let text = |key: &str| {
            c.get(key)
                .and_then(Json::as_str)
                .ok_or(format!("command {i}: no string \"{key}\""))
        };
        let point = || -> Result<FloatPoint, String> { Ok(FloatPoint::new(num("x")?, num("y")?)) };
        let name = text("cmd")?.to_string();
        let command = match name.as_str() {
            "press" => Command::Press(point()?),
            "move" => Command::Move(point()?),
            "drag_to" => Command::DragTo(point()?),
            "click" => Command::Click(point()?),
            "release" => Command::Release,
            "complete" => Command::Complete,
            "cancel" => Command::Cancel,
            "set_tool" => Command::SetTool(match text("tool")? {
                "route" => Tool::Route,
                "drag" => Tool::Drag,
                "select" => Tool::Select,
                other => return Err(format!("command {i}: unknown tool {other:?}")),
            }),
            "set_layer" => Command::SetLayer(num("layer")? as i32),
            "next_layer" => Command::NextSignalLayer,
            "previous_layer" => Command::PreviousSignalLayer,
            "signal_layer" => Command::SignalLayer(num("digit")? as u32),
            "toggle_push" => Command::TogglePush,
            "update_settings" => {
                if let Ok(layer) = num("layer") {
                    settings.layer = layer as i32;
                }
                let flag = |key: &str| c.get(key).and_then(Json::as_bool);
                if let Some(v) = flag("push") {
                    settings.push_enabled = v;
                }
                if let Some(v) = flag("stitch") {
                    settings.stitch_route = v;
                }
                if let Some(v) = flag("all_layers") {
                    settings.select_on_all_visible_layers = v;
                }
                if let Ok(v) = num("tidy_width") {
                    settings.trace_tidy_width = v as i32;
                }
                if let Ok(v) = num("accuracy") {
                    settings.pull_tight_accuracy = v as i32;
                }
                Command::UpdateSettings(settings.clone())
            }
            "update_options" => {
                if let Some(v) = c.get("drag_components").and_then(Json::as_bool) {
                    options.drag.drag_components_enabled = v;
                }
                if let Some(Json::Array(names)) = c.get("filter") {
                    let mut filter = ItemFilter::none();
                    for n in names {
                        let n = n.as_str().unwrap_or_default();
                        let c = choice(n).ok_or(format!("command {i}: unknown choice {n:?}"))?;
                        filter = filter.with(c, true);
                    }
                    options.selection_filter = filter;
                }
                Command::UpdateOptions(options.clone())
            }
            "undo" => Command::Undo,
            "redo" => Command::Redo,
            "snapshot" => Command::Snapshot,
            "start_route" => Command::StartRoute { at: point()? },
            "start_pin_swap" => Command::StartPinSwap { at: point()? },
            "select_at" => Command::SelectAt(point()?),
            "toggle_select" => Command::ToggleSelect(point()?),
            "start_hole" => Command::StartHole { at: point()? },
            "start_keepout" => Command::StartKeepout {
                kind: keepout_kind(text("kind")?, i)?,
                at: point()?,
            },
            "select_region" => {
                let Some(Json::Array(b)) = c.get("box") else {
                    return Err(format!("command {i}: no \"box\""));
                };
                let v: Vec<i32> = b
                    .iter()
                    .filter_map(Json::as_f64)
                    .map(|v| v as i32)
                    .collect();
                if v.len() != 4 {
                    return Err(format!("command {i}: a box is four numbers"));
                }
                Command::SelectRegion(IntBox::new(v[0], v[1], v[2], v[3]))
            }
            "extend" => Command::ExtendSelection(match text("extent")? {
                "nets" => Extent::Nets,
                "components" => Extent::Components,
                "connected_sets" => Extent::ConnectedSets,
                "connections" => Extent::Connections,
                other => return Err(format!("command {i}: unknown extent {other:?}")),
            }),
            "filter" => Command::FilterSelection,
            "delete" => Command::DeleteSelection,
            "fix" => Command::FixSelection(c.get("fixed").and_then(Json::as_bool).unwrap_or(true)),
            "insert_keepout" => {
                let points = points_of(c, i)?;
                let shape = match keepout_kind(text("kind")?, i)? {
                    KeepoutKind::Tile => KeepoutShape::Tile(points),
                    KeepoutKind::Polygon => KeepoutShape::Polygon(points),
                    KeepoutKind::Circle => KeepoutShape::Circle {
                        center: *points
                            .first()
                            .ok_or(format!("command {i}: a circle needs its centre"))?,
                        radius: num("radius")?,
                    },
                };
                Command::InsertKeepout {
                    shape,
                    layer: num("layer").unwrap_or(0.0) as i32,
                }
            }
            "add_hole" => Command::AddHole {
                area: ItemId(num("area")? as i32),
                corners: points_of(c, i)?,
            },
            other => return Err(format!("command {i}: unknown command {other:?}")),
        };
        commands.push((name, command));
    }
    Ok(EditorScript {
        dsn_insert,
        commands,
    })
}

fn mode_text(mode: ModeKind) -> String {
    match mode {
        ModeKind::Menu(Tool::Route) => "route_menu".into(),
        ModeKind::Menu(Tool::Drag) => "drag_menu".into(),
        ModeKind::Menu(Tool::Select) => "select_menu".into(),
        ModeKind::Routing { stitch } => format!("routing{}", if stitch { "_stitch" } else { "" }),
        ModeKind::Dragging => "dragging".into(),
        ModeKind::MakingSpace => "making_space".into(),
        ModeKind::Selecting => "selecting".into(),
        ModeKind::SelectingRegion => "selecting_region".into(),
        ModeKind::PinSwap => "pin_swap".into(),
        ModeKind::Constructing => "constructing".into(),
    }
}

fn ids(list: impl IntoIterator<Item = ItemId>) -> String {
    list.into_iter()
        .map(|id| id.0.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn response_line(i: usize, name: &str, response: &Response, board: &RoutingBoard) -> String {
    let status = response.status.as_ref().map_or("-".to_string(), |s| {
        std::iter::once(s.key.to_string())
            .chain(s.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    });
    let nets = response
        .delta
        .changed_nets
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let warp = response
        .warp_pointer
        .map_or("-".to_string(), |p| format!("{},{}", bits(p.x), bits(p.y)));
    Line::new()
        .num("command", i as i64)
        .str("cmd", name)
        .str("mode", &mode_text(response.mode))
        .str("status", &status)
        .str("inserted", &ids(response.delta.inserted.iter().copied()))
        .str("removed", &ids(response.delta.removed.iter().copied()))
        .str("changed", &ids(response.delta.changed.iter().copied()))
        .str("nets", &nets)
        .num("layer", i64::from(response.layer))
        .str("warp", &warp)
        .str(
            "selection",
            &ids(response.overlay.selection.iter().copied()),
        )
        .num(
            "maxGeneratedId",
            i64::from(board.communication.id_generator.max_generated_id()),
        )
        .end()
}

/// The pin nets and the areas (id, layer, bounding box) of `board`, highest id first.
fn board_lines(board: &RoutingBoard) -> Vec<String> {
    let pins = board
        .items()
        .filter(|i| i.is_pin())
        .map(|i| {
            let nets: Vec<String> = i.net_numbers().iter().map(|n| n.to_string()).collect();
            format!("{}:{}", i.id().0, nets.join(","))
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut lines = vec![Line::new().str("pins", &pins).end()];
    for item in board.items() {
        if let Some(area) = item.as_area() {
            let b = item.bounding_box();
            lines.push(
                Line::new()
                    .num("area", i64::from(item.id().0))
                    .num("layer", i64::from(area.layer()))
                    .str(
                        "box",
                        &format!("{} {} {} {}", b.ll.x, b.ll.y, b.ur.x, b.ur.y),
                    )
                    .num("tiles", board.tile_shape_count(item) as i64)
                    .end(),
            );
        }
    }
    lines
}

/// Replays `script` on `board` with `options`; one line per command, then the board.
pub fn replay_editor(
    board: RoutingBoard,
    script: &EditorScript,
    options: EditorOptions,
) -> Vec<String> {
    let settings = EditSettings::for_board(&board);
    let mut editor = Editor::new(board, settings);
    editor.apply(Command::UpdateOptions(options.clone()));
    let mut lines = Vec::new();
    for (i, (name, command)) in script.commands.iter().enumerate() {
        let command = match command {
            // keep the budgets of the run
            Command::UpdateOptions(o) => Command::UpdateOptions(EditorOptions {
                route_budget: options.route_budget,
                drag_budget: options.drag_budget,
                ..o.clone()
            }),
            other => other.clone(),
        };
        let response = editor.apply(command);
        lines.push(response_line(i, name, &response, editor.board()));
    }
    let board = editor.into_board();
    lines.extend(dump(&board));
    lines.extend(board_lines(&board));
    lines
}
