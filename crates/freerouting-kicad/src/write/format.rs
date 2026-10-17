//! The text of new copper blocks (K08.2).
//!
//! Coordinates are nanometres written as millimetres by integer formatting
//! ([`format_nm_mm`]): up to six decimals, trailing zeros dropped, no float
//! involved. Blocks follow KiCad 10's multi-line layout with tab indents:
//!
//! ```text
//! <TAB>(segment
//! <TAB><TAB>(start 1.5 2)
//! <TAB><TAB>(end 3 2)
//! <TAB><TAB>(width 0.25)
//! <TAB><TAB>(layer "F.Cu")
//! <TAB><TAB>(net 3)
//! <TAB><TAB>(uuid "...")
//! <TAB>)
//! ```
//!
//! and `(via [blind|buried|micro] (at) (size) (drill) (layers "A" "B") (net)
//! (uuid))` in the same layout. The net reference and the uuid token follow
//! the file ([`BlockStyle`]); new vias get no tenting token, so they take the
//! board's `(setup (tenting ...))`.

use crate::legacy::Pt;
use crate::nets::Dialect;

/// Nanometres as KiCad's millimetre text: `0`, `0.000001`, `-123.456789`,
/// `1`.
pub fn format_nm_mm(nm: i64) -> String {
    let neg = nm < 0;
    let a = nm.unsigned_abs();
    let (whole, frac) = (a / 1_000_000, a % 1_000_000);
    let mut s = String::new();
    if neg {
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

/// How the file spells what a new block needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockStyle {
    /// `(net 3)` or `(net "GND")`.
    pub dialect: Dialect,
    /// `uuid` or `tstamp`.
    pub uuid_token: String,
    /// Whether the uuid value is quoted (`(uuid "...")` in KiCad 8 and
    /// later, `(tstamp ...)` bare in KiCad 7).
    pub uuid_quoted: bool,
}

impl Default for BlockStyle {
    fn default() -> Self {
        BlockStyle {
            dialect: Dialect::Names,
            uuid_token: "uuid".to_owned(),
            uuid_quoted: true,
        }
    }
}

impl BlockStyle {
    /// The `(net ...)` item: `code` in the numeric dialect, `raw` (the
    /// escaped name as the file writes it) in the name dialect.
    pub fn net(&self, code: u32, raw: &str) -> String {
        match self.dialect {
            Dialect::Ids => format!("(net {code})"),
            Dialect::Names => format!("(net \"{raw}\")"),
        }
    }

    pub fn uuid(&self, uuid: &str) -> String {
        if self.uuid_quoted {
            format!("({} \"{uuid}\")", self.uuid_token)
        } else {
            format!("({} {uuid})", self.uuid_token)
        }
    }
}

fn pt(p: Pt) -> String {
    format!(
        "{} {}",
        format_nm_mm(i64::from(p.0)),
        format_nm_mm(i64::from(p.1))
    )
}

/// One `(segment ...)` block with its leading tab and trailing newline.
pub fn segment_block(
    style: &BlockStyle,
    start: Pt,
    end: Pt,
    width: i32,
    layer: &str,
    net: &str,
    uuid: &str,
) -> String {
    format!(
        "\t(segment\n\t\t(start {})\n\t\t(end {})\n\t\t(width {})\n\t\t(layer \"{layer}\")\n\t\t{net}\n\t\t{}\n\t)\n",
        pt(start),
        pt(end),
        format_nm_mm(i64::from(width)),
        style.uuid(uuid)
    )
}

/// One `(via ...)` block with its leading tab and trailing newline. `kind`
/// is `blind`, `buried`, `micro` or empty.
#[allow(clippy::too_many_arguments)]
pub fn via_block(
    style: &BlockStyle,
    kind: &str,
    at: Pt,
    diameter: i32,
    drill: i32,
    layers: (&str, &str),
    net: &str,
    uuid: &str,
) -> String {
    let head = if kind.is_empty() {
        "via".to_owned()
    } else {
        format!("via {kind}")
    };
    format!(
        "\t({head}\n\t\t(at {})\n\t\t(size {})\n\t\t(drill {})\n\t\t(layers \"{}\" \"{}\")\n\t\t{net}\n\t\t{}\n\t)\n",
        pt(at),
        format_nm_mm(i64::from(diameter)),
        format_nm_mm(i64::from(drill)),
        layers.0,
        layers.1,
        style.uuid(uuid)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_nm_mm_vectors() {
        let cases = [
            (0, "0"),
            (1, "0.000001"),
            (-1, "-0.000001"),
            (999, "0.000999"),
            (1_000_000, "1"),
            (-123_456_789, "-123.456789"),
            (1_500, "0.0015"),
            (i64::from(i32::MIN), "-2147.483648"),
            (i64::from(i32::MAX), "2147.483647"),
        ];
        for (nm, text) in cases {
            assert_eq!(format_nm_mm(nm), text, "{nm}");
        }
    }

    #[test]
    fn blocks_follow_the_style() {
        let ids = BlockStyle {
            dialect: Dialect::Ids,
            uuid_token: "tstamp".to_owned(),
            uuid_quoted: false,
        };
        let s = segment_block(
            &ids,
            (1_500_000, -2_000_000),
            (3_000_000, 2),
            250_000,
            "In1.Cu",
            &ids.net(3, "GND"),
            "u",
        );
        assert_eq!(
            s,
            "\t(segment\n\t\t(start 1.5 -2)\n\t\t(end 3 0.000002)\n\t\t(width 0.25)\n\t\t\
             (layer \"In1.Cu\")\n\t\t(net 3)\n\t\t(tstamp u)\n\t)\n"
        );
        let names = BlockStyle::default();
        let v = via_block(
            &names,
            "blind",
            (0, 0),
            800_000,
            400_000,
            ("F.Cu", "In2.Cu"),
            &names.net(3, "GND"),
            "u",
        );
        assert_eq!(
            v,
            "\t(via blind\n\t\t(at 0 0)\n\t\t(size 0.8)\n\t\t(drill 0.4)\n\t\t\
             (layers \"F.Cu\" \"In2.Cu\")\n\t\t(net \"GND\")\n\t\t(uuid \"u\")\n\t)\n"
        );
    }
}
