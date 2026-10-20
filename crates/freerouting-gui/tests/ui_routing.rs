//! G07: the autoroute settings dialog, the routing job, the progress panel,
//! live redraw from the router's events, the overlay and the summary.

mod common;

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use common::{fixtures, plain_harness, rendering_enabled};
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use freerouting::autoroute::events::{CompletionStatus, Phase, Progress};
use freerouting::board::RoutingBoard;
use freerouting::drc::json::JsonValue;
use freerouting::geometry::FloatPoint;
use freerouting::io::specctra::{SesStyle, write_ses};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::{FIELDS, FieldKind, FieldValue, RouterSettings, default_patch};
use freerouting_gui::App;
use freerouting_gui::canvas::chunks::{ChunkSet, kind_index, slot_index};
use freerouting_gui::commands::Command;
use freerouting_gui::dialogs::Input;
use freerouting_gui::dialogs::autoroute_settings::{
    CLI_ONLY, CONTROLS, Draft, control_paths, has_control, uncovered_fields,
};
use freerouting_gui::overlays::Frame;
use freerouting_gui::overlays::routing::{RoutingShape, shapes};
use freerouting_gui::panels::progress;
use freerouting_gui::scene::Scene;

const ISSUE159: &str = "Issue159-setonix_2hp-pcb.dsn";
const ISSUE684: &str = "Issue684-Autorouter_PCB1_2026-5-8.dsn";
const ISSUE143: &str = "Issue143-rpi_splitter.dsn";
const POWER_BOARD: &str = "Issue269-z10_module.dsn";
const CORES: usize = 8;

fn exec(h: &mut Harness<'_, App>, command: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, command);
    let _ = h.run_ok();
}

/// The dialog's draft of the open board after `edit`, applied.
fn apply_settings(h: &mut Harness<'_, App>, edit: impl FnOnce(&mut Draft)) {
    exec(h, Command::ShowAutorouteSettings);
    let mut draft = h
        .state()
        .autoroute()
        .dialog
        .clone()
        .expect("the dialog opened");
    edit(&mut draft);
    exec(h, Command::ApplyAutorouteSettings(Box::new(draft.patch)));
}

fn max_passes(h: &mut Harness<'_, App>, passes: i32) {
    apply_settings(h, |d| {
        d.set_int("autorouter.max_passes", Input::Number(f64::from(passes)))
    });
}

