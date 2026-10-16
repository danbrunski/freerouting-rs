//! O02.4: the any-angle pull-tight ends under a work limit, at the same
//! cut-off as Java.
//!
//! Java's comment in `TraceTightenerAnyAngle.pullTight` says that
//! `reduceCorners` may loop with `smoothenCorners` when consecutive corners
//! are closer than one grid unit. The port has Java's stop checks and no
//! other, so such a run ends where Java's ends: at the first stop check whose
//! work units exceed the limit.
//!
//! The board is built here, not read from a fixture: one signal layer, nine
//! rows of 14 pads of net `B`, and nine traces of net `A` that weave around
//! the rows. At every inner corner a seeded extra line passes within one unit
//! of the corner, so consecutive corners lie closer than one grid unit. The
//! whole board is marked and `opt_changed_area` runs with a limit of 1 and of
//! 2 nominal ms; both fire before the run is done. The probe
//! `PullTightAnyProbe` (sections `budget-1`, `budget-2` of
//! `fixtures/reference/probe/pull-tight-any/Issue026-J2_reference.jsonl`)
//! builds the same board in Java, from the same text and the same
//! `java.util.Random` seed.

mod board_probe;
#[path = "support/item_dump.rs"]
mod item_dump;

use board_probe::{Line, repo_root};
use freerouting::board::RoutingBoard;
use freerouting::board::structure::{AngleRestriction, FixedState};
use freerouting::drc::load_design;
use freerouting::geometry::jmath::JavaRandom;
use freerouting::geometry::{IntPoint, Line as Line2, Polyline};
use freerouting::optimize::PullTightParams;

const ROWS: i32 = 9;
const PADS: i32 = 14;
const SEED: i64 = 20261018;
const ACCURACY: i32 = 500;

/// The design text (`PullTightAnyProbe.budgetDsn`): pads in um, 10 board
/// units per um.
fn budget_dsn() -> String {
    let mut places = String::new();
    let mut pins = Vec::new();
    let mut n = 1;
    for row in 0..ROWS {
        for i in 0..PADS {
            places.push_str(&format!(
                "      (place P{n} {} {} front 0)\n",
                1000 + 2000 * i,
                2000 + 3000 * row
            ));
            pins.push(format!("P{n}-1"));
            n += 1;
        }
    }
    format!(
        "(pcb budget.dsn
  (parser
    (string_quote \")
    (space_in_quoted_tokens on)
  )
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu
      (type signal)
      (property
        (index 0)
      )
    )
    (boundary
      (path pcb 0  0 0  30000 0  30000 30000  0 30000  0 0)
    )
    (rule
      (width 250)
      (clearance 200)
    )
  )
  (placement
    (component PAD
{places}      (place U1 300 2000 front 0)
      (place U2 29700 2000 front 0)
    )
  )
  (library
    (image PAD
      (pin CirclePad_F_800_um 1 0 0)
    )
    (padstack CirclePad_F_800_um
      (shape (circle F.Cu 800))
      (attach off)
    )
  )
  (network
    (net A (pins U1-1 U2-1))
    (net B (pins {}))
    (class kicad_default A B
    )
  )
)
",
        pins.join(" ")
    )
}

/// The lines of the trace weaving around the pad row at `y0`
/// (`PullTightAnyProbe.budgetLines`): a corner above or below every pad,
/// jittered by up to 100, and after each inner corner a line through a point
/// within one unit of it in a random direction.
fn budget_lines(random: &mut JavaRandom, y0: i32) -> Vec<Line2> {
    let mut points = vec![IntPoint::new(3000, y0)];
    for i in 0..PADS {
        let x = 10000 + 20000 * i;
        let y = if i % 2 == 0 { y0 + 12000 } else { y0 - 12000 };
        let dx = random.next_int(201) - 100;
        let dy = random.next_int(201) - 100;
        points.push(IntPoint::new(x + dx, y + dy));
    }
    points.push(IntPoint::new(297000, y0));
    let n = points.len();
    let mut lines = vec![Line2::new(points[0], IntPoint::new(3000, y0 + 1))];
    for i in 0..n - 1 {
        lines.push(Line2::new(points[i], points[i + 1]));
        if i + 1 < n - 1 {
            let p = points[i + 1];
            let ax = random.next_int(3) - 1;
            let ay = random.next_int(3) - 1;
            let a = IntPoint::new(p.x + ax, p.y + ay);
            let bx = random.next_int(2001) - 1000;
            let by = random.next_int(2001) - 1000;
            let b = IntPoint::new(a.x + bx, a.y + by);
            if a != b {
                lines.push(Line2::new(a, b));
            }
        }
    }
    lines.push(Line2::new(points[n - 1], IntPoint::new(297000, y0 + 1)));
    lines
}

