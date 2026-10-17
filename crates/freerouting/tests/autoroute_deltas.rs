//! G07.4: the router's delta events carry what a listener needs to follow
//! the board, and the GUI's two-step preparation (`prepare_job_settings`,
//! `prepare_board_for_routing`) equals `prepare_job`.

use std::collections::BTreeMap;
use std::sync::Arc;

use freerouting::autoroute::events::{CancelToken, EventSink, ItemGeometry, RouterEvent};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::io::specctra::{BoardReadResult, build_board};
use freerouting::job::prepare::{
    JobInput, RulesChoice, prepare_board_for_routing, prepare_job, prepare_job_settings,
};
use freerouting::settings::ResolveContext;

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    std::fs::read(path).expect("fixture")
}

fn geometries(board: &RoutingBoard) -> BTreeMap<i32, ItemGeometry> {
    board
        .basic()
        .items()
        .filter_map(|item| board.basic().item_geometry(item.id()))
        .map(|g| (g.id.0, g))
        .collect()
}

/// The two-step preparation gives the settings and the board `prepare_job`
/// gives for a design without a rules file or a session.
#[test]
fn split_preparation_equals_prepare_job() {
    let resolve = ResolveContext { cores: 8 };
    for name in ["Issue159-setonix_2hp-pcb.dsn", "Issue143-rpi_splitter.dsn"] {
        let bytes = fixture(name);
        let mut input = JobInput::new(bytes.clone(), name.trim_end_matches(".dsn"));
        input.rules = RulesChoice::None;
        let whole = prepare_job(&input, &resolve).expect("prepared");

        let pcb = freerouting::dsn::Pcb::parse(&freerouting::dsn::decode(&bytes)).expect("DSN");
        let BoardReadResult::Success { board, .. } = build_board(&pcb) else {
            panic!("{name} loads");
        };
        let mut board = *board;
        let (patch, _) = prepare_job_settings(&board, Some(&pcb), &[], &resolve);
        let ready = prepare_board_for_routing(&mut board, &patch, &resolve);
        assert_eq!(ready.settings, whole.settings, "{name}");
        assert_eq!(patch, whole.patch, "{name}");
        assert_eq!(
            board.state_hash(),
            whole.board.state_hash(),
            "{name}: the prepared boards differ"
        );
    }
}

/// Applying the deltas (and replacing the picture at a `Replaced`) to the
/// geometry of the loaded board gives the geometry of the final board.
#[test]
fn deltas_follow_the_board() {
    let resolve = ResolveContext { cores: 8 };
    for name in [
        "Issue159-setonix_2hp-pcb.dsn",
        "Issue684-Autorouter_PCB1_2026-5-8.dsn",
    ] {
        let mut input = JobInput::new(fixture(name), name.trim_end_matches(".dsn"));
        input.rules = RulesChoice::None;
        let prepared = prepare_job(&input, &resolve).expect("prepared");
        let mut shown = geometries(&prepared.board);
        let (sink, rx) = EventSink::channel(1 << 20);
        let job = PipelineJob {
            settings: &prepared.settings,
            board_settings: &prepared.board_settings,
            cancel: Arc::new(CancelToken::new()),
            events: &sink,
            deadline: None,
            registry: None,
        };
        let outcome = run_pipeline(prepared.board, &job)
            .map_err(|f| f.error.to_string())
            .unwrap();
        assert!(outcome.board.is_none(), "the board went to the listener");
        let (mut deltas, mut replaced, mut final_board) = (0, 0, None);
        for event in rx.try_iter() {
            match event {
                RouterEvent::Delta { removed, added } => {
                    deltas += 1;
                    for id in removed {
                        shown.remove(&id.0);
                    }
                    for g in added {
                        shown.insert(g.id.0, g);
                    }
                }
                RouterEvent::Replaced { snapshot } => {
                    replaced += 1;
                    shown = geometries(&snapshot.restore());
                }
                RouterEvent::Finished { board, .. } => final_board = Some(board),
                RouterEvent::Progress(_) => {}
            }
        }
        let final_board = final_board.expect("Finished");
        assert!(
            deltas > 1,
            "{name}: a delta per connection, not one per pass ({deltas})"
        );
        let want = geometries(&final_board);
        assert_eq!(shown.len(), want.len(), "{name}: {replaced} replaced");
        for (id, g) in &want {
            assert_eq!(shown.get(id), Some(g), "{name}: item {id}");
        }
    }
}
