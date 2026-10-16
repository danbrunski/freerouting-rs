//! Dragging and make-space against Java (I02.2-I02.4) and their own
//! behaviour (I02.3).
//!
//! Probe `drag-script` (`tools/java-reference/probes/board/DragScriptProbe.java`): on five boards
//! (J2, DAC2020_bm05, DAC2020_bm08 and rpi_splitter with their fixtures.tsv session, Issue957 as
//! loaded), with every user-fixed trace and via unfixed first, one `JavaRandom(20261016)` draws 10
//! vias and 5 pins, each dragged from its centre in 5 pointer steps of 200 units in a 45 degree
//! direction (a pin of a component drags the component), then 3 make-space drags of 8 steps of
//! the board's smallest trace half width across the middle of a drawn trace's first segment.
//! Each press, step and release is one line (outcome, move vector, item centre, component
//! location, `maxGeneratedId`; for make-space the route end, the shove-failing obstacle and the
//! pointer warp); the canonical item dump follows every 10th step and each phase. On a mismatch
//! the message names the first differing line (for a digest: the section).
//!
//! Component drags are compared as well as single drill items: `MoveComponent`'s check ignores
//! the whole group from the start and does not change the board, so its result cannot depend on
//! Java's identity-hash set order (I02.1).

mod board_probe;
#[path = "support/drag_script.rs"]
mod drag_script;
#[path = "support/edit_script.rs"]
mod edit_script;
#[path = "support/item_dump.rs"]
mod item_dump;

use board_probe::{Line, bits, header};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::{ComponentId, FixedState};
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{FloatPoint, Vector};
use freerouting::interactive::{
    DragBudget, DragRelease, DragSession, DragSettings, DragStep, EditSettings,
};
use freerouting::rules::Nets;
use item_dump::{dump, point_text};

const KIND: &str = "drag-script";
const SEED: i64 = 20261016;
const VIA_DRAGS: usize = 10;
const PIN_DRAGS: usize = 5;
const SPACE_DRAGS: usize = 3;
const DRAG_STEPS: i32 = 5;
const SPACE_STEPS: i32 = 8;
const STEP: f64 = 200.0;
const DIRS45: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

fn fpoint(p: FloatPoint) -> String {
    format!("f{},{}", bits(p.x), bits(p.y))
}

fn max_id(board: &RoutingBoard) -> i64 {
    i64::from(board.communication.id_generator.max_generated_id())
}

fn opt_id(id: Option<ItemId>) -> i64 {
    id.map_or(-1, |i| i64::from(i.0))
}

fn join_ints(values: impl IntoIterator<Item = i32>) -> String {
    values
        .into_iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn vector_text(v: &Vector) -> String {
    match v {
        Vector::Int(iv) => format!("{},{}", iv.x, iv.y),
        _ => fpoint(v.to_float()),
    }
}

/// The probe's `unfixWiring`: unfixes the user-fixed traces and vias.
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

/// The probe's script state.
struct Script<'a> {
    board: &'a mut RoutingBoard,
    stage: &'a str,
    settings: EditSettings,
    drag: DragSettings,
    steps: usize,
    lines: Vec<String>,
}

