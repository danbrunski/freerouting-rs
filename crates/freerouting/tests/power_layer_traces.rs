//! Q13: `router.power_layer_traces`, a plane net's traces stay inside its
//! own plane on a power layer.
//!
//! - Q13.1: under `own_plane` the maze search lays no trace on a power layer
//!   outside the net's own plane there; under `layer` (unset, Java's rule
//!   from `7f56b062d`) it may, and does on the fixture.
//! - Q13.2: the engine's check ([`power_layer_breaches`]) flags such a trace
//!   on a board, here the fixed `+5V_PI` tracks of canary-7a's placement run
//!   a (its add-only input carries Java `7f56b062d`'s pass 1 as fixed
//!   wires). The parity harness's report of new traces is tested next to the
//!   harness (`autoroute_board_parity.rs`).
//!
//! The fixtures are synthetic three-layer boards in `tests/data/quality/`:
//! - `q13-two-islands.dsn`: power layer In1.Cu has two plane islands, PA
//!   (left) and PB (right). Each net has a through-hole pin inside its island
//!   and one between the islands, outside both, so Java's rule joins PB's
//!   outer pin to PB's island with a trace on In1.Cu that leaves the island.
//! - `q13-via-at-border.dsn`: a fixed via of PB whose centre is 50 um
//!   outside PB's island (its copper overlaps the island; a via joins a
//!   plane only with its centre inside), wired on F.Cu to PB's pad. The
//!   search's target shapes are enlarged by the clearance compensation, so
//!   the hop from the via to the island has no length there, and the
//!   insertion lays a trace from the via into the island: the case Q13.3's
//!   first run found on m-fc-g474.

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::power_layer_traces::power_layer_breaches;
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::job::prepare::{JobError, JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::profile::legacy_pin;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const ISLANDS: &str = "crates/freerouting/tests/data/quality/q13-two-islands.dsn";
const VIA_AT_BORDER: &str = "crates/freerouting/tests/data/quality/q13-via-at-border.dsn";
const EXP_A: &str = "crates/freerouting/tests/data/canary-pwr-q13/exp-a-addonly.dsn";

const OWN_PLANE: &[(&str, &str)] = &[("power_layer_traces", "own_plane")];

/// The job's load sequence on `dsn` with the test settings, `legacy` pinned,
/// and `extra` (`path=value` pairs set on the testing source).
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
    input.extra_sources = vec![
        (Source::Testing, legacy_pin()),
        (Source::Testing, profile.patch),
    ];
    prepare_job(&input, &ResolveContext { cores: 1 })
}

fn route(dsn: &str, extra: &[(&str, &str)]) -> RoutingBoard {
    let prepared = prepare(dsn, extra).unwrap_or_else(|e| panic!("{dsn}: {e}"));
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
    outcome.board.take().expect("no listener took the board")
}

/// `net on layer: kind` for every breach, in item order.
fn breaches(board: &BasicBoard) -> Vec<String> {
    power_layer_breaches(board)
        .into_iter()
        .map(|(id, kind)| {
            let item = board.item(id).expect("a trace");
            let net = item.net_numbers().first().copied().unwrap_or(0);
            let net = board.rules.nets.get(net).map_or("?", |n| n.name.as_str());
            let layer = &board.layer_structure.layers[item.first_layer() as usize].name;
            format!("{net} on {layer}: {kind:?}")
        })
        .collect()
}

/// The traces on power layers, as `net on layer`.
fn power_layer_traces(board: &BasicBoard) -> Vec<String> {
    board
        .items()
        .filter(|i| i.is_trace())
        .filter(|i| !board.layer_structure.layers[i.first_layer() as usize].is_signal)
        .map(|i| {
            let net = i.net_numbers().first().copied().unwrap_or(0);
            let net = board.rules.nets.get(net).map_or("?", |n| n.name.as_str());
            let layer = &board.layer_structure.layers[i.first_layer() as usize].name;
            format!("{net} on {layer}")
        })
        .collect()
}

fn unrouted(board: &RoutingBoard) -> usize {
    BoardStatistics::new(board).connections.incomplete_count
}

