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
// Q20.3: Info > Board Rules.
pub const MENU_BOARD_RULES: &str = "Board Rules";
pub const BOARD_RULES_TITLE: &str = "Board Rules";
pub const BOARD_RULES_EMPTY: &str = "No copper breaks a board rule.";
pub const BOARD_RULE_TEMPLATE: &str = "{{rule}}: {{items}} at {{location}}{{layer}}";
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
pub const TOOL_SELECT: &str = "Inspect";
pub const TOOL_SELECT_TIP: &str = "Inspect mode: click to select items (S)";
pub const TOOL_ROUTE: &str = "Route";
pub const TOOL_ROUTE_TIP: &str = "Route mode: click a pin or trace to start a route (R)";
pub const TOOL_DRAG: &str = "Drag";
pub const TOOL_DRAG_TIP: &str = "Drag mode: drag items, or push traces aside (D)";
pub const RATSNEST_TOGGLE: &str = "Ratsnest";
pub const RATSNEST_TOOLTIP: &str = "Toggle the ratsnest display (G)";
pub const VIOLATIONS_TOGGLE: &str = "Violations";
pub const VIOLATIONS_TOOLTIP: &str = "Toggle the clearance violation markers (V)";
pub const DRC_RUNNING: &str = "Checking…";
pub const CALCULATING: &str = "Calculating…";

// ---------------------------------------------------------------------------
// G04: board windows and display options
// ---------------------------------------------------------------------------

// BoardMenuInfo_en.properties, BoardMenuDisplay_en.properties,
// BoardMenuHelp_en.properties. Nets are in Java's Rules menu; the port keeps
// the Info menu for everything that reads the board.
pub const MENU_NETS: &str = "Nets";
pub const MENU_COMPONENTS: &str = "Placed Components";
pub const MENU_PACKAGES: &str = "Library Packages";
pub const MENU_PADSTACKS: &str = "Library Padstacks";
pub const MENU_DISPLAY: &str = "Display";
pub const MENU_VISIBILITY: &str = "Layer and Object Visibility";
pub const MENU_COLORS: &str = "Color Manager";
pub const MENU_MISC: &str = "Miscellaneous";
pub const MENU_HELP: &str = "Help";
pub const MENU_ABOUT: &str = "About";
/// Not in Java's menus: Java writes to a console and shows the counts in the
/// status bar; this is the window the status bar's counters open.
pub const MENU_LOG: &str = "Log";

// WindowNets_en.properties
pub const NETS_TITLE: &str = "Nets";
pub const NETS_INCOMPLETE_ONLY: &str = "Incomplete connections only";
pub const NETS_INCOMPLETE_ONLY_TIP: &str =
    "Display only incomplete connections for the selected nets.";
pub const NETS_SELECT: &str = "Select on Board";
pub const NETS_SELECT_TIP: &str = "Select the chosen nets on the board.";
pub const NET_COUNT: &str = "There are {{count}} nets on the board.";
pub const NET_EXPLANATION: &str =
    "A net groups the pins and other board items that must be electrically connected.";
/// `WindowObjectList_en.properties` `list_empty`.
pub const LIST_EMPTY: &str = "No items found.";
/// `Net.printInfo` with the words of `Common_en.properties`.
pub const NET_INFO: &str = "Net {{name}}: class {{class}}, {{terminals}} terminal items, via count {{vias}}, trace length {{length}}";
/// The row of `WindowNets`: `"Net #" + n + " (" + name + ") - Class: " + class`.
pub const NET_ROW: &str = "Net #{{number}} ({{name}}) - Class: {{class}}";

// Library windows. Java's three windows have no bundle of their own; their
// titles are the menu items.
pub const COMPONENTS_TITLE: &str = MENU_COMPONENTS;
pub const PACKAGES_TITLE: &str = MENU_PACKAGES;
pub const PADSTACKS_TITLE: &str = MENU_PADSTACKS;

// WindowVisibility_en.properties
pub const VISIBILITY_TITLE: &str = "Visibility";
pub const VISIBILITY_HEADER: &str = "Use the sliders to adjust layer and object visibility.";
pub const VISIBILITY_LAYER_SECTION: &str = "Layer Visibility";
pub const VISIBILITY_LAYER_TIP: &str =
    "Adjust the visibility of board layers from fully hidden to fully visible.";
pub const VISIBILITY_OBJECT_SECTION: &str = "Object Visibility";
pub const VISIBILITY_OBJECT_TIP: &str =
    "Adjust the visibility of board objects from fully hidden to fully visible.";
pub const VISIBILITY_RESET: &str = "Reset to Defaults";
pub const VISIBILITY_RESET_TIP: &str = "Restore the original visibility values.";
pub const OBJ_TRACES: &str = "Traces";
pub const OBJ_VIAS: &str = "Vias";
pub const OBJ_PINS: &str = "Pins";
pub const OBJ_CONDUCTION_AREAS: &str = "Conduction Areas";
pub const OBJ_KEEPOUTS: &str = "Keepouts";
pub const OBJ_VIA_KEEPOUTS: &str = "Via Keepouts";
pub const OBJ_PLACE_KEEPOUTS: &str = "Component Keepouts";
pub const OBJ_COMPONENT_OUTLINES: &str = "Component Outlines";
pub const OBJ_HIGHLIGHT: &str = "Highlight";
pub const OBJ_INCOMPLETES: &str = "Incomplete Connections";

// WindowDisplayMisc_en.properties
pub const DISPLAY_TITLE: &str = "Display Options";
pub const CROSSHAIR: &str = "Crosshair cursor";
pub const CROSSHAIR_SMALL: &str = "Small";
pub const CROSSHAIR_LARGE: &str = "Large";
pub const CROSSHAIR_TIP: &str = "Shortcut: comma key.";
pub const ROTATION: &str = "Board rotation:";
pub const ROTATION_NONE: &str = "None";
pub const ROTATION_90: &str = "90 degrees";
pub const ROTATION_180: &str = "180 degrees";
pub const ROTATION_270: &str = "-90 degrees";
pub const MIRRORING: &str = "Board mirroring:";
pub const MIRROR_NONE: &str = "None";
pub const MIRROR_LEFT_RIGHT: &str = "Mirror left to right";
pub const MIRROR_TOP_BOTTOM: &str = "Mirror top to bottom";
pub const LAYER_DIMMING: &str = "Automatic layer dimming:";
pub const LAYER_DIMMING_TIP: &str = "Use the slider to adjust automatic layer-dimming intensity.";
/// BoardToolbar_en.properties `unit_heading`.
pub const UNIT: &str = "Unit";

// WindowSelectParameter_en.properties: the virtual layers.
pub const F_SILKSCREEN: &str = "F.Silkscreen";
pub const B_SILKSCREEN: &str = "B.Silkscreen";
pub const F_COURTYARD: &str = "F.Courtyard";
pub const B_COURTYARD: &str = "B.Courtyard";
pub const F_FAB: &str = "F.Fab";
pub const B_FAB: &str = "B.Fab";
pub const F_SILKSCREEN_TIP: &str =
    "Front silkscreen with component labels and reference designators.";
pub const B_SILKSCREEN_TIP: &str =
    "Back silkscreen with component labels and reference designators.";
