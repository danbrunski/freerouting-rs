//! The board scores (B06): the legacy score with Java's `float`/`double`
//! split (B06.1) and the dispatch of the router and optimizer scores on the
//! `RouterSettings` (B06.2), at `a773a70ec`.
//!
//! The final bits of every legacy case come from
//! `tools/java-reference/probes/LegacyScoreProbe.java` run at `a773a70ec`;
//! the intermediates are worked out by hand in `f32` (and checked to give
//! Java's bits). The scores of real boards are compared with Java in
//! `tests/board_probe_scores.rs`.

use freerouting::scoring::BoardStatistics;
use freerouting::scoring::legacy::{
    legacy_costs, legacy_maximum_score, legacy_normalized_score, legacy_penalties, legacy_score,
};
use freerouting::settings::fields::ScoringSettings;
use freerouting::settings::{
    OptimizerScoringVersion, ResolveContext, RouterScoringVersion, RouterSettings, SettingsPatch,
    Source, default_patch, resolve,
};

/// Java's defaults (`DefaultSettings`, merged as `SettingsMerger` does).
fn defaults() -> RouterSettings {
    RouterSettings::from_patch(&default_patch(4), 4)
}

/// Statistics with the fields the legacy score reads.
fn stats(
    maximum: usize,
    incomplete: usize,
    violations: usize,
    bends: usize,
    vias: usize,
    length_mm: f32,
) -> BoardStatistics {
    let mut s = BoardStatistics::default();
    s.connections.maximum_count = maximum;
    s.connections.incomplete_count = incomplete;
    s.clearance_violations.total_count = violations;
    s.bends.total_count = bends;
    s.vias.total_count = vias;
    s.traces.total_length_mm = length_mm;
    s
}

/// The cost settings of a case: unrouted, clearance and bend penalties, via
/// costs and the preferred-direction trace cost.
fn costs(unrouted: f32, clearance: f32, bend: f32, via: i32, trace: f64) -> ScoringSettings {
    let mut c = defaults().scoring;
    c.unrouted_net_penalty = unrouted;
    c.clearance_violation_penalty = clearance;
    c.bend_penalty = bend;
    c.via_costs = via;
    c.default_preferred_direction_trace_cost = trace;
    c
}

fn default_costs() -> ScoringSettings {
    costs(5_000_000.0, 1_000_000.0, 10.0, 50, 1.0)
}

/// `(case, getMaximumScore, calculateScore, getRouterScore(RoutingCostSettings))`
/// as raw `float` bits, from `LegacyScoreProbe.java` at `a773a70ec`.
const JAVA_LEGACY: [(&str, u32, u32, u32); 9] = [
    ("defaults", 0x4c3ebc20, 0x4c14c59a, 0x4442ff8f),
    ("split", 0x4a989680, 0xcb33b4c2, 0x00000000),
    ("fraction", 0x4d306e04, 0x4d1895ba, 0x44583641),
    ("doubled", 0x4d306e04, 0x4d1894f4, 0x44583529),
    ("negative", 0x4a989680, 0xc9f42b82, 0x00000000),
    ("via-wrap", 0x4b64e1c0, 0x4ed7c3d2, 0x47ebac3f),
    ("no-connections", 0x00000000, 0xc2960000, 0x00000000),
    ("zero-penalty", 0x00000000, 0xc2960000, 0x00000000),
    ("negative-penalty", 0xc0400000, 0xc29a0000, 0x00000000),
];

/// The probe's cases, in its order.
fn legacy_case(name: &str) -> (BoardStatistics, ScoringSettings) {
    let d = default_costs();
    match name {
        "defaults" => (stats(10, 2, 1, 7, 3, 123.5), d),
        "split" => (
            stats(1, 0, 0, 0, 1, 0.1),
            costs(5_000_000.0, 1_000_000.0, 10.0, 16_777_217, 0.3),
        ),
        "fraction" => (stats(37, 5, 0, 113, 21, 987.654), d),
        "doubled" => (
            stats(37, 5, 0, 113, 21, 987.654),
            costs(5_000_000.0, 1_000_000.0, 20.0, 100, 2.0),
        ),
        "negative" => (stats(1, 1, 2, 3, 4, 10.25), d),
        "via-wrap" => (stats(3, 0, 0, 0, 50_000_000, 0.0), d),
        "no-connections" => (stats(0, 0, 0, 2, 1, 5.0), d),
        "zero-penalty" => (
            stats(4, 1, 0, 2, 1, 5.0),
            costs(0.0, 1_000_000.0, 10.0, 50, 1.0),
        ),
        "negative-penalty" => (
            stats(3, 1, 0, 2, 1, 5.0),
            costs(-1.0, 1_000_000.0, 10.0, 50, 1.0),
        ),
        _ => unreachable!("{name}"),
    }
}

