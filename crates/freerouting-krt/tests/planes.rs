//! X02: plane-tap verification and pour region repair on synthetic boards,
//! routed through the whole pipeline. The boards are in `tests/data/`, not
//! `fixtures/krt/` as the checklist planned: every DSN under `fixtures/`
//! needs a Java reference (`reference_load.rs`), as Q01 found for its own
//! synthetic boards.
//!
//! - `x02-floating-island.dsn`: a GND pour on B.Cu with two through-hole
//!   GND pins, and two SMD GND pins on F.Cu whose own vias land on two small
//!   GND pours cut off from it (X02.2);
//! - `x02-split-pour.dsn`: a GND pour on B.Cu cut in two by a pre-routed
//!   trace of net S, with room for a strap around the trace's upper end
//!   (X02.3, X02.4).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::StageRegistry;
use freerouting::board::items::{Item, ItemId};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::drc::DesignRulesChecker;
#[allow(unused_imports)]
use freerouting::geometry::{PolylineShapeOps, TileShapeOps};
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};
use freerouting_krt::planes::PlaneComponents;
use freerouting_krt::planes::repair::REGION_REPAIR_ID;
use freerouting_krt::planes::verify::{TAP_REPORT_ID, TapReport, TapStatus};

const FLOATING: &str = "crates/freerouting-krt/tests/data/x02-floating-island.dsn";
const SPLIT: &str = "crates/freerouting-krt/tests/data/x02-split-pour.dsn";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The job's load sequence on `dsn` (relative to the repository root or
/// absolute) with `extra` (`path=value`) on the testing source; with
/// `testing`, the test profile (no optimizer, at most 10 passes).
fn prepare(dsn: &str, testing: bool, extra: &[(&str, &str)]) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = TestingProfile::default();
    if testing {
        profile.set_optimizer_enabled(false);
        profile.set_max_passes(10);
    }
    for (key, value) in extra {
        profile
            .patch
            .set_path(key, value)
            .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    }
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    if testing || !extra.is_empty() {
        input.extra_sources = vec![(Source::Testing, profile.patch)];
    }
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

/// The registry with this crate's stages.
fn krt_registry() -> StageRegistry {
    let mut registry = StageRegistry::builtin();
    freerouting_krt::register(&mut registry);
    registry
}

/// Routes the prepared job with `registry` (`None`: the built-in one) and
/// returns the routed board and the run's log lines.
fn route_prepared(
    prepared: PreparedJob,
    registry: Option<&StageRegistry>,
) -> (RoutingBoard, Vec<String>) {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry,
    };
    let mut outcome = run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{}", f.error));
    let log = outcome.log.iter().map(|l| l.text.clone()).collect();
    (outcome.board.take().expect("the routed board"), log)
}

/// Routes `dsn` with the test profile, this crate's registry and the board
/// stages `stages` (comma separated, empty for none).
fn route(dsn: &str, stages: &str) -> (RoutingBoard, Vec<String>) {
    let extra: Vec<(&str, &str)> = if stages.is_empty() {
        Vec::new()
    } else {
        vec![("special.stages", stages)]
    };
    let registry = krt_registry();
    route_prepared(prepare(dsn, true, &extra), Some(&registry))
}

fn ses(board: &RoutingBoard, dsn: &str) -> String {
    let name = Path::new(dsn).file_stem().unwrap().to_string_lossy();
    write_ses(board, &name, SesStyle::Java)
}

fn net_number(board: &BasicBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .unwrap_or_else(|| panic!("net {name}"))
        .net_number
}

fn layer_number(board: &BasicBoard, name: &str) -> i32 {
    board
        .layer_structure
        .layers
        .iter()
        .position(|l| l.name == name)
        .unwrap_or_else(|| panic!("layer {name}")) as i32
}

/// The vias whose centre lies inside a pad of any net, on a layer both
/// share (D8: no via in a pad).
fn vias_in_pads(board: &BasicBoard) -> Vec<ItemId> {
    let pins: Vec<&Item> = board.items().filter(|i| i.is_pin()).collect();
    board
        .items()
        .filter(|v| v.is_via())
        .filter(|v| {
            let centre = v.drill().expect("a via has a drill").center;
            pins.iter().any(|pin| {
                (0..board.tile_shape_count(pin)).any(|i| {
                    v.is_on_layer(pin.shape_layer(i))
                        && board
                            .tile_shape(pin, i)
                            .is_some_and(|s| s.contains_float(centre.to_float()))
                })
            })
        })
        .map(Item::id)
        .collect()
}

