//! Q01: pours of listed net classes block only pass-through nets
//! (`quality::pour_guard`), and the pour-cut report (`quality::pour_audit`).
//!
//! The fixtures are synthetic one-layer boards in `tests/data/quality/`
//! (not `fixtures/`, whose boards all need Java references):
//! - `q01-pour-between-pads.dsn`: a 12 mm wide pour of net Z (class `Pwr`)
//!   on F.Cu between the two pads of net S, with room around it on F.Cu;
//! - `q01-pad-inside-pour.dsn`: the same with a pad of net T inside a hole
//!   in the pour and T's other pad above the pour.

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::items::{Item, ItemId};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::geometry::{PolylineShapeOps, TileShape};
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobError, JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::quality::pour_audit::audit;
use freerouting::quality::pour_guard::ExemptionTable;
use freerouting::rules::AreaObstaclePolicy;
use freerouting::scoring::BoardStatistics;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const BETWEEN: &str = "crates/freerouting/tests/data/quality/q01-pour-between-pads.dsn";
const INSIDE: &str = "crates/freerouting/tests/data/quality/q01-pad-inside-pour.dsn";
const CANARY_PLANES: &str = "fixtures/canary-pwr/canary-pwr-pours-as-planes.dsn";

/// The job's load sequence on `dsn` with the test settings and `extra`
/// (`path=value` pairs set on the testing source).
fn prepare(dsn: &str, extra: &[(&str, &str)]) -> Result<PreparedJob, JobError> {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(false);
    profile.set_max_passes(10);
    for (key, value) in extra {
        profile
            .patch
            .set_path(key, value)
            .unwrap_or_else(|e| panic!("{key}: {e:?}"));
    }
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    // These boards pin routes that predate Q18's class floor (`enhanced`'s
    // `board_minimums.from_classes`); what is tested here is not the width.
    profile
        .patch
        .board_minimums
        .from_classes
        .get_or_insert(false);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 })
}

/// The prepared board before routing and the routed board.
fn route(dsn: &str, extra: &[(&str, &str)]) -> (RoutingBoard, RoutingBoard) {
    let prepared = prepare(dsn, extra).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let before = prepared.board.deep_copy();
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome =
        run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{dsn}: {}", f.error));
    let after = outcome.board.take().expect("no listener took the board");
    (before, after)
}

const GUARD_PWR: &[(&str, &str)] = &[("plane_obstacle_net_classes", "Pwr")];

fn net_number(board: &BasicBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .unwrap_or_else(|| panic!("net {name}"))
        .net_number
}

/// The conduction area of net `net`.
fn pour_of(board: &BasicBoard, net: i32) -> ItemId {
    board
        .conduction_areas()
        .into_iter()
        .find(|id| board.item(*id).is_some_and(|a| a.contains_net(net)))
        .expect("the pour")
}

fn tiles(board: &BasicBoard, item: &Item) -> Vec<TileShape> {
    (0..board.tile_shape_count(item))
        .filter_map(|i| board.tile_shape(item, i))
        .collect()
}

/// The traces of net `net` that overlap pour `area` by area.
fn traces_crossing(board: &BasicBoard, net: i32, area: ItemId) -> usize {
    let area = board.item(area).expect("area");
    let area_tiles = tiles(board, area);
    board
        .items()
        .filter(|i| i.is_trace() && i.contains_net(net) && i.first_layer() == area.first_layer())
        .filter(|trace| {
            tiles(board, trace).iter().any(|t| {
                area_tiles
                    .iter()
                    .any(|a| t.intersection(a).dimension() == 2)
            })
        })
        .count()
}

/// The incomplete connections of net `net`.
fn incomplete(board: &BasicBoard, net: i32) -> usize {
    let mut checker = freerouting::drc::DesignRulesChecker::new(board);
    checker.net_incompletes_of(net).map_or(0, |n| n.count())
}

