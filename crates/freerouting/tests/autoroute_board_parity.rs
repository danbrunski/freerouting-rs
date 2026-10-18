//! A14: the board-parity gate (rung 3 of the verification ladder).
//!
//! Every board with a Java route reference (`fixtures/reference/route/<b>.json`, J01 at
//! `a773a70ec`) is loaded through the job's load sequence (C03) with J01's `route` mode
//! settings (the optimizer off, the JSON's `maxPasses` and `maxItems` at the testing
//! priority), routed with `run_pipeline` (A13), written with the SES writer (C05) to
//! `target/parity/<b>.ses` and compared with Java's session by `ses-diff`'s comparison
//! (F02.2, the library code of `crates/freerouting-dev`, included here by path): the same
//! wires and vias per net, as sets, and the same statistics after importing each.
//!
//! Boards are classed from the JSON (A14's decisions):
//! - `exact`: Java `completed` with zero `workLimitCutoffs`; the sessions must be equivalent,
//!   the per-pass score lines must equal Java's (A14.4), or the board is listed in
//!   [`EXPECTED_MISMATCHES`] with its reason and a DIVERGENCES line;
//! - `cutoff`: one or more Java work-limit cut-offs; compared on aggregates until B02's
//!   tree-shape proof (incomplete connections and clearance violations no worse than Java's,
//!   vias and trace length within ±5 %, and no worse summed over the class);
//! - `java-failure`: Java's `result` is not `completed`, or the board is in
//!   [`JAVA_FAILURES`]; compared on the recorded outcome (A14.5).
//!
//! On every routed board the D8 owner rules are checked as well: no via the router added
//! overlaps an SMD pad of its layer unless its padstack allows attach, and no SMD pad of a
//! plane net that reaches the plane is chained by new traces to another SMD pad of its net
//! (each gets its own via), and no new trace lies on a power layer unless its net has a plane
//! there (Q01.7, Java `7f56b062d`).
//!
//! The gate routes 118 boards and is `#[ignore]`:
//!
//! ```text
//! cargo test --release -p freerouting --test autoroute_board_parity -- --ignored \
//!     autoroute_board_parity --test-threads=1 --nocapture
//! ```
//!
//! It writes `target/parity/summary.md` and `summary.json` and one record per board in
//! `target/parity/boards/`. Environment:
//! - `PARITY_OUT=<dir>`: the output directory instead of `target/parity`;
//! - `PARITY_BOARDS=a,b,...`: only these boards (names without `.dsn`);
//! - `PARITY_SET=<file>`: only the boards of a `tools/bench/sets` file;
//! - `PARITY_SUBDIRS=1`: the references in subdirectories of `fixtures/reference/route` too
//!   (`canary-pwr/`, the multi-board issues), named by their path below it
//!   (`canary-pwr/canary-pwr-pours-as-planes`); `PARITY_BOARDS` takes those names;
//! - `PARITY_MAX_JAVA_S=<s>`: only the boards whose Java route took at most `s` seconds;
//! - `PARITY_RESUME=1`: reuse the record of a board already routed into `PARITY_OUT`;
//! - `PARITY_SINK=counters`: route with a listening sink that counts events (A04's sink
//!   requirement at the SES level: the SES files must equal the `noop` run's).
//! - `PARITY_AUDIT=1`: route with P02.1's incremental search-tree audit
//!   (`router.debug.audit_incremental`); a board whose audit finds a difference fails with
//!   `audit_difference` and the first difference.

#[path = "../../freerouting-dev/src/session_geometry.rs"]
#[allow(dead_code)]
mod session_geometry;

#[path = "../../freerouting-dev/src/ses_diff.rs"]
#[allow(dead_code, clippy::wrong_self_convention)]
mod ses_diff;

mod board_parity;

#[path = "autoroute_common/mod.rs"]
mod autoroute_common;
#[path = "support/fanout_lines.rs"]
mod fanout_lines;
#[path = "support/insert_lines.rs"]
mod insert_lines;
#[path = "support/pass_lines.rs"]
mod pass_lines;

use std::path::{Path, PathBuf};

use board_parity::{
    BoardRecord, Class, Reference, Sink, Verdict, class_check, compare_sessions, references,
    route_board, write_summary,
};

