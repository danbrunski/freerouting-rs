//! The editor against Java (I03.7) and its own behaviour (I03.1-I03.6).
//!
//! Probe `editor` (`tools/java-reference/probes/board/EditorProbe.java`): on J2 and DAC2020_bm05
//! with their fixtures.tsv session and on the pin-swap board (`Issue143-rpi_splitter` with a
//! logical part for J3 added to its DSN text, as loaded and with its session), one
//! `JavaRandom(20261016)` per board draws single-item selections (picked at their location,
//! extended to nets, components, connected sets and connections, filtered), region boxes (all
//! layers and layer 0), then per angle restriction three circles, tiles and polygons in the board
//! box with a hole in each inserted tile and polygon, and a circle on a pin. The pin-swap board
//! swaps every pin of J3 with each of its swappable pins (as loaded: all complete; with the
//! session: refused as already connected) and dumps the pin nets. Every line is compared.

mod board_probe;
#[path = "support/edit_script.rs"]
mod edit_script;
#[path = "support/editor_script.rs"]
mod editor_script;
#[path = "support/item_dump.rs"]
mod item_dump;

use std::collections::BTreeSet;

use board_probe::{Line, bits, header};
use freerouting::board::ItemFilter;
use freerouting::board::RoutingBoard;
use freerouting::board::items::{Item, ItemId, ItemKind};
use freerouting::board::structure::{AngleRestriction, ComponentId};
use freerouting::datastructures::IdGenerator;
use freerouting::drc::load_design;
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{FloatPoint, IntBox, PolylineShapeOps, TileShape};
use freerouting::interactive::select::{self, Extent};
use freerouting::interactive::{Construction, EditSettings, KeepoutShape, PinSwap, insert_keepout};

const KIND: &str = "editor";
const SEED: i64 = 20261016;
const SELECTIONS: usize = 12;
const REGIONS: usize = 6;
const SWAP_BOARD: &str = "Issue143-rpi_splitter";
const PART_LIBRARY: &str = "  (part_library\n    (logical_part_mapping \"USB-MINIB-SWAP\" (component \"J3\"))\n    (logical_part \"USB-MINIB-SWAP\"\n      (pin \"D+\" 1 \"A\" 0 \"1\" 1)\n      (pin \"D-\" 2 \"A\" 0 \"2\" 1)\n      (pin \"ID\" 3 \"A\" 0 \"3\" 1)\n      (pin \"VBUS\" 4 \"A\" 0 \"4\" 1)\n    )\n  )\n";
const DIAMOND: [(i32, i32); 5] = [(1000, 0), (0, 1000), (0, 0), (-1000, 0), (0, -1000)];
const STAR: [(i32, i32); 6] = [
    (1000, 0),
    (500, 866),
    (-500, 866),
    (-1000, 0),
    (-500, -866),
    (500, -866),
];

// ---------------------------------------------------------------------------------------------
// Boards
// ---------------------------------------------------------------------------------------------

fn load_row(board: &str) -> RoutingBoard {
    let row = board_probe::fixture(board);
    let mut routing = board_probe::load(&row);
    board_probe::import_session(&mut routing, &row);
    routing
}

/// The pin-swap board's DSN text: rpi_splitter with [`PART_LIBRARY`] before `(network`.
fn pin_swap_text() -> String {
    let row = board_probe::fixture(SWAP_BOARD);
    let text = std::fs::read_to_string(board_probe::repo_root().join(&row.dsn)).expect("DSN");
    let at = text.find("  (network").expect("network scope");
    format!("{}{}{}", &text[..at], PART_LIBRARY, &text[at..])
}

fn load_pin_swap_board(with_session: bool) -> RoutingBoard {
    let mut board = load_design(pin_swap_text().as_bytes()).expect("pin-swap board");
    if with_session {
        board_probe::import_session(&mut board, &board_probe::fixture(SWAP_BOARD));
    }
    board
}

fn settings(board: &RoutingBoard) -> EditSettings {
    EditSettings::for_board(board)
}

fn max_id(board: &RoutingBoard) -> i64 {
    i64::from(board.communication.id_generator.max_generated_id())
}

// ---------------------------------------------------------------------------------------------
// The probe mirror
// ---------------------------------------------------------------------------------------------

fn ids(items: Option<&BTreeSet<ItemId>>) -> String {
    match items {
        None => "-".to_string(),
        Some(items) => items
            .iter()
            .map(|id| id.0.to_string())
            .collect::<Vec<_>>()
            .join(","),
    }
}

/// The probe's `location`: a drill centre, a trace's first corner, else the box centre.
fn location(item: &Item) -> FloatPoint {
    if let Some(drill) = item.drill() {
        return drill.center.to_float();
    }
    if let ItemKind::Trace(trace) = &item.kind {
        return trace.polyline.corner(0).to_float();
    }
    let b = item.bounding_box();
    FloatPoint::new(
        (f64::from(b.ll.x) + f64::from(b.ur.x)) / 2.0,
        (f64::from(b.ll.y) + f64::from(b.ur.y)) / 2.0,
    )
}

fn shape_text(s: Option<&TileShape>) -> String {
    match s {
        None => "null".to_string(),
        Some(TileShape::IntBox(b)) => format!("box {} {} {} {}", b.ll.x, b.ll.y, b.ur.x, b.ur.y),
        Some(TileShape::IntOctagon(o)) => format!(
            "oct {} {} {} {} {} {} {} {}",
            o.left_x,
            o.bottom_y,
            o.right_x,
            o.top_y,
            o.upper_left_diagonal_x,
            o.lower_right_diagonal_x,
            o.lower_left_diagonal_x,
            o.upper_right_diagonal_x
        ),
        Some(other) => {
            let mut text = String::from("simplex");
            for i in 0..other.border_line_count() {
                let line = other.border_line(i);
                let (a, b) = (line.a(), line.b());
                text.push_str(&format!(" {},{} {},{}", a.x, a.y, b.x, b.y));
            }
            text
        }
    }
}

fn tiles(board: &RoutingBoard, id: Option<ItemId>) -> String {
    let Some(item) = id.and_then(|id| board.item(id)) else {
        return "-".to_string();
    };
    (0..board.tile_shape_count(item))
        .map(|i| shape_text(board.tile_shape(item, i).as_ref()))
        .collect::<Vec<_>>()
        .join("|")
}