fn violations(board: &BasicBoard) -> usize {
    BoardStatistics::new(board).clearance_violations.total_count
}

// ---------------------------------------------------------------------------
// Q01.1: the exemption table
// ---------------------------------------------------------------------------

#[test]
fn a_pad_inside_a_hole_of_the_pour_is_exempt() {
    let board = prepare(INSIDE, &[]).expect("loads").board;
    let table = ExemptionTable::build(&board, &["Pwr".to_string()]).expect("builds");
    let z = net_number(&board, "Z");
    let t = net_number(&board, "T");
    let area = pour_of(&board, z);
    assert_eq!(table.listed_areas().collect::<Vec<_>>(), vec![area]);
    assert_eq!(table.exempt_nets(area), Some(&[t][..]));
    assert!(!table.blocks(area, t));
}

#[test]
fn a_pad_outside_the_pour_is_not_exempt() {
    let board = prepare(INSIDE, &[]).expect("loads").board;
    let table = ExemptionTable::build(&board, &["Pwr".to_string()]).expect("builds");
    let s = net_number(&board, "S");
    let area = pour_of(&board, net_number(&board, "Z"));
    assert!(!table.exempt_nets(area).expect("listed").contains(&s));
    assert!(table.blocks(area, s));
}

#[test]
fn the_area_own_net_is_never_listed() {
    let board = prepare(BETWEEN, &[]).expect("loads").board;
    let table = ExemptionTable::build(&board, &["Pwr".to_string()]).expect("builds");
    let z = net_number(&board, "Z");
    let area = pour_of(&board, z);
    // Z's two pads lie inside its pour.
    assert_eq!(table.exempt_nets(area), Some(&[][..]));
    assert_eq!(table.exemption_count(), 0);
}

#[test]
fn an_unlisted_class_lists_no_pour() {
    let board = prepare(BETWEEN, &[]).expect("loads").board;
    // The class of S, whatever the reader named it.
    let s = board.rules.nets.get(net_number(&board, "S")).expect("S");
    let class = board
        .rules
        .net_classes
        .get(s.net_class())
        .name()
        .to_string();
    assert_ne!(class, "Pwr");
    let table = ExemptionTable::build(&board, &[class]).expect("builds");
    assert!(table.is_empty());
}

#[test]
fn an_unknown_class_name_fails_with_its_name() {
    let board = prepare(BETWEEN, &[]).expect("loads").board;
    let error = ExemptionTable::build(&board, &["Pwr".to_string(), "Nope".to_string()])
        .expect_err("unknown class");
    assert!(error.to_string().contains("'Nope'"), "{error}");
    assert!(
        error.to_string().contains("Pwr"),
        "names the board's classes: {error}"
    );
    let job_error =
        prepare(BETWEEN, &[("plane_obstacle_net_classes", "Nope")]).expect_err("the job fails");
    assert!(job_error.to_string().contains("'Nope'"), "{job_error}");
    assert!(
        job_error
            .to_string()
            .contains("router.plane_obstacle_net_classes"),
        "{job_error}"
    );
}

// ---------------------------------------------------------------------------
// Q01.2: installing the policy
// ---------------------------------------------------------------------------

#[test]
fn the_policy_is_installed_only_when_the_key_lists_a_class() {
    let off = prepare(BETWEEN, &[]).expect("loads");
    assert!(off.board.rules.area_obstacle_policy.is_none());
    let empty = prepare(BETWEEN, &[("plane_obstacle_net_classes", "")]).expect("loads");
    assert!(empty.board.rules.area_obstacle_policy.is_none());
    let on = prepare(BETWEEN, GUARD_PWR).expect("loads");
    let policy = on.board.rules.area_obstacle_policy.as_ref().expect("on");
    let area = pour_of(&on.board, net_number(&on.board, "Z"));
    assert!(policy.blocks(area, net_number(&on.board, "S")));
}

