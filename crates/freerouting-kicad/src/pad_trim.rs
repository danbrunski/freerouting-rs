//! Q22.16: pads cut back to their part outside a footprint mask aperture
//! that pads of two or more nets share (decided 2026-10-20: routing never
//! breaks the board's manufacturing rules, option (b)).
//!
//! KiCad's DRC (`solder_mask_bridge`) flags copper of a second net inside a
//! mask aperture, and inside an aperture that pads of several nets share it
//! flags a track of a pad's own net too. The router ends a trace at the pin's
//! centre (Java `Pin.getTraceConnectionShape`), so a trace to an edge
//! connector's finger, whose centre lies inside the connector's one aperture
//! (fc-rom's J1), runs along the finger into it. [`trim_shared_pads`] works
//! on the DSN, after [`crate::footprint_masks`] has added the footprints'
//! `solder_mask_pads` keepouts:
//!
//! 1. A keepout over pads (any footprint's, on its copper layer, overlapping
//!    by more than [`EPS_UM2`]) of two or more nets is renamed
//!    `solder_mask_shared`: the router keeps every net out of it, the pads'
//!    own too.
//! 2. Each one-layer pad that overlaps such a keepout is given to the router
//!    as its largest part outside them all: the pad cut by the outer
//!    half-plane of one keepout edge (each edge of each such keepout tried in
//!    file order, the largest area that overlaps none of them kept, the first
//!    of equal ones). Its image pin gets a new padstack of that part
//!    (`<padstack>_outside_mask_<k>`, a polygon on the same layer, corners
//!    rounded to 1 µm) and its anchor at the part's centroid, unrotated. An
//!    image used by other components too gets a copy for the component
//!    (`<image>_<ref>`), unless all of them change alike. Rects, polygons
//!    (convex), circles and paths are cut (a circle as its inscribed 16-gon,
//!    a path as the hull of 16-gons at its points); a pad on several layers
//!    stays as it is, since it can be reached on another layer.
//! 3. A pad with no part outside (it lies inside the aperture) or that cannot
//!    be cut (a concave polygon) is named in a warning with its net: it
//!    cannot be reached without a bridge, so the router leaves it unrouted.
//!
//! The SES carries only wires and vias, and the routed `.kicad_pcb` only
//! traces and vias (`freerouting::io::kicad::final_copper`), so the cut pads
//! never reach KiCad; a trace that ends at the anchor ends inside the real
//! pad. The KiCad plugin does the same to KiCad's export
//! (`kicad-plugin/freerouting_rs/footprint_masks.py`, operation for
//! operation, so both give the same pads). `docs/DIVERGENCES.md` lists it.

use std::collections::BTreeMap;

use freerouting_dsn::ast::{
    ComponentPlacement, Image, ImagePin, LayerRef, Padstack, Pcb, Place, Point, Shape, Side,
};

use crate::footprint_masks::{FOOTPRINT_MASK_KEEPOUT_NAME, SHARED_MASK_KEEPOUT_NAME};

/// Overlap below this many µm² is touching, not overlapping.
pub const EPS_UM2: f64 = 1.0;
/// Corners of a circle's inscribed polygon.
const CIRCLE_CORNERS: usize = 16;

type P = (f64, f64);

/// The new image pins and padstacks per (component block, place): the pin's
/// index, the pin and its padstack.
type Changes = BTreeMap<(usize, usize), Vec<(usize, ImagePin, Padstack)>>;

/// One pad shape on the board: DSN frame, µm, counterclockwise.
struct Pad {
    comp: usize,
    place: usize,
    pin: usize,
    name: String,
    net: Option<String>,
    layer: String,
    poly: Vec<P>,
    /// One shape on one layer, and convex: it can be cut.
    cuttable: bool,
    one_layer: bool,
}

