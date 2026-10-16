//! Importing the wires and vias of a Specctra session file into a board.
//!
//! Port of `io.specctra.SesReader` and `SesImportSummary`.

use freerouting_dsn::Session;
use freerouting_dsn::ast::{LayerRef, Shape, WiringItem};
use freerouting_geometry::jmath::round_to_i32;
use freerouting_geometry::{IntPoint, Point, Polyline};

use crate::board::BasicBoard;
use crate::board::structure::FixedState;
use crate::rules::ItemClass;
use crate::util::remove_dot_numbers;

/// What a session import did.
///
/// Port of `SesImportSummary`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SesImportSummary {
    pub wires_imported: usize,
    pub vias_imported: usize,
    /// Nets, wires or vias that could not be imported.
    pub errors_encountered: usize,
}

/// Reads the session file `text` and inserts its wires (as user fixed
/// traces, normalized) and vias into `board`.
///
/// Port of `SesReader.read`. The coordinates are divided by
/// `dsn_to_board(1) / resolution` like in Java. Returns an error if the
/// text is not a session file.
pub fn read_session(board: &mut BasicBoard, text: &str) -> Result<SesImportSummary, String> {
    let session = Session::parse(text).map_err(|e| {
        format!("SesReader: not a Specctra session file — expected '(session <name>' header: {e}")
    })?;
    Ok(import_session(board, &session))
}

/// Inserts the wires and vias of `session` into `board`.
pub fn import_session(board: &mut BasicBoard, session: &Session) -> SesImportSummary {
    let mut summary = SesImportSummary::default();
    let Some(routes) = &session.routes else {
        return summary;
    };
    let communication = &board.communication;
    let denominator =
        communication.coordinate_transform.dsn_to_board(1.0) / f64::from(communication.resolution);
    for net in &routes.network_out {
        let Some(net_number) = board
            .rules
            .nets
            .get_by_name(&net.name, 1)
            .map(|n| n.net_number)
        else {
            // Java: "SesReader: net not found"
            summary.errors_encountered += 1;
            continue;
        };
        let net_numbers = [net_number];
        for item in &net.items {
            match item {
                WiringItem::Wire(wire) => {
                    // Java keeps the last `path` scope of the wire; a path
                    // with less than two points or an unknown layer counts
                    // as none, and a wire without one is skipped silently.
                    let Some(Shape::Path {
                        layer,
                        width,
                        points,
                    }) = wire
                        .shapes
                        .iter()
                        .rev()
                        .find(|s| matches!(s, Shape::Path { .. }))
                    else {
                        continue;
                    };
                    if points.len() < 2 {
                        continue;
                    }
                    let layer_no = match layer {
                        LayerRef::Pcb => -1,
                        LayerRef::Signal => -2,
                        LayerRef::Named(name) => {
                            let no = board.layer_structure.get_no(name);
                            if no < 0 {
                                continue;
                            }
                            no
                        }
                    };
                    let Some(class) = trace_clearance_class(board, ItemClass::Trace) else {
                        summary.errors_encountered += 1;
                        continue;
                    };
                    let corners: Vec<Point> = points
                        .iter()
                        .map(|p| {
                            Point::Int(IntPoint::new(
                                round_to_i32(p.x / denominator),
                                round_to_i32(p.y / denominator),
                            ))
                        })
                        .collect();
                    let half_width = round_to_i32(width / (2.0 * denominator));
                    board.insert_trace(
                        Polyline::from_points(&corners),
                        layer_no,
                        half_width,
                        &net_numbers,
                        class,
                        FixedState::UserFixed,
                    );
                    summary.wires_imported += 1;
                }
                WiringItem::Via(via) => {
                    let padstack = board
                        .library
                        .padstacks
                        .get_by_name(&via.padstack)
                        .or_else(|| {
                            board
                                .library
                                .padstacks
                                .get_by_name(&remove_dot_numbers(&via.padstack))
                        })
                        .map(|p| p.id);
                    let Some(padstack) = padstack else {
                        // Java: "via padstack not found"
                        summary.errors_encountered += 1;
                        continue;
                    };
                    let Some(class) = trace_clearance_class(board, ItemClass::Via) else {
                        summary.errors_encountered += 1;
                        continue;
                    };
                    let center = IntPoint::new(
                        round_to_i32(via.at.x / denominator),
                        round_to_i32(via.at.y / denominator),
                    );
                    board.insert_via(
                        padstack,
                        center,
                        &net_numbers,
                        class,
                        FixedState::UserFixed,
                        true,
                    );
                    summary.vias_imported += 1;
                }
            }
        }
    }
    summary
}

/// The clearance class of `item_class` in the default net class.
fn trace_clearance_class(board: &BasicBoard, item_class: ItemClass) -> Option<i32> {
    let id = board.rules.default_net_class_id()?;
    Some(
        board
            .rules
            .net_classes
            .get(id)
            .default_item_clearance_classes
            .get(item_class),
    )
}
