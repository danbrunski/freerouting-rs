//! Info > Clearance Violations: the rows of Java's `WindowClearanceViolations`.
//!
//! Java sorts the violations in a `TreeSet<ViolationInfo>` by the amount the
//! clearance is too small (largest first), then by layer; two violations
//! with the same amount on the same layer compare equal there, and the
//! `TreeSet` drops all but one. This list keeps them all, in the engine's
//! order (DIVERGENCES: GUI Info lists).

use std::cmp::Ordering;

use freerouting::board::BasicBoard;
use freerouting::board::items::{AreaKind, ItemId, ItemKind};
use freerouting::drc::ClearanceViolation;
use freerouting::geometry::PolylineShapeOps;

use super::object_list::Row;
use crate::overlays::Frame;
use crate::text;

/// Java `WindowClearanceViolations.itemInfo`.
pub fn item_info(board: &BasicBoard, id: ItemId) -> String {
    let Some(item) = board.item(id) else {
        return text::UNKNOWN.to_string();
    };
    let net_name = |item: &freerouting::board::items::Item| {
        item.net_numbers()
            .first()
            .and_then(|n| board.rules.nets.get(*n))
            .map_or_else(String::new, |n| n.name.clone())
    };
    match &item.kind {
        ItemKind::Pin(_) => text::PIN.to_string(),
        ItemKind::Via(_) => text::fill(text::VIA_WITH_NET, &[&net_name(item)]),
        ItemKind::Trace(_) => text::fill(text::TRACE_WITH_NET, &[&net_name(item)]),
        ItemKind::Area(area) => match area.kind {
            AreaKind::Conduction { .. } => text::CONDUCTION_AREA.to_string(),
            AreaKind::ViaObstacle => text::VIA_KEEPOUT.to_string(),
            AreaKind::ComponentObstacle => text::COMPONENT_KEEPOUT.to_string(),
            AreaKind::Obstacle | AreaKind::TraceObstacle => text::KEEPOUT.to_string(),
        },
        ItemKind::BoardOutline(_) => text::BOARD_OUTLINE.to_string(),
        ItemKind::ComponentOutline(_) => text::UNKNOWN.to_string(),
    }
}

/// The amount the clearance is too small, in millimetres. Java divides by
/// 10000 whatever the board's resolution is (`ViolationInfo.delta`).
pub fn delta(violation: &ClearanceViolation) -> f64 {
    (violation.expected_clearance - violation.actual_clearance) / 10000.0
}

/// The text of one violation (Java `ViolationInfo.toString`).
pub fn row_text(board: &BasicBoard, frame: &Frame, violation: &ClearanceViolation) -> String {
    let [x, y] = frame.user(violation.shape.centre_of_gravity());
    let layer = usize::try_from(violation.layer)
        .ok()
        .and_then(|l| board.layer_structure.layers.get(l))
        .map_or("", |l| l.name.as_str());
    text::fill(
        text::CLEARANCE_TEMPLATE,
        &[
            &freerouting::drc::json::java_format_fixed(delta(violation), 4),
            &item_info(board, violation.first_item),
            &item_info(board, violation.second_item),
            &text::format_point(x, y),
            layer,
        ],
    )
}

/// Java's order: the largest shortfall first, then the lower layer.
pub fn order(a: &ClearanceViolation, b: &ClearanceViolation) -> Ordering {
    delta(b)
        .partial_cmp(&delta(a))
        .unwrap_or(Ordering::Equal)
        .then(a.layer.cmp(&b.layer))
}

/// The violations in list order.
pub fn sorted(violations: &[ClearanceViolation]) -> Vec<&ClearanceViolation> {
    let mut sorted: Vec<_> = violations.iter().collect();
    sorted.sort_by(|a, b| order(a, b));
    sorted
}

/// The rows of the list.
pub fn rows(board: &BasicBoard, violations: &[ClearanceViolation]) -> Vec<Row> {
    let frame = Frame::new(board);
    sorted(violations)
        .into_iter()
        .map(|v| Row {
            text: row_text(board, &frame, v),
            items: vec![v.first_item, v.second_item],
            detail: None,
            filter_text: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting::geometry::{IntBox, TileShape};

    fn violation(layer: i32, expected: f64, actual: f64) -> ClearanceViolation {
        ClearanceViolation {
            first_item: ItemId(1),
            second_item: ItemId(2),
            shape: TileShape::from(IntBox::new(10_000, 20_000, 30_000, 60_000)),
            layer,
            expected_clearance: expected,
            actual_clearance: actual,
        }
    }

    #[test]
    fn order_is_largest_shortfall_then_layer() {
        let list = [
            violation(1, 2000.0, 1000.0),
            violation(0, 3000.0, 1000.0),
            violation(0, 2000.0, 1000.0),
        ];
        let shortfalls: Vec<_> = sorted(&list).iter().map(|v| (delta(v), v.layer)).collect();
        assert_eq!(shortfalls, [(0.2, 0), (0.1, 0), (0.1, 1)]);
    }

    const DESIGN: &str = r#"(pcb t
      (resolution um 10)
      (structure
        (layer F.Cu (type signal)) (layer B.Cu (type signal))
        (boundary (rect pcb 0 0 100000 50000))
        (via v))
      (placement (component fp (place U1 1000 2000 front 0)))
      (library
        (image fp (pin sq 1 0 0))
        (padstack sq (shape (rect F.Cu -10 -10 10 10)))
        (padstack v (shape (circle F.Cu 50)) (shape (circle B.Cu 50))))
      (network (net N1 (pins U1-1)))
      (wiring (via v 5000 5000 (net N1)) (wire (path B.Cu 20 0 0 1000 0) (net N1))))"#;

    /// The row of the template in `WindowClearanceViolations_en.properties`,
    /// filled the way `ViolationInfo.toString` does: `"%.4f".formatted(delta)`
    /// with the delta in 1/10000 of the board unit, `itemInfo` for both items
    /// (`pin`, `trace [net]`), `FloatPoint.toString(Locale.ENGLISH)` of the
    /// centre of gravity in user coordinates, and the layer name.
    #[test]
    fn row_text_follows_the_java_template() {
        let pcb = freerouting::dsn::Pcb::parse(DESIGN).unwrap();
        let result = freerouting::io::specctra::build_board(&pcb);
        let board = result.board().unwrap().basic();
        let frame = Frame::new(board);
        let id_of = |pred: fn(&freerouting::board::items::Item) -> bool| {
            board.items().find(|i| pred(i)).unwrap().id()
        };
        let (pin, trace, via) = (
            id_of(|i| i.is_pin()),
            id_of(|i| i.is_trace()),
            id_of(|i| i.is_via()),
        );
        // Board units are 0.1 um here: the box centre is (20000, 40000), the
        // design unit is um with resolution 10, so (2000 , 4000).
        let mut v = violation(1, 2500.0, 1500.0);
        v.first_item = pin;
        v.second_item = trace;
        assert_eq!(
            row_text(board, &frame, &v),
            "A clearance violation of 0.1000 mm exists between pin and trace [N1] \
             at (2,000 , 4,000) on layer B.Cu."
        );
        v.first_item = via;
        v.second_item = ItemId(9999);
        assert_eq!(
            row_text(board, &frame, &v),
            "A clearance violation of 0.1000 mm exists between via [N1] and unknown \
             at (2,000 , 4,000) on layer B.Cu."
        );
    }
}
