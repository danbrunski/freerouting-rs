//! Helpers shared by the settings comparison tests.

#![allow(dead_code)]

use freerouting::drc::json::JsonValue;

/// Java `Float`/`Double` text and integer text compare by value and kind.
pub fn numbers_equal(a: &str, b: &str) -> bool {
    let is_float = |s: &str| s.contains(['.', 'e', 'E']);
    is_float(a) == is_float(b)
        && match (a.parse::<f64>(), b.parse::<f64>()) {
            (Ok(x), Ok(y)) => x == y,
            _ => a == b,
        }
}

/// The differences between two JSON trees, member order ignored.
pub fn diff(path: &str, java: &JsonValue, rust: &JsonValue, out: &mut Vec<String>) {
    match (java, rust) {
        (JsonValue::Object(a), JsonValue::Object(b)) => {
            let mut keys: Vec<&String> = a.iter().chain(b.iter()).map(|(k, _)| k).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let find = |m: &[(String, JsonValue)]| m.iter().position(|(k, _)| k == key);
                let (a, b) = (find(a).map(|i| &a[i].1), find(b).map(|i| &b[i].1));
                let here = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                match (a, b) {
                    (Some(x), Some(y)) => diff(&here, x, y, out),
                    (Some(x), None) => out.push(format!(
                        "{here}: Java {} / Rust missing",
                        x.to_pretty_string()
                    )),
                    (None, Some(y)) => out.push(format!(
                        "{here}: Java missing / Rust {}",
                        y.to_pretty_string()
                    )),
                    (None, None) => {}
                }
            }
        }
        (JsonValue::Array(a), JsonValue::Array(b)) => {
            if a.len() != b.len() {
                out.push(format!("{path}: Java {} items / Rust {}", a.len(), b.len()));
            } else {
                for (i, (x, y)) in a.iter().zip(b).enumerate() {
                    diff(&format!("{path}[{i}]"), x, y, out);
                }
            }
        }
        (JsonValue::Number(a), JsonValue::Number(b)) if numbers_equal(a, b) => {}
        (a, b) if a == b => {}
        (a, b) => out.push(format!(
            "{path}: Java {} / Rust {}",
            a.to_pretty_string(),
            b.to_pretty_string()
        )),
    }
}
