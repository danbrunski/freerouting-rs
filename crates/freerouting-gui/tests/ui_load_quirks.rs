//! A board the GUI opens is read with the `router.java_quirks.*` switches of the
//! settings it will route with (the profile, the saved control values and the
//! front-end flags), not with Java's: every load path, the session import's
//! `ses_reader_scale` and the DSN reader's `dsn_*` switches agree with the CLI's
//! `prepare_job`, which reads with the job's switches.

mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{SIZE, fixtures};
use eframe::egui;
use egui_kittest::Harness;
use freerouting::board::RoutingBoard;
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::profile::legacy_pin;
use freerouting::settings::{JavaQuirks, Profile, ResolveContext};
use freerouting_gui::args::Args;
use freerouting_gui::commands::Command;
use freerouting_gui::{App, Prompt};

/// The switches `enhanced` turns off (N06.13, and K15.19's
/// `edge_pin_net_crosses_outline`), as `settings_java_quirks.rs` lists them.
const PROMOTED: usize = 38;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gl-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

/// An app started with `args` (files and front-end flags), loaded.
fn app(args: Vec<OsString>) -> Harness<'static, App> {
    let args = Args::parse(args).expect("the arguments parse");
    let mut h = Harness::builder()
        .with_size(SIZE)
        .build_eframe(move |cc| App::build(&cc.egui_ctx, cc.storage, args.clone()));
    wait_loaded(&mut h);
    h
}

fn wait_loaded(h: &mut Harness<'_, App>) {
    let start = Instant::now();
    h.step();
    while h.state().is_loading() {
        assert!(start.elapsed() < Duration::from_secs(300), "no load");
        std::thread::sleep(Duration::from_millis(5));
        h.step();
    }
    assert!(h.state().error().is_none(), "{:?}", h.state().error());
    for _ in 0..4 {
        h.step();
        if matches!(h.state().prompt(), Some(Prompt::ImportRules { .. })) {
            exec(
                h,
                Command::AnswerPrompt(freerouting_gui::commands::Answer::No),
            );
        }
    }
}

fn exec(h: &mut Harness<'_, App>, command: Command) {
    let ctx: egui::Context = h.ctx.clone();
    h.state_mut().execute(&ctx, command);
    h.step();
    h.step();
    wait_loaded(h);
}

fn quirks(h: &Harness<'_, App>) -> JavaQuirks {
    h.state()
        .loaded()
        .expect("a board is open")
        .session
        .board
        .basic()
        .java_quirks()
}

fn enhanced() -> JavaQuirks {
    JavaQuirks::from_patch(&Profile::Enhanced.defaults(8).java_quirks)
}

fn fixture(name: &str) -> PathBuf {
    fixtures().join(name)
}

#[test]
fn enhanced_loads_with_the_promoted_fixes() {
    let h = app(vec![fixture("Issue159-setonix_2hp-pcb.dsn").into()]);
    let q = quirks(&h);
    assert_eq!(q, enhanced());
    let fixed = q.fixed_ids();
    assert_eq!(fixed.len(), PROMOTED, "{fixed:?}");
    for id in [
        "dsn_via_alias",
        "dsn_class_class_pairs",
        "dsn_shape_bbox",
        "dsn_unit_ignored",
        "ses_reader_scale",
    ] {
        assert_eq!(q.get(id), Some(false), "{id}");
    }
}

#[test]
fn profile_legacy_loads_with_java_switches() {
    let h = app(os(&[
        "--profile",
        "legacy",
        &fixture("Issue159-setonix_2hp-pcb.dsn").to_string_lossy(),
    ]));
    assert_eq!(quirks(&h), JavaQuirks::JAVA);
    assert!(quirks(&h).fixed_ids().is_empty());
}

#[test]
fn a_saved_profile_applies_to_every_later_load() {
    let mut h = app(vec![fixture("Issue159-setonix_2hp-pcb.dsn").into()]);
    assert_eq!(quirks(&h), enhanced());
    // The settings dialog's Apply keeps the legacy profile as a saved control
    // value; reload and open (the path of a drop) read with it.
    exec(&mut h, Command::ShowAutorouteSettings);
    let mut draft = h
        .state()
        .autoroute()
        .dialog
        .clone()
        .expect("the dialog is open");
    draft.set_profile(Profile::Legacy);
    let applied = draft.patch;
    exec(&mut h, Command::ApplyAutorouteSettings(Box::new(applied)));
    exec(&mut h, Command::Reload);
    assert_eq!(quirks(&h), JavaQuirks::JAVA, "after Reload");
    let other = fixture("Issue754-avionics_hub.dsn");
    exec(&mut h, Command::Open(other));
    assert_eq!(quirks(&h), JavaQuirks::JAVA, "after Open (a drop)");
}

