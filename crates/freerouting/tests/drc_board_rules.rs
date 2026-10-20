//! Q20.1: the board's manufacturing rules on copper
//! (`drc::board_rules`, `check_board_rules`).
//!
//! One synthetic board (written here, [`BASE`]), with the wiring of each
//! case in its `wiring` section: net A runs from the SMD pad S1 (5, 15 mm)
//! to S2 (35, 15 mm); T1 is a through-hole pad of net B at (20, 25 mm)
//! (1.6 mm copper, 0.8 mm drill), S3 an SMD pad of net C at (20, 5 mm); a
//! keepout covers (30..34, 20..24 mm) on F.Cu. Width and clearance 0.2 mm.
//! Each rule has a breaching case and a clean one; the clean case of every
//! rule is checked to break no rule at all.

use freerouting::board::RoutingBoard;
use freerouting::drc::{
    BoardRule, BoardRuleCounts, BoardRuleLimits, InputCopper, Origin, check_board_rules,
};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};

const BASE: &str = r#"(pcb q20-board-rules.dsn
  (parser (string_quote ") (space_in_quoted_tokens on) (host_cad "freerouting-rs Q20 synthetic fixture"))
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (layer B.Cu (type signal) (property (index 1)))
    (boundary (path pcb 0  0 0  40000 0  40000 30000  0 30000  0 0))
    (keepout "" (rect F.Cu 30000 20000 34000 24000))
    (via "Via[0-1]_600:300_um")
    (rule (width 200) (clearance 200))
  )
  (placement
    (component SMD
      (place S1 5000 15000 front 0)
      (place S2 35000 15000 front 0)
      (place S3 20000 5000 front 0)
    )
    (component TH (place T1 20000 25000 front 0))
  )
  (library
    (image SMD (pin SmdPad 1 0 0))
    (image TH (pin "Pad_1600:800_um" 1 0 0))
    (padstack SmdPad (shape (rect F.Cu -300 -300 300 300)) (attach off))
    (padstack "Pad_1600:800_um" (shape (circle F.Cu 1600)) (shape (circle B.Cu 1600)) (attach off))
    (padstack "Via[0-1]_600:300_um" (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off))
    (padstack "Via[0-1]_400:300_um" (shape (circle F.Cu 400)) (shape (circle B.Cu 400)) (attach off))
  )
  (network
    (net A (pins S1-1 S2-1))
    (net B (pins T1-1))
    (net C (pins S3-1))
    (class kicad_default "" A B C
      (circuit (use_via "Via[0-1]_600:300_um"))
      (rule (width 200) (clearance 200))
    )
  )
  (wiring
WIRING
  )
)
"#;

/// Net A straight from S1 to S2: breaks nothing.
const STRAIGHT: &str = "(wire (path F.Cu 200  5000 15000  35000 15000) (net A))";

/// Net A from S1 to S2 through the corner `(x, y)` on F.Cu, width 200.
fn detour(x: i32, y: i32) -> String {
    format!("(wire (path F.Cu 200  5000 15000  {x} {y}  35000 15000) (net A))")
}

/// Net A from S1 to S2 with a hop to B.Cu between two vias of `via`.
fn hop(via: &str) -> String {
    format!(
        "(wire (path F.Cu 200  5000 15000  20000 15000) (net A))\n\
         (via \"{via}\" 20000 15000 (net A))\n\
         (wire (path B.Cu 200  20000 15000  25000 15000) (net A))\n\
         (via \"{via}\" 25000 15000 (net A))\n\
         (wire (path F.Cu 200  25000 15000  35000 15000) (net A))"
    )
}

/// The base board with `wiring`, loaded with `patch` under profile
/// `legacy` (so nothing but `patch` sets a minimum).
fn load(wiring: &str, patch: SettingsPatch) -> (RoutingBoard, BoardRuleLimits) {
    let dsn = BASE.replace("WIRING", wiring);
    let mut input = JobInput::new(dsn.into_bytes(), "q20-board-rules");
    let mut patch = patch;
    patch.profile = Some("legacy".to_string());
    input.extra_sources = vec![(Source::Testing, patch)];
    let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).expect("the board loads");
    let limits = BoardRuleLimits::of(&prepared.board, &prepared.board_settings)
        .with_mask_expansions(&prepared.board, &prepared.settings);
    (prepared.board, limits)
}

