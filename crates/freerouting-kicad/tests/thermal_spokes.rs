//! Q22.20: room for a zone's thermal spokes, as keepouts named
//! `thermal_spoke` (`freerouting_kicad::thermal_spokes`).
//!
//! Fixture `crates/freerouting/tests/data/quality/q22-thermal.kicad_pcb`
//! (mm, KiCad's frame): a GND zone on F.Cu and B.Cu over (10..30, 5..25)
//! with thermal relief (gap 0.5, spoke 0.5, clearance 0.3, minimum width
//! 0.2); J1 at (20, 15), a 1.7 mm through-hole GND pad (spokes at 45°); U2
//! at (14, 9), a 2 x 1 mm F.Cu GND pad turned 20° with a 30° spoke angle; U3
//! at (26, 21), a 1.7 mm through-hole GND pad with a 30° spoke angle; U4 at
//! (14, 21), a through-hole GND pad with `(zone_connect 2)` (solid, no
//! spokes); U5 at (24, 9), a 1 mm through-hole GND pad 1.27 mm from one of
//! net A (a fine-pitch header: its diagonal spoke ways overlap the A pad); P2
//! at (35, 15) a GND pad outside the zone, and net A's PA1 (16.5, 15) and PA2
//! (23.5, 15) either side of J1. `q22-thermal.dsn` is what the
//! KiCad plugin writes for it (`kicad-plugin/tests/test_thermal_spokes.py`).

use freerouting_dsn::ast::{Keepout, LayerRef, Pcb, Shape};

const BOARD: &str = include_str!("../../freerouting/tests/data/quality/q22-thermal.kicad_pcb");
const PLUGIN_DSN: &str = include_str!("../../freerouting/tests/data/quality/q22-thermal.dsn");

/// A keepout's layer, points (DSN frame: µm, Y negated) and clearance class.
type Corridor = (String, Vec<(f64, f64)>, Option<String>);

fn corridors(keepouts: &[Keepout]) -> Vec<Corridor> {
    keepouts
        .iter()
        .filter(|k| k.area.name.as_deref() == Some("thermal_spoke"))
        .map(|k| {
            let Shape::Polygon {
                layer: LayerRef::Named(layer),
                points,
                ..
            } = &k.area.shape
            else {
                panic!("{k:?}");
            };
            (
                layer.clone(),
                points.iter().map(|p| (p.x, p.y)).collect(),
                k.area.clearance_class.clone(),
            )
        })
        .collect()
}

fn read(text: &str, project: Option<&str>) -> Vec<Corridor> {
    let r = freerouting_kicad::read(text, project).unwrap();
    corridors(&r.pcb.structure.unwrap().keepouts)
}

/// The far end's centre of a corridor (its 2nd and 3rd corners).
fn far(c: &Corridor) -> (f64, f64) {
    ((c.1[1].0 + c.1[2].0) / 2.0, (c.1[1].1 + c.1[2].1) / 2.0)
}

/// J1's, U2's, U3's and U5's spokes, F.Cu and B.Cu for the through-hole pads,
/// F.Cu only for U2; none for U4 (solid) or P2 (outside the zone). Each is
/// the spoke's width plus the zone's clearance either side (0.5 + 2 x 0.3 =
/// 1.1 mm) wide and runs from the pad's centre to the spoke's end (pad, gap
/// and half the minimum width) and 0.8 mm on (spoke and clearance), in KiCad's
/// directions: J1's at 45°, 135°, 225°, 315°; U3's at 30° + k 90° (a circle's
/// spokes turn counter-clockwise on screen); U2's at -30° + k 90° in the pad's
/// frame, then turned 20°: -10°, 80°, 170°, 260° (DSN frame, Y up).
#[test]
fn every_thermal_pad_gets_a_keepout_along_each_spoke() {
    let got = read(BOARD, None);
    assert_eq!(got.len(), 28, "{got:?}");
    let on = |layer: &str| got.iter().filter(|c| c.0 == layer).count();
    assert_eq!((on("F.Cu"), on("B.Cu")), (16, 12));
    assert!(
        got.iter()
            .all(|c| c.1.len() == 5 && c.2.as_deref() == Some("null"))
    );
    let angle_from = |c: &Corridor, centre: (f64, f64)| {
        let f = far(c);
        let a = (f.1 - centre.1).atan2(f.0 - centre.0).to_degrees();
        (a + 360.0) % 360.0
    };
    let check = |range: std::ops::Range<usize>, centre: (f64, f64), angles: [f64; 4], len: f64| {
        for (c, want) in got[range].iter().zip(angles) {
            let a = angle_from(c, centre);
            assert!((a - want).abs() < 1e-6, "{a} vs {want}: {c:?}");
            let f = far(c);
            let l = (f.0 - centre.0).hypot(f.1 - centre.1);
            assert!((l - len).abs() < 1e-6, "{l} vs {len}");
            let w = (c.1[1].0 - c.1[2].0).hypot(c.1[1].1 - c.1[2].1);
            assert!((w - 1100.0).abs() < 1e-6, "{w}");
        }
    };
    // J1: 0.85 + 0.5 + 0.1 + 0.5 + 0.3
    check(
        0..4,
        (20000.0, -15000.0),
        [45.0, 315.0, 225.0, 135.0],
        2250.0,
    );
    // U2 (F.Cu): the half-box is 1.6 x 1.1 mm (pad + gap + half the minimum
    // width); the ray at 30° meets its short side at 1.6 / cos 30° =
    // 1.8475 mm, the one at 120° its long side at 1.1 / sin 60° = 1.2702 mm
    let along = 1600.0 / 30f64.to_radians().cos() + 800.0;
    let across = 1100.0 / 60f64.to_radians().sin() + 800.0;
    for (c, (want, len)) in got[4..8].iter().zip([
        (-10.0 + 360.0, along),
        (260.0, across),
        (170.0, along),
        (80.0, across),
    ]) {
        let a = angle_from(c, (14000.0, -9000.0));
        assert!((a - want).abs() < 1e-6, "{a} vs {want}");
        let f = far(c);
        let l = (f.0 - 14000.0).hypot(f.1 + 9000.0);
        assert!((l - len).abs() < 1e-6, "{l} vs {len}");
    }
    check(
        8..12,
        (26000.0, -21000.0),
        [30.0, 300.0, 210.0, 120.0],
        2250.0,
    );
    // U5: 0.5 + 0.5 + 0.1 + 0.5 + 0.3
    check(
        12..16,
        (24000.0, -9000.0),
        [45.0, 315.0, 225.0, 135.0],
        1900.0,
    );
    check(
        16..20,
        (20000.0, -15000.0),
        [45.0, 315.0, 225.0, 135.0],
        2250.0,
    );
    check(
        20..24,
        (26000.0, -21000.0),
        [30.0, 300.0, 210.0, 120.0],
        2250.0,
    );
    check(
        24..28,
        (24000.0, -9000.0),
        [45.0, 315.0, 225.0, 135.0],
        1900.0,
    );
}

