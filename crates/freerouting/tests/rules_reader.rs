//! C04.1 to C04.4: reading a rules file onto a board, and `prepare_job`'s
//! rules discovery.
//!
//! Ports `RulesRoundTripTest` (`invalidRulesFileReturnsFalse`,
//! `rulesViaWithStrippedPadstackNameResolvesToDecimalPadstack`,
//! `readExistingRulesFixture`) and `RulesFileSettingsTest` through
//! `apply_rules_file` (the settings half is also checked by
//! `settings_autoroute_scope.rs`). Not ported:
//! - `RulesRoundTripTest.rulesRoundTrip` and
//!   `rulesRoundTripWithAutorouteSettings`: they write rules first, so they
//!   are in `rules_writer.rs` (C04.6);
//! - `RulesRoundTripTest.loadingProducesWarningsForDegenerateWires`: a DSN
//!   reader test, ported with the DSN reader;
//! - `RulesFileSettingsTest.priorityIs40`: Java source metadata
//!   (`Source::RulesFile` carries priority 40 and the name `RULES file`).
//!
//! The comparison with Java's dumps (clearance matrix, default trace half
//! widths, net classes, via infos in order, via rules, snap angle, padstacks,
//! and the settings) is `job_prepare_reference.rs` on the five fixtures with
//! an adjacent rules file and `explicit-cost-rules`, in all four scenarios.

use std::path::{Path, PathBuf};

use freerouting::board::RoutingBoard;
use freerouting::board::structure::AngleRestriction;
use freerouting::drc::json::JsonValue;
use freerouting::drc::load_design;
use freerouting::dsn::ast::RulesFile;
use freerouting::io::specctra::{RulesOutcome, apply_rules_file, read_rules};
use freerouting::job::prepare::{JobInput, RulesChoice, find_rules_file, prepare_job};
use freerouting::settings::{ResolveContext, RouterSettings, parse_cli_arguments};
use freerouting_geometry::jmath::round_to_i32;

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn board(file: &str) -> RoutingBoard {
    load_design(&std::fs::read(repo(&format!("fixtures/{file}"))).unwrap()).unwrap()
}

fn rules_text(file: &str) -> String {
    freerouting::dsn::decode(&std::fs::read(repo(&format!("fixtures/{file}"))).unwrap())
        .into_owned()
}

fn read(board: RoutingBoard, text: &str, name: &str) -> (RoutingBoard, RulesOutcome) {
    let (board, result) = read_rules(board, text, name);
    (board, result.unwrap_or_else(|e| panic!("{e}")))
}

/// The default net class's trace half width on each layer.
fn half_widths(board: &RoutingBoard) -> Vec<i32> {
    let class = board.rules.default_net_class_id().unwrap();
    (0..board.layer_count() as i32)
        .map(|l| board.rules.net_classes.get(class).trace_half_width(l))
        .collect()
}

fn dsn_half_width(board: &RoutingBoard, width: f64) -> i32 {
    round_to_i32(board.communication.coordinate_transform.dsn_to_board(width) / 2.0)
}

fn via_info_names(board: &RoutingBoard) -> Vec<String> {
    board
        .rules
        .via_infos
        .iter()
        .map(|(_, info)| info.name.clone())
        .collect()
}

fn warnings(outcome: &RulesOutcome) -> Vec<&str> {
    outcome.log.iter().map(|l| l.text.as_str()).collect()
}

// ---- C04.1: header, rule, layer ------------------------------------------

#[test]
fn width_rule_sets_the_default_half_width_on_every_layer() {
    let board = board("Issue029-hw48na.dsn");
    let before = half_widths(&board);
    let (board, outcome) = read(
        board,
        "(rules PCB Issue029-hw48na (rule (width 400)))",
        "Issue029-hw48na",
    );
    let expected = dsn_half_width(&board, 400.0);
    assert_ne!(before, vec![expected; before.len()]);
    assert_eq!(half_widths(&board), vec![expected; before.len()]);
    assert!(outcome.log.is_empty(), "{:?}", warnings(&outcome));
}

