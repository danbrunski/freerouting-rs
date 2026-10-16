//! What the board view draws: the items of a board, flattened to drawable
//! shapes in millimetres.
//!
//! The scene is a snapshot of the board model (`freerouting::board`) taken
//! when a file is loaded. Coordinates are those of the design file, converted
//! to millimetres, so the cursor position reads the same as in the CAD tool
//! that wrote the file.

use std::collections::HashMap;

use freerouting::board::RoutingBoard;
use freerouting::board::items::{AreaKind, ItemKind};
use freerouting::board::structure::ComponentId;
use freerouting::dsn::ast::Side;
use freerouting::geometry::{Area, FloatPoint, PolylineShapeOps, Shape};
/// A point in millimetres, Y up (board coordinates, as in the file).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct P {
    pub x: f64,
    pub y: f64,
}

impl P {
    pub const fn new(x: f64, y: f64) -> Self {
        P { x, y }
    }
}

/// An axis-aligned box in millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: P,
    pub max: P,
}

impl Bounds {
    pub const EMPTY: Bounds = Bounds {
        min: P::new(f64::INFINITY, f64::INFINITY),
        max: P::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
    };

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y
    }

    pub fn add(&mut self, p: P) {
        self.min.x = self.min.x.min(p.x);
        self.min.y = self.min.y.min(p.y);
        self.max.x = self.max.x.max(p.x);
        self.max.y = self.max.y.max(p.y);
    }

    pub fn grow(&self, d: f64) -> Bounds {
        Bounds {
            min: P::new(self.min.x - d, self.min.y - d),
            max: P::new(self.max.x + d, self.max.y + d),
        }
    }

    pub fn union(&mut self, other: &Bounds) {
        if !other.is_empty() {
            self.add(other.min);
            self.add(other.max);
        }
    }

    pub fn contains(&self, p: P) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn center(&self) -> P {
        P::new(
            (self.min.x + self.max.x) / 2.0,
            (self.min.y + self.max.y) / 2.0,
        )
    }

    pub fn width(&self) -> f64 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f64 {
        self.max.y - self.min.y
    }
}

/// The shape of an item.
#[derive(Clone, Debug, PartialEq)]
pub enum Geom {
    /// A filled polygon, with holes.
    Polygon {
        outer: Vec<P>,
        holes: Vec<Vec<P>>,
    },
    /// A line of the given width with round ends and corners. Width 0 is a
    /// hairline.
    Path {
        points: Vec<P>,
        width: f64,
    },
    Circle {
        center: P,
        radius: f64,
    },
}

impl Geom {
    pub fn bounds(&self) -> Bounds {
        let mut b = Bounds::EMPTY;
        match self {
            Geom::Polygon { outer, .. } => outer.iter().for_each(|&p| b.add(p)),
            Geom::Path { points, width } => {
                points.iter().for_each(|&p| b.add(p));
                b = b.grow(width / 2.0);
            }
            Geom::Circle { center, radius } => {
                b.add(*center);
                b = b.grow(*radius);
            }
        }
        b
    }

    /// Whether `p` is on the shape, with `slack` millimetres of tolerance.
    pub fn hit(&self, p: P, slack: f64) -> bool {
        match self {
            Geom::Circle { center, radius } => dist(p, *center) <= radius + slack,
            Geom::Path { points, width } => {
                let r = width / 2.0 + slack;
                match points.as_slice() {
                    [] => false,
                    [only] => dist(p, *only) <= r,
                    _ => points.windows(2).any(|s| seg_dist(p, s[0], s[1]) <= r),
                }
            }
            Geom::Polygon { outer, holes } => {
                (in_ring(p, outer) && !holes.iter().any(|h| in_ring(p, h)))
                    || (slack > 0.0 && ring_dist(p, outer) <= slack)
            }
        }
    }
}

fn dist(a: P, b: P) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

fn seg_dist(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return dist(p, a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    dist(p, P::new(a.x + t * dx, a.y + t * dy))
}

fn ring_dist(p: P, ring: &[P]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|i| seg_dist(p, ring[i], ring[(i + 1) % n]))
        .fold(f64::INFINITY, f64::min)
}