pub const F_COURTYARD_TIP: &str = "Front courtyard: component placement clearance boundary.";
pub const B_COURTYARD_TIP: &str = "Back courtyard: component placement clearance boundary.";
pub const F_FAB_TIP: &str =
    "Front fabrication layer showing component body outlines for assembly drawings.";
pub const B_FAB_TIP: &str =
    "Back fabrication layer with component body outlines for assembly drawings.";

// ColorManager_en.properties and ColorTableModel_en.properties
pub const COLORS_TITLE: &str = "Color Manager";
pub const COLOR_LAYER: &str = "Layer";
pub const COLOR_BACKGROUND: &str = "Background";
pub const COLOR_HIGHLIGHT: &str = "Highlight";
pub const COLOR_INCOMPLETES: &str = "Incomplete Connections";
pub const COLOR_VIOLATIONS: &str = "Clearance Violations";
pub const COLOR_OUTLINE: &str = "Outline";
pub const COLOR_SILKSCREEN_FRONT: &str = "Silkscreen Front";
pub const COLOR_SILKSCREEN_BACK: &str = "Silkscreen Back";
pub const COLOR_COURTYARD_FRONT: &str = "Courtyard Front";
pub const COLOR_COURTYARD_BACK: &str = "Courtyard Back";
pub const COLOR_FAB_FRONT: &str = "Fabrication Front";
pub const COLOR_FAB_BACK: &str = "Fabrication Back";
/// Not in Java, which has no reset.
pub const COLORS_RESET: &str = "Reset to Defaults";
pub const COLORS_RESET_TIP: &str = "Restore the original colours.";

// BoardPanelStatus_en.properties and the log window
pub const STATUS_ERRORS: &str = "Errors";
pub const STATUS_WARNINGS: &str = "Warnings";
pub const STATUS_COUNTER_TIP: &str = "Click to open the log.";
pub const STATUS_CURRENT_LAYER: &str = "Current layer:";
pub const STATUS_CURSOR: &str = "Cursor:";
pub const LOG_TITLE: &str = "Log";
pub const LOG_EMPTY: &str = "The log is empty.";
pub const LOG_LEVEL_ERROR: &str = "error";
pub const LOG_LEVEL_WARNING: &str = "warning";
pub const LOG_LEVEL_INFO: &str = "info";

// WindowAbout_en.properties. No telemetry and no sponsor dialog (G00
// question 2); the donate line is not shown.
pub const ABOUT_TITLE: &str = "About";
pub const ABOUT_DESCRIPTION: &str = "Freerouting — an open-source PCB autorouter";
pub const ABOUT_VERSION: &str = "Version:";
pub const ABOUT_COMMIT: &str = "Commit:";
pub const ABOUT_LICENSE: &str = "License: GPL-3.0";
pub const ABOUT_WARRANTY: &str = "This program is provided without warranty.";
pub const ABOUT_PORT: &str = "Rust port: https://github.com/danbrunski/freerouting-rs";
pub const ABOUT_HOMEPAGE: &str =
    "Latest release: https://github.com/freerouting/freerouting/releases";
pub const ABOUT_SUPPORT: &str =
    "Report an issue: https://github.com/freerouting/freerouting/issues";

/// The version of the app (`CARGO_PKG_VERSION`).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The git commit the app was built from. R01.2's build stamp sets
/// `FREEROUTING_GIT_COMMIT` at compile time; until it lands this is
/// "unknown".
pub const GIT_COMMIT: &str = match option_env!("FREEROUTING_GIT_COMMIT") {
    Some(commit) => commit,
    None => "unknown",
};

/// The unit selector's button text (`BoardToolbar_en.properties`).
pub fn unit_button(unit: freerouting::board::structure::Unit) -> &'static str {
    use freerouting::board::structure::Unit;
    match unit {
        Unit::Mil => "mil",
        Unit::Inch => "in",
        Unit::Mm => "mm",
        Unit::Um => "µm",
    }
}

/// The unit selector's tooltip.
pub fn unit_tooltip(unit: freerouting::board::structure::Unit) -> &'static str {
    use freerouting::board::structure::Unit;
    match unit {
        Unit::Mil => "A thousandth of an inch (mil).",
        Unit::Inch => "Inches.",
        Unit::Mm => "Millimeters.",
        Unit::Um => "Micrometers.",
    }
}

/// The unit label of the status bar (Java `Unit.toString`).
pub fn unit_label(unit: freerouting::board::structure::Unit) -> &'static str {
    unit.name()
}

/// A length given in millimetres, in `unit`, as Java's `NumberFormat` writes
/// it in the lists: grouped by thousands, at most four fraction digits.
pub fn format_length(mm: f64, unit: freerouting::board::structure::Unit) -> String {
    use freerouting::board::structure::Unit;
    format_number(Unit::scale(mm, Unit::Mm, unit))
}

/// `NumberFormat` with exactly `digits` fraction digits, grouped by
/// thousands (Java `nf.setMinimumFractionDigits(d); setMaximumFractionDigits(d)`).
pub fn format_fixed(value: f64, digits: usize) -> String {
    let fixed = format!("{value:.digits$}");
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
    if frac.is_empty() {
        format!("{sign}{grouped}")
    } else {
        format!("{sign}{grouped}.{frac}")
    }
}

/// The cursor position of the status bar, in `unit`: Java
/// `FloatPoint.toString(locale, 2, 10)`, `X` and `Y` with two fraction digits
/// padded to 10 characters. (Java prints `-y` because its user Y runs down;
/// the port's coordinates are the design file's, with Y up, as the CAD tool
/// shows them.)
pub fn format_position(x_mm: f64, y_mm: f64, unit: freerouting::board::structure::Unit) -> String {
    use freerouting::board::structure::Unit;
    let (x, y) = (
        Unit::scale(x_mm, Unit::Mm, unit),
        Unit::scale(y_mm, Unit::Mm, unit),
    );
    format!(
        "X {:>10}   Y {:>10}",
        format_fixed(x, 2),
        format_fixed(y, 2)
    )
}

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

// ---------------------------------------------------------------------------
// G06: rules dialogs. Texts follow the English bundles of
// `gui/windows/routing/` at the pin; keys are in the comments.
// ---------------------------------------------------------------------------

pub const MENU_RULES: &str = "Rules"; // BoardMenuRules_en: rules
pub const MENU_CLEARANCE_MATRIX: &str = "Clearance Matrix";
pub const MENU_VIAS: &str = "Vias";
pub const MENU_NET_CLASSES: &str = "Net Classes";
/// BoardMenuParameter_en: parameter, route.
pub const MENU_PARAMETER: &str = "Settings";
pub const MENU_ROUTING: &str = "Routing";
pub const OK: &str = "OK";
pub const CANCEL: &str = "Cancel";
pub const YES: &str = "Yes";
pub const NO: &str = "No";

// WindowNetClasses_en.properties
pub const NC_TITLE: &str = "Net Classes";
pub const NC_ADD: &str = "Add";
pub const NC_ADD_TIP: &str = "Add a new net class.";
pub const NC_REMOVE: &str = "Remove";
pub const NC_REMOVE_TIP: &str = "Remove the selected net class.";
pub const NC_ASSIGN: &str = "Assign";
pub const NC_ASSIGN_TIP: &str = "Assign net classes to nets.";
pub const NC_SELECT: &str = "Select";
pub const NC_SELECT_TIP: &str =
    "Select the items belonging to the chosen net classes on the board.";
