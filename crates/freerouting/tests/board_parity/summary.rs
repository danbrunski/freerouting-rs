//! The A14 gate's verdicts, the class rules and the summary files.

use super::*;

// ---------------------------------------------------------------------------
// Verdicts and the summary
// ---------------------------------------------------------------------------

/// A class-level rule's result.
#[derive(Clone, Debug)]
pub struct ClassVerdict {
    pub ok: bool,
    pub text: String,
}

/// Rust's D8 breaches that Java's session does not have.
fn new_d8(record: &BoardRecord) -> Vec<&String> {
    record
        .d8_rust
        .iter()
        .filter(|b| !record.d8_java.contains(b))
        .collect()
}

/// Rust within the `cutoff` tolerance of Java: `None` if within, else why not.
fn aggregate_breach(java: &[f64; 5], rust: &[f64; 5]) -> Option<String> {
    let mut why = Vec::new();
    for (i, label) in [(0, "incomplete"), (1, "violations")] {
        if rust[i] > java[i] {
            why.push(format!("{label} {} > {}", rust[i], java[i]));
        }
    }
    for (i, label) in [(2, "vias"), (3, "length")] {
        if (rust[i] - java[i]).abs() > TOLERANCE * java[i] + 1e-9 {
            why.push(format!("{label} {:.2} vs {:.2} (> 5 %)", rust[i], java[i]));
        }
    }
    (!why.is_empty()).then(|| why.join(", "))
}

fn judge(record: &mut BoardRecord, expected: &[(&str, &str)], failures: &[(&str, &str, &str)]) {
    let listed = expected.iter().find(|e| e.0 == record.name).map(|e| e.1);
    let mut why: Vec<String> = Vec::new();
    match record.class {
        Class::JavaFailure => {
            let entry = failures.iter().find(|f| f.0 == record.name);
            let want = entry.map_or(record.java_result.as_str(), |f| f.1);
            if record.rust_result != want {
                why.push(format!("outcome {} (expected {want})", record.rust_result));
            }
            if record.rust_result == "completed" && record.java.is_some() && !record.equivalent {
                why.push(format!("{} net(s) differ", record.net_count));
            }
        }
        Class::Exact | Class::Cutoff if record.rust_result != "completed" => {
            why.push(format!("not routed: {}", record.error));
        }
        Class::Exact => {
            if !record.equivalent {
                why.push(format!(
                    "{} net(s) differ ({})",
                    record.net_count,
                    record.differing_nets.join(", ")
                ));
            }
            if let Some(pass) = record.first_pass_diff {
                why.push(format!("pass {pass} differs"));
            }
        }
        Class::Cutoff => match (&record.java, &record.rust) {
            (Some(java), Some(rust)) => why.extend(aggregate_breach(java, rust)),
            _ => why.push("no statistics".to_string()),
        },
    }
    let d8 = new_d8(record);
    if !d8.is_empty() {
        why.push(format!("D8: {}", d8.len()));
    }
    (record.verdict, record.reason) = match (why.is_empty(), listed) {
        (true, _) => (Verdict::Pass, String::new()),
        (false, Some(reason)) => (Verdict::Expected, format!("{} [{reason}]", why.join("; "))),
        (false, None) => (Verdict::Fail, why.join("; ")),
    };
}

/// Judges every record and checks the class-level rules (the `cutoff` class summed).
pub fn class_check(
    records: &mut [BoardRecord],
    expected: &[(&str, &str)],
    failures: &[(&str, &str, &str)],
) -> Vec<ClassVerdict> {
    for record in records.iter_mut() {
        judge(record, expected, failures);
    }
    for (name, _) in expected {
        if !records.iter().any(|r| r.name == *name) {
            continue;
        }
        if records
            .iter()
            .any(|r| r.name == *name && r.verdict == Verdict::Pass)
        {
            eprintln!("parity: {name} is listed as a mismatch but passes; drop the entry");
        }
    }
    let mut java = [0.0; 5];
    let mut rust = [0.0; 5];
    let mut boards = 0;
    for r in records.iter().filter(|r| r.class == Class::Cutoff) {
        if let (Some(j), Some(s)) = (&r.java, &r.rust) {
            boards += 1;
            for i in 0..5 {
                java[i] += j[i];
                rust[i] += s[i];
            }
        }
    }
    if boards == 0 {
        return Vec::new();
    }
    let breach = aggregate_breach(&java, &rust);
    vec![ClassVerdict {
        ok: breach.is_none(),
        text: format!(
            "cutoff class summed over {boards} boards: java {} rust {}{}",
            fmt_stats(&java),
            fmt_stats(&rust),
            breach.map_or(String::new(), |b| format!(": {b}"))
        ),
    }]
}