/// The board with the nine traces, angle restriction `None`.
fn budget_board() -> RoutingBoard {
    let mut board = load_design(budget_dsn().as_bytes()).expect("the budget design loads");
    board
        .rules
        .set_trace_angle_restriction(AngleRestriction::None);
    let mut random = JavaRandom::new(SEED);
    for row in 0..ROWS {
        let polyline = Polyline::new(&budget_lines(&mut random, 20000 + 30000 * row));
        board.insert_trace_without_cleaning(polyline, 0, 1250, &[1], 1, FixedState::Unfixed);
    }
    board
}

/// The section `budget-<ms>`: the stop checks, the units at the last one,
/// whether the limit fired, then the dump; and whether the run stopped.
fn budget_section(limit_ms: i64) -> (Vec<String>, bool) {
    let mut board = budget_board();
    let mut lines = vec![
        Line::new()
            .str("stage", &format!("budget-{limit_ms}"))
            .num("limitMs", limit_ms)
            .end(),
    ];
    board.start_marking_changed_area();
    board.mark_all_changed_area();
    let mut params = PullTightParams::new(ACCURACY);
    params.limit = Some(limit_ms);
    let run = board.opt_changed_area(&params);
    let used = run.units_at_last_check.expect("a limited run checks") as i64;
    lines.push(
        Line::new()
            .num("checks", run.stop_checks as i64)
            .num("unitsAtLastCheck", used)
            .bool("exceeded", used > limit_ms * 64_000)
            .end(),
    );
    lines.extend(item_dump::dump(&board));
    (lines, run.stopped)
}

/// Java's section `name` of the J2 output of probe `pull-tight-any`.
fn java_section(name: &str) -> Vec<String> {
    let path =
        repo_root().join("fixtures/reference/probe/pull-tight-any/Issue026-J2_reference.jsonl");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; run PullTightAnyProbe on J2", path.display()));
    let stage = format!("{{\"stage\":\"{name}\"");
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let starts_section = line.starts_with("{\"stage\":\"");
        if starts_section && !lines.is_empty() {
            break;
        }
        if line.starts_with(&stage) || !lines.is_empty() {
            lines.push(line.to_string());
        }
    }
    assert!(!lines.is_empty(), "Java has no section {name}");
    lines
}

/// The board has the case of Java's comment: consecutive corners closer
/// than one grid unit.
#[test]
fn budget_board_has_corners_closer_than_one_unit() {
    let board = budget_board();
    let mut close = 0;
    let mut traces = 0;
    for item in board.items() {
        let Some(trace) = item.as_trace() else {
            continue;
        };
        traces += 1;
        let p = &trace.polyline;
        close += (1..p.corner_count())
            .filter(|&i| {
                let d = p.corner_approx(i).distance(p.corner_approx(i - 1));
                d > 0.0 && d < 1.0
            })
            .count();
    }
    assert_eq!(traces, ROWS as usize);
    assert!(close > 0, "no consecutive corners closer than one unit");
}

/// O02.4: with a limit of 1 and of 2 nominal ms the run returns, cut off by
/// the limit; 100 runs give the same dump and the same units at the cut-off;
/// Java's run on the same board gives the same.
#[test]
fn any_angle_terminates_under_budget() {
    for limit_ms in [1, 2] {
        let (first, stopped) = budget_section(limit_ms);
        assert!(stopped, "limit {limit_ms} ms didn't fire");
        for run in 1..100 {
            let (again, _) = budget_section(limit_ms);
            assert_eq!(again, first, "run {run} with limit {limit_ms} ms differs");
        }
        let java = java_section(&format!("budget-{limit_ms}"));
        let mismatch = (0..java.len().max(first.len())).find(|&i| java.get(i) != first.get(i));
        if let Some(i) = mismatch {
            panic!(
                "budget-{limit_ms} line {} differs\n  Java: {}\n  Rust: {}",
                i + 1,
                java.get(i).map_or("<end>", String::as_str),
                first.get(i).map_or("<end>", String::as_str)
            );
        }
    }
}
