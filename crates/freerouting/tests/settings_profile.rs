//! N01.1: the `router.profile` key and the profile table.
//!
//! - under `legacy` every default equals Java's (today's) default, key by key;
//! - an explicit key from any source beats the profile;
//! - an unknown profile is a settings error that names the valid values;
//! - every entry of the `enhanced` table is a valid setting and has its
//!   DIVERGENCES line (the "Profile `enhanced`" section), and `enhanced`
//!   sets each one while `legacy` keeps Java's value (N05.3).

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
fn enhanced_is_the_default_and_differs_from_legacy_only_by_its_table() {
    let unset = resolve_with(vec![]);
    assert_eq!(unset.profile, Ok(Profile::Enhanced));
    assert_eq!(unset.patch.profile, None, "the default is not written");
    // Every key outside the table is Java's; every key in it has the
    // table's value, from the profile source.
    let java = default_patch(CORES);
    let mut expected = java.clone();
    for difference in ENHANCED {
        expected
            .set_path(difference.path, difference.value)
            .expect("a valid entry");
    }
    for field in FIELDS.iter().filter(|f| f.path != "profile") {
        assert_eq!(
            (field.get)(&unset.patch),
            (field.get)(&expected),
            "enhanced: {}",
            field.path
        );
        let in_table = ENHANCED.iter().any(|d| d.path == field.path);
        assert_eq!(
            unset.provenance.get(field.path) == Some(&Source::Profile(Profile::Enhanced)),
            in_table,
            "only the table's keys come from the profile: {}",
            field.path
        );
    }
}

/// N05.3: each promotion sets its key under `enhanced` and leaves `legacy`
/// at Java's value.
fn assert_enhanced_sets(path: &str, value: &str, java: Option<FieldValue>, other: &str) {
    let enhanced = resolve_with(vec![]);
    let legacy = resolve_with(vec![(Source::Cli, legacy_pin())]);
    assert_eq!(
        self::value(&enhanced.patch, path),
        parsed(path, value),
        "enhanced sets {path}"
    );
    assert_eq!(
        enhanced.provenance.get(path),
        Some(&Source::Profile(Profile::Enhanced))
    );
    assert_eq!(
        self::value(&legacy.patch, path),
        java,
        "legacy keeps {path}"
    );
    // An explicit key (here `other`) still beats the profile.
    let mut cli = SettingsPatch::default();
    cli.set_path(path, other).expect("a valid value");
    let explicit = resolve_with(vec![(Source::Cli, cli)]);
    assert_eq!(self::value(&explicit.patch, path), parsed(path, other));
}

/// `text` as the field at `path` holds it (`"300"` is a `Double` for a
/// micrometre field, a `Str` for a string field).
fn parsed(path: &str, text: &str) -> Option<FieldValue> {
    let mut patch = SettingsPatch::default();
    patch.set_path(path, text).expect("a valid value");
    value(&patch, path)
}

#[test]
fn enhanced_sets_power_layer_traces_none() {
    // N05.3, docs/roadmap/prereg/N05-power-layer-traces.json.
    assert_enhanced_sets("power_layer_traces", "none", None, "layer");
}

#[test]
fn enhanced_sets_neck_width_um_300() {
    // N05.3, docs/roadmap/prereg/N05-neck-down.json: Java's necked retry of a
    // failed connection at 300 um; Java's default 0 (off) under `legacy`.
    assert_eq!(
        parsed("neck_width_um", "300"),
        Some(FieldValue::Double(300.0))
    );
    assert_enhanced_sets("neck_width_um", "300", Some(FieldValue::Double(0.0)), "0");
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

#[test]
fn the_profile_is_never_java_s_merge_base() {
    // With no defaults source the profile sorts first; Java's merge base is
    // still the first real source (its clone drops `result_json`), laid
    // over the profile's values, and an empty list still warns.
    with_enhanced_differences(DUMMY, || {
        let first = SettingsPatch {
            result_json: Some("out.json".to_string()),
            ..SettingsPatch::default()
        };
        let merged = resolve(&[(Source::ConfigFile, first)], &ctx());
        assert_eq!(merged.patch.result_json, None);
        assert_eq!(merged.patch.fanout.max_passes, Some(7));
        let empty = resolve(&[], &ctx());
        assert!(
            empty
                .warnings
                .contains(&"No settings sources provided, using defaults".to_string())
        );
        assert_eq!(empty.patch.fanout.max_passes, Some(7));
    });
}
