//! K15.7: the copper-to-edge cap names the pin that sets it, and under
//! `enhanced` leaves a pin wholly outside the outline out of it.
//!
//! The board is 20 mm x 10 mm. A fiducial `FID1` sits at (0, 0), outside the
//! outline, and `U2-1`'s pad is 0.1 mm inside the left edge. Under `legacy`
//! (Java's computation) the fiducial gives a gap of 0 and caps the default
//! 0.25 mm clearance at 0 for the whole board; under `enhanced` (the setting
//! `edge_cap_skips_off_board_pins`) it is left out with a warning and the cap
//! is U2-1's 0.1 mm. The names are part of the text, so the tests pin it.

use freerouting::job::LogLevel;
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::{ResolveContext, SettingsPatch, Source};

const BOARD: &str = r#"(pcb k15-edge-cap
  (parser (string_quote ") (space_in_quoted_tokens on))
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (layer B.Cu (type signal) (property (index 1)))
    (boundary (path pcb 0  1000 1000  21000 1000  21000 11000  1000 11000  1000 1000))
    (via V600)
    (rule (width 250) (clearance 200))
  )
  (placement
    (component PAD (place U1 10000 6000 front 0) (place U2 1500 6000 front 0))
    (component FID (place FID1 0 0 front 0))
  )
  (library
    (image PAD (pin SmdPad 1 0 0))
    (image FID (pin FidPad 1 0 0))
    (padstack SmdPad (shape (rect F.Cu -400 -400 400 400)) (attach off))
    (padstack FidPad (shape (rect F.Cu -300 -300 300 300)) (attach off))
    (padstack V600 (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off))
  )
  (network
    (net A (pins U1-1 U2-1))
    (class kicad_default "" A (circuit (use_via V600)))
  )
)
"#;

fn prepare(profile: &str) -> PreparedJob {
    prepare_with(profile, None)
}

fn prepare_with(profile: &str, skip_off_board: Option<bool>) -> PreparedJob {
    let mut input = JobInput::new(BOARD.as_bytes().to_vec(), "k15-edge-cap");
    input.rules = RulesChoice::None;
    let patch = SettingsPatch {
        profile: Some(profile.to_string()),
        edge_cap_skips_off_board_pins: skip_off_board,
        ..SettingsPatch::default()
    };
    input.extra_sources.push((Source::Testing, patch));
    prepare_job(&input, &ResolveContext { cores: 1 }).expect("the board loads")
}

fn edge_units(job: &PreparedJob) -> i32 {
    let matrix = &job.board.rules.clearance_matrix;
    let edge = matrix.get_no("board_edge");
    assert!(edge >= 0);
    matrix.get_value(edge, edge, 0, false)
}

fn cap_line(job: &PreparedJob) -> String {
    let lines: Vec<_> = job
        .log
        .iter()
        .filter(|l| l.text.starts_with("Copper-to-edge clearance capped"))
        .collect();
    assert_eq!(lines.len(), 1, "{:?}", job.log);
    assert_eq!(lines[0].level, LogLevel::Info);
    lines[0].text.clone()
}

#[test]
fn legacy_keeps_javas_cap_and_names_the_off_board_pin() {
    let job = prepare("legacy");
    assert_eq!(edge_units(&job), 0, "the fiducial caps the clearance at 0");
    assert_eq!(
        cap_line(&job),
        "Copper-to-edge clearance capped from 2500 to 0 board units because pin FID1-1 is 0.0 um \
         from the board outline and lies wholly outside the board outline \
         (router.edge_cap_skips_off_board_pins leaves such a pin out)."
    );
    assert!(
        job.log
            .iter()
            .all(|l| l.level != LogLevel::Warn || !l.text.contains("FID1")),
        "legacy leaves nothing out"
    );
}

#[test]
fn an_off_board_pin_is_left_out_of_the_edge_cap_when_the_setting_is_on() {
    let job = prepare_with("enhanced", Some(true));
    assert_eq!(job.settings.edge_cap_skips_off_board_pins, Some(true));
    // 0.1 mm at 10 units per um is 1000 units, less rounding of the pad corner.
    let units = edge_units(&job);
    assert!((998..=1000).contains(&units), "{units}");
    let warns: Vec<&str> = job
        .log
        .iter()
        .filter(|l| l.level == LogLevel::Warn && l.text.contains("FID1"))
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(
        warns,
        [
            "Copper-to-edge clearance cap: pin FID1-1 lies wholly outside the board outline and \
             is left out."
        ]
    );
    let line = cap_line(&job);
    assert!(
        line.starts_with("Copper-to-edge clearance capped from 2500 to ")
            && line.contains("because pin U2-1 is 100.0 um from the board outline."),
        "{line}"
    );
}

#[test]
fn both_profiles_leave_the_setting_unset() {
    // Opt-in until a preregistered measurement accepts it for `enhanced` (AGENTS.md).
    assert_eq!(
        prepare("legacy").settings.edge_cap_skips_off_board_pins,
        None
    );
    assert_eq!(
        prepare("enhanced").settings.edge_cap_skips_off_board_pins,
        None
    );
}
