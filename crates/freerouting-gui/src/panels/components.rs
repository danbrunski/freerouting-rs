//! Info > Placed Components: the rows of Java's `WindowComponents` (G04.2).
//!
//! `fillList` lists every component in `Components` sorted by name without
//! regard to case (`Component.compareTo`; `Components.count()` counts all of
//! them, placed or not), and Select takes all items whose component id is
//! the component's.

use freerouting::board::BasicBoard;

use super::object_list::Row;

/// The rows of the list: the component's name, and its items.
pub fn rows(board: &BasicBoard) -> Vec<Row> {
    let mut components: Vec<_> = board.components.iter().collect();
    // Java's `Arrays.sort` is stable and compares names ignoring case.
    components.sort_by_key(|c| c.name.to_lowercase());
    components
        .into_iter()
        .map(|c| Row {
            text: c.name.clone(),
            items: board.component_items(c.id.0),
            detail: None,
            filter_text: None,
        })
        .collect()
}
