//! N01.1: the `router.profile` key and the profile table.
//!
//! - under `legacy` every default equals Java's (today's) default, key by key;
//! - an explicit key from any source beats the profile;
//! - an unknown profile is a settings error that names the valid values;
//! - every entry of the `enhanced` table is a valid setting and has its
//!   DIVERGENCES line (the "Profile `enhanced`" section, empty until N05).

use freerouting::job::prepare::{JobError, JobInput, RulesChoice, prepare_job};
use freerouting::settings::env::parse_environment;
use freerouting::settings::json_file::patch_from_config_text;
use freerouting::settings::profile::{
    self, Difference, ENHANCED, legacy_pin, require_legacy, with_enhanced_differences,
};
use freerouting::settings::{
    FIELDS, FieldValue, Profile, ResolveContext, Resolved, SettingsPatch, Source, default_patch,
    field_by_path, parse_router_flags, resolve,
};

const CORES: usize = 8;

fn ctx() -> ResolveContext {
    ResolveContext { cores: CORES }
}

/// A test-only difference of `enhanced`: the fanout's pass limit.
const DUMMY: &[Difference] = &[Difference {
    path: "fanout.max_passes",
    value: "7",
    measurement: "test-only dummy (N01.2)",
}];

fn with_profile(name: &str) -> SettingsPatch {
    SettingsPatch {
        profile: Some(name.to_string()),
        ..SettingsPatch::default()
    }
}

fn resolve_with(sources: Vec<(Source, SettingsPatch)>) -> Resolved {
    let mut all = vec![(Source::Defaults, default_patch(CORES))];
    all.extend(sources);
    resolve(&all, &ctx())
}

fn value(patch: &SettingsPatch, path: &str) -> Option<FieldValue> {
    (field_by_path(path).expect("a settings path").get)(patch)
}

/// Every field but `profile` of `patch` equals Java's default.
fn assert_java_defaults(patch: &SettingsPatch, what: &str) {
    let java = default_patch(CORES);
    for field in FIELDS.iter().filter(|f| f.path != "profile") {
        assert_eq!(
            (field.get)(patch),
            (field.get)(&java),
            "{what}: {} differs from Java's default",
            field.path
        );
    }
    assert_eq!(patch.layers, java.layers, "{what}: layers");
}

#[test]
fn legacy_defaults_equal_todays_defaults_key_by_key() {
    let legacy = resolve_with(vec![(Source::Cli, legacy_pin())]);
    assert_eq!(legacy.profile, Ok(Profile::Legacy));
    assert_java_defaults(&legacy.patch, "legacy");
    assert_java_defaults(&Profile::Legacy.defaults(CORES), "Profile::Legacy.defaults");
    assert_eq!(Profile::Legacy.patch(), SettingsPatch::default());
    // With a difference in `enhanced`, `legacy` still is Java's.
    with_enhanced_differences(DUMMY, || {
        let legacy = resolve_with(vec![(Source::Cli, legacy_pin())]);
        assert_java_defaults(&legacy.patch, "legacy beside a dummy enhanced");
        assert!(
            !legacy
                .provenance
                .values()
                .any(|s| matches!(s, Source::Profile(_))),
            "legacy sets nothing"
        );
    });
}

#[test]
fn enhanced_is_the_default_and_starts_equal_to_legacy() {
    let unset = resolve_with(vec![]);
    assert_eq!(unset.profile, Ok(Profile::Enhanced));
    assert_eq!(unset.patch.profile, None, "the default is not written");
    // N05 adds the first difference; until then `enhanced` is `legacy`.
    assert!(ENHANCED.is_empty());
    assert_java_defaults(&unset.patch, "enhanced (empty table)");
    assert_java_defaults(
        &Profile::Enhanced.defaults(CORES),
        "Profile::Enhanced.defaults",
    );
}