pub const NC_SHOW_NETS: &str = "Show Nets";
pub const NC_SHOW_NETS_TIP: &str = "Display a list of all nets in the selected classes.";
pub const NC_FILTER_INCOMPLETES: &str = "Filter Incomplete Connections";
pub const NC_FILTER_INCOMPLETES_TIP: &str =
    "Display only incomplete connections for the selected net classes.";
/// The `NAME` .. `IGNORED_BY_AUTOROUTER` columns, in Java's order.
pub const NC_COLUMNS: [&str; 10] = [
    "Name",
    "Via Rule",
    "Clearance Class",
    "Trace Width",
    "Active Layers",
    "Shove Fixed",
    "Ignore Cycles with Areas",
    "Min. Trace Length",
    "Max. Trace Length",
    "Ignored by Autorouter",
];
/// `column_tooltip_1` .. `column_tooltip_9` (the name column has none).
pub const NC_COLUMN_TIPS: [&str; 9] = [
    "The via rule assigned to this net class.",
    "The clearance class used by traces in this net class.",
    "The trace width for this net class.",
    "The layers on which this trace width applies.",
    "When enabled, traces and vias in this net class cannot be displaced during push-and-shove routing and are excluded from pull-tight optimization.",
    "Whether the cycle-removal algorithm ignores cycles involving conduction areas.",
    "Minimum cumulative trace length for this net class. 0 means no minimum.",
    "Maximum cumulative trace length for this net class. 0 means no maximum.",
    "Whether this net class is ignored by the Autorouter.",
];
pub const NC_LAYERS_ALL: &str = "All Layers";
pub const NC_LAYERS_INNER: &str = "Inner Layers";
pub const NC_LAYERS_OUTER: &str = "Outer Layers";
pub const NC_LAYERS_NONE: &str = "None";
pub const NC_LAYERS_CUSTOM: &str = "Custom ({{layer_count}} layers)";
pub const NC_WIDTH_MULTIPLE: &str = "Multiple widths...";
pub const NC_DEFAULT_NOT_REMOVED: &str = "The default net class cannot be removed.";
pub const NC_IN_USE: &str = "Net class not removed because it is still used by net {{name}}.";
pub const NC_REMOVED: &str = "Net class {{name}} removed.";
pub const NC_LAYER_TITLE: &str = "Layer Routing Rules";
pub const NC_ALL: &str = "Select All";
pub const NC_OUTER: &str = "Outer";
pub const NC_INNER: &str = "Inner";
pub const NC_CLEAR: &str = "Clear All";
pub const NC_CONTAINED_NETS: &str = "Contained Nets";

// WindowAssignNetClass_en.properties
pub const ASSIGN_TITLE: &str = "Assign Net Class";
pub const ASSIGN_NET_NAME: &str = "Net name";
pub const ASSIGN_CLASS_NAME: &str = "Class name";
pub const ASSIGN_NET_NAME_TIP: &str = "The net name.";
pub const ASSIGN_CLASS_NAME_TIP: &str = "The assigned net class.";

// WindowClearanceMatrix_en.properties
pub const CM_TITLE: &str = "Clearance Matrix";
pub const CM_LAYER: &str = "Layer:";
pub const CM_LAYER_TIP: &str = "Select the layer whose clearance matrix you want to edit.";
pub const CM_ADD_CLASS: &str = "Add Class";
pub const CM_ADD_CLASS_TIP: &str = "Add a new clearance class.";
pub const CM_PRUNE: &str = "Prune";
pub const CM_PRUNE_TIP: &str = "Remove redundant clearance classes.";
pub const CM_NEW_NAME: &str = "Enter a name for the new class.";
pub const CM_CONFIRM_REMOVE: &str = "Remove clearance class {{name}}?";
pub const CM_ASSIGNED_SINGLE: &str = "The selected items are already assigned to clearance class {{class_name}}. Change the clearance anyway?";
pub const CM_ASSIGNED_PAIR: &str = "The selected items are already assigned to clearance classes {{class_a}} and {{class_b}}. Change the clearance anyway?";
pub const CM_CLASS: &str = "Class";
/// Common_en.properties: all, inner (the layer combo box).
pub const LAYER_ALL: &str = "all";
pub const LAYER_INNER: &str = "inner";

// WindowEditVias_en.properties
pub const EV_TITLE: &str = "Edit Vias";
pub const EV_COLUMNS: [&str; 4] = ["Name", "Padstack", "Clearance class", "Attach SMD"];
pub const EV_ADD: &str = "Add";
pub const EV_ADD_TIP: &str = "Add a new via.";
pub const EV_REMOVE: &str = "Remove";
pub const EV_REMOVE_TIP: &str = "Remove the selected via.";
pub const EV_NEW_VIA: &str = "new_via_";
pub const EV_LAST_NOT_REMOVED: &str = "The last via cannot be removed.";
pub const EV_IN_RULE: &str =
    "The via was not removed because it is still used by via rule {{name}}.";
pub const EV_REMOVED: &str = "Via {{name}} removed.";

// WindowVia_en.properties
pub const VIA_TITLE: &str = "Via Rules";
pub const VIA_PADSTACKS: &str = "Available Via Padstacks";
pub const VIA_VIAS: &str = "Available Vias";
pub const VIA_RULES: &str = "Via Rules";
pub const VIA_INFO: &str = "Info";
pub const VIA_INFO_TIP_PADSTACKS: &str = "Show the available via padstacks.";
pub const VIA_INFO_TIP_VIAS: &str = "Show the vias allowed by a via rule.";
pub const VIA_INFO_TIP_RULE: &str = "Show information about the selected via rule.";
pub const VIA_CREATE: &str = "Create";
pub const VIA_CREATE_TIP_PADSTACK: &str = "Create a new via padstack.";
pub const VIA_CREATE_TIP_RULE: &str = "Create a new empty via rule.";
pub const VIA_EDIT: &str = "Edit";
pub const VIA_EDIT_TIP_VIAS: &str = "Edit the vias allowed by a via rule.";
pub const VIA_EDIT_TIP_RULE: &str = "Edit the selected via rule.";
pub const VIA_REMOVE: &str = "Remove";
pub const VIA_REMOVE_TIP_PADSTACK: &str = "Remove an existing via padstack.";
pub const VIA_REMOVE_TIP_RULE: &str = "Remove the selected via rule.";
pub const VIA_ADJUST_CIRCLES: &str = "Adjust Circles";
pub const VIA_CHOOSE_TO_REMOVE: &str = "Choose a padstack to remove.";
pub const VIA_REMOVE_PADSTACK: &str = "Remove Via Padstack";
pub const VIA_NAME_EXISTS: &str = "A name already exists. Choose another name.";
pub const VIA_PADSTACK_IN_USE: &str =
    "The via padstack was not removed because it is still used by the via {{name}}.";
