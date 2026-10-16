//! The shove sampler (O03.2) and the text forms the shove probes write.
//!
//! Mirrors the `ShoveSampler` block of the probes `EntrySideProbe.java` and
//! `TraceEntriesProbe.java` (`tools/java-reference/probes/board/`), whose
//! doc comment is the specification: `java.util.Random(20261015)`, a trace by
//! index in the descending-id trace list, a point on one of its segments, a
//! direction (8 for 45 degree boards, 4 for 90 degree), a length of 1 to 20
//! times the minimum trace half width; the shape is the segment's offset
//! shape by the trace's half width.

#![allow(dead_code)]

use freerouting::board::RoutingBoard;
use freerouting::board::items::{Item, ItemId};
use freerouting::board::structure::AngleRestriction;
use freerouting::geometry::jmath::{JavaRandom, round_to_i32};
use freerouting::geometry::{
    FloatPoint, IntPoint, LineSegment, Point, Polyline, PolylineShapeOps, TileShape,
};

use crate::board_probe::bits;

/// Java `ShoveSampler.SAMPLER_SEED`.
pub const SAMPLER_SEED: i64 = 20261015;

const DIRS45: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];
const DIRS90: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

/// One shove sample.
#[derive(Clone, Debug)]
pub struct Sample {
    pub trace: ItemId,
    pub segment: usize,
    pub from: IntPoint,
    pub to: IntPoint,
    /// The segment and its offset shape; `None` where Java's
    /// `offsetShapes` does not give exactly one shape.
    pub line_segment: Option<LineSegment>,
    pub shape: Option<TileShape>,
    pub layer: i32,
    pub nets: Vec<i32>,
    pub clearance_class: i32,
    pub half_width: i32,
}

/// The first `count` samples of `board` (none on a board without traces).
pub fn samples(board: &RoutingBoard, count: usize) -> Vec<Sample> {
    let mut traces = board.traces();
    traces.sort_by(|a, b| b.0.cmp(&a.0));
    if traces.is_empty() {
        return Vec::new();
    }
    let dirs: &[(i32, i32)] =
        if board.rules.trace_angle_restriction() == AngleRestriction::NinetyDegree {
            &DIRS90
        } else {
            &DIRS45
        };
    let unit = board.min_trace_half_width();
    let mut random = JavaRandom::new(SAMPLER_SEED);
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        let id = traces[random.next_int(traces.len() as i32) as usize];
        let item = board.item(id).expect("trace");
        let trace = item.as_trace().expect("trace");
        let lines = &trace.polyline;
        let segment = 1 + random.next_int(lines.lines().len() as i32 - 2) as usize;
        let a = lines.corner_approx(segment - 1);
        let b = lines.corner_approx(segment);
        let f = f64::from(random.next_int(1001));
        let x = round_to_i32(a.x + (b.x - a.x) * f / 1000.0);
        let y = round_to_i32(a.y + (b.y - a.y) * f / 1000.0);
        let dir = dirs[random.next_int(dirs.len() as i32) as usize];
        let length = (1 + random.next_int(20)) * unit;
        let from = IntPoint { x, y };
        let to = IntPoint {
            x: x + dir.0 * length,
            y: y + dir.1 * length,
        };
        let polyline = Polyline::from_corners(&Point::Int(from), &Point::Int(to));
        let shapes = polyline.offset_shapes(trace.half_width);
        let one = shapes.len() == 1;
        result.push(Sample {
            trace: id,
            segment,
            from,
            to,
            line_segment: one.then(|| LineSegment::from_polyline(&polyline, 1)),
            shape: shapes.into_iter().next().filter(|_| one),
            layer: trace.layer,
            nets: item.net_numbers().to_vec(),
            clearance_class: item.clearance_class(),
            half_width: trace.half_width,
        });
    }
    result
}

/// Java `point`: `x,y` for an integer point, else `f<x bits>,<y bits>`.
pub fn point_text(p: &Point) -> String {
    match p {
        Point::Int(ip) => format!("{},{}", ip.x, ip.y),
        other => {
            let f = other.to_float();
            format!("f{},{}", bits(f.x), bits(f.y))
        }
    }
}

/// Java `floatPoint`: `<x bits>,<y bits>`, `null` for none.
pub fn float_point_text(f: Option<FloatPoint>) -> String {
    match f {
        Some(f) => format!("{},{}", bits(f.x), bits(f.y)),
        None => "null".to_string(),
    }
}

