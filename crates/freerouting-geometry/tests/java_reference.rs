//! Differential tests: the expected strings were produced by running the Java
//! `app.freerouting.geometry.planar` classes on the same inputs.

use freerouting_geometry::{
    FloatPoint, IntBox, IntOctagon, IntPoint, LineSegment, Point, PolygonShape, Polyline,
    PolylineArea, PolylineShape, PolylineShapeOps, TileShape,
};

fn describe(t: &TileShape) -> String {
    let name = match t {
        TileShape::IntBox(_) => "IntBox",
        TileShape::IntOctagon(_) => "IntOctagon",
        TileShape::Simplex(_) => "Simplex",
    };
    let corners: String = (0..t.border_line_count())
        .map(|i| format!("{};", t.corner_approx(i)))
        .collect();
    format!("{name}[{corners}]")
}

fn pts(c: &[i32]) -> Vec<Point> {
    c.chunks(2)
        .map(|p| Point::from(IntPoint::new(p[0], p[1])))
        .collect()
}

fn stair(points: &[IntPoint]) -> String {
    let inner: Vec<String> = points.iter().map(ToString::to_string).collect();
    format!("[{}]", inner.join(", "))
}

#[test]
fn offset_shapes_cut_dog_ears_like_java() {
    let p = Polyline::from_points(&pts(&[0, 0, 100, 0, 105, 40, 0, 45, 3, 90, 200, 95]));
    let got: Vec<String> = p.offset_shapes(10).iter().map(describe).collect();
    assert_eq!(
        got,
        [
            "Simplex[(-4 , -10);(104 , -10);(109.4286 , -4.5714);(110 , 0);(110 , 4);(104 , 10);(-4 , 10);(-10 , 4);(-10 , -4);]",
            "Simplex[(96 , -10);(104 , -10);(109.4286 , -4.5714);(115 , 40);(115 , 44);(109.2 , 49.8);(105 , 50);(101 , 50);(95.5714 , 44.5714);(90 , 0);(90 , -4);]",
            "Simplex[(105 , 30);(109 , 30);(114.4286 , 35.4286);(115 , 40);(115 , 44);(109.2 , 49.8);(0 , 55);(-4 , 55);(-9.7143 , 49.2857);(-10 , 45);(-10 , 41);(-4.2 , 35.2);]",
            "Simplex[(0 , 35);(4 , 35);(9.7143 , 40.7143);(13 , 90);(13 , 94);(7 , 100);(3 , 100);(-1.1042 , 99.8958);(-6.7143 , 94.2857);(-10 , 45);(-10 , 41);(-4.2 , 35.2);]",
            "Simplex[(-1 , 80);(3 , 80);(204.1042 , 85.1042);(210 , 91);(210 , 95);(209.8958 , 99.1042);(204 , 105);(200 , 105);(-1.1042 , 99.8958);(-6.7143 , 94.2857);(-7 , 90);(-7 , 86);]",
        ]
    );
}

#[test]
fn stair_approximations_like_java() {
    let ls = LineSegment::from_polyline(&Polyline::from_points(&pts(&[3, 7, 101, 43])), 1);
    assert_eq!(
        stair(&ls.stair_approximation(7.0, true)),
        "[(3,7), (22,7), (22,14), (41,14), (41,21), (60,21), (60,28), (79,28), (79,35), (98,35), (98,42), (101,42), (101,43)]"
    );
    assert_eq!(
        stair(&ls.stair_approximation(7.0, false)),
        "[(3,7), (3,14), (22,14), (22,21), (41,21), (41,28), (60,28), (60,35), (79,35), (79,42), (98,42), (98,43), (101,43)]"
    );
    assert_eq!(
        stair(&ls.stair_approximation_45(7.0, true)),
        "[(3,7), (15,7), (22,14), (34,14), (41,21), (53,21), (60,28), (72,28), (79,35), (91,35), (98,42), (100,42), (101,43)]"
    );
    // The function-of-y case keeps the Java bug (function_value_approx evaluated at y).
    let ls2 = LineSegment::from_polyline(&Polyline::from_points(&pts(&[3, 7, 41, 103])), 1);
    assert_eq!(
        stair(&ls2.stair_approximation_45(9.0, false)),
        "[(3,7), (3,-42), (75,30), (75,-5), (133,53), (133,18), (191,76), (191,40), (250,99), (250,-106), (41,103)]"
    );
}