fn clicks_text(points: &[FloatPoint]) -> String {
    points
        .iter()
        .map(|p| format!("{},{}", bits(p.x), bits(p.y)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn nets_text(item: &Item) -> String {
    item.net_numbers()
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

struct Probe<'a> {
    board: &'a mut RoutingBoard,
    stage: &'a str,
    random: JavaRandom,
    filter: ItemFilter,
    lines: Vec<String>,
}

impl Probe<'_> {
    fn line(&self, phase: &str) -> Line {
        Line::new().str("stage", self.stage).str("phase", phase)
    }

    fn sorted_items(&self) -> Vec<ItemId> {
        self.board.items().map(|i| i.id()).collect()
    }

    fn selections(&mut self) {
        let items = self.sorted_items();
        let settings = settings(self.board);
        for _ in 0..SELECTIONS {
            let id = items[self.random.next_int(items.len() as i32) as usize];
            let single: BTreeSet<ItemId> = [id].into();
            let board = &**self.board;
            let at = location(board.item(id).expect("item"));
            let picked = select::pick_items(board, &settings, at, &self.filter);
            let ext = |e| select::extend(board, &single, e, &self.filter);
            let line = self
                .line("select")
                .num("item", i64::from(id.0))
                .str("pick", &ids(Some(&picked)))
                .str("nets", &ids(ext(Extent::Nets).as_ref()))
                .str("components", &ids(ext(Extent::Components).as_ref()))
                .str("connectedSets", &ids(ext(Extent::ConnectedSets).as_ref()))
                .str("connections", &ids(ext(Extent::Connections).as_ref()))
                .str(
                    "filter",
                    &ids(select::filter(board, &single, &self.filter).as_ref()),
                )
                .end();
            self.lines.push(line);
        }
    }

    fn regions(&mut self) {
        let items = self.sorted_items();
        let all = settings(self.board);
        let layer0 = EditSettings {
            select_on_all_visible_layers: false,
            ..all.clone()
        };
        for _ in 0..REGIONS {
            let id = items[self.random.next_int(items.len() as i32) as usize];
            let centre = location(self.board.item(id).expect("item"));
            let w = f64::from(100 + self.random.next_int(3000)) + 0.5;
            let h = f64::from(100 + self.random.next_int(3000)) + 0.25;
            let c1 = FloatPoint::new(centre.x - w, centre.y - h);
            let c2 = FloatPoint::new(centre.x + w, centre.y + h);
            let b = select::region_box(c1, c2);
            let board = &**self.board;
            let line = self
                .line("region")
                .str(
                    "box",
                    &format!("{} {} {} {}", b.ll.x, b.ll.y, b.ur.x, b.ur.y),
                )
                .str(
                    "allLayers",
                    &ids(Some(&select::select_in_region(
                        board,
                        &all,
                        b,
                        &self.filter,
                    ))),
                )
                .str(
                    "layer0",
                    &ids(Some(&select::select_in_region(
                        board,
                        &layer0,
                        b,
                        &self.filter,
                    ))),
                )
                .end();
            self.lines.push(line);
        }
    }

    fn pins_of(&self, component: &str) -> Vec<ItemId> {
        self.board
            .items()
            .filter(|i| i.is_pin() && i.component_id() > 0)
            .filter(|i| {
                self.board
                    .components
                    .get(ComponentId(i.component_id()))
                    .is_some_and(|c| c.name == component)
            })
            .map(|i| i.id())
            .collect()
    }

    fn swap_lines(&mut self, list_swappable: bool) {
        if list_swappable {
            for from in self.pins_of("J3") {
                let swappable = self.board.swappable_pins(from);
                let line = self
                    .line("swappable")
                    .num("pin", i64::from(from.0))
                    .str("pins", &ids(Some(&swappable)))
                    .end();
                self.lines.push(line);
            }
        }
        for from in self.pins_of("J3") {
            for to in self.board.swappable_pins(from) {
                let swap = PinSwap::for_pin(self.board, from).expect("swappable");
                let outcome = swap.complete(self.board, Some(to));
                let key = outcome.status.map_or("-", |s| s.key);
                let line = self
                    .line("swap")
                    .num("from", i64::from(from.0))
                    .num("to", i64::from(to.0))
                    .str("status", key)
                    .str("fromNets", &nets_text(self.board.item(from).expect("pin")))
                    .str("toNets", &nets_text(self.board.item(to).expect("pin")))
                    .end();
                self.lines.push(line);
            }
        }
        let text = self
            .board
            .items()
            .filter(|i| i.is_pin())
            .map(|i| format!("{}:{}", i.id().0, nets_text(i)))
            .collect::<Vec<_>>()
            .join(" ");
        let line = self.line("pins").str("nets", &text).end();
        self.lines.push(line);
    }

    fn draw_clicks(
        &mut self,
        centre: FloatPoint,
        size: i32,
        dirs: &[(i32, i32)],
        star: bool,
    ) -> Vec<FloatPoint> {
        let mut points = Vec::new();
        for (i, (dx, dy)) in dirs.iter().enumerate() {
            let mut r = f64::from(size / 2 + self.random.next_int(size / 2 + 1));
            if star && i % 2 == 1 {
                r *= 0.4;
            }
            points.push(FloatPoint::new(
                centre.x + f64::from(*dx) * r / 1000.0,
                centre.y + f64::from(*dy) * r / 1000.0,
            ));
        }
        points
    }

    fn draw_point(&mut self, b: IntBox) -> FloatPoint {
        let x = f64::from(b.ll.x + self.random.next_int(b.ur.x - b.ll.x + 1)) + 0.25;
        let y = f64::from(b.ll.y + self.random.next_int(b.ur.y - b.ll.y + 1)) - 0.5;
        FloatPoint::new(x, y)
    }

    fn keepout_line(
        &mut self,
        kind: &str,
        restriction: AngleRestriction,
        input: &[FloatPoint],
        id: Option<ItemId>,
    ) {
        let line = self
            .line("keepout")
            .str("kind", kind)
            .str("angle", angle_name(restriction))
            .str("clicks", &clicks_text(input))
            .num("id", id.map_or(-1, |i| i64::from(i.0)))
            .str("tiles", &tiles(self.board, id))
            .num("maxGeneratedId", max_id(self.board))
            .end();
        self.lines.push(line);
    }

    fn hole(&mut self, restriction: AngleRestriction, area: Option<ItemId>) {
        let Some(area) = area else {
            return;
        };
        let b = self.board.item(area).expect("area").bounding_box();
        let cx = (f64::from(b.ll.x) + f64::from(b.ur.x)) / 2.0;
        let cy = (f64::from(b.ll.y) + f64::from(b.ur.y)) / 2.0;
        let d = f64::from(1.max((b.ur.x - b.ll.x).min(b.ur.y - b.ll.y) / 6)) + 0.5;
        let points = [
            FloatPoint::new(cx - d, cy - d),
            FloatPoint::new(cx + d, cy - d),
            FloatPoint::new(cx + d, cy + d),
            FloatPoint::new(cx - d, cy + d),
        ];
        let settings = settings(self.board);
        let (key, inserted) = match Construction::hole(self.board, &settings, points[0]) {
            Err(status) => (status.key, None),
            Ok(mut construction) => {
                for p in &points[1..] {
                    construction.click(self.board, *p);
                }
                let outcome = construction.complete(self.board, 0);
                (outcome.status.key, outcome.inserted)
            }
        };
        let line = self
            .line("hole")
            .str("angle", angle_name(restriction))
            .num("area", i64::from(area.0))
            .str("status", key)
            .num("id", inserted.map_or(-1, |i| i64::from(i.0)))
            .str("tiles", &tiles(self.board, inserted))
            .end();
        self.lines.push(line);
    }

    fn keepouts(&mut self) {
        let own = self.board.rules.trace_angle_restriction();
        let pins: Vec<ItemId> = self
            .board
            .items()
            .filter(|i| i.is_pin())
            .map(|i| i.id())
            .collect();
        let b = self.board.bounding_box();
        let size = ((b.ur.x - b.ll.x).min(b.ur.y - b.ll.y) / 30).max(10);
        for restriction in [
            AngleRestriction::None,
            AngleRestriction::FortyfiveDegree,
            AngleRestriction::NinetyDegree,
        ] {
            self.board.rules.set_trace_angle_restriction(restriction);
            for _ in 0..3 {
                let centre = self.draw_point(b);
                let radius = f64::from(size / 4 + self.random.next_int(size / 2 + 1)) + 0.5;
                let shape = KeepoutShape::Circle {
                    center: centre,
                    radius,
                };
                let circle = insert_keepout(self.board, &shape, 0).inserted;
                self.keepout_line(
                    "circle",
                    restriction,
                    &[centre, FloatPoint::new(radius, 0.0)],
                    circle,
                );
                let tile_centre = self.draw_point(b);
                let tile_clicks = self.draw_clicks(tile_centre, size, &DIAMOND, false);
                let tile = insert_keepout(self.board, &KeepoutShape::Tile(tile_clicks.clone()), 0)
                    .inserted;
                self.keepout_line("tile", restriction, &tile_clicks, tile);
                self.hole(restriction, tile);
                let polygon_centre = self.draw_point(b);
                let polygon_clicks = self.draw_clicks(polygon_centre, size, &STAR, true);
                let polygon = insert_keepout(
                    self.board,
                    &KeepoutShape::Polygon(polygon_clicks.clone()),
                    0,
                )
                .inserted;
                self.keepout_line("polygon", restriction, &polygon_clicks, polygon);
                self.hole(restriction, polygon);
            }
            if !pins.is_empty() {
                let pin = pins[self.random.next_int(pins.len() as i32) as usize];
                let centre = self
                    .board
                    .item(pin)
                    .and_then(|i| i.drill())
                    .expect("pin")
                    .center
                    .to_float();
                let shape = KeepoutShape::Circle {
                    center: centre,
                    radius: 100.0,
                };
                let overlap = insert_keepout(self.board, &shape, 0).inserted;
                self.keepout_line(
                    "overlap",
                    restriction,
                    &[centre, FloatPoint::new(100.0, 0.0)],
                    overlap,
                );
            }
        }
        self.board.rules.set_trace_angle_restriction(own);
        let line = self
            .line("end")
            .num("maxGeneratedId", max_id(self.board))
            .num("items", self.board.items().count() as i64)
            .end();
        self.lines.push(line);
    }
}

fn angle_name(restriction: AngleRestriction) -> &'static str {
    match restriction {
        AngleRestriction::None => "NONE",
        AngleRestriction::FortyfiveDegree => "FORTYFIVE_DEGREE",
        AngleRestriction::NinetyDegree => "NINETY_DEGREE",
    }
}

fn probe<'a>(board: &'a mut RoutingBoard, stage: &'a str) -> Probe<'a> {
    Probe {
        board,
        stage,
        random: JavaRandom::new(SEED),
        filter: ItemFilter::default(),
        lines: Vec::new(),
    }
}

fn editor_lines(board_name: &str) -> Vec<String> {
    let mut lines = vec![header(KIND, board_name)];
    if board_name == SWAP_BOARD {
        let mut board = load_pin_swap_board(false);
        let mut p = probe(&mut board, "pinswap");
        p.selections();
        p.regions();
        p.swap_lines(true);
        p.keepouts();
        lines.append(&mut p.lines);
        let mut routed = load_pin_swap_board(true);
        let mut p = probe(&mut routed, "pinswap-ses");
        p.swap_lines(false);
        lines.append(&mut p.lines);
    } else {
        let mut board = load_row(board_name);
        let mut p = probe(&mut board, "ses");
        p.selections();
        p.regions();
        p.keepouts();
        lines.append(&mut p.lines);
    }
    lines
}

fn check(board_name: &str) {
    let lines = editor_lines(board_name);
    let row = board_probe::fixture(board_name);
    if let Err(e) = board_probe::compare(KIND, &row, &lines) {
        panic!("{e}");
    }
}

#[test]
fn editor_matches_java_j2() {
    check("Issue026-J2_reference");
}

#[test]
fn editor_matches_java_bm05() {
    check("Issue508-DAC2020_bm05");
}

#[test]
fn editor_matches_java_pin_swap_board() {
    check(SWAP_BOARD);
}

// ---------------------------------------------------------------------------------------------
// The editor's own behaviour (I03.1-I03.6)
// ---------------------------------------------------------------------------------------------

use freerouting::board::structure::FixedState;
use freerouting::geometry::Point;
use freerouting::interactive::{
    Command, Editor, EditorOptions, KeepoutKind, ModeKind, Response, Tool, delete_selection,
    fix_selection,
};

/// J2 as loaded (no wiring) with a logical part that makes U1's pins 1-15 swappable, the way
/// the probe's pin-swap board edits its DSN text.
const J2_PART_LIBRARY: &str = "  (part_library\n    (logical_part_mapping \"U1-SWAP\" (component \"U1\"))\n    (logical_part \"U1-SWAP\"\n      (pin \"1\" 1 \"A\" 0 \"1\" 1)\n      (pin \"2\" 2 \"A\" 0 \"2\" 1)\n      (pin \"3\" 3 \"A\" 0 \"3\" 1)\n      (pin \"4\" 4 \"A\" 0 \"4\" 1)\n      (pin \"5\" 5 \"A\" 0 \"5\" 1)\n      (pin \"6\" 6 \"A\" 0 \"6\" 1)\n      (pin \"7\" 7 \"A\" 0 \"7\" 1)\n      (pin \"8\" 8 \"A\" 0 \"8\" 1)\n      (pin \"9\" 9 \"A\" 0 \"9\" 1)\n      (pin \"10\" 10 \"A\" 0 \"10\" 1)\n      (pin \"11\" 11 \"A\" 0 \"11\" 1)\n      (pin \"12\" 12 \"A\" 0 \"12\" 1)\n      (pin \"13\" 13 \"A\" 0 \"13\" 1)\n      (pin \"14\" 14 \"A\" 0 \"14\" 1)\n      (pin \"15\" 15 \"A\" 0 \"15\" 1)\n    )\n  )\n";

fn j2_with_part() -> RoutingBoard {
    let row = board_probe::fixture("Issue026-J2_reference");
    let text = std::fs::read_to_string(board_probe::repo_root().join(&row.dsn)).expect("DSN");
    let at = text.find("  (network").expect("network scope");
    let text = format!("{}{}{}", &text[..at], J2_PART_LIBRARY, &text[at..]);
    load_design(text.as_bytes()).expect("J2 with a logical part")
}

/// The centre of pin `pin` of component `component`.
fn pin_at(board: &RoutingBoard, component: &str, pin: &str) -> FloatPoint {
    board
        .items()
        .find(|i| {
            i.is_pin()
                && board
                    .components
                    .get(ComponentId(i.component_id()))
                    .is_some_and(|c| c.name == component)
                && board.pin_name(i) == Some(pin)
        })
        .and_then(|i| i.drill())
        .unwrap_or_else(|| panic!("pin {component}-{pin}"))
        .center
        .to_float()
}

fn pin_id(board: &RoutingBoard, at: FloatPoint) -> ItemId {
    let pins = board.pick_items_filtered(
        &Point::Int(at.round()),
        -1,
        Some(&ItemFilter::only(
            freerouting::board::SelectableChoice::Pins,
        )),
    );
    *pins.iter().next().expect("a pin")
}

/// A point inside the board box where nothing is on any layer.
fn empty_point(board: &RoutingBoard) -> FloatPoint {
    let b = board.bounding_box();
    for i in 1..40 {
        for j in 1..40 {
            let p = IntPoint_new(
                b.ll.x + (b.ur.x - b.ll.x) / 40 * i,
                b.ll.y + (b.ur.y - b.ll.y) / 40 * j,
            );
            if board.pick_items(&Point::Int(p), -1).is_empty() {
                return p.to_float();
            }
        }
    }
    panic!("no empty point");
}

#[allow(non_snake_case)]
fn IntPoint_new(x: i32, y: i32) -> freerouting::geometry::IntPoint {
    freerouting::geometry::IntPoint { x, y }
}

fn status_key(response: &Response) -> Option<&'static str> {
    response.status.as_ref().map(|s| s.key)
}

