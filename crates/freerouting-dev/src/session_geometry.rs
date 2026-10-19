//! The wires and vias of a session, keyed so that two sessions compare
//! structurally: net by net, as multisets, in board coordinates.
//!
//! A wire is (layer, half width, point list); a via is (padstack, position).
//! Coordinates are converted the way the writer-scale reader converts them
//! (`ses_diff::import_at_writer_scale`), so a
//! number written `1000` or `1000.0`, the order of nets and wires, and the
//! direction a path is written in don't count.

use std::collections::BTreeMap;
use std::fmt;

use freerouting::board::BasicBoard;
use freerouting::dsn::Session;
use freerouting::dsn::ast::{LayerRef, Shape, WiringItem};
use freerouting::geometry::jmath::round_to_i32;

/// A wire of a session net.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WireKey {
    /// `path`, `polyline_path`, `polygon`, `rect` or `circle`.
    pub kind: &'static str,
    pub layer: String,
    /// Half the width in board units, rounded as the importer rounds it.
    pub half_width: i32,
    /// The points in board units. Paths are stored in the direction whose
    /// point list sorts first.
    pub points: Vec<(i32, i32)>,
}

/// A via of a session net.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ViaKey {
    pub padstack: String,
    pub x: i32,
    pub y: i32,
}

/// The wires and vias of one net, with their multiplicities.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetGeometry {
    pub wires: BTreeMap<WireKey, usize>,
    pub vias: BTreeMap<ViaKey, usize>,
}

/// The wires and vias of a session, by net name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionGeometry {
    pub nets: BTreeMap<String, NetGeometry>,
    /// Board units per session coordinate unit, for printing positions in
    /// the session's own numbers.
    pub denominator: f64,
}

/// Converts session coordinates to board units like the writer-scale reader
/// (`ses_reader_scale` off): board = session times `denominator`.
struct Converter {
    denominator: f64,
}

impl Converter {
    fn new(board: &BasicBoard) -> Self {
        let communication = &board.communication;
        Converter {
            denominator: communication.coordinate_transform.dsn_to_board(1.0)
                / f64::from(communication.resolution),
        }
    }

    fn coordinate(&self, value: f64) -> i32 {
        round_to_i32(value * self.denominator)
    }

    fn point(&self, x: f64, y: f64) -> (i32, i32) {
        (self.coordinate(x), self.coordinate(y))
    }

    fn half_width(&self, width: f64) -> i32 {
        round_to_i32(width * self.denominator / 2.0)
    }
}

fn layer_name(layer: &LayerRef) -> String {
    match layer {
        LayerRef::Pcb => "pcb".into(),
        LayerRef::Signal => "signal".into(),
        LayerRef::Named(name) => name.clone(),
    }
}

/// Reverses `points` if the reversed list sorts first.
fn normalize_direction(mut points: Vec<(i32, i32)>) -> Vec<(i32, i32)> {
    let reversed: Vec<(i32, i32)> = points.iter().rev().copied().collect();
    if reversed < points {
        points = reversed;
    }
    points
}

fn wire_key(converter: &Converter, shape: &Shape) -> WireKey {
    let points_of = |points: &[freerouting::dsn::ast::Point]| -> Vec<(i32, i32)> {
        points.iter().map(|p| converter.point(p.x, p.y)).collect()
    };
    let (kind, half_width, points) = match shape {
        Shape::Path { width, points, .. } => (
            "path",
            converter.half_width(*width),
            normalize_direction(points_of(points)),
        ),
        Shape::PolylinePath { width, points, .. } => (
            "polyline_path",
            converter.half_width(*width),
            normalize_direction(points_of(points)),
        ),
        Shape::Polygon {
            aperture_width,
            points,
            ..
        } => (
            "polygon",
            converter.half_width(*aperture_width),
            points_of(points),
        ),
        Shape::Rect { x1, y1, x2, y2, .. } => (
            "rect",
            0,
            vec![converter.point(*x1, *y1), converter.point(*x2, *y2)],
        ),
        Shape::Circle {
            diameter, center, ..
        } => (
            "circle",
            converter.half_width(*diameter),
            vec![converter.point(center.x, center.y)],
        ),
    };
    WireKey {
        kind,
        layer: layer_name(shape.layer()),
        half_width,
        points,
    }
}

