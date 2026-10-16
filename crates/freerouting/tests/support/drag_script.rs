//! Replays a JSON drag script on a board (I02.5): the drag mode of
//! `examples/edit_script.rs`, with the JSON reader of `edit_script.rs`.
//!
//! A drag script is a script whose settings name the drag tool:
//!
//! ```json
//! {"settings": {"tool": "drag", "unfix": true},
//!  "commands": [{"cmd": "press", "x": 1275650, "y": -903250, "layer": 0},
//!               {"cmd": "drag_to", "x": 1275850, "y": -903250},
//!               {"cmd": "release"}]}
//! ```
//!
//! `press` starts a drag at the point (with the current layer `layer` if
//! given; a running drag is released first): an item drag, make-space, or
//! nothing; `drag_to` drags on; `release` ends the drag. A command without a
//! drag is reported as `"skip"`, and a failed step ends the drag. With
//! `"unfix": true` every user-fixed trace and via is unfixed before the first
//! command (the session import fixes them), as the `drag-script` probe does.
//! Each command gives one line (the outcome, the move vector or the route end,
//! and `maxGeneratedId`); the canonical item dump follows at the end.

#![allow(dead_code)]

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::FixedState;
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::{FloatPoint, Vector};
use freerouting::interactive::{
    DragBudget, DragRelease, DragSession, DragSettings, DragStep, EditSettings,
};

use crate::board_probe::{Line, bits};
use crate::edit_script::{Json, parse_json};
use crate::item_dump::{dump, point_text};

/// One drag command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DragCommand {
    Press { at: FloatPoint, layer: Option<i32> },
    DragTo(FloatPoint),
    Release,
}

/// A parsed drag script.
#[derive(Clone, Debug, PartialEq)]
pub struct DragScript {
    pub unfix: bool,
    pub commands: Vec<DragCommand>,
}

/// True if the script text names the drag tool in its settings.
pub fn is_drag_script(text: &str) -> bool {
    parse_json(text).is_ok_and(|json| {
        json.get("settings")
            .and_then(|s| s.get("tool"))
            .and_then(Json::as_str)
            == Some("drag")
    })
}

/// Parses a drag script (see the module docs).
pub fn parse_drag_script(text: &str) -> Result<DragScript, String> {
    let json = parse_json(text)?;
    let unfix = json
        .get("settings")
        .and_then(|s| s.get("unfix"))
        .and_then(Json::as_bool)
        .unwrap_or(false);
    let Some(Json::Array(list)) = json.get("commands") else {
        return Err("no \"commands\" array".to_string());
    };
    let mut commands = Vec::new();
    for (i, c) in list.iter().enumerate() {
        let num = |key: &str| {
            c.get(key)
                .and_then(Json::as_f64)
                .ok_or(format!("command {i}: no number \"{key}\""))
        };
        let point = || -> Result<FloatPoint, String> { Ok(FloatPoint::new(num("x")?, num("y")?)) };
        let command = match c.get("cmd").and_then(Json::as_str) {
            Some("press") => DragCommand::Press {
                at: point()?,
                layer: c.get("layer").and_then(Json::as_f64).map(|l| l as i32),
            },
            Some("drag_to") => DragCommand::DragTo(point()?),
            Some("release") => DragCommand::Release,
            other => return Err(format!("command {i}: unknown drag cmd {other:?}")),
        };
        commands.push(command);
    }
    Ok(DragScript { unfix, commands })
}

fn fpoint(p: FloatPoint) -> String {
    format!("f{},{}", bits(p.x), bits(p.y))
}

fn max_id(board: &RoutingBoard) -> i64 {
    i64::from(board.communication.id_generator.max_generated_id())
}

fn vector_text(v: &Vector) -> String {
    match v {
        Vector::Int(iv) => format!("{},{}", iv.x, iv.y),
        _ => fpoint(v.to_float()),
    }
}

