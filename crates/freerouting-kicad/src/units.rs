//! KiCad board units: integer nanometres.
//!
//! Ported from `PCB_IO_KICAD_SEXPR_PARSER::parseBoardUnits`
//! (`pcb_io_kicad_sexpr_parser.cpp` at `10.0.0`) and `KiROUND`
//! (`libs/kimath/include/math/util.h`).

/// `std::numeric_limits<int>::max()` less 100 nm, KiCad's `INT_LIMIT`.
pub const INT_LIMIT: f64 = (i32::MAX - 100) as f64;

/// Nanometres per millimetre, the scale of `pcbIUScale` for distances.
pub const NM_PER_MM: f64 = 1e6;

/// `KiROUND`: `std::llround` (half away from zero) clamped to `i32`.
pub fn ki_round(v: f64) -> i32 {
    if v.is_nan() {
        return 0;
    }
    let r = v.round();
    r.clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

/// A distance in millimetres (as written in the file) to board units:
/// `KiROUND(clamp(mm * 1e6, -INT_LIMIT, INT_LIMIT))`.
pub fn mm_to_nm(mm: f64) -> i32 {
    ki_round((mm * NM_PER_MM).clamp(-INT_LIMIT, INT_LIMIT))
}

/// `parseBoardUnits` on the text of a number.
pub fn parse_board_units(text: &str) -> Option<i32> {
    text.parse::<f64>().ok().map(mm_to_nm)
}

/// Board units back to the text KiCad writes for millimetres: up to six
/// decimals, trailing zeros trimmed (`10000000` is `10`, `-1500` is
/// `-0.0015`).
pub fn nm_to_mm_text(nm: i32) -> String {
    let neg = nm < 0;
    let a = (nm as i64).unsigned_abs();
    let (whole, frac) = (a / 1_000_000, a % 1_000_000);
    let mut s = String::new();
    if neg && a != 0 {
        s.push('-');
    }
    s.push_str(&whole.to_string());
    if frac != 0 {
        let f = format!("{frac:06}");
        s.push('.');
        s.push_str(f.trim_end_matches('0'));
    }
    s
}
