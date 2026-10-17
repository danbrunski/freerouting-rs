//! The job timeout rules of `WindowAutorouteParameterState` (Java at
//! `cda540f`): the dialog edits the timeout as a number and a unit, and
//! stores it as `HH:mm:ss` capped at four weeks.

use freerouting::settings::timespan::parse_timespan_seconds;

use crate::dialogs::input::Input;

/// `MAX_TIMEOUT_SECONDS`: four weeks (28 days, 672 hours).
pub const MAX_TIMEOUT_SECONDS: i64 = 4 * 7 * 24 * 3600;
/// `DEFAULT_TIMEOUT_SECONDS`.
pub const DEFAULT_TIMEOUT_SECONDS: i64 = 0;

/// `normalizeIntInput`.
pub fn normalize_int_input(input: Input<'_>, old: i32, min: i32, max: i32) -> i32 {
    crate::dialogs::input::normalize_int_input(input, old, min, max)
}

/// `normalizePositiveDoubleInput`.
pub fn normalize_positive_double_input(input: Input<'_>, old: f64) -> f64 {
    crate::dialogs::input::normalize_positive_double_input(input, old)
}

/// `normalizeTimeoutInput`.
pub fn normalize_timeout_input(input: Input<'_>, old: &str) -> String {
    crate::dialogs::input::normalize_timeout_input(input, old)
}

/// A parsed timeout (`WindowAutorouteParameterState.Timeout`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeout {
    pub hours: i64,
    pub minutes: i64,
    pub seconds: i64,
    pub total_seconds: i64,
}

fn clamp_seconds(total: i64) -> i64 {
    total.clamp(DEFAULT_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS)
}

/// `parseTimeout`: an unparsable or missing text is 0, anything above four
/// weeks is four weeks.
pub fn parse_timeout(text: Option<&str>) -> Timeout {
    let total = text
        .and_then(parse_timespan_seconds)
        .unwrap_or(DEFAULT_TIMEOUT_SECONDS);
    let total = clamp_seconds(total);
    let rest = total % 3600;
    Timeout {
        hours: total / 3600,
        minutes: rest / 60,
        seconds: rest % 60,
        total_seconds: total,
    }
}

/// `TimeoutUnit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimeoutUnit {
    Minutes,
    Hours,
    Days,
    Weeks,
}

impl TimeoutUnit {
    pub const ALL: [TimeoutUnit; 4] = [
        TimeoutUnit::Minutes,
        TimeoutUnit::Hours,
        TimeoutUnit::Days,
        TimeoutUnit::Weeks,
    ];

    pub fn seconds_per_unit(self) -> i64 {
        match self {
            TimeoutUnit::Minutes => 60,
            TimeoutUnit::Hours => 3600,
            TimeoutUnit::Days => 86_400,
            TimeoutUnit::Weeks => 7 * 86_400,
        }
    }

    pub fn max_units(self) -> i64 {
        match self {
            TimeoutUnit::Minutes => 40_320,
            TimeoutUnit::Hours => 672,
            TimeoutUnit::Days => 28,
            TimeoutUnit::Weeks => 4,
        }
    }

    /// `timeout_unit_minutes` to `timeout_unit_weeks`.
    pub fn label(self) -> &'static str {
        match self {
            TimeoutUnit::Minutes => "Minutes",
            TimeoutUnit::Hours => "Hours",
            TimeoutUnit::Days => "Days",
            TimeoutUnit::Weeks => "Weeks",
        }
    }
}

/// `DecomposedTimeout`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decomposed {
    pub value: i64,
    pub unit: TimeoutUnit,
}

/// `decomposeTimeout`: the largest unit that divides the seconds evenly.
pub fn decompose_timeout(total_seconds: i64) -> Decomposed {
    let clamped = clamp_seconds(total_seconds);
    if clamped <= 0 {
        return Decomposed {
            value: 0,
            unit: TimeoutUnit::Minutes,
        };
    }
    for unit in [TimeoutUnit::Weeks, TimeoutUnit::Days, TimeoutUnit::Hours] {
        if clamped % unit.seconds_per_unit() == 0 {
            return Decomposed {
                value: clamped / unit.seconds_per_unit(),
                unit,
            };
        }
    }
    Decomposed {
        value: (clamped / TimeoutUnit::Minutes.seconds_per_unit()).max(1),
        unit: TimeoutUnit::Minutes,
    }
}

/// `toSeconds`.
pub fn to_seconds(value: i64, unit: TimeoutUnit) -> i64 {
    let value = value.clamp(0, unit.max_units());
    clamp_seconds(value * unit.seconds_per_unit())
}

fn hms(total_seconds: i64) -> String {
    let rest = total_seconds % 3600;
    format!(
        "{:02}:{:02}:{:02}",
        total_seconds / 3600,
        rest / 60,
        rest % 60
    )
}

/// `buildTimeout(value, unit)`: `HH:mm:ss`, the hours not wrapped at 24.
pub fn build_timeout_in(value: i64, unit: TimeoutUnit) -> String {
    hms(to_seconds(value, unit))
}

/// `buildTimeout(hours, minutes, seconds)`.
pub fn build_timeout(hours: i64, minutes: i64, seconds: i64) -> String {
    hms(clamp_seconds(hours * 3600 + minutes * 60 + seconds))
}

