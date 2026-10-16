//! Delaunay triangulation of points in the plane.
//!
//! Port of `datastructures.PlanarDelaunayTriangulation`: the randomized
//! incremental algorithm of chapter 9.3 of *Computational Geometry,
//! Algorithms and Applications* (de Berg, van Kreveld, Overmars,
//! Schwarzkopf), with a history DAG of triangles for point location.
//!
//! Java keeps its edges, triangles and corners as objects referencing each
//! other; here they live in vectors and refer to each other by index. Edge
//! ids are handed out in the same order as Java's, so the result edges come
//! out in the same order.

use std::collections::BTreeMap;

use freerouting_geometry::jmath::JavaRandom;
use freerouting_geometry::{CRIT_INT, IntPoint, Point, Side};

/// The fixed seed Java shuffles the input corners with.
const SEED: i64 = 99;

/// An edge of the triangulation result.
///
/// Port of `PlanarDelaunayTriangulation.ResultEdge`; the objects are the
/// indices of the input objects.
#[derive(Clone, Debug, PartialEq)]
pub struct ResultEdge {
    pub start_point: Point,
    pub start_object: usize,
    pub end_point: Point,
    pub end_object: usize,
}

#[derive(Clone, Debug)]
struct Corner {
    /// The input object of the corner; `None` for the corners of the
    /// bounding triangle.
    object: Option<usize>,
    coor: Point,
}

#[derive(Clone, Debug)]
struct Edge {
    start: usize,
    end: usize,
    id: i32,
    left: Option<usize>,
    right: Option<usize>,
}

#[derive(Clone, Debug)]
struct Triangle {
    /// The edges in counter clock sense around the border.
    edges: [usize; 3],
    /// The parent used when traversing the history graph, so that each node
    /// is visited once.
    first_parent: Option<usize>,
    children: Vec<usize>,
    /// Whether the triangle is on the left of each of its edges, fixed when
    /// it is inserted into the history graph.
    on_left: Option<[bool; 3]>,
}

/// A Delaunay triangulation of the corners of a list of objects.
///
/// Port of `datastructures.PlanarDelaunayTriangulation`.
#[derive(Clone, Debug)]
pub struct PlanarDelaunayTriangulation {
    corners: Vec<Corner>,
    edges: Vec<Edge>,
    triangles: Vec<Triangle>,
    anchor: Option<usize>,
    degenerate_edges: Vec<usize>,
    last_edge_id: i32,
    warnings: Vec<String>,
}

impl PlanarDelaunayTriangulation {
    /// Triangulates the corners of `objects`; the corners of object `i` are
    /// `objects[i]`.
    ///
    /// Java throws a `NullPointerException` when a corner is outside the
    /// bounding triangle (no triangle contains it); the port leaves such a
    /// corner out and records a warning.
    pub fn new(objects: &[Vec<Point>]) -> Self {
        let mut corner_list: Vec<Corner> = Vec::new();
        for (object, corners) in objects.iter().enumerate() {
            for coor in corners {
                corner_list.push(Corner {
                    object: Some(object),
                    coor: coor.clone(),
                });
            }
        }
        JavaRandom::new(SEED).shuffle(&mut corner_list);

        let mut result = PlanarDelaunayTriangulation {
            corners: Vec::with_capacity(corner_list.len() + 3),
            edges: Vec::new(),
            triangles: Vec::new(),
            anchor: None,
            degenerate_edges: Vec::new(),
            last_edge_id: 0,
            warnings: Vec::new(),
        };
        let bounding = [
            IntPoint::new(CRIT_INT, 0),
            IntPoint::new(0, CRIT_INT),
            IntPoint::new(-CRIT_INT, -CRIT_INT),
        ]
        .map(|p| {
            result.corners.push(Corner {
                object: None,
                coor: Point::Int(p),
            });
            result.corners.len() - 1
        });
        let e0 = result.new_edge(bounding[0], bounding[1]);
        let e1 = result.new_edge(bounding[1], bounding[2]);
        let e2 = result.new_edge(bounding[2], bounding[0]);
        let start_triangle = result.new_triangle([e0, e1, e2], None);
        for e in [e0, e1, e2] {
            result.edges[e].left = Some(start_triangle);
        }
        result.graph_insert(start_triangle, None);

        for corner in corner_list {
            result.corners.push(corner);
            let corner = result.corners.len() - 1;
            match result.position_locate(corner) {
                Some(triangle) => {
                    result.split(triangle, corner);
                }
                None => result.warnings.push(format!(
                    "PlanarDelaunayTriangulation: no triangle contains corner {:?}; corner skipped",
                    result.corners[corner].coor
                )),
            }
        }
        result
    }