#[test]
fn rules_layer_scope_applies_to_the_named_layer_only() {
    let board = board("Issue029-hw48na.dsn");
    let before = half_widths(&board);
    let (board, _) = read(
        board,
        "(rules PCB Issue029-hw48na (layer B.Cu (rule (width 500))))",
        "Issue029-hw48na",
    );
    let after = half_widths(&board);
    assert_eq!(after[0], before[0]);
    assert_eq!(after[1], dsn_half_width(&board, 500.0));
}

#[test]
fn rules_layer_scope_with_unknown_layer_applies_to_all_layers() {
    // Java's `applyRules` keeps `layerIndex = -1` ("default") for a layer it
    // cannot find, logs a warning and applies the rule everywhere.
    let board = board("Issue029-hw48na.dsn");
    let before = half_widths(&board);
    let (board, outcome) = read(
        board,
        r#"(rules PCB Issue029-hw48na (layer "No.Such.Layer" (rule (width 400))))"#,
        "Issue029-hw48na",
    );
    let expected = dsn_half_width(&board, 400.0);
    assert_ne!(before, vec![expected; before.len()]);
    assert_eq!(half_widths(&board), vec![expected; before.len()]);
    assert_eq!(
        warnings(&outcome),
        vec!["RulesReader.applyRules: layer not found: 'No.Such.Layer'"]
    );
}

#[test]
fn clearance_rule_goes_through_the_dsn_helper() {
    let board = board("Issue029-hw48na.dsn");
    let transform = board.communication.coordinate_transform;
    let (board, outcome) = read(
        board,
        "(rules PCB Issue029-hw48na (rule (clear 333.3)) (layer F.Cu (rule (clear 444.4))))",
        "Issue029-hw48na",
    );
    let matrix = &board.rules.clearance_matrix;
    // `(clear v)` without types sets the default value (class 1 against
    // itself) on every layer, and a layer scope on one layer.
    // (The matrix stores even values: an odd one is rounded up.)
    let even = |v: i32| v + v % 2;
    assert_eq!(
        matrix.get_value(1, 1, 1, false),
        even(round_to_i32(transform.dsn_to_board(333.3)))
    );
    assert_eq!(
        matrix.get_value(1, 1, 0, false),
        even(round_to_i32(transform.dsn_to_board(444.4)))
    );
    assert!(outcome.log.is_empty(), "{:?}", warnings(&outcome));
}

#[test]
fn invalid_rules_file_returns_an_error_and_leaves_the_board_unchanged() {
    // `RulesRoundTripTest.invalidRulesFileReturnsFalse`.
    let board = board("Issue029-hw48na.dsn");
    let widths = half_widths(&board);
    let infos = via_info_names(&board);
    let classes = board.rules.clearance_matrix.class_count();
    let (board, result) = read_rules(board, "not a rules file", "x");
    assert!(result.is_err());
    assert_eq!(half_widths(&board), widths);
    // Java applies the scopes before a syntax error; the port parses the
    // whole file first (docs/DIVERGENCES.md, "Rules files").
    let (board, result) = read_rules(board, "(rules PCB x (rule (width 400)) (via \"v\" ", "x");
    assert!(result.is_err());
    assert_eq!(half_widths(&board), widths);
    assert_eq!(via_info_names(&board), infos);
    assert_eq!(board.rules.clearance_matrix.class_count(), classes);
}

#[test]
fn a_known_valid_rules_fixture_reads() {
    // `RulesRoundTripTest.readExistingRulesFixture`.
    let (_, result) = read_rules(
        board("Issue029-hw48na.dsn"),
        &rules_text("Issue029-hw48na_valid.rules"),
        "hw48na",
    );
    assert!(result.is_ok());
}

#[test]
fn a_different_design_name_is_a_warning_and_reading_goes_on() {
    let board = board("Issue029-hw48na.dsn");
    let (board, outcome) = read(
        board,
        "(rules PCB hw48na (rule (width 400)))",
        "Issue029-hw48na",
    );
    let lines = warnings(&outcome);
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("RulesReader.read: designName not matching at '")
            && lines[0].ends_with("' (expected 'Issue029-hw48na', got 'hw48na')"),
        "{}",
        lines[0]
    );
    // The scope identifier is whatever the DSN read noted last, as in Java's
    // static field; Java's own log for this board says '/J801_5V'.
    assert!(lines[0].contains("'/J801_5V'"), "{}", lines[0]);
    assert_eq!(half_widths(&board)[0], dsn_half_width(&board, 400.0));
}

