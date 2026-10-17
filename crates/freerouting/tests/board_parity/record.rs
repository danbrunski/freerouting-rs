//! The per-board record of the A14 gate, its JSON file, and the route of one board.

use super::*;

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// How a board fared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Not yet judged.
    Pending,
    /// Within its class's rule.
    Pass,
    /// A listed mismatch.
    Expected,
    /// Outside its class's rule.
    Fail,
}

impl Verdict {
    pub(super) fn name(self) -> &'static str {
        match self {
            Verdict::Pending => "pending",
            Verdict::Pass => "pass",
            Verdict::Expected => "expected",
            Verdict::Fail => "FAIL",
        }
    }
}

/// One board's result.
#[derive(Clone, Debug)]
pub struct BoardRecord {
    pub name: String,
    pub class: Class,
    pub java_result: String,
    /// `completed`, `load_error`, `pipeline_error`, `panic` or `no_reference`.
    pub rust_result: String,
    /// The error text, if Rust did not route.
    pub error: String,
    pub equivalent: bool,
    /// Rust's SES text equals Java's.
    pub byte_equal: bool,
    pub net_count: usize,
    pub differing_nets: Vec<String>,
    pub java: Option<[f64; 5]>,
    pub rust: Option<[f64; 5]>,
    pub java_passes: Vec<String>,
    pub rust_passes: Vec<String>,
    /// The first pass (1-based) whose score line differs, or the first missing one.
    pub first_pass_diff: Option<usize>,
    /// Items removed by each last-mile ripup with removals.
    pub last_mile: Vec<usize>,
    pub restores: usize,
    pub d8_rust: Vec<String>,
    pub d8_java: Vec<String>,
    pub rust_seconds: f64,
    pub java_seconds: Option<f64>,
    pub cutoffs: usize,
    pub verdict: Verdict,
    pub reason: String,
}

impl BoardRecord {
    fn new(reference: &Reference) -> BoardRecord {
        BoardRecord {
            name: reference.name.clone(),
            class: reference.class,
            java_result: reference.result.clone(),
            rust_result: String::new(),
            error: String::new(),
            equivalent: false,
            byte_equal: false,
            net_count: 0,
            differing_nets: Vec::new(),
            java: None,
            rust: None,
            java_passes: reference.passes.clone(),
            rust_passes: Vec::new(),
            first_pass_diff: None,
            last_mile: Vec::new(),
            restores: 0,
            d8_rust: Vec::new(),
            d8_java: Vec::new(),
            rust_seconds: 0.0,
            java_seconds: reference.java_ms.map(|ms| ms as f64 / 1000.0),
            cutoffs: reference.cutoffs,
            verdict: Verdict::Pending,
            reason: String::new(),
        }
    }

    /// A record holding only `comparison` (A14.1's self-test).
    pub fn from_comparison(reference: &Reference, comparison: Comparison) -> BoardRecord {
        let mut record = BoardRecord::new(reference);
        record.rust_result = "completed".to_string();
        record.rust_passes = reference.passes.clone();
        record.apply(comparison);
        record
    }

    fn apply(&mut self, comparison: Comparison) {
        self.equivalent = comparison.equivalent;
        self.net_count = comparison.net_count;
        self.differing_nets = comparison.differing_nets;
        self.java = Some(comparison.java);
        self.rust = Some(comparison.rust);
    }

    fn path(dir: &Path, name: &str) -> PathBuf {
        dir.join("boards").join(format!("{name}.json"))
    }

    /// Writes the record to `<dir>/boards/<name>.json`.
    pub fn write(&self, dir: &Path) {
        let path = Self::path(dir, &self.name);
        std::fs::create_dir_all(path.parent().unwrap()).expect("the records directory");
        std::fs::write(&path, self.to_json().to_pretty_string())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    }

    /// Reads a record written by [`BoardRecord::write`].
    pub fn read(dir: &Path, name: &str) -> Option<BoardRecord> {
        let body = std::fs::read_to_string(Self::path(dir, name)).ok()?;
        let json = JsonValue::parse(&body).ok()?;
        let stats = |key: &str| -> Option<[f64; 5]> {
            let values: Vec<f64> = array(field(&json, key))
                .iter()
                .filter_map(|v| number(Some(v)))
                .collect();
            values.try_into().ok()
        };
        let counts = |key: &str| -> Vec<usize> {
            array(field(&json, key))
                .iter()
                .filter_map(|v| int(Some(v)))
                .map(|v| v as usize)
                .collect()
        };
        Some(BoardRecord {
            name: text(field(&json, "board"))?,
            class: Class::parse(&text(field(&json, "class"))?)?,
            java_result: text(field(&json, "java_result"))?,
            rust_result: text(field(&json, "rust_result"))?,
            error: text(field(&json, "error")).unwrap_or_default(),
            equivalent: matches!(field(&json, "equivalent"), Some(JsonValue::Bool(true))),
            byte_equal: matches!(field(&json, "byte_equal"), Some(JsonValue::Bool(true))),
            net_count: int(field(&json, "net_count"))? as usize,
            differing_nets: strings(field(&json, "differing_nets")),
            java: stats("java"),
            rust: stats("rust"),
            java_passes: strings(field(&json, "java_passes")),
            rust_passes: strings(field(&json, "rust_passes")),
            first_pass_diff: int(field(&json, "first_pass_diff")).map(|v| v as usize),
            last_mile: counts("last_mile"),
            restores: int(field(&json, "restores")).unwrap_or(0) as usize,
            d8_rust: strings(field(&json, "d8_rust")),
            d8_java: strings(field(&json, "d8_java")),
            rust_seconds: number(field(&json, "rust_seconds")).unwrap_or(0.0),
            java_seconds: number(field(&json, "java_seconds")),
            cutoffs: int(field(&json, "cutoffs")).unwrap_or(0) as usize,
            verdict: Verdict::Pending,
            reason: String::new(),
        })
    }