/// Applies the module's steps to `pcb`; the warnings name the pads that
/// cannot be reached.
pub fn trim_shared_pads(pcb: &mut Pcb) -> Vec<String> {
    let Some(structure) = pcb.structure.as_ref() else {
        return Vec::new();
    };
    let keepouts: Vec<(usize, String, Vec<P>)> = structure
        .keepouts
        .iter()
        .enumerate()
        .filter(|(_, k)| k.area.name.as_deref() == Some(FOOTPRINT_MASK_KEEPOUT_NAME))
        .filter_map(|(i, k)| match &k.area.shape {
            Shape::Polygon {
                layer: LayerRef::Named(layer),
                points,
                ..
            } => Some((i, layer.clone(), open_ring(points))),
            _ => None,
        })
        .collect();
    if keepouts.is_empty() {
        return Vec::new();
    }
    let layers: Vec<String> = structure.layers.iter().map(|l| l.name.clone()).collect();
    let pads = board_pads(pcb, &layers);
    // 1. the shared keepouts
    let mut shared: Vec<(String, Vec<P>)> = Vec::new();
    let mut renamed = Vec::new();
    for (i, layer, ring) in &keepouts {
        let mut nets: Vec<&str> = pads
            .iter()
            .filter(|p| p.layer == *layer && p.net.is_some())
            .filter(|p| overlap(ring, &p.poly) > EPS_UM2)
            .filter_map(|p| p.net.as_deref())
            .collect();
        nets.sort_unstable();
        nets.dedup();
        if nets.len() >= 2 {
            shared.push((layer.clone(), ring.clone()));
            renamed.push(*i);
        }
    }
    if let Some(s) = pcb.structure.as_mut() {
        for i in renamed {
            s.keepouts[i].area.name = Some(SHARED_MASK_KEEPOUT_NAME.to_owned());
        }
    }
    // 2. the pads under them
    let mut warnings = Vec::new();
    let mut changes: Changes = BTreeMap::new();
    for pad in &pads {
        let under: Vec<&Vec<P>> = shared
            .iter()
            .filter(|(l, ring)| *l == pad.layer && overlap(ring, &pad.poly) > EPS_UM2)
            .map(|(_, ring)| ring)
            .collect();
        if under.is_empty() || !pad.one_layer {
            continue;
        }
        let part = if pad.cuttable {
            outside_part(&pad.poly, &under)
        } else {
            None
        };
        match part.and_then(|part| new_pin(pcb, pad, &part)) {
            Some(change) => changes
                .entry((pad.comp, pad.place))
                .or_default()
                .push(change),
            None => warnings.push(format!(
                "pad {} (net {}) lies inside a solder-mask aperture on {} that pads of other nets \
                 share: it cannot be reached without a bridge, so it is left unrouted",
                pad.name,
                pad.net.as_deref().unwrap_or(""),
                pad.layer
            )),
        }
    }
    apply_changes(pcb, changes);
    warnings
}

/// The pad shapes of every placed component, in placement, place, image pin
/// and padstack shape order.
fn board_pads(pcb: &Pcb, layers: &[String]) -> Vec<Pad> {
    let (Some(placement), Some(library)) = (pcb.placement.as_ref(), pcb.library.as_ref()) else {
        return Vec::new();
    };
    let mut nets: BTreeMap<(String, String), String> = BTreeMap::new();
    for net in pcb.network.iter().flat_map(|n| n.nets.iter()) {
        for p in net.pins.iter().chain(net.fromtos.iter().flatten()) {
            nets.entry((p.component.clone(), p.pin.clone()))
                .or_insert_with(|| net.name.clone());
        }
    }
    let rotate_first = placement.flip_style_rotate_first == Some(true);
    let mut out = Vec::new();
    for (ci, comp) in placement.components.iter().enumerate() {
        let Some(image) = library.images.iter().find(|i| i.name == comp.image) else {
            continue;
        };
        for (pi, place) in comp.places.iter().enumerate() {
            let Some(loc) = place.location else {
                continue;
            };
            let frame = Frame::new(loc.at, loc.rotation, loc.side == Side::Back, rotate_first);
            for (k, pin) in image.pins.iter().enumerate() {
                let Some(ps) = library.padstacks.iter().find(|p| p.name == pin.padstack) else {
                    continue;
                };
                let one_layer = ps.shapes.len() == 1;
                for shape in &ps.shapes {
                    let LayerRef::Named(layer) = shape.layer() else {
                        continue;
                    };
                    let Some((local, convex)) = shape_polygon(shape) else {
                        continue;
                    };
                    let flip = frame.mirrored && ps.absolute != Some(true);
                    let layer = match layers.iter().position(|l| l == layer) {
                        Some(i) if flip => layers[layers.len() - 1 - i].clone(),
                        _ => layer.clone(),
                    };
                    let poly = ccw(local
                        .iter()
                        .map(|q| {
                            let (x, y) = turn(*q, pin.rotation.unwrap_or(0.0));
                            frame.to_board((pin.at.x + x, pin.at.y + y))
                        })
                        .collect());
                    out.push(Pad {
                        comp: ci,
                        place: pi,
                        pin: k,
                        name: format!("{}-{}", place.name, pin.name),
                        net: nets.get(&(place.name.clone(), pin.name.clone())).cloned(),
                        layer,
                        poly,
                        cuttable: one_layer && convex,
                        one_layer,
                    });
                }
            }
        }
    }
    out
}