impl Script<'_> {
    fn line(&self, phase: &str, n: usize) -> Line {
        Line::new()
            .str("stage", self.stage)
            .str("phase", phase)
            .num("drag", n as i64)
    }

    fn count_step(&mut self) {
        self.steps += 1;
        if self.steps % 10 == 0 {
            self.lines.extend(dump(self.board));
        }
    }

    fn press_kind(session: &Option<DragSession>) -> &'static str {
        match session {
            Some(DragSession::Item(_)) => "item",
            Some(DragSession::MakeSpace(_)) => "makespace",
            None => "none",
        }
    }

    fn run(&mut self) {
        let mut random = JavaRandom::new(SEED);
        for n in 0..VIA_DRAGS {
            let vias: Vec<FloatPoint> = self
                .board
                .items()
                .filter(|i| i.is_via())
                .map(|i| i.drill().expect("via").center.to_float())
                .collect();
            if vias.is_empty() {
                let line = self.line("via", n).str("skip", "no vias").end();
                self.lines.push(line);
                continue;
            }
            let at = vias[random.next_int(vias.len() as i32) as usize];
            let dir = DIRS45[random.next_int(8) as usize];
            self.item_drag("via", n, at, dir);
        }
        self.lines.extend(dump(self.board));
        for n in 0..PIN_DRAGS {
            let pins: Vec<FloatPoint> = self
                .board
                .items()
                .filter(|i| i.is_pin() && i.net_count() > 0)
                .map(|i| i.drill().expect("pin").center.to_float())
                .collect();
            if pins.is_empty() {
                let line = self.line("pin", n).str("skip", "no pins").end();
                self.lines.push(line);
                continue;
            }
            let at = pins[random.next_int(pins.len() as i32) as usize];
            let dir = DIRS45[random.next_int(8) as usize];
            self.item_drag("pin", n, at, dir);
        }
        self.lines.extend(dump(self.board));
        for n in 0..SPACE_DRAGS {
            let traces: Vec<(i32, FloatPoint)> = self
                .board
                .items()
                .filter_map(|i| i.as_trace())
                .map(|t| {
                    let middle = t
                        .polyline
                        .corner_approx(0)
                        .middle_point(t.polyline.corner_approx(1));
                    (t.layer, middle)
                })
                .collect();
            if traces.is_empty() {
                let line = self.line("space", n).str("skip", "no traces").end();
                self.lines.push(line);
                continue;
            }
            let (layer, middle) = traces[random.next_int(traces.len() as i32) as usize];
            let dir = DIRS45[random.next_int(8) as usize];
            self.make_space(n, layer, middle, dir);
        }
        self.lines.extend(dump(self.board));
    }

    fn item_drag(&mut self, phase: &str, n: usize, at: FloatPoint, dir: (i32, i32)) {
        self.settings.layer = 0;
        let session = DragSession::start(self.board, &self.settings, &self.drag, at);
        let press = self
            .line(phase, n)
            .str("kind", "press")
            .str("at", &fpoint(at))
            .str("press", Self::press_kind(&session));
        let Some(DragSession::Item(item_drag)) = &session else {
            let line = press.num("maxGeneratedId", max_id(self.board)).end();
            self.lines.push(line);
            return;
        };
        let item = item_drag.item();
        let component = self.board.item(item).expect("item").component_id();
        let group = if component > 0 {
            self.board.component_items(component).len()
        } else {
            1
        };
        let line = press
            .num("item", i64::from(item.0))
            .num("group", group as i64)
            .num("maxGeneratedId", max_id(self.board))
            .end();
        self.lines.push(line);
        let mut session = session.expect("session");
        for k in 1..=DRAG_STEPS {
            let to = FloatPoint::new(
                at.x + f64::from(dir.0) * STEP * f64::from(k),
                at.y + f64::from(dir.1) * STEP * f64::from(k),
            );
            let step = session.drag_to(self.board, &self.settings, to, DragBudget::JAVA);
            let (outcome, vector, changed) = match &step {
                DragStep::Unchanged => ("unchanged", "-".to_string(), "-".to_string()),
                DragStep::Fixed => ("fixed", "-".to_string(), "-".to_string()),
                DragStep::Blocked => ("blocked", "-".to_string(), "-".to_string()),
                DragStep::Moved { vector } => ("moved", vector_text(vector), "-".to_string()),
                DragStep::Failed {
                    vector,
                    changed_nets,
                } => (
                    "failed",
                    vector_text(vector),
                    join_ints(changed_nets.iter().copied()),
                ),
                DragStep::MadeSpace { .. } => unreachable!("an item drag"),
            };
            let DragSession::Item(item_drag) = &session else {
                unreachable!("an item drag");
            };
            let previous = item_drag.previous_location();
            let line = self
                .line(phase, n)
                .str("kind", "step")
                .num("k", i64::from(k))
                .str("to", &fpoint(to))
                .str("outcome", outcome)
                .str("vector", &vector)
                .str("changed", &changed);
            let line = self
                .item_state(line, item)
                .str("previous", &fpoint(previous))
                .num("maxGeneratedId", max_id(self.board))
                .end();
            self.lines.push(line);
            self.count_step();
            if step.ends_drag() {
                return;
            }
        }
        let DragRelease::Item { changed_nets } = session.release(self.board) else {
            unreachable!("an item drag");
        };
        let line = self
            .line(phase, n)
            .str("kind", "release")
            .str("changed", &join_ints(changed_nets))
            .num("maxGeneratedId", max_id(self.board))
            .end();
        self.lines.push(line);
    }

    fn item_state(&self, line: Line, id: ItemId) -> Line {
        let item = self.board.item(id).expect("item");
        let center = item
            .drill()
            .map_or_else(|| "-".to_string(), |d| point_text(&d.center.into()));
        let location = if item.component_id() > 0 {
            self.board
                .components
                .get(ComponentId(item.component_id()))
                .and_then(|c| c.location())
                .map_or_else(|| "-".to_string(), |p| point_text(&p.into()))
        } else {
            "-".to_string()
        };
        line.str("center", &center).str("location", &location)
    }

    fn make_space(&mut self, n: usize, layer: i32, middle: FloatPoint, dir: (i32, i32)) {
        self.settings.layer = layer;
        let h = f64::from(self.board.min_trace_half_width());
        let at = FloatPoint::new(
            middle.x + f64::from(dir.0 * 4) * h,
            middle.y + f64::from(dir.1 * 4) * h,
        );
        let session = DragSession::start(self.board, &self.settings, &self.drag, at);
        let press = self
            .line("space", n)
            .str("kind", "press")
            .num("layer", i64::from(layer))
            .str("at", &fpoint(at))
            .str("press", Self::press_kind(&session))
            .num("maxGeneratedId", max_id(self.board))
            .end();
        self.lines.push(press);
        let Some(mut session @ DragSession::MakeSpace(_)) = session else {
            return;
        };
        for k in 1..=SPACE_STEPS {
            let to = FloatPoint::new(
                middle.x + f64::from(dir.0 * (4 - k)) * h,
                middle.y + f64::from(dir.1 * (4 - k)) * h,
            );
            let step = session.drag_to(self.board, &self.settings, to, DragBudget::JAVA);
            let DragStep::MadeSpace { step, warp_pointer } = step else {
                unreachable!("a make-space drag");
            };
            let DragSession::MakeSpace(space) = &session else {
                unreachable!("a make-space drag");
            };
            let route = space.route();
            let line = self
                .line("space", n)
                .str("kind", "step")
                .num("k", i64::from(k))
                .str("to", &fpoint(to))
                .bool("ended", step.ends_route())
                .str("lastCorner", &point_text(&route.last_corner().into()))
                .num("failing", opt_id(route.shove_failing_obstacle()))
                .bool("warp", warp_pointer.is_some())
                .num("maxGeneratedId", max_id(self.board))
                .end();
            self.lines.push(line);
            self.count_step();
        }
        let DragRelease::MakeSpace { removed } = session.release(self.board) else {
            unreachable!("a make-space drag");
        };
        let line = self
            .line("space", n)
            .str("kind", "release")
            .num("removed", removed as i64)
            .num("maxGeneratedId", max_id(self.board))
            .end();
        self.lines.push(line);
    }
}

