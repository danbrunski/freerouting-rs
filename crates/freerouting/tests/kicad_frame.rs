//! K07.4: the coordinate frame of the direct reader against KiCad's.
//!
//! `fixtures/kicad/Issue558-dev-board-autoroute-demo/dev-board.kicad10.java.ses`
//! is the session Java (`a773a70ec`, `-mp 100`, optimizer off, 0 unrouted)
//! wrote for KiCad 10.0.7's export of the board, `dev-board.kicad10.dsn`
//! (see `fixtures/kicad/README.md`). Step 2 of the checklist: import it onto
//! the board built from the reader's tree and onto the board built from the
//! export; every imported trace corner and via must be at the same board
//! coordinates, and the two boards must agree on what the session connects.
//! `tools/kicad-reader/frame_check.py` does step 3 (KiCad's own import).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemKind;
use freerouting::drc::DesignRulesChecker;
use freerouting::dsn::ast::Pcb;
use freerouting::io::kicad::{ReadOptions, read_kicad_input};
use freerouting::io::read_session;
use freerouting::io::specctra::{BoardReadResult, SesStyle, build_board, write_ses};

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/kicad/Issue558-dev-board-autoroute-demo")
}

fn board_of(r: BoardReadResult) -> RoutingBoard {
    match r {
        BoardReadResult::Success { board, .. } => *board,
        other => panic!("{other:?}"),
    }
}

/// The reader's board without its Q22.16 footprint mask-aperture keepouts
/// (`solder_mask_pads`, over the USB-C pads), which KiCad's export does not
/// write: Java's session, routed without them, runs 82 times within their
/// clearance, and this test is about the frame.
fn boards() -> (RoutingBoard, RoutingBoard) {
    let mut input =
        read_kicad_input(&dir().join("dev-board.kicad_pcb"), ReadOptions::default()).unwrap();
    let keepouts = &mut input.pcb.structure.as_mut().unwrap().keepouts;
    let before = keepouts.len();
    keepouts.retain(|k| {
        k.area.name.as_deref()
            != Some(freerouting_kicad::footprint_masks::FOOTPRINT_MASK_KEEPOUT_NAME)
    });
    // J1's 12 F.Mask polygons, each with the four strips of the board's
    // solder-mask-to-copper clearance (Q22.12)
    assert_eq!(before - keepouts.len(), 60);
    let export =
        Pcb::parse(&std::fs::read_to_string(dir().join("dev-board.kicad10.dsn")).unwrap()).unwrap();
    (
        board_of(build_board(&input.pcb)),
        board_of(build_board(&export)),
    )
}

/// Traces (layer, net, corners) and vias (padstack, net, centre) of a board.
fn routes(board: &RoutingBoard) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut traces = BTreeSet::new();
    let mut vias = BTreeSet::new();
    for item in board.items() {
        let nets: Vec<String> = item
            .net_numbers()
            .iter()
            .map(|n| board.rules.nets.get(*n).unwrap().name.clone())
            .collect();
        match &item.kind {
            ItemKind::Trace(t) => {
                let corners: Vec<String> = t
                    .polyline
                    .corners()
                    .iter()
                    .map(|c| format!("{:?}", c.to_float()))
                    .collect();
                traces.insert(format!(
                    "{} {nets:?} w{} {}",
                    t.layer,
                    t.half_width,
                    corners.join(" ")
                ));
            }
            ItemKind::Via(v) => {
                vias.insert(format!(
                    "{:?} {nets:?} {:?}",
                    v.drill.padstack, v.drill.center
                ));
            }
            _ => {}
        }
    }
    (traces, vias)
}

fn drc_counts(board: &RoutingBoard) -> (usize, usize, usize) {
    let findings = DesignRulesChecker::new(board).check();
    (
        findings.clearance_violations.len(),
        findings.incompletes.iter().map(|n| n.count()).sum(),
        findings.unconnected_items.len(),
    )
}

/// Step 2: the session lands at identical coordinates on both boards.
#[test]
fn java_session_lands_at_the_same_coordinates() {
    let ses = std::fs::read_to_string(dir().join("dev-board.kicad10.java.ses")).unwrap();
    let (mut reader, mut export) = boards();
    let before = (routes(&reader), routes(&export));
    assert!(
        before.0.0.is_empty() && before.1.0.is_empty(),
        "the board has no wiring"
    );
    let a = read_session(&mut reader, &ses).unwrap();
    let b = read_session(&mut export, &ses).unwrap();
    assert_eq!(
        (a.wires_imported, a.vias_imported),
        (b.wires_imported, b.vias_imported)
    );
    assert_eq!(a.errors_encountered, 0);
    assert_eq!(b.errors_encountered, 0);
    let (traces_r, vias_r) = routes(&reader);
    let (traces_e, vias_e) = routes(&export);
    assert!(!traces_r.is_empty() && !vias_r.is_empty());
    assert_eq!(traces_r, traces_e, "trace corners");
    assert_eq!(vias_r, vias_e, "via centres");
    // the session connects everything on both boards, and the checks agree
    let (rc, ec) = (drc_counts(&reader), drc_counts(&export));
    assert_eq!(rc, ec);
    assert_eq!(rc.1, 0, "no incomplete connection is left");
    println!(
        "imported {} wires and {} vias; clearance violations / incompletes / unconnected: {rc:?}",
        a.wires_imported, a.vias_imported
    );
}

/// The reader's board writes the same routes back: a session written from the
/// board after the import has the Java session's wires and vias.
#[test]
fn session_written_from_the_reader_board_has_the_same_routes() {
    let java = std::fs::read_to_string(dir().join("dev-board.kicad10.java.ses")).unwrap();
    let (mut reader, _) = boards();
    read_session(&mut reader, &java).unwrap();
    let ours = write_ses(&reader, "dev-board", SesStyle::Java);
    let (j, o) = (
        freerouting::dsn::Session::parse(&java).unwrap(),
        freerouting::dsn::Session::parse(&ours).unwrap(),
    );
    // per net, the set of items: the order of nets and of the items in a net is not compared
    let key = |s: &freerouting::dsn::Session| -> std::collections::BTreeMap<String, Vec<String>> {
        s.routes
            .as_ref()
            .unwrap()
            .network_out
            .iter()
            .map(|n| {
                let mut items: Vec<String> = n
                    .items
                    .iter()
                    // an imported item is user-fixed, so it is written back as `(type protect)`
                    .map(|i| format!("{i:?}").replace("Some(Protect)", "None"))
                    .collect();
                items.sort();
                (n.name.clone(), items)
            })
            .collect()
    };
    let (kj, ko) = (key(&j), key(&o));
    assert_eq!(kj.keys().collect::<Vec<_>>(), ko.keys().collect::<Vec<_>>());
    for (net, items) in &kj {
        assert_eq!(items, &ko[net], "net {net}");
    }
    assert_eq!(j.placement, o.placement);
    if let Ok(out) = std::env::var("FR_KICAD_FRAME_OUT") {
        std::fs::write(Path::new(&out).join("dev-board.rust.ses"), ours).unwrap();
    }
}
