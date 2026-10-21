//! K06: the reader's copper (rule areas, planes, wires, vias) against the tree
//! of KiCad's own DSN export. Each function returns the differences as text.
#![allow(dead_code)]

use freerouting_dsn::ast::{
    Area, Image, Keepout, KeepoutKind, Pcb, Point, Shape, Wire, WiringItem,
};
use freerouting_kicad::ReadReport;
use freerouting_kicad::rule_areas::RuleAreaReport;

use super::parity::g6_tol;

fn pt_close(a: Point, b: Point, tol: f64) -> bool {
    (a.x - b.x).abs() <= tol.max(g6_tol(b.x)) && (a.y - b.y).abs() <= tol.max(g6_tol(b.y))
}

fn poly(s: &Shape) -> Option<(&str, &[Point])> {
    match s {
        Shape::Polygon { layer, points, .. } | Shape::Path { layer, points, .. } => {
            Some((layer.name(), points))
        }
        _ => None,
    }
}

fn area2(pts: &[Point]) -> f64 {
    let n = pts.len();
    (0..n)
        .map(|i| pts[i].x * pts[(i + 1) % n].y - pts[(i + 1) % n].x * pts[i].y)
        .sum::<f64>()
        .abs()
        / 2.0
}

/// Net area of an `Area`: the outline minus its windows.
pub fn net_area(a: &Area) -> f64 {
    let outer = poly(&a.shape).map(|(_, p)| area2(p)).unwrap_or(0.0);
    let holes: f64 = a
        .windows
        .iter()
        .filter_map(poly)
        .map(|(_, p)| area2(p))
        .sum();
    outer - holes
}

/// What KiCad's export makes of a rule area with these flags
/// (`FromBOARD`).
pub fn kicad_kind(f: &freerouting_kicad::zones::RuleFlags) -> KeepoutKind {
    match (f.vias, f.tracks) {
        (true, true) => KeepoutKind::Keepout,
        (true, false) => KeepoutKind::ViaKeepout,
        (false, true) => KeepoutKind::WireKeepout,
        (false, false) => KeepoutKind::Keepout,
    }
}

fn same_area(ours: &Area, theirs: &Area, tol: f64) -> bool {
    let (Some((l1, p1)), Some((l2, p2))) = (poly(&ours.shape), poly(&theirs.shape)) else {
        return false;
    };
    l1 == l2
        && p1.len() == p2.len()
        && p1.iter().zip(p2).all(|(a, b)| pt_close(*a, *b, tol))
        && ours.windows.len() == theirs.windows.len()
        && ours
            .windows
            .iter()
            .zip(&theirs.windows)
            .all(|(a, b)| match (poly(a), poly(b)) {
                (Some((la, pa)), Some((lb, pb))) => {
                    la == lb
                        && pa.len() == pb.len()
                        && pa.iter().zip(pb).all(|(x, y)| pt_close(*x, *y, tol))
                }
                _ => false,
            })
}

/// Walks the rule areas of `areas` (our report, the ones with `owner`) against
/// the export's polygon keepouts: KiCad makes one keepout per area and layer
/// (kind from the tracks and vias flags), the reader makes the kinds of the
/// mapping in `emitted` from the same polygon.
fn walk(
    areas: &[&RuleAreaReport],
    ours: &[&Keepout],
    theirs: &[&Keepout],
    what: &str,
    out: &mut Vec<String>,
) {
    let (mut i, mut j) = (0, 0);
    for a in areas {
        for layer in &a.layers {
            let Some(t) = theirs.get(j) else {
                out.push(format!(
                    "{what}: export has no keepout for {a:?} on {layer}"
                ));
                return;
            };
            j += 1;
            if t.kind != kicad_kind(&a.flags) {
                out.push(format!(
                    "{what}: export kind {:?} for flags {:?}",
                    t.kind, a.flags
                ));
            }
            let first = i;
            for kw in &a.emitted {
                let Some(o) = ours.get(i) else {
                    out.push(format!("{what}: we lack a {kw} for {a:?} on {layer}"));
                    return;
                };
                i += 1;
                if o.kind.keyword() != *kw {
                    out.push(format!("{what}: our kind {} vs {kw}", o.kind.keyword()));
                }
                if !same_area(&o.area, &t.area, 0.001) {
                    out.push(format!(
                        "{what}: polygon of {a:?} on {layer}\n  ours  {:?}\n  kicad {:?}",
                        o.area, t.area
                    ));
                }
                if poly(&o.area.shape).map(|p| p.0) != Some(layer.as_str()) {
                    out.push(format!("{what}: layer {:?} vs {layer}", o.area.shape));
                }
            }
            let _ = first;
        }
    }
    if i != ours.len() || j != theirs.len() {
        out.push(format!(
            "{what}: {} of our and {} of the export's keepouts left over",
            ours.len() - i.min(ours.len()),
            theirs.len() - j.min(theirs.len())
        ));
    }
}

