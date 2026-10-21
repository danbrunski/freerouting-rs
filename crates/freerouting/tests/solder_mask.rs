//! Q18.7: other nets' copper keeps out of a pad's solder-mask aperture
//! (`router.solder_mask_expansions`, `job::solder_mask`).
//!
//! Fixture `tests/data/quality/q18-mask-bridge.dsn`: net SIG runs straight
//! from P1 (3, 10 mm) to P2 (27, 10 mm) on F.Cu, past the 0.8 mm test point
//! TP1 of net CLK at (15, 10.7 mm). A 0.2 mm trace at y = 10 mm passes
//! 0.2 mm from TP1's copper, the clearance, so it is legal copper; with a
//! 0.5 mm mask expansion KiCad flags it (`solder_mask_bridge`, Track-Pad).

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::job::solder_mask::{MaskExpansion, format_expansions, parse_expansions};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};

const BRIDGE: &str = "crates/freerouting/tests/data/quality/q18-mask-bridge.dsn";
const BRIDGE_PCB: &str = "crates/freerouting/tests/data/quality/q18-mask-bridge.kicad_pcb";
/// TP1's radius in board units (0.1 µm; both fixtures).
const TP_RADIUS: f64 = 4_000.0;

fn prepare(expansions: Option<&str>) -> PreparedJob {
    let path = repo_root().join(BRIDGE);
    let bytes = std::fs::read(&path).unwrap();
    let mut input = JobInput::new(bytes, JobInput::design_name_of(BRIDGE));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    let mut patch = SettingsPatch {
        solder_mask_expansions: expansions.map(str::to_owned),
        ..SettingsPatch::default()
    };
    patch.profile = Some("legacy".to_string());
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = Some(3);
    input.extra_sources = vec![(Source::Testing, patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap()
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

fn segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

/// The smallest copper gap between net SIG's traces on `layer` and TP1, in
/// board units, and the number of SIG traces.
fn gap_to_tp(board: &RoutingBoard, layer: i32) -> (f64, usize) {
    let sig = board.rules.nets.get_by_name("SIG", 1).unwrap().net_number;
    let clk = board.rules.nets.get_by_name("CLK", 1).unwrap().net_number;
    let tp = board
        .items()
        .find(|i| i.is_pin() && i.contains_net(clk))
        .and_then(|i| i.drill())
        .map(|d| (f64::from(d.center.x), f64::from(d.center.y)))
        .expect("TP1");
    let mut gap = f64::INFINITY;
    let mut n = 0;
    for item in board.items().filter(|i| i.contains_net(sig)) {
        let Some(t) = item.as_trace() else { continue };
        n += 1;
        if t.layer != layer {
            continue;
        }
        let pts: Vec<(f64, f64)> = t
            .polyline
            .corner_approx_arr()
            .iter()
            .map(|p| (p.x, p.y))
            .collect();
        for w in pts.windows(2) {
            let d = segment_distance(tp, w[0], w[1]) - TP_RADIUS - f64::from(t.half_width);
            gap = gap.min(d);
        }
    }
    (gap, n)
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

#[test]
fn without_the_expansion_the_trace_passes_inside_the_mask_aperture() {
    let board = route(prepare(None));
    assert_eq!(incomplete(&board), 0);
    let (gap, n) = gap_to_tp(&board, 0);
    assert!(n > 0);
    // legal copper (0.2 mm clearance, 1 unit of rounding), but inside 0.5 mm
    assert!((1999.0..5000.0).contains(&gap), "gap {gap}");
}

#[test]
fn a_front_expansion_keeps_other_nets_out_of_the_aperture() {
    let prepared = prepare(Some("TP1:1=500,0"));
    let log: Vec<String> = prepared.log.iter().map(|l| l.text.clone()).collect();
    assert!(
        log.iter()
            .any(|l| l.contains("Solder-mask apertures: 1 pad")),
        "{log:?}"
    );
    let board = route(prepared);
    assert_eq!(incomplete(&board), 0);
    let (gap, _) = gap_to_tp(&board, 0);
    assert!(gap >= 4999.0, "gap {gap}");
}

#[test]
fn a_back_expansion_leaves_the_front_alone() {
    use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
    let plain = route(prepare(None));
    let back = route(prepare(Some("TP1:1=0,500")));
    assert_eq!(
        write_ses(&plain, BRIDGE, SesStyle::Java),
        write_ses(&back, BRIDGE, SesStyle::Java)
    );
}

#[test]
fn an_expansion_within_the_clearance_changes_nothing() {
    use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
    let plain = route(prepare(None));
    let small = route(prepare(Some("TP1:1=150,150")));
    assert_eq!(
        write_ses(&plain, BRIDGE, SesStyle::Java),
        write_ses(&small, BRIDGE, SesStyle::Java)
    );
}

#[test]
fn unknown_pins_and_bad_entries_are_warnings() {
    let prepared = prepare(Some("TP1:1=500,0;U9:1=300,300"));
    let log: Vec<String> = prepared.log.iter().map(|l| l.text.clone()).collect();
    assert!(
        log.iter().any(|l| l.contains("1 unknown pin(s): U9:1")),
        "{log:?}"
    );
    let bad = prepare(Some("TP1=500"));
    let log: Vec<String> = bad.log.iter().map(|l| l.text.clone()).collect();
    assert!(
        log.iter()
            .any(|l| l.starts_with("Ignoring router.solder_mask_expansions:")),
        "{log:?}"
    );
}

#[test]
fn the_setting_text_round_trips() {
    let list = vec![
        MaskExpansion {
            component: "TP9".into(),
            pin: "1".into(),
            front_um: 0.0,
            back_um: 101.6,
        },
        MaskExpansion {
            component: "U1".into(),
            pin: "EP".into(),
            front_um: 50.8,
            back_um: 0.0,
        },
    ];
    let text = format_expansions(&list);
    assert_eq!(text, "TP9:1=0,101.6;U1:EP=50.8,0");
    assert_eq!(parse_expansions(&text).unwrap(), list);
    assert_eq!(parse_expansions("").unwrap(), Vec::new());
    assert!(parse_expansions("TP9:1=x,1").is_err());
    assert!(parse_expansions("TP9:1=-1,1").is_err());
}

/// The `.kicad_pcb` path: the reader finds TP1's 0.5 mm `solder_mask_margin`
/// (over the board's 0.05 mm), lists only it (P1 and P2 open 0.05 mm, under
/// every clearance), and the job routes around its aperture.
#[test]
fn a_kicad_pcb_input_carries_the_pads_mask_expansions() {
    use freerouting::io::kicad::{ReadOptions, read_kicad_input};
    let read = read_kicad_input(&repo_root().join(BRIDGE_PCB), ReadOptions::default()).unwrap();
    assert_eq!(
        read.extras.solder_mask,
        vec![freerouting_kicad::mask::PadMaskExpansion {
            component: "TP1".into(),
            pin: "1".into(),
            front_nm: 500_000,
            back_nm: 0,
        }]
    );
    let routed = |mask: bool| {
        let mut read =
            read_kicad_input(&repo_root().join(BRIDGE_PCB), ReadOptions::default()).unwrap();
        if !mask {
            read.extras.solder_mask.clear();
        }
        let mut input = JobInput::new(Vec::new(), "q18-mask-bridge");
        input.set_kicad(read);
        let mut patch = SettingsPatch {
            profile: Some("legacy".to_string()),
            ..SettingsPatch::default()
        };
        patch.optimizer.enabled = Some(false);
        patch.autorouter.max_passes = Some(3);
        input.extra_sources.push((Source::Testing, patch));
        let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).unwrap();
        let setting = prepared.settings.solder_mask_expansions.clone();
        (setting, route(prepared))
    };
    let (setting, board) = routed(true);
    assert_eq!(setting.as_deref(), Some("TP1:1=500,0"));
    assert_eq!(incomplete(&board), 0);
    assert!(
        gap_to_tp(&board, 0).0 >= 4999.0,
        "{:?}",
        gap_to_tp(&board, 0)
    );
    let (setting, board) = routed(false);
    assert_eq!(setting, None);
    assert!(
        gap_to_tp(&board, 0).0 < 4999.0,
        "{:?}",
        gap_to_tp(&board, 0)
    );
}

/// `q18-mask-bridge.kicad_pcb` with a `.kicad_pro` setting
/// `solder_mask_to_copper_clearance` (none: no project file).
fn read_with_mask_clearance(clearance_mm: Option<f64>) -> freerouting::io::kicad::KicadInput {
    use freerouting::io::kicad::{ReadOptions, read_kicad_input};
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "q2212-mask-clearance-{}",
        clearance_mm.map_or("none".to_owned(), |c| c.to_string())
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let pcb = dir.join("q18-mask-bridge.kicad_pcb");
    std::fs::copy(repo_root().join(BRIDGE_PCB), &pcb).unwrap();
    let pro = dir.join("q18-mask-bridge.kicad_pro");
    match clearance_mm {
        Some(c) => std::fs::write(
            &pro,
            format!(
                r#"{{"board": {{"design_settings": {{"rules": {{"solder_mask_to_copper_clearance": {c}}}}}}}}}"#
            ),
        )
        .unwrap(),
        None => {
            let _ = std::fs::remove_file(&pro);
        }
    }
    read_kicad_input(&pcb, ReadOptions::default()).unwrap()
}

fn expansions(read: &freerouting::io::kicad::KicadInput) -> Vec<(String, i32, i32)> {
    read.extras
        .solder_mask
        .iter()
        .map(|e| (format!("{}:{}", e.component, e.pin), e.front_nm, e.back_nm))
        .collect()
}

/// Q22.12: the project's `solder_mask_to_copper_clearance` grows each pad's
/// aperture by it (TP1 0.5 + 0.05 mm); 0 is no project at all; a clearance
/// that takes the board's 0.05 mm margin past the 0.2 mm class clearance
/// lists P1 and P2 too. The job routes SIG outside the grown aperture.
#[test]
fn the_projects_mask_to_copper_clearance_grows_the_pad_apertures() {
    let none = expansions(&read_with_mask_clearance(None));
    assert_eq!(none, vec![("TP1:1".to_owned(), 500_000, 0)]);
    assert_eq!(expansions(&read_with_mask_clearance(Some(0.0))), none);
    assert_eq!(
        expansions(&read_with_mask_clearance(Some(0.05))),
        vec![("TP1:1".to_owned(), 550_000, 0)]
    );
    assert_eq!(
        expansions(&read_with_mask_clearance(Some(0.2))),
        vec![
            ("P1:1".to_owned(), 250_000, 0),
            ("P2:1".to_owned(), 250_000, 0),
            ("TP1:1".to_owned(), 700_000, 0)
        ]
    );
    let mut input = JobInput::new(Vec::new(), "q18-mask-bridge");
    input.set_kicad(read_with_mask_clearance(Some(0.05)));
    let mut patch = SettingsPatch {
        profile: Some("legacy".to_string()),
        ..SettingsPatch::default()
    };
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = Some(3);
    input.extra_sources.push((Source::Testing, patch));
    let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).unwrap();
    assert_eq!(
        prepared.settings.solder_mask_expansions.as_deref(),
        Some("TP1:1=550,0")
    );
    let board = route(prepared);
    assert_eq!(incomplete(&board), 0);
    let (gap, _) = gap_to_tp(&board, 0);
    assert!(gap >= 5499.0, "gap {gap}");
}