fn in_ring(p: P, ring: &[P]) -> bool {
    let mut inside = false;
    let n = ring.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (ring[i], ring[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// What an item is, which decides its color and whether it is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// A conduction area: a plane or a copper fill.
    Plane,
    Keepout,
    ViaKeepout,
    /// A keepout for component placement.
    PlaceKeepout,
    Trace,
    Pad,
    Via,
    /// The board outline.
    Outline,
    /// A footprint's outline.
    ComponentOutline,
}

impl Kind {
    pub const ALL: [Kind; 9] = [
        Kind::Plane,
        Kind::Keepout,
        Kind::ViaKeepout,
        Kind::PlaceKeepout,
        Kind::Trace,
        Kind::Pad,
        Kind::Via,
        Kind::Outline,
        Kind::ComponentOutline,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Kind::Plane => "Planes and fills",
            Kind::Keepout => "Keepouts",
            Kind::ViaKeepout => "Via keepouts",
            Kind::PlaceKeepout => "Placement keepouts",
            Kind::Trace => "Traces",
            Kind::Pad => "Pads",
            Kind::Via => "Vias",
            Kind::Outline => "Board outline",
            Kind::ComponentOutline => "Footprint outlines",
        }
    }
}

/// Where an item is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    /// A layer of the layer stack, by index.
    Layer(usize),
    /// Not on a copper layer: the board outline.
    Board,
    /// A footprint outline on the front or back.
    Silk(Side),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub kind: Kind,
    pub slot: Slot,
    pub geom: Geom,
    /// Index into [`Scene::nets`].
    pub net: Option<usize>,
    /// What the item belongs to, such as `U1-3` for a pad or `U1` for a
    /// footprint outline.
    pub owner: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    /// False for a power plane layer.
    pub signal: bool,
}

#[derive(Debug, Default)]
pub struct Scene {
    pub name: String,
    pub layers: Vec<Layer>,
    pub items: Vec<Item>,
    pub nets: Vec<String>,
    pub components: usize,
    /// Reference designators, one per placed component.
    pub labels: Vec<Label>,
    pub bounds: Bounds,
}

/// A component's reference designator. DSN files carry no silkscreen text,
/// so the label is placed and sized from the footprint: centred on its pads
/// and outline, along its longer side.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub text: String,
    pub center: P,
    /// Text height in millimetres.
    pub height: f64,
    /// Reads bottom to top instead of left to right.
    pub vertical: bool,
    pub side: Side,
}

impl Label {
    fn fit(text: &str, footprint: &Bounds, side: Side) -> Label {
        let (w, h) = (footprint.width(), footprint.height());
        let vertical = h > w * 1.25;
        let (long, short) = if vertical { (h, w) } else { (w, h) };
        let chars = text.chars().count().max(1) as f64;
        let height = (short * 0.4).min(long / (chars * 0.8)).clamp(0.2, 2.0);
        Label {
            text: text.to_string(),
            center: footprint.center(),
            height,
            vertical,
            side,
        }
    }
}

impl Default for Bounds {
    fn default() -> Self {
        Bounds::EMPTY
    }
}

// ---------------------------------------------------------------------------
// Board to scene
// ---------------------------------------------------------------------------

/// A circle as a polygon, for areas with holes.
fn circle_ring(center: P, radius: f64) -> Vec<P> {
    (0..48)
        .map(|i| {
            let a = f64::from(i) * std::f64::consts::TAU / 48.0;
            P::new(center.x + radius * a.cos(), center.y + radius * a.sin())
        })
        .collect()
}

