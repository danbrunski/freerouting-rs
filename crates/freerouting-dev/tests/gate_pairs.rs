//! `freerouting-dev gate --prereg` honours `--arms`, `--rows`, `--eligible` and
//! `--all-pairs`, and refuses what it can't honour (never a silent ignore).
//! Synthetic records of five boards a..e, three arms (`off`, `on`, `alt`) and a
//! registration written here: primary pair off -> on, `pairs` listing the
//! primary pair and off -> alt, `eligible` = a, b, c, d.

use std::path::{Path, PathBuf};
use std::process::Command;

use freerouting_dev::gate_prereg::{PairRole, Prereg};

const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BOARDS: [&str; 5] = ["a", "b", "c", "d", "e"];

/// `(off, on, alt)` unrouted per board: `on` improves a-d and regresses e;
/// `alt` regresses a and improves the rest.
const UNROUTED: [(u32, u32, u32); 5] =
    [(10, 5, 12), (10, 6, 4), (10, 7, 5), (10, 8, 6), (10, 12, 4)];

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
        "{{\"format\": \"freerouting-ab-prereg-1\", \"id\": \"t\", \"arms\": {{{}, {}, {}}}, \
         \"baseline_arm\": \"off\", \"candidate_arm\": \"on\", \
         \"boards\": {{\"files\": {{{files}}}}}, \
         \"metrics\": [{{\"name\": \"via_in_pad\", \"role\": \"hard\"}}, {{\"name\": \"unrouted\"}}], \
         \"pass_rule\": {{\"min_boards\": 3}}, \
         \"pairs\": [[\"off\", \"on\"], [\"off\", \"alt\"]], \
         \"eligible\": [\"a\", \"b\", \"c\", \"d\"]{extra}}}",
        arm_json("off"),
        arm_json("on"),
        arm_json("alt")
    )
}

