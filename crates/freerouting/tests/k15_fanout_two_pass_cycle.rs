//! K15.50: `router.java_quirks.fanout_two_pass_cycle`, the fanout pass loop's stop
//! on a cycle of two pass outcomes.
//!
//! Java's loop (`BatchFanout.fanoutBoard` at `a773a70ec`, lines 113-164) stops on an
//! outcome `((long) routed << 32) ^ vias` equal to the previous pass's three times
//! running, or on an unchanged board hash; a fanout whose passes alternate between
//! two outcomes runs to `fanout.max_passes`. With the switch `false`,
//! [`FanoutPassTracker`] also stops after a pass whose outcome equals the one two
//! passes before while the pass in between differs.
//!
//! The scripted passes are `(routed, vias)` per pass, each with a new board hash (a
//! pass that rips and routes again changes the hash: new item ids, a deeper undo
//! stack; no hash repeated in K15.50's 13 diagnostic runs), so only the outcome
//! rules can stop the loop.

use freerouting::autoroute::batch::fanout_rules::{FanoutPassTracker, FanoutStop};
use freerouting::settings::profile::legacy_pin;
use freerouting::settings::{
    JavaQuirks, ResolveContext, RouterSettings, SettingsPatch, Source, default_patch,
    parse_cli_arguments, resolve,
};

const CORES: usize = 8;

/// Java's default `fanout.max_passes`.
const MAX_PASSES: usize = 20;

/// canary-pwr's `pcb-on` input (K15.48's arm, session `cf00bc73`): the outcome
/// `(routed, vias)` of each of its 20 fanout passes, measured with K15.50's
/// diagnostic build. From pass 13 the passes alternate between two outcomes, so
/// Java's loop runs to the cap; passes 9-11 are the first A B A.
const CANARY_PCB_ON: [(i32, usize); MAX_PASSES] = [
    (287, 103),
    (91, 117),
    (66, 123),
    (51, 127),
    (33, 133),
    (34, 126),
    (38, 130),
    (33, 127),
    (40, 130),
    (35, 128),
    (40, 130),
    (37, 127),
    (35, 131),
    (31, 128),
    (35, 131),
    (31, 128),
    (35, 131),
    (31, 128),
    (35, 131),
    (31, 128),
];

/// A cycle of two outcomes `A` and `B` (canary-pwr's from pass 13).
const A: (i32, usize) = (35, 131);
const B: (i32, usize) = (31, 128);

/// The driver of `FanoutRun::run`: at most `MAX_PASSES` passes, each asked of the
/// tracker; returns the passes run and the stop.
fn run(tracker: &mut FanoutPassTracker, passes: &[(i32, usize)]) -> (usize, Option<FanoutStop>) {
    for (i, &(routed, vias)) in passes.iter().take(MAX_PASSES).enumerate() {
        let hash = 1000 + i as u128;
        if let Some(stop) = tracker.after_pass(routed, vias, false, hash) {
            return (i + 1, Some(stop));
        }
    }
    (passes.len().min(MAX_PASSES), None)
}

fn resolved(sources: Vec<(Source, SettingsPatch)>) -> RouterSettings {
    let mut all = vec![(Source::Defaults, default_patch(CORES))];
    all.extend(sources);
    resolve(&all, &ResolveContext { cores: CORES }).settings
}

/// The switch `false`: the loop ends after pass 11, the first pass whose outcome
/// (40 routed, 130 vias) is the one of two passes before (pass 9) while pass 10's
/// differs (35, 128).
#[test]
fn the_switch_false_stops_after_the_cycles_first_repeat() {
    assert_eq!(&CANARY_PCB_ON[8..11], &[(40, 130), (35, 128), (40, 130)]);
    let mut tracker = FanoutPassTracker::with_quirk(1, false);
    assert_eq!(
        run(&mut tracker, &CANARY_PCB_ON),
        (11, Some(FanoutStop::TwoPassCycle))
    );
    // From pass 13 on alone, the cycle A B A ends after its third pass.
    let mut tracker = FanoutPassTracker::with_quirk(1, false);
    assert_eq!(
        run(&mut tracker, &CANARY_PCB_ON[12..]),
        (3, Some(FanoutStop::TwoPassCycle))
    );
}

