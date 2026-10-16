//! The canonical item dump of the O-series probes (O00 "Canonical item
//! dump"), written with B01.1's line writer and mirrored by the `dump`
//! method of each Java probe (`PullTightProbe.java` first).
//!
//! Traces as `(id, nets, layer, half width, clearance class, fixed state,
//! corners)`, vias as `(id, nets, padstack, centre, fixed state)`, highest id
//! first, then `maxGeneratedId` and the item count. [`compare_dumps`]
//! compares geometry first and ids second, so a failure says which one
//! diverged.

#![allow(dead_code)]

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemKind;
use freerouting::board::structure::FixedState;
use freerouting::datastructures::IdGenerator;
use freerouting::geometry::{Point, Polyline};

use crate::board_probe::{Line, bits};

/// A point as the probes write it: `x,y`, or `f` + the raw bits of a
/// rational point's float approximation.
pub fn point_text(p: &Point) -> String {
    match p {
        Point::Int(ip) => format!("{},{}", ip.x, ip.y),
        _ => {
            let f = p.to_float();
            format!("f{},{}", bits(f.x), bits(f.y))
        }
    }
}

/// The corners of `polyline`, space separated.
pub fn corners_text(polyline: &Polyline) -> String {
    (0..polyline.corner_count())
        .map(|i| point_text(&polyline.corner(i)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Java's `FixedState.ordinal()`.
pub fn fixed_ordinal(state: FixedState) -> i64 {
    match state {
        FixedState::Unfixed => 0,
        FixedState::ShoveFixed => 1,
        FixedState::UserFixed => 2,
        FixedState::SystemFixed => 3,
    }
}

fn nets_text(nets: &[i32]) -> String {
    nets.iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// The dump lines of `board`: its traces and vias, highest id first, then
/// `{"maxGeneratedId":…,"items":…}`.
pub fn dump(board: &RoutingBoard) -> Vec<String> {
    let mut lines = Vec::new();
    // the item list iterates highest id first, as Java's sorted copy does
    for item in board.items() {
        match &item.kind {
            ItemKind::Trace(trace) => lines.push(
                Line::new()
                    .num("id", i64::from(item.id().0))
                    .str("kind", "trace")
                    .str("nets", &nets_text(item.net_numbers()))
                    .num("layer", i64::from(trace.layer))
                    .num("halfWidth", i64::from(trace.half_width))
                    .num("class", i64::from(item.clearance_class()))
                    .num("fixed", fixed_ordinal(item.fixed_state()))
                    .str("corners", &corners_text(&trace.polyline))
                    .end(),
            ),
            ItemKind::Via(via) => {
                let padstack = board
                    .library
                    .padstacks
                    .get(via.drill.padstack)
                    .map_or_else(String::new, |p| p.name.clone());
                lines.push(
                    Line::new()
                        .num("id", i64::from(item.id().0))
                        .str("kind", "via")
                        .str("nets", &nets_text(item.net_numbers()))
                        .str("padstack", &padstack)
                        .str("center", &point_text(&Point::Int(via.drill.center)))
                        .num("fixed", fixed_ordinal(item.fixed_state()))
                        .end(),
                );
            }
            _ => {}
        }
    }
    lines.push(
        Line::new()
            .num(
                "maxGeneratedId",
                i64::from(board.communication.id_generator.max_generated_id()),
            )
            .num("items", board.items().count() as i64)
            .end(),
    );
    lines
}

/// A dump line without its `"id":…,` prefix (the geometry and data only).
fn without_id(line: &str) -> &str {
    line.strip_prefix("{\"id\":")
        .and_then(|rest| rest.find(',').map(|i| &rest[i + 1..]))
        .unwrap_or(line)
}

/// Compares two dumps: first the lines without their ids, as sorted
/// multisets (the geometry and data), then the full lines in order. `Err`
/// names the first difference and whether it is in the geometry or only in
/// the ids.
pub fn compare_dumps(java: &[&str], rust: &[String]) -> Result<(), String> {
    let mut j: Vec<&str> = java.iter().map(|l| without_id(l)).collect();
    let mut r: Vec<&str> = rust.iter().map(|l| without_id(l)).collect();
    j.sort_unstable();
    r.sort_unstable();
    if let Some(i) = (0..j.len().max(r.len())).find(|&i| j.get(i) != r.get(i)) {
        return Err(format!(
            "geometry differs (sorted without ids) at {}\n  Java: {}\n  Rust: {}",
            i + 1,
            j.get(i).copied().unwrap_or("<end>"),
            r.get(i).copied().unwrap_or("<end>")
        ));
    }
    let n = java.len().max(rust.len());
    if let Some(i) = (0..n).find(|&i| java.get(i).copied() != rust.get(i).map(String::as_str)) {
        return Err(format!(
            "ids differ at dump line {}\n  Java: {}\n  Rust: {}",
            i + 1,
            java.get(i).copied().unwrap_or("<end>"),
            rust.get(i).map(String::as_str).unwrap_or("<end>")
        ));
    }
    Ok(())
}
