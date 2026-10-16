//! Info > Length Violations: the rows of Java's `WindowLengthViolations`.
//!
//! One row per net whose traces are too long or too short, sorted by net
//! name ignoring case. Java keeps them in a `TreeSet`, which drops a net
//! whose name equals another's ignoring case; this list keeps both
//! (DIVERGENCES: GUI Info lists). Lengths are in the design's unit and are
//! printed as Java's `String.valueOf(float)`.

use freerouting::board::BasicBoard;
use freerouting::board::items::ItemId;

use super::object_list::Row;
use crate::overlays::Frame;
use crate::text;

/// Java `String.compareToIgnoreCase`.
pub fn compare_ignore_case(a: &str, b: &str) -> std::cmp::Ordering {
    let key = |s: &str| -> Vec<char> {
        s.chars()
            .map(|c| c.to_uppercase().next().unwrap_or(c))
            .map(|c| c.to_lowercase().next().unwrap_or(c))
            .collect()
    };
    key(a).cmp(&key(b))
}

/// The row of one net; `violation` is the signed amount, positive when the
/// traces are too long.
pub fn row_text(board: &BasicBoard, frame: &Frame, net_number: i32, violation: f64) -> String {
    let Some(net) = board.rules.nets.get(net_number) else {
        return String::new();
    };
    let classes = &board.rules.net_classes;
    let length = frame.user_len(board.net_trace_length(net_number)) as f32;
    let (template, allowed) = if violation > 0.0 {
        (text::LENGTH_MAX, net.maximum_trace_length(classes))
    } else {
        (text::LENGTH_MIN, net.minimum_trace_length(classes))
    };
    text::fill(
        template,
        &[
            &net.name,
            &text::java_float_to_string(length),
            &text::java_float_to_string(frame.user_len(allowed) as f32),
        ],
    )
}

/// The rows; `violations` are (net number, signed violation) as the
/// analysis found them.
pub fn rows(board: &BasicBoard, violations: &[(i32, f64)]) -> Vec<Row> {
    let frame = Frame::new(board);
    let name = |n: i32| board.rules.nets.get(n).map_or("", |net| net.name.as_str());
    let mut sorted: Vec<&(i32, f64)> = violations.iter().collect();
    sorted.sort_by(|a, b| compare_ignore_case(name(a.0), name(b.0)));
    sorted
        .into_iter()
        .map(|(net, violation)| Row {
            text: row_text(board, &frame, *net, *violation),
            items: board
                .items()
                .filter(|i| i.contains_net(*net))
                .map(|i| i.id())
                .collect::<Vec<ItemId>>(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn names_compare_ignoring_case() {
        assert_eq!(compare_ignore_case("gnd", "GND"), Ordering::Equal);
        assert_eq!(compare_ignore_case("a1", "B1"), Ordering::Less);
        assert_eq!(compare_ignore_case("Zed", "alpha"), Ordering::Greater);
    }
}