pub const VIA_PROMPT_RADIUS: &str = "Enter the default radius.";
pub const VIA_PROMPT_PADSTACK_NAME: &str = "Enter the name of the new padstack.";
pub const VIA_PROMPT_RULE_NAME: &str = "Enter the name of the new via rule.";
pub const VIA_RADIUS_ON_LAYER: &str = "Radius on layer {{layer}}:";
pub const VIA_REMOVE_RULE_CONFIRM: &str = "Remove via rule {{name}}?";
pub const VIA_SELECT_END_LAYER: &str = "Select the end layer";
pub const VIA_SELECT_START_LAYER: &str = "Select the start layer";
pub const VIA_SELECTED_RULE: &str = "Selected Rule";

// WindowViaRule_en.properties
pub const VR_TITLE: &str = "Via Rule";
pub const VR_APPEND: &str = "Append";
pub const VR_APPEND_TIP: &str = "Add a via to the rule.";
pub const VR_REMOVE: &str = "Remove";
pub const VR_REMOVE_TIP: &str = "Remove the selected via from the rule.";
pub const VR_MOVE_UP: &str = "Move Up";
pub const VR_MOVE_UP_TIP: &str = "Increase the priority of the selected via in the rule.";
pub const VR_MOVE_DOWN: &str = "Move Down";
pub const VR_MOVE_DOWN_TIP: &str = "Decrease the priority of the selected via in the rule.";
pub const VR_CHOOSE_TO_APPEND: &str = "Choose a via to add.";
pub const VR_APPEND_TITLE: &str = "Add Via to Rule";
pub const VR_REMOVE_CONFIRM: &str = "Remove {{via}} from the rule {{rule}}?";

// WindowRouteParameter_en.properties (the board-rule part)
pub const RP_TITLE: &str = "Routing Settings";
pub const RP_SNAP_ANGLE: &str = "Snap angle:";
pub const RP_SNAP_ANGLE_TIP: &str = "Restrict trace angles during interactive routing.";
pub const RP_90: &str = "90 degrees";
pub const RP_90_TIP: &str = "Constrain interactive trace corners to 90-degree angles.";
pub const RP_45: &str = "45 degrees";
pub const RP_45_TIP: &str = "Constrain interactive trace corners to 45-degree angles.";
pub const RP_NONE: &str = "None";
pub const RP_NONE_TIP: &str = "Allow arbitrary trace angles during interactive routing.";
pub const RP_CHANGE_SNAP_45: &str = "Existing traces with arbitrary angles are not fully supported with a 45° snap angle. Change the snap angle anyway?";
pub const RP_CHANGE_SNAP_90: &str = "Only orthogonal traces are fully supported with a 90° snap angle. Change the snap angle anyway?";
pub const RP_IGNORE_CONDUCTION: &str = "Ignore conduction areas";
pub const RP_IGNORE_CONDUCTION_TIP: &str =
    "When enabled, conduction areas are treated as non-obstacles during routing.";
pub const RP_RESTRICT_PIN_EXITS: &str = "Restrict pin exit directions";
pub const RP_RESTRICT_PIN_EXITS_TIP: &str =
    "When enabled, trace exits from rectangular pins are restricted to their shorter sides.";
pub const RP_PAD_TO_TURN: &str = "Pad-to-turn gap:";
pub const RP_PAD_TO_TURN_TIP: &str = "Minimum distance a trace must travel after leaving a pad before it can turn. Higher values are more restrictive.";
pub const RP_COMPENSATION: &str = "Clearance compensation";
pub const RP_COMPENSATION_TIP: &str = "Apply extra clearance compensation during routing.";
pub const RP_COMPENSATION_TIP_CURRENT: &str =
    "Apply extra clearance compensation during routing. Current compensation: {{value}}.";
pub const RP_ON: &str = "on";
pub const RP_OFF: &str = "off";
pub const RP_KEEPOUT_OUTSIDE: &str = "Keepout outside board outline";
pub const RP_KEEPOUT_OUTSIDE_TIP: &str =
    "When enabled, the entire area outside the board outline is treated as a keepout.";

// G05: inspect mode, selection settings, the selection toolbar, popups, history,
// keepouts and the item info window. Sources: `InteractiveState_en.properties`,
// `BoardToolbarInspectedItem_en.properties`, `PopupMenuMain_en.properties`,
// `WindowSelectParameter_en.properties`, `GuiBoardManager_en.properties`,
// `WindowObjectInfo_en.properties` and `Common_en.properties`.
pub const IN_INSPECT_MODE: &str = "In inspection mode";
pub const IN_INSPECT_MENU: &str = "In the inspection menu";
pub const IN_INSPECT_ITEM_MODE: &str = "In item inspection mode";
pub const DRAG_TO_SELECT_REGION: &str =
    "Drag with the left mouse button to select items in a region.";
pub const NOTHING_SELECTED: &str = "Nothing selected.";
pub const CLEARANCE_VIOLATIONS_FOUND: &str = "clearance violations found";
pub const UNDO_DONE: &str = "Undo";
pub const REDO_DONE: &str = "Redo";
pub const NO_MORE_UNDO: &str = "Nothing more to undo.";
pub const NO_MORE_REDO: &str = "Nothing more to redo.";
pub const SOME_ITEMS_FIXED: &str = "Some items are fixed and therefore could not be removed.";
pub const CREATING_TILE: &str = "Creating a tile-shaped keepout";
pub const CREATING_CIRCLE: &str = "Creating a circular keepout";
pub const CREATING_POLYGON: &str = "Creating a polygon keepout";
pub const KEEPOUT_DONE: &str = "Keepout created successfully.";
pub const KEEPOUT_OVERLAPS: &str = "Keepout canceled because it overlaps existing items.";
pub const ADDING_HOLE: &str = "Adding a hole to an obstacle area";
pub const HOLE_DONE: &str = "Hole added successfully";
pub const HOLE_FAILED: &str = "Failed to add hole";
pub const NO_ITEM_FOR_HOLE: &str = "No item found to which a hole can be added.";
pub const NO_AREA_FOR_HOLE: &str = "No obstacle area found in which to add a hole.";
pub const HOLE_IN_CIRCLE: &str = "Adding a hole to a circle is not yet implemented.";

/// The text of a hole construction's status key (Java's
/// `InteractiveState_en.properties`).
pub fn hole_status(key: &str) -> &'static str {
    match key {
        "adding_hole_completed" => HOLE_DONE,
        "no_item_found_for_adding_hole" => NO_ITEM_FOR_HOLE,
        "no_obstacle_area_found_for_adding_hole" => NO_AREA_FOR_HOLE,
        "adding_hole_to_circle_not_yet_implemented" => HOLE_IN_CIRCLE,
        _ => HOLE_FAILED,
    }
}

// WindowSelectParameter_en.properties
pub const SELECT_TITLE: &str = "General Settings";
pub const SELECT_LAYERS: &str = "Selection Layers:";
pub const SELECT_ALL_VISIBLE: &str = "All visible";
pub const SELECT_ALL_VISIBLE_TIP: &str =
    "Select on all visible layers; the current layer is preferred.";
