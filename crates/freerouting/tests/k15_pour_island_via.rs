//! K15.29: a via in a pour island KiCad's filler removes is no contact in the
//! stub cleanup.
//!
//! The boards are `tests/data/kicad/pcu-k1529/`, written by `make_boards.py`
//! with every coordinate of the zones and the copper taken from the pcu
//! board's DSN route (K15.27, `target/orchestrator/k1527/runs/pcu-dsn-off-dflt`,
//! session `55618a37...`), where KiCad reports `via_dangling` for
//! `Net-(U24-OUT)`'s via at (61.2182, 118.9218) mm: zones USB_OUT (F.Cu) and
//! USB_OUT_POUR (B.Cu), the via, `BUS_GND`'s track and via along y 120.4697
//! and `Net-(J13-CC2)`'s diagonal. Each board is there as KiCad 10.0.7's
//! `pcb drc --refill-zones --save-board` saved it, and as KiCad's Specctra
//! export of that (`.dsn`).
//!
//! - `a-via-island-cut-off`: the `BUS_GND` copper cuts USB_OUT's fill in
//!   two. The via's part has no pad, so KiCad's filler removes it
//!   (`island_removal_mode` 0); USB_OUT_POUR has no pad at all, so KiCad
//!   keeps its fill. KiCad: `via_dangling` "Via [Net-(U24-OUT)] on F.Cu -
//!   B.Cu" at (61.2182, 118.9218).
//! - `b-no-bus-gnd`: without the `BUS_GND` copper USB_OUT's fill is one
//!   piece with U24's pad and the via. KiCad: no `via_dangling` of the net.
//!
//! KiCad's filler applies the boards' `.kicad_dru` clearance (0.127 mm), not
//! the zones' 0.5 mm. Told 0.5 mm (`pour_clearance_um`, as the plugin passes
//! the zones' value), the DSN's fill model cuts USB_OUT in two on `b` as well;
//! the islands are judged on the router's clearances, so the via stays there
//! (K15.29's island model).

mod autoroute_common;

use std::collections::BTreeSet;

use autoroute_common::repo_root;
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::io::kicad::{ReadOptions, read_kicad_bytes};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::quality::pour_fill::PourFills;
use freerouting::quality::stub_contacts::{via_contact_layers, via_contact_layers_with};
use freerouting::quality::stubs::remove_stubs;
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_geometry::IntPoint;

const DIR: &str = "crates/freerouting/tests/data/kicad/pcu-k1529";
const OUT: &str = "Net-(U24-OUT)";
const CUT: &str = "a-via-island-cut-off";
const WHOLE: &str = "b-no-bus-gnd";

/// What a DSN input is told about its zones.
#[derive(Clone, Copy, Debug)]
enum Zones {
    /// The pcu board's values, as the KiCad plugin passes them: min thickness
    /// and clearance 0.5 mm, islands removed.
    Pcu,
    /// The same, islands not removed (before K15.29's plugin flag).
    KeepIslands,
    /// The same, with `Net-(U24-OUT)` among the nets that keep them.
    OutKeepsIslands,
}

fn settings() -> SettingsPatch {
    SettingsPatch {
        profile: Some("enhanced".to_string()),
        ..SettingsPatch::default()
    }
}

fn prepared(input: JobInput) -> RoutingBoard {
    prepare_job(&input, &ResolveContext { cores: 1 })
        .unwrap()
        .board
}

/// The `.kicad_pcb` board `name`, with every `island_removal_mode` set to
/// `mode`.
fn kicad_board(name: &str, mode: u8) -> RoutingBoard {
    let path = repo_root().join(DIR).join(format!("{name}.kicad_pcb"));
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text.matches("(island_removal_mode 0)").count(), 2);
    let text = text.replace(
        "(island_removal_mode 0)",
        &format!("(island_removal_mode {mode})"),
    );
    let read = read_kicad_bytes(&path, text.as_bytes(), ReadOptions::default()).unwrap();
    let mut input = JobInput::new(Vec::new(), name);
    input.set_kicad(read);
    input.extra_sources.push((Source::Testing, settings()));
    prepared(input)
}

/// The DSN board `name`, told `zones`.
fn dsn_board(name: &str, zones: Zones) -> RoutingBoard {
    let text = std::fs::read(repo_root().join(DIR).join(format!("{name}.dsn"))).unwrap();
    let mut input = JobInput::new(text, name);
    let mut patch = settings();
    patch.cleanup.pour_min_thickness_um = Some(500.0);
    patch.cleanup.pour_clearance_um = Some(500.0);
    match zones {
        Zones::Pcu => patch.cleanup.pour_island_removal = Some(true),
        Zones::KeepIslands => {}
        Zones::OutKeepsIslands => {
            patch.cleanup.pour_island_removal = Some(true);
            patch.cleanup.pour_islands_kept_nets =
                Some(vec!["BUS_GND".to_string(), OUT.to_string()]);
        }
    }
    input.extra_sources = vec![(Source::Testing, patch)];
    prepared(input)
}