#[test]
fn a_profile_difference_is_applied_after_the_defaults() {
    with_enhanced_differences(DUMMY, || {
        let enhanced = resolve_with(vec![]);
        assert_eq!(
            value(&enhanced.patch, "fanout.max_passes"),
            Some(FieldValue::Int(7))
        );
        assert_eq!(
            enhanced.provenance.get("fanout.max_passes"),
            Some(&Source::Profile(Profile::Enhanced))
        );
        assert_eq!(
            value(&Profile::Enhanced.defaults(CORES), "fanout.max_passes"),
            Some(FieldValue::Int(7))
        );
        // Every other key is Java's.
        let java = default_patch(CORES);
        for field in FIELDS.iter().filter(|f| f.path != "fanout.max_passes") {
            assert_eq!(
                (field.get)(&enhanced.patch),
                (field.get)(&java),
                "{}",
                field.path
            );
        }
        assert_eq!(Source::Profile(Profile::Enhanced).priority(), 5);
    });
    // The override ends with the closure.
    assert_eq!(Profile::Enhanced.differences(), ENHANCED);
}

#[test]
fn an_explicit_key_beats_the_profile() {
    with_enhanced_differences(DUMMY, || {
        let mut three = SettingsPatch::default();
        three.fanout.max_passes = Some(3);
        for source in [
            Source::ConfigFile,
            Source::Dsn,
            Source::RulesFile,
            Source::Environment,
            Source::Cli,
            Source::Testing,
        ] {
            let resolved = resolve_with(vec![(source.clone(), three.clone())]);
            assert_eq!(
                value(&resolved.patch, "fanout.max_passes"),
                Some(FieldValue::Int(3)),
                "{source:?} beats the profile"
            );
            assert_eq!(resolved.provenance.get("fanout.max_passes"), Some(&source));
        }
        // A settings file that sets the key routes the same under both.
        let legacy = resolve_with(vec![
            (Source::ConfigFile, three.clone()),
            (Source::Cli, legacy_pin()),
        ]);
        let enhanced = resolve_with(vec![(Source::ConfigFile, three.clone())]);
        assert_eq!(
            value(&legacy.patch, "fanout.max_passes"),
            value(&enhanced.patch, "fanout.max_passes")
        );
    });
}

#[test]
fn the_highest_priority_source_names_the_profile() {
    let resolved = resolve_with(vec![
        (Source::Cli, with_profile("enhanced")),
        (Source::ConfigFile, with_profile("legacy")),
    ]);
    assert_eq!(resolved.profile, Ok(Profile::Enhanced));
    assert_eq!(resolved.patch.profile.as_deref(), Some("enhanced"));
    // Equal priorities: the later one, as the merge's stable sort.
    let resolved = resolve_with(vec![
        (Source::Cli, with_profile("enhanced")),
        (Source::Cli, with_profile("legacy")),
    ]);
    assert_eq!(resolved.profile, Ok(Profile::Legacy));
    assert_eq!(resolved.patch.profile.as_deref(), Some("legacy"));
}