/// The holes of the outline (K05), and the edge-clearance keepouts of Q18.5,
/// Q18.7 mask-aperture keepouts, K13.1 pad-clearance circles and Q22.16's
/// footprint mask keepouts, which KiCad's export does not write
/// (`freerouting_kicad::edge_keepouts`, `pad_clearance`, `footprint_masks`).
fn is_hole(k: &Keepout) -> bool {
    matches!(poly(&k.area.shape), Some(("signal", _)))
        || matches!(
            k.area.name.as_deref(),
            Some(
                freerouting_kicad::edge_keepouts::KEEPOUT_NAME
                    | freerouting_kicad::edge_keepouts::MASK_KEEPOUT_NAME
                    | freerouting_kicad::pad_clearance::KEEPOUT_NAME
                    | freerouting_kicad::footprint_masks::FOOTPRINT_MASK_KEEPOUT_NAME
            )
        )
}

/// Structure rule areas against the export (the holes of the outline are K05).
pub fn structure_rule_areas(r: &Pcb, e: &Pcb, report: &ReadReport) -> Vec<String> {
    let ours: Vec<&Keepout> = r
        .structure
        .as_ref()
        .unwrap()
        .keepouts
        .iter()
        .filter(|k| !is_hole(k))
        .collect();
    let theirs: Vec<&Keepout> = e
        .structure
        .as_ref()
        .unwrap()
        .keepouts
        .iter()
        .filter(|k| !is_hole(k))
        .collect();
    let areas: Vec<&RuleAreaReport> = report
        .rule_areas
        .iter()
        .filter(|a| a.owner.is_none())
        .collect();
    let mut out = Vec::new();
    walk(&areas, &ours, &theirs, "structure", &mut out);
    out
}

/// The polygon keepouts of footprint images against the export's, per image
/// of the export (matched by name).
pub fn image_rule_areas(r: &Pcb, e: &Pcb, report: &ReadReport) -> Vec<String> {
    let mut out = Vec::new();
    let comps = &e.placement.as_ref().unwrap().components;
    for img in &e.library.as_ref().unwrap().images {
        let Some(ours_img) = r
            .library
            .as_ref()
            .unwrap()
            .images
            .iter()
            .find(|i| i.name == img.name)
        else {
            continue;
        };
        let first_ref = comps
            .iter()
            .find(|c| c.image == img.name)
            .and_then(|c| c.places.first())
            .map(|p| p.name.clone());
        let poly_kos = |i: &Image| -> Vec<Keepout> {
            i.keepouts
                .iter()
                .filter(|k| !matches!(k.area.shape, Shape::Circle { .. }))
                .cloned()
                .collect()
        };
        let (ours, theirs) = (poly_kos(ours_img), poly_kos(img));
        let areas: Vec<&RuleAreaReport> = report
            .rule_areas
            .iter()
            .filter(|a| a.owner.is_some() && a.owner == first_ref)
            .collect();
        let (ours, theirs): (Vec<&Keepout>, Vec<&Keepout>) =
            (ours.iter().collect(), theirs.iter().collect());
        let mut local = Vec::new();
        walk(
            &areas,
            &ours,
            &theirs,
            &format!("image {}", img.name),
            &mut local,
        );
        out.extend(local);
    }
    out
}

/// Planes: count, order, nets and layers equal; each area within 0.1 %.
pub fn planes(r: &Pcb, e: &Pcb) -> Vec<String> {
    let (a, b) = (
        &r.structure.as_ref().unwrap().planes,
        &e.structure.as_ref().unwrap().planes,
    );
    let mut out = Vec::new();
    if a.len() != b.len() {
        out.push(format!("plane count {} vs {}", a.len(), b.len()));
    }
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        let (lx, ly) = (
            poly(&x.area.shape).map(|p| p.0),
            poly(&y.area.shape).map(|p| p.0),
        );
        if x.net != y.net || lx != ly {
            out.push(format!(
                "plane {i}: {} on {lx:?} vs {} on {ly:?}",
                x.net, y.net
            ));
            continue;
        }
        // KiCad prints `%.6g`, which moves a vertex of a small zone by up to half a
        // micrometre: equal vertices within that step are equal; otherwise the area
        // must agree within 0.1 %
        let (ax, ay) = (net_area(&x.area), net_area(&y.area));
        if !same_area(&x.area, &y.area, 0.0) && (ax - ay).abs() > ay * 0.001 {
            out.push(format!("plane {i} ({}): area {ax} vs {ay}", x.net));
        }
        if x.area.windows.len() != y.area.windows.len() {
            out.push(format!("plane {i}: windows"));
        }
    }
    out
}

fn wires_of(p: &Pcb) -> Vec<&Wire> {
    p.wiring
        .as_ref()
        .unwrap()
        .items
        .iter()
        .filter_map(|i| match i {
            WiringItem::Wire(w) => Some(w),
            _ => None,
        })
        .collect()
}

