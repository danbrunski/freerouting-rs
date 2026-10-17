//! The batch optimizer (O06) against Java at the pin.
//!
//! **Candidates** (O06.2, O06.3): probe `optimize-candidates`
//! (`tools/java-reference/probes/board/OptimizeCandidatesProbe.java`, which documents its
//! stages) writes `fixtures/reference/optimize/candidates/<board>.jsonl`. The board loads as
//! the job does (defaults, the DSN, an adjacent rules file), the session named in the `order`
//! line is imported and every trace and via is unfixed.
//!
//! - `order`: the SEQUENTIAL candidate ids of pass 1 on ten boards. Equal on every board.
//! - `items`: the first 20 candidates of pass 1 on O06.7's six boards, each rerouted on its own
//!   fresh copy of the baseline with pass 1's parameters; every `ItemRouteResult` field. Java's
//!   `getHash` is not compared (Rust's `state_hash` has Java's equality, not its text). Equal
//!   for every candidate on every board.
//!
//! `Issue676-ch32v-tx118s` is not a candidate board: Java's `SesReader` imports none of its
//! wires (docs/DIVERGENCES.md), so its candidate lists differ for a reason outside O06.
//!
//! **Runs** (O06.7): `ReferenceDump optimize` writes `fixtures/reference/optimize/<board>.json`,
//! `<board>.t1.ses` and `<board>.t7.ses`: the whole optimizer (`max_passes` 3, `max_items` 200,
//! SEQUENTIAL, no timeout) on O06.7's six boards with 1 and 7 threads.
//! [`optimizer_runs_match_java`] runs the Rust optimizer on one thread and checks O06.7's
//! aggregate acceptance (incompletes and clearance violations at most Java's t1, via count and
//! weighted length within Java's t1-vs-t7 spread plus 2 %), and, on every board where Java's t1
//! and t7 sessions are equal, that the Rust session equals Java's byte for byte. At the pin
//! Java's t1 and t7 sessions are equal on all six boards (its reused worker boards made no
//! difference there) and so is the Rust session. Java accepts no pass on
//! `Issue508-DAC2020_bm05` (14 incompletes: guard G1 bypasses the optimizer),
//! `Issue508-SMD-routing-issue-demo` and `Issue269-min_fr_test/min_fr_test_no_quotes`, so only
//! bm08, J2 and `Issue954-circle` are evidence of the optimizer's moves.
//!
//! In a debug build (the workspace gate) `items` skips `Issue508-DAC2020_bm05`, whose 20
//! reroutes take minutes there; a release build runs every board.

mod board_probe;

use std::collections::BTreeMap;
use std::path::Path;

use board_probe::{bits, repo_root};
use freerouting::autoroute::events::CancelToken;
use freerouting::autoroute::events::NullEvents;
use freerouting::autoroute::stages::StageCtx;
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::io::read_session;
use freerouting::io::specctra::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::optimize::batch::candidates::sorted_candidates;
use freerouting::optimize::batch::item::ItemPassParams;
use freerouting::optimize::batch::worker::{PassSetup, evaluate_candidate};
use freerouting::optimize::batch::{OptimizerOptions, run_batch_optimizer};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::{ItemSelectionStrategy, ResolveContext, SettingsPatch, Source};

/// The ten boards of stage `order` (`OptimizeCandidatesProbe.ORDER_BOARDS`).
const ORDER_BOARDS: [&str; 10] = [
    "Issue954-circle",
    "Issue508-SMD-routing-issue-demo",
    "Issue269-min_fr_test/min_fr_test_no_quotes",
    "Issue143-rpi_splitter",
    "Issue433-my-board",
    "Issue413-test",
    "Issue954-polygon64",
    "Issue026-J2_reference",
    "Issue508-DAC2020_bm08",
    "Issue508-DAC2020_bm05",
];

/// The six boards of stage `items` (`OptimizeCandidatesProbe.ITEM_BOARDS`, O06.7's boards).
const ITEM_BOARDS: [&str; 6] = [
    "Issue954-circle",
    "Issue508-SMD-routing-issue-demo",
    "Issue269-min_fr_test/min_fr_test_no_quotes",
    "Issue026-J2_reference",
    "Issue508-DAC2020_bm08",
    "Issue508-DAC2020_bm05",
];