fn routed_j2() -> RoutingBoard {
    load_row("Issue026-J2_reference")
}

fn first_via(board: &RoutingBoard) -> FloatPoint {
    board
        .items()
        .find(|i| i.is_via())
        .and_then(|i| i.drill())
        .expect("a via")
        .center
        .to_float()
}

/// One row of the dispatch table: the commands that reach the start mode, the command, the next
/// mode and Java's status key (`None`: no message).
struct Row {
    name: &'static str,
    setup: Vec<Command>,
    command: Command,
    mode: ModeKind,
    status: Option<&'static str>,
}

fn row(
    name: &'static str,
    setup: Vec<Command>,
    command: Command,
    mode: ModeKind,
    status: Option<&'static str>,
) -> Row {
    Row {
        name,
        setup,
        command,
        mode,
        status,
    }
}

/// One row per (mode, command) pair Java handles, with the next mode; replaces Java's
/// `InteractiveStateCommandAdapterTest` (Swing glue).
#[test]
fn editor_dispatch_table() {
    let base = j2_with_part();
    let pin = pin_at(&base, "J2", "A2");
    let other_pin = pin_at(&base, "U1", "2");
    let swap_from = pin_at(&base, "U1", "8");
    let swap_to = pin_at(&base, "U1", "9");
    let empty = empty_point(&base);
    let toward = FloatPoint::new((pin.x + other_pin.x) / 2.0, (pin.y + other_pin.y) / 2.0);
    let routed = routed_j2();
    let via = first_via(&routed);
    let route = || vec![Command::Click(pin)];
    let select = || vec![Command::SetTool(Tool::Select), Command::Click(pin)];
    let tile = |at: FloatPoint| Command::StartKeepout {
        kind: KeepoutKind::Tile,
        at,
    };
    let menu = ModeKind::Menu(Tool::Route);
    let drag_menu = ModeKind::Menu(Tool::Drag);
    let select_menu = ModeKind::Menu(Tool::Select);
    let routing = ModeKind::Routing { stitch: false };
    let mut rows = vec![
        // route menu (RouteMenuState, MenuState)
        row(
            "route menu click on a pin",
            vec![],
            Command::Click(pin),
            routing,
            Some("routing_net_message"),
        ),
        row(
            "route menu click on nothing",
            vec![],
            Command::Click(empty),
            menu,
            None,
        ),
        row("route menu press", vec![], Command::Press(pin), menu, None),
        row("route menu drag", vec![], Command::DragTo(pin), menu, None),
        row("route menu complete", vec![], Command::Complete, menu, None),
        row("route menu cancel", vec![], Command::Cancel, menu, None),
        row(
            "route menu t",
            vec![],
            Command::StartRoute { at: pin },
            routing,
            Some("routing_net_message"),
        ),
        row(
            "route menu i",
            vec![],
            Command::SelectAt(pin),
            ModeKind::Selecting,
            Some("in_inspect_mode"),
        ),
        row(
            "route menu i on nothing",
            vec![],
            Command::SelectAt(empty),
            menu,
            None,
        ),
        row(
            "route menu w on a pin",
            vec![],
            Command::StartPinSwap { at: swap_from },
            ModeKind::PinSwap,
            Some("please_click_second_pin_with_the_left_mouse_button"),
        ),
        row(
            "route menu w without swappable pins",
            vec![],
            Command::StartPinSwap { at: pin },
            menu,
            Some("no_swappable_pin_found"),
        ),
        row(
            "route menu w on nothing",
            vec![],
            Command::StartPinSwap { at: empty },
            menu,
            Some("no_pin_selected"),
        ),
        row(
            "route menu undo",
            vec![],
            Command::Undo,
            menu,
            Some("no_more_undo_possible"),
        ),
        row(
            "route menu redo",
            vec![],
            Command::Redo,
            menu,
            Some("no_more_redo_possible"),
        ),
        row(
            "route menu d",
            vec![],
            Command::SetTool(Tool::Drag),
            drag_menu,
            None,
        ),
        row(
            "route menu s",
            vec![],
            Command::SetTool(Tool::Select),
            select_menu,
            None,
        ),
        row(
            "route menu tile",
            vec![],
            tile(empty),
            ModeKind::Constructing,
            None,
        ),
        row(
            "route menu hole on nothing",
            vec![],
            Command::StartHole { at: empty },
            menu,
            Some("no_item_found_for_adding_hole"),
        ),
        row("route menu layer", vec![], Command::SetLayer(1), menu, None),
        // drag menu (DragMenuState)
        row(
            "drag menu press on a pin",
            vec![Command::SetTool(Tool::Drag)],
            Command::Press(pin),
            ModeKind::Dragging,
            Some("dragging_item"),
        ),
        row(
            "drag menu press on nothing",
            vec![Command::SetTool(Tool::Drag)],
            Command::Press(empty),
            ModeKind::MakingSpace,
            None,
        ),
        row(
            "drag menu click",
            vec![Command::SetTool(Tool::Drag)],
            Command::Click(pin),
            drag_menu,
            None,
        ),
        row(
            "drag menu r",
            vec![Command::SetTool(Tool::Drag)],
            Command::SetTool(Tool::Route),
            menu,
            None,
        ),
        // select menu (InspectMenuState)
        row(
            "select menu click on a pin",
            vec![Command::SetTool(Tool::Select)],
            Command::Click(pin),
            ModeKind::Selecting,
            Some("in_inspect_mode"),
        ),
        row(
            "select menu click on nothing",
            vec![Command::SetTool(Tool::Select)],
            Command::Click(empty),
            select_menu,
            None,
        ),
        row(
            "select menu drag",
            vec![Command::SetTool(Tool::Select)],
            Command::DragTo(pin),
            ModeKind::SelectingRegion,
            Some("drag_left_mouse_button_to_select_items_in_region"),
        ),
        // routing (DynamicRouteState, StitchRouteState, RouteState)
        row(
            "routing move",
            route(),
            Command::Move(toward),
            routing,
            None,
        ),
        row(
            "routing click ends",
            route(),
            Command::Click(toward),
            menu,
            None,
        ),
        row("routing complete", route(), Command::Complete, menu, None),
        row("routing cancel", route(), Command::Cancel, menu, None),
        row(
            "routing snapshot",
            route(),
            Command::Snapshot,
            routing,
            None,
        ),
        row(
            "routing undo is ignored",
            route(),
            Command::Undo,
            routing,
            None,
        ),
        row(
            "routing redo is ignored",
            route(),
            Command::Redo,
            routing,
            None,
        ),
        row(
            "routing tool change drops the route",
            route(),
            Command::SetTool(Tool::Select),
            select_menu,
            None,
        ),
        row(
            "routing layer change",
            route(),
            Command::SetLayer(1),
            routing,
            Some("layer_not_changed_obstacle_message"),
        ),
        row(
            "stitch click",
            vec![stitch_settings(&base), Command::Click(pin)],
            Command::Click(toward),
            ModeKind::Routing { stitch: true },
            None,
        ),
        row(
            "stitch move",
            vec![stitch_settings(&base), Command::Click(pin)],
            Command::Move(toward),
            ModeKind::Routing { stitch: true },
            None,
        ),
        // dragging (DragState, DragItemState, MakeSpaceState)
        row(
            "dragging step",
            vec![Command::SetTool(Tool::Drag), Command::Press(pin)],
            Command::DragTo(FloatPoint::new(pin.x + 200.0, pin.y)),
            ModeKind::Dragging,
            None,
        ),
        row(
            "dragging release",
            vec![Command::SetTool(Tool::Drag), Command::Press(pin)],
            Command::Release,
            drag_menu,
            None,
        ),
        row(
            "dragging complete",
            vec![Command::SetTool(Tool::Drag), Command::Press(pin)],
            Command::Complete,
            drag_menu,
            None,
        ),
        row(
            "dragging cancel",
            vec![Command::SetTool(Tool::Drag), Command::Press(pin)],
            Command::Cancel,
            drag_menu,
            None,
        ),
        row(
            "dragging undo is ignored",
            vec![Command::SetTool(Tool::Drag), Command::Press(pin)],
            Command::Undo,
            ModeKind::Dragging,
            None,
        ),
        row(
            "making space release",
            vec![Command::SetTool(Tool::Drag), Command::Press(empty)],
            Command::Release,
            drag_menu,
            None,
        ),
        // selecting (InspectedItemState)
        row(
            "selecting click on the selected item ends",
            select(),
            Command::Click(pin),
            select_menu,
            None,
        ),
        row(
            "selecting click on another item",
            select(),
            Command::Click(other_pin),
            ModeKind::Selecting,
            None,
        ),
        row(
            "selecting toggle",
            select(),
            Command::ToggleSelect(other_pin),
            ModeKind::Selecting,
            None,
        ),
        row(
            "selecting nets",
            select(),
            Command::ExtendSelection(Extent::Nets),
            ModeKind::Selecting,
            None,
        ),
        row(
            "selecting components",
            select(),
            Command::ExtendSelection(Extent::Components),
            ModeKind::Selecting,
            None,
        ),
        row(
            "selecting connected sets",
            select(),
            Command::ExtendSelection(Extent::ConnectedSets),
            ModeKind::Selecting,
            None,
        ),
        row(
            "selecting connections",
            select(),
            Command::ExtendSelection(Extent::Connections),
            select_menu,
            None,
        ),
        row(
            "selecting filter",
            select(),
            Command::FilterSelection,
            ModeKind::Selecting,
            None,
        ),
        row(
            "selecting complete",
            select(),
            Command::Complete,
            select_menu,
            None,
        ),
        row(
            "selecting cancel",
            select(),
            Command::Cancel,
            select_menu,
            None,
        ),
        row(
            "selecting undo is ignored",
            select(),
            Command::Undo,
            ModeKind::Selecting,
            None,
        ),
        row(
            "selecting drag",
            select(),
            Command::DragTo(pin),
            ModeKind::SelectingRegion,
            Some("drag_left_mouse_button_to_select_items_in_region"),
        ),
        row(
            "selecting delete of a component pin",
            select(),
            Command::DeleteSelection,
            ModeKind::Selecting,
            Some("some_items_are_fixed_and_could_therefore_not_be_removed"),
        ),
        row(
            "selecting fix",
            select(),
            Command::FixSelection(true),
            ModeKind::Selecting,
            None,
        ),
        row(
            "selecting i is a menu key",
            select(),
            Command::SelectAt(other_pin),
            ModeKind::Selecting,
            None,
        ),
        // region (InspectItemsInRegionState)
        row(
            "region drag",
            vec![Command::SetTool(Tool::Select), Command::DragTo(pin)],
            Command::DragTo(other_pin),
            ModeKind::SelectingRegion,
            None,
        ),
        row(
            "region release",
            vec![
                Command::SetTool(Tool::Select),
                Command::DragTo(pin),
                Command::DragTo(other_pin),
            ],
            Command::Release,
            ModeKind::Selecting,
            None,
        ),
        row(
            "region release on nothing",
            vec![Command::SetTool(Tool::Select), Command::DragTo(empty)],
            Command::Release,
            select_menu,
            Some("nothing_selected"),
        ),
        row(
            "region cancel",
            vec![Command::SetTool(Tool::Select), Command::DragTo(pin)],
            Command::Cancel,
            select_menu,
            None,
        ),
        // pin swap (PinSwapState)
        row(
            "pin swap click on a swappable pin",
            vec![Command::StartPinSwap { at: swap_from }],
            Command::Click(swap_to),
            menu,
            Some("pin_swap_completed"),
        ),
        row(
            "pin swap click on another pin",
            vec![Command::StartPinSwap { at: swap_from }],
            Command::Click(pin),
            menu,
            None,
        ),
        row(
            "pin swap click on nothing",
            vec![Command::StartPinSwap { at: swap_from }],
            Command::Click(empty),
            menu,
            Some("no_pin_selected"),
        ),
        row(
            "pin swap complete",
            vec![Command::StartPinSwap { at: swap_from }],
            Command::Complete,
            menu,
            Some("pin_to_swap_missing"),
        ),
        row(
            "pin swap cancel",
            vec![Command::StartPinSwap { at: swap_from }],
            Command::Cancel,
            menu,
            None,
        ),
        row(
            "pin swap undo is ignored",
            vec![Command::StartPinSwap { at: swap_from }],
            Command::Undo,
            ModeKind::PinSwap,
            None,
        ),
        // construction (TileConstructionState, CircleConstructionState)
        row(
            "constructing click",
            vec![tile(empty)],
            Command::Click(FloatPoint::new(empty.x + 500.0, empty.y)),
            ModeKind::Constructing,
            None,
        ),
        row(
            "constructing complete with one corner",
            vec![tile(empty)],
            Command::Complete,
            menu,
            Some("keepout_cancelled_because_of_overlaps"),
        ),
        row(
            "constructing cancel",
            vec![tile(empty)],
            Command::Cancel,
            menu,
            None,
        ),
        row(
            "constructing undo is ignored",
            vec![tile(empty)],
            Command::Undo,
            ModeKind::Constructing,
            None,
        ),
        row(
            "circle click completes",
            vec![Command::StartKeepout {
                kind: KeepoutKind::Circle,
                at: empty,
            }],
            Command::Click(FloatPoint::new(empty.x + 50.0, empty.y)),
            menu,
            Some("keepout_successful_completed"),
        ),
    ];
    // a drag of a via of the routed board (DragItemState on a drill item)
    let mut failures = Vec::new();
    for r in rows.drain(..) {
        let mut editor = Editor::new(base.clone(), EditSettings::for_board(&base));
        for c in r.setup {
            editor.apply(c);
        }
        let response = editor.apply(r.command.clone());
        if response.mode != r.mode || status_key(&response) != r.status {
            failures.push(format!(
                "{}: {:?} -> {:?} {:?}, expected {:?} {:?}",
                r.name,
                r.command,
                response.mode,
                status_key(&response),
                r.mode,
                r.status
            ));
        }
    }
    let mut editor = Editor::new(routed.clone(), EditSettings::for_board(&routed));
    editor.apply(Command::SetTool(Tool::Drag));
    let response = editor.apply(Command::Press(via));
    if response.mode != ModeKind::Dragging {
        failures.push(format!("drag menu press on a via -> {:?}", response.mode));
    }
    assert!(
        failures.is_empty(),
        "dispatch rows failed:\n{}",
        failures.join("\n")
    );
}

