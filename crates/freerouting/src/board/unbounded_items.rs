//! Items whose bounding box is not bounded (K15.1).
//!
//! A tile shape built from points that are not convex can come out with
//! parallel border lines and no corner between them; its box then has a
//! coordinate at `i32::MIN` or `i32::MAX`
//! ([`IntBox::has_infinite_coordinate`]). On a read board this happens to a
//! component outline whose placed corners round to reflex vertices
//! (`PlacementTransform::shape`, bug 1 of
//! `docs/development/pcu-rev2-board-report.md`: C65's F.Fab circle at
//! y = 105.242039 mm). Pads and a component's tile-shaped areas go through
//! the same placement and could get one too; K15.2 makes the placed shape
//! convex.
//!
//! [`BasicBoard::expand_bounding_box_to_include_all_items`] leaves such an
//! item out of the board box and returns [`warning`] for it. Java takes the
//! union and its `IntBox.offset(1000)` wraps, so the board box is empty,
//! every room completion fails and Java aborts in `DrillPageArray` with a
//! `NegativeArraySizeException` (DIVERGENCES, "freerouting (board model)").
//! A board without such an item gets Java's box.
//!
//! [`IntBox::has_infinite_coordinate`]: freerouting_geometry::IntBox::has_infinite_coordinate

use freerouting_geometry::FloatPoint;

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemKind};
use crate::board::structure::{Component, ComponentId};

/// The log line for `item`, whose box is not bounded and which the board box
/// leaves out: the component's reference (the DSN `place` name) and the
/// outline, or the pad, with the position in the DSN's unit.
pub(crate) fn warning(board: &BasicBoard, item: &Item) -> String {
    format!(
        "{} has an unbounded shape after placement; it is left out of the board's bounding box.",
        describe(board, item)
    )
}

fn describe(board: &BasicBoard, item: &Item) -> String {
    let component = (item.component_id() > 0)
        .then(|| board.components.get(ComponentId(item.component_id())))
        .flatten();
    let id = item.id().0;
    match &item.kind {
        ItemKind::ComponentOutline(outline) => {
            let kind = if outline.is_courtyard {
                "courtyard outline"
            } else if outline.is_fabrication {
                "fabrication outline"
            } else {
                "outline"
            };
            match component {
                Some(c) => format!(
                    "The {kind} of component {}{}",
                    c.name,
                    component_position(board, c)
                ),
                None => format!("The {kind} #{id}"),
            }
        }
        ItemKind::Pin(pin) => format!(
            "Pad {} of component {} at {}",
            board.pin_name(item).unwrap_or("?"),
            component.map_or("?", |c| c.name.as_str()),
            position(board, pin.drill.center.to_float())
        ),
        ItemKind::Area(_) => match component {
            Some(c) => format!(
                "Area #{id} of component {}{}",
                c.name,
                component_position(board, c)
            ),
            None => format!("Area #{id}"),
        },
        ItemKind::Via(via) => format!(
            "The via #{id} at {}",
            position(board, via.drill.center.to_float())
        ),
        ItemKind::Trace(_) => format!("Trace #{id}"),
        ItemKind::BoardOutline(_) => "The board outline".to_string(),
    }
}

/// ` at (x, y) unit` for a placed component, empty for an unplaced one.
fn component_position(board: &BasicBoard, component: &Component) -> String {
    component
        .exact_location()
        .map_or_else(String::new, |p| format!(" at {}", position(board, p)))
}

/// `(x, y) unit`: the board point in the DSN file's coordinates.
fn position(board: &BasicBoard, point: FloatPoint) -> String {
    let [x, y] = board
        .communication
        .coordinate_transform
        .board_to_dsn_point(point);
    format!(
        "({}, {}) {}",
        number(x),
        number(y),
        board.communication.unit.name()
    )
}

/// At most four decimals, without trailing zeros.
fn number(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_string()
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::number;

    #[test]
    fn numbers_drop_trailing_zeros() {
        assert_eq!(number(572510.06 / 10.0), "57251.006");
        assert_eq!(number(-105242.039), "-105242.039");
        assert_eq!(number(12.0), "12");
        assert_eq!(number(-0.00001), "0");
        assert_eq!(number(0.5), "0.5");
    }
}
