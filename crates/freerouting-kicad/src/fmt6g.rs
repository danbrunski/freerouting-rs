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

use std::fmt::Write as _;

/// `%.6g`.
pub fn c_fmt_6g(v: f64) -> String {
    c_fmt_g(v, 6)
}

/// `%.<precision>g` (`precision >= 1`).
pub fn c_fmt_g(v: f64, precision: usize) -> String {
    let mut out = String::with_capacity(16);
    push_c_fmt_g(&mut out, v, precision);
    out
}

/// [`c_fmt_6g`] appended to `out`.
pub fn push_c_fmt_6g(out: &mut String, v: f64) {
    push_c_fmt_g(out, v, 6);
}

/// [`c_fmt_g`] appended to `out`, without allocating.
///
/// One exact `{:e}` conversion gives the digits of both styles: C defines the
/// fixed style as `%f` with `precision - 1 - X` decimals, which rounds at the
/// same digit as the `%e` conversion that chose `X`. When that conversion
/// carries into a new digit (`99999.95` to `1.00000e+05`) the fixed style
/// rounds one digit higher and also reaches `10^X`. The former second
/// conversion (`{:.*}`) and the temporary strings took about half of this
/// function's time, and the reader prints every vertex it compares (K13.4);
/// `matches_the_two_conversion_form` pins the equality.
pub fn push_c_fmt_g(out: &mut String, v: f64, precision: usize) {
    if v.is_nan() {
        out.push_str(if v.is_sign_negative() { "-nan" } else { "nan" });
        return;
    }
    if v.is_infinite() {
        out.push_str(if v < 0.0 { "-inf" } else { "inf" });
        return;
    }
    let p = precision.max(1);
    if v == 0.0 {
        out.push_str(if v.is_sign_negative() { "-0" } else { "0" });
        return;
    }
    // d.ddddde<exp> with p significant digits, correctly rounded, written at
    // the end of `out` and moved to a buffer.
    let start = out.len();
    write!(out, "{:.*e}", p - 1, v).expect("a String takes any text");
    let mut stack = [0u8; 40];
    let heap: Vec<u8>;
    let sci: &[u8] = if out.len() - start <= stack.len() {
        let len = out.len() - start;
        stack[..len].copy_from_slice(&out.as_bytes()[start..]);
        &stack[..len]
    } else {
        heap = out.as_bytes()[start..].to_vec();
        &heap
    };
    out.truncate(start);
    let e = sci
        .iter()
        .position(|&b| b == b'e')
        .expect("{:e} always has an exponent");
    let (mantissa, exp) = (&sci[..e], &sci[e + 1..]);
    let x = parse_exponent(exp);
    if x < -4 || x >= p as i32 {
        push_ascii(out, mantissa);
        strip_fraction_zeros(out, start);
        let sign = if x < 0 { '-' } else { '+' };
        write!(out, "e{sign}{:02}", x.abs()).expect("a String takes any text");
    } else {
        let (negative, unsigned) = match mantissa.split_first() {
            Some((b'-', rest)) => (true, rest),
            _ => (false, mantissa),
        };
        if negative {
            out.push('-');
        }
        // the p significant digits, without the point
        let mut digits = unsigned
            .iter()
            .filter(|&&b| b != b'.')
            .map(|&b| char::from(b));
        if x >= 0 {
            for _ in 0..=x {
                out.extend(digits.next());
            }
            out.push('.');
        } else {
            out.push_str("0.");
            for _ in 0..(-x - 1) {
                out.push('0');
            }
        }
        out.extend(digits);
        strip_fraction_zeros(out, start);
    }
}

fn push_ascii(out: &mut String, bytes: &[u8]) {
    out.extend(bytes.iter().map(|&b| char::from(b)));
}