impl Scene {
    /// Takes the drawable items of a board.
    pub fn from_board(name: &str, board: &RoutingBoard) -> Scene {
        let b = board.basic();
        let comm = &b.communication;
        let mm = comm.unit.micrometers() / 1000.0;
        let ct = comm.coordinate_transform;
        let pt = |p: FloatPoint| {
            let [x, y] = ct.board_to_dsn_point(p);
            P::new(x * mm, y * mm)
        };
        let len = |v: f64| ct.board_to_dsn(v) * mm;
        let shape_geom = |s: &Shape| -> Option<Geom> {
            match s {
                Shape::Circle(c) => (c.radius > 0).then(|| Geom::Circle {
                    center: pt(c.center.to_float()),
                    radius: len(f64::from(c.radius)),
                }),
                Shape::Polyline(_) => {
                    let outer: Vec<P> = s.corner_approx_arr().into_iter().map(pt).collect();
                    (outer.len() >= 3).then_some(Geom::Polygon {
                        outer,
                        holes: Vec::new(),
                    })
                }
            }
        };
        let ring = |s: &Shape| -> Option<Vec<P>> {
            match shape_geom(s)? {
                Geom::Circle { center, radius } => Some(circle_ring(center, radius)),
                Geom::Polygon { outer, .. } => Some(outer),
                Geom::Path { .. } => None,
            }
        };
        let area_geom = |a: &Area| -> Option<Geom> {
            let holes: Vec<Vec<P>> = a.get_holes().iter().filter_map(ring).collect();
            if holes.is_empty() {
                shape_geom(&a.get_border())
            } else {
                Some(Geom::Polygon {
                    outer: ring(&a.get_border())?,
                    holes,
                })
            }
        };
        let closed_line = |mut points: Vec<P>| {
            if let Some(&first) = points.first() {
                points.push(first);
            }
            Geom::Path { points, width: 0.0 }
        };

        let mut scene = Scene {
            name: name.to_string(),
            layers: b
                .layer_structure
                .layers
                .iter()
                .map(|l| Layer {
                    name: l.name.clone(),
                    signal: l.is_signal,
                })
                .collect(),
            components: b.components.count(),
            ..Scene::default()
        };
        let layer_count = scene.layers.len();
        let slot = |layer: i32| {
            usize::try_from(layer)
                .ok()
                .filter(|&l| l < layer_count)
                .map(Slot::Layer)
        };
        let mut net_index: HashMap<i32, usize> = HashMap::new();
        for net in b.rules.nets.iter() {
            net_index.insert(net.net_number, scene.nets.len());
            scene.nets.push(net.name.clone());
        }
        let component_name = |id: i32| b.components.get(ComponentId(id)).map(|c| c.name.clone());

        let mut footprints: HashMap<i32, Bounds> = HashMap::new();
        for item in b.items() {
            let first_new = scene.items.len();
            let net = item
                .net_numbers()
                .first()
                .and_then(|n| net_index.get(n).copied());
            let mut push = |kind: Kind, slot: Slot, geom: Geom, owner: Option<String>| {
                scene.items.push(Item {
                    kind,
                    slot,
                    geom,
                    net,
                    owner,
                });
            };
            match &item.kind {
                ItemKind::Pin(pin) => {
                    let owner = match (component_name(item.component_id()), b.pin_name(item)) {
                        (Some(c), Some(p)) => Some(format!("{c}-{p}")),
                        (c, _) => c,
                    };
                    for (i, shape) in pin.drill.shapes.iter().enumerate() {
                        let (Some(shape), Some(slot)) =
                            (shape, slot(pin.drill.first_layer + i as i32))
                        else {
                            continue;
                        };
                        if let Some(g) = shape_geom(shape) {
                            push(Kind::Pad, slot, g, owner.clone());
                        }
                    }
                }
                ItemKind::Via(via) => {
                    for (i, shape) in via.drill.shapes.iter().enumerate() {
                        let (Some(shape), Some(slot)) =
                            (shape, slot(via.drill.first_layer + i as i32))
                        else {
                            continue;
                        };
                        if let Some(g) = shape_geom(shape) {
                            push(Kind::Via, slot, g, None);
                        }
                    }
                }
                ItemKind::Trace(trace) => {
                    if let Some(slot) = slot(trace.layer) {
                        let points = trace
                            .polyline
                            .corner_approx_arr()
                            .into_iter()
                            .map(pt)
                            .collect();
                        let width = len(2.0 * f64::from(trace.half_width));
                        push(Kind::Trace, slot, Geom::Path { points, width }, None);
                    }
                }
                ItemKind::Area(area) => {
                    let kind = match area.kind {
                        AreaKind::Obstacle => Kind::Keepout,
                        AreaKind::ViaObstacle => Kind::ViaKeepout,
                        AreaKind::ComponentObstacle => Kind::PlaceKeepout,
                        AreaKind::Conduction { .. } => Kind::Plane,
                    };
                    let owner = area
                        .name
                        .clone()
                        .filter(|n| !n.is_empty())
                        .or_else(|| component_name(item.component_id()));
                    if let (Some(slot), Some(g)) = (slot(area.layer()), area_geom(area.area())) {
                        push(kind, slot, g, owner);
                    }
                }
                ItemKind::ComponentOutline(outline) => {
                    let side = if outline.is_front() {
                        Side::Front
                    } else {
                        Side::Back
                    };
                    let points: Vec<P> = outline
                        .area()
                        .get_border()
                        .corner_approx_arr()
                        .into_iter()
                        .map(pt)
                        .collect();
                    let g = match outline.area().get_border() {
                        Shape::Circle(c) => Geom::Path {
                            points: {
                                let mut r =
                                    circle_ring(pt(c.center.to_float()), len(f64::from(c.radius)));
                                r.push(r[0]);
                                r
                            },
                            width: 0.0,
                        },
                        _ if outline.is_closed => closed_line(points),
                        _ => Geom::Path { points, width: 0.0 },
                    };
                    push(
                        Kind::ComponentOutline,
                        Slot::Silk(side),
                        g,
                        component_name(item.component_id()),
                    );
                }
                ItemKind::BoardOutline(outline) => {
                    for shape in outline.shapes() {
                        let points = shape.corner_approx_arr().into_iter().map(pt).collect();
                        push(Kind::Outline, Slot::Board, closed_line(points), None);
                    }
                }
            }
            if item.component_id() > 0
                && matches!(item.kind, ItemKind::Pin(_) | ItemKind::ComponentOutline(_))
            {
                let f = footprints.entry(item.component_id()).or_default();
                for new in &scene.items[first_new..] {
                    f.union(&new.geom.bounds());
                }
            }
        }

        for component in b.components.iter().filter(|c| c.is_placed()) {
            let side = if component.placed_on_front() {
                Side::Front
            } else {
                Side::Back
            };
            let footprint = match footprints.get(&component.id.0) {
                Some(f) if !f.is_empty() => *f,
                _ => {
                    let Some(at) = component.exact_location() else {
                        continue;
                    };
                    let mut f = Bounds::EMPTY;
                    f.add(pt(at));
                    f.grow(0.5)
                }
            };
            scene
                .labels
                .push(Label::fit(&component.name, &footprint, side));
        }

        let mut bounds = Bounds::EMPTY;
        let mut outline = Bounds::EMPTY;
        for item in &scene.items {
            let ib = item.geom.bounds();
            bounds.union(&ib);
            if item.kind == Kind::Outline {
                outline.union(&ib);
            }
        }
        scene.bounds = if outline.is_empty() { bounds } else { outline };
        scene
    }

