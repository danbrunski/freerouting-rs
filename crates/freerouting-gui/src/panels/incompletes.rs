//! Info > Incomplete Connections: the rows of Java's `WindowIncompletes`.
//!
//! One row per airline, sorted by net name (`Arrays.sort` on
//! `AirLine.compareTo`, a stable sort), so the airlines of a net are
//! together. The row is `AirLine.toString`: `net: from - to`, where an item
//! prints as `Item.toString` does.

use freerouting::board::BasicBoard;
use freerouting::board::items::{Item, ItemId};
use freerouting::drc::AirLine;

use super::object_list::Row;

/// Java `Item.toString` (and `Pin.toString`): the lower case class name, the
/// pin index of a pin, and `of component #n` for items of a component.
pub fn item_string(board: &BasicBoard, id: ItemId) -> String {
    let Some(item) = board.item(id) else {
        return "null".into();
    };
    item_string_of(item)
}

fn item_string_of(item: &Item) -> String {
    let mut out = item.type_name().to_string();
    if let Some(pin) = item.as_pin()
        && pin.pin_index > 0
    {
        out.push_str(&format!(" #{}", pin.pin_index));
    }
    if item.component_id() > 0 {
        out.push_str(&format!(" of component #{}", item.component_id()));
    }
    out
}

/// Java `AirLine.toString`.
pub fn row_text(board: &BasicBoard, line: &AirLine) -> String {
    let net = board
        .rules
        .nets
        .get(line.net_number)
        .map_or("null", |n| n.name.as_str());
    format!(
        "{net}: {} - {}",
        item_string(board, line.from_item),
        item_string(board, line.to_item)
    )
}

/// The airlines in list order.
pub fn sorted<'a>(board: &BasicBoard, airlines: &'a [AirLine]) -> Vec<&'a AirLine> {
    let name = |a: &AirLine| {
        board
            .rules
            .nets
            .get(a.net_number)
            .map_or_else(String::new, |n| n.name.clone())
    };
    let mut sorted: Vec<_> = airlines.iter().collect();
    sorted.sort_by_cached_key(|a| name(a));
    sorted
}

pub fn rows(board: &BasicBoard, airlines: &[AirLine]) -> Vec<Row> {
    sorted(board, airlines)
        .into_iter()
        .map(|a| Row {
            text: row_text(board, a),
            items: vec![a.from_item, a.to_item],
        })
        .collect()
}
