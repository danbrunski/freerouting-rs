//! The Rust side of probe `transform` (I04): the move and copy script of
//! `tools/java-reference/probes/board/TransformProbe.java`, driven through
//! `MoveSession` and `CopySession`. Its header comment describes the script
//! and the lines.

#![allow(dead_code)]

use std::collections::BTreeSet;

use freerouting::board::RoutingBoard;
use freerouting::board::items::{AreaKind, Item, ItemId, ItemKind};
use freerouting::board::structure::{ComponentId, FixedState};
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{FloatPoint, IntBox, IntPoint, Point, PolylineShapeOps};
use freerouting::interactive::{CopySession, MoveSession};

use crate::board_probe::{FNV_OFFSET, Line, bits, fnv1a64, hex};

const SEED: i64 = 20261016;
const MOVE_TRIALS: usize = 12;
const FIXED_TRIALS: usize = 4;
const COPY_TRIALS: usize = 8;
const ANGLES: [f64; 5] = [45.0, -45.0, 30.0, 135.0, 12.5];

pub fn point_text(p: &Point) -> String {
    match p {
        Point::Int(ip) => format!("{},{}", ip.x, ip.y),
        _ => {
            let f = p.to_float();
            format!("f{},{}", bits(f.x), bits(f.y))
        }
    }
}

fn box_text(b: &IntBox) -> String {
    format!("{},{},{},{}", b.ll.x, b.ll.y, b.ur.x, b.ur.y)
}

fn java_type(item: &Item) -> &'static str {
    match &item.kind {
        ItemKind::Pin(_) => "Pin",
        ItemKind::Via(_) => "Via",
        ItemKind::Trace(_) => "PolylineTrace",
        ItemKind::Area(area) => match area.kind {
            AreaKind::Obstacle => "ObstacleArea",
            AreaKind::ViaObstacle => "ViaObstacleArea",
            AreaKind::ComponentObstacle => "ComponentObstacleArea",
            AreaKind::Conduction { .. } => "ConductionArea",
        },
        ItemKind::ComponentOutline(_) => "ComponentOutline",
        ItemKind::BoardOutline(_) => "BoardOutline",
    }
}

fn fixed_ordinal(state: FixedState) -> i32 {
    match state {
        FixedState::Unfixed => 0,
        FixedState::ShoveFixed => 1,
        FixedState::UserFixed => 2,
        FixedState::SystemFixed => 3,
    }
}

/// The probe's `itemText` (without the final newline), as `ItemOpsProbe` plus ` k<component>`.
pub fn item_text(item: &Item) -> String {
    let nets: Vec<String> = item.net_numbers().iter().map(i32::to_string).collect();
    let mut text = format!(
        "{} {} n{} f{} c{}",
        item.id().0,
        java_type(item),
        nets.join(","),
        fixed_ordinal(item.fixed_state()),
        item.clearance_class()
    );
    match &item.kind {
        ItemKind::Pin(_) | ItemKind::Via(_) => {
            let drill = item.drill().expect("drill item");
            text.push_str(&format!(
                " {} l{}-{} {}",
                point_text(&Point::Int(drill.center)),
                drill.first_layer,
                drill.last_layer,
                box_text(&drill.bounding_box())
            ));
        }
        ItemKind::Trace(trace) => {
            text.push_str(&format!(" l{} w{}", trace.layer, trace.half_width));
            for i in 0..trace.corner_count() {
                text.push(' ');
                text.push_str(&point_text(&trace.polyline.corner(i)));
            }
        }
        ItemKind::Area(area) => {
            text.push_str(&format!(
                " l{} {}",
                area.layer(),
                box_text(&item.bounding_box())
            ));
        }
        ItemKind::ComponentOutline(_) | ItemKind::BoardOutline(_) => {
            text.push(' ');
            text.push_str(&box_text(&item.bounding_box()));
        }
    }
    text.push_str(&format!(" k{}", item.component_id()));
    text
}