/// Steps frames until the job has ended.
fn run_to_end(h: &mut Harness<'_, App>) -> Duration {
    let start = Instant::now();
    while h.state().is_routing() {
        h.step();
        assert!(
            start.elapsed() < Duration::from_secs(300),
            "the router did not finish"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let _ = h.run_ok();
    start.elapsed()
}

fn board(h: &Harness<'_, App>) -> std::sync::Arc<RoutingBoard> {
    h.state().loaded().unwrap().session.board.clone()
}

fn trace_count(h: &Harness<'_, App>) -> usize {
    BoardStatistics::new(&board(h)).items.trace_count
}

// ---------------------------------------------------------------------------
// G07.1: the dialog
// ---------------------------------------------------------------------------

/// A value different from `value` of the same type.
fn changed(value: &FieldValue) -> FieldValue {
    match value {
        FieldValue::Bool(b) => FieldValue::Bool(!b),
        FieldValue::Int(v) => FieldValue::Int(v.wrapping_add(3).max(2)),
        FieldValue::Long(v) => FieldValue::Long(v.wrapping_add(3)),
        FieldValue::Float(v) => FieldValue::Float(v + 1.5),
        FieldValue::Double(v) => FieldValue::Double(v + 1.5),
        FieldValue::Str(s) => FieldValue::Str(format!("{s}x")),
        FieldValue::StrList(l) => {
            let mut l = l.clone();
            l.push("x".to_string());
            FieldValue::StrList(l)
        }
        FieldValue::DoubleList(l) => FieldValue::DoubleList(l.iter().map(|v| v + 1.0).collect()),
        FieldValue::Enum(name) => FieldValue::Enum(name),
    }
}

#[test]
fn settings_round_trip() {
    // The defaults: settings -> dialog -> settings is the identity.
    let defaults = default_patch(CORES);
    let want = RouterSettings::from_patch(&defaults, CORES);
    let draft = Draft::new(defaults.clone(), CORES);
    assert_eq!(draft.settings(), want);

    // A value with every field changed (enums keep their only other value
    // where there is one).
    let mut patch = defaults;
    for field in FIELDS {
        let Some(old) = (field.get)(&patch) else {
            // Unset in the defaults (the optional fields).
            let value = match field.kind {
                FieldKind::Bool => FieldValue::Bool(true),
                FieldKind::Int => FieldValue::Int(7),
                FieldKind::Long => FieldValue::Long(7),
                FieldKind::Float => FieldValue::Float(7.0),
                FieldKind::Double => FieldValue::Double(7.0),
                FieldKind::Str => FieldValue::Str("1:00:00".to_string()),
                FieldKind::StrList => FieldValue::StrList(vec!["a".to_string()]),
                FieldKind::DoubleList => FieldValue::DoubleList(vec![2.0]),
                _ => continue,
            };
            (field.set)(&mut patch, value);
            continue;
        };
        (field.set)(&mut patch, changed(&old));
    }
    let want = RouterSettings::from_patch(&patch, CORES);
    let mut draft = Draft::new(default_patch(CORES), CORES);
    // The controls take their values through the dialog's own setters...
    for path in control_paths() {
        let value = FIELDS
            .iter()
            .find(|f| f.path == path)
            .and_then(|f| (f.get)(&patch));
        if let Some(value) = value {
            draft.set(path, value);
        }
    }
    // ...and the fields without a control are kept as they were set.
    for field in FIELDS {
        if !has_control(field.path)
            && let Some(value) = (field.get)(&patch)
        {
            draft.set(field.path, value);
        }
    }
    for field in FIELDS {
        assert_eq!(draft.get(field.path), (field.get)(&patch), "{}", field.path);
    }
    assert_eq!(draft.settings(), want);
}

#[test]
fn every_router_setting_has_a_control() {
    let paths = freerouting_gui::dialogs::autoroute_settings::field_paths();
    assert!(paths.len() > 60, "the field table is walked");
    assert_eq!(
        uncovered_fields(paths.iter().copied()),
        Vec::<&str>::new(),
        "a router setting has neither a control nor a CLI_ONLY reason"
    );
    // Both tables name real fields, once, and every reason says something.
    let mut seen = std::collections::BTreeSet::new();
    for (path, reason) in CONTROLS.iter().chain(CLI_ONLY) {
        assert!(paths.contains(path), "{path} is not a router setting");
        assert!(seen.insert(*path), "{path} is listed twice");
        assert!(!reason.is_empty());
    }
    // Negative control: a field the engine grows without a control is caught.
    let mut with_fake = paths.clone();
    with_fake.push("scoring.a_new_engine_setting");
    assert_eq!(
        uncovered_fields(with_fake.iter().copied()),
        vec!["scoring.a_new_engine_setting"]
    );
}

#[test]
fn power_layers_disabled() {
    let mut h = plain_harness(&[POWER_BOARD]);
    exec(&mut h, Command::ShowAutorouteSettings);
    let _ = h.run_ok();
    let layers = &h.state().loaded().unwrap().scene.layers;
    assert!(
        layers.iter().any(|l| !l.signal),
        "the board has a power plane layer"
    );
    let power = h.get_by_label("Active: In1.Cu");
    assert!(
        power.accesskit_node().is_disabled(),
        "In1.Cu is a power plane"
    );
    let signal = h.get_by_label("Active: F.Cu");
    assert!(!signal.accesskit_node().is_disabled());
    // The optimizer stage is a live control since O06 landed.
    assert!(
        !h.get_by_label("Optimization")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn applying_the_dialog_keeps_the_user_values() {
    let mut h = plain_harness(&[ISSUE159]);
    apply_settings(&mut h, |d| {
        d.set_int("scoring.via_costs", Input::Number(77.0));
        d.set_int("autorouter.max_passes", Input::Number(100_000.0));
        d.set_timeout(
            2,
            freerouting_gui::dialogs::autoroute_settings::timeout::TimeoutUnit::Days,
        );
    });
    let snapshot = h.state().settings_snapshot();
    let kept: BTreeMap<_, _> = snapshot.autoroute.iter().cloned().collect();
    assert_eq!(kept["scoring.via_costs"], "77");
    // Java clamps the maximum passes to 9999.
    assert_eq!(kept["autorouter.max_passes"], "9999");
    assert_eq!(kept["job_timeout"], "48:00:00");
    assert!(
        !kept.contains_key("fanout.enabled"),
        "unchanged values are not kept"
    );
}

// ---------------------------------------------------------------------------
// G07.2: the job
// ---------------------------------------------------------------------------

#[test]
fn routes_small_fixture_to_completion() {
    let mut h = plain_harness(&[ISSUE159]);
    max_passes(&mut h, 3);
    let before = trace_count(&h);
    exec(&mut h, Command::StartRouting);
    assert!(h.state().is_routing());
    run_to_end(&mut h);
    assert!(!h.state().is_routing());
    let last = h
        .state()
        .autoroute()
        .last_run
        .clone()
        .expect("a run summary");
    assert_eq!(last.status, CompletionStatus::Completed);
    assert!(trace_count(&h) > before, "the router added traces");
    // The frame loop never waits for the router.
    let max = h.state().stats().max();
    assert!(max <= 50.0, "frame CPU max {max} ms");
}

/// The SES the CLI's job flow writes for `file` with `-mp 3`.
fn cli_ses(file: &str, dir: &std::path::Path) -> Vec<u8> {
    use freerouting::job::outputs::{OutputFormat, OutputSpec};
    use freerouting::job::prepare::{JobInput, RulesChoice};
    use freerouting::job::run::{JobContext, run_job};
    use freerouting::settings::{ResolveContext, parse_cli_arguments};

    let path = fixtures().join(file);
    let design = JobInput::design_name_of(file);
    let mut input = JobInput::new(std::fs::read(&path).unwrap(), design);
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(std::path::Path::to_path_buf);
    let cli = parse_cli_arguments(&["-mp".to_string(), "3".to_string()]);
    input.cli = Some(cli.patch);
    let out = dir.join(format!("{file}.cli.ses"));
    let outputs = [OutputSpec {
        path: out.clone(),
        format: OutputFormat::Ses,
    }];
    let outcome = run_job(
        input,
        &outputs,
        &JobContext::new(ResolveContext::from_machine()),
    );
    assert_eq!(outcome.exit_code(), 0, "{:?}", outcome.state);
    std::fs::read(out).unwrap()
}

#[test]
fn gui_and_cli_write_identical_ses() {
    let dir = std::env::temp_dir().join(format!("g07-ses-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for file in [ISSUE159, ISSUE143, ISSUE684] {
        let mut h = plain_harness(&[file]);
        max_passes(&mut h, 3);
        exec(&mut h, Command::StartRouting);
        run_to_end(&mut h);
        let design = freerouting::job::prepare::JobInput::design_name_of(file);
        let gui = write_ses(&board(&h), &design, SesStyle::Java);
        let cli = cli_ses(file, &dir);
        if let Ok(dump) = std::env::var("G07_DUMP_SES") {
            // For comparing with the real `freerouting -de x.dsn -do y.ses -mp 3`.
            std::fs::write(
                std::path::Path::new(&dump).join(format!("{file}.gui.ses")),
                &gui,
            )
            .unwrap();
        }
        assert!(
            gui.as_bytes() == cli.as_slice(),
            "{file}: the GUI's SES differs from the CLI's; compare with \
             `freerouting-dev ses-diff` (GUI {} bytes, CLI {} bytes)",
            gui.len(),
            cli.len()
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cancel_hands_back_board() {
    let mut h = plain_harness(&[ISSUE684]);
    exec(&mut h, Command::StartRouting);
    // Wait until the router is working (pass 1 takes about a second), then
    // press Cancel.
    let start = Instant::now();
    loop {
        h.step();
        let run = h.state().routing().expect("still routing");
        if run
            .progress
            .as_ref()
            .is_some_and(|p| p.pass >= 1 && p.routed > 0)
        {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(2));
    }
    let asked = Instant::now();
    exec(&mut h, Command::StopRouting);
    run_to_end(&mut h);
    let took = asked.elapsed();
    let last = h
        .state()
        .autoroute()
        .last_run
        .clone()
        .expect("Finished arrived");
    assert_eq!(last.status, CompletionStatus::Interrupted);
    assert!(!h.state().is_routing());
    eprintln!("cancel to Finished: {} ms", took.as_millis());
    // The session is editable again: an edit is not refused.
    exec(&mut h, Command::RunDrc);
    assert_ne!(
        h.state().notice(),
        Some(freerouting_gui::text::ROUTING_READ_ONLY)
    );
    let summary = h.state().autoroute().summary.clone().expect("summary");
    assert!(summary.interrupted);
}

#[test]
fn a_second_run_starts_again() {
    let mut h = plain_harness(&[ISSUE159]);
    exec(&mut h, Command::StartRouting);
    run_to_end(&mut h);
    let first = trace_count(&h);
    exec(&mut h, Command::StartRouting);
    run_to_end(&mut h);
    let last = h
        .state()
        .autoroute()
        .last_run
        .clone()
        .expect("a run summary");
    assert_eq!(last.status, CompletionStatus::Completed);
    assert!(
        trace_count(&h) >= first,
        "the second job keeps what the first routed"
    );
    assert_scene_is_fresh(&h);
    assert_eq!(h.state().stats().full_rebuilds, 0);
}

#[test]
fn edits_are_refused_while_routing() {
    let mut h = plain_harness(&[ISSUE684]);
    exec(&mut h, Command::StartRouting);
    let before = board(&h).revision();
    exec(&mut h, Command::ConfirmDeleteAll);
    exec(&mut h, Command::Undo);
    assert_eq!(
        h.state().notice(),
        Some(freerouting_gui::text::ROUTING_READ_ONLY)
    );
    assert_eq!(board(&h).revision(), before);
    exec(&mut h, Command::StopRouting);
    run_to_end(&mut h);
}

// ---------------------------------------------------------------------------
// Stopping with a click: the preference, and a pan is never a click
// ---------------------------------------------------------------------------

fn canvas_point(h: &Harness<'_, App>, dx: f32, dy: f32) -> egui::Pos2 {
    h.state().view().canvas.min + egui::vec2(dx, dy)
}

fn pointer(h: &mut Harness<'_, App>, pos: egui::Pos2, pressed: Option<bool>) {
    let event = match pressed {
        None => egui::Event::PointerMoved(pos),
        Some(pressed) => egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    };
    h.input_mut().events.push(event);
    h.step();
}

/// A left click with real pointer events.
fn click(h: &mut Harness<'_, App>, pos: egui::Pos2) {
    pointer(h, pos, None);
    pointer(h, pos, Some(true));
    pointer(h, pos, Some(false));
    h.step();
}

fn start_long_job(h: &mut Harness<'_, App>) {
    exec(h, Command::StartRouting);
    assert!(h.state().is_routing());
}

fn stop_and_wait(h: &mut Harness<'_, App>) {
    exec(h, Command::StopRouting);
    run_to_end(h);
}

#[test]
fn a_canvas_click_stops_the_job_by_default() {
    let mut h = plain_harness(&[ISSUE684]);
    assert!(h.state().settings_snapshot().stop_on_canvas_click);
    start_long_job(&mut h);
    let at = canvas_point(&h, 300.0, 300.0);
    click(&mut h, at);
    let run = h.state().routing();
    assert!(
        run.is_none_or(|r| r.stop_requested.is_some()),
        "a click did not stop the job"
    );
    run_to_end(&mut h);
}

#[test]
fn with_the_preference_off_a_click_is_ignored_and_esc_still_stops() {
    let mut h = plain_harness(&[ISSUE684]);
    exec(&mut h, Command::SetStopOnCanvasClick(false));
    start_long_job(&mut h);
    assert_eq!(
        h.state().notice(),
        Some(freerouting_gui::text::BATCH_AUTOROUTER_START_MESSAGE_NO_CLICK)
    );
    let at = canvas_point(&h, 300.0, 300.0);
    click(&mut h, at);
    let run = h.state().routing().expect("the click stopped the job");
    assert!(run.stop_requested.is_none(), "the click asked for a stop");
    assert_eq!(
        h.state().notice(),
        Some(freerouting_gui::text::ROUTING_CLICK_IGNORED)
    );
    exec(
        &mut h,
        Command::CanvasClick {
            at: freerouting_gui::scene::P::new(0.0, 0.0),
            toggle: false,
        },
    );
    assert!(h.state().routing().unwrap().stop_requested.is_none());
    // Esc is the Cancel command.
    h.key_press(egui::Key::Escape);
    h.step();
    h.step();
    let run = h.state().routing();
    assert!(
        run.is_none_or(|r| r.stop_requested.is_some()),
        "Esc did not stop the job"
    );
    run_to_end(&mut h);
}

#[test]
fn with_the_preference_off_the_stop_command_still_works() {
    let mut h = plain_harness(&[ISSUE684]);
    exec(&mut h, Command::SetStopOnCanvasClick(false));
    start_long_job(&mut h);
    stop_and_wait(&mut h);
    assert!(h.state().autoroute().last_run.is_some());
}

#[test]
fn the_status_text_names_what_stops_the_job() {
    let progress = recorded()
        .into_iter()
        .find(|p| p.phase == Phase::Autoroute)
        .expect("an autoroute event");
    let on = progress::texts_for(&progress, true).status;
    let off = progress::texts_for(&progress, false).status;
    assert_eq!(on, progress::texts(&progress).status);
    assert!(
        on.contains("Click anywhere on the canvas to cancel."),
        "{on}"
    );
    assert!(!off.contains("Click anywhere"), "{off}");
    assert!(off.contains("Esc"), "{off}");
}

/// A Space-held left press is a pan, never a click: not when the press stays
/// below egui's drag threshold, and not when Space is let go before the
/// button.
#[test]
fn a_space_pan_never_stops_the_job() {
    let mut h = plain_harness(&[ISSUE684]);
    start_long_job(&mut h);
    let at = canvas_point(&h, 300.0, 300.0);
    pointer(&mut h, at, None);
    h.key_down(egui::Key::Space);
    h.step();
    // Press and release at one point with Space held.
    pointer(&mut h, at, Some(true));
    pointer(&mut h, at, Some(false));
    // Press with Space, let Space go, then release.
    pointer(&mut h, at, Some(true));
    h.key_up(egui::Key::Space);
    h.step();
    pointer(&mut h, at, Some(false));
    h.step();
    // A short drag (a pan) that ends after Space went.
    h.key_down(egui::Key::Space);
    h.step();
    pointer(&mut h, at, Some(true));
    pointer(&mut h, at + egui::vec2(40.0, 0.0), None);
    h.key_up(egui::Key::Space);
    h.step();
    pointer(&mut h, at + egui::vec2(40.0, 0.0), Some(false));
    h.step();
    let run = h.state().routing().expect("a pan stopped the job");
    assert!(run.stop_requested.is_none(), "a pan asked for a stop");
    // A plain click right after still stops it: the pan left nothing behind.
    click(&mut h, at);
    let run = h.state().routing();
    assert!(
        run.is_none_or(|r| r.stop_requested.is_some()),
        "the click after the pan did not stop the job"
    );
    run_to_end(&mut h);
}

#[test]
fn the_settings_dialog_has_the_preference() {
    let mut h = plain_harness(&[ISSUE159]);
    exec(&mut h, Command::ShowAutorouteSettings);
    h.get_by_label(freerouting_gui::text::STOP_ON_CLICK_CHECKBOX)
        .click();
    let _ = h.run_ok();
    assert!(!h.state().settings_snapshot().stop_on_canvas_click);
    h.get_by_label(freerouting_gui::text::STOP_ON_CLICK_CHECKBOX)
        .click();
    let _ = h.run_ok();
    assert!(h.state().settings_snapshot().stop_on_canvas_click);
}

// ---------------------------------------------------------------------------
// G07.3: the progress texts
// ---------------------------------------------------------------------------

fn json_number(value: &JsonValue) -> f64 {
    match value {
        JsonValue::Number(text) => text.parse().expect("a number"),
        other => panic!("not a number: {other:?}"),
    }
}

fn member<'a>(object: &'a JsonValue, name: &str) -> &'a JsonValue {
    match object {
        JsonValue::Object(members) => members
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v)
            .unwrap_or_else(|| panic!("{name} missing")),
        other => panic!("not an object: {other:?}"),
    }
}

fn progress_to_json(p: &Progress) -> JsonValue {
    let int = |v: usize| JsonValue::Number(v.to_string());
    let phase = match p.phase {
        Phase::Fanout => "fanout",
        Phase::Autoroute => "autoroute",
        Phase::Optimize => "optimize",
    };
    let mut members = vec![
        ("phase".to_string(), JsonValue::string(phase)),
        ("pass".to_string(), JsonValue::Number(p.pass.to_string())),
        ("items_to_go".to_string(), int(p.items_to_go)),
        ("routed".to_string(), int(p.routed)),
        ("skipped".to_string(), int(p.skipped)),
        ("failed".to_string(), int(p.failed)),
        ("ripped".to_string(), int(p.ripped)),
        ("extra_vias".to_string(), int(p.extra_vias)),
        ("incomplete".to_string(), int(p.incomplete)),
        ("violations".to_string(), int(p.violations)),
        (
            "score".to_string(),
            JsonValue::Number(format!("{}", p.score)),
        ),
    ];
    if let Some((a, b)) = p.airline {
        members.push((
            "airline".to_string(),
            JsonValue::Array(
                [a.x, a.y, b.x, b.y]
                    .iter()
                    .map(|v| JsonValue::Number(format!("{v}")))
                    .collect(),
            ),
        ));
    }
    JsonValue::Object(members)
}

fn progress_from_json(value: &JsonValue) -> Progress {
    let int = |name: &str| json_number(member(value, name)) as usize;
    let phase = match member(value, "phase") {
        JsonValue::String(s) if s == "fanout" => Phase::Fanout,
        JsonValue::String(s) if s == "optimize" => Phase::Optimize,
        _ => Phase::Autoroute,
    };
    let airline = match value {
        JsonValue::Object(members) => members.iter().find(|(n, _)| n == "airline").map(|(_, v)| {
            let JsonValue::Array(a) = v else {
                panic!("airline")
            };
            let n: Vec<f64> = a.iter().map(json_number).collect();
            (FloatPoint::new(n[0], n[1]), FloatPoint::new(n[2], n[3]))
        }),
        _ => None,
    };
    Progress {
        phase,
        pass: json_number(member(value, "pass")) as i32,
        items_to_go: int("items_to_go"),
        routed: int("routed"),
        skipped: int("skipped"),
        failed: int("failed"),
        ripped: int("ripped"),
        extra_vias: int("extra_vias"),
        incomplete: int("incomplete"),
        violations: int("violations"),
        score: json_number(member(value, "score")) as f32,
        airline,
        ..Progress::default()
    }
}

fn recorded() -> Vec<Progress> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/progress_issue159.json");
    let text = std::fs::read_to_string(path).expect("tests/data/progress_issue159.json");
    match JsonValue::parse(&text).expect("JSON") {
        JsonValue::Array(events) => events.iter().map(progress_from_json).collect(),
        other => panic!("not an array: {other:?}"),
    }
}

/// Records the Progress events of a real run of Issue159 into
/// `tests/data/progress_issue159.json` (`cargo test ... record_progress -- --ignored`).
#[test]
#[ignore = "writes tests/data/progress_issue159.json from a real run"]
fn record_progress_issue159() {
    use freerouting::autoroute::events::{CancelToken, EventSink, RouterEvent};
    use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
    use freerouting::job::prepare::{prepare_board_for_routing, prepare_job_settings};
    use freerouting::settings::ResolveContext;
    let bytes = std::fs::read(fixtures().join(ISSUE159)).unwrap();
    let result = freerouting::io::specctra::read_board(&bytes);
    let mut board: RoutingBoard = result.board().expect("board").clone();
    let resolve = ResolveContext { cores: CORES };
    let (patch, _) = prepare_job_settings(&board, None, &[], &resolve);
    let ready = prepare_board_for_routing(&mut board, &patch, &resolve);
    // A sink big enough that no progress event is dropped.
    let (sink, rx) = EventSink::channel(1 << 16);
    let job = PipelineJob {
        settings: &ready.settings,
        board_settings: &ready.board_settings,
        cancel: std::sync::Arc::new(CancelToken::new()),
        events: &sink,
        deadline: None,
        registry: None,
    };
    run_pipeline(board, &job)
        .map_err(|f| f.error.to_string())
        .unwrap();
    let events: Vec<JsonValue> = rx
        .try_iter()
        .filter_map(|e| match e {
            RouterEvent::Progress(p) => Some(progress_to_json(&p)),
            _ => None,
        })
        .collect();
    assert!(events.len() > 10);
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/progress_issue159.json");
    std::fs::write(path, JsonValue::Array(events).to_pretty_string()).unwrap();
}

/// Java's templates, filled by hand from the event's values.
fn expected(p: &Progress) -> [String; 4] {
    let score = format!(
        "Score: {:.2} ({} incomplete connections, {} clearance violations)",
        f64::from(p.score),
        p.incomplete,
        p.violations
    );
    match p.phase {
        Phase::Fanout => [
            format!(
                "Fanout pass #{} (routed {}, failed {}, +{} vias)",
                p.pass, p.routed, p.failed, p.extra_vias
            ),
            format!("To route: {}; routed: {}", p.items_to_go, p.routed),
            format!("Failed: {}; extra vias: +{}", p.failed, p.extra_vias),
            score,
        ],
        _ => [
            format!(
                "Autorouter is running. Click anywhere on the canvas to cancel. Autorouter pass #{}",
                p.pass
            ),
            format!("To route: {}; routed: {}", p.items_to_go, p.routed),
            format!("Ripped up: {}; failed: {}", p.ripped, p.failed),
            score,
        ],
    }
}

#[test]
fn progress_texts_match_events() {
    let events = recorded();
    assert!(events.len() > 10, "{} recorded events", events.len());
    assert!(
        events
            .iter()
            .any(|p| p.phase == Phase::Autoroute && p.routed > 0)
    );
    let mut h = Harness::new_ui_state(
        |ui, p: &mut Progress| {
            let _ = progress::ui(ui, Some(p), false, true);
        },
        events[0].clone(),
    );
    for event in &events {
        *h.state_mut() = event.clone();
        h.run();
        let want = expected(event);
        let lines = progress::texts(event);
        assert_eq!(
            [
                lines.status.clone(),
                lines.add.clone(),
                lines.layer.clone(),
                lines.score.clone()
            ],
            want
        );
        for line in want {
            h.get_by_label(&line);
        }
    }
}

#[test]
fn progress_snapshot() {
    if !rendering_enabled() {
        return;
    }
    let events = recorded();
    let mid = events
        .iter()
        .filter(|p| p.phase == Phase::Autoroute)
        .nth(events.len() / 3)
        .or_else(|| events.last())
        .expect("events")
        .clone();
    let mut h = Harness::builder()
        .with_size([420.0, 170.0])
        .wgpu()
        .build_ui_state(
            |ui, p: &mut Progress| {
                let _ = progress::ui(ui, Some(p), false, true);
            },
            mid,
        );
    h.run();
    h.snapshot("routing_progress_issue159");
}

// ---------------------------------------------------------------------------
// G07.4: live redraw
// ---------------------------------------------------------------------------

type DrawList = BTreeMap<(usize, usize), Vec<(i32, u32, Vec<[f32; 2]>, Vec<u32>, Vec<[f32; 5]>)>>;

/// What a chunk set draws, independent of how it is cut into chunks.
fn draw_list(set: &ChunkSet) -> DrawList {
    let mut out: DrawList = BTreeMap::new();
    for chunk in set.chunks() {
        for e in &chunk.entries {
            let vertices = chunk.mesh.vertices[e.v0 as usize..(e.v0 + e.vn) as usize].to_vec();
            let indices = chunk.mesh.indices[e.i0 as usize..(e.i0 + e.i_n) as usize]
                .iter()
                .map(|i| i - e.v0)
                .collect();
            let lines = chunk.mesh.lines[e.l0 as usize..(e.l0 + e.ln) as usize].to_vec();
            out.entry((
                slot_index(chunk.slot, set.layer_count()),
                kind_index(chunk.kind),
            ))
            .or_default()
            .push((e.id, e.seq, vertices, indices, lines));
        }
    }
    for list in out.values_mut() {
        list.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    }
    out
}

fn assert_scene_is_fresh(h: &Harness<'_, App>) {
    let loaded = h.state().loaded().unwrap();
    let (origin, drawn) = loaded.tess.with(|set| (set.origin(), draw_list(set)));
    let scene = Scene::from_board(&loaded.session.design.name, &loaded.session.board);
    let fresh = draw_list(&ChunkSet::build(&scene, origin));
    assert_eq!(drawn.len(), fresh.len(), "the same batches");
    for (key, list) in &fresh {
        assert_eq!(
            drawn.get(key).map(Vec::len),
            Some(list.len()),
            "batch {key:?}"
        );
        assert!(drawn[key] == *list, "batch {key:?} differs");
    }
    // The scene items equal a fresh flattening too (as sets: the patched
    // scene keeps new items at the end).
    let mut a: Vec<_> = loaded
        .scene
        .items
        .iter()
        .map(|i| format!("{i:?}"))
        .collect();
    let mut b: Vec<_> = scene.items.iter().map(|i| format!("{i:?}")).collect();
    a.sort();
    b.sort();
    assert!(a == b, "the scene items differ from a fresh flattening");
}

#[test]
fn final_scene_equals_fresh_build() {
    let mut h = plain_harness(&[ISSUE159]);
    exec(&mut h, Command::StartRouting);
    run_to_end(&mut h);
    let stats = h
        .state()
        .autoroute()
        .last_stats
        .clone()
        .expect("job counters");
    assert!(stats.delta_events > 0 && stats.items_added > 0, "{stats:?}");
    assert_eq!(
        stats.reconcile_fixes, 0,
        "the deltas already made the picture right"
    );
    assert_scene_is_fresh(&h);
    assert_eq!(h.state().stats().full_rebuilds, 0);
}

#[test]
fn live_redraw_patches_before_the_job_ends() {
    let mut h = plain_harness(&[ISSUE684]);
    let before = trace_count(&h);
    exec(&mut h, Command::StartRouting);
    let drawn_traces = |h: &Harness<'_, App>| {
        h.state()
            .loaded()
            .unwrap()
            .scene
            .items
            .iter()
            .filter(|i| i.kind == freerouting_gui::scene::Kind::Trace)
            .count()
    };
    let start_items = drawn_traces(&h);
    let start = Instant::now();
    let mut live = false;
    while h.state().is_routing() {
        h.step();
        if drawn_traces(&h) > start_items {
            live = true;
        }
        assert!(start.elapsed() < Duration::from_secs(120));
        std::thread::sleep(Duration::from_millis(2));
    }
    let _ = h.run_ok();
    assert!(
        live,
        "traces appeared on the canvas while the router was running"
    );
    assert!(trace_count(&h) > before);
    assert_scene_is_fresh(&h);
    assert_eq!(h.state().stats().full_rebuilds, 0);
    let stats = h.state().autoroute().last_stats.clone().unwrap();
    assert_eq!(stats.reconcile_fixes, 0, "{stats:?}");
}

// ---------------------------------------------------------------------------
// G07.5: the overlay
// ---------------------------------------------------------------------------

#[test]
fn overlay_draws_current_airline() {
    let events = recorded();
    let with_airline = events
        .iter()
        .find(|p| p.airline.is_some())
        .expect("a recorded event with an airline")
        .clone();
    let h = plain_harness(&[ISSUE159]);
    let board = board(&h);
    let frame = Frame::new(board.basic());
    let (a, b) = with_airline.airline.unwrap();
    let list = shapes(&with_airline, &frame, 100);
    assert_eq!(list.len(), 1);
    match list[0] {
        RoutingShape::Line { from, to, width_mm } => {
            assert_eq!((from, to), (frame.pt(a), frame.pt(b)));
            assert!(width_mm > 0.0);
        }
        other => panic!("not a line: {other:?}"),
    }
    // The optimizer's position is a cross of ten default half widths.
    let mut optimizing = with_airline.clone();
    optimizing.airline = None;
    optimizing.optimizer_position = Some(a);
    match shapes(&optimizing, &frame, 100).as_slice() {
        [RoutingShape::Cross { center, radius_mm }] => {
            assert_eq!(*center, frame.pt(a));
            assert_eq!(*radius_mm, frame.len(1000.0));
        }
        other => panic!("{other:?}"),
    }
    assert!(shapes(&Progress::default(), &frame, 100).is_empty());
}

#[test]
fn overlay_snapshot() {
    if !rendering_enabled() {
        return;
    }
    let events = recorded();
    let event = events
        .iter()
        .find(|p| p.airline.is_some())
        .expect("an event with an airline")
        .clone();
    let h0 = plain_harness(&[ISSUE159]);
    let board = board(&h0);
    let frame = Frame::new(board.basic());
    let (a, b) = event.airline.unwrap();
    let (pa, pb) = (frame.pt(a), frame.pt(b));
    let center = freerouting_gui::scene::P::new((pa.x + pb.x) / 2.0, (pa.y + pb.y) / 2.0);
    let look = freerouting_gui::look::Look::new(2);
    let list = shapes(&event, &frame, 100);
    let mut h = Harness::builder()
        .with_size([300.0, 200.0])
        .wgpu()
        .build_ui(move |ui| {
            let rect = ui.max_rect();
            ui.painter()
                .rect_filled(rect, 0.0, egui::Color32::from_gray(20));
            let zoom = 150.0 / (pa.x - pb.x).abs().max((pa.y - pb.y).abs()).max(0.5);
            let camera = freerouting_gui::view::Camera::new(center, zoom);
            freerouting_gui::overlays::routing::draw(ui.painter(), rect, &camera, &list, &look);
        });
    h.run();
    h.snapshot("routing_overlay_issue159");
}

// ---------------------------------------------------------------------------
// G07.6: the summary
// ---------------------------------------------------------------------------

#[test]
fn summary_matches_board_statistics() {
    let mut h = plain_harness(&[ISSUE159]);
    exec(&mut h, Command::StartRouting);
    run_to_end(&mut h);
    let stats = BoardStatistics::new(&board(&h));
    let summary = h.state().autoroute().summary.clone().expect("a summary");
    assert!(h.state().autoroute().summary_open, "shown by default");
    assert_eq!(summary.incomplete, stats.connections.incomplete_count);
    assert_eq!(summary.vias, stats.items.via_count);
    assert_eq!(summary.violations, stats.clearance_violations.total_count);
    assert_eq!(
        summary.trace_length_mm,
        f64::from(stats.traces.total_length_mm)
    );
    assert_eq!(summary.total_nets, stats.nets.total_count);
    // The window shows them.
    let _ = h.run_ok();
    h.get_by_label("Auto-routing Completed");
    h.get_by_label("Show this summary after auto-routing finishes");
    let rows = freerouting_gui::panels::routing_summary::rows(
        &summary,
        freerouting::board::structure::Unit::Mm,
    );
    let via_row = rows.iter().find(|(l, _)| *l == "Vias Added:").unwrap();
    assert_eq!(via_row.1, stats.items.via_count.to_string());

    // "Show after routing" off survives a save and a restore and keeps the
    // window shut on the next run.
    exec(&mut h, Command::SetShowRoutingSummary(false));
    exec(&mut h, Command::CloseRoutingSummary);
    let mut storage = MemStorage::default();
    eframe::App::save(h.state_mut(), &mut storage);
    let saved = h.state().settings_snapshot();
    assert!(!saved.show_routing_summary);
    let mut fresh = Harness::builder()
        .with_size(common::SIZE)
        .build_eframe(|cc| {
            App::build(
                &cc.egui_ctx,
                Some(&storage),
                freerouting_gui::args::Args::default(),
            )
        });
    assert!(!fresh.state().settings_snapshot().show_routing_summary);
    exec(&mut fresh, Command::Open(fixtures().join(ISSUE159)));
    common::wait_for_load(&mut fresh);
    exec(&mut fresh, Command::StartRouting);
    run_to_end(&mut fresh);
    assert!(fresh.state().autoroute().summary.is_some());
    assert!(
        !fresh.state().autoroute().summary_open,
        "suppressed on the next run"
    );
}

#[derive(Default)]
struct MemStorage(std::collections::HashMap<String, String>);

impl eframe::Storage for MemStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }
    fn flush(&mut self) {}
}

// ---------------------------------------------------------------------------
// N01.3, N01.4: the profile selector and the "Algorithms" section
// ---------------------------------------------------------------------------

fn dialog(h: &Harness<'_, App>) -> Draft {
    h.state()
        .autoroute()
        .dialog
        .clone()
        .expect("the dialog is open")
}

#[test]
fn profile_selector_covers_both_profiles() {
    use freerouting::settings::Profile;
    let mut h = plain_harness(&[ISSUE159]);
    exec(&mut h, Command::ShowAutorouteSettings);
    assert_eq!(dialog(&h).profile(), Profile::Enhanced, "the default");
    // Pick Legacy in the selector.
    h.get_by_role_and_label(Role::ComboBox, "Profile").click();
    let _ = h.run_ok();
    h.get_by_label("Legacy").click();
    let _ = h.run_ok();
    let draft = dialog(&h);
    assert_eq!(draft.profile(), Profile::Legacy);
    assert_eq!(draft.patch.profile.as_deref(), Some("legacy"));
    // Applied, the profile is kept between runs like any control.
    exec(
        &mut h,
        Command::ApplyAutorouteSettings(Box::new(draft.patch)),
    );
    let kept: BTreeMap<_, _> = h
        .state()
        .settings_snapshot()
        .autoroute
        .into_iter()
        .collect();
    assert_eq!(kept.get("profile").map(String::as_str), Some("legacy"));
    // And back to Enhanced.
    exec(&mut h, Command::ShowAutorouteSettings);
    h.get_by_role_and_label(Role::ComboBox, "Profile").click();
    let _ = h.run_ok();
    h.get_by_label("Enhanced").click();
    let _ = h.run_ok();
    assert_eq!(dialog(&h).profile(), Profile::Enhanced);
}

#[test]
fn an_algorithm_stage_changes_and_resets() {
    let mut h = plain_harness(&[ISSUE159]);
    exec(&mut h, Command::ShowAutorouteSettings);
    h.get_by_label("Algorithms").click();
    let _ = h.run_ok();
    assert!(h.query_by_label("Reset Differential pairs").is_none());
    // Every slot of the catalog has a row.
    for slot in dialog(&h).stage_slots() {
        assert!(
            h.query_all_by_label(slot.desc.label).next().is_some(),
            "{} has no row",
            slot.desc.label
        );
    }
    // Change one stage: run the unrouted report as the differential-pair stage.
    h.get_by_role_and_label(Role::ComboBox, "Differential pairs")
        .click();
    let _ = h.run_ok();
    h.get_by_label("freerouting-unrouted-report").click();
    let _ = h.run_ok();
    let draft = dialog(&h);
    assert_eq!(
        draft.get("special.diff_pairs"),
        Some(FieldValue::Str("freerouting-unrouted-report".to_string()))
    );
    assert!(draft.differs_from_profile("special.diff_pairs"));
    // Reset returns it to the profile's value (unset: no stage).
    h.get_by_label("Reset Differential pairs").click();
    let _ = h.run_ok();
    let draft = dialog(&h);
    assert_eq!(draft.get("special.diff_pairs"), None);
    assert!(!draft.differs_from_profile("special.diff_pairs"));
    assert!(h.query_by_label("Reset Differential pairs").is_none());
}

#[test]
fn an_option_reset_returns_the_profile_value() {
    let mut draft = Draft::new(default_patch(CORES), CORES);
    draft.set_option_text("fanout.max_passes", "3");
    assert!(draft.differs_from_profile("fanout.max_passes"));
    draft.set_option_text("fanout.max_passes", "not a number");
    assert_eq!(draft.get("fanout.max_passes"), Some(FieldValue::Int(3)));
    draft.reset_to_profile("fanout.max_passes");
    assert_eq!(draft.get("fanout.max_passes"), Some(FieldValue::Int(20)));
    draft.set_option_text("fanout.timeout", "1:00");
    draft.set_option_text("fanout.timeout", " ");
    assert_eq!(
        draft.get("fanout.timeout"),
        None,
        "blank clears an optional option"
    );
}

/// Q21.3: the fanout row's `fanout.pin_escape` choice sets the key, and
/// picking `off` clears it again (the profile's unset).
#[test]
fn the_pin_escape_choice_sets_the_key() {
    let mut h = plain_harness(&[ISSUE159]);
    exec(&mut h, Command::ShowAutorouteSettings);
    h.get_by_label("Algorithms").click();
    let _ = h.run_ok();
    assert_eq!(dialog(&h).get("fanout.pin_escape"), None, "off by default");
    h.get_by_role_and_label(Role::ComboBox, "fanout.pin_escape")
        .scroll_to_me();
    let _ = h.run_ok();
    h.get_by_role_and_label(Role::ComboBox, "fanout.pin_escape")
        .click();
    let _ = h.run_ok();
    h.get_by_label("straight").click();
    let _ = h.run_ok();
    let draft = dialog(&h);
    assert_eq!(
        draft.get("fanout.pin_escape"),
        Some(FieldValue::Str("straight".to_string()))
    );
    assert!(draft.differs_from_profile("fanout.pin_escape"));
    // Applied, the job's settings carry it.
    exec(
        &mut h,
        Command::ApplyAutorouteSettings(Box::new(draft.patch.clone())),
    );
    let kept: BTreeMap<_, _> = h
        .state()
        .settings_snapshot()
        .autoroute
        .into_iter()
        .collect();
    assert_eq!(
        kept.get("fanout.pin_escape").map(String::as_str),
        Some("straight")
    );
    // `off` clears it.
    let mut draft = draft;
    draft.set_choice("fanout.pin_escape", "off");
    assert_eq!(draft.get("fanout.pin_escape"), None);
    draft.set_choice("fanout.pin_escape", "bent");
    assert_eq!(draft.get("fanout.pin_escape"), None, "not a value");
}

/// N05.1: the Algorithms section offers the KRT stages besides Freerouting's.
#[test]
fn the_algorithms_section_offers_the_krt_stages() {
    let mut h = plain_harness(&[ISSUE159]);
    exec(&mut h, Command::ShowAutorouteSettings);
    h.get_by_label("Algorithms").click();
    let _ = h.run_ok();
    let slots = dialog(&h).stage_slots();
    let ids_of = |kind: &str| -> Vec<String> {
        slots
            .iter()
            .filter(|s| s.desc.kind == kind)
            .flat_map(|s| s.ids.iter().map(|id| id.to_string()))
            .collect()
    };
    let routers = ids_of("connection_router");
    assert!(
        routers.iter().any(|id| id == "freerouting-connection"),
        "{routers:?}"
    );
    assert!(ids_of("fanout").iter().any(|id| id == "krt-bga-escape"));
    // The experimental stage is offered and marked (N02.4).
    let pairs = slots.iter().find(|s| s.desc.kind == "diff_pairs").unwrap();
    assert!(pairs.ids.contains(&"krt-diffpair"));
    assert!(
        pairs
            .experimental_ids
            .iter()
            .any(|e| e.key == "krt-diffpair")
    );
}
