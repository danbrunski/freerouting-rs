//! Q02.2: no width ratchet (Q00 rule 7; KRT #842, where a write-back made
//! a 0.127 mm neck the board's width on the next run). Routing a board,
//! writing its DSN (C06) and SES (C05), reading them back and routing again
//! leaves every net class width, every net's class and the rules'
//! narrowest width as loaded, also with narrow copper on the board (the
//! router's neck-down at U3-2 and the pre-routed pinch of
//! `q02-pinch.dsn`). The writers are C05's and C06's; this only checks
//! them.

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::io::specctra::dsn_writer::write_dsn;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::quality::neckdown::width_report::report;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const PINCH: &str = "crates/freerouting/tests/data/quality/q02-pinch.dsn";
const QFN: &str = "crates/freerouting/tests/data/quality/q03-qfn-0p5.dsn";

/// Every class as `(name, half width per layer)`, every net as `(name,
/// class name)`, and the rules' narrowest half width. (The board's own
/// `min_trace_half_width` is a statistic of the traces on it, not a rule:
/// it drops whenever a narrower trace is inserted, in Java too.)
type Widths = (Vec<(String, Vec<i32>)>, Vec<(String, String)>, i32);

fn widths(board: &BasicBoard) -> Widths {
    let layers = board.layer_structure.layers.len() as i32;
    let classes = &board.rules.net_classes;
    let class_widths = (0..classes.count())
        .filter_map(|i| classes.nth(i))
        .map(|id| {
            let class = classes.get(id);
            let per_layer = (0..layers).map(|l| class.trace_half_width(l)).collect();
            (class.name().to_string(), per_layer)
        })
        .collect();
    let nets = (1..=board.rules.nets.max_net_number())
        .filter_map(|n| board.rules.nets.get(n))
        .map(|net| {
            let class = classes.get(net.net_class()).name().to_string();
            (net.name.clone(), class)
        })
        .collect();
    (class_widths, nets, board.rules.min_trace_half_width())
}

fn prepare(bytes: Vec<u8>, name: &str, ses: Option<Vec<u8>>) -> PreparedJob {
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(false);
    profile.set_max_passes(2);
    let mut input = JobInput::new(bytes, name.to_string());
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = None;
    input.ses = ses;
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn route(prepared: PreparedJob) -> RoutingBoard {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome = run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{}", f.error));
    outcome.board.take().expect("no listener took the board")
}

fn check_round_trip(dsn: &str) {
    let path = repo_root().join(dsn);
    let name = Path::new(dsn).file_name().unwrap().to_str().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let loaded = prepare(bytes.clone(), name, None);
    let input = widths(&loaded.board);
    let first = route(loaded);
    assert_eq!(widths(&first), input, "{dsn}: routing changed a width");

    // The DSN the C06 writer makes of the routed board, read and routed.
    let written = write_dsn(&first, name);
    let reread = prepare(written.into_bytes(), name, None);
    assert_eq!(widths(&reread.board), input, "{dsn}: the written DSN");
    let second = route(reread);
    assert_eq!(widths(&second), input, "{dsn}: routing the written DSN");

    // The input DSN with the C05 session imported, routed again.
    let ses = write_ses(&first, name, SesStyle::Java);
    let with_ses = prepare(bytes, name, Some(ses.into_bytes()));
    assert_eq!(widths(&with_ses.board), input, "{dsn}: DSN plus SES");
    let third = route(with_ses);
    assert_eq!(widths(&third), input, "{dsn}: routing DSN plus SES");
}

#[test]
fn round_trips_leave_every_class_width_as_loaded() {
    for dsn in [PINCH, QFN] {
        check_round_trip(dsn);
    }
}

#[test]
fn the_round_trip_board_really_carries_narrow_copper() {
    // The test above means something only if narrow copper went through
    // the writers: the pinch and the router's neck.
    let bytes = std::fs::read(repo_root().join(PINCH)).unwrap();
    let first = route(prepare(bytes, "q02-pinch.dsn", None));
    let r = report(&first);
    for net in ["PWR", "PWR2"] {
        assert!(r.net(net).is_some_and(|n| n.under > 0.0), "{net}");
    }
}