/// The switch unset (`true`, Java's loop): the same passes run to the 20-pass cap,
/// as Java's and K15.48's (`passes_completed` 20), since no outcome equals the
/// previous one.
#[test]
fn the_switch_unset_runs_javas_loop_to_the_cap() {
    let quirk = JavaQuirks::default().fanout_two_pass_cycle;
    assert!(quirk, "unset is Java's loop");
    let mut tracker = FanoutPassTracker::with_quirk(1, quirk);
    assert_eq!(run(&mut tracker, &CANARY_PCB_ON), (MAX_PASSES, None));
    let mut tracker = FanoutPassTracker::new(1);
    assert_eq!(run(&mut tracker, &CANARY_PCB_ON), (MAX_PASSES, None));
}

/// With the switch `false` Java's own stops come first and are unchanged: a
/// repeated outcome is stagnation, not a cycle, and three repeats stop the loop
/// after the fourth pass as in Java; progress that comes back to an outcome only
/// after a third one is no cycle.
#[test]
fn the_switch_false_keeps_javas_stops() {
    for quirk in [true, false] {
        let mut tracker = FanoutPassTracker::with_quirk(1, quirk);
        let passes = [(9, 4), A, A, A, A, B];
        assert_eq!(
            run(&mut tracker, &passes),
            (5, Some(FanoutStop::NoProgress)),
            "quirk {quirk}"
        );
        let mut tracker = FanoutPassTracker::with_quirk(1, quirk);
        let passes = [A, B, (30, 62), A, B, (30, 62), (0, 62)];
        assert_eq!(
            run(&mut tracker, &passes),
            (7, Some(FanoutStop::NoPinRouted)),
            "quirk {quirk}: a cycle of three outcomes is not stopped"
        );
    }
    // The same routed count with another via count is another outcome.
    let mut tracker = FanoutPassTracker::with_quirk(1, false);
    let passes = [(32, 61), (28, 63), (32, 62), (28, 64), (0, 64)];
    assert_eq!(
        run(&mut tracker, &passes),
        (5, Some(FanoutStop::NoPinRouted))
    );
}

/// The setting: profile `enhanced` sets it `false` (K15.51's measurement kept it),
/// `legacy` leaves it unset (Java's loop), and `false` read from the command line
/// applies under either.
#[test]
fn enhanced_sets_the_switch_false_legacy_leaves_it_unset_and_false_parses() {
    let id = "fanout_two_pass_cycle";
    assert_eq!(JavaQuirks::fix_item(id), Some("K15.50"));
    let legacy = resolved(vec![(Source::Cli, legacy_pin())]);
    let enhanced = resolved(vec![]);
    assert_eq!(legacy.java_quirks.fanout_two_pass_cycle, None, "legacy");
    assert!(JavaQuirks::from_settings(&legacy).fanout_two_pass_cycle);
    assert_eq!(
        enhanced.java_quirks.fanout_two_pass_cycle,
        Some(false),
        "enhanced"
    );
    assert!(!JavaQuirks::from_settings(&enhanced).fanout_two_pass_cycle);
    let cli = parse_cli_arguments(&[format!("--router.java_quirks.{id}=false")]);
    assert!(cli.warnings.is_empty(), "{:?}", cli.warnings);
    for profile in [None, Some(legacy_pin())] {
        let mut sources = vec![(Source::Cli, cli.patch.clone())];
        if let Some(pin) = profile.clone() {
            sources.push((Source::Testing, pin));
        }
        let settings = resolved(sources);
        assert_eq!(settings.java_quirks.fanout_two_pass_cycle, Some(false));
        assert!(
            JavaQuirks::from_settings(&settings)
                .fixed_ids()
                .contains(&id)
        );
        assert!(!JavaQuirks::from_settings(&settings).fanout_two_pass_cycle);
    }
}
