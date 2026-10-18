//! C03.1 and C03.7: the board-specific cost tuning, the net-class
//! exclusions and the router's read-only settings view.
//!
//! Ports of the Java tests `Issue729TraceCostSettingsTest`,
//! `Issue885SettingsInitializationTest` (the explicit-cost test, the
//! `applyNetClassExclusions` test and, in `settings_cli`, the `-inc` parse
//! test) and `RouterSettingsMergeTest.applyBoardSpecificOptimizationsPreservesSettings`.
//! Not ported: `Issue885.reflectionUtilCopiesExplicitEmptyArray` (C02's
//! `copy_fields` tests cover it) and `RouterSettingsMergeTest.mergeLayersArray`
//! (C02, `settings_merge.rs`).

use freerouting::board::RoutingBoard;
use freerouting::board::state::Communication;
use freerouting::board::structure::{Layer, LayerStructure};
use freerouting::geometry::{IntBox, PolylineShape, TileShape};
use freerouting::rules::{BoardRules, ClearanceMatrix};
use freerouting::settings::board::{
    BoardRouterSettings, apply_board_specific, apply_board_specific_if_needed,
    apply_net_class_exclusions,
};
use freerouting::settings::{
    ResolveContext, RouterSettings, SettingsPatch, Source, defaults, parse_cli_arguments, resolve,
};

fn board_with(
    layers: Vec<Layer>,
    width: i32,
    height: i32,
    extra_class: Option<&str>,
) -> RoutingBoard {
    let layers = LayerStructure::new(layers);
    let matrix = ClearanceMatrix::default_instance(layers.count(), 10);
    let mut rules = BoardRules::new(layers.clone(), matrix);
    rules.create_default_net_class();
    if let Some(name) = extra_class {
        rules.append_named_net_class(name);
    }
    let outline = IntBox::new(0, 0, width, height);
    RoutingBoard::new(
        outline,
        layers,
        vec![PolylineShape::Tile(TileShape::from(outline))],
        0,
        rules,
        Communication::default(),
    )
}

/// The board of `Issue729TraceCostSettingsTest`: two signal layers, 2:1.
fn board_729() -> RoutingBoard {
    board_with(
        vec![Layer::new("Top", true), Layer::new("Bottom", true)],
        2_000_000,
        1_000_000,
        None,
    )
}

fn two_layer_settings() -> SettingsPatch {
    let mut s = SettingsPatch::default();
    s.set_layer_count(2);
    s
}

fn resolved(patch: &SettingsPatch) -> RouterSettings {
    RouterSettings::from_patch(patch, 8)
}

// ---- Issue729TraceCostSettingsTest -------------------------------------

#[test]
fn issue729_initializes_trace_costs_once() {
    let board = board_729();
    let mut settings = two_layer_settings();
    apply_board_specific(&mut settings, &board);

    assert_eq!(settings.costs_initialized, Some(true));
    let first = resolved(&settings);
    let (pref0, against0) = (
        first.preferred_direction_trace_costs(0),
        first.against_preferred_direction_trace_costs(0),
    );
    // The default undesired cost of a bare patch is the 1.0 fallback; a 2:1
    // board adds 0.1 * round(10 * 2) = 2.0 for a horizontal layer.
    assert!(
        against0 > 1.0,
        "aspect-ratio penalty on a 2:1 board: {against0}"
    );

    apply_board_specific(&mut settings, &board);
    let second = resolved(&settings);
    assert_eq!(second.preferred_direction_trace_costs(0), pref0);
    assert_eq!(second.against_preferred_direction_trace_costs(0), against0);
}