/// `Net-(U24-OUT)`'s via at (61.2182, 118.9218) mm (KiCad's DSN export
/// rounds y to 118.922).
fn the_via(board: &RoutingBoard) -> ItemId {
    let net = board.rules.nets.get_by_name(OUT, 1).unwrap().net_number;
    let vias: Vec<ItemId> = board
        .items()
        .filter(|i| i.is_via() && i.contains_net(net))
        .map(|i| i.id())
        .collect();
    assert_eq!(vias.len(), 1);
    let centre = board.item(vias[0]).unwrap().drill().unwrap().center;
    let expected = IntPoint::new(612_182, -1_189_218);
    assert!(
        (centre.x - expected.x).abs() <= 2 && (centre.y - expected.y).abs() <= 2,
        "{centre:?}"
    );
    vias[0]
}

/// The via's contact layers by the pours' outlines (Q16.4) and by their
/// fill model as the cleanup takes it.
fn contact_layers(board: &RoutingBoard) -> (BTreeSet<i32>, BTreeSet<i32>) {
    let via = board.item(the_via(board)).unwrap();
    (
        via_contact_layers(board, via),
        via_contact_layers_with(board, via, Some(&PourFills::new())),
    )
}

/// F.Cu and B.Cu.
fn outer(board: &RoutingBoard) -> BTreeSet<i32> {
    let last = board.layer_structure.layers.len() as i32 - 1;
    BTreeSet::from([0, last])
}

/// The cleanup on `board`, judging only the via (as the router's copper):
/// the vias it removes.
fn clean(mut board: RoutingBoard) -> usize {
    let via = the_via(&board);
    board.unfix(via);
    let keep: BTreeSet<ItemId> = board
        .items()
        .map(|i| i.id())
        .filter(|id| *id != via)
        .collect();
    let summary = remove_stubs(&mut board, &keep);
    assert_eq!(summary.traces, 0);
    assert_eq!(board.item(via).is_none(), summary.vias == 1);
    summary.vias
}

/// On the `.kicad_pcb`: the via's F.Cu island is cut off from U24's pad, so
/// the via touches only USB_OUT_POUR and the cleanup removes it; with the
/// fill whole it touches both pours and stays. The outlines touch it on both
/// layers either way.
#[test]
fn the_via_in_the_cut_off_island_is_no_contact_on_the_kicad_pcb() {
    let cut = kicad_board(CUT, 0);
    let both = outer(&cut);
    let b_cu = BTreeSet::from([*both.last().unwrap()]);
    assert_eq!(contact_layers(&cut), (both.clone(), b_cu));
    assert_eq!(clean(cut), 1);

    let whole = kicad_board(WHOLE, 0);
    assert_eq!(contact_layers(&whole), (both.clone(), both));
    assert_eq!(clean(whole), 0);
}

/// A zone that keeps its islands (`island_removal_mode` 1) keeps the via's.
#[test]
fn a_zone_that_keeps_its_islands_keeps_the_via() {
    let cut = kicad_board(CUT, 1);
    let both = outer(&cut);
    assert_eq!(contact_layers(&cut), (both.clone(), both));
    assert_eq!(clean(cut), 0);
}

/// On the DSN, told the pcu board's values and island removal, as on the
/// `.kicad_pcb`.
#[test]
fn the_via_in_the_cut_off_island_is_no_contact_on_the_dsn() {
    let cut = dsn_board(CUT, Zones::Pcu);
    let both = outer(&cut);
    let b_cu = BTreeSet::from([*both.last().unwrap()]);
    assert_eq!(contact_layers(&cut), (both.clone(), b_cu));
    assert_eq!(clean(cut), 1);

    let whole = dsn_board(WHOLE, Zones::Pcu);
    assert_eq!(contact_layers(&whole), (both.clone(), both));
    assert_eq!(clean(whole), 0);
}

/// On the DSN without island removal, or with the net among those that keep
/// their islands, the via keeps both contacts.
#[test]
fn without_island_removal_the_dsn_keeps_the_via() {
    for zones in [Zones::KeepIslands, Zones::OutKeepsIslands] {
        let cut = dsn_board(CUT, zones);
        let both = outer(&cut);
        assert_eq!(contact_layers(&cut), (both.clone(), both), "{zones:?}");
        assert_eq!(clean(cut), 0, "{zones:?}");
    }
}