pub const SELECT_CURRENT_ONLY: &str = "Current layer only";
pub const SELECT_CURRENT_ONLY_TIP: &str = "Select only on the current layer.";
pub const SELECT_ITEMS: &str = "Selectable Items:";
pub const SELECT_TRACES: &str = "traces";
pub const SELECT_VIAS: &str = "vias";
pub const SELECT_PINS: &str = "pins";
pub const SELECT_CONDUCTION: &str = "conduction areas";
pub const SELECT_CONDUCTION_TIP: &str = "Allow conduction areas (copper pours) to be selected.";
pub const SELECT_KEEPOUT: &str = "keepout";
pub const SELECT_KEEPOUT_TIP: &str = "Allow keepouts to be selected.";
pub const SELECT_VIA_KEEPOUT: &str = "via keepout";
pub const SELECT_VIA_KEEPOUT_TIP: &str = "Allow via keepout areas to be selected.";
pub const SELECT_COMPONENT_KEEPOUT: &str = "Component keepout";
pub const SELECT_COMPONENT_KEEPOUT_TIP: &str = "Allow component keepout areas to be selected.";
pub const SELECT_BOARD_OUTLINE: &str = "board outline";
pub const SELECT_BOARD_OUTLINE_TIP: &str = "Allow the board outline to be selected.";
pub const SELECT_FIXED: &str = "fixed";
pub const SELECT_FIXED_TIP: &str = "Allow fixed items to be selected.";
pub const SELECT_UNFIXED: &str = "unfixed";
pub const SELECT_UNFIXED_TIP: &str = "Allow unfixed items to be selected.";
pub const SELECT_CURRENT_LAYER: &str = "Current layer:";
pub const SELECT_CURRENT_LAYER_TIP: &str =
    "You can also change it with the number keys or '+' and '-'.";
pub const MENU_SETTINGS: &str = "Settings";
pub const MENU_SELECT_SETTINGS: &str = "Select";

// BoardToolbarInspectedItem_en.properties: the labels and tooltips of the
// ten buttons Java's inspect toolbar has, then the three added by decision
// (G00 question 6: Delete, Fix, Unfix).
pub const TB_CANCEL: &str = "Cancel";
pub const TB_CANCEL_TIP: &str = "Cancel the current selection (Esc).";
pub const TB_INFO: &str = "Info";
pub const TB_INFO_TIP: &str = "Display information about the selected items (I).";
pub const TB_NETS: &str = "Nets";
pub const TB_NETS_TIP: &str = "Extend the selection to whole nets (N).";
pub const TB_CONN_SETS: &str = "Connected Sets";
pub const TB_CONN_SETS_TIP: &str = "Extend the selection to entire connected sets (S).";
pub const TB_CONNECTIONS: &str = "Connections";
pub const TB_CONNECTIONS_TIP: &str = "Extend the selection to the next fork or terminal item (E).";
pub const TB_COMPONENTS: &str = "Components";
pub const TB_COMPONENTS_TIP: &str = "Extend the selection to entire components.";
pub const TB_VIOLATIONS: &str = "Violations";
pub const TB_VIOLATIONS_TIP: &str = "Show or hide clearance violations for the selected items (V).";
pub const TB_ZOOM_SELECTION: &str = "Zoom Selection";
pub const TB_ZOOM_SELECTION_TIP: &str = "Zoom to show the selected items (W).";
pub const TB_ZOOM_ALL: &str = "Zoom All";
pub const TB_ZOOM_ALL_TIP: &str = "Zoom to show the entire board (A).";
pub const TB_ZOOM_REGION: &str = "Zoom Region";
pub const TB_ZOOM_REGION_TIP: &str = "Zoom to a selected region of the board (R).";
pub const TB_DELETE: &str = "Delete";
pub const TB_DELETE_TIP: &str = "Delete the selected unfixed items (Del).";
pub const TB_FIX: &str = "Fix";
pub const TB_FIX_TIP: &str = "Fix the selected items.";
pub const TB_UNFIX: &str = "Unfix";
pub const TB_UNFIX_TIP: &str = "Unfix the selected items.";
pub const TB_ACCESSIBLE_NAME: &str = "Inspect Toolbar";

// PopupMenuMain_en.properties, PopupMenuInspectedItems (the extend items reuse
// the toolbar texts) and Common_en.properties (the display rows).
pub const POPUP_SELECT_ITEM: &str = "Select item (I)";
pub const POPUP_START_ROUTE: &str = "Start route (T)";
pub const POPUP_SWAP_PIN: &str = "Swap pin (W)";
pub const POPUP_CREATE_KEEPOUT: &str = "Create keepout";
pub const POPUP_TILE: &str = "Tile";
pub const POPUP_CIRCLE: &str = "Circle";
pub const POPUP_POLYGON: &str = "Polygon";
pub const POPUP_HOLE: &str = "Hole";
pub const POPUP_EXTEND_SELECTION: &str = "Extend selection to:";
pub const POPUP_DONE: &str = "Done (Enter)";
pub const POPUP_INSERT: &str = "Insert (left mouse button)";
pub const POPUP_CANCEL: &str = "Cancel (Esc)";
pub const POPUP_CENTER_DISPLAY: &str = "Center display (C)";
pub const POPUP_ZOOM_IN: &str = "Zoom in (Z)";
pub const POPUP_ZOOM_OUT: &str = "Zoom out (O)";

// WindowObjectInfo_en.properties and the words of Common_en.properties that
// `Item.printInfo` puts between the values.
pub const INFO_TITLE: &str = "Selected Items";
pub const INFO_SUMMARY: &str = "Summary:";
pub const INFO_PIN: &str = "pin";
pub const INFO_PINS: &str = "pins";
pub const INFO_VIA: &str = "via";
pub const INFO_VIAS: &str = "vias";
pub const INFO_TRACE: &str = "trace, cumulative length";
pub const INFO_TRACES: &str = "traces, cumulative length";
pub const INFO_EMPTY: &str = "The list is empty.";

// G09: the editor's status messages (`InteractiveState_en.properties`), the
// route and move popups (`PopupMenuMain`, `PopupMenuMove`; the bundles of the
// route popups are missing at the pin, so their words are Java's keys in
// plain English) and the interactive part of `WindowRouteParameter`.
pub const ROUTING_NET: &str = "Routing net {{net}}.";
pub const LAYER_CHANGED_TO: &str = "Layer changed to {{layer}}.";
pub const LAYER_NOT_CHANGED_INACTIVE: &str =
    "Layer not changed because {{layer}} is not active for the current net.";
pub const LAYER_NOT_CHANGED_OBSTACLE: &str =
    "Layer not changed because of an obstacle on layer {{layer}}.";
pub const DRAGGING_ITEM: &str = "Dragging item";
pub const NO_PIN_SELECTED: &str = "No pin selected.";
pub const NO_SWAPPABLE_PIN: &str = "No swappable pin found.";
pub const CLICK_SECOND_PIN: &str = "Click the second pin with the left mouse button.";
pub const PIN_NOT_SWAPPED_CONNECTED: &str = "Pin not swapped because it is already connected.";
pub const PIN_NOT_SWAPPED_SECOND_CONNECTED: &str =
    "Pin not swapped because the second pin is already connected.";
pub const PIN_SWAP_COMPLETED: &str = "Pin swap completed.";
pub const PIN_TO_SWAP_MISSING: &str = "The pin to swap is missing.";
pub const INSERTION_FAILED: &str = "Insertion failed because of obstacles.";
pub const MOVE_COMPLETED: &str = "Move completed.";
pub const CANNOT_CHANGE_SIDE: &str = "Cannot change the placement side.";
pub const UNFIX_BEFORE_MOVING: &str = "Unfix the selected items before moving them.";
pub const UNROUTE_BEFORE_MOVING: &str =
    "Unroute the selected items or extend the selection before moving.";