#[test]
fn legacy_scores_match_java_bits() {
    for (name, maximum, score, normalized) in JAVA_LEGACY {
        let (s, c) = legacy_case(name);
        assert_eq!(
            legacy_maximum_score(&s, &c).to_bits(),
            maximum,
            "{name}: maximum"
        );
        assert_eq!(legacy_score(&s, &c).to_bits(), score, "{name}: score");
        assert_eq!(
            legacy_normalized_score(&s, &c).to_bits(),
            normalized,
            "{name}: normalized"
        );
        // The `BoardStatistics` methods are the same functions.
        assert_eq!(s.legacy_score(&c).to_bits(), score, "{name}");
        assert_eq!(s.legacy_maximum_score(&c).to_bits(), maximum, "{name}");
        assert_eq!(
            s.legacy_normalized_score(&c).to_bits(),
            normalized,
            "{name}"
        );
    }
}

/// Hand case 1, Java's default costs: 10 connections, 2 open, 1 violation,
/// 7 bends, 3 vias, 123.5 mm.
#[test]
fn legacy_hand_case_defaults() {
    let (s, c) = legacy_case("defaults");
    // 10 * 5e6, exact in f32.
    assert_eq!(legacy_maximum_score(&s, &c), 50_000_000.0);
    // 2 * 5e6 + 1 * 1e6 + 7 * 10, all exact.
    assert_eq!(legacy_penalties(&s, &c), 11_000_070.0);
    // 123.5 * 1.0 + 3 * 50 in double, exact.
    assert_eq!(legacy_costs(&s, &c), 273.5);
    // 5e7 - 11 000 070 = 38 999 930 is a tie between the f32 neighbours
    // 38 999 928 and 38 999 932 (spacing 4) and rounds to the even one;
    // minus 273.5 gives 38 999 654.5, which rounds to 38 999 656.
    assert_eq!(50_000_000.0f32 - 11_000_070.0, 38_999_928.0);
    assert_eq!(legacy_score(&s, &c), 38_999_656.0);
    // 38 999 656 / 5e7 = 0.779 993 12 (f32 0.7799931168556213), times 1000.
    assert_eq!(38_999_656.0f32 / 50_000_000.0, 0.779_993_1);
    assert_eq!(legacy_normalized_score(&s, &c), 779.993_1);
}

/// Hand case 2: 37 connections, 5 open, 113 bends, 21 vias, 987.654 mm.
#[test]
fn legacy_hand_case_fraction() {
    let (s, c) = legacy_case("fraction");
    // 37 * 5e6 = 185 000 000, a multiple of the spacing 16.
    assert_eq!(legacy_maximum_score(&s, &c), 185_000_000.0);
    // 5 * 5e6 + 0 + 113 * 10 = 25 001 130 (spacing 2), exact.
    assert_eq!(legacy_penalties(&s, &c), 25_001_130.0);
    // f32 987.654 is 987.6539916992188; plus 21 * 50 = 1050 in double is
    // 2037.6539916992188, which is an f32 (2037.654052734375 after the cast).
    assert_eq!(legacy_costs(&s, &c), 2_037.654);
    // 185 000 000 - 25 001 130 = 159 998 870 rounds to 159 998 864;
    // minus 2037.654 gives 159 996 826.3, which rounds to 159 996 832.
    assert_eq!(185_000_000.0f32 - 25_001_130.0, 159_998_864.0);
    assert_eq!(legacy_score(&s, &c), 159_996_832.0);
    // 159 996 832 / 185 000 000 = 0.864 847 7, times 1000.
    assert_eq!(legacy_normalized_score(&s, &c), 864.847_7);
}

/// Hand case 3: case 2 with the bend penalty, the via costs and the trace
/// cost doubled.
#[test]
fn legacy_hand_case_doubled_costs() {
    let (s, c) = legacy_case("doubled");
    assert_eq!(legacy_maximum_score(&s, &c), 185_000_000.0);
    // 25 000 000 + 113 * 20 = 25 002 260, exact.
    assert_eq!(legacy_penalties(&s, &c), 25_002_260.0);
    // 987.6539916992188 * 2.0 + 21 * 100 = 4075.3079833984375, an f32.
    assert_eq!(legacy_costs(&s, &c), 4_075.308);
    // 185 000 000 - 25 002 260 = 159 997 740 rounds to 159 997 744; minus
    // 4075.308 gives 159 993 668.7, which rounds to 159 993 664.
    assert_eq!(legacy_score(&s, &c), 159_993_664.0);
    assert_eq!(legacy_normalized_score(&s, &c), 864.830_6);
}