fn fmt_stats(s: &[f64; 5]) -> String {
    format!(
        "{} unrouted, {} violations, {} vias, {:.2} mm, {} bends",
        s[0], s[1], s[2], s[3], s[4]
    )
}

fn cell(s: &Option<[f64; 5]>, i: usize) -> String {
    s.map_or("-".to_string(), |v| {
        if i == 3 {
            format!("{:.2}", v[i])
        } else {
            format!("{}", v[i])
        }
    })
}

/// Writes `summary.md` and `summary.json` to `dir`.
pub fn write_summary(dir: &Path, records: &[BoardRecord], verdicts: &[ClassVerdict]) {
    std::fs::create_dir_all(dir).expect("the output directory");
    let mut md = String::from("# A14 board-parity summary\n\n");
    for class in [Class::Exact, Class::Cutoff, Class::JavaFailure] {
        let of: Vec<&BoardRecord> = records.iter().filter(|r| r.class == class).collect();
        let count = |v: Verdict| of.iter().filter(|r| r.verdict == v).count();
        let _ = writeln!(
            md,
            "- {}: {} boards, {} pass, {} expected, {} fail, {} SES byte-identical, {} equivalent",
            class.name(),
            of.len(),
            count(Verdict::Pass),
            count(Verdict::Expected),
            count(Verdict::Fail),
            of.iter().filter(|r| r.byte_equal).count(),
            of.iter().filter(|r| r.equivalent).count(),
        );
    }
    let total: f64 = records.iter().map(|r| r.rust_seconds).sum();
    let _ = writeln!(md, "- Rust route time summed: {total:.1} s");
    let d8: usize = records.iter().map(|r| new_d8(r).len()).sum();
    let _ = writeln!(md, "- D8 breaches Rust adds beyond Java's: {d8}");
    let fired: Vec<String> = records
        .iter()
        .filter(|r| !r.last_mile.is_empty())
        .map(|r| format!("{} {:?}", r.name, r.last_mile))
        .collect();
    let _ = writeln!(md, "- last-mile ripup fired: {}", fired.join(", "));
    for v in verdicts {
        let _ = writeln!(md, "- {} {}", if v.ok { "ok:" } else { "FAIL:" }, v.text);
    }
    md.push_str(
        "\nStatistics are `ses-diff`'s after importing each session: unrouted (u), \
         violations (v), vias, length (mm), bends; Java / Rust.\n\n",
    );
    md.push_str(
        "| board | class | verdict | eq | bytes | nets | first nets | passes J/R | first pass diff \
         | u J/R | v J/R | vias J/R | mm J/R | bends J/R | D8 R/J | last-mile | R s | J s | reason |\n",
    );
    md.push_str(&"|---".repeat(19));
    md.push_str("|\n");
    for r in records {
        let _ = writeln!(
            md,
            "| {} | {} | {} | {} | {} | {} | {} | {}/{} | {} | {}/{} | {}/{} | {}/{} | {}/{} | {}/{} \
             | {}/{} | {:?} | {:.1} | {} | {} |",
            r.name,
            r.class.name(),
            r.verdict.name(),
            r.equivalent,
            r.byte_equal,
            r.net_count,
            r.differing_nets.join(" "),
            r.java_passes.len(),
            r.rust_passes.len(),
            r.first_pass_diff
                .map_or("-".to_string(), |p| format!("#{p}")),
            cell(&r.java, 0),
            cell(&r.rust, 0),
            cell(&r.java, 1),
            cell(&r.rust, 1),
            cell(&r.java, 2),
            cell(&r.rust, 2),
            cell(&r.java, 3),
            cell(&r.rust, 3),
            cell(&r.java, 4),
            cell(&r.rust, 4),
            r.d8_rust.len(),
            r.d8_java.len(),
            r.last_mile,
            r.rust_seconds,
            r.java_seconds
                .map_or("-".to_string(), |s| format!("{s:.1}")),
            if r.reason.is_empty() {
                r.error.replace('|', "/")
            } else {
                r.reason.replace('|', "/")
            },
        );
    }
    std::fs::write(dir.join("summary.md"), md).expect("summary.md");
    let json = JsonValue::Object(vec![
        (
            "class_verdicts".into(),
            JsonValue::Array(
                verdicts
                    .iter()
                    .map(|v| {
                        JsonValue::Object(vec![
                            ("ok".into(), JsonValue::Bool(v.ok)),
                            ("text".into(), JsonValue::string(v.text.clone())),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "boards".into(),
            JsonValue::Array(records.iter().map(BoardRecord::to_json).collect()),
        ),
    ]);
    std::fs::write(dir.join("summary.json"), json.to_pretty_string()).expect("summary.json");
}