/// The probe on `board_name`.
fn drag_script_lines(board_name: &str, with_session: bool) -> (board_probe::Row, Vec<String>) {
    let row = board_probe::fixture(board_name);
    let mut board = board_probe::load(&row);
    let stage = if with_session {
        board_probe::import_session(&mut board, &row);
        "ses"
    } else {
        "load"
    };
    unfix_wiring(&mut board);
    let settings = EditSettings::for_board(&board);
    let mut script = Script {
        board: &mut board,
        stage,
        settings,
        drag: DragSettings::default(),
        steps: 0,
        lines: vec![header(KIND, &row.board)],
    };
    script.run();
    (row, script.lines)
}

fn check_board(board_name: &str, with_session: bool) {
    let (row, lines) = drag_script_lines(board_name, with_session);
    if let Err(e) = board_probe::compare(KIND, &row, &lines) {
        panic!("{e}");
    }
}

#[test]
fn drag_script_matches_java_j2() {
    check_board("Issue026-J2_reference", true);
}

#[test]
fn drag_script_matches_java_bm05() {
    check_board("Issue508-DAC2020_bm05", true);
}

#[test]
fn drag_script_matches_java_bm08() {
    check_board("Issue508-DAC2020_bm08", true);
}

#[test]
fn drag_script_matches_java_rpi_splitter() {
    check_board("Issue143-rpi_splitter", true);
}

#[test]
fn drag_script_matches_java_issue957() {
    check_board("Issue957-smoke-ldo-led", false);
}

// ---------------------------------------------------------------------------
// Own behaviour

fn j2() -> RoutingBoard {
    let row = board_probe::fixture("Issue026-J2_reference");
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row);
    unfix_wiring(&mut board);
    board
}

fn hidden_items(board: &RoutingBoard) -> usize {
    board
        .items()
        .filter(|i| i.net_numbers().contains(&Nets::HIDDEN_NET_NUMBER))
        .count()
}