#[test]
fn java_s_rule_leaves_pb_s_island_on_the_power_layer() {
    // Q13.1's premise: with `layer` (unset) the search joins B2, between the
    // islands, to PB's island with a trace on In1.Cu that starts outside it.
    let board = route(ISLANDS, &[]);
    assert_eq!(unrouted(&board), 0);
    assert_eq!(power_layer_traces(&board), ["PB on In1.Cu"]);
    assert_eq!(breaches(&board), ["PB on In1.Cu: OutsideOwnPlane"]);
    let explicit = route(ISLANDS, &[("power_layer_traces", "layer")]);
    assert_eq!(
        explicit.state_hash(),
        board.state_hash(),
        "`layer` is the default"
    );
}

#[test]
fn own_plane_keeps_every_trace_inside_its_island() {
    // Q13.1: under `own_plane` PB connects on a signal layer instead, with a
    // via that lands inside PB's island, and no trace lies on In1.Cu.
    let board = route(ISLANDS, OWN_PLANE);
    assert_eq!(unrouted(&board), 0, "both nets still connect");
    assert_eq!(power_layer_traces(&board), Vec::<String>::new());
    assert_eq!(breaches(&board), Vec::<String>::new());
    let pb = board
        .rules
        .nets
        .get_by_name("PB", 1)
        .expect("net PB")
        .net_number;
    let island = board
        .conduction_areas()
        .into_iter()
        .filter_map(|id| board.item(id))
        .find(|a| a.contains_net(pb))
        .and_then(|a| a.as_conduction_area().map(|c| c.area().clone()))
        .expect("PB's island");
    let vias: Vec<_> = board.items().filter(|i| i.is_via()).collect();
    assert_eq!(vias.len(), 1, "one via for PB");
    let centre = vias[0].drill().expect("a drill").center.to_float();
    assert!(vias[0].contains_net(pb));
    assert!(
        island.contains_float(centre),
        "the via lands in PB's island"
    );
}

#[test]
fn own_plane_refuses_the_hop_from_a_via_just_outside_the_island() {
    // Java's rule: a trace on In1.Cu from the via (outside) into the island.
    let layer = route(VIA_AT_BORDER, &[]);
    assert_eq!(unrouted(&layer), 0);
    assert_eq!(breaches(&layer), ["PB on In1.Cu: OutsideOwnPlane"]);
    // `own_plane`: no trace on In1.Cu; PB reaches its island with a new via
    // inside it.
    let own = route(VIA_AT_BORDER, OWN_PLANE);
    assert_eq!(unrouted(&own), 0, "PB still connects");
    assert_eq!(power_layer_traces(&own), Vec::<String>::new());
    let new_vias: Vec<f64> = own
        .items()
        .filter(|i| i.is_via() && !i.is_user_fixed())
        .map(|i| i.drill().expect("a drill").center.to_float().x)
        .collect();
    assert_eq!(new_vias.len(), 1, "{new_vias:?}");
    assert!(new_vias[0] > 200_000.0, "inside the island: {new_vias:?}");
}

#[test]
fn the_check_flags_the_fixed_5v_pi_tracks_of_placement_run_a() {
    // Q13.2: canary-7a's placement run a, pass 1 by Java `7f56b062d`, fixed
    // into its add-only input: `+5V_PI` on Power.Cu along (112.2, 54.0) ->
    // (114.0, 55.9) -> (114.0, 60.2), outside its island (x 107.3-112.2,
    // y 45.2-55.0).
    let prepared = prepare(EXP_A, &[]).unwrap_or_else(|e| panic!("{EXP_A}: {e}"));
    let found = breaches(&prepared.board);
    assert_eq!(
        found,
        [
            "+5V_PI on Power.Cu: OutsideOwnPlane",
            "+5V_PI on Power.Cu: OutsideOwnPlane"
        ],
        "both segments of the fixed track"
    );
}

#[test]
fn unknown_power_layer_traces_values_fail_the_job() {
    let err = match prepare(ISLANDS, &[("power_layer_traces", "island")]) {
        Ok(_) => panic!("an unknown value must fail the job"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.contains("router.power_layer_traces") && err.contains("'island'"),
        "{err}"
    );
}