#[test]
fn issue729_aspect_penalty_values() {
    // w < h is false (2e6 > 1e6): layer 0 flips to "preferred horizontal"
    // = !(false) = true, so layer 0 pays the horizontal add
    // 0.1 * round(10 * 2e6 / 1e6) = 2.0; layer 1 is vertical and pays
    // 0.1 * round(10 * 1e6 / 2e6) = 0.5.
    let board = board_729();
    let mut settings = two_layer_settings();
    apply_board_specific(&mut settings, &board);
    let s = resolved(&settings);
    assert!(s.preferred_direction_is_horizontal(0));
    assert!(!s.preferred_direction_is_horizontal(1));
    assert_eq!(s.preferred_direction_trace_costs(0), 1.0);
    assert_eq!(s.against_preferred_direction_trace_costs(0), 1.0 + 2.0);
    assert_eq!(s.against_preferred_direction_trace_costs(1), 1.0 + 0.5);
}

#[test]
fn issue729_preserves_user_trace_costs_on_second_call() {
    let board = board_729();
    let mut settings = two_layer_settings();
    apply_board_specific(&mut settings, &board);

    settings.set_against_preferred_direction_trace_costs(0, 4.5);
    settings.set_against_preferred_direction_trace_costs(1, 3.2);
    settings.set_preferred_direction_trace_costs(0, 2.0);
    apply_board_specific(&mut settings, &board);

    let s = resolved(&settings);
    assert_eq!(s.against_preferred_direction_trace_costs(0), 4.5);
    assert_eq!(s.against_preferred_direction_trace_costs(1), 3.2);
    assert_eq!(s.preferred_direction_trace_costs(0), 2.0);
}

#[test]
fn issue729_if_needed_skips_when_already_board_tuned() {
    let board = board_729();
    let mut settings = two_layer_settings();
    apply_board_specific(&mut settings, &board);
    settings.set_against_preferred_direction_trace_costs(0, 5.0);
    settings.set_against_preferred_direction_trace_costs(1, 6.0);

    apply_board_specific_if_needed(&mut settings, &board);

    let s = resolved(&settings);
    assert_eq!(s.against_preferred_direction_trace_costs(0), 5.0);
    assert_eq!(s.against_preferred_direction_trace_costs(1), 6.0);
}

#[test]
fn issue729_if_needed_runs_on_layer_count_mismatch() {
    let board = board_729();
    let mut untuned = SettingsPatch::default();
    assert_eq!(untuned.layer_count(), 0);

    apply_board_specific_if_needed(&mut untuned, &board);

    assert_eq!(untuned.layer_count(), 2);
    assert_eq!(untuned.costs_initialized, Some(true));
    let s = resolved(&untuned);
    assert!(s.layer_active(0));
    assert!(s.layer_active(1));
}

#[test]
fn issue729_set_layer_count_resets_the_flag() {
    let board = board_729();
    let mut settings = two_layer_settings();
    apply_board_specific(&mut settings, &board);
    assert_eq!(settings.costs_initialized, Some(true));

    settings.set_layer_count(4);

    assert_eq!(settings.costs_initialized, Some(false));
}

// ---- Issue885SettingsInitializationTest --------------------------------

#[test]
fn issue885_preserves_explicit_layer_trace_costs() {
    let board = board_729();
    let mut settings = two_layer_settings();
    {
        let layers = settings.layers.as_mut().unwrap();
        layers[0].preferred_direction_trace_cost = Some(0.25);
        layers[0].undesired_direction_trace_cost = Some(0.50);
    }
    apply_board_specific(&mut settings, &board);

    let s = resolved(&settings);
    assert_eq!(s.preferred_direction_trace_costs(0), 0.25);
    assert_eq!(s.against_preferred_direction_trace_costs(0), 0.50);
    let layer = &settings.layers.as_ref().unwrap()[0];
    assert_eq!(layer.preferred_direction_trace_cost, Some(0.25));
    assert_eq!(layer.undesired_direction_trace_cost, Some(0.50));
    // The array entries were written from the explicit values.
    assert_eq!(
        settings
            .scoring
            .preferred_direction_trace_cost
            .as_ref()
            .unwrap()[0],
        0.25
    );
    // Layer 1 has no explicit cost, and the flag was false: it was computed.
    assert_eq!(s.against_preferred_direction_trace_costs(1), 1.5);
}