    /// Problems found while triangulating (Java logs them).
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// All edges of the triangulation: first the degenerate edges between
    /// equal corners of different objects, then the edges between input
    /// corners in the order of their creation.
    ///
    /// Port of `getEdgeLines`.
    pub fn edge_lines(&self) -> Vec<ResultEdge> {
        let mut result = Vec::new();
        for &e in &self.degenerate_edges {
            result.push(self.result_edge(e));
        }
        if let Some(anchor) = self.anchor {
            let mut leaf_edges: BTreeMap<i32, usize> = BTreeMap::new();
            let mut stack = vec![anchor];
            while let Some(t) = stack.pop() {
                let triangle = &self.triangles[t];
                if triangle.children.is_empty() {
                    for &e in &triangle.edges {
                        let edge = &self.edges[e];
                        if self.corners[edge.start].object.is_some()
                            && self.corners[edge.end].object.is_some()
                        {
                            leaf_edges.insert(edge.id, e);
                        }
                    }
                } else {
                    for &child in triangle.children.iter().rev() {
                        if self.triangles[child].first_parent == Some(t) {
                            stack.push(child);
                        }
                    }
                }
            }
            for e in leaf_edges.into_values() {
                result.push(self.result_edge(e));
            }
        }
        result
    }

    fn result_edge(&self, e: usize) -> ResultEdge {
        let edge = &self.edges[e];
        let (start, end) = (&self.corners[edge.start], &self.corners[edge.end]);
        ResultEdge {
            start_point: start.coor.clone(),
            start_object: start.object.expect("input corner"),
            end_point: end.coor.clone(),
            end_object: end.object.expect("input corner"),
        }
    }

    fn new_edge(&mut self, start: usize, end: usize) -> usize {
        self.last_edge_id += 1;
        self.edges.push(Edge {
            start,
            end,
            id: self.last_edge_id,
            left: None,
            right: None,
        });
        self.edges.len() - 1
    }

    fn new_triangle(&mut self, edges: [usize; 3], first_parent: Option<usize>) -> usize {
        self.triangles.push(Triangle {
            edges,
            first_parent,
            children: Vec::new(),
            on_left: None,
        });
        self.triangles.len() - 1
    }

    fn side_of(&self, corner: usize, p1: usize, p2: usize) -> Side {
        self.corners[corner]
            .coor
            .side_of(&self.corners[p1].coor, &self.corners[p2].coor)
    }

    // ------------------------------------------------------------------
    // The history graph (Java `TriangleGraph`)
    // ------------------------------------------------------------------

    fn graph_insert(&mut self, triangle: usize, parent: Option<usize>) {
        if self.triangles[triangle].on_left.is_none() {
            let edges = self.triangles[triangle].edges;
            let on_left = edges.map(|e| self.edges[e].left == Some(triangle));
            self.triangles[triangle].on_left = Some(on_left);
        }
        match parent {
            None => self.anchor = Some(triangle),
            Some(parent) => self.triangles[parent].children.push(triangle),
        }
    }