fn stitch_settings(board: &RoutingBoard) -> Command {
    Command::UpdateSettings(EditSettings {
        stitch_route: true,
        ..EditSettings::for_board(board)
    })
}

/// Java's `extentToWholeComponents` keeps the old selection, adds the component items and does
/// not filter, unlike the other extensions.
#[test]
fn extend_to_components_keeps_the_selection_and_does_not_filter() {
    let board = j2_with_part();
    let pin = pin_id(&board, pin_at(&board, "U1", "2"));
    let component = board.item(pin).expect("pin").component_id();
    let trace_only = ItemFilter::only(freerouting::board::SelectableChoice::Traces);
    let selection: BTreeSet<ItemId> = [pin].into();
    let components = select::extend(&board, &selection, Extent::Components, &trace_only)
        .expect("component items");
    let expected: BTreeSet<ItemId> = board.component_items(component).into_iter().collect();
    assert!(components.contains(&pin));
    assert_eq!(
        components, expected,
        "every item of U1, the filter not applied"
    );
    assert!(components.len() > 1);
    // the other extensions filter: nets of a pin through a traces-only filter is empty but
    // stays a selection (Java ignores the filter's result there)
    let nets = select::extend(&board, &selection, Extent::Nets, &trace_only);
    assert_eq!(nets, Some(BTreeSet::new()));
    // a keepout is not connectable: nets of it alone leave the state
    let class = freerouting::rules::BoardRules::clearance_class_none();
    let mut with_keepout = board.clone();
    let keepout = with_keepout.insert_obstacle(
        freerouting::geometry::Area::from(freerouting::geometry::Shape::from(IntBox::new(
            0, 0, 10, 10,
        ))),
        0,
        class,
        FixedState::Unfixed,
    );
    let only_keepout: BTreeSet<ItemId> = [keepout].into();
    assert_eq!(
        select::extend(
            &with_keepout,
            &only_keepout,
            Extent::Nets,
            &ItemFilter::default()
        ),
        None
    );
    // components of an item without a component keep the selection
    assert_eq!(
        select::extend(
            &with_keepout,
            &only_keepout,
            Extent::Components,
            &ItemFilter::default()
        ),
        Some(only_keepout.clone())
    );
}

