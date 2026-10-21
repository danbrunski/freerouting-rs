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

/// Found in the ordered map only.
impl crate::datastructures::DenseKey for CaseInsensitive {}

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

/// A string ordered like Java `String.compareTo` (by UTF-16 code units).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct JavaString(pub String);

impl PartialOrd for JavaString {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for JavaString {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.encode_utf16().cmp(other.0.encode_utf16())
    }
}

/// Java `s.split(separator)` for a literal separator: trailing empty
/// strings are removed, and a string without the separator gives itself.
pub(crate) fn java_split(s: &str, separator: &str) -> Vec<String> {
    if !s.contains(separator) {
        return vec![s.to_string()];
    }
    let mut parts: Vec<String> = s.split(separator).map(str::to_string).collect();
    while parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    parts
}

/// Java `s.replaceAll("\\.\\d+", "")`: removes every `.` followed by digits.
pub(crate) fn remove_dot_numbers(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit) {
            i += 1;
            while chars.get(i).is_some_and(char::is_ascii_digit) {
                i += 1;
            }
            continue;
        }
        result.push(chars[i]);
        i += 1;
    }
    result
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

/// The hasher of the engine's hash maps and sets: SipHash with fixed keys.
///
/// `std`'s default `RandomState` draws new keys in every process, so the
/// bucket order and the probing of a map differ from run to run. Nothing
/// that reaches a result may depend on that (D2), but the number of
/// instructions a run executes does, a little (about 1 % of a DRC run), and
/// a profile or a callgrind baseline (P01.4) needs the same count every
/// time. These maps hash the same way in every process.
pub(crate) type StableHasher = std::hash::BuildHasherDefault<std::hash::DefaultHasher>;
/// A `HashMap` with the [`StableHasher`] hasher; build it with `default()`.
pub(crate) type HashMap<K, V> = std::collections::HashMap<K, V, StableHasher>;
/// A `HashSet` with the [`StableHasher`] hasher; build it with `default()`.
pub(crate) type HashSet<T> = std::collections::HashSet<T, StableHasher>;

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