impl SessionGeometry {
    /// The wires and vias of `session`, in the coordinates of `board`.
    ///
    /// A wire is keyed by its last `path` shape, the one the importer uses,
    /// or by its last shape if it has no path.
    pub fn new(board: &BasicBoard, session: &Session) -> Self {
        let converter = Converter::new(board);
        let mut nets: BTreeMap<String, NetGeometry> = BTreeMap::new();
        for net in session.routes.iter().flat_map(|r| &r.network_out) {
            let geometry = nets.entry(net.name.clone()).or_default();
            for item in &net.items {
                match item {
                    WiringItem::Wire(wire) => {
                        let shape = wire
                            .shapes
                            .iter()
                            .rev()
                            .find(|s| matches!(s, Shape::Path { .. }))
                            .or(wire.shapes.last());
                        if let Some(shape) = shape {
                            *geometry
                                .wires
                                .entry(wire_key(&converter, shape))
                                .or_default() += 1;
                        }
                    }
                    WiringItem::Via(via) => {
                        let (x, y) = converter.point(via.at.x, via.at.y);
                        let key = ViaKey {
                            padstack: via.padstack.clone(),
                            x,
                            y,
                        };
                        *geometry.vias.entry(key).or_default() += 1;
                    }
                }
            }
        }
        SessionGeometry {
            nets,
            denominator: converter.denominator,
        }
    }

    /// The number of wires, counting multiplicities.
    pub fn wire_count(&self) -> usize {
        self.nets.values().flat_map(|n| n.wires.values()).sum()
    }

    /// The number of vias, counting multiplicities.
    pub fn via_count(&self) -> usize {
        self.nets.values().flat_map(|n| n.vias.values()).sum()
    }
}

/// How the wires and vias of one net differ between two sessions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NetDifference {
    pub net: String,
    pub wires_only_in_a: Vec<WireKey>,
    pub wires_only_in_b: Vec<WireKey>,
    pub vias_only_in_a: Vec<ViaKey>,
    pub vias_only_in_b: Vec<ViaKey>,
}

/// The keys of `a` minus the keys of `b`, as multisets.
fn minus<K: Ord + Clone>(a: &BTreeMap<K, usize>, b: &BTreeMap<K, usize>) -> Vec<K> {
    let mut result = Vec::new();
    for (key, &count) in a {
        let other = b.get(key).copied().unwrap_or(0);
        for _ in other..count {
            result.push(key.clone());
        }
    }
    result
}

/// The nets whose wires or vias differ, in net name order.
pub fn net_differences(a: &SessionGeometry, b: &SessionGeometry) -> Vec<NetDifference> {
    let empty = NetGeometry::default();
    let mut names: Vec<&String> = a.nets.keys().chain(b.nets.keys()).collect();
    names.sort();
    names.dedup();
    names
        .into_iter()
        .filter_map(|name| {
            let na = a.nets.get(name).unwrap_or(&empty);
            let nb = b.nets.get(name).unwrap_or(&empty);
            if na == nb {
                return None;
            }
            Some(NetDifference {
                net: name.clone(),
                wires_only_in_a: minus(&na.wires, &nb.wires),
                wires_only_in_b: minus(&nb.wires, &na.wires),
                vias_only_in_a: minus(&na.vias, &nb.vias),
                vias_only_in_b: minus(&nb.vias, &na.vias),
            })
        })
        .collect()
}

/// A board coordinate printed in session units (`denominator` board units
/// per session unit), without a fraction when it is whole.
pub fn session_number(value: i32, denominator: f64) -> String {
    let v = f64::from(value) / denominator;
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Prints a wire or via in session units.
pub struct Display<'a, T> {
    pub item: &'a T,
    pub denominator: f64,
}

impl fmt::Display for Display<'_, WireKey> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = self.denominator;
        let w = self.item;
        write!(
            f,
            "{} {} width {}",
            w.kind,
            w.layer,
            session_number(w.half_width.saturating_mul(2), d)
        )?;
        for (x, y) in &w.points {
            write!(f, " ({} {})", session_number(*x, d), session_number(*y, d))?;
        }
        Ok(())
    }
}

impl fmt::Display for Display<'_, ViaKey> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let d = self.denominator;
        let v = self.item;
        write!(
            f,
            "via {} at ({} {})",
            v.padstack,
            session_number(v.x, d),
            session_number(v.y, d)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_is_normalized() {
        let forward = vec![(5, 0), (1, 1), (0, 9)];
        let backward: Vec<_> = forward.iter().rev().copied().collect();
        assert_eq!(
            normalize_direction(forward.clone()),
            normalize_direction(backward)
        );
        assert_eq!(normalize_direction(forward), vec![(0, 9), (1, 1), (5, 0)]);
    }

    #[test]
    fn multiset_difference_counts_duplicates() {
        let a = BTreeMap::from([("x", 2), ("y", 1)]);
        let b = BTreeMap::from([("x", 1), ("z", 1)]);
        assert_eq!(minus(&a, &b), vec!["x", "y"]);
        assert_eq!(minus(&b, &a), vec!["z"]);
    }

    #[test]
    fn session_numbers_drop_whole_fractions() {
        assert_eq!(session_number(1000, 1.0), "1000");
        // The writer divides board units by the denominator.
        assert_eq!(session_number(-15, 0.5), "-30");
        assert_eq!(session_number(3, 4.0), "0.75");
    }
}
