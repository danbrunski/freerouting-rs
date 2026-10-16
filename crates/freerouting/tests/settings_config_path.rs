//! C02.5: config-file lookup.

// Patches are built field by field, as the Java tests build `RouterSettings`.
#![allow(clippy::field_reassign_with_default)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use freerouting::settings::config_path::{
    ConfigChoice, EnvLookup, Os, config_file, default_config_file,
};
use freerouting::settings::json_file::{ConfigError, load_config};

fn env(vars: &[(&str, &str)]) -> HashMap<String, String> {
    vars.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn p(parts: &[&str]) -> PathBuf {
    parts.iter().collect()
}

#[test]
fn os_defaults() {
    let windows = env(&[("APPDATA", "C:\\Users\\dan\\AppData\\Roaming")]);
    assert_eq!(
        default_config_file(&windows, Os::Windows),
        Some(p(&[
            "C:\\Users\\dan\\AppData\\Roaming",
            "freerouting-rs",
            "freerouting.json"
        ]))
    );
    let mac = env(&[("HOME", "/Users/dan")]);
    assert_eq!(
        default_config_file(&mac, Os::MacOs),
        Some(p(&[
            "/Users/dan",
            "Library",
            "Application Support",
            "freerouting-rs",
            "freerouting.json"
        ]))
    );
    let xdg = env(&[("HOME", "/home/dan"), ("XDG_CONFIG_HOME", "/home/dan/cfg")]);
    assert_eq!(
        default_config_file(&xdg, Os::Linux),
        Some(p(&["/home/dan/cfg", "freerouting-rs", "freerouting.json"]))
    );
    let home = env(&[("HOME", "/home/dan")]);
    assert_eq!(
        default_config_file(&home, Os::Linux),
        Some(p(&[
            "/home/dan",
            ".config",
            "freerouting-rs",
            "freerouting.json"
        ]))
    );
    // An empty or relative XDG_CONFIG_HOME is ignored.
    for bad in ["", "relative/dir"] {
        let e = env(&[("HOME", "/home/dan"), ("XDG_CONFIG_HOME", bad)]);
        assert_eq!(
            default_config_file(&e, Os::Linux),
            Some(p(&[
                "/home/dan",
                ".config",
                "freerouting-rs",
                "freerouting.json"
            ])),
            "{bad:?}"
        );
    }
    // Without the variables there is no default.
    assert_eq!(default_config_file(&env(&[]), Os::Linux), None);
    assert_eq!(default_config_file(&env(&[]), Os::Windows), None);
    assert_eq!(default_config_file(&home, Os::Windows), None);
    // Java's own directory is never the default.
    let all = env(&[("APPDATA", "/a"), ("HOME", "/h")]);
    for os in [Os::Windows, Os::MacOs, Os::Linux] {
        let path = default_config_file(&all, os).unwrap();
        assert!(path.to_string_lossy().contains("freerouting-rs"));
    }
}

#[test]
fn precedence_of_the_choices() {
    let e = env(&[
        ("HOME", "/home/dan"),
        ("FREEROUTING__USER_DATA_PATH", "/env/data"),
        ("FREEROUTING__LOGGING__FILE__LOCATION", "/env/logging"),
    ]);
    // 1. --settings
    assert_eq!(
        config_file(
            &ConfigChoice::Explicit(p(&["/x", "my.json"])),
            &e,
            Os::Linux
        ),
        Some(p(&["/x", "my.json"]))
    );
    // 2. --user_data_path= beats the environment.
    assert_eq!(
        config_file(&ConfigChoice::UserDataPath(p(&["/cli"])), &e, Os::Linux),
        Some(p(&["/cli", "freerouting.json"]))
    );
    // 3. FREEROUTING__USER_DATA_PATH.
    assert_eq!(
        config_file(&ConfigChoice::Default, &e, Os::Linux),
        Some(p(&["/env/data", "freerouting.json"]))
    );
    // 4. The deprecated logging-location fallback.
    let e = env(&[
        ("HOME", "/home/dan"),
        ("FREEROUTING__LOGGING__FILE__LOCATION", "/env/logging"),
    ]);
    assert_eq!(
        config_file(&ConfigChoice::Default, &e, Os::Linux),
        Some(p(&["/env/logging", "freerouting.json"]))
    );
    // 5. The OS default.
    let e = env(&[("HOME", "/home/dan")]);
    assert_eq!(
        config_file(&ConfigChoice::Default, &e, Os::Linux),
        default_config_file(&e, Os::Linux)
    );
}

#[test]
fn choice_from_command_line() {
    let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(
        ConfigChoice::from_args(&args(&["-de", "a.dsn"])),
        ConfigChoice::Default
    );
    assert_eq!(
        ConfigChoice::from_args(&args(&["--settings", "f.json"])),
        ConfigChoice::Explicit(PathBuf::from("f.json"))
    );
    assert_eq!(
        ConfigChoice::from_args(&args(&["--settings=g.json"])),
        ConfigChoice::Explicit(PathBuf::from("g.json"))
    );
    assert_eq!(
        ConfigChoice::from_args(&args(&["--user_data_path=/d"])),
        ConfigChoice::UserDataPath(PathBuf::from("/d"))
    );
    // --settings wins over --user_data_path= in either order.
    for a in [
        ["--user_data_path=/d", "--settings=g.json"],
        ["--settings=g.json", "--user_data_path=/d"],
    ] {
        assert_eq!(
            ConfigChoice::from_args(&args(&a)),
            ConfigChoice::Explicit(PathBuf::from("g.json"))
        );
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("settings_config_path")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `--settings` replaces the default file; it does not stack. Both files
/// exist and hold different values.
#[test]
fn settings_flag_replaces_the_default_file() {
    let default_dir = scratch("default");
    std::fs::write(
        default_dir.join("freerouting.json"),
        r#"{"router": {"scoring": {"start_ripup_costs": 111, "via_costs": 11}}}"#,
    )
    .unwrap();
    let explicit_dir = scratch("explicit");
    let explicit = explicit_dir.join("mine.json");
    std::fs::write(
        &explicit,
        r#"{"router": {"scoring": {"start_ripup_costs": 222}}}"#,
    )
    .unwrap();

    let e = env(&[("FREEROUTING__USER_DATA_PATH", default_dir.to_str().unwrap())]);
    let from_default = load_config(&ConfigChoice::Default, &e, Os::Linux).unwrap();
    assert_eq!(from_default.patch.scoring.start_ripup_costs, Some(111));
    assert_eq!(from_default.patch.scoring.via_costs, Some(11));

    let from_explicit = load_config(&ConfigChoice::Explicit(explicit), &e, Os::Linux).unwrap();
    assert_eq!(from_explicit.patch.scoring.start_ripup_costs, Some(222));
    assert_eq!(
        from_explicit.patch.scoring.via_costs, None,
        "the default file's other values do not leak in"
    );
}

#[test]
fn a_missing_settings_file_is_an_error_naming_the_path_but_a_missing_default_is_not() {
    let dir = scratch("missing");
    let missing = dir.join("nope.json");
    let err = load_config(
        &ConfigChoice::Explicit(missing.clone()),
        &env(&[]),
        Os::Linux,
    )
    .unwrap_err();
    assert!(matches!(err, ConfigError::Missing(_)));
    assert!(
        err.to_string().contains(&missing.display().to_string()),
        "{err}"
    );
    let loaded = load_config(&ConfigChoice::UserDataPath(dir), &env(&[]), Os::Linux).unwrap();
    assert!(loaded.warnings.is_empty());
    // No default location at all is empty settings too.
    assert!(load_config(&ConfigChoice::Default, &env(&[]), Os::Linux).is_ok());
}

#[test]
fn process_env_is_an_env_lookup() {
    let e: &dyn EnvLookup = &freerouting::settings::config_path::ProcessEnv;
    assert!(e.var("FREEROUTING_NO_SUCH_VARIABLE_FOR_TEST").is_none());
}
