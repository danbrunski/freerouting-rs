//! User-visible texts, English only (G00 question 3). They follow the Java
//! bundles at `cda540f` (`src/main/resources/app/freerouting/gui/**/*_en.properties`
//! and `Common_en.properties`) so users moving from Java recognise them, and
//! Java's text formatting (`TextManager.getText`, `String.valueOf(float)`,
//! `NumberFormat`) is reproduced here.

/// Java `TextManager.getText(key, args)` for an already looked up
/// `template`: the `{{name}}` placeholders are replaced by the arguments in
/// order, whatever their names are; `{{icon:..}}` placeholders are skipped,
/// and placeholders left over stay as they are. Java replaces every
/// occurrence of a placeholder's text at once.
pub fn fill(template: &str, args: &[&str]) -> String {
    let mut placeholders = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(len) = after.find("}}") else { break };
        // The regex `\{\{(.+?)\}\}` needs at least one character inside.
        if len == 0 {
            // `{{}}` is not a placeholder; look from the next brace on.
            rest = &rest[start + 1..];
            continue;
        }
        placeholders.push(&rest[start..start + 2 + len + 2]);
        rest = &after[len + 2..];
    }
    let mut text = template.to_string();
    let mut next = 0;
    for placeholder in placeholders {
        if !placeholder.starts_with("{{icon:") && next < args.len() {
            text = text.replace(placeholder, args[next]);
            next += 1;
        }
    }
    text
}

// WindowObjectList_en.properties
pub const FILTER: &str = "Filter:";
pub const SELECT: &str = "Select";
pub const SELECT_TOOLTIP: &str = "Select the chosen objects on the board.";
pub const INVERT: &str = "Invert";
pub const INVERT_TOOLTIP: &str = "Invert the selection.";
pub const INFO: &str = "Info";
pub const INFO_TOOLTIP: &str = "Display information about the selected objects.";
pub const RECALCULATE: &str = "Recalculate";
pub const RECALCULATE_TOOLTIP: &str = "Recalculate the contents of this window.";

// BoardMenuInfo_en.properties
pub const MENU_INFO: &str = "Info";
pub const MENU_INCOMPLETES: &str = "Incomplete Connections";
pub const MENU_LENGTH_VIOLATIONS: &str = "Length Violations";
pub const MENU_CLEARANCE_VIOLATIONS: &str = "Clearance Violations";
pub const MENU_UNCONNECTED_ROUTE: &str = "Unconnected traces and vias";
/// Not in Java: Java has no window for the zone island findings of its DRC.
pub const MENU_ZONE_ISLANDS: &str = "Zone Islands";
/// Not in Java's menu: the DRC runs in the background here.
pub const MENU_RUN_DRC: &str = "Run DRC";

// BoardMenuOther_en.properties
pub const MENU_OTHER: &str = "Other";
pub const DELETE_ALL: &str = "Delete All Traces and Vias";
/// The toolbar button (Java shows an icon with the tooltip).
pub const DELETE_ALL_BUTTON: &str = "Delete All";
pub const DELETE_ALL_TOOLTIP: &str = "Delete all traces and vias to restart routing from scratch.";
/// The confirmation Java lacks. It says what Java's menu item also does.
pub const DELETE_ALL_CONFIRM: &str = "Delete all traces and vias of the board? \
    The conduction areas are unfilled as well, and later routing ignores them. \
    You can undo this with U or Ctrl+Z.";

// WindowClearanceViolations_en.properties
pub const CLEARANCE_TITLE: &str = "Clearance Violations";
pub const CLEARANCE_TEMPLATE: &str = "A clearance violation of {{clearance_mm}} mm exists between {{item_a}} and {{item_b}} at {{location}} on layer {{layer}}.";
pub const CLEARANCE_EMPTY: &str = "No clearance violations found.";
pub const BOARD_OUTLINE: &str = "board outline";
pub const COMPONENT_KEEPOUT: &str = "component keepout";
pub const CONDUCTION_AREA: &str = "conduction area";
pub const KEEPOUT: &str = "keepout";
pub const PIN: &str = "pin";
pub const VIA_WITH_NET: &str = "via [{{net}}]";
pub const TRACE_WITH_NET: &str = "trace [{{net}}]";
pub const UNKNOWN: &str = "unknown";
pub const VIA_KEEPOUT: &str = "via keepout";

// Common_en.properties and WindowIncompletes
pub const INCOMPLETES_TITLE: &str = "Incomplete Connections";
pub const INCOMPLETES_EMPTY: &str = "Route completed.";

// WindowLengthViolations_en.properties
pub const LENGTH_TITLE: &str = "Length Violations";
pub const LENGTH_EMPTY: &str = "No length violations found.";
pub const LENGTH_MAX: &str =
    "The trace length for net {{net}} is {{length}}; the maximum allowed is {{allowed_length}}.";
pub const LENGTH_MIN: &str =
    "The trace length for net {{net}} is {{length}}; the minimum allowed is {{allowed_length}}.";