/// I03.3: a pin connected on the routed pin-swap board is refused.
#[test]
fn a_connected_pin_is_refused() {
    let mut board = load_pin_swap_board(true);
    let from = pin_id(&board, pin_at(&board, "J3", "D+"));
    let to = pin_id(&board, pin_at(&board, "J3", "D-"));
    let swap = PinSwap::for_pin(&board, from).expect("swappable");
    assert!(swap.swappable_pins().contains(&to));
    let undo_hash = board.state_hash();
    let outcome = swap.complete(&mut board, Some(to));
    assert!(!outcome.swapped);
    assert_eq!(
        outcome.status.map(|s| s.key),
        Some("pin_not_swapped_because_it_is_already_connected")
    );
    assert_eq!(board.state_hash(), undo_hash, "no snapshot, no change");
}

/// I03.4: an overlapping circle is refused with Java's key and takes no snapshot.
#[test]
fn an_overlapping_circle_is_refused_without_a_snapshot() {
    let mut board = routed_j2();
    let pin = pin_at(&board, "U1", "2");
    let before = board.state_hash();
    let outcome = insert_keepout(
        &mut board,
        &KeepoutShape::Circle {
            center: pin,
            radius: 100.0,
        },
        0,
    );
    assert_eq!(outcome.inserted, None);
    assert_eq!(outcome.status.key, "keepout_cancelled_because_of_overlaps");
    assert_eq!(board.state_hash(), before, "no snapshot");
    assert!(!board.undo(None), "nothing to undo");
}