/// The plugin writes the same keepouts, to its 1 µm rounding.
#[test]
fn the_plugin_writes_the_same_keepouts() {
    let ours = read(BOARD, None);
    let pcb = Pcb::parse(PLUGIN_DSN).unwrap();
    let theirs = corridors(&pcb.structure.unwrap().keepouts);
    assert_eq!(ours.len(), theirs.len());
    for (a, b) in ours.iter().zip(&theirs) {
        assert_eq!((&a.0, &a.2), (&b.0, &b.2));
        for (p, q) in a.1.iter().zip(&b.1) {
            assert!(
                (p.0 - q.0).abs() <= 0.5 + 1e-9 && (p.1 - q.1).abs() <= 0.5 + 1e-9,
                "{p:?} vs {q:?}"
            );
        }
    }
}

/// `min_resolved_spokes` 0 in the project: KiCad does not check, so none;
/// 3 is the same as the default 2 (every spoke is kept).
#[test]
fn min_resolved_spokes_zero_writes_none() {
    let project = |n: i32| {
        format!(
            r#"{{"board": {{"design_settings": {{"rules": {{"min_resolved_spokes": {n}}}}}}}}}"#
        )
    };
    assert!(read(BOARD, Some(&project(0))).is_empty());
    assert_eq!(read(BOARD, Some(&project(3))).len(), 28);
}

/// A zone that connects pads solidly (`connect_pads yes`), a higher-priority
/// zone of another net over the spokes' ends, or a rule area that keeps fill
/// out there: no keepout.
#[test]
fn no_keepout_where_the_zone_does_not_reach_the_spoke() {
    let full = BOARD.replace(
        "(connect_pads (clearance 0.3))",
        "(connect_pads yes (clearance 0.3))",
    );
    assert!(read(&full, None).is_empty());
    let other = "  (zone (net 2) (net_name \"A\") (layer \"F.Cu\") (uuid \"6a1f0e4c-2b7d-4c55-9a0e-3f1c2d4b5a61\") (hatch edge 0.5) (priority 1)\n    \
                 (connect_pads (clearance 0.3)) (min_thickness 0.2)\n    \
                 (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5))\n    \
                 (polygon (pts (xy 17 12) (xy 23 12) (xy 23 18) (xy 17 18))))\n)\n";
    let covered = format!("{}{other}", BOARD.trim_end().trim_end_matches(')'));
    let got = read(&covered, None);
    // J1's four F.Cu spokes end in A's zone; its B.Cu ones and the others stay
    assert_eq!(got.len(), 24, "{got:?}");
    assert!(!got.iter().any(|c| c.0 == "F.Cu"
        && (far(c).0 - 20000.0).abs() < 2000.0
        && (far(c).1 + 15000.0).abs() < 2000.0));
    let keepout = other
        .replace("(net 2) (net_name \"A\") ", "")
        .replace(
            "(priority 1)",
            "(keepout (tracks allowed) (vias allowed) (pads allowed) (copperpour not_allowed) (footprints allowed))",
        );
    let ruled = format!("{}{keepout}", BOARD.trim_end().trim_end_matches(')'));
    assert_eq!(read(&ruled, None).len(), 24);
}
