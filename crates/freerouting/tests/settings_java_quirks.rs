//! N06.3: the `router.java_quirks.*` switches and the `JavaQuirks` value.
//!
//! - one switch per (b) and (c) row of the N06 inventory, each a settings key
//!   whose unset value is `true` (Java);
//! - `legacy` keeps every Java bug, and `enhanced` fixes exactly the switches
//!   N06.13 promoted ([`PROMOTED`]);
//! - the keys round-trip through a settings file, the command line and the
//!   environment;
//! - the job puts the switches on the board, its router settings and the
//!   autoroute control, and the board keeps them through a deep copy and a
//!   snapshot restore, each switch alone too;
//! - every switch has a `// PARITY(quirk, java_quirks.<id>)` marker at its site;
//! - both behaviours of the fixes whose read sites are plain functions (the
//!   others are pinned next to their code: `in_front_of_pad_keeps_java_typo`,
//!   `rational_perpendicular_projection_switch_both_behaviours`,
//!   `bounding_box_switch_both_behaviours`, `threshold_fraction_switch_both_behaviours`).

use freerouting::autoroute::batch::fanout_rules::PinSortingOrder;
use freerouting::autoroute::control::AutorouteControl;
use freerouting::autoroute::item_route_result::ItemRouteResult;
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::io::specctra::{BoardReadResult, read_board_from_str_with};
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::env::parse_environment;
use freerouting::settings::java_quirks::SETTINGS_PREFIX;
use freerouting::settings::json_file::patch_from_config_text;
use freerouting::settings::profile::{ENHANCED, legacy_pin};
use freerouting::settings::{
    FIELDS, JavaQuirks, Profile, ResolveContext, RouterSettings, SettingsPatch, Source,
    default_patch, parse_cli_arguments, resolve,
};

const CORES: usize = 8;

/// The N06 inventory: 35 rows of kind (b) (two added by N06.14, one by N06.15 and one
/// by N06.16) and 5 of kind (c) (N07.4 removed the six (c) switches no code read), and
/// K15.5's `same_net_via_pad_violation`, K15.2's `placed_shape_convex`, K15.19's
/// `edge_pin_net_crosses_outline` and K15.50's `fanout_two_pass_cycle`.
const SWITCH_COUNT: usize = 44;

/// The switches profile `enhanced` turns off (N06.13), in inventory order. The
/// first round (2026-10-19): the accepted batches M (N06.5), F (N06.6), S
/// (N06.7), P (N06.8), I (N06.10) and U (N06.11), and N06.12's flank,
/// rules-scope and skip-length fixes. The second round (2026-10-20):
/// `simplex_prev_division_line` (N06.4, inert), the optimizer's two (N06.9),
/// `shove_via_into_pending_piece` (N06.15) and `any_angle_room_side_index`
/// (N06.16). The third round (2026-10-20): N06.14's two cutout switches. K15.19's
/// `edge_pin_net_crosses_outline`, a correctness rule (decided 2026-10-20). Not
/// promoted: `octagon_box_cutout_branch3` and `ripup_pass_no_unset` (rejected) and
/// the (c) rows left as Java.
const PROMOTED: [&str; 38] = [
    "simplex_prev_division_line",
    "cutout_hole_crosses_border",
    "simplex_octagon_half_corner",
    "init_failure_room_leak",
    "failed_attempt_ripped_carryover",
    "ortho_zero_width_room",
    "edge_corner_not_advanced",
    "connect_to_trace_swapped_width",
    "fanout_stale_drill_carry",
    "drill_destination_room_index",
    "fanout_pin_order_names",
    "in_front_of_pad_typo",
    "store_trace_class_self_compare",
    "rational_perp_projection",
    "smooth_loses_trace",
    "pull_tight_reused_lines_change",
    "opt_first_pass_stops",
    "opt_equal_key_skip",
    "dsn_via_alias",
    "dsn_class_class_pairs",
    "dsn_shape_bbox",
    "dsn_unit_ignored",
    "ses_reader_scale",
    "stats_line_ab_length",
    "route_result_int_via_ratio",
    "drc_units_10000_per_mm",
    "snapshot_drops_compensation",
    "rules_matrix_trees_not_rebuilt",
    "move_drill_tidy_region",
    "route_change_layer_no_via",
    "route_connect_inner_break",
    "select_extension_ignores_filter",
    "shove_via_into_pending_piece",
    "any_angle_room_side_index",
    "in_front_of_pad_side0_flank",
    "rules_unknown_layer_scope",
    "any_angle_skip_length_squared",
    "edge_pin_net_crosses_outline",
];