    /// The topmost item at `p` on the shown layers and kinds, preferring the
    /// active layer.
    pub fn pick(
        &self,
        p: P,
        slack: f64,
        shown: impl Fn(&Item) -> bool,
        active: Option<usize>,
    ) -> Option<&Item> {
        let rank = |item: &Item| {
            let on_active = matches!(item.slot, Slot::Layer(l) if Some(l) == active);
            // Small things on top of big ones.
            let kind = match item.kind {
                Kind::Via => 0,
                Kind::Pad => 1,
                Kind::Trace => 2,
                Kind::ComponentOutline => 3,
                Kind::Plane => 4,
                Kind::Keepout | Kind::ViaKeepout | Kind::PlaceKeepout => 5,
                Kind::Outline => 6,
            };
            (!on_active, kind)
        };
        self.items
            .iter()
            .filter(|i| shown(i) && i.geom.bounds().grow(slack).contains(p) && i.geom.hit(p, slack))
            .min_by_key(|i| rank(i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting::dsn::Pcb;
    use freerouting::io::specctra::build_board;

    fn close(a: P, b: P) -> bool {
        (a.x - b.x).abs() < 1e-6 && (a.y - b.y).abs() < 1e-6
    }

    fn scene(pcb: &Pcb) -> Scene {
        let result = build_board(pcb);
        Scene::from_board(&pcb.name, result.board().expect("board"))
    }

    fn centers(scene: &Scene, kind: Kind, owner: Option<&str>) -> Vec<(Slot, P)> {
        scene
            .items
            .iter()
            .filter(|i| i.kind == kind && i.owner.as_deref() == owner)
            .map(|i| (i.slot, i.geom.bounds().center()))
            .collect()
    }

    const DESIGN: &str = r#"(pcb t
      (resolution um 10)
      (structure
        (layer F.Cu (type signal)) (layer In1 (type power)) (layer B.Cu (type signal))
        (boundary (rect pcb 0 0 10000 5000))
        (via v))
      (placement
        (component fp (place U1 1000 2000 front 90) (place U2 3000 2000 back 90)))
      (library
        (image fp (pin sq 1 100 0))
        (padstack sq (shape (rect F.Cu -10 -10 10 10)))
        (padstack v (shape (circle F.Cu 50)) (shape (circle B.Cu 50))))
      (network (net N1 (pins U1-1 U2-1)))
      (wiring (via v 500 500 (net N1)) (wire (path B.Cu 20 0 0 1000 0) (net N1))))"#;

    #[test]
    fn takes_pads_vias_and_traces_from_the_board() {
        let s = scene(&Pcb::parse(DESIGN).unwrap());
        assert_eq!(s.layers.len(), 3);
        assert_eq!(s.components, 2);

        // Front, 90 degrees: pin (100, 0) goes to (0, 100) from the location.
        let u1 = centers(&s, Kind::Pad, Some("U1-1"));
        assert_eq!(u1.len(), 1);
        assert_eq!(u1[0].0, Slot::Layer(0));
        assert!(close(u1[0].1, P::new(1.0, 2.1)), "{:?}", u1[0].1);
        // On the back the pad moves to the bottom layer, and the footprint
        // is mirrored at its Y axis before the rotation: (-100, 0) turns to
        // (0, -100).
        let u2 = centers(&s, Kind::Pad, Some("U2-1"));
        assert_eq!(u2.len(), 1);
        assert_eq!(u2[0].0, Slot::Layer(2));
        assert!(close(u2[0].1, P::new(3.0, 1.9)), "{:?}", u2[0].1);

        let vias = centers(&s, Kind::Via, None);
        assert_eq!(
            vias.iter().map(|v| v.0).collect::<Vec<_>>(),
            [Slot::Layer(0), Slot::Layer(2)]
        );
        assert!(close(vias[0].1, P::new(0.5, 0.5)));

        let trace = s.items.iter().find(|i| i.kind == Kind::Trace).unwrap();
        assert_eq!(trace.slot, Slot::Layer(2));
        assert_eq!(s.nets[trace.net.unwrap()], "N1");
        let Geom::Path { points, width } = &trace.geom else {
            panic!("{:?}", trace.geom);
        };
        assert!((width - 0.02).abs() < 1e-9);
        assert!(close(points[0], P::new(0.0, 0.0)) && close(points[1], P::new(1.0, 0.0)));

        assert!(close(s.bounds.min, P::new(0.0, 0.0)), "{:?}", s.bounds);
        assert!(close(s.bounds.max, P::new(10.0, 5.0)), "{:?}", s.bounds);
    }

    #[test]
    fn pick_prefers_small_items_on_the_active_layer() {
        let s = scene(&Pcb::parse(DESIGN).unwrap());
        let all = |_: &Item| true;
        let hit = s.pick(P::new(0.5, 0.5), 0.0, all, Some(2)).unwrap();
        assert_eq!((hit.kind, hit.slot), (Kind::Via, Slot::Layer(2)));
        let hit = s.pick(P::new(0.8, 0.0), 0.0, all, Some(0)).unwrap();
        assert_eq!(hit.kind, Kind::Trace);
        assert!(s.pick(P::new(5.0, 4.0), 0.0, all, None).is_none());
    }

    #[test]
    fn labels_sit_on_their_footprints() {
        let s = scene(&Pcb::parse(DESIGN).unwrap());
        let mut labels = s.labels.clone();
        labels.sort_by(|a, b| a.text.cmp(&b.text));
        assert_eq!(labels.len(), 2);
        assert_eq!(
            (labels[0].text.as_str(), labels[0].side),
            ("U1", Side::Front)
        );
        assert_eq!(labels[1].side, Side::Back);
        // One square pad: centred on it, as large as fits.
        assert!(close(labels[0].center, P::new(1.0, 2.1)), "{:?}", labels[0]);
        assert!((labels[0].height - 0.2).abs() < 1e-9, "{:?}", labels[0]);

        let tall = Bounds {
            min: P::new(0.0, 0.0),
            max: P::new(2.0, 10.0),
        };
        let l = Label::fit("R12", &tall, Side::Front);
        assert!(l.vertical);
        assert!((l.height - 0.8).abs() < 1e-9);
        assert!(close(l.center, P::new(1.0, 5.0)));
    }

    #[test]
    fn polygon_hit_respects_holes() {
        let square = |s: f64| {
            vec![
                P::new(0.0, 0.0),
                P::new(s, 0.0),
                P::new(s, s),
                P::new(0.0, s),
            ]
        };
        let g = Geom::Polygon {
            outer: square(10.0),
            holes: vec![square(2.0)],
        };
        assert!(g.hit(P::new(5.0, 5.0), 0.0));
        assert!(!g.hit(P::new(1.0, 1.0), 0.0));
        assert!(!g.hit(P::new(11.0, 5.0), 0.0));
        assert!(g.hit(P::new(10.5, 5.0), 1.0));
    }
}