/// The first make-space press of the probe's kind on J2: next to the middle
/// of a trace's first segment, then across it.
fn j2_make_space(board: &RoutingBoard) -> (EditSettings, FloatPoint, FloatPoint) {
    let h = f64::from(board.min_trace_half_width());
    for trace in board.items().filter_map(|i| i.as_trace()) {
        let middle = trace
            .polyline
            .corner_approx(0)
            .middle_point(trace.polyline.corner_approx(1));
        let mut settings = EditSettings::for_board(board);
        settings.layer = trace.layer;
        for (dx, dy) in DIRS45 {
            let at = FloatPoint::new(
                middle.x + f64::from(dx * 4) * h,
                middle.y + f64::from(dy * 4) * h,
            );
            if let Some(DragSession::MakeSpace(_)) =
                DragSession::start(board, &settings, &DragSettings::default(), at)
            {
                let end = FloatPoint::new(
                    middle.x - f64::from(dx * 4) * h,
                    middle.y - f64::from(dy * 4) * h,
                );
                return (settings, at, end);
            }
        }
    }
    panic!("no make-space start on J2");
}

/// I02.3: make-space pushes copper with a trace of the hidden net, which the
/// release removes again; the drag's first step takes the one snapshot.
#[test]
fn make_space_leaves_no_hidden_net_items() {
    let mut board = j2();
    let (settings, at, end) = j2_make_space(&board);
    let mut session =
        DragSession::start(&board, &settings, &DragSettings::default(), at).expect("make-space");
    assert!(session.is_make_space());
    assert_eq!(session.status(), None);
    assert!(!session.something_dragged());
    let DragSession::MakeSpace(space) = &session else {
        unreachable!();
    };
    assert_eq!(space.route().net_numbers(), &[Nets::HIDDEN_NET_NUMBER]);
    assert_eq!(space.route().start_item(), None);
    assert!(space.route().target_set().is_empty());
    let expected_half_width = (board.min_trace_half_width() / 10).clamp(5, 100);
    assert_eq!(
        space.route().params().half_widths,
        vec![expected_half_width; board.layer_count()]
    );
    let mut advanced = 0;
    for k in 1..=8 {
        let t = f64::from(k) / 8.0;
        let to = FloatPoint::new(at.x + (end.x - at.x) * t, at.y + (end.y - at.y) * t);
        if let DragStep::MadeSpace { step, .. } =
            session.drag_to(&mut board, &settings, to, DragBudget::JAVA)
            && step == freerouting::interactive::RouteStep::Advanced
        {
            advanced += 1;
        }
    }
    assert!(session.something_dragged());
    assert!(advanced > 0, "the hidden trace must grow");
    assert!(hidden_items(&board) > 0, "the hidden trace is on the board");
    let release = session.release(&mut board);
    assert!(matches!(release, DragRelease::MakeSpace { removed } if removed > 0));
    assert_eq!(hidden_items(&board), 0);
    assert!(board.connectable_items(Nets::HIDDEN_NET_NUMBER).is_empty());
}

/// I02.2: a press on a trace only starts nothing; a press on a user-fixed
/// via starts a drag whose steps report it fixed and change nothing.
#[test]
fn press_on_a_trace_starts_nothing_and_a_fixed_via_does_not_move() {
    let mut board = j2();
    let settings = EditSettings::for_board(&board);
    let drag = DragSettings::default();
    let on_trace = board
        .items()
        .filter_map(|i| i.as_trace())
        .map(|t| {
            t.polyline
                .corner_approx(0)
                .middle_point(t.polyline.corner_approx(1))
        })
        .find(|&p| {
            freerouting::interactive::find_item_to_move(&board, &settings, &drag, p) == (None, true)
        })
        .expect("a trace middle with nothing else under it");
    assert!(DragSession::start(&board, &settings, &drag, on_trace).is_none());

    let via = board.items().find(|i| i.is_via()).expect("a via");
    let (id, center) = (via.id(), via.drill().unwrap().center);
    board.set_fixed_state(id, FixedState::UserFixed);
    let mut session =
        DragSession::start(&board, &settings, &drag, center.to_float()).expect("a drag");
    assert_eq!(
        session.status(),
        Some(freerouting::interactive::Status::new("dragging_item", []))
    );
    let to = FloatPoint::new(f64::from(center.x) + 400.0, f64::from(center.y));
    assert_eq!(
        session.drag_to(&mut board, &settings, to, DragBudget::JAVA),
        DragStep::Fixed
    );
    assert!(!session.something_dragged());
    assert_eq!(board.item(id).unwrap().drill().unwrap().center, center);
    assert_eq!(
        session.release(&mut board),
        DragRelease::Item {
            changed_nets: Default::default()
        }
    );
}