/// The rules `wiring` breaks, all of its copper counted as routed.
fn rules(
    wiring: &str,
    patch: SettingsPatch,
    limits_patch: impl Fn(&mut BoardRuleLimits),
) -> Vec<BoardRule> {
    let (board, mut limits) = load(wiring, patch);
    limits_patch(&mut limits);
    let found = check_board_rules(&board, &limits, &InputCopper::none());
    assert!(
        found.iter().all(|v| v.origin == Origin::Routed),
        "{found:?}"
    );
    let mut rules: Vec<BoardRule> = found.iter().map(|v| v.rule).collect();
    rules.dedup();
    rules
}

fn plain(wiring: &str) -> Vec<BoardRule> {
    rules(wiring, SettingsPatch::default(), |_| {})
}

fn none(_: &mut BoardRuleLimits) {}

#[test]
fn a_straight_connection_breaks_no_rule() {
    assert_eq!(plain(STRAIGHT), vec![]);
    assert_eq!(plain(&hop("Via[0-1]_600:300_um")), vec![]);
}

#[test]
fn clearance_between_nets() {
    // The corner 1.0 mm from T1's centre: 0.1 mm from its pad.
    assert_eq!(plain(&detour(20000, 24000)), vec![BoardRule::Clearance]);
    // 0.3 mm from the pad.
    assert_eq!(plain(&detour(20000, 23800)), vec![]);
}

#[test]
fn clearance_has_no_tolerance() {
    // 0.5 um short of the 0.2 mm clearance to T1's pad: the DRC's default
    // 1 um tolerance passes it, the check does not (Q22.1).
    let short = "(wire (path F.Cu 200  5000 15000  20000 23900.5  35000 15000) (net A))";
    assert_eq!(plain(short), vec![BoardRule::Clearance]);
    let (board, _) = load(short, SettingsPatch::default());
    let drc = board.all_clearance_violations();
    assert!(drc.is_empty(), "the DRC's tolerance passes it: {drc:?}");
    // 5 um more than the clearance.
    assert_eq!(plain(&detour(20000, 23895)), vec![]);
}

#[test]
fn track_width_under_the_board_minimum() {
    let mut patch = SettingsPatch::default();
    patch.board_minimums.track_width_um = Some(200.0);
    let narrow = "(wire (path F.Cu 150  5000 15000  35000 15000) (net A))";
    assert_eq!(
        rules(narrow, patch.clone(), none),
        vec![BoardRule::TrackWidth]
    );
    assert_eq!(rules(STRAIGHT, patch.clone(), none), vec![]);
    // No minimum known: no rule.
    assert_eq!(plain(narrow), vec![]);
}

#[test]
fn via_diameter_drill_and_annular_ring_under_the_minimums() {
    let mut patch = SettingsPatch::default();
    patch.board_minimums.via_diameter_um = Some(500.0);
    patch.board_minimums.via_drill_um = Some(350.0);
    patch.board_minimums.via_annular_um = Some(100.0);
    assert_eq!(
        rules(&hop("Via[0-1]_400:300_um"), patch, none),
        vec![
            BoardRule::ViaDiameter,
            BoardRule::ViaDrill,
            BoardRule::AnnularRing
        ]
    );
    let mut patch = SettingsPatch::default();
    patch.board_minimums.via_diameter_um = Some(500.0);
    patch.board_minimums.via_drill_um = Some(300.0);
    patch.board_minimums.via_annular_um = Some(100.0);
    assert_eq!(rules(&hop("Via[0-1]_600:300_um"), patch, none), vec![]);
    // Each minimum alone.
    let only = |f: fn(&mut SettingsPatch)| {
        let mut p = SettingsPatch::default();
        f(&mut p);
        rules(&hop("Via[0-1]_400:300_um"), p, none)
    };
    assert_eq!(
        only(|p| p.board_minimums.via_diameter_um = Some(450.0)),
        vec![BoardRule::ViaDiameter]
    );
    assert_eq!(
        only(|p| p.board_minimums.via_drill_um = Some(310.0)),
        vec![BoardRule::ViaDrill]
    );
    assert_eq!(
        only(|p| p.board_minimums.via_annular_um = Some(60.0)),
        vec![BoardRule::AnnularRing]
    );
}

#[test]
fn copper_to_edge_clearance() {
    let patch = SettingsPatch {
        copper_to_edge_clearance_um: Some(500.0),
        ..SettingsPatch::default()
    };
    // The corner's copper 0.3 mm from the bottom edge.
    assert_eq!(
        rules(&detour(10000, 400), patch.clone(), none),
        vec![BoardRule::EdgeClearance]
    );
    // 0.9 mm.
    assert_eq!(rules(&detour(10000, 1000), patch, none), vec![]);
}

#[test]
fn copper_inside_a_keepout() {
    assert_eq!(plain(&detour(32000, 22000)), vec![BoardRule::Keepout]);
    assert_eq!(plain(&detour(28000, 18000)), vec![]);
}