#[test]
fn every_interface_spells_the_key() {
    // `--router.profile=`, `--profile <name>` and `--profile=<name>`.
    for argv in [
        vec!["--router.profile=legacy"],
        vec!["--profile", "legacy"],
        vec!["--profile=Legacy"],
    ] {
        let argv: Vec<String> = argv.into_iter().map(String::from).collect();
        let flags = parse_router_flags(&argv);
        assert!(
            flags.usage_errors.is_empty(),
            "{argv:?}: {:?}",
            flags.usage_errors
        );
        let patch = flags.parse.patch;
        assert_eq!(Profile::of_patch(&patch), Ok(Profile::Legacy), "{argv:?}");
    }
    let flags = parse_router_flags(&["--profile".to_string(), "best".to_string()]);
    assert_eq!(
        flags.usage_errors,
        vec![
            "--profile: router.profile: unknown profile 'best' (valid profiles: legacy, enhanced)"
        ]
    );
    // `FREEROUTING__ROUTER__PROFILE`.
    let env = parse_environment([("FREEROUTING__ROUTER__PROFILE", "legacy")]);
    assert_eq!(env.patch.profile.as_deref(), Some("legacy"));
    // A settings file's `router.profile`.
    let json = patch_from_config_text(r#"{"router": {"profile": "legacy"}}"#).expect("JSON");
    assert_eq!(json.patch.profile.as_deref(), Some("legacy"));
}

#[test]
fn an_unknown_profile_fails_with_the_valid_values() {
    let message = "router.profile: unknown profile 'best' (valid profiles: legacy, enhanced)";
    let e = Profile::parse("best").unwrap_err();
    assert_eq!(e.to_string(), message);
    assert_eq!(Profile::parse(" LEGACY "), Ok(Profile::Legacy));

    let resolved = resolve_with(vec![(Source::ConfigFile, with_profile("best"))]);
    assert_eq!(
        resolved.profile.as_ref().map_err(ToString::to_string),
        Err(message.to_string())
    );
    assert!(resolved.warnings.iter().any(|w| w == message));

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes = std::fs::read(root.join("fixtures/Issue756-minimal-ok.dsn")).expect("fixture");
    let mut input = JobInput::new(bytes, "Issue756-minimal-ok");
    input.rules = RulesChoice::None;
    input.cli = Some(with_profile("best"));
    match prepare_job(&input, &ctx()) {
        Err(JobError::Settings(text)) => assert_eq!(text, message),
        other => panic!("expected a settings error, got {:?}", other.map(|_| ())),
    }
    input.cli = Some(legacy_pin());
    let prepared = prepare_job(&input, &ctx()).expect("legacy loads");
    assert_eq!(require_legacy(&prepared.settings), Ok(()));
}

#[test]
fn require_legacy_fails_loudly_without_the_pin() {
    let unpinned = resolve_with(vec![]).settings;
    assert_eq!(
        require_legacy(&unpinned),
        Err(
            "a parity run must set router.profile = legacy explicitly; it has no profile \
             (the default, enhanced)"
                .to_string()
        )
    );
    let enhanced = resolve_with(vec![(Source::Cli, with_profile("enhanced"))]).settings;
    assert!(
        require_legacy(&enhanced)
            .unwrap_err()
            .ends_with("it has 'enhanced'")
    );
    let legacy = resolve_with(vec![(Source::Testing, legacy_pin())]).settings;
    assert_eq!(require_legacy(&legacy), Ok(()));
}

/// The "Profile `enhanced`" section of DIVERGENCES.md: its bullet lines.
fn divergence_lines() -> Vec<String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join("docs/DIVERGENCES.md")).expect("DIVERGENCES");
    let start = text
        .find("\n## Profile `enhanced`")
        .expect("DIVERGENCES.md has a \"Profile `enhanced`\" section");
    let section = &text[start + 1..];
    let end = section[3..].find("\n## ").map_or(section.len(), |i| i + 3);
    section[..end]
        .lines()
        .filter(|l| l.starts_with("- "))
        .map(str::to_string)
        .collect()
}

#[test]
fn every_enhanced_difference_is_a_setting_with_its_divergence_line() {
    let lines = divergence_lines();
    assert_eq!(
        lines.len(),
        ENHANCED.len(),
        "one DIVERGENCES line per enhanced difference: {lines:?}"
    );
    for difference in ENHANCED {
        assert!(
            field_by_path(difference.path).is_some(),
            "{}",
            difference.path
        );
        assert!(
            !difference.measurement.trim().is_empty(),
            "{}",
            difference.path
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains(&format!("`{}", difference.path))),
            "{} has no line in DIVERGENCES, Profile `enhanced`",
            difference.path
        );
    }
    // Every entry parses (panics otherwise).
    let _ = Profile::Enhanced.patch();
    // The dummy parses too, and touches only its key.
    with_enhanced_differences(DUMMY, || {
        let patch = profile::Profile::Enhanced.patch();
        let mut expected = SettingsPatch::default();
        expected.fanout.max_passes = Some(7);
        assert_eq!(patch, expected);
    });
}

#[test]
fn rebase_moves_only_the_profile_values() {
    with_enhanced_differences(DUMMY, || {
        let mut patch = resolve_with(vec![]).patch;
        patch.scoring.via_costs = Some(77); // the user's value
        assert_eq!(patch.fanout.max_passes, Some(7));
        profile::rebase(&mut patch, Profile::Legacy, CORES);
        assert_eq!(patch.profile.as_deref(), Some("legacy"));
        assert_eq!(patch.fanout.max_passes, Some(20), "the profile value moves");
        assert_eq!(patch.scoring.via_costs, Some(77), "the user's value stays");
        profile::rebase(&mut patch, Profile::Enhanced, CORES);
        assert_eq!(patch.fanout.max_passes, Some(7));
        // A value the user set away from both stays through both moves.
        patch.fanout.max_passes = Some(3);
        profile::rebase(&mut patch, Profile::Legacy, CORES);
        assert_eq!(patch.fanout.max_passes, Some(3));
    });
}
