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

/// The iteration order of a Java `HashMap<Integer, V>` whose keys were
/// inserted in the order `keys` (all distinct), with the default capacity
/// and load factor.
///
/// Java iterates the buckets in index order and each bucket in insertion
/// order; resizing splits buckets keeping that order. Buckets that Java
/// turns into trees (9 or more keys in one bucket of a table with at least
/// 64 buckets) keep insertion order here, which Java does not guarantee.
pub(crate) fn java_hash_map_order(keys: &[i32]) -> Vec<i32> {
    fn spread(key: i32) -> u32 {
        let h = key as u32;
        h ^ (h >> 16)
    }
    let mut capacity: usize = 16;
    let mut buckets: Vec<Vec<i32>> = vec![Vec::new(); capacity];
    let mut size = 0;
    for &key in keys {
        let index = spread(key) as usize & (capacity - 1);
        buckets[index].push(key);
        size += 1;
        if size > capacity * 3 / 4 {
            let new_capacity = capacity * 2;
            let mut new_buckets: Vec<Vec<i32>> = vec![Vec::new(); new_capacity];
            for bucket in &buckets {
                for &k in bucket {
                    new_buckets[spread(k) as usize & (new_capacity - 1)].push(k);
                }
            }
            buckets = new_buckets;
            capacity = new_capacity;
        }
    }
    buckets.into_iter().flatten().collect()
}

#[cfg(test)]
mod hash_map_order_tests {
    use super::java_hash_map_order;

    #[test]
    fn small_integers_iterate_in_bucket_order() {
        assert_eq!(java_hash_map_order(&[5, 3, 20, 1]), vec![1, 3, 20, 5]);
        // 13 keys resize the table to 32 buckets
        let keys: Vec<i32> = (1..=13).rev().collect();
        assert_eq!(java_hash_map_order(&keys), (1..=13).collect::<Vec<_>>());
        assert_eq!(java_hash_map_order(&[40, 8, 24]), vec![40, 8, 24]);
    }
}