#[test]
fn hole_to_copper_clearance() {
    let patch = SettingsPatch {
        hole_clearance_um: Some(800.0),
        ..SettingsPatch::default()
    };
    // 0.3 mm from T1's pad (clearance kept), 0.7 mm from its 0.8 mm hole.
    assert_eq!(
        rules(&detour(20000, 23800), patch.clone(), none),
        vec![BoardRule::HoleClearance]
    );
    // 1.5 mm from the hole.
    assert_eq!(rules(&detour(20000, 23000), patch.clone(), none), vec![]);
    // A via's own hole: net B's trace 0.5 mm from the edge of a 0.3 mm
    // hole of net A (0.35 mm from the via's copper).
    let via = format!(
        "{}\n(via \"Via[0-1]_600:300_um\" 20000 15000 (net A))\n\
         (wire (path F.Cu 200  20000 25000  20000 15750) (net B))",
        hop("Via[0-1]_600:300_um")
    );
    let found = rules(&via, patch.clone(), none);
    assert!(found.contains(&BoardRule::HoleClearance), "{found:?}");
    let far = format!(
        "{}\n(wire (path F.Cu 200  20000 25000  20000 17000) (net B))",
        hop("Via[0-1]_600:300_um")
    );
    assert!(
        !rules(&far, patch, none).contains(&BoardRule::HoleClearance),
        "a trace 1.85 mm from the hole"
    );
}

/// `router.solder_mask_expansions` as a patch.
fn masks(text: &str) -> SettingsPatch {
    SettingsPatch {
        solder_mask_expansions: Some(text.to_string()),
        ..SettingsPatch::default()
    }
}

/// Q20.1, Q18.7: the mask rule reads each listed pad's own expansions from
/// `router.solder_mask_expansions`, per outer side.
#[test]
fn solder_mask_bridge_for_the_listed_pads() {
    let found = |wiring: &str, text: &str| rules(wiring, masks(text), none);
    // 0.25 mm from S3's pad: inside its 0.3 mm opening.
    let bridge = detour(20000, 5650);
    let hit = found(&bridge, "S3:1=300,0");
    assert!(hit.contains(&BoardRule::MaskBridge), "{hit:?}");
    // Exactly on the opening's edge (0.3 mm): the router's own distance for
    // the pad, no breach of either rule.
    assert_eq!(found(&detour(20000, 5700), "S3:1=300,0"), vec![]);
    assert_eq!(found(&detour(20000, 6000), "S3:1=300,0"), vec![]);
    // Without the setting the rule is not checked.
    assert_eq!(plain(&bridge), vec![]);
    // Only the listed pads: S3's trace is no breach when T1 is the pad listed.
    assert_eq!(found(&bridge, "T1:1=300,300"), vec![]);
    // The sides count apart: T1 with a back-side expansion leaves a trace on
    // F.Cu 0.3 mm from its copper alone, the front side's flags it.
    let near_t1 = detour(20000, 23800);
    assert_eq!(found(&near_t1, "T1:1=0,500"), vec![]);
    let front = found(&near_t1, "T1:1=500,0");
    assert!(front.contains(&BoardRule::MaskBridge), "{front:?}");
}

#[test]
fn vias_in_pads() {
    let in_pad = format!("{STRAIGHT}\n(via \"Via[0-1]_600:300_um\" 5000 15000 (net A))");
    let found = plain(&in_pad);
    assert!(found.contains(&BoardRule::ViaInPad), "{found:?}");
    assert!(!plain(&hop("Via[0-1]_600:300_um")).contains(&BoardRule::ViaInPad));
}

#[test]
fn dangling_traces_and_vias() {
    let stub = format!("{STRAIGHT}\n(wire (path F.Cu 200  5000 15000  5000 20000) (net A))");
    assert_eq!(plain(&stub), vec![BoardRule::TrackDangling]);
    let via = format!("{STRAIGHT}\n(via \"Via[0-1]_600:300_um\" 10000 25000 (net A))");
    assert_eq!(plain(&via), vec![BoardRule::ViaDangling]);
    assert_eq!(plain(STRAIGHT), vec![]);
}

#[test]
fn input_copper_is_marked_input_and_routed_copper_routed() {
    let wiring = detour(20000, 24000);
    let (board, limits) = load(&wiring, SettingsPatch::default());
    let input = InputCopper::of(&board);
    let found = check_board_rules(&board, &limits, &input);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].origin, Origin::Input);
    assert!(found[0].routed_items.is_empty());
    let routed = check_board_rules(&board, &limits, &InputCopper::none());
    assert_eq!(routed[0].origin, Origin::Routed);
    assert_eq!(routed[0].routed_items.len(), 1);
    let counts = BoardRuleCounts::of(&routed);
    assert_eq!(counts.routed_total(), 1);
    assert_eq!(BoardRuleCounts::text(&counts.routed), "clearance 1");
}

