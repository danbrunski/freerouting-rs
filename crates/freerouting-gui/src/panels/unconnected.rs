//! Info > Unconnected traces and vias: the rows of Java's
//! `WindowUnconnectedRoute`, one per finding of the engine's
//! `all_unconnected_items()` (split nets, dangling traces, dangling vias).
//!
//! Java's window finds groups of connected traces and vias with no pin or
//! area in them (`getConnectedSet(-1)`); the engine's DRC finding is wider,
//! so the rows can differ from Java's. The row text is Java's
//! `unconnected_route_row_message`: the net, and the traces and vias among
//! the finding's items. Rows are sorted by net name, then in finding order.

use freerouting::board::BasicBoard;
use freerouting::drc::UnconnectedItems;

use super::object_list::Row;
use crate::text;

fn net_name(board: &BasicBoard, finding: &UnconnectedItems) -> String {
    board
        .item(finding.first_item)
        .and_then(|i| i.net_numbers().first().copied())
        .and_then(|n| board.rules.nets.get(n))
        .map_or_else(String::new, |n| n.name.clone())
}

pub fn row_text(board: &BasicBoard, finding: &UnconnectedItems) -> String {
    let items = finding.all_items.iter().filter_map(|id| board.item(*id));
    let (mut traces, mut vias) = (0, 0);
    for item in items {
        if item.is_trace() {
            traces += 1;
        } else if item.is_via() {
            vias += 1;
        }
    }
    text::fill(
        text::UNCONNECTED_ROW,
        &[
            &net_name(board, finding),
            &traces.to_string(),
            &vias.to_string(),
        ],
    )
}

pub fn rows(board: &BasicBoard, findings: &[UnconnectedItems]) -> Vec<Row> {
    let mut sorted: Vec<&UnconnectedItems> = findings.iter().collect();
    sorted.sort_by_cached_key(|f| net_name(board, f));
    sorted
        .into_iter()
        .map(|f| Row {
            text: row_text(board, f),
            items: f.all_items.clone(),
        })
        .collect()
}