fn ctx() -> ResolveContext {
    ResolveContext { cores: CORES }
}

fn resolved(sources: Vec<(Source, SettingsPatch)>) -> RouterSettings {
    let mut all = vec![(Source::Defaults, default_patch(CORES))];
    all.extend(sources);
    resolve(&all, &ctx()).settings
}

/// `sources` under `legacy`, where every unset switch is Java's.
fn resolved_legacy(mut sources: Vec<(Source, SettingsPatch)>) -> RouterSettings {
    sources.push((Source::Testing, legacy_pin()));
    resolved(sources)
}

/// Java's switches with the promoted ones `false`: what `enhanced` resolves to.
fn enhanced_quirks() -> JavaQuirks {
    PROMOTED.iter().fold(JavaQuirks::JAVA, |q, id| {
        q.with(id, false).expect("a known id")
    })
}

fn fixture(name: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join("fixtures").join(name)).expect("fixture")
}

fn read(name: &str, quirks: JavaQuirks) -> RoutingBoard {
    match read_board_from_str_with(&fixture(name), quirks) {
        BoardReadResult::Success { board, .. } => *board,
        _ => panic!("{name} reads"),
    }
}

#[test]
fn every_switch_is_a_settings_key_in_inventory_order() {
    let table: Vec<&str> = FIELDS
        .iter()
        .filter(|f| f.section == "java_quirks")
        .map(|f| f.name)
        .collect();
    assert_eq!(
        table,
        JavaQuirks::IDS,
        "the table and `JavaQuirks` list the same ids"
    );
    assert_eq!(JavaQuirks::IDS.len(), SWITCH_COUNT);
    for field in FIELDS.iter().filter(|f| f.section == "java_quirks") {
        assert!(field.optional, "{} is unset by default", field.path);
        assert_eq!((field.default)(CORES), None, "{}", field.path);
        assert!(field.json, "{} is read from settings files", field.path);
        assert!(field.doc.contains("`true` (unset)"), "{}", field.path);
        let item = JavaQuirks::fix_item(field.name).expect("an owning item");
        // N06's inventory, and the K15 switches found after it.
        assert!(
            item.starts_with("N06.") || item.starts_with("K15."),
            "{}: {item}",
            field.path
        );
    }
    assert_eq!(JavaQuirks::default(), JavaQuirks::JAVA);
    assert!(JavaQuirks::JAVA.fixed_ids().is_empty());
    assert_eq!(JavaQuirks::ALL_FIXED.fixed_ids(), JavaQuirks::IDS);
    assert_eq!(JavaQuirks::JAVA.get("no_such_quirk"), None);
    assert_eq!(JavaQuirks::JAVA.with("no_such_quirk", false), None);
}

#[test]
fn legacy_keeps_every_java_bug_and_enhanced_fixes_exactly_the_promoted_set() {
    let legacy = resolved(vec![(Source::Cli, legacy_pin())]);
    assert_eq!(
        JavaQuirks::from_settings(&legacy),
        JavaQuirks::JAVA,
        "legacy"
    );
    assert!(
        Profile::Legacy
            .differences()
            .iter()
            .all(|d| !d.path.starts_with("java_quirks.")),
        "legacy sets no switch"
    );
    let unset = resolved(vec![]);
    assert_eq!(
        JavaQuirks::from_settings(&unset).fixed_ids(),
        PROMOTED,
        "enhanced (unset)"
    );
    let enhanced = resolved(vec![(
        Source::Cli,
        SettingsPatch {
            profile: Some("enhanced".into()),
            ..SettingsPatch::default()
        },
    )]);
    assert_eq!(JavaQuirks::from_settings(&enhanced), enhanced_quirks());
    // K15.2's switch is Java's in both profiles until its measurement.
    assert_eq!(JavaQuirks::fix_item("placed_shape_convex"), Some("K15.2"));
    assert!(JavaQuirks::from_settings(&enhanced).placed_shape_convex);
    assert!(JavaQuirks::from_settings(&legacy).placed_shape_convex);
    // The table holds the promoted switches, each once, as `false`.
    let mut table: Vec<&str> = ENHANCED
        .iter()
        .filter_map(|d| d.path.strip_prefix("java_quirks."))
        .collect();
    assert!(
        ENHANCED
            .iter()
            .filter(|d| d.path.starts_with("java_quirks."))
            .all(|d| d.value == "false"
                && (d.measurement.starts_with("N06.13 from N06.")
                    || (d.path == "java_quirks.edge_pin_net_crosses_outline"
                        && d.measurement.starts_with("decided 2026-10-20 (K15.19")))),
        "every promoted switch is `false` with its batch's measurement or its decision"
    );
    table.sort_unstable();
    let mut promoted = PROMOTED.to_vec();
    promoted.sort_unstable();
    assert_eq!(table, promoted);
    // An explicit `true` (here on the command line) still beats the profile.
    let cli = parse_cli_arguments(&["--router.java_quirks.ses_reader_scale=true".to_string()]);
    let settings = resolved(vec![(Source::Cli, cli.patch)]);
    assert_eq!(
        JavaQuirks::from_settings(&settings),
        enhanced_quirks().with("ses_reader_scale", true).unwrap()
    );
}