/// The costs are a `double` sum cast to `float`. In `float` the via term
/// 16 777 217 would round to 16 777 216 (a tie, to even) before the length
/// term is added, and the sum would stay there; in `double` the sum is
/// 16 777 217.03, which rounds up to 16 777 218.
#[test]
fn legacy_costs_are_summed_in_double() {
    let (s, c) = legacy_case("split");
    let all_float = 0.1f32 * 0.3f32 + 16_777_217i32 as f32;
    assert_eq!(all_float, 16_777_216.0);
    assert_eq!(legacy_costs(&s, &c), 16_777_218.0);
    assert_eq!(legacy_score(&s, &c), -11_777_218.0);
    assert_ne!(5_000_000.0f32 - all_float, legacy_score(&s, &c));
    // The score is negative, so the normalized score clamps to zero.
    assert_eq!(legacy_normalized_score(&s, &c).to_bits(), 0);
}

/// The via term is an `int` product: 50 000 000 * 50 wraps to
/// -1 794 967 296 as in Java, so the costs turn negative.
#[test]
fn legacy_via_costs_wrap_as_java_int() {
    let (s, c) = legacy_case("via-wrap");
    assert_eq!(legacy_costs(&s, &c), -1_794_967_296.0);
    assert_eq!(legacy_normalized_score(&s, &c), 120_664.49);
}

/// `getLegacyNormalizedScore` returns 0 when the maximum score is not
/// positive: no connections, a zero or a negative unrouted penalty.
#[test]
fn legacy_normalized_score_guards_non_positive_maximum() {
    for name in ["no-connections", "zero-penalty", "negative-penalty"] {
        let (s, c) = legacy_case(name);
        assert!(legacy_maximum_score(&s, &c) <= 0.0, "{name}");
        assert_eq!(legacy_normalized_score(&s, &c).to_bits(), 0, "{name}");
    }
    // Without the guard, -50 / -3 would give a positive normalized score.
    let (s, c) = legacy_case("negative-penalty");
    assert!(legacy_score(&s, &c) / legacy_maximum_score(&s, &c) > 0.0);
}

/// Statistics for the dispatch tests: half the connections open, so every
/// formula gives a different value. The V2 optimizer penalties are 100
/// (length), 400 (vias) and 200 (bends), so its score is 300.
fn dispatch_stats() -> BoardStatistics {
    let mut s = stats(40, 20, 3, 25, 12, 110.0);
    s.clearance_violations.total_violation_um = 120.0;
    s.difficulty.difficulty_d = 50.0;
    s.bounds.min_trace_length_mm = 100.0;
    s.bounds.min_via_count = 2;
    s.bounds.min_bend_count = 5;
    s
}

fn with_versions(
    router: RouterScoringVersion,
    optimizer: OptimizerScoringVersion,
) -> RouterSettings {
    let mut s = defaults();
    s.router_scoring.version = router;
    s.optimizer_scoring.version = optimizer;
    s
}

#[test]
fn defaults_select_the_v2_scores() {
    let d = defaults();
    assert_eq!(d.router_scoring.version, RouterScoringVersion::V2Continuous);
    assert_eq!(
        d.optimizer_scoring.version,
        OptimizerScoringVersion::V2LowerBound
    );
    let s = dispatch_stats();
    let router = s.router_score(Some(&d));
    let optimizer = s.optimizer_score(Some(&d));
    assert_eq!(router, s.router_score_v2(&d.router_scoring.score_weights()));
    assert_eq!(router, s.router_score_v2_default());
    assert_eq!(
        optimizer,
        s.optimizer_score_v2(&d.optimizer_scoring.score_weights())
    );
    assert_eq!(optimizer, s.optimizer_score_v2_default());
    assert_ne!(router, s.legacy_normalized_score(&d.scoring));
}

/// `None` is Java's `null`: the legacy score with the default costs, for
/// both scores.
#[test]
fn no_settings_select_the_legacy_score_with_default_costs() {
    let s = dispatch_stats();
    let legacy = s.legacy_normalized_score(&defaults().scoring);
    assert_eq!(s.router_score(None), legacy);
    assert_eq!(s.optimizer_score(None), legacy);
    assert_eq!(s.optimizer_ranking_score(None), legacy);
    assert!(legacy > 0.0 && legacy < 1000.0);
}