/// The `ItemRouteResult` fields of stage `items`, in line order.
const ITEM_FIELDS: [&str; 9] = [
    "item",
    "improved",
    "viasBefore",
    "viasAfter",
    "lengthBefore",
    "lengthAfter",
    "incompleteBefore",
    "incompleteAfter",
    "percent",
];

fn java_lines(board: &str) -> Vec<String> {
    let path = repo_root().join(format!(
        "fixtures/reference/optimize/candidates/{board}.jsonl"
    ));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .map(str::to_string)
        .collect()
}

/// The raw text of `key` in a probe line: a number, `true`/`false`, a string without its
/// quotes, or a JSON array.
fn field<'a>(line: &'a str, key: &str) -> &'a str {
    let tag = format!("\"{key}\":");
    let start = line
        .find(&tag)
        .unwrap_or_else(|| panic!("no {key} in {line}"))
        + tag.len();
    let rest = &line[start..];
    if let Some(body) = rest.strip_prefix('"') {
        &body[..body.find('"').expect("a closed string")]
    } else if rest.starts_with('[') {
        &rest[..=rest.find(']').expect("a closed array")]
    } else {
        &rest[..rest.find([',', '}']).expect("a field end")]
    }
}

/// `board` loaded as the job does, `ses` imported, every trace and via unfixed.
fn load(board: &str, ses: &str) -> PreparedJob {
    load_with(board, ses, None)
}

