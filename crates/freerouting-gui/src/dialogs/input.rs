//! Parsing and conversion helpers of the rules dialogs: Java's static
//! helpers of `WindowNetClasses`, `WindowClearanceMatrix` and
//! `WindowAutorouteParameterState`, which the Java tests call without a
//! window (`WindowNetClassesTest`, `DialogInteractionHandlersTest`).
//!
//! Java passes table cell values as `Object`; [`Input`] is the part of that
//! the helpers distinguish: a number, a string, or anything else.

use freerouting::board::structure::Unit;
use freerouting::geometry::jmath::round_to_i32;
use freerouting::io::CoordinateTransform;
use freerouting::settings::text::{parse_double, parse_float};

/// A cell value as Java hands it to the helpers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input<'a> {
    /// Any `java.lang.Number`.
    Number(f64),
    /// A `String`.
    Text(&'a str),
    /// `null` or an object of another type.
    Other,
}

/// Java `WindowNetClasses.parseTraceWidthValue`: a number as it is, a string
/// with a decimal comma or point and blanks around it, else `None`.
pub fn parse_trace_width_value(value: Input<'_>) -> Option<f64> {
    match value {
        Input::Number(n) => Some(n),
        Input::Text(s) => parse_double(s.replace(',', ".").trim()).ok(),
        Input::Other => None,
    }
}

/// Java `WindowNetClasses.applyTraceWidth`: the half width is
/// `round(user_to_board(0.5 * width))`, refused (nothing is set) for a value
/// that does not parse, is 0 or less, has no transform, or gives a half width
/// of 0 or less. `set_half_width` receives the half width on success.
pub fn apply_trace_width(
    set_half_width: impl FnOnce(i32),
    value: Input<'_>,
    user_to_board: Option<&dyn Fn(f64) -> f64>,
) -> bool {
    let Some(width) = parse_trace_width_value(value) else {
        return false;
    };
    // `parsedValue <= 0` is false for NaN, as in Java.
    let Some(user_to_board) =
        user_to_board.filter(|_| width.partial_cmp(&0.0).is_none_or(|o| o.is_gt()))
    else {
        return false;
    };
    let half_width = round_to_i32(user_to_board(0.5 * width));
    if half_width <= 0 {
        return false;
    }
    set_half_width(half_width);
    true
}

/// Java `WindowNetClasses.canRemoveNetClass`.
pub fn can_remove_net_class(row_count: usize, selected_row: i32) -> bool {
    row_count > 1 && selected_row >= 0
}

/// The characters a clearance class name must not contain
/// (`WindowClearanceMatrix.reserved_name_chars`).
const RESERVED_NAME_CHARS: [char; 4] = ['(', ')', ' ', '_'];

/// Java `WindowClearanceMatrix.isLegalClassName`.
pub fn is_legal_class_name(value: &str) -> bool {
    !value.is_empty() && !value.contains(RESERVED_NAME_CHARS)
}

/// Java `WindowClearanceMatrix.parseClearanceTableValue`: a number as a
/// `float`, a string with `Float.parseFloat`, else `None`.
pub fn parse_clearance_table_value(value: Input<'_>) -> Option<f32> {
    match value {
        Input::Number(n) => Some(n as f32),
        Input::Text(s) => parse_float(s).ok(),
        Input::Other => None,
    }
}

/// The length cells of the net classes table (`setValueAt`): the text goes
/// through `Float.parseFloat` (no decimal comma, no blanks trimmed beyond what
/// Java trims) and a failure is 0.
pub fn parse_float_cell(text: &str) -> f32 {
    parse_float(text).unwrap_or(0.0)
}

/// Java `WindowAutorouteParameterState.normalizeIntInput`: anything but a
/// number keeps the old value, a number is cut to an `int` and clamped.
pub fn normalize_int_input(input: Input<'_>, old: i32, min: i32, max: i32) -> i32 {
    match input {
        Input::Number(n) => freerouting::geometry::jmath::d2i(n).clamp(min, max),
        _ => old,
    }
}

/// Java `WindowAutorouteParameterState.normalizePositiveDoubleInput`.
pub fn normalize_positive_double_input(input: Input<'_>, old: f64) -> f64 {
    match input {
        Input::Number(n) if n > 0.0 => n,
        _ => old,
    }
}

