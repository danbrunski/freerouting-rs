//! The log lines of a job that tools parse (C07.6).
//!
//! Port of `FRLogger.formatScore` and `formatDuration`, and of the texts of
//! `AutorouteBatchLoop` (the pass line), `RoutingJobSchedulerActionThread`
//! (the stage summary) and `CliRunner.writeMainOutput` (the save lines), at
//! `a773a70ec`. A job collects [`LogLine`]s; the CLI prints them with a
//! console level filter and no timestamps, so two runs differ only in times.
//!
//! The Java fork's `compare.py` reads the stage summary with
//! `Auto-routing stage completed: started with (\d+) unrouted.*?final score:
//! [\d.]+ \((\d+) unrouted and (\d+) violations\)`. Java writes
//! `1 violation` in the singular, so that regex misses a one-violation board.
//! That is Java's behaviour and is kept (`format_score`'s test pins it).
//! The summary stops after the score: Rust has no JVM CPU-time or heap
//! figures to add.

use std::path::Path;

pub use super::{LogLevel, LogLine};

/// The text a job logs when a wall-clock limit stopped it (D2): the result
/// depends on the time the machine took.
pub const TIMEOUT_WARNING: &str = "stopped by wall-clock limit; result is timing-dependent";

/// `new DecimalFormat("0.00", Locale.US).format(value)`: two fraction
/// digits, `HALF_EVEN` on the exact binary value (Java's `DecimalFormat`
/// rounds the exact value since JDK 8, as Rust's `{:.2}` does), `NaN` and
/// `∞` as in the US symbols.
pub fn decimal_format_0_00(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 { "∞" } else { "-∞" }.to_string()
    } else {
        format!("{value:.2}")
    }
}

/// `FRLogger.formatScore`: `<score> (<n> unrouted and <m> violations)`, with
/// `violation` in the singular for exactly 1. The `float` score is widened to
/// `double` before formatting.
pub fn format_score(score: f32, unrouted: u32, violations: u32) -> String {
    format!(
        "{} ({unrouted} unrouted and {violations} violation{})",
        decimal_format_0_00(f64::from(score)),
        if violations == 1 { "" } else { "s" }
    )
}

/// `FRLogger.formatDuration`: hours and minutes as words when not zero,
/// then the seconds with two decimals.
pub fn format_duration(total_seconds: f64) -> String {
    let minutes = total_seconds / 60.0;
    let hours = (minutes / 60.0).floor();
    let minutes = (minutes % 60.0).floor();
    let seconds = total_seconds % 60.0;
    let hours_text = if hours > 0.0 {
        format!(
            "{} {} ",
            hours as i64,
            if hours == 1.0 { "hour" } else { "hours" }
        )
    } else {
        String::new()
    };
    let minutes_text = if minutes > 0.0 {
        format!(
            "{} {} ",
            minutes as i64,
            if minutes == 1.0 { "minute" } else { "minutes" }
        )
    } else {
        String::new()
    };
    format!(
        "{hours_text}{minutes_text}{} seconds",
        decimal_format_0_00(seconds)
    )
}

/// `String.format(Locale.US, "%.2f", seconds)` for the elapsed times of the
/// pass and summary lines.
fn seconds_text(seconds: f64) -> String {
    format!("{seconds:.2}")
}

/// The pass line of `AutorouteBatchLoop`, with B01's state hash as `hash`.
pub fn pass_line(
    pass: u32,
    hash: &str,
    seconds: f64,
    score: f32,
    unrouted: u32,
    violations: u32,
) -> String {
    format!(
        "Auto-routing pass #{pass} on board '{hash}' was completed in {} seconds with score {}",
        seconds_text(seconds),
        format_score(score, unrouted, violations)
    )
}

/// The stage summary of `RoutingJobSchedulerActionThread`, cut after the
/// score. `status` is `completed`, `completed with timeout`, `cancelled` or
/// `interrupted`.
pub fn stage_summary_line(
    status: &str,
    unrouted_at_start: u32,
    seconds: f64,
    score: f32,
    unrouted: u32,
    violations: u32,
) -> String {
    format!(
        "Auto-routing stage {status}: started with {unrouted_at_start} unrouted nets, completed in {} \
         seconds, final score: {}.",
        seconds_text(seconds),
        format_score(score, unrouted, violations)
    )
}