/// `exact` boards whose session differs from Java's, with the reason. Each has a
/// DIVERGENCES line with the same reason.
const EXPECTED_MISMATCHES: &[(&str, &str)] = &[
    (
        "Issue684-Autorouter_PCB1_2026-5-8",
        "the board quotes its class nets with `'` (`(class $1N4406 '$1N4406' ...)`); Java keeps \
         the quotes, finds no such nets and leaves them in the default class, the port's DSN \
         reader accepts `'` as a quote and puts them in their classes (DIVERGENCES, \
         freerouting-dsn), so their traces get another clearance class: the fanout's first \
         trace (pin 23) has Java's corners and widths but clearance class 8 where Java's has 1, \
         and nets \
         `$1N4406` and `$1N7529` end up routed differently",
    ),
    (
        "Issue721-Autorouter_CE2632_HarryMu_2026-6-15",
        "the board quotes its class nets with `'`; Java keeps the quotes and leaves the nets \
         in the default class, the port reads them into their classes (DIVERGENCES, \
         freerouting-dsn and B02.2), so the clearance classes differ from the load on and \
         the route hash already differs before the fanout's first pin",
    ),
];

/// Boards whose Java route did not complete normally (A14.5): `(board, the outcome both
/// engines give, the reason)`. The outcome is compared with the Rust run's.
const JAVA_FAILURES: &[(&str, &str, &str)] = &[
    (
        "Issue006-LPC18XX_43XX_SCH",
        "load_error",
        "not a PCB file: Java's load fails (`parse_error`) and its route job ends \
         `terminated`; Rust's `prepare_job` rejects the file",
    ),
    (
        "Issue179-Autorouter_PCB1_2023-3-24",
        "completed",
        "Java throws a NullPointerException (`Cannot read field \"edgeLines\"`) in the \
         Delaunay triangulation of the incomplete connections and writes no session; the \
         port skips the corner outside the bounding triangle (DIVERGENCES, Board \
         statistics) and routes the board, so there is nothing to compare it with",
    ),
    (
        "scoring-perfect-two-pin",
        "completed",
        "Java completes with the one connection unrouted: the board has no via padstack, so \
         `AutorouteControl` throws on the null via rule and the connection is FAILED every \
         pass (A06.6); Rust fails at the same step, so the session is compared exactly",
    ),
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn out_dir() -> PathBuf {
    match std::env::var_os("PARITY_OUT").map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir,
        Some(dir) => repo_root().join(dir),
        None => repo_root().join("target/parity"),
    }
}

/// The boards selected by `PARITY_BOARDS` and `PARITY_SET`, or all.
fn selected(all: Vec<Reference>) -> Vec<Reference> {
    let mut names: Option<Vec<String>> = std::env::var("PARITY_BOARDS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect());
    if let Ok(set) = std::env::var("PARITY_SET") {
        let text = std::fs::read_to_string(repo_root().join(&set))
            .unwrap_or_else(|e| panic!("{set}: {e}"));
        let from_set = text
            .lines()
            .map(|l| l.split('#').next().unwrap_or("").trim())
            .filter(|l| !l.is_empty())
            .filter_map(|l| Path::new(l).file_stem()?.to_str().map(str::to_string));
        names.get_or_insert_with(Vec::new).extend(from_set);
    }
    let max_ms = std::env::var("PARITY_MAX_JAVA_S")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .map(|s| s * 1000);
    all.into_iter()
        .filter(|r| names.as_ref().is_none_or(|n| n.contains(&r.name)))
        .filter(|r| max_ms.is_none_or(|max| r.java_ms.unwrap_or(0) <= max))
        .collect()
}

/// The gate (A14.1-A14.5).
#[test]
#[ignore = "routes every fixture with a route reference (hours in release)"]
fn autoroute_board_parity() {
    let out = out_dir();
    let resume = std::env::var("PARITY_RESUME").is_ok_and(|v| v == "1");
    let n06_counters = std::env::var("PARITY_N06_COUNTERS").is_ok_and(|v| v == "1");
    if n06_counters {
        assert!(
            !resume,
            "N06 counters require fresh routing, not cached records"
        );
        freerouting::run_stats::enable();
    }
    let sink = match std::env::var("PARITY_SINK").as_deref() {
        Ok("counters") => Sink::Counters,
        _ => Sink::Noop,
    };
    let mut boards = selected(references(&repo_root(), JAVA_FAILURES));
    assert!(!boards.is_empty(), "no board selected");
    // The quick boards first, so a long run reports early; the summary is in name order.
    boards.sort_by_key(|r| (r.java_ms.unwrap_or(0), r.name.clone()));
    let mut records = Vec::new();
    for reference in &boards {
        let cached = if resume {
            BoardRecord::read(&out, &reference.name)
        } else {
            None
        };
        let record = match cached {
            Some(record) => record,
            None => {
                eprintln!("parity: routing {} ({:?})", reference.name, reference.class);
                let record = route_board(&repo_root(), reference, &out, sink);
                record.write(&out);
                record
            }
        };
        eprintln!("parity: {}", record.one_line());
        records.push(record);
    }
    if n06_counters {
        use freerouting::run_stats::{self, Counter, Counts};
        let reallocations = Counts::now().get(Counter::DoorSectionReallocations);
        let report = format!(
            "{{\"door_section_reallocations\":{reallocations},\"boards\":{}}}\n",
            boards.len()
        );
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("n06-counters.json"), report).unwrap();
        eprintln!(
            "N06: door_section_reallocations={reallocations} across {} boards",
            boards.len()
        );
        run_stats::disable();
    }
    records.sort_by(|a, b| a.name.cmp(&b.name));
    let verdicts = class_check(&mut records, EXPECTED_MISMATCHES, JAVA_FAILURES);
    write_summary(&out, &records, &verdicts);
    let failing: Vec<String> = records
        .iter()
        .filter(|r| r.verdict == Verdict::Fail)
        .map(|r| format!("{} ({:?}): {}", r.name, r.class, r.reason))
        .collect();
    let mut problems = failing;
    problems.extend(verdicts.iter().filter(|v| !v.ok).map(|v| v.text.clone()));
    assert!(
        problems.is_empty(),
        "{} problem(s), see {}:\n{}",
        problems.len(),
        out.join("summary.md").display(),
        problems.join("\n")
    );
}

