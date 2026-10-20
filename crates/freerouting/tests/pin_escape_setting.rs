//! Q21.3: `router.fanout.pin_escape`, its default in both profiles and its
//! parse from the command line, a JSON settings text and the value check.
//! The stage it selects is `freerouting-krt`'s (`crates/freerouting-krt/
//! tests/pin_escape.rs` routes with it).

use freerouting::autoroute::stages::StageSelection;
use freerouting::autoroute::stages::pin_escape::{
    MODES, PIN_ESCAPE_STAGE, PinEscape, SETTINGS_KEY, check_settings,
};
use freerouting::settings::json_file::patch_from_json_text;
use freerouting::settings::profile::Profile;
use freerouting::settings::{
    FieldValue, RouterSettings, SettingsPatch, field_by_path, parse_cli_arguments,
};

const CORES: usize = 4;

/// The resolved settings `profile` starts from.
fn defaults(profile: Profile) -> RouterSettings {
    RouterSettings::from_patch(&profile.defaults(CORES), CORES)
}

#[test]
fn pin_escape_is_off_by_default_in_both_profiles() {
    for profile in [Profile::Legacy, Profile::Enhanced] {
        let settings = defaults(profile);
        assert_eq!(settings.fanout.pin_escape, None, "{profile:?}");
        assert_eq!(PinEscape::of(&settings), PinEscape::Off, "{profile:?}");
        assert!(check_settings(&settings).is_ok());
        let selection = StageSelection::from_settings(&settings);
        assert!(
            selection.stages.iter().all(|c| c.key != SETTINGS_KEY),
            "{profile:?}: no stage selected"
        );
    }
    assert_eq!(Profile::Enhanced.patch().fanout.pin_escape, None);
}

#[test]
fn pin_escape_parses_off_and_straight_and_refuses_the_rest() {
    assert_eq!(PinEscape::parse(None), Ok(PinEscape::Off));
    assert_eq!(PinEscape::parse(Some("")), Ok(PinEscape::Off));
    assert_eq!(PinEscape::parse(Some("off")), Ok(PinEscape::Off));
    assert_eq!(PinEscape::parse(Some("straight")), Ok(PinEscape::Straight));
    assert_eq!(
        PinEscape::parse(Some(" straight ")),
        Ok(PinEscape::Straight)
    );
    let error = PinEscape::parse(Some("bent")).unwrap_err();
    assert!(
        error.contains(SETTINGS_KEY) && error.contains("'bent'"),
        "{error}"
    );
    assert_eq!(MODES, ["off", "straight"]);
    for mode in [PinEscape::Off, PinEscape::Straight] {
        assert_eq!(PinEscape::parse(Some(mode.as_str())), Ok(mode));
    }
    assert_eq!(PinEscape::Off.stage(), None);
    assert_eq!(PinEscape::Straight.stage(), Some(PIN_ESCAPE_STAGE));

    let mut settings = defaults(Profile::Enhanced);
    settings.fanout.pin_escape = Some("bent".to_string());
    let refused = check_settings(&settings).unwrap_err();
    assert_eq!(refused.key, SETTINGS_KEY);
    assert_eq!(refused.id, "bent");
    assert_eq!(PinEscape::of(&settings), PinEscape::Off, "no stage for it");
}

#[test]
fn the_cli_and_a_json_text_set_pin_escape() {
    let args = vec!["--router.fanout.pin_escape=straight".to_string()];
    let parsed = parse_cli_arguments(&args);
    assert_eq!(parsed.patch.fanout.pin_escape.as_deref(), Some("straight"));

    let json = patch_from_json_text(r#"{"fanout": {"pin_escape": "straight"}}"#).unwrap();
    assert_eq!(json.patch.fanout.pin_escape.as_deref(), Some("straight"));

    let field = field_by_path("fanout.pin_escape").expect("a settings field");
    let mut settings = defaults(Profile::Legacy);
    let mut patch = SettingsPatch::default();
    (field.set)(&mut patch, FieldValue::Str("straight".to_string()));
    assert_eq!(patch.fanout.pin_escape.as_deref(), Some("straight"));
    settings.fanout.pin_escape = patch.fanout.pin_escape.clone();
    let selection = StageSelection::from_settings(&settings);
    let chosen: Vec<(&str, &str)> = selection
        .stages
        .iter()
        .map(|c| (c.key, c.id.as_str()))
        .collect();
    assert_eq!(chosen, vec![(SETTINGS_KEY, PIN_ESCAPE_STAGE)]);
}