    /// The leaf triangle containing `corner` (Java `positionLocate`).
    fn position_locate(&mut self, corner: usize) -> Option<usize> {
        let anchor = self.anchor?;
        if self.triangles[anchor].children.is_empty() {
            return Some(anchor);
        }
        for i in 0..self.triangles[anchor].children.len() {
            let child = self.triangles[anchor].children[i];
            if let Some(result) = self.position_locate_from(corner, child) {
                return Some(result);
            }
        }
        self.warnings
            .push("TriangleGraph.position_locate: containing triangle not found".into());
        None
    }

    /// Java `positionLocateReku`, depth first with an explicit stack.
    fn position_locate_from(&mut self, corner: usize, start: usize) -> Option<usize> {
        if !self.triangle_contains(start, corner) {
            return None;
        }
        if self.triangles[start].children.is_empty() {
            return Some(start);
        }
        let mut stack: Vec<(usize, usize)> = vec![(start, 0)];
        while let Some((triangle, next)) = stack.last_mut() {
            let children = &self.triangles[*triangle].children;
            if *next >= children.len() {
                stack.pop();
                self.warnings.push(
                    "TriangleGraph.position_locate_reku: containing triangle not found".into(),
                );
                continue;
            }
            let child = children[*next];
            *next += 1;
            if !self.triangle_contains(child, corner) {
                continue;
            }
            if self.triangles[child].children.is_empty() {
                return Some(child);
            }
            stack.push((child, 0));
        }
        None
    }

    // ------------------------------------------------------------------
    // Triangles
    // ------------------------------------------------------------------

    /// Java `Triangle.contains`: inside or on the border.
    fn triangle_contains(&self, triangle: usize, corner: usize) -> bool {
        let t = &self.triangles[triangle];
        let Some(on_left) = t.on_left else {
            return false;
        };
        for (i, &e) in t.edges.iter().enumerate() {
            let edge = &self.edges[e];
            let side = self.side_of(corner, edge.start, edge.end);
            if on_left[i] {
                if side == Side::OnTheRight {
                    return false;
                }
            } else if side == Side::OnTheLeft {
                return false;
            }
        }
        true
    }

    /// Java `Triangle.getCorner`.
    fn triangle_corner(&self, triangle: usize, no: usize) -> usize {
        let edge = &self.edges[self.triangles[triangle].edges[no]];
        if edge.left == Some(triangle) {
            edge.start
        } else {
            edge.end
        }
    }

    /// Java `Triangle.oppositeCorner`.
    fn opposite_corner(&self, triangle: usize, edge: usize) -> usize {
        let edges = &self.triangles[triangle].edges;
        let no = edges
            .iter()
            .position(|&e| e == edge)
            .expect("edge of the triangle");
        let next = &self.edges[edges[(no + 1) % 3]];
        if next.left == Some(triangle) {
            next.end
        } else {
            next.start
        }
    }

    fn set_neighbour(&mut self, edge: usize, old: usize, new: usize) {
        let e = &mut self.edges[edge];
        if e.left == Some(old) {
            e.left = Some(new);
        } else {
            e.right = Some(new);
        }
    }

    /// Java `Triangle.splitAtInnerPoint`.
    fn split_at_inner_point(&mut self, triangle: usize, corner: usize) -> [usize; 3] {
        // Java creates three edges here that it never uses; their ids still
        // count.
        for i in 0..3 {
            let c = self.triangle_corner(triangle, i);
            self.new_edge(c, corner);
        }
        let old_edges = self.triangles[triangle].edges;
        let c0 = self.triangle_corner(triangle, 0);
        let c1 = self.triangle_corner(triangle, 1);
        let c2 = self.triangle_corner(triangle, 2);

        let e01 = self.new_edge(c1, corner);
        let e02 = self.new_edge(corner, c0);
        let t0 = self.new_triangle([old_edges[0], e01, e02], Some(triangle));
        let e11 = self.new_edge(c2, corner);
        let t1 = self.new_triangle([old_edges[1], e11, e01], Some(triangle));
        let t2 = self.new_triangle([old_edges[2], e02, e11], Some(triangle));
        let new_triangles = [t0, t1, t2];

        for &t in &new_triangles {
            let outer = self.triangles[t].edges[0];
            self.set_neighbour(outer, triangle, t);
        }
        self.edges[e01].left = Some(t0);
        self.edges[e01].right = Some(t1);
        self.edges[e11].left = Some(t1);
        self.edges[e11].right = Some(t2);
        self.edges[e02].left = Some(t0);
        self.edges[e02].right = Some(t2);
        new_triangles
    }