#[test]
fn polygon_split_to_convex_like_java() {
    let ps = PolygonShape::new(&pts(&[
        0, 0, 50, 0, 50, 10, 20, 10, 20, 30, 60, 30, 60, 50, 0, 50,
    ]));
    let got: Vec<String> = ps.split_to_convex().unwrap().iter().map(describe).collect();
    assert_eq!(
        got,
        [
            "IntBox[(20 , 30);(60 , 30);(60 , 50);(20 , 50);]",
            "IntBox[(0 , 0);(20 , 0);(20 , 50);(0 , 50);]",
            "IntBox[(20 , 0);(50 , 0);(50 , 10);(20 , 10);]",
        ]
    );
}

#[test]
fn polyline_area_split_to_convex_like_java() {
    let pa = PolylineArea::new(
        PolylineShape::from(IntBox::new(0, 0, 100, 100)),
        vec![PolylineShape::from(IntOctagon::new(
            20, 20, 60, 70, -30, 30, 50, 120,
        ))],
    );
    let got: Vec<String> = pa
        .split_to_convex(None)
        .unwrap()
        .iter()
        .map(describe)
        .collect();
    assert_eq!(
        got,
        [
            "IntBox[(0 , 30);(20 , 30);(20 , 50);(0 , 50);]",
            "IntBox[(60 , 0);(100 , 0);(100 , 60);(60 , 60);]",
            "IntBox[(30 , 0);(60 , 0);(60 , 20);(30 , 20);]",
            "IntBox[(40 , 70);(50 , 70);(50 , 100);(40 , 100);]",
            "IntOctagon[(0 , 50);(20 , 50);(40 , 70);(40 , 100);(40 , 100);(0 , 100);(0 , 100);(0 , 50);]",
            "IntOctagon[(0 , 0);(30 , 0);(30 , 0);(30 , 20);(20 , 30);(0 , 30);(0 , 30);(0 , 0);]",
            "IntOctagon[(50 , 20);(60 , 20);(60 , 20);(60 , 30);(60 , 30);(60 , 30);(50 , 20);(50 , 20);]",
            "IntOctagon[(60 , 60);(100 , 60);(100 , 60);(100 , 100);(100 , 100);(50 , 100);(50 , 100);(50 , 70);]",
        ]
    );
}

#[test]
fn simplex_cutout_and_rotation_like_java() {
    let tri = TileShape::get_instance_from_points(&pts(&[0, 0, 100, 10, 30, 90]));
    let got: Vec<String> = tri
        .cutout(&TileShape::IntBox(IntBox::new(20, 20, 40, 40)))
        .unwrap()
        .iter()
        .map(describe)
        .collect();
    assert_eq!(
        got,
        [
            "Simplex[(-0 , -0);(21.7822 , 2.1782);(20 , 20);(8 , 24);]",
            "Simplex[(21.7822 , 2.1782);(41.5842 , 4.1584);(40 , 20);(20 , 20);]",
            "Simplex[(41.5842 , 4.1584);(100 , 10);(59.115 , 56.7257);(40 , 40);(40 , 20);]",
            "Simplex[(20 , 40);(40 , 40);(59.115 , 56.7257);(30 , 90);(14 , 42);]",
            "Simplex[(20 , 20);(20 , 40);(14 , 42);(8 , 24);]",
        ]
    );
    assert_eq!(
        describe(&tri.rotate_approx(0.3, FloatPoint::new(5.0, 5.0))),
        "Simplex[(2 , -1);(94 , 38);(4 , 94);]"
    );
}