/// I02.2: the first successful step takes the drag's snapshot, so one undo
/// restores the board as it was before the drag.
#[test]
fn a_drag_is_undone_by_one_undo() {
    let board = j2();
    let settings = EditSettings::for_board(&board);
    let drag = DragSettings::default();
    let before = dump(&board);
    let pins: Vec<(ItemId, freerouting::geometry::IntPoint)> = board
        .items()
        .filter(|i| i.is_pin() && i.net_count() > 0)
        .map(|i| (i.id(), i.drill().unwrap().center))
        .collect();
    for (id, center) in pins {
        let mut copy = board.clone();
        let Some(mut session) = DragSession::start(&copy, &settings, &drag, center.to_float())
        else {
            continue;
        };
        let mut moved = 0;
        for k in 1..=3 {
            let to = FloatPoint::new(
                f64::from(center.x) + 200.0 * f64::from(k),
                f64::from(center.y),
            );
            if let DragStep::Moved { .. } =
                session.drag_to(&mut copy, &settings, to, DragBudget::JAVA)
            {
                moved += 1;
            }
        }
        if moved < 2 {
            continue;
        }
        let DragRelease::Item { changed_nets } = session.release(&mut copy) else {
            unreachable!();
        };
        assert!(changed_nets.iter().all(|n| *n > 0));
        assert_ne!(copy.item(id).unwrap().drill().unwrap().center, center);
        assert!(copy.undo(None));
        assert_eq!(copy.item(id).unwrap().drill().unwrap().center, center);
        let mut after = dump(&copy);
        let mut expected = before.clone();
        // the ids generated by the drag stay generated
        after.pop();
        expected.pop();
        assert_eq!(after, expected);
        return;
    }
    panic!("no J2 pin dragged twice");
}

/// I02.5: the committed 40-command drag script on bm05 (`tests/data/edit_script_drag_bm05.json`:
/// the probe's first three via drags, two pin drags and three steps of a make-space drag)
/// replays its first three drags exactly as the probe ran them, which are the first operations
/// on the board in both; budgets do not decide them (Java's and unlimited agree).
#[test]
fn edit_script_replays_the_probe_drags() {
    let text = std::fs::read_to_string(
        board_probe::repo_root().join("crates/freerouting/tests/data/edit_script_drag_bm05.json"),
    )
    .expect("script");
    assert!(drag_script::is_drag_script(&text));
    let script = drag_script::parse_drag_script(&text).expect("parse");
    assert_eq!(script.commands.len(), 40);
    let row = board_probe::fixture("Issue508-DAC2020_bm05");
    let replay = |budget| {
        let mut board = board_probe::load(&row);
        board_probe::import_session(&mut board, &row);
        drag_script::replay_drag(&mut board, &script, budget)
    };
    let lines = replay(DragBudget::JAVA);
    assert_eq!(lines, replay(DragBudget::UNLIMITED));
    let java_text = std::fs::read_to_string(
        board_probe::repo_root()
            .join("fixtures/reference/probe/drag-script/Issue508-DAC2020_bm05.jsonl"),
    )
    .expect("probe output");
    let field = |line: &str, key: &str| -> Option<String> {
        let json = edit_script::parse_json(line).ok()?;
        match json.get(key)? {
            edit_script::Json::Str(s) => Some(s.clone()),
            edit_script::Json::Number(n) => Some(format!("{n}")),
            _ => None,
        }
    };
    // the probe's via drags 0-2: press, five steps, release each
    let java: Vec<&str> = java_text
        .lines()
        .filter(|l| l.contains(r#""phase":"via""#))
        .filter(|l| {
            [r#""drag":0,"#, r#""drag":1,"#, r#""drag":2,"#]
                .iter()
                .any(|d| l.contains(d))
        })
        .collect();
    assert_eq!(java.len(), 21);
    for (j, r) in java.iter().zip(&lines[..21]) {
        assert_eq!(
            field(j, "maxGeneratedId"),
            field(r, "maxGeneratedId"),
            "{j}\n{r}"
        );
        match field(j, "kind").as_deref() {
            Some("press") => assert_eq!(field(r, "item"), field(j, "item"), "{j}\n{r}"),
            Some("step") => {
                assert_eq!(field(r, "result"), field(j, "outcome"), "{j}\n{r}");
                if field(j, "vector").as_deref() != Some("-") {
                    assert_eq!(field(r, "vector"), field(j, "vector"), "{j}\n{r}");
                }
            }
            _ => assert_eq!(field(r, "changed"), field(j, "changed"), "{j}\n{r}"),
        }
    }
    let space: Vec<&String> = lines[35..40].iter().collect();
    assert!(space[0].contains(r#""press":"makespace""#), "{}", space[0]);
    assert!(space[4].contains(r#""cmd":"release""#), "{}", space[4]);
}
