//! K05.1: number helpers (`units::nm`, `c_fmt_6g`).

use freerouting_kicad::fmt6g::{c_fmt_6g, c_fmt_fixed6};
use freerouting_kicad::units;

mod fmt6g_vectors;

#[test]
fn nm_rounds_like_kicad_not_like_frommm() {
    // KRT notes pcbnew.FromMM truncates 66.1 to 66,099,999; KiCad's reader rounds.
    assert_eq!(units::nm("66.1"), Some(66_100_000));
    // half away from zero
    assert_eq!(units::nm("-0.0000005"), Some(-1));
    assert_eq!(units::nm("0.0000005"), Some(1));
    assert_eq!(units::nm("-0"), Some(0));
    assert_eq!(units::nm("116.9009"), Some(116_900_900));
    assert_eq!(units::nm("abc"), None);
    // clamped like KiCad's INT_LIMIT
    assert_eq!(units::nm("1e9"), Some(i64::from(i32::MAX - 100)));
}

#[test]
fn um_and_map_pt_follow_the_exporter() {
    assert_eq!(units::um(116_900_900), 116_900.9);
    let p = units::map_pt(116_900_900, 64_086_100);
    assert_eq!((p.x, p.y), (116_900.9, -64_086.1));
    // mapY( 0 ) is +0, not -0
    let z = units::map_pt(0, 0);
    assert!(z.y.is_sign_positive() && z.x.is_sign_positive());
    // IU2um multiplies by 0.001; scale divides by 1000: they can differ
    assert_eq!(units::iu2um(1_500_000), 1500.0);
}

#[test]
fn c_fmt_6g_matches_printf_on_200_values() {
    assert_eq!(fmt6g_vectors::VECTORS.len(), 200);
    for (v, want) in fmt6g_vectors::VECTORS {
        assert_eq!(c_fmt_6g(*v), *want, "%.6g of {v:?}");
    }
}

#[test]
fn c_fmt_6g_named_cases() {
    assert_eq!(c_fmt_6g(116_900.9), "116901");
    assert_eq!(c_fmt_6g(-64_086.1), "-64086.1");
    assert_eq!(c_fmt_6g(0.000_123_456), "0.000123456");
    assert_eq!(c_fmt_6g(1e21), "1e+21");
    assert_eq!(c_fmt_6g(-0.0), "-0");
    assert_eq!(c_fmt_6g(0.0), "0");
    assert_eq!(c_fmt_6g(123_456.5), "123456");
    assert_eq!(c_fmt_6g(1_234_565.0), "1.23456e+06");
    assert_eq!(c_fmt_6g(0.000_01), "1e-05");
    assert_eq!(c_fmt_6g(100_000.0), "100000");
    assert_eq!(c_fmt_6g(1_000_000.0), "1e+06");
    assert_eq!(c_fmt_6g(f64::NAN), "nan");
    assert_eq!(c_fmt_6g(f64::INFINITY), "inf");
    assert_eq!(c_fmt_fixed6(1500.0), "1500.000000");
    assert_eq!(c_fmt_fixed6(219.582), "219.582000");
}
