//! K15.3: board-wide `.kicad_dru` rules (`dru::floors`).
//!
//! The rules are `tests/data/pcu-rev2.kicad_dru`, the owner's power-control
//! unit's fab rule file (12 rules, none with a net class). The DSN tests use
//! `crates/freerouting/tests/data/kicad/q225-item-types.dsn` (2 layers,
//! `(resolution um 10)`, one `Via[0-1]` padstack); the `.kicad_pcb` tests use
//! `fixtures/kicad/synthetic/classes.kicad_pcb` (2 layers) and `vias.kicad_pcb`
//! (6 layers, blind, buried and micro vias).

mod common;

use std::path::{Path, PathBuf};

use freerouting::dsn::ast::Pcb;
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::settings::{ResolveContext, SettingsPatch};
use freerouting_kicad::dru::{DruReport, Status, apply_to_pcb};
use freerouting_kicad::{ReadOptions, read_with_rules};

const PCU_DRU: &str = include_str!("data/pcu-rev2.kicad_dru");

fn dsn_tree() -> Pcb {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../freerouting/tests/data/kicad/q225-item-types.dsn");
    Pcb::parse(&freerouting_dsn::decode(&std::fs::read(path).unwrap())).unwrap()
}

fn synthetic(name: &str) -> String {
    std::fs::read_to_string(common::fixtures().join("synthetic").join(name)).unwrap()
}

fn read_board(board: &str, dru: &str) -> freerouting_kicad::KicadRead {
    let options = ReadOptions {
        apply_design_rules: true,
        ..ReadOptions::default()
    };
    read_with_rules("x.dsn", &synthetic(board), None, Some(dru), options).unwrap()
}

/// [`read_board`] with the board's net classes (`classes.kicad_pro`).
fn read_classes(dru: &str) -> freerouting_kicad::KicadRead {
    let options = ReadOptions {
        apply_design_rules: true,
        ..ReadOptions::default()
    };
    let pro = synthetic("classes.kicad_pro");
    read_with_rules(
        "x.dsn",
        &synthetic("classes.kicad_pcb"),
        Some(&pro),
        Some(dru),
        options,
    )
    .unwrap()
}

/// `(rule name, state, reason)` of every constraint.
fn states(report: &DruReport) -> Vec<(String, &'static str, String)> {
    report
        .rules
        .iter()
        .flat_map(|r| {
            r.constraints.iter().map(|c| {
                let (state, why) = match &c.status {
                    Status::Applied { effects } => ("applied", format!("{effects}")),
                    Status::Ignored(w) => ("ignored", w.clone()),
                    Status::Unsupported(w) => ("UNSUPPORTED", w.clone()),
                };
                (r.name.clone(), state, why)
            })
        })
        .collect()
}

fn rule(kind: &str, min: &str, layer: Option<&str>) -> String {
    let layer = layer.map_or(String::new(), |l| format!("(layer {l})"));
    format!("(rule \"{kind} {min} {layer}\" {layer} (constraint {kind} (min {min})))\n")
}

fn file(rules: &[String]) -> String {
    format!("(version 1)\n{}", rules.concat())
}

