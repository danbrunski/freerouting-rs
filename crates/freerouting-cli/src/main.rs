//! `freerouting` command-line entry point.
//!
//! Supports the design rules check of Java's CLI:
//!
//! ```text
//! freerouting -de board.dsn [board.ses] [board.rules] -drc [report.json] [-dr board.rules]
//! ```
//!
//! `-de` takes the design files (told apart by extension), `-drc` switches
//! to DRC mode and names the report file (standard output if absent), `-dr`
//! names a rules file. Routing is not implemented yet.

use std::path::PathBuf;
use std::process::ExitCode;

use freerouting::drc::{DrcRunOptions, run_drc};

/// The parsed command line (the DRC part of Java's `GlobalSettings`).
#[derive(Debug, Default, PartialEq)]
struct Args {
    design_file: Option<String>,
    session_file: Option<String>,
    rules_file: Option<String>,
    drc: bool,
    drc_report_file: Option<String>,
}

/// Java `GlobalSettings.applyCommandLineArguments` for `-de`, `-drc` and
/// `-dr`; other arguments are ignored.
fn parse_args(args: &[String]) -> Args {
    let mut result = Args::default();
    let value = |i: usize| args.get(i).filter(|a| !a.starts_with('-'));
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg.starts_with("-de") {
            let mut j = i + 1;
            while let Some(file) = value(j) {
                let file = file.trim();
                // An existing file is taken as is, otherwise `a.dsn+b.rules`
                // names several files.
                let parts: Vec<&str> = if std::path::Path::new(file).exists() {
                    vec![file]
                } else {
                    file.split('+').filter(|p| !p.trim().is_empty()).collect()
                };
                for part in parts {
                    let part = part.trim().to_string();
                    let lower = part.to_lowercase();
                    if lower.ends_with(".dsn") {
                        result.design_file = Some(part);
                    } else if lower.ends_with(".ses") {
                        result.session_file = Some(part);
                    } else if lower.ends_with(".rules") {
                        result.rules_file = Some(part);
                    } else {
                        eprintln!(
                            "Unknown file type in -de argument: {part}. Expected .dsn, .ses, or \
                             .rules"
                        );
                    }
                }
                j += 1;
            }
            i = j;
            continue;
        } else if arg.starts_with("-drc") {
            result.drc = true;
            if let Some(file) = value(i + 1) {
                result.drc_report_file = Some(file.clone());
                i += 1;
            }
        } else if arg.starts_with("-dr") {
            if let Some(file) = value(i + 1) {
                result.rules_file = Some(file.clone());
                i += 1;
            }
        }
        i += 1;
    }
    result
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&args);
    if !args.drc {
        eprintln!(
            "freerouting {} (Rust port): routing is not implemented yet; use -drc for a design \
             rules check",
            env!("CARGO_PKG_VERSION")
        );
        return ExitCode::from(2);
    }
    let Some(design_file) = args.design_file else {
        eprintln!("An input file must be specified with -de argument in DRC mode.");
        return ExitCode::FAILURE;
    };
    let mut options = DrcRunOptions::new(design_file);
    options.session_file = args.session_file.map(PathBuf::from);
    options.rules_file = args.rules_file.map(PathBuf::from);
    let run = match run_drc(&options) {
        Ok(run) => run,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    for message in &run.messages {
        eprintln!("{message}");
    }
    let json = run.report.to_json();
    match args.drc_report_file {
        Some(file) => {
            if let Err(e) = std::fs::write(&file, json) {
                eprintln!("Couldn't save the DRC report to '{file}': {e}");
                return ExitCode::FAILURE;
            }
            eprintln!("DRC report written to: {file}");
        }
        None => println!("{json}"),
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        parse_args(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn parses_drc_arguments() {
        let args = parse(&["-de", "a.dsn", "b.SES", "-drc", "out.json"]);
        assert_eq!(args.design_file.as_deref(), Some("a.dsn"));
        assert_eq!(args.session_file.as_deref(), Some("b.SES"));
        assert!(args.drc);
        assert_eq!(args.drc_report_file.as_deref(), Some("out.json"));

        let args = parse(&["-de", "a.dsn+c.rules", "-drc"]);
        assert_eq!(args.rules_file.as_deref(), Some("c.rules"));
        assert!(args.drc);
        assert_eq!(args.drc_report_file, None);

        let args = parse(&["-drc", "-de", "a.dsn", "-dr", "x.rules"]);
        assert_eq!(args.design_file.as_deref(), Some("a.dsn"));
        assert_eq!(args.rules_file.as_deref(), Some("x.rules"));
        assert_eq!(args.drc_report_file, None);
    }
}
