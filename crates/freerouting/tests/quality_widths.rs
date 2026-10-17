//! Q02.1 and Q02.2: the delivered-width report and the narrowing ledger
//! (`quality::neckdown`, the `widths` diagnostic).
//!
//! The fixtures are synthetic two-layer boards in `tests/data/quality/`:
//! - `q02-pinch.dsn`: net PWR (class Power_High, 1.5 mm) is pre-routed
//!   from J1 to J2 with one 2 mm stretch at 0.4 mm (the forced pinch); PWR2
//!   (also 1.5 mm) runs from the big pad J3 to U3-2, a 0.25 mm pad between
//!   two pads of other nets at 0.65 mm pitch, where the router necks down
//!   (Java's room-local `automatic_neckdown`), and
//!   SIG (0.2 mm) is routed by the router;
//! - Q03's `q03-qfn-0p5.dsn`: net PWR (1.5 mm) must reach a 0.25 mm QFN pad.

mod autoroute_common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::drc::json::JsonValue;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::quality::neckdown::NeckdownReport;
use freerouting::quality::neckdown::width_report::report;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const PINCH: &str = "crates/freerouting/tests/data/quality/q02-pinch.dsn";
const QFN: &str = "crates/freerouting/tests/data/quality/q03-qfn-0p5.dsn";

fn prepare(dsn: &str, widths: bool, strict: bool) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(false);
    profile.set_max_passes(2);
    if widths {
        profile.patch.special.diagnostics = Some(vec!["widths".to_string()]);
    }
    if strict {
        profile.patch.neck_down.strict = Some(true);
    }
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

fn route(dsn: &str, widths: bool, strict: bool) -> (RoutingBoard, Option<NeckdownReport>) {
    let prepared = prepare(dsn, widths, strict);
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome =
        run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{dsn}: {}", f.error));
    let board = outcome.board.take().expect("no listener took the board");
    (board, outcome.summary.widths)
}

fn ses(board: &RoutingBoard, dsn: &str) -> String {
    write_ses(board, dsn, SesStyle::Java)
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn member<'a>(v: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    match v {
        JsonValue::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn number(v: Option<&JsonValue>) -> f64 {
    match v {
        Some(JsonValue::Number(n)) => n.parse().unwrap(),
        other => panic!("not a number: {other:?}"),
    }
}

/// `tools/quality/ses_widths.py` on `ses` with `dsn`, as JSON.
fn ses_widths_py(ses: &Path, dsn: &Path) -> Option<JsonValue> {
    let script = repo_root().join("tools/quality/ses_widths.py");
    let out = match Command::new("python3")
        .arg(&script)
        .arg(ses)
        .arg("--dsn")
        .arg(dsn)
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            eprintln!("skipping the SES cross-check: no python3 ({e})");
            return None;
        }
    };
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(JsonValue::parse(&String::from_utf8(out.stdout).unwrap()).unwrap())
}

#[test]
fn the_forced_pinch_is_the_under_width_length() {
    let (board, widths) = route(PINCH, true, false);
    let widths = widths.expect("the widths diagnostic ran");
    let upm = widths.widths.units_per_mm;
    let pwr = widths.widths.net("PWR").expect("PWR is a wide net");
    assert_eq!(pwr.net_class, "Power_High");
    assert!((pwr.under / upm - 2.0).abs() < 0.01, "{}", pwr.under / upm);
    assert_eq!(pwr.min_width, Some(4000));
    assert_eq!(pwr.requested_on(0), Some(15000));
    assert!(
        (pwr.length / upm - 24.0).abs() < 0.01,
        "{}",
        pwr.length / upm
    );
    // SIG is in the narrowest class: not reported.
    assert!(widths.widths.net("SIG").is_none());
    assert_eq!(widths.widths.narrowest_class_width, 2000);
    // The ledger names the pinch, made by the input.
    let pinch: Vec<_> = widths
        .ledger
        .entries
        .iter()
        .filter(|n| n.net_name == "PWR")
        .collect();
    assert_eq!(pinch.len(), 1, "{:?}", widths.ledger);
    assert_eq!(pinch[0].stage, "input");
    assert_eq!((pinch[0].requested, pinch[0].delivered), (15000, 4000));
    // The ledger is in ascending item id.
    let ids: Vec<i32> = widths.ledger.entries.iter().map(|n| n.item).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted);

    // The independent SES parser agrees within 0.01 mm on every wide net.
    let Some(py) = cross_check(&board, &widths, PINCH) else {
        return;
    };
    let pwr_py = member(member(&py, "nets").unwrap(), "PWR").unwrap();
    assert!((number(member(pwr_py, "under_mm")) - 2.0).abs() < 0.01);
}

