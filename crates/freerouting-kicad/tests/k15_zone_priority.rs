//! K15.15: a plane covers only what its zone's fill can cover on that layer.
//!
//! Fixture `tests/data/k15-overlapping-planes.kicad_pcb`, cut from the
//! power-control unit (`../power-control-unit` `76163c7`, tracks and vias
//! stripped) with `python3 -I` text edits: the board's edge and its two In2.Cu
//! zones verbatim without their fills, V_BUS_24V (priority 41, a concave
//! outline) over V_BUS_12V (priority 1, a rectangle); J1, a V_BUS_12V pad on
//! F.Cu at (167.5, 44.32) mm with a track to a V_BUS_12V via at
//! (165.71, 44.32) mm inside the overlap; and J2, a V_BUS_12V through-hole pad
//! at (200, 60) mm in V_BUS_12V's own fill. KiCad 10.0.7 (`pcb drc
//! --refill-zones`) fills V_BUS_24V over the overlap, reports the via
//! dangling and J1's track unconnected from J2's pad.

use freerouting_dsn::ast::{Plane, Shape};
use freerouting_kicad::ReadOptions;

const PCB: &str = include_str!("data/k15-overlapping-planes.kicad_pcb");

fn read_planes(text: &str) -> Vec<Plane> {
    let read = freerouting_kicad::read_with("board.dsn", text, None, ReadOptions::default())
        .expect("the fixture reads");
    read.pcb.structure.unwrap().planes
}

fn ring(shape: &Shape) -> Vec<(f64, f64)> {
    match shape {
        Shape::Polygon { points, .. } => points.iter().map(|p| (p.x, p.y)).collect(),
        other => panic!("not a polygon: {other:?}"),
    }
}

/// Even-odd point in polygon; `p` in mm, board frame (y down), as the DSN
/// tree's µm with y up.
fn inside(poly: &[(f64, f64)], p: (f64, f64)) -> bool {
    let (x, y) = (p.0 * 1000.0, -p.1 * 1000.0);
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.1 > y) != (b.1 > y) && x < (b.0 - a.0) * (y - a.1) / (b.1 - a.1) + a.0 {
            c = !c;
        }
    }
    c
}

/// True if the plane covers `p`: inside its outline and in none of its windows.
fn covers(plane: &Plane, p: (f64, f64)) -> bool {
    inside(&ring(&plane.area.shape), p) && !plane.area.windows.iter().any(|w| inside(&ring(w), p))
}

fn by_net<'a>(planes: &'a [Plane], net: &str) -> &'a Plane {
    planes
        .iter()
        .find(|p| p.net == net)
        .expect("a plane of the net")
}

const VIA: (f64, f64) = (165.71, 44.32);
/// V_BUS_12V's own fill: J2's pad, the strip right of V_BUS_24V and the part below it.
const LOWER_ONLY: [(f64, f64); 3] = [(200.0, 60.0), (180.0, 50.0), (150.0, 100.0)];
/// Inside V_BUS_24V's outline and V_BUS_12V's.
const OVERLAP: [(f64, f64); 3] = [VIA, (175.0, 50.0), (130.0, 40.0)];

#[test]
fn the_lower_zones_plane_has_the_higher_zone_cut_out() {
    let planes = read_planes(PCB);
    assert_eq!(planes.len(), 2);
    let high = by_net(&planes, "V_BUS_24V");
    let low = by_net(&planes, "V_BUS_12V");
    assert!(high.area.windows.is_empty(), "the higher zone stays whole");
    assert!(!low.area.windows.is_empty());
    for p in OVERLAP {
        assert!(covers(high, p), "{p:?} is V_BUS_24V's");
        assert!(!covers(low, p), "{p:?} is cut out of V_BUS_12V");
    }
    for p in LOWER_ONLY {
        assert!(covers(low, p), "{p:?} stays V_BUS_12V's");
    }
    // every window lies in the lower zone's outline (within the 0.1 µm grid)
    let outline = ring(&low.area.shape);
    let (min_x, max_x) = outline
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
    let (min_y, max_y) = outline
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
    for w in &low.area.windows {
        for p in ring(w) {
            assert!(p.0 >= min_x - 0.1 && p.0 <= max_x + 0.1, "{p:?}");
            assert!(p.1 >= min_y - 0.1 && p.1 <= max_y + 0.1, "{p:?}");
        }
    }
}

#[test]
fn the_higher_priority_decides_which_plane_is_cut() {
    // V_BUS_24V below V_BUS_12V: now V_BUS_24V loses the overlap
    let text = PCB.replace("(priority 41)", "(priority 0)");
    let planes = read_planes(&text);
    let high = by_net(&planes, "V_BUS_12V");
    let low = by_net(&planes, "V_BUS_24V");
    assert!(high.area.windows.is_empty());
    for p in OVERLAP {
        assert!(covers(high, p), "{p:?}");
        assert!(!covers(low, p), "{p:?}");
    }
    assert!(covers(low, (100.0, 100.0)));
}

#[test]
fn equal_priorities_fall_to_the_larger_uuid() {
    // KiCad `ZONE::HigherPriority`: equal priorities compare the UUIDs; V_BUS_24V's
    // `1e19...` is below V_BUS_12V's `6468...`
    let text = PCB.replace("(priority 41)", "(priority 1)");
    let planes = read_planes(&text);
    assert!(by_net(&planes, "V_BUS_12V").area.windows.is_empty());
    assert!(!covers(by_net(&planes, "V_BUS_24V"), VIA));
    // without UUIDs neither zone is higher, and both stay whole
    let text = text
        .replace("(uuid \"1e193c7f-65ce-4fe7-82ba-eabcbdf90f64\")", "")
        .replace("(uuid \"6468becc-1936-44f4-8cd9-7a13946e7126\")", "");
    let planes = read_planes(&text);
    assert!(planes.iter().all(|p| p.area.windows.is_empty()));
}

#[test]
fn a_zone_of_the_same_net_cuts_nothing() {
    let text = PCB.replace("(net \"V_BUS_24V\")", "(net \"V_BUS_12V\")");
    let planes = read_planes(&text);
    assert_eq!(planes.len(), 2);
    assert!(planes.iter().all(|p| p.area.windows.is_empty()));
}

#[test]
fn a_teardrop_is_above_every_zone() {
    // KiCad: a teardrop zone is higher than a zone that is none, whatever the priorities
    let text = PCB.replacen(
        "(name \"V_BUS_12V\")",
        "(name \"V_BUS_12V\") (attr (teardrop (type padvia)))",
        1,
    );
    let planes = read_planes(&text);
    assert!(by_net(&planes, "V_BUS_12V").area.windows.is_empty());
    assert!(!covers(by_net(&planes, "V_BUS_24V"), VIA));
}

#[test]
fn zones_on_different_layers_cut_nothing() {
    let text = PCB.replacen("(layer \"In2.Cu\")", "(layer \"In1.Cu\")", 1);
    let planes = read_planes(&text);
    assert!(planes.iter().all(|p| p.area.windows.is_empty()));
}
