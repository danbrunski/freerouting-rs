//! Reachability counters in an isolated process (no other tests reset them).
use freerouting::{
    io::specctra::{BoardReadResult, read_board_from_str},
    run_stats,
};

#[test]
fn compensated_restores_count_requests_including_memo_hits_and_rebuilds() {
    let BoardReadResult::Success { board, .. } = read_board_from_str(include_str!(
        "../../../fixtures/scoring-perfect-two-pin.dsn"
    )) else {
        panic!("fixture")
    };
    let mut board = *board;
    run_stats::enable();
    let counter = run_stats::Counter::CompensatedSnapshotRestores;
    board.snapshot().restore();
    assert_eq!(run_stats::Counts::now().get(counter), 0);
    board.set_clearance_compensation_used(true);
    for java in [true, false] {
        board.set_java_quirks(
            freerouting::settings::JavaQuirks::JAVA
                .with("snapshot_drops_compensation", java)
                .unwrap(),
        );
        let snapshot = board.snapshot();
        for _ in 0..4 {
            snapshot.restore();
        }
        snapshot.rebuild();
    }
    assert_eq!(run_stats::Counts::now().get(counter), 10);
    let text = include_str!("../../../fixtures/scoring-perfect-two-pin.dsn")
        .replace("(unit um)", "(unit mm)")
        .replace("(placement", "(placement (resolution mil 100)")
        .replace("(path pcb 0", "(path pcb 20")
        .replace("(library", "(library (padstack V1 (shape (circle F.Cu 400))) (padstack V2 (shape (circle F.Cu 600)))")
        .replace("(class kicad_default PERFECT\n    )", "(class A PERFECT (circuit (use_via V1))) (class B (circuit (use_via V2))) (class C) (class_class (classes A B C) (rule (clearance 777)))");
    for java in [true, false] {
        run_stats::reset();
        let quirks = [
            "dsn_via_alias",
            "dsn_class_class_pairs",
            "dsn_shape_bbox",
            "dsn_unit_ignored",
        ]
        .into_iter()
        .fold(freerouting::settings::JavaQuirks::JAVA, |q, flag| {
            q.with(flag, java).unwrap()
        });
        assert!(matches!(
            freerouting::io::specctra::read_board_from_str_with(&text, quirks),
            BoardReadResult::Success { .. }
        ));
        for counter in [
            run_stats::Counter::DsnViaAlias,
            run_stats::Counter::DsnClassClassPairs,
            run_stats::Counter::DsnShapeBbox,
            run_stats::Counter::DsnUnitIgnored,
        ] {
            assert!(
                run_stats::Counts::now().get(counter) > 0,
                "{} arm {java}",
                counter.name()
            );
        }
    }
    run_stats::disable();
}

/// Read-only corpus inventory, run separately by the orchestrator; never routes.
#[test]
#[ignore = "N06.10 corpus reachability report; set N06_INPUT_COUNTERS_OUT"]
fn dsn_corpus_branch_inventory() {
    use std::path::{Path, PathBuf};
    fn collect(dir: &Path, paths: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                collect(&path, paths);
            } else if path.extension().is_some_and(|s| s == "dsn") {
                paths.push(path);
            }
        }
    }
    let output = std::env::var("N06_INPUT_COUNTERS_OUT").expect("report path required");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    collect(&root.join("fixtures"), &mut files);
    files.sort();
    let mut rows = Vec::new();
    run_stats::enable();
    for path in files {
        run_stats::reset();
        let board = freerouting::drc::load_design(&std::fs::read(&path).unwrap());
        let counts = run_stats::Counts::now();
        use freerouting::drc::json::JsonValue as J;
        let mut row = vec![
            (
                "board".into(),
                J::string(path.strip_prefix(&root).unwrap().to_str().unwrap()),
            ),
            ("loaded".into(), J::Bool(board.is_ok())),
        ];
        for counter in [
            run_stats::Counter::DsnViaAlias,
            run_stats::Counter::DsnClassClassPairs,
            run_stats::Counter::DsnShapeBbox,
            run_stats::Counter::DsnUnitIgnored,
        ] {
            row.push((counter.name().into(), J::int(counts.get(counter) as i64)));
        }
        rows.push(J::Object(row));
    }
    run_stats::disable();
    assert!(!rows.is_empty());
    std::fs::write(
        output,
        freerouting::drc::json::JsonValue::Array(rows).to_pretty_string(),
    )
    .unwrap();
}