/// `formatTimeout`: "x days y hours z minutes" above 24 hours, "y hours z
/// minutes" from one hour, else "z minutes".
pub fn format_timeout(total_seconds: i64) -> String {
    let clamped = clamp_seconds(total_seconds);
    let minutes = (clamped % 3600) / 60;
    if clamped > 86_400 {
        let days = clamped / 86_400;
        let hours = (clamped % 86_400) / 3600;
        return format!("{days} days {hours} hours {minutes} minutes");
    }
    if clamped >= 3600 {
        return format!("{} hours {minutes} minutes", clamped / 3600);
    }
    format!("{minutes} minutes")
}

#[cfg(test)]
mod tests {
    use super::*;

    // WindowAutorouteParameterStateTest.clampsIntegerAndPositiveDoubleInputs
    #[test]
    fn clamps_integer_and_positive_double_inputs() {
        let n = Input::Number;
        assert_eq!(normalize_int_input(n(-5.0), 4, 1, 9), 1);
        assert_eq!(normalize_int_input(n(20.0), 4, 1, 9), 9);
        assert_eq!(normalize_int_input(Input::Text("4"), 4, 1, 9), 4);
        assert_eq!(normalize_positive_double_input(n(4.5), 2.0), 4.5);
        assert_eq!(normalize_positive_double_input(n(0.0), 2.0), 2.0);
    }

    // WindowAutorouteParameterStateTest.parsesAndFormatsBoundedTimeouts
    #[test]
    fn parses_and_formats_bounded_timeouts() {
        let timeout = parse_timeout(Some("01:02:03"));
        assert_eq!(timeout.hours, 1);
        assert_eq!(timeout.minutes, 2);
        assert_eq!(timeout.seconds, 3);
        assert_eq!(build_timeout(1, 2, 3), "01:02:03");
        assert_eq!(format_timeout(timeout.total_seconds), "1 hours 2 minutes");
    }

    // WindowAutorouteParameterStateTest.capsTimeoutAtFourWeeks
    #[test]
    fn caps_timeout_at_four_weeks() {
        let timeout = parse_timeout(Some("999:99:99"));
        assert_eq!(timeout.hours, 672);
        assert_eq!(timeout.minutes, 0);
        assert_eq!(timeout.seconds, 0);
        assert_eq!(timeout.total_seconds, 2_419_200);
        assert_eq!(build_timeout(999, 99, 99), "672:00:00");
        assert_eq!(
            format_timeout(timeout.total_seconds),
            "28 days 0 hours 0 minutes"
        );
    }

    // WindowAutorouteParameterStateTest.formatsTimeoutWithDaysHoursMinutesThresholds
    #[test]
    fn formats_timeout_with_days_hours_minutes_thresholds() {
        // Greater than 24 hours: "x days y hours z minutes".
        assert_eq!(
            format_timeout(25 * 3600 + 10 * 60),
            "1 days 1 hours 10 minutes"
        );
        assert_eq!(format_timeout(2_419_200), "28 days 0 hours 0 minutes");
        // Lower or equal to 24 hours (and at least 1 hour): "y hours z minutes".
        assert_eq!(format_timeout(24 * 3600), "24 hours 0 minutes");
        assert_eq!(format_timeout(3720), "1 hours 2 minutes");
        // Less than 1 hour: "z minutes".
        assert_eq!(format_timeout(45 * 60), "45 minutes");
        assert_eq!(format_timeout(30), "0 minutes");
        assert_eq!(format_timeout(0), "0 minutes");
    }

    // WindowAutorouteParameterStateTest.decomposesAndBuildsTimeoutsWithUnits
    #[test]
    fn decomposes_and_builds_timeouts_with_units() {
        let cases = [
            (14 * 86_400, 2, TimeoutUnit::Weeks, "336:00:00"),
            (3 * 86_400, 3, TimeoutUnit::Days, "72:00:00"),
            (12 * 3600, 12, TimeoutUnit::Hours, "12:00:00"),
            (45 * 60, 45, TimeoutUnit::Minutes, "00:45:00"),
            (2_419_200, 4, TimeoutUnit::Weeks, "672:00:00"),
        ];
        for (seconds, value, unit, built) in cases {
            let decomposed = decompose_timeout(seconds);
            assert_eq!(decomposed, Decomposed { value, unit }, "{seconds}");
            assert_eq!(to_seconds(value, unit), seconds);
            assert_eq!(build_timeout_in(value, unit), built);
        }
        // Clamp over the maximum.
        assert_eq!(to_seconds(10, TimeoutUnit::Weeks), 2_419_200);
        assert_eq!(build_timeout_in(10, TimeoutUnit::Weeks), "672:00:00");
    }

    // WindowAutorouteParameterStateTest.normalizesTimeoutInput
    #[test]
    fn normalizes_timeout_input() {
        let t = Input::Text;
        assert_eq!(
            normalize_timeout_input(t("672:00:00"), "00:30:00"),
            "672:00:00"
        );
        assert_eq!(
            normalize_timeout_input(t("120:30:00"), "00:30:00"),
            "120:30:00"
        );
        assert_eq!(
            normalize_timeout_input(t("invalid"), "00:30:00"),
            "00:30:00"
        );
    }
}
