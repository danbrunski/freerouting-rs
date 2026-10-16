//! Java's `TextManager.parseTimespanString`.

/// `TextManager.parseTimespanString`: `HH:mm:ss`, `mm:ss` or `ss` as whole
/// seconds, `None` when Java returns `null` (blank text, or text that
/// `Duration.parse` rejects after the colon-to-ISO rewrite, such as `5m`).
pub fn parse_timespan_seconds(text: &str) -> Option<i64> {
    if text.trim().is_empty() {
        return None;
    }
    let iso = convert_to_duration_format(text);
    parse_iso_duration_seconds(&iso)
}

/// `TextManager.convertFromTimespanToDurationFormat`.
pub fn convert_to_duration_format(text: &str) -> String {
    // Java's `String.split(":")` drops trailing empty strings.
    let mut parts: Vec<&str> = text.split(':').collect();
    while parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    let mut out = String::from("PT");
    match parts.len() {
        3 => out.push_str(&format!("{}H{}M{}S", parts[0], parts[1], parts[2])),
        2 => out.push_str(&format!("{}M{}S", parts[0], parts[1])),
        1 => out.push_str(&format!("{}S", parts[0])),
        _ => {}
    }
    out
}

/// `Duration.parse(text).getSeconds()` for the `PT[nH][nM][n[.f]S]` subset
/// that the rewrite above can produce. `None` when Java throws.
fn parse_iso_duration_seconds(text: &str) -> Option<i64> {
    let upper = text.to_ascii_uppercase();
    let body = upper.strip_prefix("PT")?;
    let mut rest = body;
    let mut total_nanos: i128 = 0;
    let mut any = false;
    let mut stage = 0; // 0 expects H, 1 expects M, 2 expects S
    while !rest.is_empty() {
        let (number, tail) = split_number(rest)?;
        let unit = tail.chars().next()?;
        let tail = &tail[1..];
        let order = match unit {
            'H' => 0,
            'M' => 1,
            'S' => 2,
            _ => return None,
        };
        if order < stage {
            return None;
        }
        stage = order + 1;
        let unit_nanos: i128 = [3_600_000_000_000, 60_000_000_000, 1_000_000_000][order];
        total_nanos = total_nanos.checked_add(number.to_nanos(unit_nanos, order == 2)?)?;
        any = true;
        rest = tail;
    }
    if !any {
        return None;
    }
    let seconds = total_nanos.div_euclid(1_000_000_000);
    i64::try_from(seconds).ok()
}

struct Number {
    negative: bool,
    int: i128,
    fraction_nanos: i128,
}

impl Number {
    fn to_nanos(&self, unit_nanos: i128, allow_fraction: bool) -> Option<i128> {
        if self.fraction_nanos != 0 && !allow_fraction {
            return None;
        }
        let magnitude = self
            .int
            .checked_mul(unit_nanos)?
            .checked_add(self.fraction_nanos)?;
        Some(if self.negative { -magnitude } else { magnitude })
    }
}

/// `[-+]?[0-9]+([.,][0-9]{0,9})?`, returning the rest after the number.
fn split_number(s: &str) -> Option<(Number, &str)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    let mut negative = false;
    if matches!(bytes.first(), Some(b'-' | b'+')) {
        negative = bytes[0] == b'-';
        i = 1;
    }
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == digits_start {
        return None;
    }
    let int: i128 = s[digits_start..i].parse().ok()?;
    let mut fraction_nanos = 0;
    if matches!(bytes.get(i), Some(b'.' | b',')) {
        let frac_start = i + 1;
        let mut j = frac_start;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        let digits = &s[frac_start..j];
        if digits.len() > 9 {
            return None;
        }
        if !digits.is_empty() {
            let scaled = format!("{digits:0<9}");
            fraction_nanos = scaled.parse().ok()?;
        }
        i = j;
    }
    Some((
        Number {
            negative,
            int,
            fraction_nanos,
        },
        &s[i..],
    ))
}

#[cfg(test)]
mod tests {
    use super::parse_timespan_seconds as p;

    #[test]
    fn parses_like_text_manager() {
        assert_eq!(p("12:00:00"), Some(43_200));
        assert_eq!(p("01:30"), Some(90));
        assert_eq!(p("45"), Some(45));
        assert_eq!(p("0:0:5"), Some(5));
        assert_eq!(p("1.5"), Some(1));
        assert_eq!(p("12:"), Some(12));
        // Java's `"5m"` is a no-op: `PT5mS` is not an ISO duration.
        assert_eq!(p("5m"), None);
        assert_eq!(p(""), None);
        assert_eq!(p("  "), None);
        assert_eq!(p("1:2:3:4"), None);
        assert_eq!(p(":5"), None);
        assert_eq!(p("-5"), Some(-5));
    }
}