fn ids(list: impl IntoIterator<Item = ItemId>) -> String {
    list.into_iter()
        .map(|id| id.0.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn join<T: ToString>(list: impl IntoIterator<Item = T>) -> String {
    list.into_iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// The probe's `components()`.
pub fn components_text(board: &RoutingBoard) -> String {
    board
        .components
        .iter()
        .map(|c| {
            let location = c
                .exact_location()
                .map_or("-".to_string(), |p| format!("{},{}", bits(p.x), bits(p.y)));
            format!(
                "{}|{}|{}|{}",
                c.name,
                location,
                bits(c.rotation_in_degree()),
                if c.placed_on_front() { 'F' } else { 'B' }
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

pub fn items_hash(board: &RoutingBoard) -> u64 {
    board.items().fold(FNV_OFFSET, |hash, item| {
        fnv1a64(hash, &format!("{}\n", item_text(item)))
    })
}

fn max_id(board: &RoutingBoard) -> i64 {
    i64::from(board.communication.id_generator.max_generated_id())
}

/// The probe's `unfixWiring`.
pub fn unfix_wiring(board: &mut RoutingBoard) {
    let wiring: Vec<ItemId> = board
        .items()
        .filter(|i| (i.is_trace() || i.is_via()) && i.fixed_state() == FixedState::UserFixed)
        .map(Item::id)
        .collect();
    for id in wiring {
        board.set_fixed_state(id, FixedState::Unfixed);
    }
}

fn origin_of(item: &Item) -> FloatPoint {
    match item.drill() {
        Some(drill) => drill.center.to_float(),
        None => item.bounding_box().centre_of_gravity(),
    }
}

/// The probe's script on one board.
pub struct Script<'a> {
    pub board: &'a mut RoutingBoard,
    pub stage: &'a str,
    pub random: &'a mut JavaRandom,
    pub lines: Vec<String>,
}

impl Script<'_> {
    fn line(&self, phase: &str, name: &str, trial: usize) -> Line {
        Line::new()
            .str("stage", self.stage)
            .str("phase", phase)
            .num(name, trial as i64)
    }

    pub fn run(&mut self) {
        for k in 0..MOVE_TRIALS {
            if k == FIXED_TRIALS {
                unfix_wiring(self.board);
            }
            self.move_trial(k);
        }
        for k in 0..COPY_TRIALS {
            self.copy_trial(k);
        }
    }

    /// The default tree's fingerprint (as `board_probe`'s tree lines).
    fn tree_line(&mut self, phase: &str, trial: usize) {
        let tree = self.board.search_trees().default_tree();
        let fingerprint = tree.fingerprint();
        let line = self
            .line(phase, "trial", trial)
            .str("tree", &tree.key())
            .num("nodes", fingerprint.nodes as i64)
            .num("leaves", fingerprint.leaves as i64)
            .num("maxDepth", fingerprint.max_depth as i64)
            .str("shapeHash", &hex(fingerprint.shape_hash))
            .str("contentDigest", &hex(tree.content_digest()))
            .end();
        self.lines.push(line);
    }

    fn pins_of(&self, component: i32) -> Vec<ItemId> {
        self.board
            .items()
            .filter(|i| i.is_pin() && i.component_id() == component)
            .map(Item::id)
            .collect()
    }

    fn move_trial(&mut self, k: usize) {
        let kind = self.random.next_int(10);
        let mut picked: Option<ItemId> = None;
        if kind < 7 {
            let count = self.board.components.count() as i32;
            if count > 0 {
                let component = 1 + self.random.next_int(count);
                let pins = self.pins_of(component);
                if !pins.is_empty() {
                    picked = Some(pins[self.random.next_int(pins.len() as i32) as usize]);
                }
            }
        } else {
            let wiring: Vec<ItemId> = self
                .board
                .items()
                .filter(|i| i.is_via() || i.is_trace())
                .map(Item::id)
                .collect();
            if !wiring.is_empty() {
                picked = Some(wiring[self.random.next_int(wiring.len() as i32) as usize]);
            }
        }
        let Some(picked) = picked else {
            let line = self
                .line("move-start", "trial", k)
                .str("result", "skip")
                .end();
            self.lines.push(line);
            return;
        };
        let grid = if k % 4 == 3 { 500 } else { 0 };
        let at = origin_of(self.board.item(picked).expect("picked item"));
        let selection = BTreeSet::from([picked]);
        let started = MoveSession::start(self.board, &selection, (grid, grid), at);
        let (mut session, status, select) = match started {
            Ok(session) => (Some(session), "-".to_string(), String::new()),
            Err(refusal) => (
                None,
                refusal.status.key.to_string(),
                ids(refusal.select.iter().copied()),
            ),
        };
        let floating = session
            .as_ref()
            .map_or(String::new(), |s| ids(s.items().map(Item::id)));
        let line = self
            .line("move-start", "trial", k)
            .num("grid", i64::from(grid))
            .str("selection", &ids(selection.iter().copied()))
            .str("at", &point_text(&Point::Int(at.round())))
            .str("result", if session.is_some() { "ok" } else { "refused" })
            .str("status", &status)
            .str("select", &select)
            .str("floating", &floating)
            .num("maxGeneratedId", max_id(self.board))
            .end();
        self.lines.push(line);
        let Some(session) = session.as_mut() else {
            return;
        };
        let operations = 3 + self.random.next_int(4);
        for n in 0..operations {
            let kind = self.random.next_int(7);
            let (what, status) = match kind {
                0 | 1 | 6 => {
                    let span = if kind == 6 { 60001 } else { 6001 };
                    let dx = self.random.next_int(span) - span / 2;
                    let dy = self.random.next_int(span) - span / 2;
                    let now = session.position();
                    session.move_to(
                        self.board,
                        FloatPoint::new(f64::from(now.x + dx), f64::from(now.y + dy)),
                    );
                    (format!("M {dx} {dy}"), None)
                }
                2 => {
                    let factor = 1 + self.random.next_int(3);
                    (
                        format!("T {factor}"),
                        session.turn_90_degree(self.board, factor),
                    )
                }
                3 => {
                    let angle = ANGLES[self.random.next_int(ANGLES.len() as i32) as usize];
                    (
                        format!("R {}", bits(angle)),
                        session.rotate(self.board, angle),
                    )
                }
                4 => ("X".to_string(), session.change_placement_side(self.board)),
                _ => ("Z".to_string(), session.reset_rotation(self.board)),
            };
            let items: Vec<String> = session.items().map(item_text).collect();
            let line = self
                .line("move-op", "trial", k)
                .num("op", i64::from(n))
                .str("what", &what)
                .str("status", status.as_ref().map_or("-", |s| s.key))
                .num("violations", session.violations().len() as i64)
                .str("nets", &join(session.nets().iter().copied()))
                .str("position", &point_text(&Point::Int(session.position())))
                .str("items", &items.join(" | "))
                .str("components", &components_text(self.board))
                .num("maxGeneratedId", max_id(self.board))
                .end();
            self.lines.push(line);
            self.tree_line("move-op-tree", k);
        }
        let (how, status) = if self.random.next_boolean() {
            let outcome = session.complete(self.board);
            if outcome.done {
                ("complete", "-".to_string())
            } else {
                let status = outcome.status.key.to_string();
                session.clone().cancel(self.board);
                ("blocked-cancel", status)
            }
        } else {
            session.clone().cancel(self.board);
            ("cancel", "-".to_string())
        };
        let line = self
            .line("move-end", "trial", k)
            .str("how", how)
            .str("status", &status)
            .str("itemsHash", &hex(items_hash(self.board)))
            .num("items", self.board.items().count() as i64)
            .str("components", &components_text(self.board))
            .num("maxGeneratedId", max_id(self.board))
            .end();
        self.lines.push(line);
        self.tree_line("move-tree", k);
    }

    fn copy_trial(&mut self, k: usize) {
        let (mut pins, mut vias, mut keepouts) = (Vec::new(), Vec::new(), Vec::new());
        for item in self.board.items() {
            if item.is_pin() {
                pins.push(item.id());
            } else if item.is_via() {
                vias.push(item.id());
            } else if item.as_area().is_some() && !item.is_conduction_area() {
                keepouts.push(item.id());
            }
        }
        let mut selection: BTreeSet<ItemId> = BTreeSet::new();
        let picks = 1 + self.random.next_int(3);
        for _ in 0..picks {
            let kind = self.random.next_int(10);
            let from = if kind < 6 {
                &pins
            } else if kind < 8 {
                &vias
            } else {
                &keepouts
            };
            if !from.is_empty() {
                selection.insert(from[self.random.next_int(from.len() as i32) as usize]);
            }
        }
        let Some(first) = selection.iter().next().copied() else {
            let line = self
                .line("copy-start", "trial", k)
                .str("result", "skip")
                .end();
            self.lines.push(line);
            return;
        };
        let at = origin_of(self.board.item(first).expect("first item"));
        let mut session = CopySession::start(self.board, &selection, 0, at).expect("selection");
        let line = self
            .line("copy-start", "trial", k)
            .str("selection", &ids(selection.iter().copied()))
            .str("at", &point_text(&Point::Int(at.round())))
            .str("preview", &ids(session.items().iter().map(Item::id)))
            .num("maxGeneratedId", max_id(self.board))
            .end();
        self.lines.push(line);
        for n in 0..2 {
            let dx = self.random.next_int(8001) - 4000;
            let dy = self.random.next_int(8001) - 4000;
            let now = session.position();
            session.change_position(
                self.board,
                FloatPoint::new(f64::from(now.x + dx), f64::from(now.y + dy)),
            );
            let mut layer = -1;
            if self.random.next_int(3) == 0 {
                layer = self.random.next_int(self.board.layer_count() as i32);
                session.change_layer(layer);
            }
            let outcome = session.insert(self.board);
            let all = outcome
                .as_ref()
                .and_then(|o| o.status.as_ref())
                .is_some_and(|s| s.key == "allItemsInserted");
            let inserted: Vec<ItemId> = outcome.map(|o| o.inserted).unwrap_or_default();
            let items: Vec<String> = inserted
                .iter()
                .filter_map(|id| self.board.item(*id))
                .map(item_text)
                .collect();
            let line = self
                .line("copy-insert", "trial", k)
                .num("insert", n)
                .str("offset", &format!("{dx},{dy}"))
                .num("layer", i64::from(layer))
                .bool("all", all)
                .str("inserted", &ids(inserted.iter().copied()))
                .str("items", &items.join(" | "))
                .num("padstacks", self.board.library.padstacks.count() as i64)
                .num("packages", self.board.library.packages.count() as i64)
                .str("components", &components_text(self.board))
                .str("itemsHash", &hex(items_hash(self.board)))
                .num("maxGeneratedId", max_id(self.board))
                .end();
            self.lines.push(line);
            self.tree_line("copy-tree", k);
        }
    }
}

/// The Rust lines of probe `transform` (header excluded) for `board` loaded from `row`.
pub fn transform_lines(
    load: impl Fn() -> RoutingBoard,
    with_session: Option<&dyn Fn(&mut RoutingBoard)>,
) -> Vec<String> {
    let mut random = JavaRandom::new(SEED);
    let mut lines = Vec::new();
    let mut board = load();
    let mut script = Script {
        board: &mut board,
        stage: "load",
        random: &mut random,
        lines: Vec::new(),
    };
    script.run();
    lines.extend(script.lines);
    if let Some(import) = with_session {
        let mut board = load();
        import(&mut board);
        let mut script = Script {
            board: &mut board,
            stage: "ses",
            random: &mut random,
            lines: Vec::new(),
        };
        script.run();
        lines.extend(script.lines);
    }
    lines
}

pub fn component_of(board: &RoutingBoard, id: ItemId) -> Option<ComponentId> {
    board
        .item(id)
        .map(|i| i.component_id())
        .filter(|c| *c > 0)
        .map(ComponentId)
}

pub fn int_point(x: i32, y: i32) -> IntPoint {
    IntPoint::new(x, y)
}