/// Every rule of the pcu file is applied, met or reported with its reason
/// (none says "rule without a condition"), on a DSN and on a `.kicad_pcb`
/// with the same two layers alike.
#[test]
fn every_rule_of_the_pcu_file_is_applied_or_reported_with_its_reason() {
    let mut pcb = dsn_tree();
    let dsn = apply_to_pcb(&mut pcb, PCU_DRU).unwrap();
    let pcb_read = read_board("classes.kicad_pcb", PCU_DRU);
    let from_dsn = states(&dsn);
    let from_pcb = states(&pcb_read.rules);
    assert_eq!(from_dsn.len(), 12, "{from_dsn:#?}");
    for (name, state, why) in &from_dsn {
        assert!(!why.contains("without a condition"), "{name}: {why}");
        assert!(!why.contains("has no DSN form"), "{name}: {why}");
        let _ = state;
    }
    let brief = |s: &[(String, &'static str, String)]| -> Vec<(String, &'static str)> {
        s.iter().map(|(n, st, _)| (n.clone(), *st)).collect()
    };
    let want_brief = |w: &[(&str, &'static str)]| -> Vec<(String, &'static str)> {
        w.iter().map(|(n, st)| ((*n).to_owned(), *st)).collect()
    };
    assert_eq!(brief(&from_dsn), brief(&from_pcb), "both inputs agree");
    let want = [
        ("Minimum trace width (5 mil)", "applied"),
        ("Minimum copper-to-copper spacing (5 mil)", "applied"),
        ("Minimum via annular ring (5 mil)", "applied"),
        (
            "Minimum component annular ring (7 mil, drill > 0.4 mm)",
            "UNSUPPORTED",
        ),
        ("Minimum finished hole size (10 mil)", "applied"),
        ("Drill-to-copper clearance (10 mil)", "applied"),
        ("Edge clearance, outer layers (10 mil)", "applied"),
        // `inner` names no copper layer of a two-layer board
        ("Edge clearance, inner layers (15 mil)", "ignored"),
        ("No blind vias", "ignored"),
        ("No buried vias", "ignored"),
        ("No microvias", "ignored"),
        ("No plated slots", "UNSUPPORTED"),
    ];
    assert_eq!(brief(&from_dsn), want_brief(&want), "{from_dsn:#?}");
    // the three disallow rules are met by the through-only board
    for (name, _, why) in &from_dsn[8..11] {
        assert!(why.starts_with("met: "), "{name}: {why}");
    }
    // the two left out name the term and why
    for (name, _, why) in from_dsn.iter().filter(|(_, s, _)| *s == "UNSUPPORTED") {
        assert!(
            why.contains("Hole_Size_X") && why.contains("router places no pads"),
            "{name}: {why}"
        );
    }
}

/// The floors carry the rules' values; the edge clearance of a four-layer or
/// larger board is the largest of `outer` and `inner`, with a warning.
#[test]
fn the_floors_are_the_rules_values() {
    let mut pcb = dsn_tree();
    let two = apply_to_pcb(&mut pcb, PCU_DRU).unwrap();
    let nm = |f: &Option<freerouting_kicad::dru::HoleClearance>| f.as_ref().map(|f| f.nm);
    assert_eq!(nm(&two.floors.track_width), Some(127_000));
    assert_eq!(nm(&two.floors.clearance), Some(127_000));
    assert_eq!(nm(&two.floors.via_drill), Some(254_000));
    assert_eq!(nm(&two.floors.via_annular), Some(127_000));
    assert_eq!(nm(&two.floors.edge_clearance), Some(254_000));
    assert_eq!(nm(&two.hole_clearance), Some(254_000));
    assert_eq!(
        two.floors.edge_clearance.as_ref().unwrap().rules,
        ["Edge clearance, outer layers (10 mil)"]
    );

    let six = read_board("vias.kicad_pcb", PCU_DRU).rules;
    assert_eq!(
        nm(&six.floors.edge_clearance),
        Some(381_000),
        "one value, the largest"
    );
    assert_eq!(six.floors.edge_clearance.as_ref().unwrap().rules.len(), 2);
    assert!(
        six.warnings
            .iter()
            .any(|w| w.contains("edge_clearance 0.254 mm") && w.contains("its layers (F.Cu, B.Cu)")),
        "{:?}",
        six.warnings
    );
    assert!(
        six.warnings
            .iter()
            .any(|w| w.contains("edge_clearance 0.381 mm") && w.contains("In1.Cu")),
        "{:?}",
        six.warnings
    );
    assert!(!six.floors.is_empty());
}

/// Without `apply_design_rules` the floors are not taken.
#[test]
fn the_reader_does_not_apply_the_floors_when_it_is_off() {
    let options = ReadOptions {
        apply_design_rules: false,
        ..ReadOptions::default()
    };
    let read = read_with_rules(
        "x.dsn",
        &synthetic("classes.kicad_pcb"),
        None,
        Some(PCU_DRU),
        options,
    )
    .unwrap();
    assert!(read.rules.floors.is_empty());
    assert!(read.rules.hole_clearance.is_none());
    let off = states(&read.rules);
    assert!(
        off.iter()
            .filter(|(n, _, _)| n == "Minimum trace width (5 mil)")
            .all(|(_, s, w)| *s == "ignored" && w.contains("apply_design_rules is off")),
        "{off:?}"
    );
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The settings of a DSN job with `dru` and the command line `cli`.
fn dsn_job(dru: &str, cli: SettingsPatch) -> freerouting::job::prepare::PreparedJob {
    let bytes = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../freerouting/tests/data/kicad/q225-item-types.dsn"),
    )
    .unwrap();
    let mut input = JobInput::new(bytes, "q225-item-types");
    input.rules = RulesChoice::None;
    input.kicad_dru = Some((PathBuf::from("rules.kicad_dru"), dru.to_owned()));
    input.cli = Some(cli);
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap()
}

/// A `.kicad_pcb` job (its `.kicad_pro` is KiCad's defaults: track 0.2,
/// drill 0.3, annular 0.1, edge 0.5, hole clearance 0.25 mm) with `dru`.
fn pcb_job(dru: &str, name: &str) -> freerouting::job::prepare::PreparedJob {
    let dir = scratch(name);
    std::fs::write(dir.join("c.kicad_pcb"), synthetic("classes.kicad_pcb")).unwrap();
    std::fs::write(dir.join("c.kicad_pro"), synthetic("classes.kicad_pro")).unwrap();
    std::fs::write(dir.join("c.kicad_dru"), dru).unwrap();
    let options = ReadOptions {
        apply_design_rules: true,
        ..ReadOptions::default()
    };
    let read = freerouting::io::kicad::read_kicad_input(&dir.join("c.kicad_pcb"), options).unwrap();
    let mut input = JobInput::new(Vec::new(), "c");
    input.rules = RulesChoice::None;
    input.set_kicad(read);
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap()
}

/// What each rule sets, in micrometres: (track, clearance, via drill, via
/// annular, edge, hole clearance).
fn settings(job: &freerouting::job::prepare::PreparedJob) -> [Option<f64>; 6] {
    let s = &job.settings;
    [
        s.board_minimums.track_width_um,
        s.board_minimums.clearance_um,
        s.board_minimums.via_drill_um,
        s.board_minimums.via_annular_um,
        Some(s.copper_to_edge_clearance_um),
        Some(s.hole_clearance_um),
    ]
}

fn above_board_setup() -> String {
    file(&[
        rule("track_width", "0.25mm", None),
        rule("clearance", "0.3mm", None),
        rule("hole_size", "0.4mm", None),
        "(rule \"via ring\" (constraint annular_width (min 0.15mm)) (condition \"A.Type == 'Via'\"))\n"
            .to_owned(),
        rule("edge_clearance", "0.6mm", Some("outer")),
        rule("edge_clearance", "0.7mm", Some("inner")),
        rule("hole_clearance", "0.3mm", None),
    ])
}

/// Each mapped setting has the rule's value, on both inputs.
#[test]
fn each_mapped_setting_has_the_rules_value_on_both_inputs() {
    let rules = above_board_setup();
    // a DSN input: the command line gives no minimum, the rules alone
    let job = dsn_job(&rules, SettingsPatch::default());
    // `inner` is no layer of the DSN's two, so the edge value is `outer`'s
    assert_eq!(
        settings(&job),
        [
            Some(250.0),
            Some(300.0),
            Some(400.0),
            Some(150.0),
            Some(600.0),
            Some(300.0)
        ]
    );
    let log: Vec<&str> = job.log.iter().map(|l| l.text.as_str()).collect();
    assert!(
        log.iter().any(|l| l.contains(
            "router.board_minimums.track_width_um raised from 0 to 250 by the .kicad_dru rules 'track_width 0.25mm '"
        )),
        "{log:#?}"
    );

    // a .kicad_pcb input over Board Setup's defaults: track 0.2, drill 0.3,
    // annular 0.1, edge 0.5, hole clearance 0.25 mm; two layers again
    let job = pcb_job(&rules, "k15-3-above");
    assert_eq!(
        settings(&job),
        [
            Some(250.0),
            Some(300.0),
            Some(400.0),
            Some(150.0),
            Some(600.0),
            Some(300.0)
        ]
    );
}

/// A rule below Board Setup's value leaves Board Setup's (the command line's
/// on a DSN, the `.kicad_pro`'s on a `.kicad_pcb`), and says so.
#[test]
fn a_rule_below_board_setup_leaves_board_setups_value() {
    let mut cli = SettingsPatch::default();
    cli.board_minimums.track_width_um = Some(200.0);
    cli.board_minimums.clearance_um = Some(150.0);
    cli.board_minimums.via_drill_um = Some(300.0);
    cli.board_minimums.via_annular_um = Some(150.0);
    cli.copper_to_edge_clearance_um = Some(500.0);
    cli.hole_clearance_um = Some(300.0);
    let job = dsn_job(PCU_DRU, cli);
    assert_eq!(
        settings(&job),
        [
            Some(200.0),
            Some(150.0),
            Some(300.0),
            Some(150.0),
            Some(500.0),
            Some(300.0)
        ]
    );
    let log: Vec<&str> = job.log.iter().map(|l| l.text.as_str()).collect();
    assert!(
        log.iter().any(|l| l.contains(
            "router.board_minimums.track_width_um is 200, no less than the .kicad_dru rules 'Minimum trace width (5 mil)' ask (127)"
        )),
        "{log:#?}"
    );

    // the .kicad_pcb: the pcu file's hole clearance (0.254) is above
    // Board Setup's 0.25 mm, its via annular ring (0.127) above 0.1 mm; the
    // other four are below
    let job = pcb_job(PCU_DRU, "k15-3-below");
    assert_eq!(
        settings(&job),
        [
            Some(200.0),
            Some(127.0),
            Some(300.0),
            Some(127.0),
            Some(500.0),
            Some(254.0)
        ]
    );
}

fn via_names(pcb: &Pcb) -> Vec<String> {
    pcb.structure
        .as_ref()
        .and_then(|s| s.via.as_ref())
        .map(|v| v.padstacks.clone())
        .unwrap_or_default()
}

/// `disallow blind_via|buried_via|micro_via` on a board with such vias
/// removes them from the router's choices; on a through-only board it is met.
#[test]
fn disallow_removes_the_via_kinds_a_board_offers() {
    let dru = file(&[
        "(rule \"No blind vias\" (constraint disallow blind_via))\n".to_owned(),
        "(rule \"No buried vias\" (constraint disallow buried_via))\n".to_owned(),
    ]);
    let read = read_board("vias.kicad_pcb", &dru);
    let names = via_names(&read.pcb);
    assert!(!names.is_empty());
    assert!(
        names.iter().all(|n| n.starts_with("Via[0-5]")),
        "only through vias are left: {names:?}"
    );
    let s = states(&read.rules);
    assert!(s.iter().all(|(_, st, _)| *st == "applied"), "{s:?}");

    // the same rules on a read without them keep the blind and buried ones
    let kept = read_board("vias.kicad_pcb", "(version 1)\n");
    assert!(
        via_names(&kept.pcb)
            .iter()
            .any(|n| n.starts_with("Via[0-2]") || n.starts_with("Via[1-3]")),
        "{:?}",
        via_names(&kept.pcb)
    );

    // micro: an outer layer to its neighbour (and a blind via of that span)
    let micro = read_board(
        "vias.kicad_pcb",
        "(version 1)\n(rule \"No microvias\" (constraint disallow micro_via))\n",
    );
    assert!(
        micro
            .rules
            .warnings
            .iter()
            .any(|w| w.contains("disallow micro_via")),
        "{:?}",
        micro.rules.warnings
    );
    assert!(
        via_names(&micro.pcb)
            .iter()
            .all(|n| !n.starts_with("Via[0-1]")),
        "{:?}",
        via_names(&micro.pcb)
    );

    // other disallow kinds are reported by name
    let other = read_board(
        "vias.kicad_pcb",
        "(version 1)\n(rule \"x\" (constraint disallow track))\n",
    );
    let s = states(&other.rules);
    assert_eq!(s[0].1, "UNSUPPORTED");
    assert!(s[0].2.contains("disallow track"), "{s:?}");
}

fn class_width(pcb: &Pcb, name: &str) -> f64 {
    pcb.network
        .as_ref()
        .unwrap()
        .classes
        .iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no class {name}"))
        .rules
        .iter()
        .find_map(|r| match r {
            freerouting::dsn::ast::Rule::Width(w) => Some(*w),
            _ => None,
        })
        .unwrap()
}

const LIMITED: &str = "class-limited";

/// A `track_width` `min` for some net classes only raises the width of
/// those classes (never lowers one, no board-wide floor), on a DSN.
#[test]
fn a_class_limited_track_width_min_raises_only_that_class_on_a_dsn() {
    let rule = |class: &str, min: &str| {
        format!(
            "(version 1)\n(rule \"{LIMITED}\" (constraint track_width (min {min})) \
             (condition \"A.NetClass == '{class}'\"))\n"
        )
    };
    // q225: kicad_default 200 um, Power 300 um
    let mut pcb = dsn_tree();
    let report = apply_to_pcb(&mut pcb, &rule("Power", "0.5mm")).unwrap();
    assert_eq!(states(&report)[0].1, "applied", "{:?}", states(&report));
    assert_eq!(class_width(&pcb, "Power"), 500.0);
    assert_eq!(class_width(&pcb, "kicad_default"), 200.0);
    assert!(report.floors.is_empty());
    assert_eq!(
        report.track_widths,
        [("Power".to_owned(), LIMITED.to_owned())]
    );

    // the Default class, selected: its rule and the structure's width rise
    let mut pcb = dsn_tree();
    let report = apply_to_pcb(&mut pcb, &rule("Default", "0.25mm")).unwrap();
    assert_eq!(class_width(&pcb, "kicad_default"), 250.0);
    assert_eq!(class_width(&pcb, "Power"), 300.0);
    assert!(report.floors.is_empty());
    let structure_width = pcb
        .structure
        .as_ref()
        .unwrap()
        .rules
        .iter()
        .find_map(|r| match r {
            freerouting::dsn::ast::Rule::Width(w) => Some(*w),
            _ => None,
        });
    assert_eq!(structure_width, Some(250.0));

    // a min below the class's width lowers nothing; a class with no match is ignored
    let mut pcb = dsn_tree();
    let report = apply_to_pcb(&mut pcb, &rule("Power", "0.25mm")).unwrap();
    assert_eq!(states(&report)[0].1, "ignored");
    assert_eq!(class_width(&pcb, "Power"), 300.0);
    let report = apply_to_pcb(&mut dsn_tree(), &rule("Nothing", "0.5mm")).unwrap();
    assert_eq!(states(&report)[0].1, "ignored", "{:?}", states(&report));
    assert!(report.floors.is_empty());

    // the job: `board_minimums.track_width_um` is the command line's, untouched
    let mut cli = SettingsPatch::default();
    cli.board_minimums.track_width_um = Some(150.0);
    let job = dsn_job(&rule("Power", "0.5mm"), cli);
    assert_eq!(job.settings.board_minimums.track_width_um, Some(150.0));
}

/// The same on a `.kicad_pcb`: only the classes holding `Fast` (100 um) rise to
/// 150 um; Board Setup's 200 um track minimum stays.
#[test]
fn a_class_limited_track_width_min_raises_only_that_class_on_a_kicad_pcb() {
    let dru = "(version 1)\n(rule \"class-limited\" (constraint track_width (min 0.15mm)) \
               (condition \"A.hasNetclass('Fast')\"))\n";
    let read = read_classes(dru);
    assert_eq!(states(&read.rules)[0].1, "applied");
    assert_eq!(class_width(&read.pcb, "Fast"), 150.0);
    assert_eq!(class_width(&read.pcb, "Fast,Wide"), 150.0);
    assert_eq!(class_width(&read.pcb, "Wide,Default"), 600.0);
    assert_eq!(class_width(&read.pcb, "kicad_default"), 250.0);
    assert!(read.rules.floors.is_empty());
    let job = pcb_job(dru, "k15-3-class-limited");
    assert_eq!(job.settings.board_minimums.track_width_um, Some(200.0));
}

/// A `min` of any other kind for some net classes only has no board-wide
/// form: UNSUPPORTED with the reason, never a floor, on both inputs.
#[test]
fn a_class_limited_hole_size_min_is_unsupported() {
    let reason = "limited to some net classes; the router keeps one board-wide value";
    for kind in ["hole_size", "annular_width", "edge_clearance"] {
        let dru = |class: &str| {
            format!(
                "(version 1)\n(rule \"hv\" (constraint {kind} (min 0.5mm)) \
                 (condition \"A.NetClass == '{class}'\"))\n"
            )
        };
        let report = apply_to_pcb(&mut dsn_tree(), &dru("Power")).unwrap();
        let s = states(&report);
        assert_eq!((s[0].1, s[0].2.as_str()), ("UNSUPPORTED", reason), "{kind}");
        assert!(report.floors.is_empty(), "{kind}");
        let read = read_classes(&dru("Fast"));
        let s = states(&read.rules);
        assert_eq!((s[0].1, s[0].2.as_str()), ("UNSUPPORTED", reason), "{kind}");
        assert!(read.rules.floors.is_empty(), "{kind}");
    }
}
