//! Info > Board Rules (Q20.3): the breaches of the board's rules that the
//! engine's output check looks for (`freerouting::drc::check_board_rules`):
//! clearance, copper-to-edge clearance, keepouts, hole clearance, vias in
//! pads, dangling traces and vias, on the board as it is now. Not in Java.
//!
//! The list checks the board's own rules (its clearance matrix, outline
//! class and hole clearance). The board minimums of a run (track width, via
//! sizes) are the job's settings; the run's own output check holds the
//! routed copper to them, and its lines ("Output check (repair): ...") are in
//! the log. Every row counts as routed copper: the list does not know which
//! copper was loaded.

use freerouting::board::BasicBoard;
use freerouting::drc::{BoardRuleLimits, BoardRuleViolation, InputCopper, check_board_rules};

use super::object_list::Row;
use super::violations::item_info;
use crate::overlays::Frame;
use crate::text;

/// The text of one breach.
pub fn row_text(board: &BasicBoard, frame: &Frame, v: &BoardRuleViolation) -> String {
    let [x, y] = frame.user(v.location);
    let items: Vec<String> = v.items.iter().map(|id| item_info(board, *id)).collect();
    let layer = usize::try_from(v.layer)
        .ok()
        .and_then(|l| board.layer_structure.layers.get(l))
        .map(|l| format!(" on {}", l.name))
        .unwrap_or_default();
    text::fill(
        text::BOARD_RULE_TEMPLATE,
        &[
            v.rule.name(),
            &items.join(" / "),
            &text::format_point(x, y),
            &layer,
        ],
    )
}

/// The rows: every breach on `board`, in the check's order.
pub fn rows(board: &BasicBoard) -> Vec<Row> {
    let frame = Frame::new(board);
    check_board_rules(board, &BoardRuleLimits::default(), &InputCopper::none())
        .iter()
        .map(|v| Row {
            text: row_text(board, &frame, v),
            items: v.items.clone(),
            detail: None,
            filter_text: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const INPUT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../freerouting/tests/data/quality/q20-input-violations.dsn"
    );

    #[test]
    fn the_list_shows_the_boards_breaches() {
        let bytes = std::fs::read(INPUT).expect("the fixture");
        let board = freerouting::drc::load_design(&bytes).expect("loads");
        let rows = rows(board.basic());
        assert!(
            rows.iter()
                .any(|r| r.text.starts_with("clearance: ") && r.items.len() == 2),
            "{:?}",
            rows.iter().map(|r| &r.text).collect::<Vec<_>>()
        );
        assert!(rows.iter().all(|r| r.text.contains(" at ")));
    }
}