// ---- C04.2: padstacks, via infos, via rules, net classes, snap angle ---------

#[test]
fn rules_via_with_stripped_padstack_name_resolves_to_decimal_padstack() {
    // `RulesRoundTripTest.rulesViaWithStrippedPadstackNameResolvesToDecimalPadstack`.
    let board = board("Issue143-rpi_splitter.dsn");
    assert!(
        board
            .library
            .padstacks
            .get_by_name("Round1$13.779528")
            .is_some()
    );
    let (board, outcome) = read(
        board,
        "(rules PCB Issue143\n  (rule\n    (via \"Round1$13\")\n  )\n)\n",
        "Issue143",
    );
    assert!(
        board
            .library
            .get_via_padstack_by_name("Round1$13.779528")
            .is_some(),
        "the via padstack stays registered"
    );
    assert!(outcome.log.is_empty(), "{:?}", warnings(&outcome));

    // The Java test is weaker than its name: a `via` inside a `rule` scope is
    // skipped, and the padstack was a via padstack already. A `via` scope that
    // names the padstack resolves it through the library.
    let (board, outcome) = read(
        board,
        r#"(rules PCB Issue143 (via "typed" "Round1$13.779528" default))"#,
        "Issue143",
    );
    assert!(outcome.log.is_empty(), "{:?}", warnings(&outcome));
    let id = board.rules.via_infos.get_by_name("typed").unwrap();
    let padstack = board.rules.via_infos.get(id).padstack;
    assert_eq!(
        board.library.padstacks.get(padstack).unwrap().name,
        "Round1$13.779528"
    );
}

#[test]
fn a_via_with_an_existing_name_moves_to_the_end() {
    let board = board("Issue029-hw48na.dsn");
    let before = via_info_names(&board);
    let first = before[0].clone();
    let other = board
        .library
        .padstacks
        .iter()
        .find(|p| p.name != first && board.library.get_via_padstack_by_name(&p.name).is_some())
        .map(|p| p.name.clone())
        .unwrap();
    let text = format!(r#"(rules PCB Issue029-hw48na (via "{first}" "{other}" default))"#);
    let (board, outcome) = read(board, &text, "Issue029-hw48na");
    assert!(outcome.log.is_empty(), "{:?}", warnings(&outcome));
    let after = via_info_names(&board);
    assert_eq!(after.len(), before.len());
    assert_eq!(after.last(), Some(&first));
    assert_eq!(&after[..after.len() - 1], &before[1..]);
    let id = board.rules.via_infos.get_by_name(&first).unwrap();
    assert_eq!(
        board
            .library
            .padstacks
            .get(board.rules.via_infos.get(id).padstack)
            .unwrap()
            .name,
        other
    );
}

#[test]
fn an_unknown_via_padstack_is_skipped_with_javas_warning() {
    let board = board("Issue029-hw48na.dsn");
    let before = via_info_names(&board);
    let (board, outcome) = read(
        board,
        r#"(rules PCB Issue029-hw48na (via "v" "No.Such.Padstack" default))"#,
        "Issue029-hw48na",
    );
    assert_eq!(via_info_names(&board), before);
    assert_eq!(
        warnings(&outcome),
        vec!["Network.read_via_info: padstack not found at 'No.Such.Padstack'"]
    );
}

#[test]
fn a_padstack_scope_adds_a_library_padstack() {
    let board = board("Issue029-hw48na.dsn");
    let count = board.library.padstacks.count();
    let text = r#"(rules PCB Issue029-hw48na
        (padstack "TestPad" (shape (circle F.Cu 1000)) (shape (circle B.Cu 1000)) (attach off))
        (padstack "Round[A]Pad_1400_um" (shape (circle F.Cu 5)))
      )"#;
    let (board, outcome) = read(board, text, "Issue029-hw48na");
    assert!(outcome.log.is_empty(), "{:?}", warnings(&outcome));
    // The second padstack exists already and is left alone.
    assert_eq!(board.library.padstacks.count(), count + 1);
    let added = board.library.padstacks.get_by_name("TestPad").unwrap();
    assert!(!added.attach_allowed);
    assert!(added.get_shape(0).is_some() && added.get_shape(1).is_some());
}