/// The board content without the undo history (the state hash covers the undo structure): per
/// item its id, kind, nets, fixed state, component, clearance class and tile shapes. The
/// generated-id counter is left out: an undo does not take ids back (Java's neither).
fn content_hash(board: &RoutingBoard) -> u128 {
    let mut text = String::new();
    for item in board.items() {
        text.push_str(&format!(
            "{} {} {:?} {:?} {} {} {}\n",
            item.id().0,
            item.selectable_choice()
                .map_or("outline", |c| c.java_name()),
            item.net_numbers(),
            item.fixed_state(),
            item.component_id(),
            item.clearance_class(),
            tiles(board, Some(item.id()))
        ));
    }
    let mut hash: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    for b in text.bytes() {
        hash ^= u128::from(b);
        hash = hash.wrapping_mul(0x0000_0000_0100_0000_0000_0000_0000_013b);
    }
    hash
}

/// I03.5: a route, a drag, a pin swap and a keepout on J2, undone one by one and redone one by
/// one; each undo and redo restores the state recorded after that step.
#[test]
fn route_undo_redo_round_trip() {
    let board = j2_with_part();
    let start = pin_at(&board, "J2", "A2");
    let target = pin_at(&board, "U1", "2");
    let drag = pin_at(&board, "U1", "14");
    let swap_from = pin_at(&board, "U1", "8");
    let empty = empty_point(&board);
    let mut editor = Editor::new(board.clone(), EditSettings::for_board(&board));
    let mut hashes = vec![content_hash(editor.board())];
    // a route: start, four moves towards the target, complete
    assert_eq!(
        editor.apply(Command::Click(start)).mode,
        ModeKind::Routing { stitch: false }
    );
    // undo is ignored while routing
    let ignored = editor.apply(Command::Undo);
    assert_eq!(ignored.mode, ModeKind::Routing { stitch: false });
    assert_eq!(ignored.status, None);
    for k in 1..=4 {
        let f = f64::from(k) / 4.0;
        let p = FloatPoint::new(
            start.x + (target.x - start.x) * f,
            start.y + (target.y - start.y) * f,
        );
        if !matches!(
            editor.apply(Command::Move(p)).mode,
            ModeKind::Routing { .. }
        ) {
            break;
        }
    }
    editor.apply(Command::Complete);
    hashes.push(content_hash(editor.board()));
    // a drag of U1 by one of its pins
    editor.apply(Command::SetTool(Tool::Drag));
    assert_eq!(editor.apply(Command::Press(drag)).mode, ModeKind::Dragging);
    editor.apply(Command::DragTo(FloatPoint::new(drag.x, drag.y + 300.0)));
    editor.apply(Command::DragTo(FloatPoint::new(drag.x, drag.y + 600.0)));
    editor.apply(Command::Release);
    hashes.push(content_hash(editor.board()));
    // a pin swap
    editor.apply(Command::SetTool(Tool::Route));
    // the drag moved U1, so take its pins where they are now
    assert_ne!(pin_at(editor.board(), "U1", "8"), swap_from, "U1 moved");
    let (swap_from, swap_to) = (
        pin_at(editor.board(), "U1", "8"),
        pin_at(editor.board(), "U1", "9"),
    );
    assert_eq!(
        editor.apply(Command::StartPinSwap { at: swap_from }).mode,
        ModeKind::PinSwap
    );
    let swapped = editor.apply(Command::Click(swap_to));
    assert_eq!(swapped.status.map(|s| s.key), Some("pin_swap_completed"));
    hashes.push(content_hash(editor.board()));
    // a keepout
    let size = 300.0;
    let clicks = vec![
        empty,
        FloatPoint::new(empty.x + size, empty.y),
        FloatPoint::new(empty.x + size, empty.y + size),
        FloatPoint::new(empty.x, empty.y + size),
    ];
    let inserted = editor.apply(Command::InsertKeepout {
        shape: KeepoutShape::Tile(clicks),
        layer: 0,
    });
    assert_eq!(
        inserted.status.map(|s| s.key),
        Some("keepout_successful_completed")
    );
    assert_eq!(inserted.delta.inserted.len(), 1);
    hashes.push(content_hash(editor.board()));
    for pair in hashes.windows(2) {
        assert_ne!(pair[0], pair[1], "every step changes the board");
    }
    for k in (0..4).rev() {
        let undo = editor.apply(Command::Undo);
        assert_eq!(undo.status.map(|s| s.key), Some("undo"));
        assert_eq!(
            content_hash(editor.board()),
            hashes[k],
            "undo back to step {k}"
        );
    }
    assert_eq!(
        editor.apply(Command::Undo).status.map(|s| s.key),
        Some("no_more_undo_possible")
    );
    for (k, hash) in hashes.iter().enumerate().skip(1) {
        let redo = editor.apply(Command::Redo);
        assert_eq!(redo.status.map(|s| s.key), Some("redo"));
        assert_eq!(content_hash(editor.board()), *hash, "redo to step {k}");
    }
    assert_eq!(
        editor.apply(Command::Redo).status.map(|s| s.key),
        Some("no_more_redo_possible")
    );
}

