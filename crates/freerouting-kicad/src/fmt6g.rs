//! C `printf("%.6g")` for `f64`.
//!
//! KiCad prints shape and wire coordinates with `%.6g` (`specctra.h`,
//! `POINT::Format`, `PATH::Format`), and its DSN exporter decides whether two
//! images or padstacks are the same by comparing those printed strings
//! (`IMAGE::Compare`, `PADSTACK::Compare`). Reproducing the grouping needs the
//! same text, so this module prints exactly what glibc prints.
//!
//! `%.6g` uses six significant digits; the exponent `X` of the rounded value
//! picks the style: scientific (`1.23457e+06`) when `X < -4` or `X >= 6`,
//! fixed otherwise. Trailing zeros and a bare decimal point are removed.
//! Rust's `{:e}` formatting rounds the exact binary value half-to-even like
//! glibc does, so the digits agree.

/// `%.6g`.
pub fn c_fmt_6g(v: f64) -> String {
    c_fmt_g(v, 6)
}

/// `%.<precision>g` (`precision >= 1`).
pub fn c_fmt_g(v: f64, precision: usize) -> String {
    if v.is_nan() {
        return if v.is_sign_negative() { "-nan" } else { "nan" }.to_owned();
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf" } else { "inf" }.to_owned();
    }
    let p = precision.max(1);
    if v == 0.0 {
        return if v.is_sign_negative() { "-0" } else { "0" }.to_owned();
    }
    // d.ddddde<exp> with p significant digits, correctly rounded.
    let sci = format!("{:.*e}", p - 1, v);
    let (mantissa, exp) = sci.split_once('e').expect("{:e} always has an exponent");
    let x: i32 = exp.parse().expect("exponent is an integer");
    if x < -4 || x >= p as i32 {
        let mut m = mantissa.to_owned();
        if m.contains('.') {
            while m.ends_with('0') {
                m.pop();
            }
            if m.ends_with('.') {
                m.pop();
            }
        }
        let sign = if x < 0 { '-' } else { '+' };
        format!("{m}e{sign}{:02}", x.abs())
    } else {
        let decimals = (p as i32 - 1 - x).max(0) as usize;
        let mut f = format!("{:.*}", decimals, v);
        if f.contains('.') {
            while f.ends_with('0') {
                f.pop();
            }
            if f.ends_with('.') {
                f.pop();
            }
        }
        f
    }
}

/// `std::fixed << std::setprecision(6)` on a `double` (`%.6f`).
pub fn c_fmt_fixed6(v: f64) -> String {
    format!("{v:.6}")
}
