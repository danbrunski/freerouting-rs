//! K07.2: the GUI opens a `.kicad_pcb` through the engine's direct reader.

mod common;

use common::plain_harness;
use freerouting::io::kicad::{ReadOptions, read_kicad_input};
use freerouting::io::specctra::{BoardReadResult, build_board};

const CORNEY: &str = "kicad/Issue368-CorneyIslandWireless/corney_island_wireless.kicad_pcb";

/// `Issue368-CorneyIslandWireless` opens; its board has as many items as
/// `build_board` makes of the reader's tree, and the warnings window gets the
/// read report.
#[test]
fn opens_a_kicad_board() {
    let harness = plain_harness(&[CORNEY]);
    let loaded = harness.state().loaded().expect("board loaded");
    let path = common::fixtures().join(CORNEY);
    let input = read_kicad_input(&path, ReadOptions::default()).unwrap();
    let BoardReadResult::Success { board, .. } = build_board(&input.pcb) else {
        panic!("the reader's tree builds a board");
    };
    assert_eq!(
        loaded.session.board.basic().item_count(),
        board.item_count(),
        "the session's board is the board of the reader's tree"
    );
    assert!(loaded.session.board.basic().item_count() > 30);
    assert!(!loaded.scene.items.is_empty());
    assert!(
        loaded
            .session
            .paths
            .design
            .ends_with("corney_island_wireless.kicad_pcb")
    );
    assert!(
        loaded
            .session
            .design
            .name
            .ends_with("corney_island_wireless.dsn")
    );
    assert!(
        loaded
            .session
            .warnings
            .iter()
            .any(|w| w.starts_with("KiCad read report: rule areas:")),
        "{:?}",
        loaded.session.warnings
    );
}
