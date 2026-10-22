//! `freerouting-dev gate --left-out ROW=REASON` (K15.21): the gate judges the
//! rows a registration's validity rule leaves in. Synthetic records of five
//! boards a..e (arms `off`, `on`), a registration written here with a `validity`
//! rule and `must_judge: ["a"]`, and a second arm pair for `--all-pairs`.

use std::path::{Path, PathBuf};
use std::process::Command;

const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOARDS: [&str; 5] = ["a", "b", "c", "d", "e"];
const VALIDITY: &str = "a row whose arms do not all complete is left out of the judgement and \
                        reported by name with its reason";

/// `(off, on)` unrouted per board: `on` improves every board.
const UNROUTED: [(u32, u32); 5] = [(10, 5), (10, 6), (10, 7), (10, 8), (10, 4)];

fn arm_json(name: &str) -> String {
    format!("\"{name}\": {{\"engine\": \"{name}-engine\"}}")
}

fn registration(extra: &str) -> String {
    let files = BOARDS
        .iter()
        .map(|b| format!("\"{b}\": \"{HASH}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{\"format\": \"freerouting-ab-prereg-1\", \"id\": \"t\", \"arms\": {{{}, {}}}, \
         \"baseline_arm\": \"off\", \"candidate_arm\": \"on\", \
         \"boards\": {{\"files\": {{{files}}}}}, \
         \"metrics\": [{{\"name\": \"via_in_pad\", \"role\": \"hard\"}}, {{\"name\": \"unrouted\"}}], \
         \"pass_rule\": {{\"min_boards\": 3}}, \
         \"pairs\": {{\"verdict\": [\"off\", \"on\"], \"reported\": [[\"on\", \"off\"]]}}{extra}}}",
        arm_json("off"),
        arm_json("on"),
    )
}

/// One arm: measured (`Some((via_in_pad, unrouted))`) or ungraded (nulls).
fn arm(v: Option<(u32, u32)>) -> String {
    match v {
        Some((pad, un)) => format!("{{\"via_in_pad\": {pad}, \"unrouted\": {un}}}"),
        None => "{\"via_in_pad\": null, \"unrouted\": null}".to_string(),
    }
}

/// The records; `terminated` rows have both arms ungraded, `breach` rows have
/// one via in a pad in the `on` arm (and are otherwise graded).
fn records(terminated: &[&str], breach: &[&str], skip: &[&str]) -> String {
    let rows = BOARDS
        .iter()
        .zip(UNROUTED)
        .filter(|(b, _)| !skip.contains(b))
        .map(|(b, (off, on))| {
            let (a_off, a_on) = if terminated.contains(b) {
                (arm(None), arm(None))
            } else if breach.contains(b) {
                (arm(Some((0, off))), arm(Some((1, on))))
            } else {
                (arm(Some((0, off))), arm(Some((0, on))))
            };
            format!(
                "\"{b}\": {{\"board\": \"{b}\", \"input_sha256\": \"{HASH}\", \"connections\": 10, \
                 \"arms\": {{\"off\": {a_off}, \"on\": {a_on}}}}}"
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let declared = BOARDS
        .iter()
        .map(|b| format!("\"{b}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{\"format\": \"freerouting-gate-records-1\", \"partial\": false, \
         \"declared\": [{declared}], \"metrics\": [\"via_in_pad\", \"unrouted\"], \
         \"arms\": {{{}, {}}}, \"baseline_arm\": \"off\", \"candidate_arm\": \"on\", \
         \"rows\": {{{rows}}}}}",
        arm_json("off"),
        arm_json("on"),
    )
}

struct Run {
    code: i32,
    out: String,
}

fn gate(args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_freerouting-dev"))
        .arg("gate")
        .args(args)
        .output()
        .unwrap();
    Run {
        code: output.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr),
    }
}

struct Fixture {
    dir: PathBuf,
    records: String,
    prereg: String,
    baseline: String,
}

fn fixture(name: &str, records_text: &str, prereg_extra: &str) -> Fixture {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("gate_left_out")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let put = |file: &str, text: &str| {
        let path = dir.join(file);
        std::fs::write(&path, text).unwrap();
        path.display().to_string()
    };
    Fixture {
        records: put("records.json", records_text),
        prereg: put("prereg.json", &registration(prereg_extra)),
        baseline: dir.join("baseline.json").display().to_string(),
        dir,
    }
}

/// A registration that states a validity rule and the rows that must be judged.
fn stated() -> String {
    format!(", \"validity\": \"{VALIDITY}\", \"must_judge\": [\"a\"]")
}

impl Fixture {
    fn run(&self, with_baseline: bool, extra: &[&str]) -> Run {
        let mut args = vec!["--records", &self.records, "--prereg", &self.prereg];
        if with_baseline {
            args.extend(["--baseline", &self.baseline]);
        }
        args.extend(extra);
        gate(&args)
    }

    fn write_baseline(&self, extra: &[&str]) -> Run {
        let mut args = vec!["--write-baseline"];
        args.extend(extra);
        self.run(true, &args)
    }

    fn baseline_text(&self) -> String {
        std::fs::read_to_string(&self.baseline).unwrap()
    }
}

#[test]
fn a_terminated_row_is_judged_over_the_rest_and_named_in_output_and_baseline() {
    let f = fixture("terminated", &records(&["e"], &[], &[]), &stated());
    // Without the flag the gate still refuses the ungraded row.
    let r = f.write_baseline(&[]);
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.out.contains("UNGRADED e.on.via_in_pad"), "{}", r.out);
    assert!(!Path::new(&f.baseline).exists());
    // With it, the baseline records the rest, and the row and its reason.
    let r = f.write_baseline(&["--left-out", "e=both arms TERMINATED: no session"]);
    assert_eq!(r.code, 0, "{}", r.out);
    let baseline = f.baseline_text();
    assert!(baseline.contains("\"left_out\""), "{baseline}");
    assert!(
        baseline.contains("both arms TERMINATED: no session"),
        "{baseline}"
    );
    assert!(!baseline.contains("\"e\": {\n      \"arms\""), "{baseline}");
    for judged in ["a", "b", "c", "d"] {
        assert!(baseline.contains(&format!("\"{judged}\"")), "{baseline}");
    }
    // The verdict judges a-d and prints the row and the reason.
    let r = f.run(true, &["--left-out", "e=both arms TERMINATED: no session"]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("judged 4 of 5 row(s)"), "{}", r.out);
    assert!(r.out.contains("left out"), "{}", r.out);
    assert!(
        r.out.contains("e: both arms TERMINATED: no session"),
        "{}",
        r.out
    );
    assert!(r.out.contains("gate: PASS"), "{}", r.out);
    // --all-pairs prints it with every pair.
    let r = f.run(
        true,
        &[
            "--all-pairs",
            "--left-out",
            "e=both arms TERMINATED: no session",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.out);
    assert_eq!(
        r.out.matches("e: both arms TERMINATED: no session").count(),
        2,
        "{}",
        r.out
    );
    // The baseline of a run with a row left out is not the baseline of the whole run.
    let r = f.run(true, &[]);
    assert_eq!(r.code, 1, "{}", r.out);
    let other = f.run(true, &["--left-out", "d=slow machine"]);
    assert_eq!(other.code, 1, "{}", other.out);
    assert!(other.out.contains("left out"), "{}", other.out);
    assert!(other.out.contains("DRIFT"), "{}", other.out);
}

#[test]
fn a_missing_row_that_is_not_named_is_still_refused() {
    let f = fixture("missing", &records(&["e"], &[], &["d"]), &stated());
    let r = f.write_baseline(&["--left-out", "e=TERMINATED"]);
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(
        r.out
            .contains("board d is registered but missing from the run"),
        "{}",
        r.out
    );
    // Naming the missing row as left out is the operator's call: accepted.
    let r = f.write_baseline(&[
        "--left-out",
        "e=TERMINATED",
        "--left-out",
        "d=board file lost",
    ]);
    assert_eq!(r.code, 0, "{}", r.out); // a, b, c are left: 3 boards
    assert!(!r.out.contains("missing from the run"), "{}", r.out);
}

#[test]
fn left_out_rows_cannot_take_the_run_below_min_boards() {
    let f = fixture("min_boards", &records(&[], &[], &[]), &stated());
    let r = f.write_baseline(&[]);
    assert_eq!(r.code, 0, "{}", r.out);
    // Three rows left out: two judged boards, below the registration's 3.
    let args = [
        "--left-out",
        "c=x",
        "--left-out",
        "d=y",
        "--left-out",
        "e=z",
    ];
    let r = f.run(true, &args);
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.out.contains("NOT MEASURED"), "{}", r.out);
    assert!(r.out.contains("fewer than 3 distinct"), "{}", r.out);
    assert!(r.out.contains("gate: NOT MEASURED"), "{}", r.out);
    let r = f.write_baseline(&args);
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.out.contains("fewer than 3 distinct"), "{}", r.out);
    // A run too small to begin with is the ordinary refusal, not the left-out rows' doing.
    let r = f.run(true, &["--rows", "a,b", "--left-out", "e=z"]);
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.out.contains("REFUSED"), "{}", r.out);
    assert!(!r.out.contains("NOT MEASURED"), "{}", r.out);
    assert!(r.out.contains("gate: FAIL"), "{}", r.out);
    // Two rows left out keep three boards.
    let r = f.write_baseline(&["--left-out", "d=y", "--left-out", "e=z"]);
    assert_eq!(r.code, 0, "{}", r.out);
    let r = f.run(true, &["--left-out", "d=y", "--left-out", "e=z"]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("judged 3 of 5 row(s)"), "{}", r.out);
}

#[test]
fn a_left_out_row_the_registration_requires_judged_is_not_measured() {
    let f = fixture("must_judge", &records(&[], &[], &[]), &stated());
    let r = f.write_baseline(&[]);
    assert_eq!(r.code, 0, "{}", r.out);
    let r = f.run(true, &["--left-out", "a=TERMINATED"]);
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.out.contains("NOT MEASURED a must be judged"), "{}", r.out);
    assert!(r.out.contains("gate: NOT MEASURED"), "{}", r.out);
    assert!(!r.out.contains("gate: PASS"), "{}", r.out);
    // A baseline of such a run is refused for the same reason.
    let g = fixture("must_judge_baseline", &records(&["a"], &[], &[]), &stated());
    let r = g.write_baseline(&["--left-out", "a=TERMINATED"]);
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.out.contains("NOT MEASURED a must be judged"), "{}", r.out);
    assert!(!Path::new(&g.baseline).exists());
    // Not left out, a is judged (here it is ungraded, so the gate refuses it).
    let r = g.run(true, &[]);
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.out.contains("gate: FAIL"), "{}", r.out);
}