/// I03.6: delete of traces and vias, with a fixed item kept and Java's message; undo restores.
#[test]
fn delete_selection_keeps_fixed_items_and_undo_restores() {
    for push in [true, false] {
        let mut board = routed_j2();
        // the session import fixes the wiring
        let wiring: Vec<ItemId> = board
            .items()
            .filter(|i| i.is_trace() || i.is_via())
            .map(|i| i.id())
            .collect();
        for id in &wiring {
            board.set_fixed_state(*id, FixedState::Unfixed);
        }
        let unfixed_hash = content_hash(&board);
        let traces: Vec<ItemId> = board
            .items()
            .filter(|i| i.is_trace())
            .map(|i| i.id())
            .take(6)
            .collect();
        let pin = pin_id(&board, pin_at(&board, "U1", "2"));
        let fixed = traces[0];
        board.set_fixed_state(fixed, FixedState::UserFixed);
        let count = board.items().count();
        let settings = EditSettings {
            push_enabled: push,
            ..EditSettings::for_board(&board)
        };
        let selection: BTreeSet<ItemId> = traces.iter().copied().chain([pin]).collect();
        let outcome = delete_selection(&mut board, &settings, &selection);
        assert_eq!(outcome.changed, 5, "push {push}");
        assert_eq!(outcome.refused, 2, "the fixed trace and the component pin");
        assert_eq!(
            outcome.status.map(|s| s.key),
            Some("some_items_are_fixed_and_could_therefore_not_be_removed")
        );
        assert!(board.item(fixed).is_some() && board.item(pin).is_some());
        assert!(traces[1..].iter().all(|id| board.item(*id).is_none()));
        if !push {
            assert_eq!(board.items().count(), count - 5);
        }
        assert!(board.undo(None));
        assert_eq!(board.items().count(), count);
        board.set_fixed_state(fixed, FixedState::Unfixed);
        assert_eq!(content_hash(&board), unfixed_hash, "push {push}");
    }
}