/// Wires: count, nets, layers, widths, types; points within half a `%.6g` step.
/// A wire with an arc has more points than KiCad's chord: then the export's
/// points must be a subsequence of ours.
pub fn wires(r: &Pcb, e: &Pcb) -> Vec<String> {
    let (a, b) = (wires_of(r), wires_of(e));
    let mut out = Vec::new();
    if a.len() != b.len() {
        out.push(format!("wire count {} vs {}", a.len(), b.len()));
        return out;
    }
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        if x.net != y.net || x.wire_type != y.wire_type {
            out.push(format!(
                "wire {i}: net/type {:?}/{:?} vs {:?}/{:?}",
                x.net, x.wire_type, y.net, y.wire_type
            ));
            continue;
        }
        let (
            Shape::Path {
                layer: l1,
                width: w1,
                points: p1,
            },
            Shape::Path {
                layer: l2,
                width: w2,
                points: p2,
            },
        ) = (&x.shapes[0], &y.shapes[0])
        else {
            out.push(format!("wire {i}: not a path"));
            continue;
        };
        if l1 != l2 || (w1 - w2).abs() > g6_tol(*w2) {
            out.push(format!("wire {i}: layer/width {l1:?} {w1} vs {l2:?} {w2}"));
        }
        if p1.len() == p2.len() {
            if !p1.iter().zip(p2).all(|(a, b)| pt_close(*a, *b, 0.0)) {
                out.push(format!(
                    "wire {i}: points differ\n  ours  {p1:?}\n  kicad {p2:?}"
                ));
            }
        } else {
            let mut k = 0;
            for q in p2 {
                while k < p1.len() && !pt_close(p1[k], *q, 0.0) {
                    k += 1;
                }
                if k == p1.len() {
                    out.push(format!(
                        "wire {i}: point {q:?} of the export is not on our path"
                    ));
                    break;
                }
            }
        }
    }
    out
}

/// Vias in the wiring: positions (within a `%.6g` step), padstack names, nets, types.
pub fn wire_vias(r: &Pcb, e: &Pcb) -> Vec<String> {
    let vias = |p: &Pcb| -> Vec<freerouting_dsn::ast::Via> {
        p.wiring
            .as_ref()
            .unwrap()
            .items
            .iter()
            .filter_map(|i| match i {
                WiringItem::Via(v) => Some(v.clone()),
                _ => None,
            })
            .collect()
    };
    let (a, b) = (vias(r), vias(e));
    let mut out = Vec::new();
    if a.len() != b.len() {
        out.push(format!("via count {} vs {}", a.len(), b.len()));
        return out;
    }
    for (i, (x, y)) in a.iter().zip(&b).enumerate() {
        if x.padstack != y.padstack
            || x.net != y.net
            || x.via_type != y.via_type
            || !pt_close(x.at, y.at, 0.0)
        {
            out.push(format!("via {i}: {x:?} vs {y:?}"));
        }
    }
    out
}

/// The via padstacks of the library: those named `Via[`, in order, with their
/// layer spans.
pub fn via_padstacks(r: &Pcb, e: &Pcb) -> Vec<String> {
    let sorted = |mut v: Vec<(String, Vec<String>)>| {
        v.sort();
        v
    };
    let vs = |p: &Pcb| -> Vec<(String, Vec<String>)> {
        p.library
            .as_ref()
            .unwrap()
            .padstacks
            .iter()
            .filter(|s| s.name.starts_with("Via["))
            .map(|s| {
                (
                    s.name.clone(),
                    s.shapes
                        .iter()
                        .map(|x| x.layer().name().to_owned())
                        .collect(),
                )
            })
            .collect()
    };
    let (a, b) = (sorted(vs(r)), sorted(vs(e)));
    if a == b {
        Vec::new()
    } else {
        vec![format!("via padstacks\n  ours  {a:?}\n  kicad {b:?}")]
    }
}

/// The via list of the structure: the Default class's via first, the rest
/// as a set (KiCad orders the classes' vias by `std::unordered_map` order), and
/// the same padstacks in the library.
pub fn via_lists(r: &Pcb, e: &Pcb) -> Vec<String> {
    let list = |p: &Pcb| {
        p.structure
            .as_ref()
            .unwrap()
            .via
            .as_ref()
            .map(|v| v.padstacks.clone())
    };
    let (a, b) = (list(r), list(e));
    let (Some(a), Some(b)) = (a, b) else {
        return if list(r) == list(e) {
            vec![]
        } else {
            vec!["via list present in one".into()]
        };
    };
    let mut out = Vec::new();
    if a.first() != b.first() {
        out.push(format!("default via {:?} vs {:?}", a.first(), b.first()));
    }
    let (mut sa, mut sb) = (a.clone(), b.clone());
    sa.sort();
    sb.sort();
    if sa != sb {
        out.push(format!("via list {a:?} vs {b:?}"));
    }
    out
}