pub const CANNOT_RESET_ROTATIONS: &str = "Unable to reset components with different rotations.";
pub const SOME_ITEMS_CANNOT_BE_MOVED: &str = "Some items cannot be moved because they are fixed.";
pub const ITEMS_NOT_INSERTED: &str = "Some items were not inserted because of obstacles.";
pub const ITEMS_ALREADY_CONNECTED: &str =
    "Some items were not changed because they are already connected.";

/// The text of an editor status (`interactive::Status`): Java's
/// `InteractiveState_en.properties` for the key, with its arguments
/// filled in. A key without a text here shows as the key itself, so a new
/// engine message is never lost.
pub fn interactive_status(status: &freerouting::interactive::Status) -> String {
    let args: Vec<&str> = status.args.iter().map(String::as_str).collect();
    let template = match status.key {
        "routing_net_message" => ROUTING_NET,
        "layer_changed_to_message" => LAYER_CHANGED_TO,
        "layer_not_changed_inactive_net_message" => LAYER_NOT_CHANGED_INACTIVE,
        "layer_not_changed_obstacle_message" => LAYER_NOT_CHANGED_OBSTACLE,
        "dragging_item" => DRAGGING_ITEM,
        "no_pin_selected" => NO_PIN_SELECTED,
        "no_swappable_pin_found" => NO_SWAPPABLE_PIN,
        "please_click_second_pin_with_the_left_mouse_button" => CLICK_SECOND_PIN,
        "pin_not_swapped_because_it_is_already_connected" => PIN_NOT_SWAPPED_CONNECTED,
        "pin_not_swapped_because_second_pin_is_already_connected" => {
            PIN_NOT_SWAPPED_SECOND_CONNECTED
        }
        "pin_swap_completed" => PIN_SWAP_COMPLETED,
        "pin_to_swap_missing" => PIN_TO_SWAP_MISSING,
        "insertion_failed_because_of_obstacles" => INSERTION_FAILED,
        "move_completed" => MOVE_COMPLETED,
        "cannot_change_placement_side" => CANNOT_CHANGE_SIDE,
        "please_unfix_selected_items_before_moving" => UNFIX_BEFORE_MOVING,
        "please_unroute_or_extend_selection_before_moving" => UNROUTE_BEFORE_MOVING,
        "unable_to_reset_components_with_different_rotations" => CANNOT_RESET_ROTATIONS,
        "some_items_cannot_be_moved_because_they_are_fixed" => SOME_ITEMS_CANNOT_BE_MOVED,
        "some_items_not_inserted_because_of_obstacles" => ITEMS_NOT_INSERTED,
        "some_items_are_not_changed_because_they_are_already_connected" => ITEMS_ALREADY_CONNECTED,
        "in_inspect_mode" => IN_INSPECT_MODE,
        "nothing_selected" => NOTHING_SELECTED,
        "drag_left_mouse_button_to_select_items_in_region" => DRAG_TO_SELECT_REGION,
        "some_items_are_fixed_and_could_therefore_not_be_removed" => SOME_ITEMS_FIXED,
        "undo" => UNDO_DONE,
        "redo" => REDO_DONE,
        "no_more_undo_possible" => NO_MORE_UNDO,
        "no_more_redo_possible" => NO_MORE_REDO,
        "adding_hole_to_obstacle_area" => ADDING_HOLE,
        "adding_hole_completed"
        | "no_item_found_for_adding_hole"
        | "no_obstacle_area_found_for_adding_hole"
        | "adding_hole_to_circle_not_yet_implemented"
        | "adding_hole_failed" => {
            return hole_status(status.key).to_string();
        }
        other => return other.to_string(),
    };
    fill(template, &args)
}

pub const POPUP_END_ROUTE: &str = "End route";
pub const POPUP_CANCEL_ROUTE: &str = "Cancel route";
pub const POPUP_CHANGE_LAYER: &str = "Change layer";
pub const POPUP_TURN: &str = "Turn";
pub const POPUP_TURN_90: &str = "90 degrees (+)";
pub const POPUP_TURN_180: &str = "180 degrees (*)";
pub const POPUP_TURN_270: &str = "-90 degrees (-)";
pub const POPUP_CHANGE_SIDE: &str = "Change placement side (/)";
pub const POPUP_RESET_ROTATION: &str = "Reset rotation";
pub const POPUP_MOVE_SELECTION: &str = "Move selection (M)";
pub const POPUP_COPY_SELECTION: &str = "Copy selection (Ctrl+D)";

// WindowRouteParameter_en.properties, the interactive fields.
pub const RP_ROUTE_MODE: &str = "Route mode:";
pub const RP_ROUTE_MODE_TIP: &str = "Switch to stitching mode when routing shove-fixed nets.";
pub const RP_DYNAMIC: &str = "Dynamic";
pub const RP_DYNAMIC_TIP: &str = "Route interactively while moving the mouse.";
pub const RP_STITCHING: &str = "Stitching";
pub const RP_STITCHING_TIP: &str = "Click to place corners and manually stitch the route.";
pub const RP_RULE_SELECTION: &str = "Rule selection:";
pub const RP_RULE_SELECTION_TIP: &str = "Choose manual routing rules or use the net rules.";
pub const RP_AUTOMATIC: &str = "Automatic";
pub const RP_AUTOMATIC_TIP: &str = "Use routing rules defined in the net settings.";
pub const RP_MANUAL: &str = "Manual";
pub const RP_MANUAL_TIP: &str = "Select routing rules and trace widths manually.";
pub const RP_PUSH_SHOVE: &str = "Push-and-shove enabled";
pub const RP_PUSH_SHOVE_TIP: &str =
    "Allow interactive routing to displace existing traces (push-and-shove).";
pub const RP_DRAG_COMPONENTS: &str = "Drag components enabled";
pub const RP_DRAG_COMPONENTS_TIP: &str =
    "Enable dragging components while holding the left mouse button.";
pub const RP_VIA_SNAP: &str = "Vias snap to SMD pad centers";
pub const RP_VIA_SNAP_TIP: &str =
    "When enabled, vias attached to SMD pins snap to the centers of their pads.";
pub const RP_HIGHLIGHT_OBSTACLE: &str = "Highlight routing obstacle";
pub const RP_HIGHLIGHT_OBSTACLE_TIP: &str =
    "Highlight the current routing obstacle in dynamic routing.";
