//! Replays a JSON edit script on a board (I01.8): the shared part of
//! `examples/edit_script.rs` and `tests/interactive_route.rs`.
//!
//! A script is one JSON object:
//!
//! ```json
//! {"settings": {"push": true, "stitch": false},
//!  "commands": [{"cmd": "start", "x": 1275650, "y": -903250, "layer": 0},
//!               {"cmd": "move", "x": 1275650, "y": -901250},
//!               {"cmd": "click", "x": 1275650, "y": -899250},
//!               {"cmd": "layer", "to": 1},
//!               {"cmd": "snapshot"}, {"cmd": "complete"}, {"cmd": "cancel"}]}
//! ```
//!
//! `start` begins a route at the point (with the current layer `layer` if
//! given; a running route is completed first); `move`, `click`, `snapshot`
//! and `complete` are the session's inputs in its mode; `layer` changes the
//! route's layer; `cancel` removes the trace tail and ends the route. A
//! command without a route is reported as `"skip"`. Each command gives one
//! line (the outcome, the route's and the current layer, the last corner, the
//! shove-failing obstacle, the nearest target and `maxGeneratedId`); the
//! canonical item dump of the board follows at the end.
//!
//! The parser reads the JSON this format needs (objects, arrays, strings
//! without escapes other than `\"` and `\\`, numbers, booleans, `null`); the
//! engine has no JSON dependency.

#![allow(dead_code)]

use std::collections::BTreeMap;

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::FloatPoint;
use freerouting::interactive::{
    EditSettings, LayerChange, RouteBudget, RouteEvent, RouteInput, RouteSession, RouteStep,
    StartError,
};

use crate::board_probe::{Line, bits};
use crate::item_dump::{dump, point_text};

/// A JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Str(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(map) => map.get(key),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// Parses `text` as one JSON value.
pub fn parse_json(text: &str) -> Result<Json, String> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        pos: 0,
    };
    let value = parser.value()?;
    parser.skip_ws();
    if parser.pos != parser.bytes.len() {
        return Err(format!("trailing characters at byte {}", parser.pos));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn expect(&mut self, c: u8) -> Result<(), String> {
        self.skip_ws();
        if self.bytes.get(self.pos) == Some(&c) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected '{}' at byte {}", c as char, self.pos))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.skip_ws();
        match self.bytes.get(self.pos) {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(_) => self.number(),
            None => Err("unexpected end".to_string()),
        }
    }

    fn literal(&mut self, word: &str, value: Json) -> Result<Json, String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(value)
        } else {
            Err(format!("bad literal at byte {}", self.pos))
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        while self.pos < self.bytes.len()
            && matches!(
                self.bytes[self.pos],
                b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
            )
        {
            self.pos += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|e| e.to_string())?;
        text.parse::<f64>()
            .map(Json::Number)
            .map_err(|_| format!("bad number '{text}' at byte {start}"))
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = Vec::new();
        while let Some(&c) = self.bytes.get(self.pos) {
            self.pos += 1;
            match c {
                b'"' => return String::from_utf8(out).map_err(|e| e.to_string()),
                b'\\' => {
                    let escaped = *self.bytes.get(self.pos).ok_or("unexpected end")?;
                    self.pos += 1;
                    out.push(escaped);
                }
                _ => out.push(c),
            }
        }
        Err("unterminated string".to_string())
    }

    fn array(&mut self) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.bytes.get(self.pos) == Some(&b']') {
            self.pos += 1;
            return Ok(Json::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.bytes.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(format!("expected ',' or ']' at byte {}", self.pos)),
            }
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.bytes.get(self.pos) == Some(&b'}') {
            self.pos += 1;
            return Ok(Json::Object(map));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.expect(b':')?;
            map.insert(key, self.value()?);
            self.skip_ws();
            match self.bytes.get(self.pos) {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Json::Object(map));
                }
                _ => return Err(format!("expected ',' or '}}' at byte {}", self.pos)),
            }
        }
    }
}

/// One script command.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Start { at: FloatPoint, layer: Option<i32> },
    Move(FloatPoint),
    Click(FloatPoint),
    Layer(i32),
    Snapshot,
    Complete,
    Cancel,
}

/// A parsed script: the settings switches and the commands.
#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    pub push: bool,
    pub stitch: bool,
    pub commands: Vec<Command>,
}

/// Parses a script (see the module docs).
pub fn parse_script(text: &str) -> Result<Script, String> {
    let json = parse_json(text)?;
    let settings = json.get("settings");
    let flag = |key: &str, default: bool| {
        settings
            .and_then(|s| s.get(key))
            .and_then(Json::as_bool)
            .unwrap_or(default)
    };
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
            Some("start") => Command::Start {
                at: point()?,
                layer: c.get("layer").and_then(Json::as_f64).map(|l| l as i32),
            },
            Some("move") => Command::Move(point()?),
            Some("click") => Command::Click(point()?),
            Some("layer") => Command::Layer(num("to")? as i32),
            Some("snapshot") => Command::Snapshot,
            Some("complete") => Command::Complete,
            Some("cancel") => Command::Cancel,
            other => return Err(format!("command {i}: unknown cmd {other:?}")),
        };
        commands.push(command);
    }
    Ok(Script {
        push: flag("push", true),
        stitch: flag("stitch", false),
        commands,
    })
}