/// Compares the report with `tools/quality/ses_widths.py` on the board's
/// SES, net by net, within 0.01 mm; `None` without python3.
fn cross_check(board: &RoutingBoard, widths: &NeckdownReport, dsn: &str) -> Option<JsonValue> {
    let upm = widths.widths.units_per_mm;
    let name = Path::new(dsn).file_name().unwrap().to_str().unwrap();
    let dir = scratch("q02-widths");
    let ses_path = dir.join(format!("{name}.ses"));
    std::fs::write(&ses_path, ses(board, name)).unwrap();
    let py = ses_widths_py(&ses_path, &repo_root().join(dsn))?;
    let nets = member(&py, "nets").expect("nets");
    for net in &widths.widths.nets {
        if net.length == 0.0 && member(nets, &net.net_name).is_none() {
            continue;
        }
        let entry = member(nets, &net.net_name)
            .unwrap_or_else(|| panic!("{} missing from the SES report", net.net_name));
        let under = number(member(entry, "under_mm"));
        let length = number(member(entry, "length_mm"));
        assert!(
            (under - net.under / upm).abs() < 0.01,
            "{}: SES {under} mm, board {} mm",
            net.net_name,
            net.under / upm
        );
        assert!(
            (length - net.length / upm).abs() < 0.01,
            "{}: SES {length} mm, board {} mm",
            net.net_name,
            net.length / upm
        );
    }
    Some(py)
}

#[test]
fn the_router_made_neck_at_a_small_pad_is_in_the_ledger() {
    let (board, widths) = route(PINCH, true, false);
    let widths = widths.expect("the widths diagnostic ran");
    let pwr2 = widths.widths.net("PWR2").expect("PWR2 is a wide net");
    let upm = widths.widths.units_per_mm;
    assert!(pwr2.under > 0.0, "the router necks down at U3-2");
    assert!(pwr2.min_width.unwrap() < 15000);
    // Everything shipped below 1.5 mm is in the ledger, made by the
    // connection router (Java's room-local neck-down).
    let entries: Vec<_> = widths
        .ledger
        .entries
        .iter()
        .filter(|n| n.net_name == "PWR2")
        .collect();
    let under: f64 = entries.iter().map(|n| n.length).sum();
    assert!((under - pwr2.under).abs() < 1e-6);
    for n in &entries {
        assert_eq!(n.stage, "freerouting-connection", "{n:?}");
    }
    eprintln!(
        "PWR2: under {:.4} mm, min {:.4} mm, {} ledger entries",
        pwr2.under / upm,
        f64::from(pwr2.min_width.unwrap()) / upm,
        entries.len()
    );
    cross_check(&board, &widths, PINCH);
}

#[test]
fn the_report_is_also_a_pure_function_of_the_board() {
    let (board, widths) = route(PINCH, true, false);
    assert_eq!(report(&board), widths.unwrap().widths);
}

#[test]
fn the_ses_is_byte_identical_with_the_report_on_and_off() {
    for dsn in [PINCH, QFN] {
        let (on, report) = route(dsn, true, false);
        let (off, none) = route(dsn, false, false);
        assert!(report.is_some() && none.is_none());
        assert_eq!(ses(&on, dsn), ses(&off, dsn), "{dsn}");
    }
}

#[test]
fn the_report_json_has_its_schema_and_ledger() {
    let (_, widths) = route(PINCH, true, false);
    let json = widths.unwrap().to_json();
    assert!(
        matches!(member(&json, "schema"), Some(JsonValue::String(s)) if s == "freerouting-widths/1")
    );
    assert_eq!(number(member(&json, "narrowest_class_width_mm")), 0.2);
    assert!(matches!(
        member(&json, "strict"),
        Some(JsonValue::Bool(false))
    ));
    let Some(JsonValue::Array(narrowings)) = member(&json, "narrowings") else {
        panic!("no narrowings");
    };
    assert!(!narrowings.is_empty());
    let text = json.to_pretty_string();
    assert!(text.contains("\"under_mm\": 2.0000"), "{text}");
}

#[test]
fn strict_sizes_turn_the_neck_downs_off_and_add_the_report() {
    let prepared = prepare(QFN, false, true);
    assert!(!prepared.board_settings.automatic_neckdown());
    assert_eq!(prepared.board_settings.neck_width_um(), 0.0);
    assert_eq!(
        prepared.settings.special.diagnostics.as_deref(),
        Some(&["widths".to_string()][..])
    );
    let (_, widths) = route(QFN, false, true);
    assert!(widths.expect("strict adds the report").strict);
    // Off by default: Java's neck-down stays on.
    let default = prepare(QFN, false, false);
    assert!(default.board_settings.automatic_neckdown());
    assert!(default.settings.special.diagnostics.is_none());
}