pub const RP_NECKDOWN: &str = "Automatic neckdown";
pub const RP_NECKDOWN_TIP: &str = "When enabled, trace width is automatically reduced at pins narrower than the current trace width.";
pub const RP_PULL_TIGHT_REGION: &str = "Pull-tight radius:";
pub const RP_PULL_TIGHT_REGION_TIP: &str = "The radius around the cursor within which traces are straightened after each routing step. Smaller values limit cleanup to a smaller area and are faster; the maximum applies to the entire board.";
pub const RP_PULL_TIGHT_WHOLE_BOARD: &str = "entire board (no limit)";
pub const RP_PULL_TIGHT_ACCURACY: &str = "Pull-tight accuracy:";
pub const RP_PULL_TIGHT_ACCURACY_TIP: &str = "Routing precision. Lower values are slower but more precise; higher values are faster but less precise.";
// ManualRulesPanel_en.properties
pub const MR_TITLE: &str = "Manual Rules";
pub const MR_ALL_LAYERS: &str = "all layers";
pub const MR_VIA_RULE: &str = "Via rule:";
pub const MR_VIA_RULE_TIP: &str = "Via rule used for layer changes during manual routing.";
pub const MR_CLEARANCE_CLASS: &str = "Trace clearance class:";
pub const MR_CLEARANCE_CLASS_TIP: &str = "Clearance class used by traces during manual routing.";
pub const MR_TRACE_WIDTH: &str = "Trace width:";
pub const MR_TRACE_WIDTH_TIP: &str = "Trace width used for manual routing.";
pub const MR_ON_LAYER: &str = "On layer:";
pub const MR_ON_LAYER_TIP: &str = "Layer on which to apply the manual trace width.";
// WindowMoveParameter_en.properties
pub const MV_TITLE: &str = "Component Movement Settings";
pub const MENU_MOVE_SETTINGS: &str = "Component Movement";
pub const MV_HORIZONTAL_GRID: &str = "Horizontal component placement grid spacing:";
pub const MV_HORIZONTAL_GRID_TIP: &str = "Horizontal spacing of the component placement grid.";
pub const MV_VERTICAL_GRID: &str = "Vertical component placement grid spacing:";
pub const MV_VERTICAL_GRID_TIP: &str = "Vertical spacing of the component placement grid.";

// ---------------------------------------------------------------------------
// G07: the autorouter. `WindowAutorouteParameter_en`, `ScreenMessages_en`,
// `WindowRoutingSummary_en`, `BoardToolbar_en` and `BoardMenuParameter_en`.
// ---------------------------------------------------------------------------

pub const MENU_AUTOROUTER: &str = "Autorouter"; // BoardMenuParameter_en: autoroute
pub const AUTOROUTE_BUTTON: &str = "Autoroute";
pub const AUTOROUTE_BUTTON_TOOLTIP: &str = "Start the autorouter and route optimizer.";
pub const CANCEL_BUTTON: &str = "Cancel";
pub const CANCEL_BUTTON_TOOLTIP: &str = "Stop the current process.";
pub const AUTOROUTE_SETTINGS_TITLE: &str = "Autorouter Settings";
pub const POWER_LAYER_TOOLTIP: &str =
    "Disabled because this is not a signal layer and cannot be used for regular trace routing.";
pub const BEND_COST_TOOLTIP: &str = "Per-layer bend penalty added to the expansion cost each time the router changes direction on this layer. Valid range: 0.0 (no penalty; default) to 9.9 (strongly discourages bends). Set a high value on a single-sided or jump-wire layer to favor straight traces.";
pub const FANOUT_TOOLTIP: &str = "The fanout stage creates initial vias and short trace segments from dense-component pads (such as BGA pads) to other PCB layers.";
pub const AUTOROUTE_TOOLTIP: &str = "During the Auto-routing stage, the Autorouter establishes electrical connections by placing traces and vias.";
pub const OPTIMIZATION_TOOLTIP: &str =
    "During the optimization stage, the initial routes are refined to improve the overall design.";
pub const VIAS_ALLOWED_TOOLTIP: &str = "Allow the Autorouter to change layers by placing vias.";
pub const VIA_COSTS_TOOLTIP: &str = "Cost of changing layers with a via. Higher values keep the router on the current layer longer.";
pub const PLANE_VIA_COSTS_TOOLTIP: &str =
    "Cost of a via used when a net connects to a power or ground plane.";
pub const RIPUP_COSTS_TOOLTIP: &str = "Initial penalty for ripping up existing traces. Higher values make the router less likely to undo existing work.";
pub const MAX_PASSES_TOOLTIP: &str =
    "Set the maximum number of routing passes. Use 0 for no limit.";
pub const MAX_THREADS_TOOLTIP: &str = "Number of routing threads. 1 uses one CPU core; higher values use more cores, up to the limit of this machine.";
pub const JOB_TIMEOUT_TOOLTIP: &str = "Set the routing timeout. The value is capped at four weeks.";

// ScreenMessages_en.properties
pub const BATCH_AUTOROUTE_ADD: &str = "To route: {{items_to_go}}; routed: {{routed_count}}";
pub const BATCH_AUTOROUTE_LAYER: &str = "Ripped up: {{ripped_count}}; failed: {{failed_count}}";
pub const BATCH_FANOUT_LAYER: &str = "Failed: {{failed_count}}; extra vias: +{{extra_vias}}";
pub const SCORE: &str = "Score: {{score}} ({{incomplete_count}} incomplete connections, {{violation_count}} clearance violations)";
pub const AUTOROUTER_STARTED: &str = "Autorouter is running. Click anywhere on the canvas to cancel. Autorouter pass #{{autorouter_pass_number}}";
pub const OPTIMIZER_STARTED: &str = "Optimizer is running. Click anywhere on the canvas to cancel. Optimizer pass #{{optimizer_pass_number}}.";
pub const BATCH_AUTOROUTER_START_MESSAGE: &str =
    "Autorouter is running. Click anywhere on the canvas to cancel.";
pub const BATCH_OPTIMIZER_START_MESSAGE: &str =
    "Optimizer is running. Click anywhere on the canvas to cancel.";
pub const AUTOROUTE_END_MESSAGE: &str =
    "Autoroute {{status}}; {{incomplete_count}} connections remain unrouted.";
// Port-only variants of the four lines above for "Stop routing on a canvas
// click" switched off (`PersistedSettings::stop_on_canvas_click`): the same
// lines without the click, naming what stops the job instead.
pub const AUTOROUTER_STARTED_NO_CLICK: &str = "Autorouter is running. Press Esc or Cancel to stop it. Autorouter pass #{{autorouter_pass_number}}";
pub const OPTIMIZER_STARTED_NO_CLICK: &str = "Optimizer is running. Press Esc or Cancel to stop it. Optimizer pass #{{optimizer_pass_number}}.";
pub const BATCH_AUTOROUTER_START_MESSAGE_NO_CLICK: &str =
    "Autorouter is running. Press Esc or Cancel to stop it.";
pub const BATCH_OPTIMIZER_START_MESSAGE_NO_CLICK: &str =
    "Optimizer is running. Press Esc or Cancel to stop it.";
/// Port-only: the status line when a canvas click is ignored during a job.
pub const ROUTING_CLICK_IGNORED: &str =
    "A click does not stop the autorouter. Press Esc or Cancel to stop it.";
/// Port-only: the settings dialog's checkbox and its tooltip.
pub const STOP_ON_CLICK_CHECKBOX: &str = "Stop routing on a canvas click";
pub const STOP_ON_CLICK_TOOLTIP: &str = "On (Java's behaviour): a left click on the canvas stops a running job. Off: only Cancel, Esc and the stop button do.";
/// Port-only: front-end mode after the user stopped a job. The output is
/// written and the window stays open (Java exits at once).
pub const FRONTEND_STOPPED_KEPT_OPEN: &str = "Routing stopped. The result is saved to {{file}}; close the window to return it, or route again.";
pub const OPTIMIZATION_END_MESSAGE: &str = "Optimization {{status}}.";
pub const COMPLETED: &str = "completed";
pub const INTERRUPTED: &str = "interrupted";
/// Port-only: the status line when an edit is refused while the router runs
/// (Java makes the board read-only).
pub const OPEN_A_BOARD: &str = "Open a board first.";
pub const ROUTING_READ_ONLY: &str =
    "The autorouter is running; the board can't be edited until it finishes.";
