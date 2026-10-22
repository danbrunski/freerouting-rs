//! Settings keys the port removed (`settings::removed`): Q12's pass-1
//! portfolio keys (`autorouter.portfolio`, `autorouter.restarts`,
//! `autorouter.portfolio_memory_mb`), removed 2026-10-20 by Q12.5's fail
//! branch, and Q23's `autorouter.restarts_from_best`, removed 2026-10-22 by
//! Q23.4's fail branch. Each source refuses them with a message that names the
//! valid `router.autorouter` keys; none of them reaches a patch.

use freerouting::settings::env::parse_environment;
use freerouting::settings::json_file::patch_from_config_text;
use freerouting::settings::removed::{REMOVED, find_property};
use freerouting::settings::{FIELDS, SetError, SettingsPatch, parse_cli_arguments};

const KEYS: [&str; 4] = [
    "router.autorouter.portfolio",
    "router.autorouter.restarts",
    "router.autorouter.portfolio_memory_mb",
    "router.autorouter.restarts_from_best",
];

fn valid_autorouter_keys() -> String {
    FIELDS
        .iter()
        .filter(|f| f.section == "autorouter")
        .map(|f| f.name)
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn the_message_names_the_key_the_reason_and_the_valid_keys() {
    let removed = find_property("router.autorouter.portfolio").expect("removed");
    let message = removed.message();
    assert_eq!(
        message,
        format!(
            "router.autorouter.portfolio was removed on 2026-10-20: the pass-1 portfolio \
             failed its measurement (Q12.5); valid router.autorouter settings: {}",
            valid_autorouter_keys()
        )
    );
    assert!(message.contains("max_passes"), "{message}");
    assert!(!valid_autorouter_keys().contains("portfolio"));
    assert!(!valid_autorouter_keys().contains("restarts_from_best"));
    assert_eq!(
        REMOVED.iter().map(|r| r.property()).collect::<Vec<_>>(),
        KEYS
    );
}

#[test]
fn the_cli_property_is_refused_and_sets_nothing() {
    for key in KEYS {
        let mut patch = SettingsPatch::default();
        let mut warnings = Vec::new();
        let error = patch.cli_property(key, "4", &mut warnings).unwrap_err();
        assert!(matches!(error, SetError::Removed(_)), "{key}: {error:?}");
        assert!(error.to_string().starts_with(&format!("{key} was removed")));
        assert_eq!(patch, SettingsPatch::default(), "{key}");
    }
    // The camel-case name of the memory key is refused the same way.
    let mut patch = SettingsPatch::default();
    let error = patch
        .set_path("autorouter.portfolioMemoryMb", "512")
        .unwrap_err();
    assert!(matches!(error, SetError::Removed(r) if r.name == "portfolio_memory_mb"));

    let parse = parse_cli_arguments(&["--router.autorouter.portfolio=4".to_string()]);
    assert_eq!(parse.patch, SettingsPatch::default());
    assert!(parse.parsed_arguments.is_empty());
    assert_eq!(
        parse.warnings,
        [format!(
            "Failed to apply CLI router setting: router.autorouter.portfolio: {}",
            find_property("router.autorouter.portfolio")
                .unwrap()
                .message()
        )]
    );
}

#[test]
fn a_settings_file_that_sets_one_fails_to_read() {
    for (name, date) in [
        ("portfolio", "2026-10-20"),
        ("restarts", "2026-10-20"),
        ("portfolio_memory_mb", "2026-10-20"),
        ("portfolioMemoryMb", "2026-10-20"),
        ("restarts_from_best", "2026-10-22"),
        ("restartsFromBest", "2026-10-22"),
    ] {
        let text = format!(r#"{{"router": {{"autorouter": {{"max_passes": 3, "{name}": 4}}}}}}"#);
        let error = patch_from_config_text(&text).unwrap_err();
        assert!(
            error.contains(&format!(" was removed on {date}")),
            "{name}: {error}"
        );
        assert!(
            error.contains("valid router.autorouter settings: "),
            "{error}"
        );
    }
    // Without it the file reads.
    let ok = patch_from_config_text(r#"{"router": {"autorouter": {"max_passes": 3}}}"#).unwrap();
    assert_eq!(ok.patch.autorouter.max_passes, Some(3));
}

#[test]
fn an_environment_variable_that_sets_one_is_refused_with_a_warning() {
    let parse = parse_environment([("FREEROUTING__ROUTER__AUTOROUTER__RESTARTS", "2")]);
    assert_eq!(parse.patch, SettingsPatch::default());
    assert_eq!(parse.parsed_count(), 0);
    assert_eq!(parse.warnings.len(), 1);
    assert!(
        parse.warnings[0].contains("router.autorouter.restarts was removed on 2026-10-20"),
        "{:?}",
        parse.warnings
    );
}