#[test]
fn issue885_apply_net_class_exclusions() {
    let mut board = board_with(
        vec![Layer::new("Top", true), Layer::new("Bottom", true)],
        2_000_000,
        1_000_000,
        Some("POWER"),
    );
    let power = board.rules.net_classes.get_by_name("POWER").expect("POWER");
    assert!(!board.rules.net_classes.get(power).is_ignored_by_autorouter);

    let mut settings = two_layer_settings();
    settings.autorouter.ignore_net_classes = Some(vec!["POWER".to_string()]);
    let warnings = apply_net_class_exclusions(&settings, &mut board);

    assert!(board.rules.net_classes.get(power).is_ignored_by_autorouter);
    assert!(warnings.is_empty());
    let default = board.rules.default_net_class_id().unwrap();
    assert!(
        !board
            .rules
            .net_classes
            .get(default)
            .is_ignored_by_autorouter
    );
}

#[test]
fn net_class_exclusions_ignore_case_blanks_and_warn_on_unknown() {
    let mut board = board_with(
        vec![Layer::new("Top", true), Layer::new("Bottom", true)],
        2_000_000,
        1_000_000,
        Some("Power"),
    );
    let mut settings = SettingsPatch::default();
    settings.autorouter.ignore_net_classes = Some(vec![
        "pOWER".to_string(),
        "  ".to_string(),
        String::new(),
        "GND".to_string(),
    ]);
    let warnings = apply_net_class_exclusions(&settings, &mut board);
    let power = board.rules.net_classes.get_by_name("Power").unwrap();
    assert!(board.rules.net_classes.get(power).is_ignored_by_autorouter);
    assert_eq!(
        warnings,
        vec!["ignore_net_classes: no net class named 'GND' on the board".to_string()]
    );
    // Unset: nothing happens.
    let mut other = board_729();
    assert!(apply_net_class_exclusions(&SettingsPatch::default(), &mut other).is_empty());
}