/// The exponent of a `{:e}` string: an optional `-` and decimal digits.
fn parse_exponent(exp: &[u8]) -> i32 {
    let (negative, digits) = match exp.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, exp),
    };
    let magnitude = digits
        .iter()
        .fold(0i32, |acc, &d| acc * 10 + i32::from(d - b'0'));
    if negative { -magnitude } else { magnitude }
}

/// Removes the trailing zeros of a fraction and a bare point from
/// `out[start..]`.
fn strip_fraction_zeros(out: &mut String, start: usize) {
    if out[start..].contains('.') {
        let kept = out.trim_end_matches('0').trim_end_matches('.').len();
        out.truncate(kept);
    }
}

/// `std::fixed << std::setprecision(6)` on a `double` (`%.6f`).
pub fn c_fmt_fixed6(v: f64) -> String {
    format!("{v:.6}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The former implementation: `{:e}` for the exponent, then `{:.*}` for
    /// the fixed style.
    fn two_conversions(v: f64, p: usize) -> String {
        if v.is_nan() || v.is_infinite() || v == 0.0 {
            return c_fmt_g(v, p);
        }
        let sci = format!("{:.*e}", p - 1, v);
        let (mantissa, exp) = sci.split_once('e').unwrap();
        let x: i32 = exp.parse().unwrap();
        let strip = |s: &str| {
            if s.contains('.') {
                s.trim_end_matches('0').trim_end_matches('.').to_owned()
            } else {
                s.to_owned()
            }
        };
        if x < -4 || x >= p as i32 {
            let m = strip(mantissa);
            let sign = if x < 0 { '-' } else { '+' };
            format!("{m}e{sign}{:02}", x.abs())
        } else {
            let decimals = (p as i32 - 1 - x).max(0) as usize;
            strip(&format!("{:.*}", decimals, v))
        }
    }

    /// The one-conversion form prints what the two-conversion form printed:
    /// random bit patterns, random board coordinates in mm and µm, and the
    /// rounding boundaries `k·10^e ± 0.5·10^(e-p)` and their neighbours, for
    /// precisions 1 to 8.
    #[test]
    fn matches_the_two_conversion_form() {
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let mut checked = 0;
        let mut check = |v: f64, p: usize| {
            let expected = two_conversions(v, p);
            assert_eq!(c_fmt_g(v, p), expected, "{v:e} at {p}");
            // appended after other text, as `shape_key` uses it
            let mut appended = String::from("x1.50");
            push_c_fmt_g(&mut appended, v, p);
            assert_eq!(appended, format!("x1.50{expected}"), "{v:e} at {p}");
            checked += 1;
        };
        for _ in 0..200_000 {
            let bits = next();
            check(f64::from_bits(bits), 6);
            let um = (next() % 2_000_000_000) as f64 / 10.0 - 1.0e8;
            check(um, 6);
            check(um / 1000.0, 6);
            check(um, 1 + (bits % 8) as usize);
        }
        for p in 1..=8usize {
            for e in -8..=10 {
                let unit = 10f64.powi(e);
                for k in [1.0, 9.0, 99999.0, 999999.0, 123456.0, 5.0] {
                    for half in [-0.5, 0.0, 0.5] {
                        let v = k * unit + half * 10f64.powi(e - p as i32);
                        for w in [
                            v,
                            f64::from_bits(v.to_bits() + 1),
                            f64::from_bits(v.to_bits() - 1),
                            -v,
                        ] {
                            check(w, p);
                        }
                    }
                }
            }
        }
        for p in [17, 30, 60] {
            for v in [
                1.0 / 3.0,
                -2.0e-300,
                123456.789,
                f64::MAX,
                f64::MIN_POSITIVE,
            ] {
                check(v, p);
            }
        }
        for v in [
            f64::NAN,
            -f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            -0.0,
        ] {
            let mut appended = String::from("x1.50");
            push_c_fmt_g(&mut appended, v, 6);
            assert_eq!(appended, format!("x1.50{}", c_fmt_g(v, 6)));
        }
        assert!(checked > 800_000);
    }
}
