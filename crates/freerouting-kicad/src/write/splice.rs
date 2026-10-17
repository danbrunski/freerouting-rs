//! Cutting copper blocks out of the text and adding new ones (K08.2).
//!
//! The board is never re-serialized: a removed block goes with its leading
//! indentation and the line break before it, new blocks go in before the
//! board's final `)`, and every other byte stays as it was.

/// The byte range a removed block takes: the block itself (`start..end`,
/// from its `(` to just past its `)`), plus the spaces and tabs before it and
/// the line break before those when the block starts its line. A block that
/// shares its line with something else loses only itself.
pub fn removal_range(src: &str, start: usize, end: usize) -> (usize, usize) {
    let bytes = src.as_bytes();
    let mut i = start;
    while i > 0 && matches!(bytes[i - 1], b' ' | b'\t') {
        i -= 1;
    }
    if i > 0 && bytes[i - 1] == b'\n' {
        let mut s = i - 1;
        if s > 0 && bytes[s - 1] == b'\r' {
            s -= 1;
        }
        return (s, end);
    }
    (start, end)
}

/// `src` without the `remove` ranges (each `start..end`, any order, not
/// overlapping) and with `insert` placed at byte `at` (outside every removed
/// range).
pub fn splice(src: &str, remove: &[(usize, usize)], at: usize, insert: &str) -> String {
    let mut ranges: Vec<(usize, usize)> = remove.to_vec();
    ranges.sort_unstable();
    let mut out = String::with_capacity(src.len() + insert.len());
    let mut pos = 0;
    let mut inserted = false;
    for (s, e) in ranges {
        if !inserted && at <= s {
            out.push_str(&src[pos..at]);
            out.push_str(insert);
            pos = at;
            inserted = true;
        }
        out.push_str(&src[pos..s]);
        pos = e.max(pos);
    }
    if !inserted {
        out.push_str(&src[pos..at]);
        out.push_str(insert);
        pos = at;
    }
    out.push_str(&src[pos..]);
    out
}

/// Where new blocks go: the offset of the board's final `)` and whether a
/// line break must come first (when the `)` does not start its line). The
/// final `)` is the last byte that is not whitespace.
pub fn insertion_point(src: &str) -> Option<(usize, bool)> {
    let trimmed = src.trim_end();
    if !trimmed.ends_with(')') {
        return None;
    }
    let at = trimmed.len() - 1;
    let line_start = src[..at]
        .bytes()
        .rev()
        .take_while(|b| *b != b'\n')
        .all(|b| b == b' ' || b == b'\t');
    let mut at = at;
    if line_start {
        // insert before the indentation of the `)`
        while at > 0 && matches!(src.as_bytes()[at - 1], b' ' | b'\t') {
            at -= 1;
        }
    }
    Some((at, !line_start || at == 0))
}

/// Quote-aware parenthesis balance (KiCad's `\"` and `\\` escapes): true when
/// the depth ends at 0, never goes negative and no string is left open.
pub fn is_balanced(text: &str) -> bool {
    let mut depth: i64 = 0;
    let mut in_str = false;
    let mut esc = false;
    for b in text.bytes() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0 && !in_str
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removal_takes_the_line() {
        let src = "(a\n\t(b 1)\n\t(c 2) (d 3)\n)\n";
        let b = src.find("(b").unwrap();
        assert_eq!(removal_range(src, b, b + 5), (2, b + 5));
        let d = src.find("(d").unwrap();
        assert_eq!(removal_range(src, d, d + 5), (d, d + 5));
        let crlf = "(a\r\n  (b 1)\r\n)";
        let b = crlf.find("(b").unwrap();
        assert_eq!(removal_range(crlf, b, b + 5), (2, b + 5));
    }

    #[test]
    fn splice_removes_and_inserts() {
        let src = "(a\n\t(b 1)\n\t(c 2)\n)\n";
        let (at, nl) = insertion_point(src).unwrap();
        assert!(!nl);
        let b = src.find("(b").unwrap();
        let r = removal_range(src, b, b + 5);
        let out = splice(src, &[r], at, "\t(n 9)\n");
        assert_eq!(out, "(a\n\t(c 2)\n\t(n 9)\n)\n");
        assert!(is_balanced(&out));
        let one_line = "(a (b 1))";
        let (at, nl) = insertion_point(one_line).unwrap();
        assert!(nl);
        assert_eq!(&one_line[at..], ")");
    }

    #[test]
    fn balance_is_quote_aware() {
        assert!(is_balanced("(a \"x(\" (b))"));
        assert!(is_balanced("(a \"x\\\")\")"));
        assert!(!is_balanced("(a (b)"));
        assert!(!is_balanced(")("));
        assert!(!is_balanced("(a \"open)"));
    }
}