#[test]
fn a_graded_left_out_row_with_a_breach_still_fails() {
    // e's arms are graded, with a via in a pad in `on`; the operator left it out.
    let f = fixture("breach", &records(&[], &["e"], &[]), &stated());
    let r = f.write_baseline(&["--left-out", "e=aborted arm"]);
    assert_eq!(r.code, 0, "{}", r.out);
    let r = f.run(true, &["--left-out", "e=aborted arm"]);
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.out.contains("HARD e.on.via_in_pad = 1"), "{}", r.out);
    assert!(r.out.contains("left out"), "{}", r.out);
    assert!(r.out.contains("gate: FAIL"), "{}", r.out);
    // A breach in the baseline arm of a left-out row fails too (the reported pair's candidate).
    let r = f.run(true, &["--all-pairs", "--left-out", "e=aborted arm"]);
    assert_eq!(r.code, 1, "{}", r.out);
    // An ungraded left-out row with null limits does not.
    let g = fixture("no_breach", &records(&["e"], &[], &[]), &stated());
    g.write_baseline(&["--left-out", "e=TERMINATED"]);
    let r = g.run(true, &["--left-out", "e=TERMINATED"]);
    assert_eq!(r.code, 0, "{}", r.out);
}

#[test]
fn the_flag_is_refused_where_it_cannot_apply() {
    let f = fixture("usage", &records(&["e"], &[], &[]), &stated());
    // No registration.
    let r = gate(&[
        "--records",
        &f.records,
        "--baseline",
        &f.baseline,
        "--left-out",
        "e=x",
    ]);
    assert_eq!(r.code, 2, "{}", r.out);
    assert!(r.out.contains("--left-out needs --prereg"), "{}", r.out);
    // A row the registration does not list.
    let r = f.run(true, &["--left-out", "zz=x"]);
    assert_eq!(r.code, 2, "{}", r.out);
    assert!(r.out.contains("zz is not a registered board"), "{}", r.out);
    // No reason.
    let r = f.run(true, &["--left-out", "e"]);
    assert_eq!(r.code, 2, "{}", r.out);
    assert!(r.out.contains("ROW=REASON"), "{}", r.out);
    let r = f.run(true, &["--left-out", "e= "]);
    assert_eq!(r.code, 2, "{}", r.out);
    // The same row twice.
    let r = f.run(true, &["--left-out", "e=x", "--left-out", "e=y"]);
    assert_eq!(r.code, 2, "{}", r.out);
    // Left out and asked for.
    let r = f.run(true, &["--left-out", "e=x", "--rows", "a,b,c,e"]);
    assert_eq!(r.code, 2, "{}", r.out);
    assert!(r.out.contains("--rows"), "{}", r.out);
    // A registration that states no validity rule.
    let g = fixture("no_validity", &records(&["e"], &[], &[]), "");
    let r = g.run(true, &["--left-out", "e=x"]);
    assert_eq!(r.code, 2, "{}", r.out);
    assert!(r.out.contains("states no `validity` rule"), "{}", r.out);
    let _ = &f.dir;
}