// CleanupWindows_en.properties
pub const UNCONNECTED_TITLE: &str = "Unconnected traces and vias";
pub const UNCONNECTED_EMPTY: &str = "No unconnected traces or vias found.";
pub const UNCONNECTED_ROW: &str = "Net {{net}}: {{trace_count}} traces; {{via_count}} vias.";

// Zone islands: not in Java's windows. The rows use the DRC report's text.
pub const ISLANDS_TITLE: &str = "Zone Islands";
pub const ISLANDS_EMPTY: &str = "No zone islands found.";

// Side panel and the toggles (BoardToolbar_en.properties: "Toggle the
// ratsnest display (G)").
pub const INCOMPLETE_COUNT: &str = "Incomplete connections";
pub const RATSNEST_TOGGLE: &str = "Ratsnest";
pub const RATSNEST_TOOLTIP: &str = "Toggle the ratsnest display (G)";
pub const VIOLATIONS_TOGGLE: &str = "Violations";
pub const VIOLATIONS_TOOLTIP: &str = "Toggle the clearance violation markers (V)";
pub const DRC_RUNNING: &str = "Checking…";
pub const CALCULATING: &str = "Calculating…";

/// Java `String.valueOf(float)`: the shortest decimal that reads back as the
/// same float, in plain notation from 1e-3 up to (not including) 1e7 and as
/// computerized scientific notation (`1.0E7`) outside.
pub fn java_float_to_string(value: f32) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    if value == 0.0 {
        return if value.is_sign_negative() {
            "-0.0"
        } else {
            "0.0"
        }
        .into();
    }
    // Rust's `{:e}` gives the shortest round-trip digits: "d.ddde<exp>".
    let sci = format!("{value:e}");
    let (mantissa, exp) = sci.split_once('e').expect("exponent");
    let exp: i32 = exp.parse().expect("exponent number");
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(m) => ("-", m),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    if (-3..7).contains(&exp) {
        let body = if exp >= 0 {
            let point = exp as usize + 1;
            let int = if digits.len() >= point {
                digits[..point].to_string()
            } else {
                format!("{digits}{}", "0".repeat(point - digits.len()))
            };
            let frac = if digits.len() > point {
                &digits[point..]
            } else {
                "0"
            };
            format!("{int}.{frac}")
        } else {
            format!("0.{}{digits}", "0".repeat((-exp - 1) as usize))
        };
        format!("{sign}{body}")
    } else {
        let frac = if digits.len() > 1 { &digits[1..] } else { "0" };
        format!("{sign}{}.{frac}E{exp}", &digits[..1])
    }
}

/// Java `NumberFormat.getInstance(Locale.ENGLISH)` with at most 4 fraction
/// digits: grouping by thousands, no trailing zeros, half-even rounding.
pub fn format_number(value: f64) -> String {
    let fixed = format!("{value:.4}");
    let (int, frac) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let (sign, int) = match int.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", int),
    };
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        format!("{sign}{grouped}")
    } else {
        format!("{sign}{grouped}.{frac}")
    }
}

/// Java `FloatPoint.toString(Locale.ENGLISH)`: `(x , y)`.
pub fn format_point(x: f64, y: f64) -> String {
    format!("({} , {})", format_number(x), format_number(y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_replaces_placeholders_in_order() {
        assert_eq!(
            fill(
                CLEARANCE_TEMPLATE,
                &["0.0100", "pin", "trace [GND]", "(1.5 , 2)", "F.Cu"]
            ),
            "A clearance violation of 0.0100 mm exists between pin and trace [GND] at (1.5 , 2) on layer F.Cu."
        );
        // Names are ignored: the arguments go by position.
        assert_eq!(fill("{{b}} {{a}}", &["1", "2"]), "1 2");
        // Too few arguments leave the rest; icons are skipped.
        assert_eq!(
            fill("{{icon:cog}} {{a}} {{b}}", &["x"]),
            "{{icon:cog}} x {{b}}"
        );
        // The same placeholder twice is replaced at once by the first
        // argument; the second match still takes the next argument (Java's
        // `argIndex` counts matches), so `{{b}}` gets the third.
        assert_eq!(fill("{{a}}-{{a}}-{{b}}", &["1", "2", "3"]), "1-1-3");
    }

    #[test]
    fn java_float_strings() {
        for (v, s) in [
            (0.0f32, "0.0"),
            (1.0, "1.0"),
            (12.5, "12.5"),
            (0.001, "0.001"),
            (0.0001, "1.0E-4"),
            (1234567.0, "1234567.0"),
            (1.0e7, "1.0E7"),
            (12345678.0, "1.2345678E7"),
            (-3.25, "-3.25"),
            (100.0, "100.0"),
            (0.1, "0.1"),
            (123.456, "123.456"),
        ] {
            assert_eq!(java_float_to_string(v), s, "{v}");
        }
    }

    #[test]
    fn numbers_like_number_format() {
        assert_eq!(format_number(1234567.891_23), "1,234,567.8912");
        assert_eq!(format_number(2.0), "2");
        assert_eq!(format_number(-0.5), "-0.5");
        assert_eq!(format_number(0.12345), "0.1235");
        assert_eq!(format_number(-0.00001), "-0");
        assert_eq!(format_point(1.0, -2.5), "(1 , -2.5)");
    }
}