#[test]
fn plane_as_obstacle_wins_and_the_policy_is_not_installed() {
    let prepared = prepare(
        BETWEEN,
        &[
            ("plane_obstacle_net_classes", "Pwr"),
            ("plane_as_obstacle", "true"),
        ],
    )
    .expect("loads");
    assert!(prepared.board.rules.area_obstacle_policy.is_none());
    assert!(
        prepared
            .log
            .iter()
            .any(|l| l.text.contains("router.plane_as_obstacle")),
        "the log says why"
    );
}

/// D8: a signal-layer pour of a net with a power-layer plane is dropped by
/// the reader, so it is never listed; nor is any power-layer plane.
#[test]
fn pours_of_nets_with_a_power_plane_stay_ignored() {
    let board = prepare(CANARY_PLANES, &[]).expect("loads").board;
    let table = ExemptionTable::build(&board, &["Power_High".to_string(), "GND".to_string()])
        .expect("builds");
    assert!(!table.is_empty());
    let plane_nets: Vec<i32> = board
        .conduction_areas()
        .into_iter()
        .filter_map(|id| board.item(id))
        .filter(|a| !board.layer_structure.layers[a.first_layer() as usize].is_signal)
        .flat_map(|a| a.net_numbers().to_vec())
        .collect();
    for net in ["+12V_BUS", "+5V_PI", "GND"] {
        assert!(
            plane_nets.contains(&net_number(&board, net)),
            "{net} has a plane"
        );
    }
    for area in table.listed_areas() {
        let item = board.item(area).expect("area");
        assert!(board.layer_structure.layers[item.first_layer() as usize].is_signal);
        assert!(
            !item.net_numbers().iter().any(|n| plane_nets.contains(n)),
            "listed pour {area} belongs to a net with a power-layer plane"
        );
    }
}

// ---------------------------------------------------------------------------
// Q01.3: routing
// ---------------------------------------------------------------------------

#[test]
fn with_the_key_empty_s_crosses_the_pour() {
    let (before, after) = route(BETWEEN, &[]);
    let s = net_number(&after, "S");
    let area = pour_of(&after, net_number(&after, "Z"));
    assert_eq!(incomplete(&after, s), 0, "S routes");
    assert!(traces_crossing(&after, s, area) > 0, "S crosses the pour");
    assert_eq!(violations(&after), violations(&before));
}

#[test]
fn with_the_key_set_s_routes_around_the_pour() {
    let (before, after) = route(BETWEEN, GUARD_PWR);
    let s = net_number(&after, "S");
    let area = pour_of(&after, net_number(&after, "Z"));
    assert_eq!(incomplete(&after, s), 0, "S routes");
    assert_eq!(traces_crossing(&after, s, area), 0, "S stays off the pour");
    assert_eq!(violations(&after), violations(&before));
    assert!(
        !after.items().any(|i| i.is_via()),
        "no via on a one-layer board"
    );
}

#[test]
fn t_routes_into_the_hole_in_both_cases() {
    for extra in [&[][..], GUARD_PWR] {
        let (before, after) = route(INSIDE, extra);
        let t = net_number(&after, "T");
        let s = net_number(&after, "S");
        let area = pour_of(&after, net_number(&after, "Z"));
        assert_eq!(incomplete(&after, t), 0, "T routes ({extra:?})");
        assert!(traces_crossing(&after, t, area) > 0, "T crosses the pour");
        assert_eq!(incomplete(&after, s), 0, "S routes ({extra:?})");
        if !extra.is_empty() {
            assert_eq!(traces_crossing(&after, s, area), 0, "S stays off the pour");
        }
        assert_eq!(violations(&after), violations(&before), "{extra:?}");
        assert!(!after.items().any(|i| i.is_via()));
    }
}

// ---------------------------------------------------------------------------
// Q01.4: the pour-cut report
// ---------------------------------------------------------------------------