/// [`load`] with `patch` at the testing priority.
fn load_with(board: &str, ses: &str, patch: Option<SettingsPatch>) -> PreparedJob {
    let path = repo_root().join(format!("fixtures/{board}.dsn"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{board}: {e}"));
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("board");
    let mut input = JobInput::new(bytes, JobInput::design_name_of(file_name));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input
        .extra_sources
        .extend(patch.map(|p| (Source::Testing, p)));
    let mut prepared = prepare_job(&input, &ResolveContext { cores: 8 })
        .unwrap_or_else(|e| panic!("{board}: {e}"));
    let text = std::fs::read_to_string(repo_root().join(ses)).unwrap_or_else(|e| panic!("{e}"));
    read_session(&mut prepared.board, &text).unwrap_or_else(|e| panic!("{ses}: {e}"));
    unfix_routes(&mut prepared.board);
    prepared
}

fn unfix_routes(board: &mut RoutingBoard) {
    for id in board.item_ids() {
        if board.item(id).is_some_and(|i| i.is_trace() || i.is_via()) {
            board.unfix(id);
        }
    }
}

fn order_line(java: &[String]) -> &str {
    java.iter()
        .find(|l| l.contains("\"stage\":\"order\""))
        .expect("an order line")
}

fn rust_order(board: &RoutingBoard) -> String {
    let ids: Vec<String> = sorted_candidates(board)
        .iter()
        .map(|c| c.id.0.to_string())
        .collect();
    format!("[{}]", ids.join(","))
}

#[test]
fn candidate_order_matches_java() {
    let mut failures = Vec::new();
    for board in ORDER_BOARDS {
        let java = java_lines(board);
        let order = order_line(&java);
        let prepared = load(board, field(order, "ses"));
        let rust = rust_order(&prepared.board);
        let expected = field(order, "ids");
        eprintln!("{board}: {} candidates", field(order, "count"));
        if rust != expected {
            failures.push(format!("{board}:\n  Java {expected}\n  Rust {rust}"));
        }
    }
    assert!(
        failures.is_empty(),
        "candidate order differs:\n{}",
        failures.join("\n")
    );
}

/// Rust's `items` line for candidate `item` in Java's field order.
fn rust_item_fields(
    snapshot: &freerouting::board::BoardSnapshot,
    item: ItemId,
    setup: &PassSetup<'_>,
) -> Vec<String> {
    let r = evaluate_candidate(snapshot, item, setup).result;
    vec![
        r.item_id().0.to_string(),
        r.improved().to_string(),
        (r.via_count() + r.via_count_reduced()).to_string(),
        r.via_count().to_string(),
        bits(r.length_reduced() + r.trace_length()),
        bits(r.trace_length()),
        r.incomplete_count_before().to_string(),
        r.incomplete_count().to_string(),
        format!("{:x}", r.improvement_percentage().to_bits()),
    ]
}

#[test]
fn candidate_items_match_java() {
    let mut table = Vec::new();
    let mut failures = Vec::new();
    for board in ITEM_BOARDS {
        if cfg!(debug_assertions) && board == "Issue508-DAC2020_bm05" {
            continue;
        }
        let java = java_lines(board);
        let order = order_line(&java);
        let prepared = load(board, field(order, "ses"));
        let baseline = f64::from(
            BoardStatistics::new(&prepared.board)
                .traces
                .total_weighted_length,
        );
        let header = java
            .iter()
            .find(|l| l.contains("\"stage\":\"items\""))
            .expect("an items line");
        assert_eq!(
            bits(baseline),
            field(header, "baseline"),
            "{board}: baseline length"
        );
        let cancel = CancelToken::new();
        let setup = PassSetup {
            settings: &prepared.settings,
            board_settings: &prepared.board_settings,
            cancel: &cancel,
            params: ItemPassParams {
                baseline_length: baseline,
                with_preferred_directions: true,
                use_increased_ripup_costs: true,
            },
            threads: 1,
            chunk_size: 1,
            max_consecutive_failures: i32::MAX,
        };
        let snapshot = prepared.board.snapshot();
        let mut per_field: BTreeMap<&str, usize> = BTreeMap::new();
        let (mut matching, mut total) = (0, 0);
        for line in java.iter().filter(|l| l.starts_with("{\"item\":")) {
            let item = ItemId(field(line, "item").parse().expect("an item id"));
            let rust = rust_item_fields(&snapshot, item, &setup);
            let mut all = true;
            for (key, value) in ITEM_FIELDS.iter().zip(&rust) {
                if field(line, key) == value {
                    *per_field.entry(key).or_default() += 1;
                } else {
                    all = false;
                }
            }
            total += 1;
            matching += usize::from(all);
        }
        let rates: Vec<String> = ITEM_FIELDS
            .iter()
            .map(|k| format!("{k} {}/{total}", per_field.get(k).copied().unwrap_or(0)))
            .collect();
        table.push(format!(
            "{board}: {matching}/{total} match; {}",
            rates.join(", ")
        ));
        if matching != total {
            failures.push(format!("{board}: {matching} of {total} candidates match"));
        }
    }
    eprintln!("{}", table.join("\n"));
    assert!(
        failures.is_empty(),
        "{}\n{}",
        failures.join("\n"),
        table.join("\n")
    );
}

/// The figure `key` (`"via_count"` style path `section.key`) of a statistics object in a
/// `ReferenceDump` JSON.
fn json_number(text: &str, path: &[&str]) -> f64 {
    let mut rest = text;
    for key in path {
        let tag = format!("\"{key}\":");
        rest = &rest[rest.find(&tag).unwrap_or_else(|| panic!("no {key}")) + tag.len()..];
    }
    let rest = rest.trim_start();
    let end = rest.find([',', '\n', '}']).expect("a number end");
    rest[..end]
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("{path:?}: {e}"))
}

/// The six boards of `ReferenceDump optimize` and their routed sessions (below fixtures/).
const RUN_BOARDS: [(&str, &str); 6] = [
    (
        "Issue508-DAC2020_bm08",
        "fixtures/Issue508-DAC2020_bm08-routed.ses",
    ),
    (
        "Issue026-J2_reference",
        "fixtures/reference/route/Issue026-J2_reference.ses",
    ),
    (
        "Issue508-DAC2020_bm05",
        "fixtures/reference/route/Issue508-DAC2020_bm05.ses",
    ),
    (
        "Issue954-circle",
        "fixtures/reference/route/Issue954-circle.ses",
    ),
    (
        "Issue508-SMD-routing-issue-demo",
        "fixtures/reference/route/Issue508-SMD-routing-issue-demo.ses",
    ),
    (
        "Issue269-min_fr_test/min_fr_test_no_quotes",
        "fixtures/reference/route/Issue269-min_fr_test/min_fr_test_no_quotes.ses",
    ),
];

#[test]
fn optimizer_runs_match_java() {
    let base = repo_root().join("fixtures/reference/optimize");
    let mut table = Vec::new();
    let mut failures = Vec::new();
    for (board, ses) in RUN_BOARDS {
        let json = std::fs::read_to_string(base.join(format!("{board}.json"))).expect("the json");
        let runs = &json[json.find("\"runs\"").expect("runs")..];
        let t7_at = runs.find("\"t7\"").expect("a t7 run");
        let (t1, t7) = (&runs[..t7_at], &runs[t7_at..]);
        let after = |run: &str, path: &[&str]| {
            json_number(
                &run[run.rfind("\"statistics\"").expect("statistics")..],
                path,
            )
        };
        let java_t1_ses = std::fs::read(base.join(format!("{board}.t1.ses"))).expect("t1 ses");
        let java_t7_ses = std::fs::read(base.join(format!("{board}.t7.ses"))).expect("t7 ses");

        let mut patch = SettingsPatch::default();
        patch.optimizer.max_passes = Some(3);
        patch.optimizer.max_items = Some(200);
        patch.optimizer.item_selection_strategy = Some(ItemSelectionStrategy::Sequential);
        let mut prepared = load_with(board, ses, Some(patch));
        let cancel = CancelToken::new();
        let ctx = StageCtx {
            settings: &prepared.settings,
            board_settings: &prepared.board_settings,
            cancel: &cancel,
            events: &NullEvents,
        };
        let options = OptimizerOptions {
            threads: Some(1),
            ..OptimizerOptions::default()
        };
        let run = run_batch_optimizer(&mut prepared.board, &ctx, &options);
        let stats = BoardStatistics::new(&prepared.board);
        let stem = board.rsplit('/').next().expect("a name");
        let design = JobInput::design_name_of(&format!("{stem}.dsn"));
        let rust_ses = write_ses(&prepared.board, &design, SesStyle::Java);

        let incomplete = stats.connections.incomplete_count as f64;
        let violations = stats.clearance_violations.total_count as f64;
        let vias = stats.items.via_count as f64;
        let length = f64::from(stats.traces.total_weighted_length);
        let java = |key: &[&str]| (after(t1, key), after(t7, key));
        let (j_inc, _) = java(&["connections", "incomplete_count"]);
        let (j_viol, _) = java(&["clearance_violations", "total_count"]);
        let (v1, v7) = java(&["items", "via_count"]);
        let (l1, l7) = java(&["traces", "total_weighted_length"]);
        let within = |value: f64, a: f64, b: f64| {
            let (lo, hi) = (a.min(b), a.max(b));
            value >= lo - 0.02 * lo.abs() && value <= hi + 0.02 * hi.abs()
        };
        let java_stable = java_t1_ses == java_t7_ses;
        let ses_equal = rust_ses.as_bytes() == java_t1_ses.as_slice();
        table.push(format!(
            "{board}: passes {}, vias {vias} (Java {v1}/{v7}), weighted length {length} (Java \
             {l1}/{l7}), incomplete {incomplete} (Java {j_inc}), violations {violations} (Java \
             {j_viol}), Java t1==t7 {java_stable}, SES equal {ses_equal}",
            run.passes
        ));
        if incomplete > j_inc || violations > j_viol {
            failures.push(format!("{board}: more incompletes or violations than Java"));
        }
        if !within(vias, v1, v7) || !within(length, l1, l7) {
            failures.push(format!(
                "{board}: vias or length outside Java's spread + 2 %"
            ));
        }
        if !java_stable {
            // Java's reused worker boards made t1 and t7 differ: only the aggregates count.
            eprintln!("{board}: Java's t1 and t7 sessions differ");
        } else if !ses_equal {
            let out = repo_root().join(format!("target/probe/optimize/{board}.rust.ses"));
            std::fs::create_dir_all(out.parent().expect("a dir")).expect("the dir");
            std::fs::write(&out, &rust_ses).expect("the Rust SES");
            failures.push(format!(
                "{board}: SES differs from Java's t1 ({})",
                out.display()
            ));
        }
    }
    eprintln!("{}", table.join("\n"));
    assert!(
        failures.is_empty(),
        "{}\n{}",
        failures.join("\n"),
        table.join("\n")
    );
}