/// A14.1's self-test: a reference copy with one wire removed is reported as different,
/// with the wire's net.
#[test]
fn a_removed_wire_is_reported_with_its_net() {
    use freerouting::dsn::ast::WiringItem;
    let root = repo_root();
    let reference = references(&root, JAVA_FAILURES)
        .into_iter()
        .find(|r| r.name == "Issue026-J2_reference")
        .expect("J2 has a route reference");
    assert_eq!(reference.class, Class::Exact);
    let java =
        std::fs::read_to_string(root.join("fixtures/reference/route/Issue026-J2_reference.ses"))
            .expect("Java's session");
    let mut edited = ses_diff::parse_session(&java).expect("parses");
    let route = edited.routes.as_mut().expect("the session has routes");
    let net = route
        .network_out
        .iter_mut()
        .find_map(|net| {
            let at = net
                .items
                .iter()
                .position(|i| matches!(i, WiringItem::Wire(_)))?;
            net.items.remove(at);
            Some(net.name.clone())
        })
        .expect("the session has a wire");
    let board = board_parity::load(&root, &reference)
        .expect("J2 loads")
        .board;
    let java_session = ses_diff::parse_session(&java).expect("parses");
    let comparison = compare_sessions(&board, &java_session, &edited);
    assert!(!comparison.equivalent, "the edited copy must differ");
    assert_eq!(comparison.differing_nets, vec![net.clone()]);
    assert_eq!(comparison.net_count, 1);
    // The same session compares equivalent.
    let same = compare_sessions(&board, &java_session, &java_session);
    assert!(same.equivalent);
    assert!(same.differing_nets.is_empty());
    // And a record built from the comparison names the net in the summary.
    let out = root.join(format!("target/parity-selftest-{}", std::process::id()));
    let record = BoardRecord::from_comparison(&reference, comparison);
    record.write(&out);
    let read = BoardRecord::read(&out, &reference.name).expect("the record reads back");
    assert_eq!(read.differing_nets, vec![net.clone()]);
    let mut records = vec![read];
    records.sort_by(|a, b| a.name.cmp(&b.name));
    let verdicts = class_check(&mut records, &[], JAVA_FAILURES);
    assert_eq!(records[0].verdict, Verdict::Fail);
    write_summary(&out, &records, &verdicts);
    let summary = std::fs::read_to_string(out.join("summary.md")).expect("summary.md");
    assert!(summary.contains(&net), "{summary}");
    let _ = std::fs::remove_dir_all(&out);
}

