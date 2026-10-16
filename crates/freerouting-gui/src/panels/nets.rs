//! Info > Nets: the rows of Java's `WindowNets` (G04.1).
//!
//! One row per net, in net number order, reading
//! `Net #n (name) - Class: class`. The filter looks at `Net #n (name)`, as
//! Java's does (`Net.toString`). A chosen row also shows what
//! `Net.printInfo` prints for the net: its terminal items, its vias and the
//! length of its traces. Select on Board selects the net's connectable items
//! (Java walks every item of the board and keeps those that contain the net;
//! `connectable_items` is the same set for pins, vias, traces and areas).

use std::collections::BTreeMap;

use freerouting::board::BasicBoard;
use freerouting::board::structure::Unit;

use super::object_list::Row;
use crate::analysis::Ratsnest;
use crate::overlays::Frame;
use crate::text;

/// How many incomplete connections each net has, from the airlines of the
/// ratsnest (Java `RatsNest.incompleteCount(net)`).
pub fn incomplete_counts(ratsnest: &Ratsnest) -> BTreeMap<i32, usize> {
    let mut counts = BTreeMap::new();
    for line in &ratsnest.airlines {
        *counts.entry(line.net_number).or_insert(0) += 1;
    }
    counts
}

/// Java `Net.toString`.
pub fn net_string(number: i32, name: &str) -> String {
    text::fill("Net #{{number}} ({{name}})", &[&number.to_string(), name])
}

/// Java `Net.printInfo`, as one line: the terminal items are counted, the
/// trace length is in `unit`.
pub fn net_info(board: &BasicBoard, frame: &Frame, unit: Unit, number: i32) -> String {
    let Some(net) = board.rules.nets.get(number) else {
        return String::new();
    };
    let class = board.rules.net_classes.get(net.net_class()).name();
    let length = text::format_length(frame.len(board.net_trace_length(number)), unit);
    text::fill(
        text::NET_INFO,
        &[
            &net.name,
            class,
            &board.net_terminal_items(number).len().to_string(),
            &board.net_via_count(number).to_string(),
            &length,
        ],
    )
}

/// The rows of the list. With `incomplete_only`, nets without an incomplete
/// connection are left out (`counts` has the nets that have one).
pub fn rows(
    board: &BasicBoard,
    unit: Unit,
    incomplete_only: bool,
    counts: &BTreeMap<i32, usize>,
) -> Vec<Row> {
    let frame = Frame::new(board);
    board
        .rules
        .nets
        .iter()
        .filter(|net| !incomplete_only || counts.get(&net.net_number).is_some_and(|c| *c > 0))
        .map(|net| {
            let class = board.rules.net_classes.get(net.net_class()).name();
            Row {
                text: text::fill(
                    text::NET_ROW,
                    &[&net.net_number.to_string(), &net.name, class],
                ),
                items: board.connectable_items(net.net_number),
                detail: Some(net_info(board, &frame, unit, net.net_number)),
                filter_text: Some(net_string(net.net_number, &net.name)),
            }
        })
        .collect()
}

/// The header line: `There are n nets on the board.`
pub fn count_line(board: &BasicBoard) -> String {
    text::fill(
        text::NET_COUNT,
        &[&board.rules.nets.iter().count().to_string()],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DESIGN: &str = r#"(pcb t
      (resolution um 10)
      (structure
        (layer F.Cu (type signal)) (layer B.Cu (type signal))
        (boundary (rect pcb 0 0 100000 50000))
        (via v))
      (placement (component fp (place U1 1000 2000 front 0) (place U2 9000 2000 front 0)))
      (library
        (image fp (pin sq 1 0 0))
        (padstack sq (shape (rect F.Cu -10 -10 10 10)))
        (padstack v (shape (circle F.Cu 50)) (shape (circle B.Cu 50))))
      (network (net GND (pins U1-1 U2-1)) (net SIG (pins U1-1)))
      (wiring (via v 5000 5000 (net GND)) (wire (path B.Cu 20 0 0 1000 0) (net GND))))"#;

    #[test]
    fn rows_follow_javas_format_and_filter_on_the_net_string() {
        let pcb = freerouting::dsn::Pcb::parse(DESIGN).unwrap();
        let result = freerouting::io::specctra::build_board(&pcb);
        let board = result.board().unwrap().basic();
        let rows = rows(board, Unit::Mm, false, &BTreeMap::new());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].text, "Net #1 (GND) - Class: default");
        assert_eq!(rows[0].filter_text.as_deref(), Some("Net #1 (GND)"));
        let detail = rows[0].detail.as_deref().unwrap();
        assert!(
            detail.starts_with("Net GND: class default, 2 terminal items, via count 1, "),
            "{detail}"
        );
        assert!(!rows[0].items.is_empty());
        // Only nets with an incomplete connection.
        let counts = BTreeMap::from([(2, 1usize)]);
        let rows = super::rows(board, Unit::Mm, true, &counts);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].text.starts_with("Net #2 (SIG)"));
    }
}