// ---------------------------------------------------------------------------
// X02.2: the plane-tap report
// ---------------------------------------------------------------------------

#[test]
fn both_pins_on_the_cut_off_islands_are_floating_taps() {
    let board = prepare(FLOATING, true, &[]).board;
    let report = TapReport::build(&board);
    let floating: Vec<&str> = report.floating().map(|p| p.label.as_str()).collect();
    assert_eq!(floating, vec!["P1-1", "P2-1"]);
    for pin in report.floating() {
        assert_eq!(pin.vias.len(), 1, "{}: its own via", pin.label);
        assert!(!pin.direct);
        assert_eq!(pin.layers.len(), 1);
        assert!(!pin.layers[0].on_main && !pin.layers[0].joined);
    }
    let main: Vec<&str> = report
        .pins
        .iter()
        .filter(|p| p.status == TapStatus::Main)
        .map(|p| p.label.as_str())
        .collect();
    assert_eq!(main, vec!["G1-1", "G2-1"]);
    let json = report.to_json();
    assert!(json.starts_with("{\"floating_tap\":2,"), "{json}");
}

#[test]
fn the_drc_incompletes_of_the_net_equal_the_floating_connections() {
    let board = prepare(FLOATING, true, &[]).board;
    let gnd = net_number(&board, "GND");
    let report = TapReport::build(&board);
    let mut checker = DesignRulesChecker::new(&board);
    let incompletes = checker.incomplete_count_of(gnd);
    assert_eq!(incompletes, 2);
    assert_eq!(report.floating_connections(gnd), incompletes);
    assert_eq!(report.floating().count(), incompletes);
}

#[test]
fn the_session_is_the_same_with_the_tap_report_on_and_off() {
    let (off, _) = route(FLOATING, "");
    let (on, log) = route(FLOATING, TAP_REPORT_ID);
    assert_eq!(ses(&off, FLOATING), ses(&on, FLOATING));
    let line = log
        .iter()
        .find(|l| l.starts_with(TAP_REPORT_ID))
        .expect("the report's log line");
    assert!(line.contains("\"pins\":[{\"pin\":\"G1-1\""), "{line}");
}

// ---------------------------------------------------------------------------
// X02.3: default off
// ---------------------------------------------------------------------------

#[test]
fn registering_the_stages_changes_no_session_while_they_are_unselected() {
    for dsn in [SPLIT, FLOATING] {
        let (builtin, _) = route_prepared(prepare(dsn, true, &[]), None);
        let registry = krt_registry();
        let (krt, _) = route_prepared(prepare(dsn, true, &[]), Some(&registry));
        assert_eq!(ses(&builtin, dsn), ses(&krt, dsn), "{dsn}");
    }
}