/// A bisecting aid (not a check): writes the Rust `fanout` probe lines of
/// `PARITY_PROBE_DSN` (a path below the repository root) to `PARITY_PROBE_OUT`, to diff
/// with the output of `AutorouteProbe.java fanout` on a board outside the probe's row.
#[test]
#[ignore = "a debugging aid; needs PARITY_PROBE_DSN and PARITY_PROBE_OUT"]
fn fanout_probe_lines() {
    let dsn = std::env::var("PARITY_PROBE_DSN").expect("PARITY_PROBE_DSN");
    let out = std::env::var("PARITY_PROBE_OUT").expect("PARITY_PROBE_OUT");
    let mut lines = fanout_lines::fanout_lines("fanout", &dsn);
    // `PARITY_PROBE_ITEMS=1`: the route items after the fanout as well, as the probe's
    // `item` lines (Java's `itemFields`), to find the field behind a different hash.
    if std::env::var("PARITY_PROBE_ITEMS").is_ok() {
        let mut loaded = fanout_lines::load_for_probe(&dsn, None);
        fanout_lines::run_fanout_lines(&mut loaded, &mut Vec::new());
        let board = &loaded.board;
        let mut items: Vec<_> = board
            .items()
            .filter(|i| i.is_trace() || i.is_via())
            .collect();
        items.sort_by_key(|i| i.id().0);
        lines.extend(items.into_iter().map(|item| {
            let mut fields = vec![("item", item.id().0.into())];
            fields.extend(insert_lines::item_fields(board, item));
            freerouting::autoroute::probe::record_line(&fields)
        }));
    }
    std::fs::write(&out, lines.join("\n") + "\n").expect("PARITY_PROBE_OUT");
}

/// A bisecting aid (not a check): the fanout stage, then `PARITY_PROBE_PASSES` passes of
/// `PARITY_PROBE_DSN` in the `pass` probe's line format, to `PARITY_PROBE_OUT`.
#[test]
#[ignore = "a debugging aid; needs PARITY_PROBE_DSN, PARITY_PROBE_PASSES and PARITY_PROBE_OUT"]
fn fanout_pass_probe_lines() {
    let dsn = std::env::var("PARITY_PROBE_DSN").expect("PARITY_PROBE_DSN");
    let out = std::env::var("PARITY_PROBE_OUT").expect("PARITY_PROBE_OUT");
    let passes = std::env::var("PARITY_PROBE_PASSES")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(2);
    let lines = pass_lines::fanout_pass_lines(&dsn, passes);
    std::fs::write(&out, lines.join("\n") + "\n").expect("PARITY_PROBE_OUT");
}

/// Java's `fanout` probe lines of `Issue508-DAC2020_bm10` at `a773a70ec`
/// (`AutorouteProbe.java fanout`, A14 bisection): pin 63, where the port used to stop
/// matching, and the pass line, with the board hash after the whole fanout.
const BM10_JAVA_PIN_63: &str = r#"{"pin":63,"component":45,"item":216,"net":1,"state":"ROUTED","strict":false,"maxIdBefore":1359,"maxIdAfter":1379,"added":7,"addedHash":"d432f309175b2cf0","boardHash":"8032abd7b0d3e6ef","units":220432}"#;
const BM10_JAVA_PASS: &str = r#"{"pass":0,"routed":242,"notRouted":0,"insertErrors":0,"alreadyConnected":0,"extraVias":127,"maxGeneratedId":3394,"boardHash":"bebac8d8ebae2dca"}"#;