#[test]
fn the_report_names_the_cut_with_the_key_empty_and_none_with_it_set() {
    let (before, after) = route(BETWEEN, &[]);
    let report = audit(&before, &after);
    let z = report
        .nets
        .iter()
        .find(|n| n.net_name == "Z")
        .expect("Z is a pour net");
    assert!(z.islands_after > z.islands_before, "{z:?}");
    assert_eq!(z.cuts.len(), 1, "{z:?}");
    assert_eq!(z.cuts[0].by_nets, vec!["S".to_string()]);
    assert!(!report.is_clean());
    assert!(report.to_json().contains("\"by_nets\":[\"S\"]"));

    let (before, after) = route(BETWEEN, GUARD_PWR);
    let report = audit(&before, &after);
    assert!(report.is_clean(), "{report:?}");
    let z = report.nets.iter().find(|n| n.net_name == "Z").expect("Z");
    assert_eq!(z.islands_after, z.islands_before);
    assert!(z.cuts.is_empty());
}

#[test]
fn the_session_is_the_same_with_the_report_on_and_off() {
    let (before, after) = route(BETWEEN, &[]);
    let off = write_ses(&after, "q01-pour-between-pads", SesStyle::Java);
    let report = audit(&before, &after);
    assert!(!report.nets.is_empty());
    let on = write_ses(&after, "q01-pour-between-pads", SesStyle::Java);
    assert_eq!(off, on);
    // And a second routing run gives the same session (D2).
    let (_, again) = route(BETWEEN, &[]);
    assert_eq!(
        write_ses(&again, "q01-pour-between-pads", SesStyle::Java),
        off
    );
}

/// Q01.4's comparison with KiCad's refill: per conduction area of each DSN in
/// `Q01_ISLANDS` (comma separated), the net, layer, island count and the
/// islands with an anchor of the area's net, one JSON line each, to
/// `Q01_ISLANDS_OUT`. Run by hand (release) on KiCad exports; the counts are
/// in Q01.4's done note.
#[test]
#[ignore = "a measurement helper; set Q01_ISLANDS and Q01_ISLANDS_OUT"]
fn dump_island_counts() {
    let list = std::env::var("Q01_ISLANDS").expect("Q01_ISLANDS");
    let out_path = std::env::var("Q01_ISLANDS_OUT").expect("Q01_ISLANDS_OUT");
    let mut out = String::new();
    for dsn in list.split(',').filter(|s| !s.is_empty()) {
        let board = prepare(dsn, &[])
            .unwrap_or_else(|e| panic!("{dsn}: {e}"))
            .board;
        let mut areas = board.conduction_areas();
        areas.sort_by_key(|id| id.0);
        for id in areas {
            let area = board.item(id).expect("area");
            let fill = freerouting::drc::conduction_fill(&board, id);
            let anchored = fill.iter().filter(|i| !i.anchors.is_empty()).count();
            let net = area.net_numbers().first().copied().unwrap_or(0);
            let net_name = board.rules.nets.get(net).map_or("", |n| n.name.as_str());
            let layer = &board.layer_structure.layers[area.first_layer() as usize].name;
            // Each island's smaller bounding-box side in mm, and whether it is anchored.
            let units_per_mm = board
                .communication
                .get_resolution(freerouting::board::structure::Unit::Mm);
            let sides: Vec<String> = fill
                .iter()
                .map(|i| {
                    let b = i.island.bounds;
                    let side = (b[2] - b[0]).min(b[3] - b[1]) / units_per_mm;
                    format!("[{side:.4},{}]", !i.anchors.is_empty())
                })
                .collect();
            out.push_str(&format!(
                "{{\"dsn\":{dsn:?},\"area\":{},\"net\":{net_name:?},\"layer\":{layer:?},\
                 \"islands\":{},\"anchored\":{anchored},\"sides\":[{}]}}\n",
                id.0,
                fill.len(),
                sides.join(",")
            ));
        }
    }
    std::fs::write(&out_path, out).expect("the output");
}
