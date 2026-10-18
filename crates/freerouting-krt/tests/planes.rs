//! X02: plane-tap verification on synthetic boards,
//! routed through the whole pipeline. The boards are in `tests/data/`, not
//! `fixtures/krt/` as the checklist planned: every DSN under `fixtures/`
//! needs a Java reference (`reference_load.rs`), as Q01 found for its own
//! synthetic boards.
//!
//! - `x02-floating-island.dsn`: a GND pour on B.Cu with two through-hole
//!   GND pins, and two SMD GND pins on F.Cu whose own vias land on two small
//!   GND pours cut off from it (X02.2).
//!
//! X02.3-X02.4's region repair (`krt-region-repair`) and its split-pour
//! board were removed after X02.5's registered run rejected it (the X02
//! checklist's X02.5 note has the numbers).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::StageRegistry;
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::drc::DesignRulesChecker;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};
use freerouting_krt::planes::verify::{TAP_REPORT_ID, TapReport, TapStatus};

const FLOATING: &str = "crates/freerouting-krt/tests/data/x02-floating-island.dsn";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The job's load sequence on `dsn` (relative to the repository root or
/// absolute) with `extra` (`path=value`) on the testing source; with
/// `testing`, the test profile (no optimizer, at most 10 passes).
fn prepare(dsn: &str, testing: bool, extra: &[(&str, &str)]) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = default_profile();
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
    let (builtin, _) = route_prepared(prepare(FLOATING, true, &[]), None);
    let registry = krt_registry();
    let (krt, _) = route_prepared(prepare(FLOATING, true, &[]), Some(&registry));
    assert_eq!(ses(&builtin, FLOATING), ses(&krt, FLOATING));
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

/// The testing source's starting patch: empty, so every run starts from
/// the default settings. Not `TestingProfile::default()`, which carries
/// Java's fixture-test settings (`java_fixture_test`: copper-to-edge
/// clearance 0) and biased the pre-P04 measurements.
fn default_profile() -> TestingProfile {
    TestingProfile {
        patch: SettingsPatch::default(),
    }
}