/// A14 regression: Java's `FoundConnectionInserter` connects the new path's ends to its
/// start and target traces through the Java objects it located, also after the insertion
/// split or removed them (`:108-137`); the port skipped the connection when the trace id was
/// gone, so bm10's fanout differed from pin 63 on (and bm01's third pass). The fanout now
/// equals Java's line by line, except the work units of pins 119 and 170, whose pull-tight
/// counts one unit less (unexplained, see the A14 checklist).
#[test]
fn bm10_fanout_connects_to_the_removed_start_trace_like_java() {
    let dsn = repo_root().join("fixtures/Issue508-DAC2020_bm10.dsn");
    let lines = fanout_lines::fanout_lines("fanout", dsn.to_str().expect("a UTF-8 path"));
    let pin_63 = lines
        .iter()
        .find(|l| l.starts_with(r#"{"pin":63,"#))
        .expect("pin 63");
    assert_eq!(pin_63, BM10_JAVA_PIN_63);
    let pass = lines
        .iter()
        .find(|l| l.starts_with(r#"{"pass":0,"#))
        .expect("the pass line");
    assert_eq!(pass, BM10_JAVA_PASS);
}

/// N01.2: the gate pins `router.profile = legacy`. With a dummy difference in `enhanced`
/// (the fanout's pass limit), J2 still loads with Java's settings and routes to Java's
/// session; the same harness without the pin would take the dummy and is refused.
#[test]
fn the_gate_pins_legacy_beside_a_dummy_enhanced_difference() {
    use freerouting::job::prepare::prepare_job;
    use freerouting::settings::ResolveContext;
    use freerouting::settings::profile::{Difference, require_legacy, with_enhanced_differences};

    const DUMMY: &[Difference] = &[Difference {
        path: "fanout.max_passes",
        value: "7",
        measurement: "test-only dummy (N01.2)",
    }];
    let root = repo_root();
    let reference = references(&root, JAVA_FAILURES)
        .into_iter()
        .find(|r| r.name == "Issue026-J2_reference")
        .expect("J2 has a route reference");
    assert_eq!(reference.class, Class::Exact);
    let plain = board_parity::load(&root, &reference).expect("J2 loads");
    let out = root.join(format!(
        "target/parity-profile-selftest-{}",
        std::process::id()
    ));
    with_enhanced_differences(DUMMY, || {
        let pinned = board_parity::load(&root, &reference).expect("J2 loads pinned");
        assert_eq!(
            pinned.settings, plain.settings,
            "the pin keeps Java's settings"
        );
        assert_eq!(pinned.settings.fanout.max_passes, 20);

        // Without the pin the dummy would reach the gate, and `load` refuses it.
        let mut input = board_parity::job_input(&root, &reference).expect("the input");
        for (_, patch) in &mut input.extra_sources {
            patch.profile = None;
        }
        let unpinned = prepare_job(&input, &ResolveContext { cores: 8 }).expect("loads");
        assert_eq!(unpinned.settings.fanout.max_passes, 7, "the dummy is live");
        assert!(require_legacy(&unpinned.settings).is_err());

        // The routed session still equals Java's.
        let record = route_board(&root, &reference, &out, Sink::Noop);
        assert_eq!(record.rust_result, "completed", "{}", record.error);
        assert!(
            record.equivalent && record.byte_equal,
            "J2 under the pin: {}",
            record.one_line()
        );
    });
    let _ = std::fs::remove_dir_all(&out);
}

/// Q13.2: the D8 report's power-layer lines flag a new trace of a plane's own net that
/// leaves its plane (`trace of <net> outside its plane on power layer <layer>`), on the
/// canary-pwr sessions that showed it, and pass on the control.
///
/// - The default-options KiCad plugin run at canary `e21ee05e` (profile `enhanced`, which
///   equals `legacy` there), against its input with the earlier routing taken out
///   (`plugin-e21ee05e-unrouted.dsn`): `+5V_PI` on Power.Cu from (106.66, 56.44) to its
///   island's border at (107.89, 54.99). The island is x 107.3-112.2, y 45.2-55.0.
/// - canary-7a's placement run a+b, add-only by Java `7f56b062d`: `+5V_PI` on Power.Cu
///   along (108.2, 53.2) -> (105.7, 53.9).
/// - The control: Q10's pass-1 session of `canary-pwr-9511d9a.dsn`, which KiCad grades at
///   45 with no inner-layer track (`inner_tracks.py` 0).
///
/// Run a's tracks are fixed in its add-only input, so they are not new; the engine's
/// check finds them (`tests/power_layer_traces.rs`).
#[test]
fn d8_flags_a_plane_net_trace_outside_its_plane() {
    use freerouting::io::specctra::import_session;

    const DATA: &str = "crates/freerouting/tests/data/canary-pwr-q13";
    let root = repo_root();
    let lines = |dsn: &str, ses: &str| {
        let input = autoroute_common::load(dsn).board;
        let text = std::fs::read_to_string(root.join(ses)).expect("the session");
        let session = ses_diff::parse_session(&text).expect("the session parses");
        let mut routed = input.clone();
        import_session(&mut routed, &session);
        board_parity::d8_breaches(&routed, &input)
            .into_iter()
            .filter(|line| line.starts_with("trace of "))
            .collect::<Vec<_>>()
    };
    let outside = "trace of +5V_PI outside its plane on power layer Power.Cu".to_string();
    let plugin = lines(
        &format!("{DATA}/plugin-e21ee05e-unrouted.dsn"),
        &format!("{DATA}/plugin-e21ee05e.ses"),
    );
    assert_eq!(plugin, std::slice::from_ref(&outside), "the plugin run");
    let ab = lines(
        &format!("{DATA}/exp-ab-addonly.dsn"),
        &format!("{DATA}/exp-ab-addonly.7f56b062d.ses"),
    );
    assert_eq!(ab, [outside], "placement run a+b");
    let control = lines(
        "fixtures/canary-pwr/canary-pwr-9511d9a.dsn",
        &format!("{DATA}/9511d9a-q10-mp1.ses"),
    );
    assert_eq!(control, Vec::<String>::new(), "the control");
}
