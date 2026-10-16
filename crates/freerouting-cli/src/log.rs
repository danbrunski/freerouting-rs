//! The console printer for the engine's log lines (C07.6).
//!
//! Lines go to standard error with no timestamp, so two runs differ only in
//! times. Warnings are prefixed `warning: `, errors `error: ` and debug lines
//! `debug: `; info lines are plain. The texts are Java's for the lines tools
//! parse (the pass line, the stage summary, the save lines); see
//! `freerouting::job::log`.

use freerouting::job::{LogLevel, LogLine};

/// The console level (`-ll`, `--logging.console.level`): the lowest level
/// that is printed, or `None` for `OFF`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsoleLevel(pub Option<LogLevel>);

impl Default for ConsoleLevel {
    fn default() -> Self {
        ConsoleLevel(Some(LogLevel::Info))
    }
}

impl ConsoleLevel {
    /// Java's level names: `TRACE`, `DEBUG` and `ALL` print everything,
    /// `INFO`, `WARN`, `ERROR` and `FATAL` the lines from there up, `OFF`
    /// nothing. An unknown name or none is `INFO`.
    pub fn from_name(name: Option<&str>) -> ConsoleLevel {
        let Some(name) = name else {
            return ConsoleLevel::default();
        };
        ConsoleLevel(match name.trim().to_uppercase().as_str() {
            "TRACE" | "DEBUG" | "ALL" => Some(LogLevel::Debug),
            "WARN" | "WARNING" => Some(LogLevel::Warn),
            "ERROR" | "FATAL" => Some(LogLevel::Error),
            "OFF" => None,
            _ => Some(LogLevel::Info),
        })
    }

    /// Whether a line of `level` is printed.
    pub fn allows(self, level: LogLevel) -> bool {
        self.0.is_some_and(|min| level >= min)
    }
}

/// One line as it is printed, without the newline.
pub fn format_line(line: &LogLine) -> String {
    match line.level {
        LogLevel::Debug => format!("debug: {}", line.text),
        LogLevel::Info => line.text.clone(),
        LogLevel::Warn => format!("warning: {}", line.text),
        LogLevel::Error => format!("error: {}", line.text),
    }
}

/// The printed form of `lines` at `level`, one per line.
pub fn render(lines: &[LogLine], level: ConsoleLevel) -> String {
    lines
        .iter()
        .filter(|l| level.allows(l.level))
        .map(|l| format_line(l) + "\n")
        .collect()
}

/// Prints `lines` to standard error.
pub fn print(lines: &[LogLine], level: ConsoleLevel) {
    let text = render(lines, level);
    if !text.is_empty() {
        eprint!("{text}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(level: LogLevel, text: &str) -> LogLine {
        LogLine {
            level,
            text: text.to_string(),
        }
    }

    #[test]
    fn prefixes_and_levels() {
        let lines = [
            line(LogLevel::Debug, "d"),
            line(LogLevel::Info, "i"),
            line(LogLevel::Warn, "w"),
            line(LogLevel::Error, "e"),
        ];
        assert_eq!(
            render(&lines, ConsoleLevel::default()),
            "i\nwarning: w\nerror: e\n"
        );
        assert_eq!(
            render(&lines, ConsoleLevel::from_name(Some("trace"))),
            "debug: d\ni\nwarning: w\nerror: e\n"
        );
        assert_eq!(
            render(&lines, ConsoleLevel::from_name(Some("WARN"))),
            "warning: w\nerror: e\n"
        );
        assert_eq!(render(&lines, ConsoleLevel::from_name(Some("off"))), "");
        assert_eq!(
            ConsoleLevel::from_name(Some("nonsense")),
            ConsoleLevel::default()
        );
    }
}