    /// Java `Triangle.splitAtBorderPoint`. The first two result triangles
    /// divide `triangle`, the last two `neighbour`.
    fn split_at_border_point(
        &mut self,
        triangle: usize,
        corner: usize,
        neighbour: usize,
    ) -> Option<[usize; 4]> {
        let mut this_no = None;
        let mut neighbour_no = None;
        for i in 0..3 {
            let e = self.triangles[triangle].edges[i];
            if self.side_of(corner, self.edges[e].start, self.edges[e].end) == Side::Collinear {
                this_no = Some(i);
            }
            let e = self.triangles[neighbour].edges[i];
            if self.side_of(corner, self.edges[e].start, self.edges[e].end) == Side::Collinear {
                neighbour_no = Some(i);
            }
        }
        let (Some(this_no), Some(neighbour_no)) = (this_no, neighbour_no) else {
            self.warnings
                .push("Triangle.split_at_border_point: touching edge not found".into());
            return None;
        };
        let touching = self.triangles[triangle].edges[this_no];
        if touching != self.triangles[neighbour].edges[neighbour_no] {
            self.warnings
                .push("Triangle.split_at_border_point: edges inconsistent".into());
            return None;
        }
        let (t_start, t_end) = (self.edges[touching].start, self.edges[touching].end);
        let (first_common, second_common) = if self.edges[touching].left == Some(triangle) {
            (self.new_edge(t_start, corner), self.new_edge(corner, t_end))
        } else {
            (self.new_edge(t_end, corner), self.new_edge(corner, t_start))
        };

        // The first split triangle of this triangle.
        let prev_edge = self.triangles[triangle].edges[(this_no + 2) % 3];
        let this_splitting = if self.edges[prev_edge].left == Some(triangle) {
            let c = self.edges[prev_edge].start;
            self.new_edge(corner, c)
        } else {
            let c = self.edges[prev_edge].end;
            self.new_edge(corner, c)
        };
        let t0 = self.new_triangle([prev_edge, first_common, this_splitting], Some(triangle));
        self.set_neighbour(prev_edge, triangle, t0);
        self.edges[first_common].left = Some(t0);
        self.edges[this_splitting].left = Some(t0);

        // The second split triangle of this triangle.
        let next_edge = self.triangles[triangle].edges[(this_no + 1) % 3];
        let t1 = self.new_triangle([this_splitting, second_common, next_edge], Some(triangle));
        self.edges[this_splitting].right = Some(t1);
        self.edges[second_common].left = Some(t1);
        self.set_neighbour(next_edge, triangle, t1);

        // The first split triangle of the neighbour.
        let neighbour_next = self.triangles[neighbour].edges[(neighbour_no + 1) % 3];
        let neighbour_splitting = if self.edges[neighbour_next].left == Some(neighbour) {
            let c = self.edges[neighbour_next].end;
            self.new_edge(c, corner)
        } else {
            let c = self.edges[neighbour_next].start;
            self.new_edge(c, corner)
        };
        let t2 = self.new_triangle(
            [neighbour_splitting, first_common, neighbour_next],
            Some(neighbour),
        );
        self.edges[neighbour_splitting].left = Some(t2);
        self.edges[first_common].right = Some(t2);
        self.set_neighbour(neighbour_next, neighbour, t2);

        // The second split triangle of the neighbour.
        let neighbour_prev = self.triangles[neighbour].edges[(neighbour_no + 2) % 3];
        let t3 = self.new_triangle(
            [neighbour_prev, second_common, neighbour_splitting],
            Some(neighbour),
        );
        self.set_neighbour(neighbour_prev, neighbour, t3);
        self.edges[second_common].right = Some(t3);
        self.edges[neighbour_splitting].right = Some(t3);

        Some([t0, t1, t2, t3])
    }