/// Java `WindowAutorouteParameterState.normalizeTimeoutInput`: a text of the
/// form `[days.]hours:mm:ss` is kept, anything else gives the old value.
pub fn normalize_timeout_input(input: Input<'_>, old: &str) -> String {
    match input {
        Input::Text(s) if is_timeout(s) => s.to_string(),
        _ => old.to_string(),
    }
}

/// `^(\d+\.)?\d+:\d{2}:\d{2}$`.
fn is_timeout(s: &str) -> bool {
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    let mut parts = s.split(':');
    let (Some(first), Some(minutes), Some(seconds), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let two = |t: &str| t.len() == 2 && digits(t);
    let hours_ok = match first.split_once('.') {
        Some((days, hours)) => digits(days) && digits(hours) && !hours.contains('.'),
        None => digits(first),
    };
    hours_ok && two(minutes) && two(seconds)
}

/// The two ends of a length conversion: the units the dialogs show values
/// in (the display unit of G04.6) and the board's own coordinates.
///
/// A value typed in the display unit goes to the design file's unit
/// (`Unit::scale`), then through `CoordinateTransform` to board units, the
/// order Java's dialogs use with `userToBoard` and `boardToUser`.
#[derive(Clone, Copy, Debug)]
pub struct RuleUnits {
    pub transform: CoordinateTransform,
    /// The unit of the design file, which `transform` converts from.
    pub design: Unit,
    /// The unit the dialogs show.
    pub display: Unit,
}

impl RuleUnits {
    /// Board units from a length in the display unit
    /// (`CoordinateTransform.userToBoard`).
    pub fn to_board(&self, display_value: f64) -> f64 {
        self.transform
            .dsn_to_board(Unit::scale(display_value, self.display, self.design))
    }

    /// A board length in the display unit (`boardToUser`).
    pub fn to_display(&self, board_value: f64) -> f64 {
        Unit::scale(
            self.transform.board_to_dsn(board_value),
            self.design,
            self.display,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // WindowNetClassesTest.parseTraceWidthValueHandlesVariousNumericAndStringInputs
    #[test]
    fn parse_trace_width_value_handles_numeric_and_string_inputs() {
        let n = Input::Number;
        let t = Input::Text;
        assert_eq!(parse_trace_width_value(n(200.0)), Some(200.0));
        assert_eq!(parse_trace_width_value(n(f64::from(150.5f32))), Some(150.5));
        assert_eq!(parse_trace_width_value(n(300.25)), Some(300.25));

        assert_eq!(parse_trace_width_value(t("200.0000")), Some(200.0));
        assert_eq!(parse_trace_width_value(t("0.35")), Some(0.35));
        assert_eq!(parse_trace_width_value(t("0,35")), Some(0.35));
        assert_eq!(parse_trace_width_value(t("  120.0  ")), Some(120.0));

        assert_eq!(parse_trace_width_value(t("invalid")), None);
        assert_eq!(parse_trace_width_value(t("")), None);
        assert_eq!(parse_trace_width_value(Input::Other), None);
    }

    // WindowNetClassesTest.applyTraceWidthAppliesCorrectHalfWidthToNetClass:
    // `ct.userToBoard(100.0)` returns 10000.0; the class gets half width 10000
    // and the transform is asked for exactly 100.0.
    #[test]
    fn apply_trace_width_applies_the_half_width() {
        let asked = std::cell::RefCell::new(Vec::new());
        let ct = |v: f64| {
            asked.borrow_mut().push(v);
            if v == 100.0 { 10000.0 } else { 0.0 }
        };
        let mut set = Vec::new();
        let ok = apply_trace_width(|hw| set.push(hw), Input::Number(200.0), Some(&ct));
        assert!(ok);
        assert_eq!(*asked.borrow(), [100.0]);
        assert_eq!(set, [10000]);
    }

    // WindowNetClassesTest.applyTraceWidthFailsOnInvalidOrNonPositiveInputs:
    // the class is not touched.
    #[test]
    fn apply_trace_width_refuses_invalid_input() {
        let ct = |v: f64| v * 100.0;
        let mut touched = false;
        let mut run = |value, ct: Option<&dyn Fn(f64) -> f64>| {
            apply_trace_width(|_| touched = true, value, ct)
        };
        assert!(!run(Input::Number(0.0), Some(&ct)));
        assert!(!run(Input::Number(-50.0), Some(&ct)));
        assert!(!run(Input::Text("bad_input"), Some(&ct)));
        assert!(!run(Input::Number(100.0), None));
        // A half width that rounds to 0 is refused as well.
        assert!(!run(Input::Number(0.0001), Some(&ct)));
        assert!(!touched);
    }

    // DialogInteractionHandlersTest.autoRouterSettingsTextFieldNormalizationHandlesBoundsAndInvalidInputs
    #[test]
    fn autorouter_text_field_normalization() {
        let n = Input::Number;
        assert_eq!(normalize_int_input(n(0.0), 9, 1, 9999), 1);
        assert_eq!(normalize_int_input(n(0.0), 9, 0, 9999), 0);
        assert_eq!(normalize_int_input(n(12000.0), 9, 1, 9999), 9999);
        assert_eq!(normalize_int_input(n(42.0), 9, 1, 9999), 42);
        assert_eq!(normalize_int_input(Input::Text("bad"), 9, 1, 9999), 9);

        let t = Input::Text;
        assert_eq!(
            normalize_timeout_input(t("12:00:00"), "00:30:00"),
            "12:00:00"
        );
        assert_eq!(normalize_timeout_input(t("bad"), "00:30:00"), "00:30:00");
        // The optional day part, and the shapes the pattern refuses.
        assert_eq!(normalize_timeout_input(t("2.12:00:00"), "x"), "2.12:00:00");
        for bad in [
            "12:0:00",
            "12:00",
            "1.2.3:00:00",
            ":00:00",
            "12:00:00:00",
            "",
        ] {
            assert_eq!(normalize_timeout_input(t(bad), "old"), "old", "{bad}");
        }
        assert_eq!(normalize_timeout_input(n(1.0), "old"), "old");

        assert_eq!(normalize_positive_double_input(n(2.5), 1.0), 2.5);
        assert_eq!(normalize_positive_double_input(n(-3.0), 1.0), 1.0);
        assert_eq!(normalize_positive_double_input(n(f64::NAN), 1.0), 1.0);
    }

    // DialogInteractionHandlersTest.clearanceMatrixGridCellParsingAndLayeredApplyWorkAsExpected
    // (the parsing half; the layered apply is `rules_edit`'s, tested against
    // the engine in tests/ui_rules.rs).
    #[test]
    fn clearance_matrix_cell_parsing() {
        assert!(is_legal_class_name("default2"));
        assert!(!is_legal_class_name("invalid_name"));
        assert!(!is_legal_class_name(""));
        assert!(!is_legal_class_name("a b"));
        assert!(!is_legal_class_name("a(b"));
        assert!(!is_legal_class_name("a)b"));

        assert_eq!(
            parse_clearance_table_value(Input::Number(f64::from(2.5f32))),
            Some(2.5)
        );
        assert_eq!(parse_clearance_table_value(Input::Text("1.25")), Some(1.25));
        assert_eq!(
            parse_clearance_table_value(Input::Text("not-a-number")),
            None
        );
        assert_eq!(parse_clearance_table_value(Input::Other), None);
    }

    // DialogInteractionHandlersTest.clearanceMatrixParseReturnsNumberForNumericTypes
    #[test]
    fn clearance_matrix_parse_returns_numbers() {
        assert_eq!(parse_clearance_table_value(Input::Number(7.0)), Some(7.0));
        assert_eq!(parse_clearance_table_value(Input::Number(3.75)), Some(3.75));
    }

    // DialogInteractionHandlersTest.netClassesButtonAndTableHelpersApplyExpectedChanges
    // (canRemoveNetClass; the two setters are `rules_edit`'s).
    #[test]
    fn can_remove_net_class_needs_two_classes_and_a_row() {
        assert!(!can_remove_net_class(1, 0));
        assert!(!can_remove_net_class(2, -1));
        assert!(can_remove_net_class(2, 0));
    }

    #[test]
    fn rule_units_convert_through_the_design_unit() {
        // 10 board units per design unit (um); the display unit is mm.
        let units = RuleUnits {
            transform: CoordinateTransform::new(10.0, 0.0, 0.0),
            design: Unit::Um,
            display: Unit::Mm,
        };
        assert_eq!(units.to_board(0.3), 3000.0);
        assert_eq!(units.to_display(3000.0), 0.3);
    }
}