#[test]
fn issue885_inc_flag_parses_into_ignore_net_classes() {
    let args: Vec<String> = ["-inc", "POWER,GND"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let parsed = parse_cli_arguments(&args).patch;
    assert_eq!(
        parsed.autorouter.ignore_net_classes,
        Some(vec!["POWER".to_string(), "GND".to_string()])
    );
}

// ---- RouterSettingsMergeTest -------------------------------------------

#[test]
fn router_settings_merge_tuning_preserves_settings() {
    let board = board_with(
        vec![Layer::new("Top", true), Layer::new("Bottom", true)],
        2_000_000,
        2_000_000,
        None,
    );
    let mut settings = two_layer_settings();
    {
        let layers = settings.layers.as_mut().unwrap();
        layers[0].routable = Some(false);
        layers[1].routable = Some(true);
        layers[0].preferred_direction_horizontal = Some(true);
        layers[1].preferred_direction_horizontal = Some(false);
    }
    apply_board_specific(&mut settings, &board);

    let layers = settings.layers.as_ref().unwrap();
    assert_eq!(layers[0].routable, Some(false));
    assert_eq!(layers[1].routable, Some(true));
    assert_eq!(layers[0].preferred_direction_horizontal, Some(true));
    assert_eq!(layers[1].preferred_direction_horizontal, Some(false));
}

// ---- the tuning's rules, one by one ------------------------------------

#[test]
fn square_board_adds_one_tenth_times_ten_rounded() {
    // w == h: 0.1 * round(10) = 1.0 on both axes.
    let board = board_with(
        vec![Layer::new("Top", true), Layer::new("Bottom", true)],
        1_000_000,
        1_000_000,
        None,
    );
    let mut settings = two_layer_settings();
    apply_board_specific(&mut settings, &board);
    let s = resolved(&settings);
    assert_eq!(s.against_preferred_direction_trace_costs(0), 2.0);
    assert_eq!(s.against_preferred_direction_trace_costs(1), 2.0);
}

#[test]
fn four_signal_layers_add_the_outer_layer_costs() {
    let board = board_with(
        vec![
            Layer::new("F", true),
            Layer::new("In1", true),
            Layer::new("In2", true),
            Layer::new("B", true),
        ],
        1_000_000,
        1_000_000,
        None,
    );
    let mut settings = SettingsPatch::default();
    apply_board_specific(&mut settings, &board);
    let s = resolved(&settings);
    // outer add = 0.2 * 4 = 0.8 on both arrays at layers 0 and 3.
    assert_eq!(s.preferred_direction_trace_costs(0), 1.0 + 0.8);
    assert_eq!(s.preferred_direction_trace_costs(1), 1.0);
    assert_eq!(s.preferred_direction_trace_costs(3), 1.0 + 0.8);
    assert_eq!(
        s.against_preferred_direction_trace_costs(0),
        1.0 + 1.0 + 0.8
    );
    assert_eq!(s.against_preferred_direction_trace_costs(1), 1.0 + 1.0);
}

#[test]
fn a_plane_layer_is_unroutable_and_does_not_flip_the_direction() {
    let board = board_with(
        vec![
            Layer::new("F", true),
            Layer::new("GND", false),
            Layer::new("B", true),
        ],
        1_000_000,
        1_000_000,
        None,
    );
    let mut settings = SettingsPatch::default();
    // An explicit routable=true on a plane layer is overwritten.
    settings.set_layer_count(3);
    settings.set_layer_active(1, true);
    apply_board_specific(&mut settings, &board);
    let layers = settings.layers.as_ref().unwrap();
    assert_eq!(layers[1].routable, Some(false));
    // w < h false: F flips to true, GND keeps true, B flips to false.
    assert_eq!(layers[0].preferred_direction_horizontal, Some(true));
    assert_eq!(layers[1].preferred_direction_horizontal, Some(true));
    assert_eq!(layers[2].preferred_direction_horizontal, Some(false));
    assert_eq!(layers[0].bend_cost, Some(0.0));
}

#[test]
fn wrong_length_arrays_become_zero_arrays_and_clear_the_flag() {
    let board = board_729();
    let mut settings = two_layer_settings();
    settings.costs_initialized = Some(true);
    // One array of the wrong length: replaced by zeros, flag cleared, so the
    // tuning computes both arrays.
    settings.scoring.preferred_direction_trace_cost = Some(vec![9.0; 3]);
    apply_board_specific(&mut settings, &board);
    assert_eq!(
        settings.scoring.preferred_direction_trace_cost,
        Some(vec![1.0, 1.0])
    );
    assert_eq!(settings.costs_initialized, Some(true));

    // Wrong layer count: old elements are kept by index, the rest are empty.
    let mut settings = two_layer_settings();
    settings.layers.as_mut().unwrap()[1].bend_cost = Some(3.0);
    settings.costs_initialized = Some(true);
    let four = board_with(
        vec![
            Layer::new("a", true),
            Layer::new("b", true),
            Layer::new("c", true),
            Layer::new("d", true),
        ],
        1_000_000,
        1_000_000,
        None,
    );
    apply_board_specific(&mut settings, &four);
    assert_eq!(settings.layer_count(), 4);
    assert_eq!(settings.layers.as_ref().unwrap()[1].bend_cost, Some(3.0));
    assert_eq!(settings.layers.as_ref().unwrap()[2].bend_cost, Some(0.0));
    assert_eq!(settings.costs_initialized, Some(true), "recomputed");
    assert_eq!(
        settings
            .scoring
            .preferred_direction_trace_cost
            .as_ref()
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn a_degenerate_box_does_not_panic() {
    // h == 0: 10 * w / 0 is infinite and Math.round saturates; 0 / 0 is NaN
    // and rounds to 0. No panic, and the add is finite or infinite as in Java.
    let board = board_with(
        vec![Layer::new("Top", true), Layer::new("Bottom", true)],
        1_000,
        0,
        None,
    );
    let mut settings = two_layer_settings();
    apply_board_specific(&mut settings, &board);
    assert_eq!(settings.costs_initialized, Some(true));
}

// ---- C03.7: the router's view, each fallback --------------------------

fn view_of(patch: &SettingsPatch) -> BoardRouterSettings {
    BoardRouterSettings::from_settings(&resolved(patch))
}

#[test]
fn view_trace_cost_fallbacks() {
    let mut p = SettingsPatch::default();
    p.set_layer_count(3);
    // Layer 0: explicit layer value wins over the array entry.
    p.layers.as_mut().unwrap()[0].preferred_direction_trace_cost = Some(2.5);
    p.scoring.preferred_direction_trace_cost = Some(vec![9.0, 4.0]); // shorter than the layers
    p.scoring.undesired_direction_trace_cost = None;
    let v = view_of(&p);
    assert_eq!(v.layer_count(), 3);
    assert_eq!(
        v.preferred_direction_trace_costs(0),
        2.5,
        "explicit layer value"
    );
    assert_eq!(v.preferred_direction_trace_costs(1), 4.0, "array entry");
    assert_eq!(
        v.preferred_direction_trace_costs(2),
        1.0,
        "past the array: 1.0"
    );
    assert_eq!(
        v.against_preferred_direction_trace_costs(1),
        1.0,
        "no array: 1.0"
    );
    // getTraceCosts has the length of the preferred array, not the layers.
    assert_eq!(v.trace_costs().len(), 2);
    // Out of range layers read as Java's out-of-range results.
    assert!(!v.layer_active(3));
    assert_eq!(v.bend_cost(3), 0.0);
    assert_eq!(v.horizontal_trace_costs(3), 0.0);
    assert!(!v.preferred_direction_is_horizontal(3));
}

#[test]
fn view_direction_and_bend_fallbacks() {
    let mut p = SettingsPatch::default();
    p.set_layer_count(3);
    p.scoring.default_bend_cost = Some(12.0); // above the maximum: clamped
    p.layers.as_mut().unwrap()[1].bend_cost = Some(1.5);
    p.layers.as_mut().unwrap()[2].preferred_direction_horizontal = Some(true);
    p.layers.as_mut().unwrap()[0].routable = None;
    let v = view_of(&p);
    // Unset direction: layer % 2 == 1.
    assert!(!v.preferred_direction_is_horizontal(0));
    assert!(v.preferred_direction_is_horizontal(1));
    assert!(v.preferred_direction_is_horizontal(2), "explicit");
    assert_eq!(v.bend_cost(0), 9.9, "the clamped default");
    assert_eq!(v.bend_cost(1), 1.5);
    assert!(v.layer_active(0), "unset is routable");
    // Horizontal and vertical costs follow the direction.
    assert_eq!(
        v.horizontal_trace_costs(1),
        v.preferred_direction_trace_costs(1)
    );
    assert_eq!(
        v.vertical_trace_costs(1),
        v.against_preferred_direction_trace_costs(1)
    );
    assert_eq!(
        v.horizontal_trace_costs(0),
        v.against_preferred_direction_trace_costs(0)
    );
}

#[test]
fn view_scalar_fallbacks_and_fields() {
    let ctx = ResolveContext { cores: 8 };
    // Java's defaults are the `legacy` profile's (N05.3 sets the neck width
    // under `enhanced`, checked below).
    let all = resolve(
        &[
            (Source::Defaults, defaults(&ctx)),
            (
                Source::Testing,
                freerouting::settings::profile::legacy_pin(),
            ),
        ],
        &ctx,
    );
    let v = BoardRouterSettings::from_settings(&all.settings);
    let enhanced = resolve(&[(Source::Defaults, defaults(&ctx))], &ctx);
    assert_eq!(
        BoardRouterSettings::from_settings(&enhanced.settings).neck_width_um(),
        300.0
    );
    assert_eq!(v.via_costs(), 50);
    assert_eq!(v.plane_via_costs(), 5);
    assert_eq!(v.start_ripup_costs(), 100);
    assert!(v.vias_allowed());
    assert!(v.automatic_neckdown());
    assert_eq!(v.fanout_min_escape_length_mm(), 2.5);
    assert_eq!(v.fanout_max_escape_length_mm(), 4.5);
    assert!(v.fallback_to_board_vias());
    assert_eq!(v.trace_pull_tight_accuracy(), 500);
    assert_eq!(v.neck_width_um(), 0.0);
    assert!(!v.strict_drc());

    // A bare patch reads Java's getter fallbacks: costs at least 1, neckdown
    // off, neck width off.
    let bare = view_of(&SettingsPatch::default());
    assert_eq!(bare.via_costs(), 1);
    assert_eq!(bare.plane_via_costs(), 1);
    assert_eq!(bare.start_ripup_costs(), 1);
    assert!(!bare.automatic_neckdown());
    assert_eq!(bare.neck_width_um(), 0.0);
    assert_eq!(bare.layer_count(), 0);
    assert!(bare.trace_costs().is_empty());

    // A negative neck width is off.
    let mut p = defaults(&ctx);
    p.neck_width_um = Some(-3.0);
    assert_eq!(view_of(&p).neck_width_um(), 0.0);
    p.neck_width_um = Some(150.0);
    assert_eq!(view_of(&p).neck_width_um(), 150.0);
}

// ---- the quirk, against C01's dumps ------------------------------------

mod reference {
    use std::path::{Path, PathBuf};

    use freerouting::drc::json::JsonValue;
    use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
    use freerouting::settings::ResolveContext;

    fn repo(path: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path)
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

    /// The `(preferredCost, againstCost)` of each layer in Java's `default`
    /// scenario dump, and the same from `prepare_job`.
    type Costs = Vec<(f64, f64)>;

    fn costs(case: &str) -> (Costs, Costs, bool) {
        let dump = JsonValue::parse(
            &std::fs::read_to_string(repo(&format!(
                "fixtures/reference/cases/settings/{case}.json"
            )))
            .unwrap(),
        )
        .unwrap();
        let effective = member(member(member(&dump, "scenarios"), "default"), "effective");
        let JsonValue::Array(layers) = member(effective, "layers") else {
            panic!("layers")
        };
        let java: Vec<(f64, f64)> = layers
            .iter()
            .map(|l| {
                (
                    number(member(l, "preferredCost")),
                    number(member(l, "againstCost")),
                )
            })
            .collect();
        let bytes = std::fs::read(repo(&format!("tools/java-reference/cases/{case}.dsn"))).unwrap();
        let mut input = JobInput::new(bytes, case);
        // `explicit-cost-rules` has an adjacent `.rules` file (C04.4); the
        // other cases have none.
        input.rules = RulesChoice::Adjacent;
        input.dsn_dir = Some(repo("tools/java-reference/cases"));
        let job = prepare_job(&input, &ResolveContext { cores: 8 }).unwrap();
        let rust: Vec<(f64, f64)> = (0..job.board.layer_count())
            .map(|i| {
                (
                    job.board_settings.preferred_direction_trace_costs(i),
                    job.board_settings
                        .against_preferred_direction_trace_costs(i),
                )
            })
            .collect();
        (java, rust, job.settings.costs_initialized)
    }

    fn assert_bits_equal(java: &[(f64, f64)], rust: &[(f64, f64)]) {
        assert_eq!(java.len(), rust.len());
        for (i, (j, r)) in java.iter().zip(rust).enumerate() {
            assert_eq!(
                j.0.to_bits(),
                r.0.to_bits(),
                "layer {i} preferred: {j:?} / {r:?}"
            );
            assert_eq!(
                j.1.to_bits(),
                r.1.to_bits(),
                "layer {i} against: {j:?} / {r:?}"
            );
        }
    }

    #[test]
    fn explicit_layer_cost_suppresses_computed_costs() {
        // Without an explicit cost the tuning adds the aspect and outer-layer
        // costs: a 100 x 50 mm board with 4 signal layers.
        let (java, rust, applied) = costs("aspect-4layer");
        assert!(applied);
        assert_bits_equal(&java, &rust);
        assert_eq!(
            rust[0],
            (1.8, 3.8),
            "outer layer: 0.2 * 4 and the aspect add"
        );
        assert_eq!(rust[1], (1.0, 1.5));

        // One explicit cost on In1.Cu (preferred 2.0): the layer rule sets
        // `costs_initialized`, so no layer gets a computed cost. The other
        // layers keep the seeded 1.0/1.0, with no aspect add and no
        // outer-layer add.
        let (java, rust, applied) = costs("explicit-one-layer-cost");
        assert!(applied);
        assert_bits_equal(&java, &rust);
        assert_eq!(rust, vec![(1.0, 1.0), (2.0, 1.0), (1.0, 1.0), (1.0, 1.0)]);

        // The same rule in an adjacent rules file lands on **both** F.Cu and
        // In1.Cu (C01's observation, matched on purpose): the second merge
        // reads the rules file's layer names alone, so `In1.Cu`, the only
        // layer it names, is layer 0 there and sets F.Cu's cost, and
        // `RulesReader.read` then sets the real In1.Cu. The explicit cost
        // still suppresses the computed ones.
        let (java, rust, applied) = costs("explicit-cost-rules");
        assert!(applied);
        assert_bits_equal(&java, &rust);
        assert_eq!(rust, vec![(2.0, 1.0), (2.0, 1.0), (1.0, 1.0), (1.0, 1.0)]);
    }

    #[test]
    fn owner_config_reaches_router_view() {
        // Dan's `freerouting.json` sets `scoring.start_ripup_costs = 1500`.
        // `Issue413-test` has no rules file or DSN override, so the router's
        // view shows it. (On `Issue029-hw48na` the rules file's value wins
        // through the quirk C04.3 pins; the second half below.)
        let dump = JsonValue::parse(
            &std::fs::read_to_string(repo("fixtures/reference/settings/Issue413-test.json"))
                .unwrap(),
        )
        .unwrap();
        let java = member(
            member(
                member(member(&dump, "scenarios"), "owner-config"),
                "effective",
            ),
            "startRipupCosts",
        );
        assert_eq!(number(java), 1500.0);

        let bytes = std::fs::read(repo("fixtures/Issue413-test.dsn")).unwrap();
        let mut input = JobInput::new(bytes, "Issue413-test");
        input.rules = RulesChoice::None;
        input.config_file = Some(
            freerouting::settings::json_file::load_config_file(
                &repo("tools/java-reference/cases/owner-freerouting.json"),
                true,
            )
            .unwrap()
            .patch,
        );
        let job = prepare_job(&input, &ResolveContext { cores: 8 }).unwrap();
        assert_eq!(job.board_settings.start_ripup_costs(), 1500);
        // The other costs are the defaults.
        assert_eq!(job.board_settings.via_costs(), 50);
        assert_eq!(job.board_settings.plane_via_costs(), 5);

        // `Issue029-hw48na` has an adjacent rules file with `start_ripup_costs
        // 100`: its `autoroute_settings` override the config file, so the
        // router's view shows 100 and not 1500, as in Java's dump.
        let dump = JsonValue::parse(
            &std::fs::read_to_string(repo("fixtures/reference/settings/Issue029-hw48na.json"))
                .unwrap(),
        )
        .unwrap();
        let java = member(
            member(
                member(member(&dump, "scenarios"), "owner-config"),
                "effective",
            ),
            "startRipupCosts",
        );
        assert_eq!(number(java), 100.0);
        let bytes = std::fs::read(repo("fixtures/Issue029-hw48na.dsn")).unwrap();
        let mut input = JobInput::new(bytes, "Issue029-hw48na");
        input.rules = RulesChoice::Adjacent;
        input.dsn_dir = Some(repo("fixtures"));
        input.config_file = Some(
            freerouting::settings::json_file::load_config_file(
                &repo("tools/java-reference/cases/owner-freerouting.json"),
                true,
            )
            .unwrap()
            .patch,
        );
        let job = prepare_job(&input, &ResolveContext { cores: 8 }).unwrap();
        assert!(job.rules_path.is_some());
        assert_eq!(job.board_settings.start_ripup_costs(), 100);
        assert_eq!(job.settings.scoring.start_ripup_costs, 100);
    }
}
