//! K14.4 port test: `fill_wires` against the plugin's `dsn_fixups.fill_rects`
//! and `_wire`, run by `tests/data/fill_wires_gen.py` on synthetic chains and
//! recorded outer-layer zone fills (`tests/data/fill_wires_zones.json`).
//! Rectangles and wire geometry must be bit-identical in mm; the path
//! integers must equal the ones `_wire` writes.

use freerouting_kicad::fill_wires::{FillRect, MIN_MM, MmPoint, SLAB_MM, fill_rects, um, wire};
use serde_json::Value;

fn data() -> Value {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/fill_wires.json"
    ))
    .expect("tests/data/fill_wires.json (run fill_wires_gen.py)");
    serde_json::from_str(&text).expect("valid JSON")
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn i(v: &Value) -> i64 {
    v.as_i64().unwrap_or_else(|| panic!("not an integer: {v}"))
}

fn arr(v: &Value) -> &Vec<Value> {
    v.as_array().unwrap_or_else(|| panic!("not an array: {v}"))
}

/// Bit equality, so `-0.0` and `0.0` differ and nothing is within a tolerance.
fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
}

fn rect(v: &Value) -> FillRect {
    let r = arr(v);
    FillRect {
        x0: f(&r[0]),
        y0: f(&r[1]),
        x1: f(&r[2]),
        y1: f(&r[3]),
    }
}

fn chains(v: &Value) -> Vec<Vec<MmPoint>> {
    arr(v)
        .iter()
        .map(|c| {
            arr(c)
                .iter()
                .map(|p| (f(&arr(p)[0]), f(&arr(p)[1])))
                .collect()
        })
        .collect()
}

/// One `_wire` record: `rect`, `pts_mm`, `width_mm`, `path_um`.
fn check_wire(case: &str, w: &Value) {
    let got = wire(rect(&w["rect"]));
    let pts: Vec<f64> = arr(&w["pts_mm"]).iter().map(f).collect();
    let mine = [got.from.0, got.from.1, got.to.0, got.to.1];
    for (k, (a, b)) in mine.iter().zip(&pts).enumerate() {
        assert!(same(*a, *b), "{case}: point coordinate {k}: {a:?} != {b:?}");
    }
    assert!(
        same(got.width, f(&w["width_mm"])),
        "{case}: width {:?} != {}",
        got.width,
        w["width_mm"]
    );
    let path: Vec<i64> = arr(&w["path_um"]).iter().map(i).collect();
    assert_eq!(got.dsn_path_um().to_vec(), path, "{case}: path integers");
    let board = got.to_um();
    assert_eq!(
        [
            board.width,
            board.from.0,
            -board.from.1,
            board.to.0,
            -board.to.1
        ]
        .to_vec(),
        path,
        "{case}: board-frame integers"
    );
}

#[test]
fn constants_are_the_plugins() {
    let d = data();
    assert!(same(SLAB_MM, f(&d["slab_mm"])));
    assert!(same(MIN_MM, f(&d["min_mm"])));
}

#[test]
fn um_rounds_like_python_round() {
    let d = data();
    let cases = arr(&d["um_cases"]);
    assert!(cases.len() >= 20);
    for c in cases {
        let mm = f(&c["mm"]);
        assert!(same(mm * 1000.0, f(&c["times_1000"])), "{mm} * 1000");
        assert_eq!(um(mm), i(&c["um"]), "um({mm})");
    }
}

/// Exact ties go to the even integer (Python's `round`), not away from zero.
#[test]
fn um_ties_go_to_even() {
    assert_eq!(um(0.0005), 0); // 0.5
    assert_eq!(um(0.0015), 2); // 1.5
    assert_eq!(um(0.0025), 2); // 2.5, where f64::round gives 3
    assert_eq!(um(-0.0025), -2);
    assert_eq!(um(1.0025), 1002); // 1002.5
    assert_eq!(um(-123.4565), -123456); // -123456.5
    // look like ties, are not: 1.0035 * 1000 is 1003.5000000000001
    assert_eq!(um(1.0035), 1004);
    assert_eq!(um(2.0045), 2005);
    assert_eq!(um(0.0004999999), 0);
    assert_eq!(um(0.00050001), 1);
}

#[test]
fn fill_rects_match_the_plugin() {
    let d = data();
    let cases = arr(&d["fill_cases"]);
    let mut real = 0;
    let mut total = 0;
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let got = fill_rects(&chains(&c["chains"]));
        let want: Vec<FillRect> = arr(&c["rects"]).iter().map(rect).collect();
        assert_eq!(got.len(), want.len(), "{name}: rectangle count");
        for (k, (a, b)) in got.iter().zip(&want).enumerate() {
            let bits = |r: &FillRect| [r.x0, r.y0, r.x1, r.y1].map(f64::to_bits);
            assert_eq!(bits(a), bits(b), "{name}: rectangle {k}: {a:?} != {b:?}");
        }
        for w in arr(&c["wires"]) {
            check_wire(name, w);
        }
        if c.get("source").is_some() {
            real += 1;
        }
        total += want.len();
    }
    assert!(real >= 1, "at least one recorded zone fill");
    assert!(total > 700, "{total} rectangles");
}

#[test]
fn wires_match_the_plugin() {
    let d = data();
    let cases = arr(&d["wire_cases"]);
    assert!(cases.len() >= 6);
    for w in cases {
        check_wire(w["name"].as_str().unwrap(), w);
    }
}

#[test]
fn no_chains_no_rectangles() {
    assert!(fill_rects(&[]).is_empty());
    assert!(fill_rects(&[Vec::new()]).is_empty());
}
