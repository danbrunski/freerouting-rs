//! `--stats json` (P01.2): per-stage work counts and times of a run.
//!
//! The flag is a developer extra, handled before the Java argument parser
//! sees the command line ([`extract`] removes it), so it is not one of the
//! plugin flags and not in `--capabilities`:
//!
//! - `--stats json` writes the statistics to `<output>.stats.json` (the
//!   primary `-do` file with `.stats.json` appended; `<design>.stats.json`
//!   when there is no `-do`, as in `-drc` mode);
//! - `--stats-out <file>` names the file instead.
//!
//! The statistics are the engine's ([`freerouting::run_stats`]): stages
//! `load`, `fanout`, each autorouter `pass`, each `optimizer_pass`, `drc` and
//! `write`, with the counts of each. They change no result: a run with the
//! flag writes the same session as a run without it, and the counts of two
//! runs are equal.

use std::path::PathBuf;

use freerouting::run_stats;

/// What `--stats` asked for.
#[derive(Debug, PartialEq, Eq)]
pub struct StatsRequest {
    /// The command line without the statistics flags.
    pub argv: Vec<String>,
    /// Where the JSON goes.
    pub file: PathBuf,
    /// The design name the JSON carries.
    pub design: String,
}

/// The value of `name` in `argv` (`-do file`).
fn value_of<'a>(argv: &'a [String], name: &str) -> Option<&'a str> {
    argv.iter()
        .position(|a| a == name)
        .and_then(|i| argv.get(i + 1))
        .map(String::as_str)
}

/// Takes `--stats json` and `--stats-out <file>` out of `argv`. `Ok(None)`
/// when there is no `--stats`; `Err` for a format other than `json` or a
/// missing value.
pub fn extract(argv: &[String]) -> Result<Option<StatsRequest>, String> {
    let mut rest = Vec::new();
    let mut format = None;
    let mut out = None;
    let mut i = 0;
    while i < argv.len() {
        let (name, inline) = match argv[i].split_once('=') {
            Some((n, v)) if n.starts_with("--stats") => (n, Some(v.to_string())),
            _ => (argv[i].as_str(), None),
        };
        if name != "--stats" && name != "--stats-out" {
            rest.push(argv[i].clone());
            i += 1;
            continue;
        }
        let value = match inline {
            Some(v) => {
                i += 1;
                v
            }
            None => {
                let v = argv
                    .get(i + 1)
                    .ok_or_else(|| format!("{name} needs a value"))?
                    .clone();
                i += 2;
                v
            }
        };
        if name == "--stats" {
            if value.to_lowercase() != "json" {
                return Err(format!("--stats: '{value}' is not a format (use json)"));
            }
            format = Some(());
        } else {
            out = Some(PathBuf::from(value));
        }
    }
    if format.is_none() && out.is_none() {
        return Ok(None);
    }
    let design = value_of(&rest, "-de").unwrap_or_default();
    let file = out.unwrap_or_else(|| {
        let base = value_of(&rest, "-do").unwrap_or(design);
        PathBuf::from(format!("{base}.stats.json"))
    });
    Ok(Some(StatsRequest {
        design: PathBuf::from(design)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        argv: rest,
        file,
    }))
}

impl StatsRequest {
    /// Turns the statistics on.
    pub fn start(&self) {
        run_stats::enable();
    }

    /// Writes the statistics file and turns them off. Returns the error
    /// text if the file cannot be written.
    pub fn finish(&self) -> Result<(), String> {
        run_stats::disable();
        let text = run_stats::report_json(&self.design).to_pretty_string() + "\n";
        std::fs::write(&self.file, text).map_err(|e| {
            format!(
                "Couldn't write the statistics to '{}': {e}",
                self.file.display()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_flag_is_none() {
        assert_eq!(extract(&args(&["-de", "a.dsn"])), Ok(None));
    }

    #[test]
    fn the_file_defaults_next_to_the_output() {
        let request = extract(&args(&[
            "-de", "d/a.dsn", "--stats", "json", "-do", "o/a.ses",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(request.argv, args(&["-de", "d/a.dsn", "-do", "o/a.ses"]));
        assert_eq!(request.file, PathBuf::from("o/a.ses.stats.json"));
        assert_eq!(request.design, "a.dsn");
    }

    #[test]
    fn stats_out_names_the_file_and_the_equals_form_works() {
        let request = extract(&args(&[
            "--stats=json",
            "--stats-out=x.json",
            "-drc",
            "-de",
            "a.dsn",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(request.file, PathBuf::from("x.json"));
        assert_eq!(request.argv, args(&["-drc", "-de", "a.dsn"]));
        let request = extract(&args(&["--stats", "json", "-drc", "-de", "a.dsn"]))
            .unwrap()
            .unwrap();
        assert_eq!(request.file, PathBuf::from("a.dsn.stats.json"));
    }

    #[test]
    fn a_bad_format_or_a_missing_value_is_an_error() {
        assert!(extract(&args(&["--stats", "xml"])).is_err());
        assert!(extract(&args(&["--stats"])).is_err());
    }
}
