//! Info > Zone Islands: the copper islands of filled conduction areas that
//! the DRC finds. Java has no window for them; the rows use the text of the
//! DRC report (`DesignRulesChecker::generate_report`).

use freerouting::board::BasicBoard;
use freerouting::drc::{ZoneIslandKind, ZoneIslandViolation};

use super::object_list::Row;

pub fn row_text(board: &BasicBoard, island: &ZoneIslandViolation) -> String {
    let net = if island.net_number > 0 {
        board
            .rules
            .nets
            .get(island.net_number)
            .map_or("unknown", |n| n.name.as_str())
    } else {
        "unknown"
    };
    let area = island.conduction_area.0;
    match island.kind {
        ZoneIslandKind::IsolatedIslandDeadCopper => format!(
            "Isolated copper island without electrical connection on net [{net}] \
             (Conduction Area #{area})"
        ),
        ZoneIslandKind::IsolatedIslandUnconnected => format!(
            "Isolated copper island on net [{net}] containing {} disconnected item(s) \
             from Conduction Area #{area}",
            island.items_in_island.len()
        ),
    }
}

pub fn rows(board: &BasicBoard, islands: &[ZoneIslandViolation]) -> Vec<Row> {
    islands
        .iter()
        .map(|island| {
            let mut items = vec![island.conduction_area];
            items.extend(island.items_in_island.iter().copied());
            Row {
                text: row_text(board, island),
                items,
                detail: None,
                filter_text: None,
            }
        })
        .collect()
}
