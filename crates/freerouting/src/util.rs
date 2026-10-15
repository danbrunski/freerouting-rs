//! Java string comparisons that the board model relies on.

use std::cmp::Ordering;

fn upper(c: char) -> char {
    let mut it = c.to_uppercase();
    match (it.next(), it.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

fn lower(c: char) -> char {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

/// Java `String.equalsIgnoreCase`.
pub fn equals_ignore_case(a: &str, b: &str) -> bool {
    a.chars().count() == b.chars().count()
        && a.chars().zip(b.chars()).all(|(x, y)| {
            x == y || {
                let (ux, uy) = (upper(x), upper(y));
                ux == uy || lower(ux) == lower(uy)
            }
        })
}

/// Java `String.compareToIgnoreCase`.
pub fn compare_ignore_case(a: &str, b: &str) -> Ordering {
    let mut ia = a.chars();
    let mut ib = b.chars();
    loop {
        match (ia.next(), ib.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                if x != y {
                    let x = lower(upper(x));
                    let y = lower(upper(y));
                    if x != y {
                        return x.cmp(&y);
                    }
                }
            }
        }
    }
}

/// A string ordered by Java `compareToIgnoreCase`, for keys of maps that Java
/// orders that way.
#[derive(Clone, Debug)]
pub struct CaseInsensitive(pub String);

impl PartialEq for CaseInsensitive {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for CaseInsensitive {}

impl PartialOrd for CaseInsensitive {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CaseInsensitive {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_ignore_case(&self.0, &other.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn java_case_insensitive_comparisons() {
        assert!(equals_ignore_case("GND", "gnd"));
        assert!(!equals_ignore_case("GND", "gnd1"));
        assert_eq!(compare_ignore_case("abc", "ABD"), Ordering::Less);
        assert_eq!(compare_ignore_case("R1", "r1"), Ordering::Equal);
        assert_eq!(compare_ignore_case("R1", "R10"), Ordering::Less);
        assert_eq!(CaseInsensitive("U1".into()), CaseInsensitive("u1".into()));
    }
}