/// Port-only: the status line after Cancel, until the job hands its board back.
pub const ROUTING_STOPPING: &str =
    "Stopping the autorouter: it finishes the current step and keeps the best board so far.";
/// Port-only: the status line when a stop takes longer than `STOP_GRACE`.
pub const ROUTING_STOP_SLOW: &str = "The autorouter has not stopped yet. Close the window again to quit without waiting; this run's routing is then discarded.";
/// Port-only: the log line when a stop has been answered.
pub const ROUTING_STOPPED_AFTER: &str =
    "The autorouter stopped {{seconds}} s after the stop request.";
/// Port-only: the log line when the window closes without waiting for the job.
pub const ROUTING_ABANDONED: &str =
    "Closed without waiting for the autorouter; nothing was written.";

// WindowRoutingSummary_en.properties
pub const SUMMARY_TITLE: &str = "Routing Summary";
pub const SUMMARY_HEADER_COMPLETED: &str = "Auto-routing Completed";
pub const SUMMARY_HEADER_INTERRUPTED: &str = "Auto-routing Interrupted";
pub const SUMMARY_NETS_TOTAL: &str = "Total Nets:";
pub const SUMMARY_CONNECTIONS_INCOMPLETE: &str = "Incomplete connections:";
pub const SUMMARY_ZERO_INCOMPLETES: &str = "0 (100% routed)";
pub const SUMMARY_CLEARANCE_VIOLATIONS: &str = "Clearance Violations:";
pub const SUMMARY_VIAS_ADDED: &str = "Vias Added:";
pub const SUMMARY_TRACE_LENGTH: &str = "Total Trace Length:";
pub const SUMMARY_SCORE: &str = "Score:";
pub const SUMMARY_EXECUTION_TIME: &str = "Execution Time:";
pub const SUMMARY_SHOW_CHECKBOX: &str = "Show this summary after auto-routing finishes";
pub const SUMMARY_CLOSE: &str = "Close";

// G08: saving, the prompts and the front-end mode.
// BoardMenuFile_en.properties
pub const SAVE_AS: &str = "Save as...";
pub const SAVE_MESSAGE: &str = "Design file saved.";
pub const ERROR_DESIGN_SAVE_FAILED: &str = "Unable to save the design file.";
// BoardFrame_en.properties
pub const MESSAGE_SPECCTRA_SES_SAVED: &str =
    "Specctra session file '{{filename}}' saved successfully.";
pub const MESSAGE_SPECCTRA_SES_SAVE_FAILED: &str =
    "Unable to save the Specctra session file '{{filename}}'.";
pub const CONFIRM_RULES_SAVE: &str =
    "Would you like to save the current design rules to a separate .rules file?";
pub const CONFIRM_SAVE_TITLE: &str = "Unsaved Changes";
pub const CONFIRM_SAVE_CHANGES: &str =
    "The current board has unsaved changes. Would you like to save it before loading a new file?";
pub const CONFIRM_SAVE_YES: &str = "Save";
pub const CONFIRM_SAVE_NO: &str = "Don't Save";
pub const CONFIRM_SAVE_CANCEL: &str = "Cancel";
pub const CONFIRM_EXIT_TITLE: &str = "Warning";
pub const CONFIRM_EXIT: &str =
    "The board has changed since you last saved it.\nAre you sure you want to exit?";
pub const CONFIRM_EXIT_YES: &str = "Yes";
pub const CONFIRM_EXIT_NO: &str = "No";
// GuiManager_en.properties
pub const CONFIRM_IMPORT_RULES: &str = "Please confirm that you want to import the stored rules.\nRules imported from the host system may overwrite your current rules.";
pub const AUTO_START_ROUTING_TITLE: &str = "Autorouter Confirmation";
pub const AUTO_START_ROUTING_MESSAGE: &str = "The Autorouter is about to start.\n\nDefault settings will be used unless you cancel now and change them.\nYou can start Auto-routing later by clicking the Autoroute button.";
pub const AUTO_START_ROUTING_STARTNOW: &str = "Start Now";
pub const AUTO_START_ROUTING_CANCEL: &str = "Cancel";
/// The Select an Option title of `JOptionPane.showConfirmDialog`, which
/// Java's yes/no questions (`WindowMessage.confirm`) use.
pub const SELECT_AN_OPTION: &str = "Select an Option";
pub const SAVE_FILTER_SES: &str = "Specctra session file (*.ses)";
pub const SAVE_FILTER_DSN: &str = "Specctra design file (*.dsn)";
/// Port-only texts.
pub const SAVE_BOARD_TITLE: &str = "Save as";
pub const SAVE_BUTTON: &str = "Save";
pub const FILE_NAME_LABEL: &str = "File name:";
pub const OVERWRITE_CONFIRM: &str =
    "The file '{{filename}}' already exists. Do you want to replace it?";
pub const SAVE_UNSUPPORTED_EXTENSION: &str =
    "Only .ses and .dsn files can be saved; '{{filename}}' has another extension.";
pub const SAVE_WHILE_ROUTING: &str = "The autorouter is running; stop it before saving.";
pub const ERROR_RULES_SAVE_FAILED: &str = "Unable to save the rules file '{{filename}}'.";
pub const MESSAGE_RULES_SAVED: &str = "Rules file '{{filename}}' saved successfully.";
pub const RULES_IMPORTED: &str = "Rules imported from '{{filename}}'.";
pub const RULES_IMPORT_FAILED: &str = "Failed to read the rules file '{{filename}}': {{reason}}";
pub const RULES_NOT_FOUND: &str = "Rules file not found: '{{filename}}'.";

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

    /// Java: `Unit.scale(2.54, MM, MIL)` is 100 (25.4 um per mil), and the
    /// lists print it through `NumberFormat` with at most four fraction
    /// digits, which gives "100".
    #[test]
    fn mil_is_mm_over_0_0254() {
        use freerouting::board::structure::Unit;
        assert_eq!(format_length(2.54, Unit::Mil), "100");
        assert_eq!(format_length(2.54, Unit::Mm), "2.54");
        assert_eq!(format_length(2.54, Unit::Um), "2,540");
        assert_eq!(format_length(25.4, Unit::Inch), "1");
        assert_eq!(format_length(0.1, Unit::Mil), "3.937");
    }

    #[test]
    fn status_position_is_padded_with_two_digits() {
        use freerouting::board::structure::Unit;
        assert_eq!(
            format_position(12.345, -0.5, Unit::Mm),
            "X      12.35   Y      -0.50"
        );
        assert_eq!(
            format_position(25.4, 2540.0, Unit::Inch),
            "X       1.00   Y     100.00"
        );
        assert_eq!(format_fixed(1234567.891, 2), "1,234,567.89");
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