/// I03.6: fix and unfix of the selection, each one undo step.
#[test]
fn fix_and_unfix_are_undoable() {
    let mut board = routed_j2();
    let vias: Vec<ItemId> = board
        .items()
        .filter(|i| i.is_via())
        .map(|i| i.id())
        .take(4)
        .collect();
    let selection: BTreeSet<ItemId> = vias.iter().copied().collect();
    let states = |board: &RoutingBoard| -> Vec<FixedState> {
        vias.iter()
            .map(|id| board.item(*id).expect("via").fixed_state())
            .collect()
    };
    let initial = states(&board);
    let initial_hash = content_hash(&board);
    let unfixed = fix_selection(&mut board, &selection, false);
    assert!(states(&board).iter().all(|s| *s == FixedState::Unfixed));
    let after_unfix = content_hash(&board);
    let fixed = fix_selection(&mut board, &selection, true);
    assert_eq!(fixed.changed, 4);
    assert!(states(&board).iter().all(|s| *s == FixedState::UserFixed));
    // fixing again changes nothing and takes no snapshot
    assert_eq!(fix_selection(&mut board, &selection, true).changed, 0);
    assert!(board.undo(None));
    assert_eq!(content_hash(&board), after_unfix);
    if unfixed.changed > 0 {
        assert!(board.undo(None));
    }
    assert_eq!(states(&board), initial);
    assert_eq!(content_hash(&board), initial_hash);
}

/// The editor drains the change journal into each response.
#[test]
fn responses_carry_the_board_delta() {
    let board = j2_with_part();
    let empty = empty_point(&board);
    let mut editor = Editor::new(board.clone(), EditSettings::for_board(&board));
    let response = editor.apply(Command::InsertKeepout {
        shape: KeepoutShape::Circle {
            center: empty,
            radius: 120.0,
        },
        layer: 0,
    });
    assert_eq!(response.delta.inserted.len(), 1);
    assert!(response.delta.dirty.is_some());
    let undo = editor.apply(Command::Undo);
    assert_eq!(undo.delta.removed, response.delta.inserted);
    assert!(editor.apply(Command::Move(empty)).delta.inserted.is_empty());
}

/// I03.8: the committed mixed script (route, drag, make-space, select, region, fix, delete, pin
/// swap, keepouts, layers, undo, redo, a stitch route) replays to identical output, with Java's
/// budgets and without.
#[test]
fn editor_script_replays_identically() {
    let path =
        board_probe::repo_root().join("crates/freerouting/tests/data/edit_script_editor_j2.json");
    let text = std::fs::read_to_string(path).expect("script");
    assert!(editor_script::is_editor_script(&text));
    let insert = editor_script::dsn_insert(&text).expect("dsn_insert");
    let row = board_probe::fixture("Issue026-J2_reference");
    let dsn = std::fs::read_to_string(board_probe::repo_root().join(&row.dsn)).expect("DSN");
    let dsn = editor_script::insert_before_network(&dsn, &insert).expect("network scope");
    let board = load_design(dsn.as_bytes()).expect("board");
    let script = editor_script::parse_editor_script(&text, board.layer_count()).expect("parse");
    assert_eq!(script.commands.len(), 64);
    let java = editor_script::replay_editor(board.clone(), &script, EditorOptions::default());
    let again = editor_script::replay_editor(board.clone(), &script, EditorOptions::default());
    assert_eq!(java, again);
    let unlimited = EditorOptions {
        route_budget: freerouting::interactive::RouteBudget::UNLIMITED,
        drag_budget: freerouting::interactive::DragBudget::UNLIMITED,
        ..EditorOptions::default()
    };
    assert_eq!(
        java,
        editor_script::replay_editor(board, &script, unlimited)
    );
    let has = |cmd: &str, needle: &str| {
        java.iter()
            .any(|l| l.contains(&format!("\"cmd\":\"{cmd}\"")) && l.contains(needle))
    };
    assert!(has("click", "\"mode\":\"routing\""), "the route starts");
    assert!(has("press", "\"mode\":\"dragging\""), "the drag starts");
    assert!(has("click", "pin_swap_completed"), "the pin swap completes");
    assert!(has("insert_keepout", "keepout_successful_completed"));
    assert!(has("undo", "\"status\":\"undo\""));
    assert!(has("redo", "\"status\":\"redo\""));
}

/// The logical part of a component survives an undo: Java's `setLogicalPart` changes the
/// object its undo list holds, so an undo restores the component with its part (the port's
/// copy in the undo list was stale before I03 found it in the editor script).
#[test]
fn a_logical_part_survives_an_undo() {
    let mut board = j2_with_part();
    let pin = pin_id(&board, pin_at(&board, "U1", "8"));
    assert_eq!(board.swappable_pins(pin).len(), 14);
    board.generate_snapshot();
    let u1 = ComponentId(board.item(pin).expect("pin").component_id());
    board
        .components
        .move_by(u1, &freerouting::geometry::Vector::ZERO);
    assert!(board.undo(None));
    assert_eq!(board.swappable_pins(pin).len(), 14);
}