    // ------------------------------------------------------------------
    // Edges
    // ------------------------------------------------------------------

    /// Java `Edge.commonCorner`.
    fn common_corner(&self, e1: usize, e2: usize) -> Option<usize> {
        let (this, other) = (&self.edges[e1], &self.edges[e2]);
        if other.start == this.start || other.end == this.start {
            Some(this.start)
        } else if other.start == this.end || other.end == this.end {
            Some(this.end)
        } else {
            None
        }
    }

    /// Java `Edge.otherNeighbour`.
    fn other_neighbour(&self, edge: usize, triangle: usize) -> Option<usize> {
        let e = &self.edges[edge];
        if e.left == Some(triangle) {
            e.right
        } else if e.right == Some(triangle) {
            e.left
        } else {
            None
        }
    }

    /// Java `Edge.isLegal`.
    fn is_legal(&self, edge: usize) -> bool {
        let e = &self.edges[edge];
        let (Some(left), Some(right)) = (e.left, e.right) else {
            return true;
        };
        let left_opposite = self.opposite_corner(left, edge);
        let right_opposite = self.opposite_corner(right, edge);
        let inside = self.corners[right_opposite].coor.to_float().inside_circle(
            self.corners[e.start].coor.to_float(),
            self.corners[left_opposite].coor.to_float(),
            self.corners[e.end].coor.to_float(),
        );
        !inside
    }

    /// Java `Edge.flip`: replaces `edge` by the edge between the opposite
    /// corners of its two triangles and returns the new edge.
    fn flip(&mut self, edge: usize) -> usize {
        let (left, right) = (
            self.edges[edge].left.expect("left"),
            self.edges[edge].right.expect("right"),
        );
        let start = self.opposite_corner(right, edge);
        let end = self.opposite_corner(left, edge);
        let flipped = self.new_edge(start, end);
        let first_parent = left;
        let left_index = self.triangles[left]
            .edges
            .iter()
            .position(|&e| e == edge)
            .expect("edge");
        let right_index = self.triangles[right]
            .edges
            .iter()
            .position(|&e| e == edge)
            .expect("edge");
        let left_prev = self.triangles[left].edges[(left_index + 2) % 3];
        let left_next = self.triangles[left].edges[(left_index + 1) % 3];
        let right_prev = self.triangles[right].edges[(right_index + 2) % 3];
        let right_next = self.triangles[right].edges[(right_index + 1) % 3];

        let new_left = self.new_triangle([flipped, left_prev, right_next], Some(first_parent));
        self.edges[flipped].left = Some(new_left);
        self.set_neighbour(left_prev, left, new_left);
        self.set_neighbour(right_next, right, new_left);

        let new_right = self.new_triangle([flipped, right_prev, left_next], Some(first_parent));
        self.edges[flipped].right = Some(new_right);
        self.set_neighbour(right_prev, right, new_right);
        self.set_neighbour(left_next, left, new_right);
        flipped
    }

    // ------------------------------------------------------------------
    // Insertion
    // ------------------------------------------------------------------