/// The image pin and padstack that give the router `part` (DSN frame) of
/// `pad`, with the pin's index; `None` if fewer than three corners remain.
fn new_pin(pcb: &Pcb, pad: &Pad, part: &[P]) -> Option<(usize, ImagePin, Padstack)> {
    let placement = pcb.placement.as_ref()?;
    let library = pcb.library.as_ref()?;
    let comp = &placement.components[pad.comp];
    let loc = comp.places[pad.place].location?;
    let rotate_first = placement.flip_style_rotate_first == Some(true);
    let frame = Frame::new(loc.at, loc.rotation, loc.side == Side::Back, rotate_first);
    let image = library.images.iter().find(|i| i.name == comp.image)?;
    let pin = &image.pins[pad.pin];
    let ps = library.padstacks.iter().find(|p| p.name == pin.padstack)?;
    let LayerRef::Named(layer) = ps.shapes[0].layer() else {
        return None;
    };
    let c = frame.to_image(centroid(part));
    let anchor = (round(c.0), round(c.1));
    let mut corners: Vec<Point> = Vec::new();
    for q in part {
        let (x, y) = frame.to_image(*q);
        let p = Point::new(round(x - anchor.0), round(y - anchor.1));
        if corners.last() != Some(&p) {
            corners.push(p);
        }
    }
    while corners.len() > 1 && corners.first() == corners.last() {
        corners.pop();
    }
    if corners.len() < 3 {
        return None;
    }
    let first = corners[0];
    corners.push(first);
    let padstack = Padstack {
        name: String::new(),
        shapes: vec![Shape::Polygon {
            layer: LayerRef::Named(layer.clone()),
            aperture_width: 0.0,
            points: corners,
        }],
        attach: ps.attach,
        absolute: ps.absolute,
    };
    let pin = ImagePin {
        padstack: pin.padstack.clone(),
        name: pin.name.clone(),
        at: Point::new(anchor.0, anchor.1),
        rotation: None,
    };
    Some((pad.pin, pin, padstack))
}

/// Writes the new pins and padstacks: each component's image is changed in
/// place when every component that uses it changes alike, else copied.
fn apply_changes(pcb: &mut Pcb, changes: Changes) {
    let (Some(placement), Some(library)) = (pcb.placement.as_mut(), pcb.library.as_mut()) else {
        return;
    };
    // padstacks, named and shared by shape
    let mut named: Vec<(String, Padstack)> = Vec::new();
    let mut per_place: BTreeMap<(usize, usize), Vec<(usize, ImagePin)>> = BTreeMap::new();
    for (key, list) in changes {
        for (k, mut pin, mut ps) in list {
            let base = pin.padstack.clone();
            let found = named
                .iter()
                .find(|(b, n)| *b == base && n.shapes == ps.shapes && n.attach == ps.attach)
                .map(|(_, n)| n.name.clone());
            let name = found.unwrap_or_else(|| {
                let count = named.iter().filter(|(b, _)| *b == base).count();
                ps.name = format!("{base}_outside_mask_{}", count + 1);
                named.push((base.clone(), ps.clone()));
                ps.name.clone()
            });
            pin.padstack = name;
            per_place.entry(key).or_default().push((k, pin));
        }
    }
    library
        .padstacks
        .extend(named.into_iter().map(|(_, ps)| ps));
    // images: by component block
    let mut copies: Vec<(usize, ComponentPlacement)> = Vec::new();
    let mut moved: Vec<(usize, usize)> = Vec::new();
    for (ci, comp) in placement.components.iter().enumerate() {
        let pins_of = |pi: usize| per_place.get(&(ci, pi));
        let changed: Vec<usize> = (0..comp.places.len())
            .filter(|pi| pins_of(*pi).is_some())
            .collect();
        if changed.is_empty() {
            continue;
        }
        let Some(ii) = library.images.iter().position(|i| i.name == comp.image) else {
            continue;
        };
        let users: usize = placement
            .components
            .iter()
            .filter(|c| c.image == comp.image)
            .map(|c| c.places.len())
            .sum();
        let alike = users == comp.places.len()
            && changed.len() == comp.places.len()
            && changed.iter().all(|pi| pins_of(*pi) == pins_of(0));
        if alike {
            set_pins(&mut library.images[ii], pins_of(0).expect("changed"));
            continue;
        }
        for pi in changed {
            let place: &Place = &comp.places[pi];
            let mut image = library.images[ii].clone();
            image.name = unique_image_name(library, &format!("{}_{}", comp.image, place.name));
            set_pins(&mut image, pins_of(pi).expect("changed"));
            copies.push((
                ci,
                ComponentPlacement {
                    image: image.name.clone(),
                    places: vec![place.clone()],
                },
            ));
            library.images.push(image);
            moved.push((ci, pi));
        }
    }
    for (ci, pi) in moved.iter().rev() {
        placement.components[*ci].places.remove(*pi);
    }
    for (ci, block) in copies.into_iter().rev() {
        placement.components.insert(ci + 1, block);
    }
    placement.components.retain(|c| !c.places.is_empty());
}