/// Version `V1_LEGACY` selects the legacy score with the settings' own
/// costs, not the defaults.
#[test]
fn v1_legacy_uses_the_settings_costs() {
    let s = dispatch_stats();
    let mut settings = with_versions(
        RouterScoringVersion::V1Legacy,
        OptimizerScoringVersion::V1Legacy,
    );
    settings.scoring.bend_penalty = 50_000.0;
    settings.scoring.via_costs = 100_000;
    let legacy = s.legacy_normalized_score(&settings.scoring);
    assert_eq!(s.router_score(Some(&settings)), legacy);
    assert_eq!(s.optimizer_score(Some(&settings)), legacy);
    assert_eq!(s.optimizer_ranking_score(Some(&settings)), legacy);
    assert_ne!(legacy, s.legacy_normalized_score(&defaults().scoring));
}

/// The two versions are read separately: each score falls back to legacy
/// only for its own version.
#[test]
fn router_and_optimizer_versions_dispatch_separately() {
    let s = dispatch_stats();
    let d = defaults();
    let legacy = s.legacy_normalized_score(&d.scoring);
    let router_v1 = with_versions(
        RouterScoringVersion::V1Legacy,
        OptimizerScoringVersion::V2LowerBound,
    );
    assert_eq!(s.router_score(Some(&router_v1)), legacy);
    assert_eq!(
        s.optimizer_score(Some(&router_v1)),
        s.optimizer_score_v2_default()
    );
    let optimizer_v1 = with_versions(
        RouterScoringVersion::V2Continuous,
        OptimizerScoringVersion::V1Legacy,
    );
    assert_eq!(
        s.router_score(Some(&optimizer_v1)),
        s.router_score_v2_default()
    );
    assert_eq!(s.optimizer_score(Some(&optimizer_v1)), legacy);
}

/// The V2 branches read the settings' weights.
#[test]
fn v2_branches_read_the_settings_weights() {
    let s = dispatch_stats();
    let mut settings = defaults();
    settings.router_scoring.clearance_violation_count_weight = 50.0;
    settings.optimizer_scoring.excess_via_weight = 3000.0;
    assert_ne!(s.router_score(Some(&settings)), s.router_score_v2_default());
    assert_eq!(
        s.router_score(Some(&settings)),
        s.router_score_v2(&settings.router_scoring.score_weights())
    );
    assert_ne!(
        s.optimizer_score(Some(&settings)),
        s.optimizer_score_v2_default()
    );
    assert_eq!(
        s.optimizer_score(Some(&settings)),
        s.optimizer_score_v2(&settings.optimizer_scoring.score_weights())
    );
}

/// `getOptimizerRankingScore` (`a773a70ec`): the V2 score without the
/// clamp at zero, so two boards whose penalties both pass 1000 still rank.
#[test]
fn optimizer_ranking_score_is_the_unclamped_v2_score() {
    let d = defaults();
    let mut worse = dispatch_stats();
    worse.vias.total_count = 500;
    let mut bad = dispatch_stats();
    bad.vias.total_count = 400;
    assert_eq!(worse.optimizer_score(Some(&d)), 0.0);
    assert_eq!(bad.optimizer_score(Some(&d)), 0.0);
    let (worse_rank, bad_rank) = (
        worse.optimizer_ranking_score(Some(&d)),
        bad.optimizer_ranking_score(Some(&d)),
    );
    assert!(worse_rank < bad_rank && bad_rank < 0.0);
    let weights = d.optimizer_scoring.score_weights();
    assert_eq!(
        worse_rank,
        worse.optimizer_score_v2_unclamped(&weights) as f32
    );
    // Above zero the two agree.
    let fine = dispatch_stats();
    assert_eq!(
        fine.optimizer_ranking_score(Some(&d)),
        fine.optimizer_score(Some(&d))
    );
}

/// The resolved `RouterSettings` (C02) has no missing section: a source
/// that sets nothing under `routerScoring` resolves to Java's
/// `DefaultSettings` section, as `SettingsMerger` merges the defaults
/// first. Java's `null` settings are `None` here.
#[test]
fn settings_without_scoring_sections_resolve_to_the_v2_defaults() {
    let ctx = ResolveContext { cores: 4 };
    let sources = [
        (Source::Defaults, default_patch(4)),
        (Source::Cli, SettingsPatch::default()),
    ];
    let settings = resolve(&sources, &ctx).settings;
    assert_eq!(settings.router_scoring, defaults().router_scoring);
    assert_eq!(settings.optimizer_scoring, defaults().optimizer_scoring);
    let s = dispatch_stats();
    assert_eq!(s.router_score(Some(&settings)), s.router_score_v2_default());
    assert_eq!(
        s.optimizer_score(Some(&settings)),
        s.optimizer_score_v2_default()
    );
}