    pub(super) fn to_json(&self) -> JsonValue {
        let stats = |s: &Option<[f64; 5]>| match s {
            Some(values) => JsonValue::Array(values.iter().map(|&v| float(v)).collect()),
            None => JsonValue::Null,
        };
        let opt_int = |v: Option<usize>| v.map_or(JsonValue::Null, |v| JsonValue::int(v as i64));
        JsonValue::Object(vec![
            ("board".into(), JsonValue::string(self.name.clone())),
            ("class".into(), JsonValue::string(self.class.name())),
            ("verdict".into(), JsonValue::string(self.verdict.name())),
            ("reason".into(), JsonValue::string(self.reason.clone())),
            (
                "java_result".into(),
                JsonValue::string(self.java_result.clone()),
            ),
            (
                "rust_result".into(),
                JsonValue::string(self.rust_result.clone()),
            ),
            ("error".into(), JsonValue::string(self.error.clone())),
            ("equivalent".into(), JsonValue::Bool(self.equivalent)),
            ("byte_equal".into(), JsonValue::Bool(self.byte_equal)),
            ("net_count".into(), JsonValue::int(self.net_count as i64)),
            ("differing_nets".into(), string_array(&self.differing_nets)),
            (
                "statistic_fields".into(),
                string_array(&STATISTIC_FIELDS.map(String::from)),
            ),
            ("java".into(), stats(&self.java)),
            ("rust".into(), stats(&self.rust)),
            ("java_passes".into(), string_array(&self.java_passes)),
            ("rust_passes".into(), string_array(&self.rust_passes)),
            ("first_pass_diff".into(), opt_int(self.first_pass_diff)),
            (
                "last_mile".into(),
                JsonValue::Array(
                    self.last_mile
                        .iter()
                        .map(|&n| JsonValue::int(n as i64))
                        .collect(),
                ),
            ),
            ("restores".into(), JsonValue::int(self.restores as i64)),
            ("d8_rust".into(), string_array(&self.d8_rust)),
            ("d8_java".into(), string_array(&self.d8_java)),
            ("rust_seconds".into(), float(self.rust_seconds)),
            (
                "java_seconds".into(),
                self.java_seconds.map_or(JsonValue::Null, float),
            ),
            ("cutoffs".into(), JsonValue::int(self.cutoffs as i64)),
        ])
    }

    /// One log line.
    pub fn one_line(&self) -> String {
        let stats = |s: &Option<[f64; 5]>| {
            s.map_or("-".to_string(), |v| {
                format!("{}u {}v {}vias {:.2}mm", v[0], v[1], v[2], v[3])
            })
        };
        format!(
            "{} [{}] rust {} eq {} bytes {} nets {} pass-diff {:?} java {} rust {} d8 {} {:.1}s",
            self.name,
            self.class.name(),
            self.rust_result,
            self.equivalent,
            self.byte_equal,
            self.net_count,
            self.first_pass_diff,
            stats(&self.java),
            stats(&self.rust),
            self.d8_rust.len(),
            self.rust_seconds
        )
    }
}

/// The first pass (1-based) whose line differs, or where one list ends early.
fn first_pass_diff(java: &[String], rust: &[String]) -> Option<usize> {
    let common = java.len().min(rust.len());
    (0..common)
        .find(|&i| java[i] != rust[i])
        .or((java.len() != rust.len()).then_some(common))
        .map(|i| i + 1)
}

/// Routes `reference` and compares it with Java's session (A14.1).
pub fn route_board(root: &Path, reference: &Reference, out: &Path, sink: Sink) -> BoardRecord {
    let mut record = BoardRecord::new(reference);
    let loaded = match load(root, reference) {
        Ok(loaded) => loaded,
        Err(e) => {
            record.rust_result = "load_error".to_string();
            record.error = e;
            return record;
        }
    };
    let input: BasicBoard = (*loaded.board).clone();
    let design = loaded.design.clone();
    let routed = match run(loaded, sink) {
        Ok(routed) => routed,
        Err(e) => {
            record.rust_result = e.split(':').next().unwrap_or("error").to_string();
            record.error = e;
            return record;
        }
    };
    record.rust_result = "completed".to_string();
    record.rust_seconds = routed.seconds;
    record.rust_passes = routed.passes;
    record.last_mile = routed.last_mile.into_iter().filter(|&n| n > 0).collect();
    record.restores = routed.restores;
    record.first_pass_diff = first_pass_diff(&record.java_passes, &record.rust_passes);
    let ses = write_ses(&routed.board, &design, SesStyle::Java);
    // A board below a subdirectory (PARITY_SUBDIRS) keeps its directory.
    let ses_path = out.join(format!("{}.ses", reference.name));
    std::fs::create_dir_all(ses_path.parent().unwrap_or(out)).expect("the output directory");
    std::fs::write(&ses_path, &ses).expect("the SES");
    record.d8_rust = d8_breaches(&routed.board, &input);
    drop(routed.board);
    let java_path = root.join(format!("fixtures/reference/route/{}.ses", reference.name));
    let Ok(java_text) = std::fs::read_to_string(&java_path) else {
        record.error = "Java wrote no session".to_string();
        return record;
    };
    record.byte_equal = java_text == ses;
    let java = parse_session(&java_text).expect("Java's session parses");
    let rust = parse_session(&ses).expect("Rust's session parses");
    record.apply(compare_sessions(&input, &java, &rust));
    let mut java_board = input.clone();
    import_session(&mut java_board, &java);
    record.d8_java = d8_breaches(&java_board, &input);
    record
}