/// The stage-off sessions of every board in the set file `X02_SET`
/// (`tools/bench/sets/*.txt`, at most `X02_LIMIT` boards), routed with the
/// default settings, once with the built-in registry and once with this
/// crate's stages registered but not selected; byte-identical, and written
/// to `X02_SES_DIR/<stem>.{builtin,krt}.ses` for `freerouting-dev ses-diff`.
#[test]
#[ignore = "a release run over a board set; set X02_SET, X02_SES_DIR (and X02_LIMIT)"]
fn default_off_sessions_on_a_set() {
    let set = std::env::var("X02_SET").expect("X02_SET");
    let dir = PathBuf::from(std::env::var("X02_SES_DIR").expect("X02_SES_DIR"));
    let limit: usize = std::env::var("X02_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);
    std::fs::create_dir_all(&dir).unwrap();
    let text = std::fs::read_to_string(repo_root().join(&set)).unwrap();
    let boards: Vec<&str> = text
        .lines()
        .map(|l| l.split('#').next().unwrap().trim())
        .filter(|l| !l.is_empty())
        .take(limit)
        .collect();
    let registry = krt_registry();
    for dsn in boards {
        let started = std::time::Instant::now();
        let (builtin, _) = route_prepared(prepare(dsn, false, &[]), None);
        let (krt, _) = route_prepared(prepare(dsn, false, &[]), Some(&registry));
        let (a, b) = (ses(&builtin, dsn), ses(&krt, dsn));
        let stem = Path::new(dsn).file_stem().unwrap().to_string_lossy();
        std::fs::write(dir.join(format!("{stem}.builtin.ses")), &a).unwrap();
        std::fs::write(dir.join(format!("{stem}.krt.ses")), &b).unwrap();
        println!(
            "{dsn}: {} ({:.1} s)",
            if a == b { "identical" } else { "DIFFERENT" },
            started.elapsed().as_secs_f64()
        );
        assert_eq!(a, b, "{dsn}");
    }
}

// ---------------------------------------------------------------------------
// X02.4: the repair on a split pour
// ---------------------------------------------------------------------------

#[test]
fn the_split_pour_has_two_regions_and_a_floating_pin_before_repair() {
    let board = prepare(SPLIT, true, &[]).board;
    let (gnd, bcu) = (net_number(&board, "GND"), layer_number(&board, "B.Cu"));
    let pc = PlaneComponents::build(&board);
    assert_eq!(pc.islands_on(gnd, bcu).len(), 2);
    assert_eq!(pc.regions(gnd, bcu).len(), 2);
    let report = TapReport::from_components(&board, &pc);
    let floating: Vec<&str> = report.floating().map(|p| p.label.as_str()).collect();
    assert_eq!(floating, vec!["G3-1"]);
    // Freerouting's connectivity counts the pour as one conductor: the DRC
    // sees no incomplete connection of GND, only the zone-island finding.
    let mut checker = DesignRulesChecker::new(&board);
    assert_eq!(checker.incomplete_count_of(gnd), 0);
    let islands = checker.zone_island_violations();
    assert_eq!(islands.len(), 1);
    assert_eq!(islands[0].kind.type_name(), "isolated_island_unconnected");
}

#[test]
fn the_repair_joins_the_split_pour_with_one_strap() {
    let (off, _) = route(SPLIT, "");
    let (on, log) = route(SPLIT, REGION_REPAIR_ID);
    let (gnd, bcu) = (net_number(&on, "GND"), layer_number(&on, "B.Cu"));

    let pc = PlaneComponents::build(&on);
    // Java's fill ignores same-net traces: still two fill islands, now one
    // electrical group.
    assert_eq!(pc.islands_on(gnd, bcu).len(), 2);
    assert_eq!(pc.regions(gnd, bcu).len(), 1);
    assert!(pc.joined(pc.islands_on(gnd, bcu)[0], pc.islands_on(gnd, bcu)[1]));
    assert_eq!(TapReport::from_components(&on, &pc).floating().count(), 0);
    assert_eq!(
        PlaneComponents::build(&off).regions(gnd, bcu).len(),
        2,
        "routing alone leaves the pour split"
    );

    let straps: Vec<&String> = log
        .iter()
        .filter(|l| l.starts_with(REGION_REPAIR_ID) && l.contains("strap at width"))
        .collect();
    assert_eq!(straps.len(), 1, "{log:#?}");

    // The DRC adds nothing: no clearance violation, no new unconnected item,
    // and the isolated-island finding is gone.
    let off_findings = off.design_rules_check();
    let on_findings = on.design_rules_check();
    assert_eq!(on_findings.clearance_violations.len(), 0);
    assert_eq!(off_findings.clearance_violations.len(), 0);
    assert!(on_findings.unconnected_items.len() <= off_findings.unconnected_items.len());
    assert_eq!(off_findings.zone_islands.len(), 1);
    assert_eq!(on_findings.zone_islands.len(), 0);
    assert!(vias_in_pads(&on).is_empty());

    // The strap is user-fixed GND copper between the two halves' pins, at
    // the ladder's first width (2.0 mm, 20000 board units), never narrower
    // than the class width.
    let class_width = 2 * on.rules.trace_half_width(gnd, bcu);
    let new_gnd_traces: Vec<&Item> = on
        .items()
        .filter(|i| i.is_trace() && i.contains_net(gnd))
        .collect();
    assert!(off.items().all(|i| !(i.is_trace() && i.contains_net(gnd))));
    assert!(!new_gnd_traces.is_empty());
    for t in &new_gnd_traces {
        let width = 2 * t.as_trace().expect("a trace").half_width;
        assert!(width >= class_width, "width {width} < {class_width}");
        assert_eq!(width, 20000);
        assert!(t.is_user_fixed());
    }
    assert!(ses(&on, SPLIT).contains("(type protect)"));
}

/// Writes the session of the split pour routed with the repair on to
/// `X02_SES_OUT`, for `tools/determinism-check.sh`.
#[test]
#[ignore = "for tools/determinism-check.sh; set X02_SES_OUT"]
fn write_the_repaired_session() {
    let out = std::env::var("X02_SES_OUT").expect("X02_SES_OUT");
    let (on, log) = route(SPLIT, REGION_REPAIR_ID);
    let mut text = ses(&on, SPLIT);
    for line in log.iter().filter(|l| l.starts_with(REGION_REPAIR_ID)) {
        text.push_str(line);
        text.push('\n');
    }
    std::fs::write(out, text).unwrap();
}