#[test]
fn via_rules_and_net_classes_apply_in_file_order() {
    let board = board("Issue029-hw48na.dsn");
    let rules_before = board.rules.via_rules.count();
    let classes_before = board.rules.net_classes.count();
    let text = r#"(rules PCB Issue029-hw48na
        (via "t1" "Via[0-1]_600:300_um" default)
        (via_rule "tr" "t1")
        (via_rule "bad" "no-such-via")
        (class "TESTCLS" "no-such-net" (rule (width 300)) (via_rule "tr"))
        (snap_angle ninety_degree)
      )"#;
    let (board, outcome) = read(board, text, "Issue029-hw48na");
    assert_eq!(
        warnings(&outcome),
        vec!["Network.insert_via_rules: viaInfo not found"]
    );
    assert_eq!(board.rules.via_rules.count(), rules_before + 1);
    let rule = board.rules.via_rules.get_by_name("tr").unwrap();
    assert_eq!(board.rules.via_rules.get(rule).via_count(), 1);
    assert!(board.rules.via_rules.get_by_name("bad").is_none());
    assert_eq!(board.rules.net_classes.count(), classes_before + 1);
    let class = board.rules.net_classes.get_by_name("TESTCLS").unwrap();
    let class = board.rules.net_classes.get(class);
    assert_eq!(class.trace_half_width(0), dsn_half_width(&board, 300.0));
    assert_eq!(class.via_rule(), Some(rule));
    assert_eq!(
        board.rules.trace_angle_restriction(),
        AngleRestriction::NinetyDegree
    );
}

// ---- C04.3: autoroute_settings on top of the merged settings -----------------

const CORES: usize = 8;

fn ctx() -> ResolveContext {
    ResolveContext { cores: CORES }
}

fn member<'a>(value: &'a JsonValue, key: &str) -> &'a JsonValue {
    match value {
        JsonValue::Object(m) => m
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
            .unwrap_or_else(|| panic!("no member {key}")),
        _ => panic!("not an object looking for {key}"),
    }
}

fn number(value: &JsonValue) -> f64 {
    match value {
        JsonValue::Number(text) => text.parse().unwrap(),
        other => panic!("not a number: {other:?}"),
    }
}

fn boolean(value: &JsonValue) -> bool {
    match value {
        JsonValue::Bool(b) => *b,
        other => panic!("not a boolean: {other:?}"),
    }
}