/// Java `lines`: the two points of each line of `polyline`.
pub fn lines_text(polyline: &Polyline) -> String {
    let texts: Vec<String> = polyline
        .lines()
        .iter()
        .map(|l| {
            let (a, b) = (l.a(), l.b());
            format!("{},{} {},{}", a.x, a.y, b.x, b.y)
        })
        .collect();
    texts.join(" ")
}

/// Java `shape`: `box`, `oct` or `simplex` with the border lines; `null`.
pub fn shape_text(s: Option<&TileShape>) -> String {
    match s {
        None => "null".to_string(),
        Some(TileShape::IntBox(b)) => format!("box {} {} {} {}", b.ll.x, b.ll.y, b.ur.x, b.ur.y),
        Some(TileShape::IntOctagon(o)) => format!(
            "oct {} {} {} {} {} {} {} {}",
            o.left_x,
            o.bottom_y,
            o.right_x,
            o.top_y,
            o.upper_left_diagonal_x,
            o.lower_right_diagonal_x,
            o.lower_left_diagonal_x,
            o.upper_right_diagonal_x
        ),
        Some(other) => {
            let mut text = String::from("simplex");
            for i in 0..other.border_line_count() {
                let line = other.border_line(i);
                let (a, b) = (line.a(), line.b());
                text.push_str(&format!(" {},{} {},{}", a.x, a.y, b.x, b.y));
            }
            text
        }
    }
}

/// Java `ids`: item ids as a JSON array, in the given order.
pub fn ids_text(ids: impl IntoIterator<Item = ItemId>) -> String {
    let texts: Vec<String> = ids.into_iter().map(|id| id.0.to_string()).collect();
    format!("[{}]", texts.join(","))
}

/// Java `ints`: a JSON array.
pub fn ints_text(values: &[i32]) -> String {
    let texts: Vec<String> = values.iter().map(i32::to_string).collect();
    format!("[{}]", texts.join(","))
}

/// The item of a sample's trace.
pub fn trace_item(board: &RoutingBoard, sample: &Sample) -> Item {
    board.item(sample.trace).expect("sampled trace").clone()
}

// ----------------------------------------------------------------------
// O04.7: the start points of the shove-insert probe
// ----------------------------------------------------------------------

/// A start of probe `shove-insert` (`ShoveInsertProbe.Start`): a pin centre
/// on the pin's first layer, or a trace end on its layer.
#[derive(Clone, Copy, Debug)]
pub struct Start {
    pub point: IntPoint,
    pub layer: i32,
    pub net: i32,
}

/// Java `ShoveInsertProbe.starts`: items highest id first; per pin with
/// exactly one positive net, its centre on its first layer; per trace with
/// exactly one positive net, its first and last corner (if integer).
pub fn insert_starts(board: &RoutingBoard) -> Vec<Start> {
    let mut items: Vec<&Item> = board.items().collect();
    items.sort_by(|a, b| b.id().0.cmp(&a.id().0));
    let mut result = Vec::new();
    for item in items {
        let nets = item.net_numbers();
        if nets.len() != 1 || nets[0] <= 0 {
            continue;
        }
        let net = nets[0];
        if item.is_pin() {
            if let Some(drill) = item.drill() {
                result.push(Start {
                    point: drill.center,
                    layer: item.first_layer(),
                    net,
                });
            }
        } else if let Some(trace) = item.as_trace() {
            for corner in [trace.polyline.first_corner(), trace.polyline.last_corner()] {
                if let Point::Int(point) = corner {
                    result.push(Start {
                        point,
                        layer: trace.layer,
                        net,
                    });
                }
            }
        }
    }
    result
}

/// Java `ShoveInsertProbe.drawStart`: a net of the starts (ascending), then
/// one of its starts in list order.
pub fn draw_start(starts: &[Start], random: &mut JavaRandom) -> Start {
    let nets: std::collections::BTreeSet<i32> = starts.iter().map(|s| s.net).collect();
    let nets: Vec<i32> = nets.into_iter().collect();
    let net = nets[random.next_int(nets.len() as i32) as usize];
    let of_net: Vec<Start> = starts.iter().copied().filter(|s| s.net == net).collect();
    of_net[random.next_int(of_net.len() as i32) as usize]
}
