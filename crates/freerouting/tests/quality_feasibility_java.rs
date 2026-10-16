//! Q03.7: the infeasible-connection diagnostic against Java's routes.
//!
//! For every fixture with a Java-routed session in
//! `fixtures/reference/route/` (Java at `cda540f`), the check runs on the
//! unrouted board, then the session is imported into a fresh copy. A hard
//! false positive is a pin reported `infeasible` at width W on which Java's
//! routed board has a trace of width ≥ W on one of the pin's layers whose
//! end point lies inside the pin's shape there, with no clearance violation
//! involving that trace (Rust DRC). The preregistered target is zero.
//!
//! Also printed, per board: H, the pins reported infeasible, and N, how
//! many of those Java connected at all (with a narrower trace or a via).
//! Run with `cargo test --release -p freerouting --test
//! quality_feasibility_java -- --ignored --nocapture`.

use std::path::{Path, PathBuf};

use freerouting::drc::load_design;
use freerouting::io::read_session;
use freerouting::quality::feasibility::{FeasibilityOptions, StartModel, Verdict, check_board};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

/// The per-board counts.
#[derive(Default, Debug)]
struct Counts {
    infeasible: usize,
    connected: usize,
    hard_false_positives: Vec<String>,
    /// Java trace ends inside an infeasible pin: total, and at its centre.
    trace_ends: usize,
    trace_ends_at_centre: usize,
}

fn check_fixture(stem: &str, options: &FeasibilityOptions) -> Option<Counts> {
    let dsn = fixtures_dir().join(format!("{stem}.dsn"));
    let bytes = std::fs::read(&dsn).ok()?;
    let board = load_design(&bytes).ok()?;
    let report = check_board(&board, options);
    let infeasible: Vec<_> = report
        .pins
        .iter()
        .filter(|p| p.verdict == Verdict::Infeasible)
        .collect();
    let mut counts = Counts {
        infeasible: infeasible.len(),
        ..Counts::default()
    };
    if infeasible.is_empty() {
        return Some(counts);
    }
    let mut routed = load_design(&bytes).expect("board loads twice");
    let ses = fixtures_dir().join(format!("reference/route/{stem}.ses"));
    let text = std::fs::read(&ses).expect("session");
    read_session(&mut routed, &String::from_utf8_lossy(&text))
        .unwrap_or_else(|e| panic!("{stem}: {e}"));
    for p in infeasible {
        let pin = routed.item(p.id).expect("pin keeps its id");
        let drill = pin.drill().expect("pin");
        if !routed.normal_contacts(p.id).is_empty() {
            counts.connected += 1;
        }
        let class = routed.rules.net_class_of(p.net_number).expect("class");
        for trace_id in routed.traces() {
            let item = routed.item(trace_id).expect("trace");
            let Some(trace) = item.as_trace() else {
                continue;
            };
            if !item.contains_net(p.net_number) {
                continue;
            }
            let Some(shape) = drill.shape_on_layer(trace.layer) else {
                continue;
            };
            let ends = [trace.first_corner(), trace.last_corner()];
            let inside: Vec<_> = ends.iter().filter(|e| shape.contains(e)).collect();
            if inside.is_empty() {
                continue;
            }
            counts.trace_ends += inside.len();
            let centre = freerouting::geometry::Point::Int(drill.center);
            counts.trace_ends_at_centre += inside.iter().filter(|e| ***e == centre).count();
            let full_width = 2 * trace.half_width >= 2 * class.trace_half_width(trace.layer);
            if full_width && routed.clearance_violations(trace_id).is_empty() {
                counts.hard_false_positives.push(format!(
                    "{stem}: {}-{} ({}) trace {} half width {}",
                    p.component, p.pin, p.net, trace_id.0, trace.half_width
                ));
            }
        }
    }
    Some(counts)
}

/// The totals over all reference boards for one start model.
#[derive(Default, Debug)]
struct Totals {
    boards: usize,
    infeasible: usize,
    connected: usize,
    trace_ends: usize,
    trace_ends_at_centre: usize,
    false_positives: Vec<String>,
}

fn run(stems: &[String], options: &FeasibilityOptions) -> Totals {
    let mut totals = Totals::default();
    println!("start model {}: board\tH\tN", options.start.name());
    for stem in stems {
        let Some(counts) = check_fixture(stem, options) else {
            println!("{stem}\tnot loaded");
            continue;
        };
        totals.boards += 1;
        totals.infeasible += counts.infeasible;
        totals.connected += counts.connected;
        totals.trace_ends += counts.trace_ends;
        totals.trace_ends_at_centre += counts.trace_ends_at_centre;
        if counts.infeasible > 0 {
            println!("{stem}\t{}\t{}", counts.infeasible, counts.connected);
        }
        totals.false_positives.extend(counts.hard_false_positives);
    }
    println!(
        "start model {}: {} boards, H = {}, N = {}, {} hard false positives, \
         {} Java trace ends inside infeasible pins ({} at the centre)",
        options.start.name(),
        totals.boards,
        totals.infeasible,
        totals.connected,
        totals.false_positives.len(),
        totals.trace_ends,
        totals.trace_ends_at_centre,
    );
    for fp in &totals.false_positives {
        println!("  {fp}");
    }
    totals
}

/// Zero hard false positives with the default start model (the pin's
/// shapes). The centre model's counts are printed for comparison: it is
/// canary-09's model and is not expected to be a proof against Java.
/// `Q03_ONLY=<substring>` restricts the boards.
#[test]
#[ignore = "about two minutes in release; run with --release --ignored --nocapture"]
fn no_hard_false_positives_against_java_routes() {
    let mut stems: Vec<String> = std::fs::read_dir(fixtures_dir().join("reference/route"))
        .expect("route references")
        .filter_map(|e| {
            let path = e.ok()?.path();
            if path.extension()? != "ses" {
                return None;
            }
            Some(path.file_stem()?.to_string_lossy().into_owned())
        })
        .collect();
    stems.sort();
    if let Ok(only) = std::env::var("Q03_ONLY") {
        stems.retain(|s| s.contains(&only));
    }
    let started = std::time::Instant::now();
    let shape = run(&stems, &FeasibilityOptions::default());
    let centre = run(
        &stems,
        &FeasibilityOptions {
            start: StartModel::PinCentre,
            ..FeasibilityOptions::default()
        },
    );
    println!("{:.1} s", started.elapsed().as_secs_f64());
    if std::env::var_os("Q03_ONLY").is_none() {
        assert!(
            shape.boards > 100,
            "only {} reference boards loaded",
            shape.boards
        );
    }
    assert!(centre.boards == shape.boards);
    assert!(
        shape.false_positives.is_empty(),
        "hard false positives:\n{}",
        shape.false_positives.join("\n")
    );
}