fn fpoint(p: FloatPoint) -> String {
    format!("f{},{}", bits(p.x), bits(p.y))
}

fn opt_id(id: Option<ItemId>) -> i64 {
    id.map_or(-1, |i| i64::from(i.0))
}

fn max_id(board: &RoutingBoard) -> i64 {
    i64::from(board.communication.id_generator.max_generated_id())
}

fn step_text(step: &RouteStep) -> &'static str {
    match step {
        RouteStep::Rejected => "rejected",
        RouteStep::Blocked { .. } => "blocked",
        RouteStep::Advanced => "advanced",
        RouteStep::Completed => "completed",
        RouteStep::DatabaseDamaged { .. } => "damaged",
    }
}

fn start_error_text(e: StartError) -> &'static str {
    match e {
        StartError::NothingPicked => "nothing",
        StartError::NoNets => "nonets",
        StartError::ConnectToTraceFailed => "connect",
        StartError::UnknownNet => "unknown-net",
    }
}

/// One route command on a running session: its line and whether the route
/// ended.
fn command_line(
    board: &mut RoutingBoard,
    s: &mut RouteSession,
    command: Command,
    line: Line,
    budget: RouteBudget,
) -> (Line, bool) {
    if let Command::Layer(to) = command {
        let (change, _) = s.change_layer(board, to, budget);
        let text = match change {
            LayerChange::Ignored => "ignored",
            LayerChange::Changed { via: true } => "via",
            LayerChange::Changed { via: false } => "changed",
            LayerChange::PlaneReached => "plane",
            LayerChange::Failed { .. } => "failed",
        };
        let line = line
            .str("cmd", "layer")
            .num("to", i64::from(to))
            .str("result", text);
        return (line, change.ends_route());
    }
    let (name, input) = match command {
        Command::Move(p) => ("move", RouteInput::Move(p)),
        Command::Click(p) => ("click", RouteInput::Click(p)),
        Command::Snapshot => ("snapshot", RouteInput::Snapshot),
        _ => ("complete", RouteInput::Complete),
    };
    let event = s.handle(board, input, budget);
    let text = match &event {
        RouteEvent::Step(step) => step_text(step),
        RouteEvent::TargetUpdated => "target",
        RouteEvent::SnapshotTaken => "snapshot",
        RouteEvent::Ended => "ended",
        RouteEvent::Ignored => "ignored",
    };
    (
        line.str("cmd", name).str("result", text),
        event.ends_route(),
    )
}

/// Replays `script` on `board` with `budget`; returns one line per
/// command, then the item dump.
pub fn replay(board: &mut RoutingBoard, script: &Script, budget: RouteBudget) -> Vec<String> {
    let mut settings = EditSettings::for_board(board);
    settings.push_enabled = script.push;
    settings.stitch_route = script.stitch;
    let mut session: Option<RouteSession> = None;
    let mut lines = Vec::new();
    for (i, command) in script.commands.iter().enumerate() {
        let line = Line::new().num("command", i as i64);
        let (line, ended) = match *command {
            Command::Start { at, layer } => {
                if let Some(s) = &session {
                    settings.layer = s.current_layer();
                }
                if let Some(layer) = layer {
                    settings.layer = layer;
                }
                let line = line.str("cmd", "start").str("at", &fpoint(at));
                match RouteSession::start(board, &settings, at) {
                    Ok((started, info)) => {
                        session = Some(started);
                        let line = line
                            .str("result", "ok")
                            .num("item", i64::from(info.item.0))
                            .str("point", &point_text(&info.start.into()));
                        (line, false)
                    }
                    Err(e) => {
                        session = None;
                        (line.str("result", start_error_text(e)), false)
                    }
                }
            }
            Command::Cancel => match session.take() {
                Some(s) => {
                    settings.layer = s.current_layer();
                    s.cancel(board, &settings);
                    (line.str("cmd", "cancel").str("result", "cancelled"), false)
                }
                None => (line.str("cmd", "cancel").str("result", "skip"), false),
            },
            input => match session.as_mut() {
                None => (line.str("result", "skip"), false),
                Some(s) => command_line(board, s, input, line, budget),
            },
        };
        let line = match &session {
            Some(s) => line
                .num("layer", i64::from(s.layer()))
                .num("current", i64::from(s.current_layer()))
                .str("lastCorner", &point_text(&s.last_corner().into()))
                .num("failing", opt_id(s.shove_failing_obstacle()))
                .num("nearest", opt_id(s.nearest_target_item())),
            None => line,
        };
        lines.push(line.num("maxGeneratedId", max_id(board)).end());
        if ended {
            if let Some(s) = &session {
                settings.layer = s.current_layer();
            }
            session = None;
        }
    }
    lines.extend(dump(board));
    lines
}