    /// Java `split`: inserts `corner` into `triangle`.
    fn split(&mut self, triangle: usize, corner: usize) -> bool {
        let mut containing: Option<usize> = None;
        for i in 0..3 {
            let e = self.triangles[triangle].edges[i];
            let (start, end) = (self.edges[e].start, self.edges[e].end);
            let side = if self.edges[e].left == Some(triangle) {
                self.side_of(corner, start, end)
            } else {
                self.side_of(corner, end, start)
            };
            if side == Side::OnTheRight {
                self.warnings
                    .push("PlanarDelaunayTriangulation.split: corner is outside".into());
                return false;
            } else if side == Side::Collinear {
                if let Some(containing) = containing {
                    // The corner equals a corner of the triangle.
                    let Some(common) = self.common_corner(e, containing) else {
                        self.warnings.push(
                            "PlanarDelaunayTriangulation.split: common corner expected".into(),
                        );
                        return false;
                    };
                    if self.corners[corner].object == self.corners[common].object {
                        return false;
                    }
                    let degenerate = self.new_edge(corner, common);
                    self.degenerate_edges.push(degenerate);
                    return true;
                }
                containing = Some(e);
            }
        }

        match containing {
            None => {
                let new_triangles = self.split_at_inner_point(triangle, corner);
                for t in new_triangles {
                    self.graph_insert(t, Some(triangle));
                }
                for i in 0..3 {
                    let e = self.triangles[triangle].edges[i];
                    self.legalize_edge(corner, e);
                }
            }
            Some(containing) => {
                let Some(neighbour) = self.other_neighbour(containing, triangle) else {
                    self.warnings
                        .push("Edge.other_neighbour: inconsistent neighbour triangle".into());
                    return false;
                };
                let Some(new_triangles) = self.split_at_border_point(triangle, corner, neighbour)
                else {
                    return false;
                };
                self.graph_insert(new_triangles[0], Some(triangle));
                self.graph_insert(new_triangles[1], Some(triangle));
                self.graph_insert(new_triangles[2], Some(neighbour));
                self.graph_insert(new_triangles[3], Some(neighbour));
                for i in 0..3 {
                    let e = self.triangles[triangle].edges[i];
                    if e != containing {
                        self.legalize_edge(corner, e);
                    }
                }
                for i in 0..3 {
                    let e = self.triangles[neighbour].edges[i];
                    if e != containing {
                        self.legalize_edge(corner, e);
                    }
                }
            }
        }
        true
    }

