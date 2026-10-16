//! K04.5: the arc conversion against KiCad 10.0.7 itself.
//!
//! The expected points were read from `pcbnew` (WSL, KiCad 10.0.7) after
//! `pcbnew.LoadBoard` of a `(version 20210925)` file holding the six
//! `gr_arc` items below: `GetStart()`, `GetArcMid()`, `GetEnd()`, nanometres.
//! (`tools/kicad-reader/old_arcs.py` reproduces the run.)

use freerouting_kicad::legacy::legacy_arc_points;
use freerouting_kicad::units::mm_to_nm;

fn mm(x: f64, y: f64) -> (i32, i32) {
    (mm_to_nm(x), mm_to_nm(y))
}

#[test]
fn legacy_arcs_match_kicad_10_load() {
    // (centre, old end, angle) -> (start, mid, end) as KiCad 10.0.7 reports them
    let cases = [
        (
            (0.0, 0.0),
            (10.0, 0.0),
            90.0,
            [(10000000, 0), (7071068, 7071068), (0, 10000000)],
        ),
        (
            (0.0, 0.0),
            (10.0, 0.0),
            -90.0,
            [(0, -10000000), (7071068, -7071068), (10000000, 0)],
        ),
        (
            (5.0, 5.0),
            (5.0, 10.0),
            180.0,
            [(5000000, 10000000), (0, 5000000), (5000000, 0)],
        ),
        (
            (0.0, 0.0),
            (10.0, 0.0),
            30.0,
            [(10000000, 0), (9659258, 2588190), (8660254, 5000000)],
        ),
        (
            (1.234567, 2.345678),
            (9.1, 3.3),
            123.4,
            [(9100000, 3300000), (4123217, 9723446), (-3891916, 8386782)],
        ),
        (
            (1.234567, 2.345678),
            (9.1, 3.3),
            -200.5,
            [(-6466976, 4206324), (774056, -5564044), (9100000, 3300000)],
        ),
    ];
    for (c, s, a, want) in cases {
        let (start, mid, end) = legacy_arc_points(mm(c.0, c.1), mm(s.0, s.1), a);
        assert_eq!([start, mid, end], want, "centre {c:?} end {s:?} angle {a}");
    }
}

#[test]
fn legacy_overbars_match_kicad_10_load() {
    // net names of a (version 20201115) board, as pcbnew reports them after loading
    for (old, kicad) in [
        ("~RESET~", "~{RESET}"),
        ("A~~B", "A~B"),
        ("~OE GND", "~{OE} GND"),
    ] {
        assert_eq!(
            freerouting_kicad::legacy::convert_overbar(old),
            kicad,
            "{old}"
        );
    }
}