#[test]
fn the_check_is_deterministic() {
    let wiring = format!(
        "{}\n(via \"Via[0-1]_600:300_um\" 10000 25000 (net A))\n\
         (wire (path F.Cu 200  31000 21000  33000 23000) (net C))",
        detour(20000, 24000)
    );
    let (board, limits) = load(&wiring, SettingsPatch::default());
    let a = check_board_rules(&board, &limits, &InputCopper::none());
    let b = check_board_rules(&board, &limits, &InputCopper::none());
    assert_eq!(a, b);
    assert!(a.len() >= 3, "{a:?}");
}

/// The check on kept sessions, for the comparison with KiCad's DRC (Q20.1's
/// second clause). `Q20_DATA_ROOT` is the checkout the set's rows are
/// relative to, `Q20_SES_DIR` the directory of the arm's `<board>.ses`,
/// `Q20_SET` the board set (`tools/bench/sets/q18-board-minimums.txt`) and
/// `Q20_FLAGS` the arm's flags before each row's own. Prints, per row, the
/// routed breaches per rule and the input ones.
#[test]
#[ignore = "needs kept sessions; see the doc comment"]
fn board_rules_on_kept_sessions() {
    use freerouting::board::items::{Item, ItemId};
    use freerouting::board::structure::FixedState;
    use freerouting::settings::cli_flags::parse_router_flags;
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"));
    let root = std::path::PathBuf::from(var("Q20_DATA_ROOT"));
    let ses_dir = std::path::PathBuf::from(var("Q20_SES_DIR"));
    let set = std::fs::read_to_string(var("Q20_SET")).expect("the set");
    let arm_flags: Vec<String> = std::env::var("Q20_FLAGS")
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    for line in set.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let dsn = root.join(words.next().expect("a path"));
        let stem = dsn.file_stem().unwrap().to_string_lossy().into_owned();
        let ses = ses_dir.join(format!("{stem}.ses"));
        if !dsn.exists() || !ses.exists() {
            println!("{stem}: skipped (no DSN or session)");
            continue;
        }
        let mut argv = arm_flags.clone();
        argv.extend(words.map(str::to_string));
        let flags = parse_router_flags(&argv);
        let mut input = JobInput::new(std::fs::read(&dsn).unwrap(), stem.clone());
        input.dsn_dir = dsn.parent().map(std::path::Path::to_path_buf);
        input.cli = Some(flags.parse.patch);
        let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).expect("loads");
        let mut board = prepared.board;
        let limits = BoardRuleLimits::of(&board, &prepared.board_settings);
        let input_copper = InputCopper::of(&board);
        // The session repeats every trace and via but the system-fixed ones.
        let repeated: Vec<ItemId> = board
            .items()
            .filter(|i| (i.is_trace() || i.is_via()) && i.fixed_state() != FixedState::SystemFixed)
            .map(Item::id)
            .collect();
        for id in repeated {
            board.set_fixed_state(id, FixedState::Unfixed);
            board.remove_item(id);
        }
        let text = std::fs::read_to_string(&ses).unwrap();
        let session = freerouting::dsn::Session::parse(&text).expect("a session");
        let saved = board.java_quirks();
        board.set_java_quirks(saved.with("ses_reader_scale", false).unwrap());
        freerouting::io::specctra::import_session(&mut board, &session);
        board.set_java_quirks(saved);
        let start = std::time::Instant::now();
        let found = check_board_rules(&board, &limits, &input_copper);
        let seconds = start.elapsed().as_secs_f64();
        let counts = BoardRuleCounts::of(&found);
        let traces = board.items().filter(|i| i.is_trace()).count();
        let vias = board.items().filter(|i| i.is_via()).count();
        println!(
            "{stem}: {traces} traces, {vias} vias; routed {} [{}]; input {} [{}]; {seconds:.3} s",
            counts.routed_total(),
            BoardRuleCounts::text(&counts.routed),
            counts.input_total(),
            BoardRuleCounts::text(&counts.input)
        );
        for v in found.iter().filter(|v| v.origin == Origin::Routed) {
            println!(
                "  {} layer {} at ({:.0}, {:.0}) items {:?} required {:.0} actual {:.0}",
                v.rule.name(),
                v.layer,
                v.location.x,
                v.location.y,
                v.items,
                v.required,
                v.actual
            );
        }
    }
}
