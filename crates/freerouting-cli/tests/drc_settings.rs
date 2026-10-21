//! K15.5: `-drc` reads no router settings and warns about each one it is
//! given, on the command line or in the environment.

mod common;

use std::process::Command;

use common::*;

const DESIGN: &str = "Issue029-hw48na.dsn";

#[test]
fn drc_warns_once_per_ignored_router_setting() {
    let dir = scratch("drc-settings");
    copy_fixtures(&dir, &[DESIGN]);
    let out = Command::new(env!("CARGO_BIN_EXE_freerouting-rs"))
        .args([
            "-de",
            DESIGN,
            "-drc",
            "a.json",
            "--router.hole_clearance_um=260",
            "--router.pad_contacts=inside_pad",
        ])
        .current_dir(&dir)
        .env("XDG_CONFIG_HOME", dir.join("no-config"))
        .env("FREEROUTING__ROUTER__CLEARANCE_TOLERANCE_UM", "2")
        .output()
        .expect("the binary runs");
    assert_eq!(code(&out), 0, "{}", stderr(&out));
    let err = stderr(&out);
    let warnings: Vec<&str> = err
        .lines()
        .filter(|l| l.contains("The DRC (-drc) ignores router."))
        .collect();
    assert_eq!(warnings.len(), 3, "{err}");
    for (setting, source) in [
        ("router.hole_clearance_um", "the command line"),
        ("router.pad_contacts", "the command line"),
        ("router.clearance_tolerance_um", "the environment"),
    ] {
        assert_eq!(
            warnings
                .iter()
                .filter(|l| l.contains(&format!("ignores {setting}, set by {source}")))
                .count(),
            1,
            "{setting}: {err}"
        );
    }

    // The report is the one without the settings.
    let plain = run_in(&dir, &["-de", DESIGN, "-drc", "b.json"]);
    assert_eq!(code(&plain), 0);
    assert!(!stderr(&plain).contains("ignores router."));
    let report = |name: &str| {
        let text = std::fs::read_to_string(dir.join(name)).unwrap();
        // The date line differs between runs.
        text.lines()
            .filter(|l| !l.contains("\"date\""))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(report("a.json"), report("b.json"));
}