#[test]
fn each_key_round_trips_through_a_settings_file() {
    for id in JavaQuirks::IDS {
        let text = format!(r#"{{"router": {{"java_quirks": {{"{id}": false}}}}}}"#);
        let file = patch_from_config_text(&text).expect("a settings file");
        assert!(file.warnings.is_empty(), "{id}: {:?}", file.warnings);
        let settings = resolved_legacy(vec![(Source::ConfigFile, file.patch)]);
        let quirks = JavaQuirks::from_settings(&settings);
        assert_eq!(quirks.fixed_ids(), vec![*id], "{id}");
        // Its explicit `true` is Java's value again.
        let text = format!(r#"{{"router": {{"java_quirks": {{"{id}": true}}}}}}"#);
        let file = patch_from_config_text(&text).expect("a settings file");
        let settings = resolved_legacy(vec![(Source::ConfigFile, file.patch)]);
        assert_eq!(
            JavaQuirks::from_settings(&settings),
            JavaQuirks::JAVA,
            "{id}"
        );
    }
}

/// N07.4: the six switches no code read are gone. A settings file that names one changes
/// nothing, and the command line reports it as an unknown option.
#[test]
fn removed_inert_switches_are_unknown_keys() {
    for id in [
        "insert_stale_trace_object",
        "rules_autoroute_settings_reenable",
        "direction_compare_zero",
        "route_stubs_layer_test",
        "polyline_array_reversal",
        "avoid_acid_traps_disabled",
    ] {
        assert!(!JavaQuirks::IDS.contains(&id), "{id}");
        assert_eq!(JavaQuirks::JAVA.get(id), None, "{id}");
        let text = format!(r#"{{"router": {{"java_quirks": {{"{id}": false}}}}}}"#);
        let file = patch_from_config_text(&text).expect("a settings file");
        let settings = resolved_legacy(vec![(Source::ConfigFile, file.patch)]);
        assert_eq!(
            JavaQuirks::from_settings(&settings),
            JavaQuirks::JAVA,
            "{id}"
        );
        let cli = parse_cli_arguments(&[format!("--{SETTINGS_PREFIX}{id}=false")]);
        assert!(!cli.warnings.is_empty(), "{id}: no warning");
    }
}

#[test]
fn each_key_is_set_on_the_command_line_and_in_the_environment() {
    for id in JavaQuirks::IDS {
        let cli = parse_cli_arguments(&[format!("--{SETTINGS_PREFIX}{id}=false")]);
        assert!(cli.warnings.is_empty(), "{id}: {:?}", cli.warnings);
        let settings = resolved_legacy(vec![(Source::Cli, cli.patch)]);
        assert_eq!(JavaQuirks::from_settings(&settings).fixed_ids(), vec![*id]);

        let name = format!("FREEROUTING__ROUTER__JAVA_QUIRKS__{}", id.to_uppercase());
        let env = parse_environment([(name.as_str(), "false")]);
        let settings = resolved_legacy(vec![(Source::Environment, env.patch)]);
        assert_eq!(
            JavaQuirks::from_settings(&settings).fixed_ids(),
            vec![*id],
            "{name}"
        );
    }
    // The command line beats a settings file, as for every key.
    let file =
        patch_from_config_text(r#"{"router": {"java_quirks": {"ses_reader_scale": false}}}"#)
            .expect("a settings file");
    let cli = parse_cli_arguments(&["--router.java_quirks.ses_reader_scale=true".to_string()]);
    let settings = resolved_legacy(vec![
        (Source::ConfigFile, file.patch),
        (Source::Cli, cli.patch),
    ]);
    assert_eq!(JavaQuirks::from_settings(&settings), JavaQuirks::JAVA);
}

#[test]
fn the_job_carries_the_switches_to_the_board_and_the_control() {
    let bytes = fixture("Issue413-test.dsn").into_bytes();
    let mut input = JobInput::new(bytes, "Issue413-test");
    input.rules = RulesChoice::None;
    // No flag: profile `enhanced`, the promoted fixes.
    let enhanced = prepare_job(&input, &ctx()).expect("loads");
    assert_eq!(enhanced.board.java_quirks(), enhanced_quirks());
    assert_eq!(enhanced.board_settings.java_quirks(), enhanced_quirks());
    input.cli = Some(legacy_pin());
    let java = prepare_job(&input, &ctx()).expect("loads");
    assert_eq!(java.board.java_quirks(), JavaQuirks::JAVA);
    assert_eq!(java.board_settings.java_quirks(), JavaQuirks::JAVA);

    // A switch the profile leaves `true` turned off, a promoted one turned back on.
    let cli = parse_cli_arguments(&[
        "--router.java_quirks.ripup_pass_no_unset=false".to_string(),
        "--router.java_quirks.dsn_shape_bbox=true".to_string(),
    ]);
    input.cli = Some(cli.patch);
    let prepared = prepare_job(&input, &ctx()).expect("loads");
    let want = enhanced_quirks()
        .with("ripup_pass_no_unset", false)
        .and_then(|q| q.with("dsn_shape_bbox", true))
        .unwrap();
    assert_eq!(prepared.board.java_quirks(), want);
    assert_eq!(prepared.board_settings.java_quirks(), want);
    assert_eq!(JavaQuirks::from_settings(&prepared.settings), want);
    let net = (1..=prepared.board.rules.nets.max_net_number())
        .find(|n| {
            AutorouteControl::for_net_default(&prepared.board, *n, &prepared.board_settings).is_ok()
        })
        .expect("a routable net");
    let control = AutorouteControl::for_net_default(&prepared.board, net, &prepared.board_settings)
        .expect("a control");
    assert_eq!(control.quirks, want);
}

#[test]
fn the_board_keeps_its_switches_through_deep_copy_and_restore() {
    let quirks = JavaQuirks::JAVA
        .with("snapshot_drops_compensation", false)
        .and_then(|q| q.with("smooth_loses_trace", false))
        .unwrap();
    let board = read("Issue756-minimal-ok.dsn", quirks);
    assert_eq!(
        board.java_quirks(),
        quirks,
        "the reader's switches stay on the board"
    );
    assert_eq!(board.deep_copy().java_quirks(), quirks, "deep copy");
    let snapshot = board.snapshot();
    // The first restore rebuilds, the second fills the memo, the third clones it.
    for n in 0..3 {
        assert_eq!(snapshot.restore().java_quirks(), quirks, "restore {n}");
    }
    let mut changed = board.deep_copy();
    changed.set_java_quirks(JavaQuirks::ALL_FIXED);
    assert_eq!(
        changed.snapshot().restore().java_quirks(),
        JavaQuirks::ALL_FIXED
    );
    assert_eq!(
        read("Issue756-minimal-ok.dsn", JavaQuirks::JAVA).java_quirks(),
        JavaQuirks::JAVA
    );
}

/// Each switch alone survives a deep copy and the three kinds of snapshot
/// restore (rebuild, memo fill, memo clone), so no field is dropped on the way.
#[test]
fn every_switch_survives_deep_copy_and_restore() {
    let board = read("Issue756-minimal-ok.dsn", JavaQuirks::JAVA);
    for id in JavaQuirks::IDS {
        let quirks = JavaQuirks::JAVA.with(id, false).expect("a known id");
        let mut changed = board.deep_copy();
        changed.set_java_quirks(quirks);
        assert_eq!(changed.deep_copy().java_quirks(), quirks, "{id}: deep copy");
        let snapshot = changed.snapshot();
        for n in 0..3 {
            assert_eq!(
                snapshot.restore().java_quirks(),
                quirks,
                "{id}: restore {n}"
            );
        }
    }
}

/// Every switch has at least one `// PARITY(quirk, java_quirks.<id>)` marker
/// beside Java's path in the engine or geometry sources (the inert ones at the
/// Java code they would change), and every marker names a known switch.
#[test]
fn every_switch_has_a_parity_marker_at_its_site() {
    const MARKER: &str = "PARITY(quirk, java_quirks.";
    fn scan(dir: &std::path::Path, found: &mut Vec<(String, String)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .expect("a source directory")
            .map(|e| e.expect("an entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                scan(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).expect("a source file");
                for rest in text.split(MARKER).skip(1) {
                    let id: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    // `java_quirks.<id>` in the docs is the pattern, not a marker.
                    if !id.is_empty() {
                        found.push((id, path.display().to_string()));
                    }
                }
            }
        }
    }
    let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = Vec::new();
    for krate in ["freerouting/src", "freerouting-geometry/src"] {
        scan(&crates.join(krate), &mut found);
    }
    for (id, path) in &found {
        assert!(
            JavaQuirks::IDS.contains(&id.as_str()),
            "{path}: marker names unknown switch {id:?}"
        );
    }
    let missing: Vec<&str> = JavaQuirks::IDS
        .iter()
        .copied()
        .filter(|id| !found.iter().any(|(f, _)| f == id))
        .collect();
    assert!(missing.is_empty(), "switches without a marker: {missing:?}");
}

/// `fanout_pin_order_names`: Java compares the exact camelCase strings.
#[test]
fn fanout_pin_order_names_both_behaviours() {
    for (name, java, fixed) in [
        (
            "distance_to_closest_on_net",
            PinSortingOrder::PinIndex,
            PinSortingOrder::DistanceToClosestOnNet,
        ),
        (
            "surroundings_density",
            PinSortingOrder::PinIndex,
            PinSortingOrder::SurroundingsDensity,
        ),
        (
            "distanceToClosestOnNet",
            PinSortingOrder::DistanceToClosestOnNet,
            PinSortingOrder::DistanceToClosestOnNet,
        ),
        (
            "bogus",
            PinSortingOrder::PinIndex,
            PinSortingOrder::PinIndex,
        ),
    ] {
        assert_eq!(PinSortingOrder::from_name(name), java, "{name}");
        assert_eq!(PinSortingOrder::from_name_with(name, true), java, "{name}");
        assert_eq!(
            PinSortingOrder::from_name_with(name, false),
            fixed,
            "{name}"
        );
    }
}

/// `route_result_int_via_ratio`: 2 vias of 3 count 0 in Java's `int` division.
#[test]
fn route_result_via_ratio_both_behaviours() {
    let java = ItemRouteResult::new(ItemId(1), 3, 2, 100.0, 100.0, 0, 0);
    // (1 - (0 + 1) / 2) = 0.5 with the int ratio 2 / 3 = 0.
    assert_eq!(java.improvement_percentage(), 0.5);
    assert_eq!(java.with_int_via_ratio(true), java);
    let fixed = java.with_int_via_ratio(false);
    let want = (1.0 - ((2.0 / 3.0 + 1.0) / 2.0)) as f32;
    assert_eq!(fixed.improvement_percentage(), want);
    // No via before: 0 under both.
    let none = ItemRouteResult::new(ItemId(1), 0, 2, 100.0, 90.0, 0, 0);
    assert_eq!(none.with_int_via_ratio(false).improvement_percentage(), 0.0);
}

/// `stats_line_ab_length`: Java sums the distance between each line's two
/// defining points, the two end lines included; `false` sums the segments.
/// The statistics convert to millimetres, so the test compares the two sums'
/// ratio with the ratio of the raw sums.
#[test]
fn trace_length_statistics_both_behaviours() {
    let mut board = read("Issue756-minimal-ok.dsn", JavaQuirks::JAVA);
    let traces: Vec<_> = board
        .traces()
        .into_iter()
        .filter_map(|id| board.item(id).and_then(|i| i.as_trace()).cloned())
        .collect();
    let segment_sum: f64 = traces
        .iter()
        .map(|t| {
            let p = &t.polyline;
            (1..p.corner_count())
                .map(|i| p.corner_approx(i - 1).distance(p.corner_approx(i)))
                .sum::<f64>()
        })
        .sum();
    let line_sum: f64 = traces
        .iter()
        .flat_map(|t| t.polyline.lines().to_vec())
        .map(|l| l.a().to_float().distance(l.b().to_float()))
        .sum();
    assert!(segment_sum > 0.0, "the fixture has traces");
    assert!(
        (segment_sum - line_sum).abs() > 1.0,
        "the two sums differ here"
    );
    let total = |s: &BoardStatistics| {
        f64::from(s.traces.total_horizontal_length)
            + f64::from(s.traces.total_vertical_length)
            + f64::from(s.traces.total_angled_length)
    };
    let java = BoardStatistics::new(&board);
    board.set_java_quirks(
        JavaQuirks::JAVA
            .with("stats_line_ab_length", false)
            .unwrap(),
    );
    let fixed = BoardStatistics::new(&board);
    let ratio = total(&java) / total(&fixed);
    assert!(
        (ratio - line_sum / segment_sum).abs() < 1e-4,
        "statistics ratio {ratio} vs raw ratio {}",
        line_sum / segment_sum
    );
}