    /// Java `legalizeEdge`: flips `edge` if it is not a legal Delaunay edge
    /// and continues with the edges of the changed triangle.
    fn legalize_edge(&mut self, corner: usize, edge: usize) -> bool {
        if self.is_legal(edge) {
            return false;
        }
        let (left, right) = (
            self.edges[edge].left.expect("left"),
            self.edges[edge].right.expect("right"),
        );
        let triangle_to_change = if self.opposite_corner(left, edge) == corner {
            right
        } else if self.opposite_corner(right, edge) == corner {
            left
        } else {
            self.warnings
                .push("PlanarDelaunayTriangulation.legalize_edge: edge lines inconsistent".into());
            return false;
        };
        let flipped = self.flip(edge);
        let (new_left, new_right) = (
            self.edges[flipped].left.expect("left"),
            self.edges[flipped].right.expect("right"),
        );
        self.graph_insert(new_left, Some(left));
        self.graph_insert(new_right, Some(left));
        self.graph_insert(new_left, Some(right));
        self.graph_insert(new_right, Some(right));
        for i in 0..3 {
            let e = self.triangles[triangle_to_change].edges[i];
            if e != edge {
                self.legalize_edge(corner, e);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i32, y: i32) -> Point {
        Point::Int(IntPoint::new(x, y))
    }

    /// The edges as `start-end` object pairs in result order.
    fn edges(objects: &[Vec<Point>]) -> String {
        PlanarDelaunayTriangulation::new(objects)
            .edge_lines()
            .iter()
            .map(|e| format!("{}-{}", e.start_object, e.end_object))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn single(points: &[(i32, i32)]) -> Vec<Vec<Point>> {
        points.iter().map(|&(x, y)| vec![p(x, y)]).collect()
    }

    // The expected edge lists come from Java's PlanarDelaunayTriangulation
    // (cda540f) on the same input.

    #[test]
    fn small_inputs_match_java() {
        assert_eq!(
            edges(&single(&[
                (0, 0),
                (1000, 0),
                (1000, 1000),
                (0, 1000),
                (500, 400)
            ])),
            "4-3 4-0 3-0 1-0 4-1 1-3 2-3"
        );
        assert_eq!(
            edges(&single(&[(0, 0), (1000, 0), (0, 1000)])),
            "2-0 1-0 2-1"
        );
        assert_eq!(
            edges(&[vec![p(0, 0)], vec![p(0, 0)], vec![p(1000, 0), p(1000, 0)]]),
            "0-1 1-2"
        );
        let line: Vec<(i32, i32)> = (0..6).map(|i| (i * 100, 0)).collect();
        assert_eq!(edges(&single(&line)), "4-5 2-3 3-4 2-1 1-0");
    }

    #[test]
    fn random_points_match_java() {
        let mut random = JavaRandom::new(5);
        let points: Vec<(i32, i32)> = (0..40)
            .map(|_| {
                let x = random.next_int(200_000) - 100_000;
                let y = random.next_int(200_000) - 100_000;
                (x, y)
            })
            .collect();
        assert_eq!(
            edges(&single(&points)),
            "26-21 5-33 33-39 21-7 7-16 21-16 15-10 7-31 16-8 8-7 9-3 39-30 30-5 33-30 8-19 \
             16-19 31-19 0-10 15-0 3-0 29-5 33-29 7-37 37-31 8-37 37-19 36-9 36-29 1-38 39-4 \
             36-4 33-4 29-4 15-34 16-34 34-38 0-13 13-38 1-13 13-15 13-34 13-3 20-4 9-20 20-3 \
             20-39 5-32 32-23 30-32 32-39 10-25 25-20 25-3 25-0 26-18 21-18 39-18 31-28 28-23 \
             28-2 2-5 32-2 2-23 2-24 3-24 5-24 24-29 1-11 11-24 3-11 11-2 13-11 19-6 6-2 1-6 \
             6-31 28-6 11-6 28-22 22-26 21-22 22-7 22-31 18-22 19-12 12-34 16-12 12-38 24-35 \
             35-29 9-35 3-35 36-35 22-17 17-39 23-17 17-18 17-28 17-32 6-14 14-38 19-14 1-14 \
             14-12 20-27 27-36 4-27 9-27"
        );
    }

    #[test]
    fn cocircular_grid_matches_java() {
        let grid: Vec<(i32, i32)> = (0..5)
            .flat_map(|i| (0..4).map(move |j| (i * 2540, j * 2540)))
            .collect();
        assert_eq!(
            edges(&single(&grid)),
            "5-4 5-10 4-10 16-17 17-10 4-9 9-10 16-9 10-14 14-18 5-3 4-3 3-10 3-14 14-11 \
             18-11 0-4 8-4 9-8 18-19 16-12 12-8 9-12 3-2 2-4 10-13 13-16 13-17 9-13 3-6 6-10 \
             5-6 18-15 15-11 2-1 1-0 1-4 11-7 7-3 7-14"
        );
    }

    #[test]
    fn objects_with_several_corners_match_java() {
        let objects = vec![
            vec![p(0, 0), p(5000, 0), p(5000, 5000), p(0, 5000)],
            vec![p(2500, 2500)],
            vec![p(9000, 100)],
            vec![p(-3000, 7000)],
        ];
        assert_eq!(
            edges(&objects),
            "0-0 0-0 0-2 0-2 0-3 0-3 2-0 0-3 0-0 0-1 1-0 1-0 2-1"
        );
    }

    #[test]
    fn corner_outside_the_bounding_triangle_is_skipped() {
        let t = PlanarDelaunayTriangulation::new(&[
            vec![p(0, 0)],
            vec![p(1000, 0)],
            vec![p(CRIT_INT, CRIT_INT)],
        ]);
        let lines = t.edge_lines();
        assert_eq!(lines.len(), 1);
        assert!(!t.warnings().is_empty());
    }
}