/// The rows of a board that the DSN reader's switches change.
fn read_signature(board: &RoutingBoard) -> String {
    let b = board.basic();
    let class = b.rules.net_classes.get_by_name("A").expect("class A");
    let rule = b.rules.net_classes.get(class).via_rule().expect("via rule");
    let vias = b.rules.via_rules.get(rule).vias().len();
    let m = &b.rules.clearance_matrix;
    let (a, bb, c) = (m.get_no("A"), m.get_no("B"), m.get_no("C"));
    format!(
        "{:?} {} items={} vias={vias} a-b={} a-c={} b-c={}",
        b.communication.unit,
        b.communication.resolution,
        b.item_count(),
        m.get_value(a, bb, 0, false),
        m.get_value(a, c, 0, false),
        m.get_value(bb, c, 0, false),
    )
}

const SYNTHETIC_BASE: &str = include_str!("../../../fixtures/scoring-perfect-two-pin.dsn");

/// A DSN where `dsn_via_alias`, `dsn_class_class_pairs` and `dsn_unit_ignored`
/// each change the board.
fn synthetic_dsn() -> String {
    SYNTHETIC_BASE
        .replace("(unit um)", "(unit mm)")
        .replace(
            "(library",
            "(library (padstack V1 (shape (circle F.Cu 400))) (padstack V2 (shape (circle F.Cu 600)))",
        )
        .replace(
            "(class kicad_default PERFECT\n    )",
            "(class A PERFECT (circuit (use_via V1))) (class B (circuit (use_via V2))) (class C) \
             (class_class (classes A B C) (rule (clearance 777)))",
        )
}

fn cli_board(dsn: &str, legacy: bool) -> RoutingBoard {
    let mut input = JobInput::new(dsn.as_bytes().to_vec(), "synthetic");
    input.rules = RulesChoice::None;
    if legacy {
        input.cli = Some(legacy_pin());
    }
    prepare_job(&input, &ResolveContext::from_machine())
        .expect("the CLI loads it")
        .board
}

fn gui_board(path: &Path, legacy: bool) -> Harness<'static, App> {
    let mut args = Vec::new();
    if legacy {
        args.extend(os(&["--profile", "legacy"]));
    }
    args.push(path.into());
    app(args)
}

#[test]
fn the_gui_board_equals_the_cli_board_where_the_dsn_switches_matter() {
    let dir = scratch("dsn");
    let dsn = synthetic_dsn();
    let path = dir.join("synthetic.dsn");
    std::fs::write(&path, &dsn).unwrap();
    let mut signatures = Vec::new();
    for legacy in [false, true] {
        let h = gui_board(&path, legacy);
        let gui = &h.state().loaded().unwrap().session.board;
        let cli = cli_board(&dsn, legacy);
        // The CLI's prepared board is bigger (expanded box, nets reduced); the
        // read signature and the switches are what the reader decided.
        assert_eq!(read_signature(gui), read_signature(&cli), "legacy={legacy}");
        assert_eq!(gui.basic().java_quirks(), cli.basic().java_quirks());
        signatures.push(read_signature(gui));
    }
    assert_ne!(
        signatures[0], signatures[1],
        "the switches change this board"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_session_import_follows_the_profiles_ses_reader_scale() {
    // Issue676: the unit factor `dsn_to_board(1) / resolution` is not 1, so
    // the fixed import (enhanced) and Java's differ.
    let (dsn, ses) = (
        fixture("Issue676-ch32v-tx118s.dsn"),
        fixture("reference/route/Issue676-ch32v-tx118s.ses"),
    );
    let mut stats = Vec::new();
    for legacy in [false, true] {
        let mut args = Vec::new();
        if legacy {
            args.extend(os(&["--profile", "legacy"]));
        }
        args.push(dsn.clone().into());
        args.push(ses.clone().into());
        let h = app(args);
        let gui = BoardStatistics::new(&h.state().loaded().unwrap().session.board);

        let mut input = JobInput::new(std::fs::read(&dsn).unwrap(), "Issue676-ch32v-tx118s");
        input.rules = RulesChoice::None;
        input.ses = Some(std::fs::read(&ses).unwrap());
        if legacy {
            input.cli = Some(legacy_pin());
        }
        let cli = prepare_job(&input, &ResolveContext::from_machine()).expect("loads");
        let cli = BoardStatistics::new(&cli.board);
        assert_eq!(
            gui.items.trace_count, cli.items.trace_count,
            "legacy={legacy}"
        );
        assert_eq!(gui.items.via_count, cli.items.via_count, "legacy={legacy}");
        // The geometry the scale moves: items, trace lengths and the box. (The
        // pre-existing violation count is recorded when routing starts in the
        // GUI, so it is not compared.)
        let geometry = |s: &BoardStatistics| format!("{:?} {:?} {:?}", s.items, s.traces, s.board);
        assert_eq!(geometry(&gui), geometry(&cli), "legacy={legacy}");
        stats.push(geometry(&gui));
    }
    assert_ne!(stats[0], stats[1], "the scale changes the imported wiring");
}