/// `Saving output file '<abs path>' (<format>)...`.
pub fn saving_line(path: &Path, format: &str) -> String {
    format!(
        "Saving output file '{}' ({format})...",
        absolute(path).display()
    )
}

/// `Successfully saved output file '<abs path>' (<n> bytes).`, or the
/// `additional` form.
pub fn saved_line(path: &Path, bytes: u64, additional: bool) -> String {
    format!(
        "Successfully saved {}output file '{}' ({bytes} bytes).",
        if additional { "additional " } else { "" },
        absolute(path).display()
    )
}

/// `Path.toAbsolutePath()`: the path made absolute against the working
/// directory without touching the file system.
pub fn absolute(path: &Path) -> std::path::PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_score_table() {
        // (score, unrouted, violations, expected). The ties are exact in
        // binary, so HALF_EVEN decides: 0.125 -> 0.12, 0.375 -> 0.38.
        let table: &[(f32, u32, u32, &str)] = &[
            (0.0, 0, 0, "0.00 (0 unrouted and 0 violations)"),
            (0.125, 3, 1, "0.12 (3 unrouted and 1 violation)"),
            (0.375, 3, 2, "0.38 (3 unrouted and 2 violations)"),
            (0.625, 0, 0, "0.62 (0 unrouted and 0 violations)"),
            (0.875, 1, 0, "0.88 (1 unrouted and 0 violations)"),
            (1.0, 0, 7, "1.00 (0 unrouted and 7 violations)"),
            (999.5, 12, 3, "999.50 (12 unrouted and 3 violations)"),
            (0.1, 0, 0, "0.10 (0 unrouted and 0 violations)"),
            (-0.001, 0, 0, "-0.00 (0 unrouted and 0 violations)"),
        ];
        for &(score, unrouted, violations, expected) in table {
            assert_eq!(
                format_score(score, unrouted, violations),
                expected,
                "{score}"
            );
        }
    }

    #[test]
    fn non_finite_scores_use_the_us_symbols() {
        assert_eq!(decimal_format_0_00(f64::NAN), "NaN");
        assert_eq!(decimal_format_0_00(f64::INFINITY), "∞");
        assert_eq!(decimal_format_0_00(f64::NEG_INFINITY), "-∞");
    }

    #[test]
    fn durations_name_hours_and_minutes() {
        assert_eq!(format_duration(0.0), "0.00 seconds");
        assert_eq!(format_duration(59.994), "59.99 seconds");
        assert_eq!(format_duration(61.5), "1 minute 1.50 seconds");
        assert_eq!(format_duration(3600.0), "1 hour 0.00 seconds");
        assert_eq!(format_duration(7325.25), "2 hours 2 minutes 5.25 seconds");
    }

    #[test]
    fn pass_and_stage_lines_have_javas_texts() {
        assert_eq!(
            pass_line(2, "0123abcd", 1.234, 0.5, 4, 0),
            "Auto-routing pass #2 on board '0123abcd' was completed in 1.23 seconds with score \
             0.50 (4 unrouted and 0 violations)"
        );
        assert_eq!(
            stage_summary_line("completed", 9, 3.0, 0.25, 1, 2),
            "Auto-routing stage completed: started with 9 unrouted nets, completed in 3.00 \
             seconds, final score: 0.25 (1 unrouted and 2 violations)."
        );
    }

    #[test]
    fn save_lines_name_the_absolute_path() {
        let path = Path::new("out.ses");
        let abs = absolute(path);
        assert!(abs.is_absolute());
        assert_eq!(
            saving_line(path, "SES"),
            format!("Saving output file '{}' (SES)...", abs.display())
        );
        assert_eq!(
            saved_line(path, 12, false),
            format!(
                "Successfully saved output file '{}' (12 bytes).",
                abs.display()
            )
        );
        assert!(
            saved_line(path, 12, true).starts_with("Successfully saved additional output file")
        );
    }
}