fn set_pins(image: &mut Image, pins: &[(usize, ImagePin)]) {
    for (k, pin) in pins {
        image.pins[*k] = pin.clone();
    }
}

fn unique_image_name(library: &freerouting_dsn::ast::Library, base: &str) -> String {
    let mut name = base.to_owned();
    let mut n = 1;
    while library.images.iter().any(|i| i.name == name) {
        n += 1;
        name = format!("{base}_{n}");
    }
    name
}

/// The largest part of `pad` outside every ring of `under` (module docs).
fn outside_part(pad: &[P], under: &[&Vec<P>]) -> Option<Vec<P>> {
    let mut best: Option<(f64, Vec<P>)> = None;
    for ring in under {
        let sign = -area2(ring).signum();
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            if a == b {
                continue;
            }
            let part = clip(pad, a, b, sign);
            let size = area2(&part) / 2.0;
            if size <= EPS_UM2 || under.iter().any(|r| overlap(r, &part) > EPS_UM2) {
                continue;
            }
            if best.as_ref().is_none_or(|(s, _)| size > *s) {
                best = Some((size, part));
            }
        }
    }
    best.map(|(_, part)| part)
}

/// A pad shape (pin frame) as a polygon, and whether it is convex.
fn shape_polygon(shape: &Shape) -> Option<(Vec<P>, bool)> {
    match shape {
        Shape::Rect { x1, y1, x2, y2, .. } => {
            let (x0, x1, y0, y1) = (x1.min(*x2), x1.max(*x2), y1.min(*y2), y1.max(*y2));
            Some((vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)], true))
        }
        Shape::Polygon { points, .. } => {
            let ring = ccw(open_ring(points));
            let convex = is_convex(&ring);
            (ring.len() >= 3).then_some((ring, convex))
        }
        Shape::Circle {
            diameter, center, ..
        } => Some((circle((center.x, center.y), diameter / 2.0), true)),
        Shape::Path { width, points, .. } if !points.is_empty() => {
            let all: Vec<P> = points
                .iter()
                .flat_map(|p| circle((p.x, p.y), width / 2.0))
                .collect();
            Some((hull(all), true))
        }
        _ => None,
    }
}

/// Placement of an image: location, rotation (degrees), mirrored (back).
struct Frame {
    at: P,
    rotation: f64,
    mirrored: bool,
    rotate_first: bool,
}

impl Frame {
    fn new(at: Point, rotation: f64, mirrored: bool, rotate_first: bool) -> Self {
        Frame {
            at: (at.x, at.y),
            rotation,
            mirrored,
            rotate_first,
        }
    }

    fn to_board(&self, p: P) -> P {
        let mut v = p;
        if self.mirrored && !self.rotate_first {
            v.0 = -v.0;
        }
        v = turn(v, self.rotation);
        if self.mirrored && self.rotate_first {
            v.0 = -v.0;
        }
        (self.at.0 + v.0, self.at.1 + v.1)
    }