fn join_ints(values: impl IntoIterator<Item = i32>) -> String {
    values
        .into_iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn unfix_wiring(board: &mut RoutingBoard) {
    let ids: Vec<ItemId> = board
        .items()
        .filter(|i| (i.is_trace() || i.is_via()) && i.fixed_state() == FixedState::UserFixed)
        .map(|i| i.id())
        .collect();
    for id in ids {
        board.set_fixed_state(id, FixedState::Unfixed);
    }
}

fn release_line(board: &mut RoutingBoard, session: DragSession, line: Line) -> Line {
    match session.release(board) {
        DragRelease::Item { changed_nets } => line
            .str("result", "released")
            .str("changed", &join_ints(changed_nets)),
        DragRelease::MakeSpace { removed } => line
            .str("result", "released")
            .num("removed", removed as i64),
    }
}

fn step_line(step: &DragStep, session: &DragSession, line: Line) -> Line {
    match step {
        DragStep::Unchanged => line.str("result", "unchanged"),
        DragStep::Fixed => line.str("result", "fixed"),
        DragStep::Blocked => line.str("result", "blocked"),
        DragStep::Moved { vector } => line
            .str("result", "moved")
            .str("vector", &vector_text(vector)),
        DragStep::Failed {
            vector,
            changed_nets,
        } => line
            .str("result", "failed")
            .str("vector", &vector_text(vector))
            .str("changed", &join_ints(changed_nets.iter().copied())),
        DragStep::MadeSpace { step, warp_pointer } => {
            let DragSession::MakeSpace(space) = session else {
                unreachable!("make-space step of an item drag");
            };
            let route = space.route();
            line.str("result", if step.ends_route() { "ended" } else { "space" })
                .str("lastCorner", &point_text(&route.last_corner().into()))
                .num(
                    "failing",
                    route
                        .shove_failing_obstacle()
                        .map_or(-1, |i| i64::from(i.0)),
                )
                .bool("warp", warp_pointer.is_some())
        }
    }
}

/// Replays `script` on `board` with `budget`; returns one line per command,
/// then the item dump.
pub fn replay_drag(
    board: &mut RoutingBoard,
    script: &DragScript,
    budget: DragBudget,
) -> Vec<String> {
    if script.unfix {
        unfix_wiring(board);
    }
    let mut settings = EditSettings::for_board(board);
    let drag = DragSettings::default();
    let mut session: Option<DragSession> = None;
    let mut lines = Vec::new();
    for (i, command) in script.commands.iter().enumerate() {
        let line = Line::new().num("command", i as i64);
        let line = match *command {
            DragCommand::Press { at, layer } => {
                let mut line = line.str("cmd", "press").str("at", &fpoint(at));
                if let Some(running) = session.take() {
                    line = release_line(board, running, line.str("released", "running"));
                }
                if let Some(layer) = layer {
                    settings.layer = layer;
                }
                session = DragSession::start(board, &settings, &drag, at);
                match &session {
                    Some(DragSession::Item(d)) => {
                        line.str("press", "item").num("item", i64::from(d.item().0))
                    }
                    Some(DragSession::MakeSpace(_)) => line.str("press", "makespace"),
                    None => line.str("press", "none"),
                }
            }
            DragCommand::DragTo(to) => {
                let line = line.str("cmd", "drag_to").str("to", &fpoint(to));
                match session.as_mut() {
                    None => line.str("result", "skip"),
                    Some(s) => {
                        let step = s.drag_to(board, &settings, to, budget);
                        let line = step_line(&step, s, line);
                        if step.ends_drag() {
                            session = None;
                        }
                        line
                    }
                }
            }
            DragCommand::Release => {
                let line = line.str("cmd", "release");
                match session.take() {
                    None => line.str("result", "skip"),
                    Some(s) => release_line(board, s, line),
                }
            }
        };
        lines.push(line.num("maxGeneratedId", max_id(board)).end());
    }
    lines.extend(dump(board));
    lines
}