fn records() -> String {
    let rows = BOARDS
        .iter()
        .zip(UNROUTED)
        .map(|(b, (off, on, alt))| {
            let arm = |n: u32| format!("{{\"via_in_pad\": 0, \"unrouted\": {n}}}");
            format!(
                "\"{b}\": {{\"board\": \"{b}\", \"input_sha256\": \"{HASH}\", \"connections\": 10, \
                 \"arms\": {{\"off\": {}, \"on\": {}, \"alt\": {}}}}}",
                arm(off),
                arm(on),
                arm(alt)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{{\"format\": \"freerouting-gate-records-1\", \"partial\": false, \
         \"metrics\": [\"via_in_pad\", \"unrouted\"], \
         \"arms\": {{{}, {}, {}}}, \"baseline_arm\": \"off\", \"candidate_arm\": \"on\", \
         \"rows\": {{{rows}}}}}",
        arm_json("off"),
        arm_json("on"),
        arm_json("alt")
    )
}

struct Run {
    code: i32,
    out: String,
}

struct Fixture {
    dir: PathBuf,
    records: String,
    prereg: String,
    baseline: String,
}

fn fixture(name: &str, extra: &str) -> Fixture {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("gate_pairs")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let put = |file: &str, text: &str| {
        let path = dir.join(file);
        std::fs::write(&path, text).unwrap();
        path.display().to_string()
    };
    Fixture {
        records: put("records.json", &records()),
        prereg: put("prereg.json", &registration(extra)),
        baseline: dir.join("baseline.json").display().to_string(),
        dir,
    }
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

impl Fixture {
    /// Records the baseline of the primary pair on all rows, as `--write-baseline` does.
    fn with_baseline(self) -> Fixture {
        let r = gate(&[
            "--records",
            &self.records,
            "--baseline",
            &self.baseline,
            "--prereg",
            &self.prereg,
            "--write-baseline",
        ]);
        assert_eq!(r.code, 0, "{}", r.out);
        self
    }

    fn run(&self, with_baseline: bool, extra: &[&str]) -> Run {
        let mut args = vec!["--records", &self.records, "--prereg", &self.prereg];
        if with_baseline {
            args.extend(["--baseline", &self.baseline]);
        }
        args.extend(extra);
        gate(&args)
    }
}

#[test]
fn the_registration_parses_its_pairs_and_eligible_list() {
    let p = Prereg::parse(&registration("")).unwrap();
    assert_eq!(
        p.eligible.as_deref(),
        Some(&["a", "b", "c", "d"].map(String::from)[..])
    );
    let pairs: Vec<(&str, &str, PairRole)> = p
        .pairs
        .iter()
        .map(|x| (x.arms.base.as_str(), x.arms.cand.as_str(), x.role))
        .collect();
    assert_eq!(
        pairs,
        [
            ("off", "on", PairRole::Listed),
            ("off", "alt", PairRole::Listed)
        ]
    );
    let block = registration("").replace(
        "\"pairs\": [[\"off\", \"on\"], [\"off\", \"alt\"]]",
        "\"pairs\": {\"verdict\": {\"arms\": [\"off\", \"on\"], \"rows\": \"all\"}, \
         \"reported\": [[\"off\", \"alt\"]]}",
    );
    let p = Prereg::parse(&block).unwrap();
    assert_eq!(p.pairs[0].role, PairRole::Verdict);
    assert_eq!(p.pairs[1].role, PairRole::Reported);
    // A pair naming an unregistered arm and an eligible row that is no board are malformed.
    assert!(
        Prereg::parse(&registration("").replace("[\"off\", \"alt\"]]", "[\"off\", \"nope\"]]"))
            .is_err()
    );
    assert!(Prereg::parse(&registration("").replace("\"d\"]", "\"zz\"]")).is_err());
}

#[test]
fn prereg_without_flags_judges_the_primary_pair_on_all_rows() {
    let f = fixture("default", "").with_baseline();
    let r = f.run(true, &[]);
    // e regresses: 4 improved, 1 regressed -> fails on all rows.
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.out.contains("judged 5 of 5 row(s)"), "{}", r.out);
    assert!(r.out.contains("pair off -> on"), "{}", r.out);
}

#[test]
fn arms_judges_another_registered_pair_without_the_primary_baseline() {
    let f = fixture("arms", "").with_baseline();
    // off -> alt: a regresses (10 -> 12), so FAIL, and the report names the pair.
    let r = f.run(false, &["--arms", "off,alt"]);
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.out.contains("pair off -> alt"), "{}", r.out);
    assert!(r.out.contains("no baseline comparison"), "{}", r.out);
    assert!(r.out.contains("a [a]: regress"), "{}", r.out);
    // With the eligible rows minus a: off -> alt improves on b, c, d.
    let r = f.run(false, &["--arms", "off,alt", "--rows", "b,c,d"]);
    assert_eq!(r.code, 0, "{}", r.out);
    // The primary pair named with --arms still uses the baseline.
    let r = f.run(true, &["--arms", "off,on", "--eligible"]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(!r.out.contains("no baseline comparison"), "{}", r.out);
}

#[test]
fn arms_that_are_not_registered_fail_naming_the_registered_arms() {
    let f = fixture("arms_bad", "");
    for arms in ["off,nope", "nope,on", "off"] {
        let r = f.run(false, &["--arms", arms]);
        assert_eq!(r.code, 2, "{arms}: {}", r.out);
    }
    let r = f.run(false, &["--arms", "off,nope"]);
    assert!(r.out.contains("registered arms: alt, off, on"), "{}", r.out);
    assert!(r.out.contains("nope"), "{}", r.out);
}

#[test]
fn rows_and_eligible_restrict_the_judged_rows() {
    let f = fixture("rows", "").with_baseline();
    let r = f.run(true, &["--rows", "a,b,c,d"]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("judged 4 of 5 row(s)"), "{}", r.out);
    assert!(!r.out.contains("e [e]"), "{}", r.out);
    let r = f.run(true, &["--eligible"]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("judged 4 of 5 row(s)"), "{}", r.out);
}

#[test]
fn min_boards_counts_the_judged_boards() {
    let f = fixture("min_boards", "").with_baseline();
    let r = f.run(true, &["--rows", "a,b"]);
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.out.contains("2 distinct board"), "{}", r.out);
}

#[test]
fn totals_noise_is_summed_over_the_judged_rows_only() {
    let totals = ", \"rule\": \"totals\", \"totals\": {\"accept\": \"improve\", \"signal\": \
                  [\"unrouted\"], \"noise\": {\"a\": 3, \"b\": 3, \"c\": 3, \"d\": 3, \"e\": 3}}";
    let f = fixture("totals", totals).with_baseline();
    // All five boards: sqrt(5 * 9) = 6.71; the four eligible: sqrt(36) = 6.00.
    let all = f.run(true, &[]);
    assert!(
        all.out.contains("set noise sqrt(sum band^2) = 6.71"),
        "{}",
        all.out
    );
    let eligible = f.run(true, &["--eligible"]);
    assert!(
        eligible.out.contains("set noise sqrt(sum band^2) = 6.00"),
        "{}",
        eligible.out
    );
    assert_eq!(eligible.code, 0, "{}", eligible.out);
}

#[test]
fn all_pairs_prints_each_verdict_and_fails_if_a_counted_pair_fails() {
    let f = fixture("all_pairs", "").with_baseline();
    let r = f.run(true, &["--all-pairs", "--eligible"]);
    // off -> on passes on the eligible rows, off -> alt fails (a regresses).
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.out.contains("pair off -> on: PASS"), "{}", r.out);
    assert!(r.out.contains("pair off -> alt: FAIL"), "{}", r.out);
    assert!(r.out.contains("gate: FAIL"), "{}", r.out);
    // Without a failing pair the gate passes.
    let r = f.run(true, &["--all-pairs", "--rows", "b,c,d"]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("pair off -> alt: PASS"), "{}", r.out);
}

#[test]
fn a_reported_pair_is_printed_but_does_not_decide_the_exit_status() {
    let block = "\"pairs\": {\"verdict\": [\"off\", \"on\"], \"reported\": [[\"off\", \"alt\"]]}";
    let mut text = registration("");
    text = text.replace("\"pairs\": [[\"off\", \"on\"], [\"off\", \"alt\"]]", block);
    let f = fixture("reported", "");
    std::fs::write(&f.prereg, text).unwrap();
    let f = f.with_baseline();
    let r = f.run(true, &["--all-pairs", "--eligible"]);
    assert_eq!(r.code, 0, "{}", r.out);
    assert!(r.out.contains("pair off -> alt: FAIL"), "{}", r.out);
    assert!(
        r.out.contains("off -> alt (reported, not counted): FAIL"),
        "{}",
        r.out
    );
}

#[test]
fn unsupported_combinations_are_errors_not_silent() {
    let f = fixture("errors", "").with_baseline();
    let usage = |args: &[&str], needle: &str| {
        let r = f.run(true, args);
        assert_eq!(r.code, 2, "{args:?}: {}", r.out);
        assert!(r.out.contains(needle), "{args:?}: {}", r.out);
    };
    usage(&["--rows", "a", "--eligible"], "use one");
    usage(&["--all-pairs", "--arms", "off,on"], "use one");
    usage(&["--all-pairs", "--write-baseline"], "--all-pairs");
    // A baseline can't be compared with a pair it does not record.
    usage(&["--arms", "off,alt"], "primary pair");
    // --eligible on a registration without the list.
    let g = fixture("errors_no_eligible", "");
    let text = std::fs::read_to_string(&g.prereg)
        .unwrap()
        .replace(", \"eligible\": [\"a\", \"b\", \"c\", \"d\"]", "");
    std::fs::write(&g.prereg, text).unwrap();
    let r = g.run(false, &["--eligible", "--arms", "off,alt"]);
    assert_eq!(r.code, 2, "{}", r.out);
    assert!(r.out.contains("no `eligible` list"), "{}", r.out);
    // --eligible and --all-pairs need a registration.
    for flag in ["--eligible", "--all-pairs"] {
        let r = gate(&["--records", &f.records, "--baseline", &f.baseline, flag]);
        assert_eq!(r.code, 2, "{flag}: {}", r.out);
        assert!(r.out.contains("needs --prereg"), "{}", r.out);
    }
    // A row asked for that the run does not have is a refusal, not an empty judge.
    let r = f.run(true, &["--rows", "a,b,c,zz"]);
    assert_eq!(r.code, 1, "{}", r.out);
    assert!(r.out.contains("row zz was asked for"), "{}", r.out);
    // A baseline for a non-primary pair can't be written.
    let r = f.run(true, &["--arms", "off,alt", "--write-baseline"]);
    assert_eq!(r.code, 2, "{}", r.out);
    let _ = &f.dir;
}

#[test]
fn a_pair_that_names_its_rows_asks_for_rows() {
    let rows_pair = registration("").replace(
        "\"pairs\": [[\"off\", \"on\"], [\"off\", \"alt\"]]",
        "\"pairs\": [{\"arms\": [\"off\", \"on\"], \"rows\": [\"a\", \"b\", \"c\"]}]",
    );
    let f = fixture("pair_rows", "").with_baseline();
    std::fs::write(&f.prereg, rows_pair).unwrap();
    let r = f.run(true, &[]);
    assert_eq!(r.code, 2, "{}", r.out);
    assert!(
        r.out.contains("name the rows to judge with --rows"),
        "{}",
        r.out
    );
    let r = f.run(true, &["--rows", "a,b,c"]);
    assert_eq!(r.code, 0, "{}", r.out);
}

#[test]
fn arms_without_a_registration_override_the_records_own_arms() {
    // The records say off -> on; --arms off,alt must win, not be ignored.
    let f = fixture("no_prereg", "");
    let w = gate(&[
        "--records",
        &f.records,
        "--baseline",
        &f.baseline,
        "--arms",
        "off,alt",
        "--write-baseline",
    ]);
    assert_eq!(w.code, 0, "{}", w.out);
    let r = gate(&[
        "--records",
        &f.records,
        "--baseline",
        &f.baseline,
        "--arms",
        "off,alt",
    ]);
    assert!(r.out.contains("rows (arm off -> alt)"), "{}", r.out);
}