#[test]
fn the_totals_rule_sums_noise_over_the_judged_rows_only() {
    let totals = ", \"rule\": \"totals\", \"totals\": {\"accept\": \"improve\", \"signal\": \
                  [\"unrouted\"], \"noise\": {\"a\": 3, \"b\": 3, \"c\": 3, \"d\": 3, \"e\": 3}}";
    let f = fixture("totals", &records(&["e"], &[], &[]), &(stated() + totals));
    let args = ["--left-out", "e=TERMINATED"];
    let r = f.write_baseline(&args);
    assert_eq!(r.code, 0, "{}", r.out);
    // a-d: sqrt(4 * 9) = 6.00; the left-out e's band is not in the noise.
    let r = f.run(true, &args);
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(
        r.out.contains("set noise sqrt(sum band^2) = 6.00"),
        "{}",
        r.out
    );
    assert!(r.out.contains("e: TERMINATED"), "{}", r.out);
    assert!(r.out.contains("gate: PASS"), "{}", r.out);
    // A left-out row that is missing from the run does not trip the noise check.
    let g = fixture(
        "totals_missing",
        &records(&[], &[], &["e"]),
        &(stated() + totals),
    );
    let r = g.write_baseline(&["--left-out", "e=board lost"]);
    assert_eq!(r.code, 0, "{}", r.out);
    let r = g.run(true, &["--left-out", "e=board lost"]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(
        r.out.contains("e: board lost  [not in the run]"),
        "{}",
        r.out
    );
}