    fn to_image(&self, p: P) -> P {
        let mut v = (p.0 - self.at.0, p.1 - self.at.1);
        if self.mirrored && self.rotate_first {
            v.0 = -v.0;
        }
        v = turn(v, -self.rotation);
        if self.mirrored && !self.rotate_first {
            v.0 = -v.0;
        }
        v
    }
}

/// `p` turned counterclockwise by `deg` degrees (exact for multiples of 90).
fn turn(p: P, deg: f64) -> P {
    if deg.rem_euclid(90.0) == 0.0 {
        return match ((deg / 90.0).floor() as i64).rem_euclid(4) {
            0 => p,
            1 => (-p.1, p.0),
            2 => (-p.0, -p.1),
            _ => (p.1, -p.0),
        };
    }
    let (s, c) = deg.to_radians().sin_cos();
    (p.0 * c - p.1 * s, p.0 * s + p.1 * c)
}

fn round(v: f64) -> f64 {
    (v + 0.5).floor()
}

fn open_ring(points: &[Point]) -> Vec<P> {
    let mut ring: Vec<P> = points.iter().map(|p| (p.x, p.y)).collect();
    while ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    ring
}

fn circle(c: P, r: f64) -> Vec<P> {
    (0..CIRCLE_CORNERS)
        .map(|i| {
            let a = 2.0 * std::f64::consts::PI * i as f64 / CIRCLE_CORNERS as f64;
            (c.0 + r * a.cos(), c.1 + r * a.sin())
        })
        .collect()
}

fn cross(a: P, b: P, p: P) -> f64 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

/// Twice the signed area (counterclockwise positive).
fn area2(poly: &[P]) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (p, q) = (poly[i], poly[(i + 1) % n]);
            p.0 * q.1 - q.0 * p.1
        })
        .sum()
}

fn ccw(mut poly: Vec<P>) -> Vec<P> {
    if area2(&poly) < 0.0 {
        poly.reverse();
    }
    poly
}

fn is_convex(ring: &[P]) -> bool {
    let n = ring.len();
    (0..n).all(|i| cross(ring[i], ring[(i + 1) % n], ring[(i + 2) % n]) >= 0.0)
}

/// `poly` cut by the line `a`-`b`, keeping the side where `sign * cross >= 0`
/// (Sutherland-Hodgman, one edge).
fn clip(poly: &[P], a: P, b: P, sign: f64) -> Vec<P> {
    let n = poly.len();
    let mut out = Vec::with_capacity(n + 1);
    for i in 0..n {
        let (p, q) = (poly[i], poly[(i + 1) % n]);
        let (sp, sq) = (sign * cross(a, b, p), sign * cross(a, b, q));
        if sp >= 0.0 {
            out.push(p);
        }
        if (sp >= 0.0) != (sq >= 0.0) {
            let t = sp / (sp - sq);
            out.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
        }
    }
    out
}

/// The area (µm²) where `ring` (any simple polygon) and the counterclockwise
/// convex `convex` overlap.
fn overlap(ring: &[P], convex: &[P]) -> f64 {
    let n = convex.len();
    let mut part = ring.to_vec();
    for i in 0..n {
        if part.is_empty() {
            return 0.0;
        }
        part = clip(&part, convex[i], convex[(i + 1) % n], 1.0);
    }
    (area2(&part) / 2.0).abs()
}

/// The centroid of a polygon, taken about its first corner.
fn centroid(poly: &[P]) -> P {
    let o = poly[0];
    let (mut a, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for i in 0..poly.len() {
        let (p, q) = (poly[i], poly[(i + 1) % poly.len()]);
        let (px, py, qx, qy) = (p.0 - o.0, p.1 - o.1, q.0 - o.0, q.1 - o.1);
        let c = px * qy - qx * py;
        a += c;
        cx += (px + qx) * c;
        cy += (py + qy) * c;
    }
    (o.0 + cx / (3.0 * a), o.1 + cy / (3.0 * a))
}

/// The convex hull, counterclockwise (Andrew's monotone chain).
fn hull(mut pts: Vec<P>) -> Vec<P> {
    pts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut lower: Vec<P> = Vec::new();
    for p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], *p) <= 0.0 {
            lower.pop();
        }
        lower.push(*p);
    }
    let mut upper: Vec<P> = Vec::new();
    for p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], *p) <= 0.0 {
            upper.pop();
        }
        upper.push(*p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}