fn cli_patch(base: &str) -> freerouting::settings::SettingsPatch {
    let args: Vec<String> = [
        "-de",
        &format!("{base}.dsn"),
        "-do",
        &format!("{base}.ses"),
        "-mp",
        "7",
        "-mt",
        "3",
        "-oit",
        "0.035",
        "-inc",
        "1A_EXTERNAL_1oz",
        "--router.via_costs=77",
        "--router.scoring.plane_via_costs=7",
        "--router.optimizer.enabled=false",
        "--router.default_undesired_direction_trace_cost=1.5",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    parse_cli_arguments(&args).patch
}

#[test]
fn rules_autoroute_settings_override_cli() {
    // C01.1's `cli` scenario on `Issue029-hw48na`: the CLI asks for
    // `via_costs=77`, `plane_via_costs=7` and the optimizer off; the adjacent
    // rules file's `autoroute_settings` (50, 5, `postroute` on by default)
    // win, and re-enable every layer.
    let bytes = std::fs::read(repo("fixtures/Issue029-hw48na.dsn")).unwrap();
    let mut input = JobInput::new(bytes, "Issue029-hw48na");
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = Some(repo("fixtures"));
    input.cli = Some(cli_patch("Issue029-hw48na"));
    let job = prepare_job(&input, &ctx()).unwrap();
    assert!(job.rules_path.is_some());

    // Without the rules file the CLI values stand.
    input.rules = RulesChoice::None;
    let plain = prepare_job(&input, &ctx()).unwrap();
    assert_eq!(plain.settings.scoring.via_costs, 77);
    assert_eq!(plain.settings.scoring.plane_via_costs, 7);
    assert!(!plain.settings.optimizer.enabled);

    assert_eq!(job.settings.scoring.via_costs, 50);
    assert_eq!(job.settings.scoring.plane_via_costs, 5);
    assert_eq!(job.settings.scoring.start_ripup_costs, 100);
    assert!(job.settings.optimizer.enabled);
    assert!(job.settings.run_router());
    let layers = job.patch.layers.as_ref().unwrap();
    assert!(layers.iter().all(|l| l.routable == Some(true)));
    // Settings the rules file does not mention keep the CLI's value.
    assert_eq!(job.settings.autorouter.max_passes, 7);
    assert_eq!(
        job.settings.scoring.default_undesired_direction_trace_cost,
        1.5
    );

    // Field by field against Java's dump of the same scenario.
    let dump = JsonValue::parse(
        &std::fs::read_to_string(repo("fixtures/reference/settings/Issue029-hw48na.json")).unwrap(),
    )
    .unwrap();
    let java = member(member(member(&dump, "scenarios"), "cli"), "settings");
    let scoring = member(java, "scoring");
    assert_eq!(number(member(scoring, "via_costs")), 50.0);
    assert_eq!(number(member(scoring, "plane_via_costs")), 5.0);
    assert_eq!(number(member(scoring, "start_ripup_costs")), 100.0);
    assert_eq!(
        number(member(scoring, "default_undesired_direction_trace_cost")),
        job.settings.scoring.default_undesired_direction_trace_cost
    );
    assert!(boolean(member(member(java, "optimizer"), "enabled")));
    assert_eq!(
        number(member(member(java, "autorouter"), "max_passes")),
        f64::from(job.settings.autorouter.max_passes)
    );
    let JsonValue::Array(java_layers) = member(java, "layers") else {
        panic!("layers")
    };
    assert_eq!(java_layers.len(), layers.len());
    for (java, rust) in java_layers.iter().zip(layers) {
        assert_eq!(boolean(member(java, "routable")), rust.routable.unwrap());
    }
}

/// `RulesFileSettingsTest` through `apply_rules_file`: the settings that the
/// reader returns for a rules file, read the way the router sees them.
fn outcome_settings(design: &str, rules: &str, name: &str) -> RouterSettings {
    let board = board(design);
    let rules = RulesFile::parse(&rules_text(rules)).unwrap();
    let (_, outcome) = apply_rules_file(board, &rules, name);
    let patch = outcome.autoroute.expect("the file has autoroute_settings");
    freerouting::settings::resolve(&[(freerouting::settings::Source::RulesFile, patch)], &ctx())
        .settings
}

#[test]
fn rules_file_settings_of_processor_rules() {
    let s = outcome_settings(
        "Issue191-processor.Z80/processor.Z80.dsn",
        "Issue191-processor.Z80/processor.rules",
        "processor",
    );
    assert!(s.allowed_via_types);
    assert_eq!(s.scoring.via_costs, 50);
    assert_eq!(s.scoring.plane_via_costs, 5);
    assert_eq!(s.scoring.start_ripup_costs, 100);
    assert!(s.run_router());
    assert!(s.run_optimizer());
    assert_eq!(s.layer_count(), 2);
    assert!(s.layer_active(0) && s.layer_active(1));
    assert!(s.preferred_direction_is_horizontal(0));
    assert_eq!(s.preferred_direction_trace_costs(0), 1.0);
    assert_eq!(s.against_preferred_direction_trace_costs(0), 2.5);
    assert!(!s.preferred_direction_is_horizontal(1));
    assert_eq!(s.preferred_direction_trace_costs(1), 1.0);
    assert_eq!(s.against_preferred_direction_trace_costs(1), 1.7);
}

#[test]
fn rules_file_settings_of_hw48na_rules() {
    let s = outcome_settings(
        "Issue029-hw48na.dsn",
        "Issue029-hw48na_valid.rules",
        "hw48na",
    );
    assert!(s.allowed_via_types);
    assert_eq!(s.scoring.via_costs, 50);
    assert_eq!(s.scoring.plane_via_costs, 5);
    assert_eq!(s.scoring.start_ripup_costs, 100);
    assert_eq!(s.layer_count(), 2);
    assert!(!s.preferred_direction_is_horizontal(0));
    assert_eq!(s.preferred_direction_trace_costs(0), 1.0);
    assert_eq!(s.against_preferred_direction_trace_costs(0), 2.0);
    assert!(s.preferred_direction_is_horizontal(1));
    assert_eq!(s.preferred_direction_trace_costs(1), 1.0);
    assert_eq!(s.against_preferred_direction_trace_costs(1), 2.0);
}

#[test]
fn a_scope_naming_an_unknown_layer_is_dropped() {
    // `AutorouteSettings.readScope` returns null for a `layer_rule` that
    // names a layer the board does not have, and `RulesReader` ignores it.
    let board = board("Issue029-hw48na.dsn");
    let text = "(rules PCB Issue029-hw48na (autoroute_settings (via_costs 9) \
                (layer_rule Nope.Cu (active off))))";
    let (_, outcome) = read(board, text, "Issue029-hw48na");
    assert!(outcome.autoroute.is_none());
}

// ---- C04.4: discovery ---------------------------------------------------------

fn dsn_input(name: &str, dir: &Path) -> JobInput {
    let mut input = JobInput::new(Vec::new(), JobInput::design_name_of(name));
    input.dsn_dir = Some(dir.to_path_buf());
    input
}

#[test]
fn adjacent_rules_use_the_file_name_up_to_the_last_dot() {
    // `fixtures/Issue191-processor.Z80/` holds `processor.Z80.dsn` and
    // `processor.rules`: the adjacent file of `processor.Z80.dsn` is
    // `processor.Z80.rules`, which does not exist.
    let dir = repo("fixtures/Issue191-processor.Z80");
    assert!(dir.join("processor.rules").exists());
    let input = dsn_input("processor.Z80.dsn", &dir);
    assert_eq!(input.design_name, "processor.Z80");
    assert_eq!(find_rules_file(&input), None);

    // `Issue029-hw48na.dsn` finds `Issue029-hw48na.rules`.
    let input = dsn_input("Issue029-hw48na.dsn", &repo("fixtures"));
    assert_eq!(
        find_rules_file(&input),
        Some(repo("fixtures").join("Issue029-hw48na.rules"))
    );
}

#[test]
fn an_explicit_rules_file_replaces_the_adjacent_one() {
    let dir = repo("fixtures");
    let mut input = dsn_input("Issue029-hw48na.dsn", &dir);
    input.rules = RulesChoice::Explicit(dir.join("Issue029-hw48na_valid.rules"));
    assert_eq!(
        find_rules_file(&input),
        Some(dir.join("Issue029-hw48na_valid.rules"))
    );
    // A named file that does not exist is not read, and Java's chain does not
    // fall back to the adjacent file either.
    input.rules = RulesChoice::Explicit(dir.join("missing.rules"));
    assert_eq!(find_rules_file(&input), None);
    input.rules = RulesChoice::None;
    assert_eq!(find_rules_file(&input), None);
}

#[test]
fn an_explicit_rules_file_is_applied_by_prepare_job() {
    let dir = repo("fixtures");
    let bytes = std::fs::read(dir.join("Issue029-hw48na.dsn")).unwrap();
    let mut input = JobInput::new(bytes, "Issue029-hw48na");
    input.rules = RulesChoice::Explicit(dir.join("Issue029-hw48na_valid.rules"));
    let job = prepare_job(&input, &ctx()).unwrap();
    assert_eq!(
        job.rules_path,
        Some(dir.join("Issue029-hw48na_valid.rules"))
    );
    // The header name is `hw48na`: a warning, then the file is applied.
    assert!(
        job.log
            .iter()
            .any(|l| l.text.contains("designName not matching")),
        "{:?}",
        job.log
    );
}
